use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn challenge_location(
    client: &Client,
    who: &Value,
    b: &Value,
    q: &Value,
) -> Result<Option<std::path::PathBuf>> {
    vault::location(
        client,
        who,
        b,
        "tracker-oauth-pending",
        &json!([
            b["provider"],
            q["space"].as_str().unwrap_or("default"),
            q["id"]
        ])
        .to_string(),
    )
}
pub(super) fn start(client: &Client, who: &Value, b: &Value, q: &Value) -> Result<Value> {
    if b["cloud_auth"] != "oauth" || b["provider"] == "ones" {
        return Err("invalid: select OAuth on this connector first".into());
    }
    let client_id = cred(q, "client_id")?;
    let redirect = cred(q, "redirect_uri")?;
    let callback =
        Url::parse(&redirect).map_err(|_| "invalid: enter the registered redirect URL")?;
    let local = callback
        .host_str()
        .is_some_and(|s| matches!(s, "localhost" | "127.0.0.1" | "[::1]"));
    if callback.host().is_none()
        || !(callback.scheme() == "https" || local && callback.scheme() == "http")
        || !callback.username().is_empty()
        || callback.password().is_some()
        || callback.fragment().is_some()
    {
        return Err("invalid: OAuth redirect must be a registered HTTPS or loopback URL".into());
    }
    let state = Uuid::new_v4().simple().to_string();
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let linear = b["provider"] == "linear";
    let mut url = Url::parse(if linear {
        "https://linear.app/oauth/authorize"
    } else {
        "https://app.clickup.com/api"
    })
    .unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("state", &state);
    if linear {
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair(
                "scope",
                if q["write"] == true {
                    "read,write"
                } else {
                    "read"
                },
            )
            .append_pair("code_challenge_method", "S256")
            .append_pair(
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            );
    }
    vault::save(
        &challenge_location(client, who, b, q)?,
        Some(
            &json!({"state":state,"client_id":client_id,"redirect_uri":redirect,"verifier":verifier,"expires":chrono::Utc::now().timestamp()+600}),
        ),
    )?;
    Ok(json!({"authorize_url":url.as_str(),"expires_in":600}))
}
fn linear_token(data: &[(&str, String)]) -> Result<Value> {
    let root = origin("https://api.linear.app")?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(data.iter().map(|(k, v)| (*k, v.as_str())))
        .finish();
    let response = http()
        .post(format!("{root}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_bytes())
        .map_err(|_| "vendor: Linear token exchange unavailable")?;
    unpack(response)
}
pub(super) fn exchange(client: &Client, who: &Value, b: &Value, q: &Value) -> Result<Value> {
    if b["cloud_auth"] != "oauth" || b["provider"] == "ones" {
        return Err("invalid: OAuth is unavailable on this connector".into());
    }
    let location = challenge_location(client, who, b, q)?;
    let handshake = vault::read(&location)?;
    if handshake["state"] != q["state"]
        || !q["state"].is_string()
        || handshake["expires"]
            .as_i64()
            .is_none_or(|at| at < chrono::Utc::now().timestamp())
    {
        return Err("forbidden: OAuth state is invalid or expired".into());
    }
    let client_id = cred(&handshake, "client_id")?;
    let code = cred(q, "code")?;
    let mut stored = if b["provider"] == "linear" {
        let mut args = vec![
            ("grant_type", "authorization_code".into()),
            ("code", code),
            ("client_id", client_id.clone()),
            ("redirect_uri", cred(&handshake, "redirect_uri")?),
            ("code_verifier", cred(&handshake, "verifier")?),
        ];
        if let Some(secret) = q["client_secret"].as_str().filter(|s| !s.is_empty()) {
            args.push(("client_secret", secret.to_owned()));
        }
        linear_token(&args)?
    } else {
        let base = origin("https://api.clickup.com")?;
        unpack(http().post(format!("{base}/api/v2/oauth/token")).send_json(json!({"client_id":client_id,"client_secret":cred(q,"client_secret")?,"code":code})).map_err(|_|"vendor: ClickUp token exchange unavailable")?)?
    };
    let token = cred(&stored, "access_token")?;
    let mut selected = json!({"access_token":token});
    if b["provider"] == "linear" && stored["refresh_token"].is_string() {
        selected["refresh_token"] = stored["refresh_token"].take();
        selected["client_id"] = json!(client_id);
        if let Some(secret) = q["client_secret"].as_str().filter(|s| !s.is_empty()) {
            selected["client_secret"] = json!(secret);
        }
    }
    vault::save(&location, None)?;
    Ok(selected)
}
pub(super) fn refresh(old: &Value) -> Result<Value> {
    let mut args = vec![
        ("grant_type", "refresh_token".into()),
        ("refresh_token", cred(old, "refresh_token")?),
        ("client_id", cred(old, "client_id")?),
    ];
    if let Some(secret) = old["client_secret"].as_str() {
        args.push(("client_secret", secret.into()));
    }
    let result = linear_token(&args)?;
    let mut next = old.clone();
    next["access_token"] = json!(cred(&result, "access_token")?);
    if let Some(token) = result["refresh_token"].as_str() {
        next["refresh_token"] = json!(token);
    }
    Ok(next)
}
