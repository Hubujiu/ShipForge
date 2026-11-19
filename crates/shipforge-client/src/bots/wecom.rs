//! WeCom intelligent-bot API mode; the official WebSocket is authenticated by BotID/Secret.
use super::*;
use chrono::Utc;
mod media;
mod socket;
pub(super) use socket::close;
const FIXTURE: &str = "SHIPFORGE_WECOM_LOOPBACK";
pub(super) fn bind(c: &mut Client, v: &vault::Vault, q: &Value, space: &str) -> R<Value> {
    let id = Uuid::new_v4().to_string();
    let account = field(q, "bot_id")?;
    let secret = field(q, "secret")?;
    if account.len() > 512 || secret.len() > 1024 {
        return Err("invalid: invalid WeCom BotID or Secret".into());
    }
    let flag = |k: &str| {
        q.get(k).map_or(Ok(true), |v| {
            v.as_bool()
                .ok_or_else(|| format!("invalid: {k} must be boolean"))
        })
    };
    let mut s = json!({"id":id,"space":space,"provider":"wecom","account":account,"secret":secret,"agent":field(q,"agent")?,"key":STANDARD.encode(rand_bytes::<32>()?),"dm":flag("dm")?,"groups":flag("groups")?,"status":"verifying"});
    adapter(
        c,
        &s,
        "bot.wecom.reserve",
        json!({"account":account,"agent":s["agent"]}),
    )?;
    let mut result = (|| {
        socket::session(&s, v, |_| Ok(()))?;
        let binding = connect(c, v, &mut s)?;
        // The gateway may send waiting callbacks as soon as it acknowledges the subscription.
        // Buffer these in the same encrypted queue the observer will drain after handover.
        let _ = socket::session(&s, v, |g| g.collect(v, &s));
        Ok(binding)
    })();
    if result.is_err() {
        let _ = adapter(c, &s, "bot.wecom.release", json!({}));
        if v.load(&id).is_ok() {
            // The attach may have committed even when its reply or subsequent local write failed.
            // Keep the sealed profile if the team cannot currently confirm the outcome.
            if let Ok(list) =
                c.execute(json!({"op":"bot.bindings","space":space,"allow_cached":false}))
            {
                if let Some(binding) = list["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|b| b["id"] == id && b["state"] == "connected")
                {
                    result = Ok(binding.clone());
                } else {
                    v.remove(&id)?;
                }
            }
        } else {
            v.remove(&id)?;
        }
    }
    // Transfer ownership to the observer, which may be a different process.
    close(&id);
    result
}
#[derive(Default)]
pub(super) struct InputIssues {
    pub(super) rejected: usize,
    pub(super) waiting_media: usize,
}
pub(super) fn observer_cycle(v: &vault::Vault, id: &str, previous: Option<&str>) -> R<Value> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        let state = v.load(id)?;
        if let Some(current) = state["wecom_cycle"].as_str()
            && Some(current) != previous
        {
            let mut result = state["wecom_result"].clone();
            result["background"] = json!(true);
            return Ok(result);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(json!({"id":id,"state":"receiving","background":true}));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
pub(super) fn poll(
    c: &mut Client,
    v: &vault::Vault,
    s: &Value,
    issues: &mut InputIssues,
) -> R<usize> {
    socket::session(s, v, |g| g.collect(v, s))?;
    let mut received = 0;
    let id = field(s, "id")?;
    for (key, buffered) in v.wecom_pending(id)? {
        let frame = &buffered["frame"];
        let b = &frame["body"];
        let group = b["chattype"] == "group";
        if b["aibotid"] != s["account"]
            || !matches!(b["chattype"].as_str(), Some("single" | "group"))
            || s["active_binding"][if group { "groups" } else { "dm" }] != true
        {
            v.wecom_complete(id, &key)?;
            continue;
        }
        let sender = field(&b["from"], "userid")?;
        let peer = if group { field(b, "chatid")? } else { sender };
        let parsed = match media::read(b) {
            Ok(v) => v,
            Err(e)
                if e.starts_with("network:")
                    && Utc::now().timestamp() - buffered["at"].as_i64().unwrap_or(0) < 300 =>
            {
                issues.waiting_media += 1;
                continue;
            }
            Err(_) => {
                issues.rejected += 1;
                v.wecom_complete(id, &key)?;
                continue;
            }
        };
        if let Some((text, files)) = parsed {
            let result = adapter(
                c,
                s,
                "bot.wecom.event",
                json!({"bot_id":b["aibotid"],"chattype":b["chattype"],"chatid":b["chatid"],"userid":sender,"msgid":b["msgid"],"body":text,"files":files}),
            );
            let out = match result {
                Err(e)
                    if e.starts_with("invalid:")
                        || e.contains("external message ID already identifies another payload") =>
                {
                    issues.rejected += 1;
                    v.wecom_complete(id, &key)?;
                    continue;
                }
                other => other?,
            };
            if out["duplicate"] != true && out["turn"].is_string() {
                if let Some(req) = frame["headers"]["req_id"]
                    .as_str()
                    .filter(|r| r.len() <= 512 && !r.chars().any(char::is_control))
                {
                    let context = json!({"req_id":req,"received":Utc::now().timestamp()});
                    v.context(id, peer, field(b, "msgid")?, &context.to_string())?;
                }
                received += 1;
            }
        }
        v.wecom_complete(id, &key)?;
    }
    Ok(received)
}
pub(super) fn send(c: &mut Client, v: &vault::Vault, s: &Value, w: &Value) -> R<String> {
    let peer = field(w, "peer")?;
    let delivery = field(w, "delivery")?;
    let context = v
        .reply(field(s, "id")?, peer, field(w, "remote")?)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);
    let recent = context["received"]
        .as_i64()
        .is_some_and(|t| Utc::now().timestamp().saturating_sub(t) < 180);
    let req = context["req_id"]
        .as_str()
        .filter(|_| recent && w["part"] == 1);
    socket::session(s, v, |g| {
        let text = w["payload"]["text"].as_str();
        let mut payload = if let Some(content) = text {
            if req.is_some() {
                json!({"msgtype":"stream","stream":{"id":delivery,"finish":true,"content":content}})
            } else {
                json!({"msgtype":"markdown","markdown":{"content":content}})
            }
        } else {
            media::upload(c, v, s, g, &w["payload"])?
        };
        let cmd = if req.is_some() {
            "aibot_respond_msg"
        } else {
            payload["chatid"] = json!(peer);
            "aibot_send_msg"
        };
        let ack = g.command(v, s, cmd, req, payload)?;
        Ok(field(&ack["headers"], "req_id")?.to_owned())
    })
}
