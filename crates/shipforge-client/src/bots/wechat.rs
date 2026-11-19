use super::*;
use aes::{
    Aes128,
    cipher::{BlockEncrypt, KeyInit},
};
use md5::{Digest, Md5};
use url::Url;
const ROOT: &str = "https://ilinkai.weixin.qq.com";
const WX: &[&str] = &["weixin.qq.com", "wechat.com"];
fn headers(s: &Value, authenticated: bool) -> R<Vec<(&'static str, String)>> {
    let mut h = vec![
        ("iLink-App-Id", "bot".into()),
        ("iLink-App-ClientVersion", "196609".into()),
    ];
    if authenticated {
        h.push(("AuthorizationType", "ilink_bot_token".into()));
        h.push((
            "X-WECHAT-UIN",
            STANDARD.encode(u32::from_le_bytes(rand_bytes::<4>()?).to_string()),
        ));
        if let Some(token) = s["token"].as_str() {
            h.push(("Authorization", format!("Bearer {token}")));
        }
    }
    Ok(h)
}
fn api(s: &Value, route: &str, mut body: Value) -> R<Value> {
    body["base_info"] = json!({"channel_version":concat!("3.0.1/ShipForge-",env!("CARGO_PKG_VERSION")),"bot_agent":concat!("ShipForge/",env!("CARGO_PKG_VERSION"))});
    let data = net::request(
        &net::endpoint(field(s, "root")?, route),
        Some(body),
        &headers(s, true)?,
    )?;
    for k in ["ret", "errcode"] {
        if let Some(code) = data[k].as_i64().filter(|code| *code != 0) {
            return Err(if code == -14 {
                "provider: WeChat session expired; reconnect".into()
            } else {
                format!("provider: WeChat rejected operation ({code})")
            });
        }
    }
    Ok(data)
}
pub(super) fn start(s: &mut Value) -> R<()> {
    let base = net::root(ROOT)?;
    let d = net::request(
        &net::endpoint(&base, "/ilink/bot/get_bot_qrcode?bot_type=3"),
        Some(json!({"local_token_list":[]})),
        &headers(s, true)?,
    )?;
    s["qr_id"] = json!(field(&d, "qrcode")?);
    s["qr"] = json!(qr_content(&d["qrcode_img_content"])?);
    s["root"] = json!(base);
    Ok(())
}
pub(super) fn check(s: &mut Value, q: &Value) -> R<()> {
    let mut u = Url::parse(&net::endpoint(
        field(s, "root")?,
        "/ilink/bot/get_qrcode_status",
    ))
    .map_err(|_| "invalid: WeChat QR endpoint")?;
    u.query_pairs_mut()
        .append_pair("qrcode", field(s, "qr_id")?);
    if let Some(pin) = q["verify_code"].as_str().filter(|p| !p.is_empty()) {
        if !s["status"]
            .as_str()
            .is_some_and(|v| v.contains("need_verifycode"))
            || !(4..=12).contains(&pin.len())
            || !pin.bytes().all(|x| x.is_ascii_digit())
        {
            return Err("invalid: enter the digits from the current WeChat confirmation".into());
        }
        u.query_pairs_mut().append_pair("verify_code", pin);
    }
    let d = net::request(u.as_str(), None, &headers(s, false)?)?;
    let status = field(&d, "status")?;
    if status == "confirmed" {
        let base = net::trusted(field(&d, "baseurl")?, "https", WX)?;
        s["root"] = json!(base.as_str().trim_end_matches('/'));
        s["account"] = json!(field(&d, "ilink_user_id")?);
        s["bot"] = json!(field(&d, "ilink_bot_id")?);
        s["token"] = json!(field(&d, "bot_token")?);
    } else if status == "scaned_but_redirect" {
        let host = field(&d, "redirect_host")?;
        let url = net::trusted(&format!("https://{host}"), "https", WX)?;
        s["root"] = json!(url.as_str().trim_end_matches('/'));
    } else if !matches!(
        status,
        "wait" | "scaned" | "verify_code_blocked" | "binded_redirect" | "expired"
    ) && !status.contains("need_verifycode")
    {
        return Err("provider: unknown WeChat QR status".into());
    }
    s["status"] = json!(status);
    Ok(())
}
pub(super) fn poll(c: &mut Client, v: &vault::Vault, s: &mut Value) -> R<usize> {
    let data = api(
        s,
        "/ilink/bot/getupdates",
        json!({"get_updates_buf":s["cursor"].as_str().unwrap_or("")}),
    )?;
    let mut count = 0;
    for msg in data["msgs"].as_array().into_iter().flatten() {
        if msg["message_type"] != 1
            || msg["from_user_id"] != s["account"]
            || msg["group_id"].as_str().is_some_and(|s| !s.is_empty())
            || msg["to_user_id"]
                .as_str()
                .is_some_and(|x| !x.is_empty() && s["bot"] != x)
        {
            continue;
        }
        let text = msg["item_list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| match v["type"].as_i64() {
                Some(1) => v["text_item"]["text"].as_str(),
                Some(3) => v["voice_item"]["text"].as_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            continue;
        }
        let remote = msg["message_id"]
            .as_u64()
            .filter(|id| *id > 0)
            .map(|n| n.to_string())
            .or_else(|| {
                msg["message_id"]
                    .as_str()
                    .or(msg["client_id"].as_str())
                    .map(str::to_owned)
            })
            .ok_or("provider: missing WeChat message identity")?;
        let peer = field(msg, "from_user_id")?;
        if let Ok(context) = field(msg, "context_token") {
            v.context(field(s, "id")?, peer, &remote, context)?;
        }
        let r = adapter(
            c,
            s,
            "bot.receive",
            json!({"scene":"direct","peer":peer,"sender":peer,"remote":remote,"body":text}),
        )?;
        if r["duplicate"] == false {
            count += 1;
        }
    }
    if data["get_updates_buf"]
        .as_str()
        .is_some_and(|v| !v.is_empty())
    {
        s["cursor"] = data["get_updates_buf"].clone();
    }
    Ok(count)
}
fn file_item(c: &mut Client, s: &Value, work: &Value) -> R<Value> {
    let p = &work["payload"];
    let file=c.execute(json!({"op":"attachment.download","space":s["space"],"kind":"artifact","id":p["artifact"],"attachment_id":p["attachment"],"allow_cached":false}))?;
    let bytes = STANDARD
        .decode(
            file["content_base64"]
                .as_str()
                .ok_or("invalid: attachment bytes unavailable")?,
        )
        .map_err(|_| "invalid: corrupted attachment")?;
    let len = bytes.len();
    let raw_md5 = format!("{:x}", Md5::digest(&bytes));
    let key = rand_bytes::<16>()?;
    let aeskey = key.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let cipher = Aes128::new((&key).into());
    let mut encrypted = bytes;
    let pad = 16 - len % 16;
    encrypted.resize(len + pad, pad as u8);
    for chunk in encrypted.as_chunks_mut::<16>().0 {
        cipher.encrypt_block(chunk.into());
    }
    let filekey = Uuid::new_v4().simple().to_string();
    let info = api(
        s,
        "/ilink/bot/getuploadurl",
        json!({"filekey":filekey,"media_type":3,"to_user_id":work["peer"],"rawsize":len,"rawfilemd5":raw_md5,"filesize":encrypted.len(),"no_need_thumb":true,"aeskey":aeskey}),
    )?;
    let url = if let Some(raw) = info["upload_full_url"].as_str().filter(|v| !v.is_empty()) {
        net::trusted(raw, "https", WX)?
    } else {
        let base = net::root("https://novac2c.cdn.weixin.qq.com")?;
        let mut u =
            Url::parse(&net::endpoint(&base, "/c2c/upload")).map_err(|_| "invalid: CDN address")?;
        u.query_pairs_mut()
            .append_pair("encrypted_query_param", field(&info, "upload_param")?)
            .append_pair("filekey", &filekey);
        u
    };
    let res = net::http()
        .post(url.as_str())
        .header("Content-Type", "application/octet-stream")
        .send(encrypted.as_slice())
        .map_err(|_| "provider: WeChat file upload did not finish")?;
    if !res.status().is_success() {
        return Err(format!("provider: upload HTTP {}", res.status().as_u16()));
    }
    let param = res
        .headers()
        .get("x-encrypted-param")
        .and_then(|h| h.to_str().ok())
        .filter(|p| !p.is_empty())
        .ok_or("provider: missing WeChat upload receipt")?;
    Ok(
        json!({"type":4,"file_item":{"file_name":file["filename"],"len":len.to_string(),"media":{"encrypt_query_param":param,"aes_key":STANDARD.encode(aeskey.as_bytes()),"encrypt_type":1}}}),
    )
}
pub(super) fn send(c: &mut Client, v: &vault::Vault, s: &Value, w: &Value) -> R<String> {
    let context = v.reply(field(s, "id")?, field(w, "peer")?, field(w, "remote")?)?;
    let item = if let Some(text) = w["payload"]["text"].as_str() {
        json!({"type":1,"text_item":{"text":text}})
    } else {
        file_item(c, s, w)?
    };
    api(
        s,
        "/ilink/bot/sendmessage",
        json!({"msg":{"from_user_id":"","to_user_id":w["peer"],"client_id":w["delivery"],"message_type":2,"message_state":2,"context_token":context,"item_list":[item]}}),
    )?;
    Ok(field(w, "delivery")?.into())
}
