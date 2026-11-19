//! Device-owned official messaging adapters. The team server stores bindings and conversation work.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::rand::{SecureRandom, SystemRandom};
use uuid::Uuid;
mod gateway;
mod lark;
mod net;
mod qq;
mod vault;
mod wechat;
mod wecom;
type R<T> = Result<T, String>;
pub(crate) const LOCAL: &[&str] = &[
    "bot.qr.start",
    "bot.qr.check",
    "bot.qq.connect",
    "bot.wecom.connect",
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
    if !matches!(provider, "qq" | "wechat" | "feishu" | "lark") {
        return Err("invalid: unknown messaging provider".into());
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
    let mut s = json!({"id":id,"space":space,"provider":provider,"key":STANDARD.encode(rand_bytes::<32>()?),"agent":agent,"dm":q["dm"].as_bool().unwrap_or(true),"groups":q["groups"].as_bool().unwrap_or(provider!="wechat"),"status":"wait"});
    if provider == "wechat" && s["groups"] == true {
        return Err("invalid: personal WeChat supports direct chat".into());
    }
    if manual {
        s["account"] = json!(field(q, "app_id")?);
        s["secret"] = json!(field(q, "app_secret")?);
        qq::token(&mut s)?;
        return connect(c, v, &mut s);
    }
    if lark::branded(provider) {
        lark::start(&mut s)?;
    } else if provider == "wechat" {
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
    let mut lookup = json!({"op":"bot.bindings","space":space,"allow_cached":false});
    if op == "bot.wecom.connect" {
        lookup["available_for"] = json!("wecom");
    }
    if op == "bot.qr.start" && q["provider"].as_str().is_some_and(lark::branded) {
        lookup["available_for"] = q["provider"].clone();
    }
    let identity = c.execute(lookup)?;
    let scope = json!([identity["workspace"], identity["space"], identity["actor"]]).to_string();
    let v = vault::Vault::new(
        c.directory
            .as_deref()
            .ok_or("invalid: external accounts require a saved device profile")?,
        scope,
    )?;
    if op == "bot.wecom.connect" {
        if !identity["available_agents"]
            .as_array()
            .is_some_and(|a| a.iter().any(|r| r["id"] == q["agent"]))
        {
            return Err("conflict: selected Agent is unavailable for WeCom".into());
        }
        return wecom::bind(c, &v, q, space);
    }
    if matches!(op, "bot.qr.start" | "bot.qq.connect") {
        if q["provider"].as_str().is_some_and(lark::branded)
            && !identity["available_agents"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v["id"] == q["agent"]))
        {
            return Err("conflict: this Agent is unavailable for this cloud".into());
        }
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
        lark::gateway::close(id);
        wecom::close(id);
        return Ok(result);
    }
    if op == "bot.sync" && !binding.is_some_and(|b| b["state"] == "connected") {
        v.remove(id)?;
        gateway::close(id);
        lark::gateway::close(id);
        wecom::close(id);
        return Err("conflict: binding disconnected or replaced".into());
    }
    let wecom_sync = op == "bot.sync" && binding.is_some_and(|b| b["provider"] == "wecom");
    let previous_cycle = if wecom_sync {
        v.load(id)?["wecom_cycle"].as_str().map(str::to_owned)
    } else {
        None
    };
    let _guard = match v.lock(id) {
        Err(e) if wecom_sync && e == "conflict: another adapter is already active" => {
            return wecom::observer_cycle(&v, id, previous_cycle.as_deref());
        }
        result => result?,
    };
    let mut s = v.load(id)?;
    if op == "bot.qr.check" {
        if let Some(b) = binding {
            return Ok(b.clone());
        }
        if matches!(
            s["status"].as_str(),
            Some("expired" | "verify_code_blocked" | "binded_redirect" | "denied" | "wrong_cloud")
        ) {
            return Ok(json!({"id":id,"state":s["status"]}));
        }
        if s["status"] != "confirmed" {
            if s["provider"].as_str().is_some_and(lark::branded) {
                lark::check(&mut s)?;
            } else if s["provider"] == "wechat" {
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
        if s["provider"] == "wecom" || s["provider"].as_str().is_some_and(lark::branded) {
            s["active_binding"] = binding.cloned().unwrap_or(Value::Null);
        }
        // Establish the verified subscription before claiming WeCom work so a revoked secret cannot consume a delivery.
        let pending = if s["provider"] == "wecom" {
            0
        } else {
            deliver(c, &v, &mut s)?
        };
        let mut wecom_issues = wecom::InputIssues::default();
        let poll = if s["provider"] == "wecom" {
            wecom::poll(c, &v, &s, &mut wecom_issues)
        } else if s["provider"].as_str().is_some_and(lark::branded) {
            lark::gateway::poll(c, &v, &mut s)
        } else if s["provider"] == "wechat" {
            wechat::poll(c, &v, &mut s)
        } else {
            gateway::poll(c, &v, &mut s)
        };
        if wecom_sync
            && poll
                .as_ref()
                .is_err_and(|e| e == "conflict: another adapter is already active")
        {
            drop(_guard);
            return wecom::observer_cycle(&v, id, previous_cycle.as_deref());
        }
        if poll.as_ref().is_err_and(|e| {
            e.contains("WeChat session expired")
                || e.contains("QQ credentials rejected")
                || e.contains("Lark application revoked")
                || e.contains("WeCom authorization rejected")
                || e.contains("WeCom session replaced")
        }) {
            adapter(c, &s, "bot.expire", json!({}))?;
            v.remove(id)?;
            gateway::close(id);
            lark::gateway::close(id);
            wecom::close(id);
        }
        let incoming = poll?;
        if !wecom_sync {
            v.save(id, &s)?;
        }
        let result = deliver(c, &v, &mut s);
        if result.as_ref().is_err_and(|e| {
            e.contains("WeCom authorization rejected") || e.contains("WeCom session replaced")
        }) {
            adapter(c, &s, "bot.expire", json!({}))?;
            v.remove(id)?;
            wecom::close(id);
        }
        let outgoing = result?;
        let mut result =
            json!({"id":id,"received":incoming,"outgoing":pending+outgoing,"state":"connected"});
        if wecom_sync {
            result["rejected"] = json!(wecom_issues.rejected);
            result["waiting_media"] = json!(wecom_issues.waiting_media);
            s["wecom_result"] = result.clone();
            s["wecom_cycle"] = json!(Uuid::new_v4().to_string());
            v.save(id, &s)?;
        }
        Ok(result)
    }
}
fn deliver(c: &mut Client, v: &vault::Vault, s: &mut Value) -> R<usize> {
    let mut count = 0;
    for _ in 0..8 {
        let work = adapter(c, s, "bot.claim", json!({}))?;
        if work.is_null() {
            break;
        }
        let send = if s["provider"] == "wecom" {
            wecom::send(c, v, s, &work)
        } else if s["provider"].as_str().is_some_and(lark::branded) {
            lark::messages::send(c, v, s, &work)
        } else if s["provider"] == "wechat" {
            wechat::send(c, v, s, &work)
        } else {
            qq::send(s, &work)
        };
        let outcome = match &send {
            Ok(_) => "sent",
            Err(e)
                if e.contains("HTTP 429")
                    || matches!(
                        e.as_str(),
                        "provider: WeCom temporarily unavailable" | "provider: WeCom rate limit"
                    ) =>
            {
                "retry"
            }
            Err(e) if e.starts_with("network:") || e.contains("HTTP 5") => "uncertain",
            Err(_) => "failed",
        };
        adapter(
            c,
            s,
            "bot.ack",
            json!({"delivery":work["delivery"],"lease":work["lease"],"outcome":outcome,"receipt":send.as_ref().ok(),"retry_after":60}),
        )?;
        if let Err(e) = &send
            && (e.contains("WeCom authorization rejected") || e.contains("WeCom session replaced"))
        {
            return Err(e.clone());
        }
        if send.is_err() {
            break;
        }
        count += 1;
    }
    Ok(count)
}
pub(super) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"const":"bot.wecom.connect"}}},"then":{"required":["agent","bot_id","secret"],"properties":{"agent":{"type":"string","minLength":1},"bot_id":{"type":"string","minLength":1,"maxLength":512},"secret":{"type":"string","minLength":1,"maxLength":1024},"dm":{"type":"boolean"},"groups":{"type":"boolean"}}}}));
    for (op, req) in [
        ("bot.qr.start", vec!["provider", "agent"]),
        ("bot.qr.check", vec!["id"]),
        ("bot.qq.connect", vec!["agent", "app_id", "app_secret"]),
        ("bot.sync", vec!["id"]),
        ("bot.disconnect", vec!["id"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":req,"properties":{"provider":{"enum":["wechat","qq","feishu","lark"]},"dm":{"type":"boolean"},"groups":{"type":"boolean"},"verify_code":{"type":"string","pattern":"^[0-9]{4,12}$"}}}}));
    }
}
