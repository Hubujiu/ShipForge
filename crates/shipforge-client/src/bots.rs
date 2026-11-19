//! Device-owned official Tencent adapters. The team server stores bindings and conversation work.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::rand::{SecureRandom, SystemRandom};
use uuid::Uuid;
mod gateway;
mod net;
mod qq;
mod vault;
mod wechat;
type R<T> = Result<T, String>;
pub(crate) const LOCAL: &[&str] = &[
    "bot.qr.start",
    "bot.qr.check",
    "bot.qq.connect",
    "bot.sync",
    "bot.disconnect",
];
fn field<'a>(v: &'a Value, k: &str) -> R<&'a str> {
    v[k].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.chars().any(char::is_control))
        .ok_or_else(|| format!("invalid: missing or invalid {k}"))
}
fn rand_bytes<const N: usize>() -> R<[u8; N]> {
    let mut b = [0; N];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| "storage: random generator unavailable")?;
    Ok(b)
}
fn adapter(c: &mut Client, s: &Value, op: &str, mut q: Value) -> R<Value> {
    q["op"] = json!(op);
    q["id"] = s["id"].clone();
    q["space"] = s["space"].clone();
    q["device_key"] = s["key"].clone();
    q["allow_cached"] = json!(false);
    q["queue_if_offline"] = json!(false);
    c.execute(q)
}
pub fn qr_png(data: &str) -> Option<Vec<u8>> {
    let data = data.strip_prefix("data:image/png;base64,")?;
    if data.len() > 256 * 1024 {
        return None;
    }
    let bytes = STANDARD.decode(data).ok()?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    ((1..=1024).contains(&width) && (1..=1024).contains(&height)).then_some(bytes)
}
fn qr_content(v: &Value) -> R<&str> {
    let content = v
        .as_str()
        .filter(|t| !t.is_empty() && t.len() <= 256 * 1024 && !t.chars().any(char::is_control))
        .ok_or("provider: invalid QR content")?;
    if content.starts_with("data:") && qr_png(content).is_none() {
        return Err("provider: QR image must be a valid PNG".into());
    }
    Ok(content)
}
fn qr(s: &Value) -> R<Value> {
    let content = qr_content(&s["qr"])?;
    let mut result =
        json!({"id":s["id"],"state":s["status"],"provider":s["provider"],"qr_content":content});
    if !content.starts_with("data:image/png;base64,") {
        let code = qrcodegen::QrCode::encode_text(content, qrcodegen::QrCodeEcc::Medium)
            .map_err(|_| "provider: QR content is too long")?;
        let matrix = (0..code.size())
            .map(|y| {
                (0..code.size())
                    .map(|x| if code.get_module(x, y) { '1' } else { '0' })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        result["qr_matrix"] = json!(matrix);
    }
    Ok(result)
}
fn connect(c: &mut Client, v: &vault::Vault, s: &mut Value) -> R<Value> {
    let id = field(s, "id")?.to_owned();
    v.save(&id, s)?;
    let binding = adapter(
        c,
        s,
        "bot.attach",
        json!({"provider":s["provider"],"account":s["account"],"agent":s["agent"],"dm":s["dm"],"groups":s["groups"]}),
    )?;
    s["status"] = json!("connected");
    for key in ["qr", "qr_key", "qr_id"] {
        s.as_object_mut().unwrap().remove(key);
    }
    v.save(&id, s)?;
    Ok(binding)
}
fn start(c: &mut Client, v: &vault::Vault, q: &Value, space: &str, manual: bool) -> R<Value> {
    let provider = if manual { "qq" } else { field(q, "provider")? };
    if !matches!(provider, "qq" | "wechat") {
        return Err("invalid: provider must be qq or wechat".into());
    }
    let who = c.execute(json!({"op":"whoami","space":space,"allow_cached":false}))?;
    if !matches!(
        who["role"].as_str(),
        Some("owner" | "admin" | "member" | "editor")
    ) {
        return Err("forbidden: space editing access required".into());
    }
    let agent = field(q, "agent")?;
    let record = c.execute(
        json!({"op":"get","space":space,"kind":"agent","id":agent,"allow_cached":false}),
    )?;
    if record["fields"]["archived"] == true || record["fields"]["can_use"] == false {
        return Err("forbidden: Agent unavailable".into());
    }
    let id = Uuid::new_v4().to_string();
    let mut s = json!({"id":id,"space":space,"provider":provider,"key":STANDARD.encode(rand_bytes::<32>()?),"agent":agent,"dm":q["dm"].as_bool().unwrap_or(true),"groups":q["groups"].as_bool().unwrap_or(provider=="qq"),"status":"wait"});
    if provider == "wechat" && s["groups"] == true {
        return Err("invalid: personal WeChat supports direct chat".into());
    }
    if manual {
        s["account"] = json!(field(q, "app_id")?);
        s["secret"] = json!(field(q, "app_secret")?);
        qq::token(&mut s)?;
        return connect(c, v, &mut s);
    }
    if provider == "wechat" {
        wechat::start(&mut s)?;
    } else {
        qq::start(&mut s)?;
    }
    v.save(&id, &s)?;
    qr(&s)
}
pub(super) fn execute(c: &mut Client, q: &Value) -> R<Value> {
    let space = q["space"].as_str().unwrap_or("default");
    let op = field(q, "op")?;
    let identity = c.execute(json!({"op":"bot.bindings","space":space,"allow_cached":false}))?;
    let scope = json!([identity["workspace"], identity["space"], identity["actor"]]).to_string();
    let v = vault::Vault::new(
        c.directory
            .as_deref()
            .ok_or("invalid: external accounts require a saved device profile")?,
        scope,
    )?;
    if matches!(op, "bot.qr.start" | "bot.qq.connect") {
        return start(c, &v, q, space, op == "bot.qq.connect");
    }
    let id = field(q, "id")?;
    let binding = identity["items"]
        .as_array()
        .and_then(|items| items.iter().find(|b| b["id"] == id));
    if op == "bot.disconnect" {
        let result = if binding.is_some() {
            c.execute(json!({"op":"bot.detach","id":id,"space":space}))?
        } else {
            v.load(id)?;
            json!({"id":id,"state":"disconnected"})
        };
        v.remove(id)?;
        gateway::close(id);
        return Ok(result);
    }
    if op == "bot.sync" && !binding.is_some_and(|b| b["state"] == "connected") {
        v.remove(id)?;
        gateway::close(id);
        return Err("conflict: binding disconnected or replaced".into());
    }
    let _guard = v.lock(id)?;
    let mut s = v.load(id)?;
    if op == "bot.qr.check" {
        if let Some(b) = binding {
            return Ok(b.clone());
        }
        if matches!(
            s["status"].as_str(),
            Some("expired" | "verify_code_blocked" | "binded_redirect")
        ) {
            return Ok(json!({"id":id,"state":s["status"]}));
        }
        if s["status"] != "confirmed" {
            if s["provider"] == "wechat" {
                wechat::check(&mut s, q)?;
            } else {
                qq::check(&mut s)?;
            }
            v.save(id, &s)?;
        }
        if s["status"] == "confirmed" {
            return connect(c, &v, &mut s);
        }
        Ok(json!({"id":id,"state":s["status"],"provider":s["provider"]}))
    } else {
        let pending = deliver(c, &v, &mut s)?;
        let poll = if s["provider"] == "wechat" {
            wechat::poll(c, &v, &mut s)
        } else {
            gateway::poll(c, &v, &mut s)
        };
        if poll.as_ref().is_err_and(|e| {
            e.contains("WeChat session expired") || e.contains("QQ credentials rejected")
        }) {
            adapter(c, &s, "bot.expire", json!({}))?;
            v.remove(id)?;
            gateway::close(id);
        }
        let incoming = poll?;
        v.save(id, &s)?;
        let outgoing = deliver(c, &v, &mut s)?;
        Ok(json!({"id":id,"received":incoming,"outgoing":pending+outgoing,"state":"connected"}))
    }
}
fn deliver(c: &mut Client, v: &vault::Vault, s: &mut Value) -> R<usize> {
    let mut count = 0;
    for _ in 0..8 {
        let work = adapter(c, s, "bot.claim", json!({}))?;
        if work.is_null() {
            break;
        }
        let send = if s["provider"] == "wechat" {
            wechat::send(c, v, s, &work)
        } else {
            qq::send(s, &work)
        };
        let outcome = match &send {
            Ok(_) => "sent",
            Err(e) if e.contains("HTTP 429") => "retry",
            Err(e) if e.starts_with("network:") || e.contains("HTTP 5") => "uncertain",
            Err(_) => "failed",
        };
        adapter(
            c,
            s,
            "bot.ack",
            json!({"delivery":work["delivery"],"lease":work["lease"],"outcome":outcome,"receipt":send.as_ref().ok(),"retry_after":60}),
        )?;
        if send.is_err() {
            break;
        }
        count += 1;
    }
    Ok(count)
}
pub(super) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    for (op, req) in [
        ("bot.qr.start", vec!["provider", "agent"]),
        ("bot.qr.check", vec!["id"]),
        ("bot.qq.connect", vec!["agent", "app_id", "app_secret"]),
        ("bot.sync", vec!["id"]),
        ("bot.disconnect", vec!["id"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":req,"properties":{"provider":{"enum":["wechat","qq"]},"dm":{"type":"boolean"},"groups":{"type":"boolean"},"verify_code":{"type":"string","pattern":"^[0-9]{4,12}$"}}}}));
    }
}
