use super::*;
use flate2::{Compression, write::GzEncoder};
use std::io::Write;
use url::Url;
pub(super) mod gateway;
pub(super) mod messages;
pub(super) fn branded(p: &str) -> bool {
    matches!(p, "feishu" | "lark")
}
fn fixture(brand: &str) -> R<Option<Url>> {
    net::fixture_from(if brand == "lark" {
        "SHIPFORGE_LARK_LOOPBACK"
    } else {
        "SHIPFORGE_FEISHU_LOOPBACK"
    })
}
fn domain(brand: &str) -> &'static str {
    if brand == "lark" {
        "larksuite.com"
    } else {
        "feishu.cn"
    }
}
fn root(brand: &str, service: &str) -> R<String> {
    Ok(fixture(brand)?.map_or_else(
        || format!("https://{service}.{}", domain(brand)),
        |u| u.as_str().trim_end_matches('/').into(),
    ))
}
fn url(brand: &str, route: &str) -> R<String> {
    Ok(net::endpoint(&root(brand, "open")?, route))
}
fn safe(brand: &str, raw: &str, scheme: &str) -> R<Url> {
    net::trusted_with(raw, scheme, &[domain(brand)], fixture(brand)?)
}
fn registration(brand: &str, pairs: &[(&str, &str)]) -> R<Value> {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs.iter().copied())
        .finish();
    let u = net::endpoint(&root(brand, "accounts")?, "/oauth/v1/app/registration");
    let mut res = net::http()
        .post(u)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(encoded.as_bytes())
        .map_err(|_| "network: device authorization interrupted")?;
    let code = res.status().as_u16();
    if code != 400 && !(200..300).contains(&code) {
        return Err(format!("provider: HTTP {code}"));
    }
    res.body_mut()
        .with_config()
        .limit(128 * 1024)
        .read_json()
        .map_err(|_| "provider: invalid device authorization response".into())
}
fn addons() -> R<String> {
    let config = json!({"scopes":{"tenant":["im:chat:readonly","im:message.p2p_msg:readonly","im:message.group_at_msg:readonly","im:message:send_as_bot","im:resource","im:message.reactions:read","im:message.reactions:write_only"]},"events":{"items":{"tenant":["im.message.receive_v1","im.message.reaction.created_v1"]}}});
    let mut zip = GzEncoder::new(Vec::new(), Compression::fast());
    zip.write_all(config.to_string().as_bytes())
        .map_err(|_| "storage: app configuration encoding failed")?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        zip.finish()
            .map_err(|_| "storage: app configuration encoding failed")?,
    ))
}
pub(super) fn start(s: &mut Value) -> R<()> {
    let brand = field(s, "provider")?;
    let d = registration(
        "feishu",
        &[
            ("action", "begin"),
            ("archetype", "PersonalAgent"),
            ("auth_method", "client_secret"),
            ("request_user_info", "open_id tenant_brand"),
        ],
    )?;
    let mut u = if let Some(raw) = d["verification_uri_complete"]
        .as_str()
        .filter(|_| brand == "feishu")
    {
        safe(brand, raw, "https")?
    } else {
        let mut u = Url::parse(if brand == "lark" {
            "https://open.larksuite.com/page/cli"
        } else {
            "https://open.feishu.cn/page/cli"
        })
        .unwrap();
        u.query_pairs_mut()
            .append_pair("user_code", field(&d, "user_code")?);
        u
    };
    u.query_pairs_mut()
        .append_pair("tp", "sdk")
        .append_pair("from", "sdk")
        .append_pair("source", "ShipForge")
        .append_pair("addons", &addons()?);
    s["qr"] = json!(u.as_str());
    s["qr_id"] = json!(field(&d, "device_code")?);
    s["poll_cloud"] = json!("feishu");
    s["interval"] = json!(d["interval"].as_i64().unwrap_or(5).clamp(1, 60));
    s["expires"] = json!(
        chrono::Utc::now().timestamp()
            + d["expire_in"]
                .as_i64()
                .or(d["expires_in"].as_i64())
                .unwrap_or(600)
                .clamp(1, 3600)
    );
    Ok(())
}
pub(super) fn check(s: &mut Value) -> R<()> {
    let now = chrono::Utc::now().timestamp();
    if now >= s["expires"].as_i64().unwrap_or(0) {
        s["status"] = json!("expired");
        return Ok(());
    }
    if s["next_poll"].as_i64().is_some_and(|next| next > now) {
        return Ok(());
    }
    let cloud = field(s, "poll_cloud")?.to_owned();
    let d = registration(
        &cloud,
        &[("action", "poll"), ("device_code", field(s, "qr_id")?)],
    )?;
    let selected = field(s, "provider")?.to_owned();
    if let Some(brand) = d["user_info"]["tenant_brand"]
        .as_str()
        .filter(|v| !v.is_empty())
    {
        if brand != selected {
            s["status"] = json!("wrong_cloud");
            return Ok(());
        }
        if brand == "lark" && cloud != "lark" {
            s["poll_cloud"] = json!("lark");
            s["next_poll"] = json!(0);
            return Ok(());
        }
    }
    if d["client_id"].is_string() && d["client_secret"].is_string() {
        if cloud != selected {
            s["status"] = json!("wrong_cloud");
            return Ok(());
        }
        s["account"] = json!(field(&d, "client_id")?);
        s["secret"] = json!(field(&d, "client_secret")?);
        let info = api(s, "/open-apis/bot/v3/info", None)?;
        s["bot_open_id"] = json!(field(&info["bot"], "open_id")?);
        s["status"] = json!("confirmed");
        return Ok(());
    }
    match d["error"].as_str() {
        Some("authorization_pending") => {}
        Some("slow_down") => {
            s["interval"] = json!((s["interval"].as_i64().unwrap_or(5) + 5).min(60))
        }
        Some("access_denied") => s["status"] = json!("denied"),
        Some("expired_token") => s["status"] = json!("expired"),
        _ => return Err("provider: unrecognized Lark authorization status".into()),
    }
    s["next_poll"] = json!(now + s["interval"].as_i64().unwrap_or(5));
    Ok(())
}
fn token(s: &mut Value) -> R<String> {
    let now = chrono::Utc::now().timestamp();
    if s["access_expiry"].as_i64().is_some_and(|v| v > now + 60)
        && let Some(token) = s["access"].as_str()
    {
        return Ok(token.into());
    }
    let u = url(
        field(s, "provider")?,
        "/open-apis/auth/v3/tenant_access_token/internal",
    )?;
    let d = net::request(
        &u,
        Some(json!({"app_id":field(s,"account")?,"app_secret":field(s,"secret")?})),
        &[],
    )?;
    if matches!(d["code"].as_i64(), Some(10003 | 10014 | 99991673)) {
        return Err("provider: Lark application revoked or invalid".into());
    }
    let value = field(&d, "tenant_access_token")?.to_owned();
    s["access"] = json!(value);
    s["access_expiry"] = json!(now + d["expire"].as_i64().unwrap_or(600).clamp(1, 86400));
    Ok(value)
}
fn api(s: &mut Value, route: &str, body: Option<Value>) -> R<Value> {
    let u = url(field(s, "provider")?, route)?;
    for attempt in 0..2 {
        let h = [("Authorization", format!("Bearer {}", token(s)?))];
        let r = net::request(&u, body.clone(), &h);
        if attempt == 0 && r.as_ref().is_err_and(|e| e.contains("HTTP 401")) {
            s["access_expiry"] = json!(0);
            continue;
        }
        let data = r?;
        let code = data["code"].as_i64().unwrap_or(0);
        if code == 0 {
            return Ok(data);
        }
        if attempt == 0 && matches!(code, 99991663 | 99991664 | 99991668) {
            s["access_expiry"] = json!(0);
            continue;
        }
        return Err(if matches!(code, 99991400 | 99991401) {
            "provider: HTTP 429".into()
        } else {
            format!("provider: Lark rejected the operation ({code})")
        });
    }
    Err("provider: Lark authorization expired".into())
}
fn component(x: &str) -> R<&str> {
    if x.is_empty()
        || x.len() > 256
        || !x
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err("provider: invalid platform resource identifier".into());
    }
    Ok(x)
}
