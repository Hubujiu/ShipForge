use super::*;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use url::Url;
pub(super) fn start(s: &mut Value) -> R<()> {
    let key = STANDARD.encode(rand_bytes::<32>()?);
    let root = net::root("https://q.qq.com")?;
    let d = net::request(
        &net::endpoint(&root, "/lite/create_bind_task"),
        Some(json!({"key":key})),
        &[],
    )?;
    let task = field(&d["data"], "task_id")?;
    let mut u = Url::parse("https://q.qq.com/qqbot/openclaw/connect.html").unwrap();
    u.query_pairs_mut()
        .append_pair("task_id", task)
        .append_pair("source", "shipforge")
        .append_pair("_wv", "2");
    s["qr"] = json!(u.as_str());
    s["qr_key"] = json!(key);
    s["qr_id"] = json!(task);
    Ok(())
}
pub(super) fn check(s: &mut Value) -> R<()> {
    let root = net::root("https://q.qq.com")?;
    let response = net::request(
        &net::endpoint(&root, "/lite/poll_bind_result"),
        Some(json!({"task_id":s["qr_id"]})),
        &[],
    )?;
    let d = &response["data"];
    let status = d["status"]
        .as_i64()
        .ok_or("provider: missing QQ confirmation status")?;
    s["status"] = json!(match status {
        0 | 1 => "wait",
        2 => "confirmed",
        3 => "expired",
        _ => return Err("provider: unknown QQ confirmation status".into()),
    });
    if status == 2 {
        let raw = STANDARD
            .decode(field(s, "qr_key")?)
            .map_err(|_| "storage: QR key corrupt")?;
        let key = UnboundKey::new(&AES_256_GCM, &raw).map_err(|_| "storage: QR key corrupt")?;
        let mut encrypted = STANDARD
            .decode(field(d, "bot_encrypt_secret")?)
            .map_err(|_| "provider: invalid QQ sealed credential")?;
        if encrypted.len() < 29 {
            return Err("provider: truncated QQ sealed credential".into());
        }
        let iv = Nonce::try_assume_unique_for_key(&encrypted[..12])
            .map_err(|_| "provider: invalid QQ sealed credential")?;
        let secret = LessSafeKey::new(key)
            .open_in_place(iv, Aad::empty(), &mut encrypted[12..])
            .map_err(|_| "provider: QQ credential authentication failed")?;
        s["account"] = json!(field(d, "bot_appid")?);
        s["secret"] =
            json!(std::str::from_utf8(secret).map_err(|_| "provider: invalid QQ credential")?);
        token(s)?;
    }
    Ok(())
}
pub(super) fn token(s: &mut Value) -> R<String> {
    let now = chrono::Utc::now().timestamp();
    if s["access_expiry"]
        .as_i64()
        .is_some_and(|expiry| expiry > now + 60)
        && let Some(token) = s["access"].as_str()
    {
        return Ok(token.into());
    }
    let root = net::root("https://bots.qq.com")?;
    let d = net::request(
        &net::endpoint(&root, "/app/getAppAccessToken"),
        Some(json!({"appId":field(s,"account")?,"clientSecret":field(s,"secret")?})),
        &[],
    )
    .map_err(|e| {
        if e.contains("HTTP 401") || e.contains("HTTP 403") {
            "provider: QQ credentials rejected; reconnect".to_owned()
        } else {
            e
        }
    })?;
    if d.get("code").is_some() && !d["access_token"].is_string() {
        return Err("provider: QQ credentials rejected; reconnect".into());
    }
    let value = field(&d, "access_token")?.to_owned();
    let lifetime = d["expires_in"]
        .as_i64()
        .or_else(|| d["expires_in"].as_str()?.parse().ok())
        .unwrap_or(300)
        .clamp(1, 86400);
    s["access"] = json!(value);
    s["access_expiry"] = json!(now + lifetime);
    Ok(value)
}
pub(super) fn api(s: &mut Value, route: &str, body: Option<Value>) -> R<Value> {
    let base = net::root("https://api.sgroup.qq.com")?;
    for attempt in 0..2 {
        let token = token(s)?;
        let h = [
            ("Authorization", format!("QQBot {token}")),
            ("X-Union-Appid", field(s, "account")?.into()),
        ];
        let res = net::request(&net::endpoint(&base, route), body.clone(), &h);
        if attempt == 0 && res.as_ref().is_err_and(|e| e == "provider: HTTP 401") {
            s["access_expiry"] = json!(0);
            continue;
        }
        let d = res?;
        if let Some(code) = d["code"].as_i64().filter(|c| *c != 0) {
            return Err(format!("provider: QQ rejected operation ({code})"));
        }
        return Ok(d);
    }
    Err("provider: QQ credentials expired".into())
}
pub(super) fn send(s: &mut Value, w: &Value) -> R<String> {
    let peer = field(w, "peer")?;
    if !peer
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
    {
        return Err("provider: invalid QQ conversation identity".into());
    }
    let scope = if w["scene"] == "group" {
        "groups"
    } else {
        "users"
    };
    let d = api(
        s,
        &format!("/v2/{scope}/{peer}/messages"),
        Some(
            json!({"msg_type":0,"content":w["payload"]["text"],"msg_id":w["remote"],"msg_seq":w["part"]}),
        ),
    )?;
    Ok(field(&d, "id")?.into())
}
