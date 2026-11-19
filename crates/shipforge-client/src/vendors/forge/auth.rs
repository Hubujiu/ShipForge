use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use sha2::{Digest, Sha256};

pub(super) fn purpose(q: &Value) -> Result<&str> {
    match q["purpose"].as_str() {
        Some(s @ ("organization" | "personal")) => Ok(s),
        _ => Err("invalid: choose organization or personal".into()),
    }
}
fn check(cfg: &Value, purpose: &str) -> Result<()> {
    if purpose == "organization" && cfg["can_admin"] != true {
        return Err("forbidden: organization administrator required".into());
    }
    if purpose == "personal" && cfg["account"].is_null() {
        return Err("invalid: connect organization before personal identity".into());
    }
    Ok(())
}
fn var(p: &str, key: &str) -> String {
    format!(
        "SHIPFORGE_{}_{key}",
        if p == "github" { "GITHUB" } else { "GITLAB" }
    )
}
fn path(client: &Client, cfg: &Value, p: &str, id: &str) -> Result<Option<std::path::PathBuf>> {
    org_location(client, cfg, &format!("forge-{p}"), id)
}
fn save(client: &mut Client, q: &Value, cfg: &Value, meta: &Value, data: &Value) -> Result<Value> {
    let p = field(q, "id")?;
    let generation = Uuid::new_v4().to_string();
    let new = path(client, cfg, p, &generation)?;
    vault::save(&new, Some(data))?;
    let mut cmd = meta.clone();
    cmd["credential"] = json!(generation);
    cmd["purpose"] = q["purpose"].clone();
    cmd["expected_org"] = cfg["account"]["credential"].clone();
    cmd["expected_owner"] = cfg[if q["purpose"] == "personal" {
        "personal"
    } else {
        "account"
    }]["credential"]
        .clone();
    let result = invoke(client, q, "account", &cmd);
    if result.is_err() {
        vault::save(&new, None)?;
    } else {
        let before = &cfg[if q["purpose"] == "organization" {
            "account"
        } else {
            "personal"
        }];
        if let Some(old) = before["credential"].as_str() {
            vault::save(&path(client, cfg, p, old)?, None)?;
        }
    }
    result.and_then(|mut out| {
        if let Some(old) = out
            .as_object_mut()
            .and_then(|m| m.remove("replaced"))
            .filter(Value::is_object)
        {
            vault::save(
                &path(
                    client,
                    cfg,
                    field(&old, "platform")?,
                    field(&old, "credential")?,
                )?,
                None,
            )?;
            out["replaced_platform"] = old["platform"].clone();
        }
        Ok(out)
    })
}
pub(super) fn disconnect(client: &mut Client, q: &Value, cfg: &Value) -> Result<Value> {
    let pur = purpose(q)?;
    check(cfg, pur)?;
    let result = invoke(
        client,
        q,
        "disconnect",
        &json!({"purpose":pur,"expected_org":cfg["account"]["credential"],"expected_owner":cfg[if pur=="personal"{"personal"}else{"account"}]["credential"]}),
    )?;
    for old in result["generations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        vault::save(&path(client, cfg, field(q, "id")?, old)?, None)?;
    }
    Ok(json!({"connected":false}))
}
pub(super) fn api(client: &Client, cfg: &Value, p: &str, personal: bool) -> Result<Api> {
    let meta = &cfg[if personal { "personal" } else { "account" }];
    let site = field(meta, "instance")?;
    if p == "github" && !personal {
        return Ok(Api {
            platform: p.into(),
            site: site.into(),
            token: app_token(field(meta, "installation")?, site)?,
        });
    }
    let at = path(client, cfg, p, field(meta, "credential")?)?;
    let mut data = vault::read(&at)?;
    let mut token = field(&data, "access_token")?.to_owned();
    if data["expires_at"]
        .as_i64()
        .is_some_and(|exp| exp <= Utc::now().timestamp() + 30)
        && let Some(refresh) = data["refresh_token"].as_str()
    {
        let body = json!({"grant_type":"refresh_token","refresh_token":refresh,"client_id":env(&var(p,"CLIENT_ID"))?,"client_secret":env(&var(p,"CLIENT_SECRET"))?});
        let fresh = exchange(p, site, &body)?;
        token = field(&fresh, "access_token")?.into();
        data["access_token"] = json!(token);
        data["expires_at"] = json!(
            fresh["expires_in"]
                .as_i64()
                .map(|t| Utc::now().timestamp() + t)
        );
        if fresh["refresh_token"].is_string() {
            data["refresh_token"] = fresh["refresh_token"].clone();
        }
        vault::save(&at, Some(&data))?;
    }
    Ok(Api {
        platform: p.into(),
        site: site.into(),
        token,
    })
}
fn exchange(p: &str, site: &str, body: &Value) -> Result<Value> {
    let endpoint = format!(
        "{}{}",
        transport(p, site, false)?,
        if p == "github" {
            "/login/oauth/access_token"
        } else {
            "/oauth/token"
        }
    );
    let v = call("POST", &endpoint, "", body)?;
    if v["error"].is_string() {
        return Err("forbidden: source authorization was rejected".into());
    }
    Ok(v)
}
fn identify(
    client: &mut Client,
    q: &Value,
    cfg: &Value,
    site: &str,
    mut data: Value,
) -> Result<Value> {
    let p = field(q, "id")?;
    let pur = purpose(q)?;
    let gh = p == "github";
    let api = Api {
        platform: p.into(),
        site: site.into(),
        token: field(&data, "access_token")?.into(),
    };
    let me = api.request("GET", "/user", &Value::Null)?;
    if p == "gitlab"
        && !data["scope"]
            .as_str()
            .is_some_and(|s| s.split_whitespace().any(|scope| scope == "api"))
    {
        return Err("forbidden: GitLab OAuth api scope required".into());
    }
    let mut account = id(&me["id"])?;
    let mut login = field(&me, if gh { "login" } else { "username" })?.to_owned();
    let installation = if pur == "personal" {
        field(&cfg["account"], "installation")?.into()
    } else if gh {
        let install = id(&q["installation"])?;
        let list = api.pages("/user/installations", Some("installations"))?;
        let selected=list.iter().find(|v|id(&v["id"]).ok().as_ref()==Some(&install)&&id(&v["app_id"]).ok()==env("SHIPFORGE_GITHUB_APP_ID").ok()).ok_or("forbidden: requested GitHub installation is not accessible to the authorized user and App")?;
        let owner = &selected["account"];
        let owner_id = id(&owner["id"])?;
        let owner_login = field(owner, "login")?;
        let signer = invoke(client, q, "installer", &json!({"installation":install}))?;
        if owner_id != account && signer != account {
            if owner["type"] != "Organization"
                || !owner_login
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err("forbidden: GitHub installation owner or installer required".into());
            }
            let member = api.request(
                "GET",
                &format!("/user/memberships/orgs/{owner_login}"),
                &Value::Null,
            )?;
            if member["state"] != "active" || member["role"] != "admin" {
                return Err("forbidden: GitHub organization owner or installer required".into());
            }
        }
        app_token(&install, site)?;
        account = owner_id;
        login = owner_login.into();
        install
    } else {
        account.clone()
    };
    let expires = if p == "gitlab-self" {
        let details = api.request("GET", "/personal_access_tokens/self", &Value::Null)?;
        if details["revoked"] == true
            || details["active"] != true
            || id(&details["user_id"])? != account
            || !details["scopes"]
                .as_array()
                .is_some_and(|s| s.iter().any(|s| s == "api"))
        {
            return Err("forbidden: active GitLab personal token with api scope required".into());
        }
        details["expires_at"].clone()
    } else {
        let exp = data["expires_in"]
            .as_i64()
            .map(|t| Utc::now().timestamp() + t);
        data["expires_at"] = json!(exp);
        json!(
            exp.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                .map(|d| d.to_rfc3339())
        )
    };
    if pur == "organization" {
        let old = &cfg["account"];
        let prior = if let Some(k) = old["credential"].as_str() {
            vault::read(&path(client, cfg, p, k)?)?
        } else {
            Value::Null
        };
        let seed = prior["hook_seed"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()));
        data["hook_seed"] = json!(seed);
        if gh {
            data = json!({"hook_seed":seed});
        }
    }
    save(
        client,
        q,
        cfg,
        &json!({"instance":site,"account":account,"login":login,"installation":installation,"expires":expires}),
        &data,
    )
}
pub(super) fn credential(client: &mut Client, q: &Value, cfg: &Value) -> Result<Value> {
    let pur = purpose(q)?;
    check(cfg, pur)?;
    if q["id"] != "gitlab-self" {
        return Err("invalid: personal-access-token setup applies to self-managed GitLab".into());
    }
    let site = root(
        "gitlab-self",
        if pur == "personal" {
            cfg["account"]["instance"].as_str()
        } else {
            q["instance"].as_str()
        },
    )?;
    identify(
        client,
        q,
        cfg,
        &site,
        json!({"access_token":super::super::input(q,"token")?}),
    )
}
pub(super) fn authorize(client: &mut Client, q: &Value, cfg: &Value) -> Result<Value> {
    let pur = purpose(q)?;
    check(cfg, pur)?;
    let p = field(q, "id")?;
    if p == "gitlab-self" {
        return Err("invalid: use the self-managed token form".into());
    }
    let site = root(p, None)?;
    let client_id = env(&var(p, "CLIENT_ID"))?;
    let redirect = env(&var(p, "REDIRECT_URI"))?;
    let callback = Url::parse(&redirect).map_err(|_| "invalid: OAuth redirect URI")?;
    if !(callback.scheme() == "https"
        || callback.scheme() == "http"
            && (matches!(callback.host(),Some(Host::Ipv4(ip)) if ip.is_loopback())
                || matches!(callback.host(),Some(Host::Ipv6(ip)) if ip.is_loopback())))
        || !callback.username().is_empty()
        || callback.password().is_some()
        || callback.fragment().is_some()
        || callback.query().is_some()
    {
        return Err("invalid: OAuth callback requires HTTPS or loopback".into());
    }
    let key = format!("consent:{}:{pur}", field(cfg, "actor")?);
    let pending = path(client, cfg, p, &key)?;
    if q["callback"].is_null() {
        let state = Uuid::new_v4().simple().to_string();
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = Url::parse(&format!(
            "{}{suffix}",
            transport(p, &site, false)?,
            suffix = if p == "github" {
                "/login/oauth/authorize"
            } else {
                "/oauth/authorize"
            }
        ))
        .map_err(|_| "invalid: authorization endpoint")?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", client_id.as_str()),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("state", &state),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
        ]);
        if p == "gitlab" {
            url.query_pairs_mut().append_pair("scope", "api");
        }
        let install = if p == "github" && pur == "organization" {
            let slug = env("SHIPFORGE_GITHUB_APP_SLUG")?;
            if !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err("invalid: invalid GitHub App slug".into());
            }
            json!(format!("https://github.com/apps/{slug}/installations/new"))
        } else {
            Value::Null
        };
        vault::save(
            &pending,
            Some(
                &json!({"state":state,"verifier":verifier,"expected_org":cfg["account"]["credential"],"expected_owner":cfg[if pur=="personal"{"personal"}else{"account"}]["credential"],"expires":Utc::now().timestamp()+600,"instance":cfg["account"]["instance"],"installation":cfg["account"]["installation"]}),
            ),
        )?;
        return Ok(
            json!({"authorize_url":url.as_str(),"installation_url":install,"redirect_uri":redirect}),
        );
    }
    let received =
        Url::parse(field(q, "callback")?).map_err(|_| "invalid: paste the full callback URL")?;
    let value = |key: &str| {
        let mut values = received
            .query_pairs()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned());
        let v = values.next();
        v.filter(|_| values.next().is_none())
    };
    let row = vault::read(&pending)?;
    if received.origin() != callback.origin()
        || received.path() != callback.path()
        || received.fragment().is_some()
        || !received.username().is_empty()
        || received.password().is_some()
        || value("state").as_deref().is_none_or(|v| row["state"] != v)
        || row["expires"]
            .as_i64()
            .is_none_or(|t| t < Utc::now().timestamp())
        || row["expected_org"] != cfg["account"]["credential"]
        || row["expected_owner"]
            != cfg[if pur == "personal" {
                "personal"
            } else {
                "account"
            }]["credential"]
        || row["installation"] != cfg["account"]["installation"]
        || row["instance"] != cfg["account"]["instance"]
    {
        return Err("forbidden: OAuth state, callback, organization or validity changed".into());
    }
    let code = value("code").ok_or("invalid: authorization code missing")?;
    vault::save(&pending, None)?;
    let data = exchange(
        p,
        &site,
        &json!({"client_id":client_id,"client_secret":env(&var(p,"CLIENT_SECRET"))?,"grant_type":"authorization_code","code":code,"redirect_uri":redirect,"code_verifier":row["verifier"]}),
    )?;
    identify(client, q, cfg, &site, data)
}
pub(super) fn hook_seed(client: &Client, cfg: &Value, p: &str) -> Result<String> {
    let data = vault::read(&path(
        client,
        cfg,
        p,
        field(&cfg["account"], "credential")?,
    )?)?;
    Ok(field(&data, "hook_seed")?.into())
}
