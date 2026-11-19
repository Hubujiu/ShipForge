use super::*;

const LIMIT: usize = 4 * 1024 * 1024;
fn filename(raw: &str) -> String {
    let s = raw
        .chars()
        .map(|ch| {
            if ch.is_control() || "/\\:<>|?*\"".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .take(180)
        .collect::<String>();
    if s.trim_matches([' ', '.']).is_empty() {
        "attachment.bin".into()
    } else {
        s
    }
}
struct Resource {
    key: String,
    name: String,
    kind: &'static str,
}
impl Resource {
    fn new(node: &Value, kind: &'static str) -> Option<Self> {
        let key = node[if kind == "image" {
            "image_key"
        } else {
            "file_key"
        }]
        .as_str()?;
        let name = node["file_name"].as_str().map(filename).unwrap_or_else(|| {
            format!(
                "{}.{}",
                filename(key),
                match kind {
                    "image" => "png",
                    "media" => "mp4",
                    "audio" => "ogg",
                    _ => "bin",
                }
            )
        });
        Some(Self {
            key: key.into(),
            kind: if kind == "image" { "image" } else { "file" },
            name,
        })
    }
    fn read(&self, s: &mut Value, message: &str, left: usize) -> R<(Value, usize)> {
        let key = component(&self.key)?;
        let route = format!(
            "/open-apis/im/v1/messages/{message}/resources/{key}?type={}",
            self.kind
        );
        let mut res = net::http()
            .get(url(field(s, "provider")?, &route)?)
            .header("Authorization", format!("Bearer {}", token(s)?))
            .call()
            .map_err(|_| "network: resource download interrupted")?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("provider: resource HTTP {status}"));
        }
        let mime = res
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("application/octet-stream")
            .to_owned();
        if mime == "application/json" {
            return Err("provider: resource unavailable".into());
        }
        let bytes = res
            .body_mut()
            .with_config()
            .limit(left as u64 + 1)
            .read_to_vec()
            .map_err(|_| "provider: resource unreadable or over 4 MiB")?;
        if bytes.len() > left {
            return Err("provider: resource over 4 MiB".into());
        }
        Ok((
            json!({"filename":self.name,"media_type":mime,"content_base64":STANDARD.encode(&bytes)}),
            bytes.len(),
        ))
    }
}
fn content(node: &Value) -> Value {
    node.as_str()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| node.clone())
}
fn post_node(n: &Value, files: &mut Vec<Resource>) -> String {
    match n["tag"].as_str().unwrap_or_default() {
        "a" => format!(
            "{} {}",
            n["text"].as_str().unwrap_or(""),
            n["href"].as_str().unwrap_or("")
        ),
        "at" if n["user_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("@_user_")) =>
        {
            n["user_id"].as_str().unwrap_or("").into()
        }
        "at" => format!(
            "@{}",
            n["user_name"]
                .as_str()
                .or(n["user_id"].as_str())
                .unwrap_or("成员")
        ),
        "code_block" => format!(
            "~~~{}\n{}\n~~~",
            n["language"].as_str().unwrap_or(""),
            n["text"].as_str().unwrap_or("")
        ),
        "img" | "media" => {
            if let Some(f) = Resource::new(n, if n["tag"] == "img" { "image" } else { "media" }) {
                let name = f.name.clone();
                files.push(f);
                format!("[{name}]")
            } else {
                String::new()
            }
        }
        _ => n["text"]
            .as_str()
            .or(n["content"].as_str())
            .unwrap_or("")
            .into(),
    }
}
fn card_text(n: &Value, parts: &mut Vec<String>, files: &mut Vec<Resource>, depth: usize) {
    if depth > 16 || parts.len() >= 128 || files.len() > 6 {
        return;
    }
    match n {
        Value::Object(o) => {
            for key in ["title", "text", "content", "url", "href"] {
                if let Some(v) = o.get(key) {
                    if let Some(s) = v.as_str() {
                        parts.push(s.to_owned());
                    } else {
                        card_text(v, parts, files, depth + 1);
                    }
                }
            }
            if let Some(k) = o.get("img_key").and_then(Value::as_str)
                && let Some(f) = Resource::new(&json!({"image_key":k}), "image")
            {
                files.push(f);
            }
            for key in ["header", "body", "elements", "fields", "actions", "columns"] {
                if let Some(v) = o.get(key) {
                    card_text(v, parts, files, depth + 1);
                }
            }
        }
        Value::Array(a) => {
            for child in a {
                card_text(child, parts, files, depth + 1);
            }
        }
        _ => {}
    }
}
fn readable(kind: &str, data: &Value, files: &mut Vec<Resource>) -> String {
    match kind {
        "text" => data["text"].as_str().unwrap_or("").into(),
        "post" => {
            let post = if data["content"].is_array() {
                data
            } else {
                data.get("zh_cn")
                    .or(data.get("en_us"))
                    .or_else(|| data.as_object()?.values().next())
                    .unwrap_or(data)
            };
            let mut lines = Vec::new();
            if let Some(title) = post["title"].as_str().filter(|s| !s.is_empty()) {
                lines.push(title.into());
            }
            for line in post["content"].as_array().into_iter().flatten() {
                lines.push(
                    line.as_array()
                        .into_iter()
                        .flatten()
                        .map(|n| post_node(n, files))
                        .collect::<String>(),
                );
            }
            lines.join("\n")
        }
        "image" | "file" | "audio" | "media" => {
            let Some(f) = Resource::new(
                data,
                match kind {
                    "image" => "image",
                    "audio" => "audio",
                    "media" => "media",
                    _ => "file",
                },
            ) else {
                return String::new();
            };
            let name = f.name.clone();
            files.push(f);
            format!("[{name}]")
        }
        "interactive" => {
            let mut parts = Vec::new();
            card_text(data, &mut parts, files, 0);
            parts.join("\n")
        }
        _ => String::new(),
    }
}
fn topic_group(s: &mut Value, channel: &str) -> bool {
    if let Some(topic) = s["chat_modes"][channel].as_bool() {
        return topic;
    }
    let info = api(s, &format!("/open-apis/im/v1/chats/{channel}"), None);
    if let Some(mode) = info
        .ok()
        .and_then(|d| d["data"]["chat_mode"].as_str().map(str::to_owned))
    {
        if s["chat_modes"].as_object().is_some_and(|m| m.len() >= 256) {
            s["chat_modes"] = json!({});
        }
        let topic = mode == "topic";
        s["chat_modes"][channel] = json!(topic);
        return topic;
    }
    false
}
fn mark(v: &vault::Vault, s: &mut Value, source: &str) {
    if let Ok(out) = api(
        s,
        &format!("/open-apis/im/v1/messages/{source}/reactions"),
        Some(json!({"reaction_type":{"emoji_type":"OnIt"}})),
    ) && let Ok(rid) = field(&out["data"], "reaction_id")
    {
        let _ = v.context(s["id"].as_str().unwrap_or(""), "working", source, rid);
    }
}
fn unmark(v: &vault::Vault, s: &mut Value, source: &str) {
    let id = s["id"].as_str().unwrap_or("").to_owned();
    if let Ok(rid) = v.reply(&id, "working", source)
        && let Ok(rid) = component(&rid)
        && let Ok(u) = url(
            s["provider"].as_str().unwrap_or(""),
            &format!("/open-apis/im/v1/messages/{source}/reactions/{rid}"),
        )
        && let Ok(t) = token(s)
        && let Ok(res) = net::http()
            .delete(u)
            .header("Authorization", format!("Bearer {t}"))
            .call()
        && res.status().is_success()
    {
        let _ = v.context(&id, "working", source, "");
    }
}
pub(super) fn receive(
    c: &mut Client,
    v: &vault::Vault,
    s: &mut Value,
    envelope: &Value,
) -> R<bool> {
    let head = &envelope["header"];
    let e = &envelope["event"];
    if head["app_id"] != s["account"]
        || s["verified_tenant"]
            .as_str()
            .is_some_and(|t| head["tenant_key"] != t)
    {
        return Ok(false);
    }
    let Some(kind) = head["event_type"].as_str() else {
        return Ok(false);
    };
    let reaction = kind == "im.message.reaction.created_v1";
    let sender = if reaction {
        if e["operator_type"] != "user" {
            return Ok(false);
        }
        e["user_id"]["open_id"].as_str()
    } else {
        if kind != "im.message.receive_v1"
            || e["sender"]["sender_type"] != "user"
            || e["sender"]["tenant_key"]
                .as_str()
                .is_some_and(|t| !t.is_empty() && head["tenant_key"] != t)
        {
            return Ok(false);
        }
        e["sender"]["sender_id"]["open_id"].as_str()
    };
    let Some(sender) = sender.filter(|s| !s.is_empty()) else {
        return Ok(false);
    };
    if s["bot_open_id"] == sender {
        return Ok(false);
    }
    let source = component(field(
        if reaction { e } else { &e["message"] },
        "message_id",
    )?)?;
    let original;
    let m = if reaction {
        let emoji = e["reaction_type"]["emoji_type"].as_str().unwrap_or("");
        let policy = &s["active_binding"]["settings"];
        if ![
            policy["trigger_emoji"].as_str().unwrap_or("THUMBSUP"),
            policy["stop_emoji"].as_str().unwrap_or("CrossMark"),
        ]
        .iter()
        .any(|v| v.eq_ignore_ascii_case(emoji))
        {
            return Ok(false);
        }
        original = api(s, &format!("/open-apis/im/v1/messages/{source}"), None)?;
        let Some(first) = original["data"]["items"].as_array().and_then(|a| a.first()) else {
            return Ok(false);
        };
        first
    } else {
        &e["message"]
    };
    let channel = component(field(m, "chat_id")?)?;
    let recorded = if reaction {
        v.reply(field(s, "id")?, "scene", source).ok()
    } else {
        None
    };
    let scene = match (m["chat_type"].as_str(), recorded.as_deref()) {
        (Some("p2p"), _) | (_, Some("direct")) => "direct",
        (Some("group"), _) | (_, Some("group")) => "group",
        (None, _) if reaction => {
            let info = api(s, &format!("/open-apis/im/v1/chats/{channel}"), None)?;
            match info["data"]["chat_mode"].as_str() {
                Some("p2p") => "direct",
                Some("group" | "topic") => "group",
                _ => return Ok(false),
            }
        }
        _ => return Ok(false),
    };
    let topic_group = scene == "group" && topic_group(s, channel);
    let root_id = m["root_id"].as_str().filter(|v| !v.is_empty());
    let thread = m["thread_id"].as_str().filter(|v| !v.is_empty());
    let root = if scene == "group" {
        thread
            .or(root_id)
            .or(topic_group.then_some(source))
            .unwrap_or("")
    } else {
        ""
    };
    let mut mentions_bot = false;
    let mut mentions_other = false;
    let mut resources = Vec::new();
    let mut body = readable(
        m["message_type"]
            .as_str()
            .or(m["msg_type"].as_str())
            .unwrap_or(""),
        &content(
            m.get("content_v2")
                .or(m.get("content"))
                .or(m["body"].get("content_v2"))
                .unwrap_or(&m["body"]["content"]),
        ),
        &mut resources,
    );
    for user in m["mentions"].as_array().into_iter().flatten() {
        let is_bot = user["id"]["open_id"] == s["bot_open_id"]
            || user["id"].as_str().is_some_and(|id| s["bot_open_id"] == id);
        mentions_bot |= is_bot;
        mentions_other |= !is_bot;
        if let Some(key) = user["key"].as_str().filter(|t| !t.is_empty()) {
            body = body.replace(
                key,
                &if is_bot {
                    String::new()
                } else {
                    format!("@{}", user["name"].as_str().unwrap_or("成员"))
                },
            );
        }
    }
    let followup = root_id.is_some_and(|id| id != source)
        || m["parent_id"].as_str().is_some_and(|p| !p.is_empty());
    let b = &s["active_binding"];
    let p = &b["settings"];
    let allowed = if reaction {
        p["reactions"] == true
            && e["reaction_type"]["emoji_type"].as_str().is_some_and(|x| {
                p["trigger_emoji"]
                    .as_str()
                    .is_some_and(|v| x.eq_ignore_ascii_case(v))
            })
    } else if scene == "direct" {
        b["dm"] == true
    } else if mentions_bot {
        b["groups"] == true
    } else {
        !mentions_other
            && p["plain_chats"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(channel)))
            && (!followup || p["thread_followup"] == true)
    };
    let mut files = Vec::new();
    let mut left = LIMIT;
    if allowed {
        for f in resources.iter().take(6) {
            match f.read(s, source, left) {
                Ok((file, size)) => {
                    left -= size;
                    files.push(file);
                }
                Err(e) if e.starts_with("network:") => return Err(e),
                Err(_) => body.push_str(&format!("\n[无法读取附件 {}]", f.name)),
            }
        }
    }
    let out = adapter(
        c,
        s,
        "bot.lark.event",
        json!({"app_id":head["app_id"],"tenant":field(head,"tenant_key")?,"event_id":field(head,"event_id")?,"source":source,"channel":channel,"sender":sender,"scene":scene,"root":root,"topic_group":topic_group,"topic_reply":followup,"trigger":if reaction{"reaction"}else{"message"},"reaction":e["reaction_type"]["emoji_type"],"mentions_bot":mentions_bot,"mentions_other":mentions_other,"body":body,"files":files}),
    )?;
    if out["ignored"] != "other_tenant" {
        s["verified_tenant"] = head["tenant_key"].clone();
    }
    if out["duplicate"] == true {
        return Ok(false);
    }
    if out["turn"].is_string() {
        v.context(field(s, "id")?, "scene", source, scene)?;
    }
    if out["run"].is_string() {
        mark(v, s, source);
    }
    if out["stopped"] == true {
        unmark(v, s, source);
    }
    Ok(out["turn"].is_string() || out["stopped"] == true)
}
fn upload(c: &mut Client, s: &mut Value, p: &Value) -> R<String> {
    let file=c.execute(json!({"op":"attachment.download","space":s["space"],"kind":"artifact","id":p["artifact"],"attachment_id":p["attachment"],"allow_cached":false}))?;
    let bytes = STANDARD
        .decode(
            file["content_base64"]
                .as_str()
                .ok_or("invalid: missing artifact bytes")?,
        )
        .map_err(|_| "invalid: corrupt artifact bytes")?;
    let name = filename(field(&file, "filename")?);
    let boundary = format!("ShipForge{}", Uuid::new_v4().simple());
    let mut payload=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file_type\"\r\n\r\nstream\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file_name\"\r\n\r\n{name}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    payload.extend_from_slice(&bytes);
    payload.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut reply = net::http()
        .post(url(field(s, "provider")?, "/open-apis/im/v1/files")?)
        .header("Authorization", format!("Bearer {}", token(s)?))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .send(payload.as_slice())
        .map_err(|_| "network: artifact upload interrupted")?;
    if !reply.status().is_success() {
        return Err(format!("provider: HTTP {}", reply.status().as_u16()));
    }
    let result: Value = reply
        .body_mut()
        .with_config()
        .limit(128 * 1024)
        .read_json()
        .map_err(|_| "network: invalid upload response")?;
    Ok(field(&result["data"], "file_key")?.into())
}
fn formatted(body: &str) -> (&'static str, Value) {
    let tables = body
        .lines()
        .filter(|line| {
            let line = line.trim_matches([' ', '|']);
            line.contains('|')
                && line.contains("---")
                && line.bytes().all(|b| matches!(b, b' ' | b'|' | b'-' | b':'))
        })
        .count();
    let fence = char::from(96).to_string().repeat(3);
    if tables <= 5 && (body.contains(&fence) || body.contains("~~~") || tables > 0) {
        return (
            "interactive",
            json!({"schema":"2.0","body":{"elements":[{"tag":"markdown","content":body}]}}),
        );
    }
    if tables > 0
        || ["**", "[", "> ", "- ", "1. ", "# "]
            .iter()
            .any(|m| body.contains(m))
    {
        let markdown = if tables > 5 {
            let mut lines = Vec::new();
            let mut inside = false;
            for line in body.lines() {
                let table = line.contains('|');
                if table != inside {
                    lines.push(fence.as_str());
                    inside = table;
                }
                lines.push(line);
            }
            if inside {
                lines.push(&fence);
            }
            lines.join("\n")
        } else {
            body.to_owned()
        };
        return (
            "post",
            json!({"zh_cn":{"title":"","content":[[{"tag":"md","text":markdown}]]}}),
        );
    }
    ("text", json!({"text":body}))
}
pub(in crate::bots) fn send(
    c: &mut Client,
    v: &vault::Vault,
    s: &mut Value,
    w: &Value,
) -> R<String> {
    let d = &w["destination"];
    let source = component(field(d, "source")?)?;
    let p = &w["payload"];
    let (kind, content) = if p["artifact"].is_string() {
        ("file", json!({"file_key":upload(c,s,p)?}))
    } else {
        formatted(p["text"].as_str().ok_or("invalid: missing result text")?)
    };
    let result = api(
        s,
        &format!("/open-apis/im/v1/messages/{source}/reply"),
        Some(
            json!({"msg_type":kind,"content":content.to_string(),"uuid":w["delivery"],"reply_in_thread":d["in_thread"]}),
        ),
    )?;
    let receipt = field(&result["data"], "message_id")?.to_owned();
    unmark(v, s, source);
    Ok(receipt)
}
