use super::*;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ring::{rand::SystemRandom, signature};

pub(super) fn env(key: &str) -> Result<String> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("invalid: configure {key} on the integration service"))
}
pub(super) fn id(v: &Value) -> Result<String> {
    let n = v
        .as_i64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| *n > 0)
        .ok_or("invalid: expected positive numeric source ID")?;
    Ok(n.to_string())
}
pub(super) fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("invalid: missing {key}"))
}
pub(super) fn root(platform: &str, input: Option<&str>) -> Result<String> {
    match platform {
        "github" => Ok("https://github.com".into()),
        "gitlab" => Ok("https://gitlab.com".into()),
        "gitlab-self" => {
            let u = Url::parse(input.unwrap_or(""))
                .map_err(|_| "invalid: expected self-managed GitLab HTTPS origin")?;
            if u.scheme() != "https"
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
                || u.path() != "/"
                || u.query().is_some()
                || u.fragment().is_some()
                || u.host_str() == Some("gitlab.com")
            {
                return Err("invalid: expected a separate self-managed GitLab HTTPS origin".into());
            }
            Ok(u.origin().ascii_serialization())
        }
        _ => Err("invalid: unknown source platform".into()),
    }
}
pub(super) fn transport(p: &str, site: &str, api: bool) -> Result<String> {
    let var = match p {
        "github" => "SHIPFORGE_FORGE_GITHUB_LOOPBACK",
        "gitlab" => "SHIPFORGE_FORGE_GITLAB_LOOPBACK",
        "gitlab-self" => "SHIPFORGE_FORGE_SELF_LOOPBACK",
        _ => return Err("invalid: unknown source platform".into()),
    };
    if let Ok(test) = std::env::var(var) {
        let u = Url::parse(&test).map_err(|_| "invalid: invalid forge fixture")?;
        if u.scheme() != "http"
            || !(matches!(u.host(),Some(Host::Ipv4(ip)) if ip.is_loopback())
                || matches!(u.host(),Some(Host::Ipv6(ip)) if ip.is_loopback()))
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(
                "invalid: integration fixture must be a literal HTTP loopback origin".into(),
            );
        }
        return Ok(u.origin().ascii_serialization());
    }
    Ok(if api && p == "github" {
        "https://api.github.com"
    } else {
        site
    }
    .into())
}
pub(super) fn call(method: &str, url: &str, token: &str, body: &Value) -> Result<Value> {
    let headers = |builder: ureq::http::request::Builder| {
        let b = builder
            .header("Accept", "application/json")
            .header("User-Agent", "ShipForge-repositories");
        if token.is_empty() {
            b
        } else {
            b.header("Authorization", format!("Bearer {token}"))
        }
    };
    let mut req = headers(ureq::http::Request::builder().method(method).uri(url));
    let payload = if body.is_null() {
        Vec::new()
    } else {
        req = req.header("Content-Type", "application/json");
        serde_json::to_vec(body).map_err(|_| "invalid: source request")?
    };
    let req = req
        .body(payload)
        .map_err(|_| "invalid: invalid source request")?;
    response(
        http()
            .run(req)
            .map_err(|_| "vendor: repository API unreachable")?,
    )
}
pub(super) struct Api {
    pub platform: String,
    pub site: String,
    pub token: String,
}
impl Api {
    pub fn request(&self, method: &str, path: &str, body: &Value) -> Result<Value> {
        let prefix = if self.platform == "github" {
            ""
        } else {
            "/api/v4"
        };
        call(
            method,
            &format!(
                "{}{prefix}{path}",
                transport(&self.platform, &self.site, true)?
            ),
            &self.token,
            body,
        )
    }
    pub fn pages(&self, path: &str, field: Option<&str>) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for page in 1..=50 {
            let sep = if path.contains('?') { '&' } else { '?' };
            let res = self.request(
                "GET",
                &format!("{path}{sep}per_page=100&page={page}"),
                &Value::Null,
            )?;
            let list = field
                .map(|k| &res[k])
                .unwrap_or(&res)
                .as_array()
                .ok_or("vendor: expected paginated source array")?;
            for r in list {
                if !seen.insert(id(&r["id"])?) {
                    return Err("vendor: repeated source page".into());
                }
                out.push(r.clone());
            }
            if list.len() < 100 {
                return Ok(out);
            }
        }
        Err("vendor: repository results exceed 5000 items".into())
    }
    pub fn repos(&self) -> Result<Vec<Value>> {
        let gh = self.platform == "github";
        let input = if gh {
            self.pages("/installation/repositories", Some("repositories"))?
        } else {
            self.pages("/projects?membership=true&min_access_level=40", None)?
        };
        input
            .into_iter()
            .filter(|r| {
                gh || [
                    &r["permissions"]["project_access"],
                    &r["permissions"]["group_access"],
                ]
                .iter()
                .filter_map(|p| p["access_level"].as_i64())
                .max()
                .unwrap_or(0)
                    >= 40
            })
            .map(|r| {
                let path = field(
                    &r,
                    if gh {
                        "full_name"
                    } else {
                        "path_with_namespace"
                    },
                )?;
                if path.len() > 400
                    || path.split('/').count() < 2
                    || path.split('/').any(|s| {
                        s.is_empty()
                            || matches!(s, "." | "..")
                            || !s
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    })
                {
                    return Err("vendor: invalid repository path".into());
                }
                let public = if gh {
                    !r["private"]
                        .as_bool()
                        .ok_or("vendor: missing repository visibility")?
                } else {
                    match r["visibility"].as_str() {
                        Some("public") => true,
                        Some("internal" | "private") => false,
                        _ => return Err("vendor: missing project visibility".into()),
                    }
                };
                Ok(json!({"repository_id":id(&r["id"])?,"repository":path,"public":public}))
            })
            .collect()
    }
}
pub(super) fn app_token(installation: &str, site: &str) -> Result<String> {
    let key = std::fs::read_to_string(env("SHIPFORGE_GITHUB_PRIVATE_KEY_FILE")?)
        .map_err(|_| "invalid: cannot read GitHub App RSA private key")?;
    let der = STANDARD
        .decode(
            key.lines()
                .filter(|l| !l.starts_with('-'))
                .map(str::trim)
                .collect::<String>(),
        )
        .map_err(|_| "invalid: invalid GitHub PEM key")?;
    let key = signature::RsaKeyPair::from_der(&der)
        .or_else(|_| signature::RsaKeyPair::from_pkcs8(&der))
        .map_err(|_| "invalid: unsupported GitHub RSA key")?;
    let now = chrono::Utc::now().timestamp();
    let enc = |v: Value| URL_SAFE_NO_PAD.encode(v.to_string().as_bytes());
    let head = format!(
        "{}.{}",
        enc(json!({"alg":"RS256","typ":"JWT"})),
        enc(json!({"iat":now-60,"exp":now+480,"iss":env("SHIPFORGE_GITHUB_APP_ID")?}))
    );
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &signature::RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        head.as_bytes(),
        &mut signature,
    )
    .map_err(|_| "vendor: could not sign GitHub App request")?;
    let api = Api {
        platform: "github".into(),
        site: site.into(),
        token: format!("{head}.{}", URL_SAFE_NO_PAD.encode(signature)),
    };
    let res = api.request(
        "POST",
        &format!(
            "/app/installations/{}/access_tokens",
            id(&json!(installation))?
        ),
        &json!({}),
    )?;
    Ok(field(&res, "token")?.into())
}
pub(super) fn permission_error(e: &str) -> bool {
    matches!(
        e,
        "vendor: upstream returned HTTP 401"
            | "vendor: upstream returned HTTP 403"
            | "vendor: upstream returned HTTP 404"
    )
}
