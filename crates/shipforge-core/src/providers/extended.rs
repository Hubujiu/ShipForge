use super::{Incoming, issue_title, matching, message, timestamp_date, timestamp_ms, upstream_id};
use crate::{Result, invalid, required};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

pub(super) fn linear(p: &Value, f: &Value) -> Result<Incoming> {
    if p["type"] != "Issue"
        || !matches!(p["action"].as_str(), Some("create" | "update"))
        || !p["data"]["archivedAt"].is_null()
    {
        return Err(invalid("expected an active Linear issue create or update"));
    }
    let d = &p["data"];
    let id = upstream_id(d, "id")?;
    if f.get("team_id").is_some() {
        let team = d["teamId"]
            .as_str()
            .or_else(|| d["team"]["id"].as_str())
            .ok_or_else(|| invalid("Linear team_id scope requires a team in the event"))?;
        matching(f, "team_id", team)?;
    }
    if f.get("organization_id").is_some() {
        matching(f, "organization_id", required(p, "organizationId")?)?;
    }
    let closed = match d["state"]["type"].as_str() {
        Some("completed" | "canceled" | "duplicate") => true,
        Some("backlog" | "triage" | "unstarted" | "started") => false,
        None if d.get("completedAt").is_some() && d.get("canceledAt").is_some() => {
            !d["completedAt"].is_null() || !d["canceledAt"].is_null()
        }
        _ => {
            return Err(invalid(
                "Linear issue requires its workflow state or completion and cancellation fields",
            ));
        }
    };
    let description = if d["description"].is_null() {
        String::new()
    } else {
        d["description"]
            .as_str()
            .ok_or_else(|| invalid("expected Linear Markdown description"))?
            .to_owned()
    };
    Ok(Incoming {
        object: format!("linear:{id}"),
        delivery: None,
        micros: timestamp_date(required(d, "updatedAt")?)?,
        title: issue_title(&d["title"])?,
        description,
        closed,
    })
}
pub(super) fn asana(p: &Value, f: &Value) -> Result<Incoming> {
    // Asana's compact webhook doesn't carry a complete task. The authorized relay pairs a single
    // event with the GET /tasks/{gid} response, or submits the complete native task response.
    let task = if let Some(events) = p.get("events") {
        let events = events.as_array().filter(|a| a.len() == 1).ok_or_else(|| {
            invalid("split Asana event batches and attach each task's GET response")
        })?;
        let e = &events[0];
        if e["resource"]["resource_type"] != "task"
            || !matches!(
                e["action"].as_str(),
                Some("added" | "changed" | "undeleted")
            )
        {
            return Err(invalid(
                "expected an Asana task added, changed or undeleted event",
            ));
        }
        let task = &p["task"]["data"];
        if task["gid"].as_str() != Some(required(&e["resource"], "gid")?) {
            return Err(invalid(
                "Asana webhook and fetched task must have the same gid",
            ));
        }
        task
    } else {
        &p["data"]
    };
    if task["resource_type"] != "task" {
        return Err(invalid("expected a complete Asana task response"));
    }
    let gid = upstream_id(task, "gid")?;
    if f.get("workspace_id").is_some() {
        matching(f, "workspace_id", required(&task["workspace"], "gid")?)?;
    }
    if let Some(project) = f.get("project_id") {
        let project = project
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("project_id must be a string"))?;
        if !task["memberships"]
            .as_array()
            .is_some_and(|ms| ms.iter().any(|m| m["project"]["gid"] == project))
        {
            return Err(invalid("Asana task is outside the configured project_id"));
        }
    }
    let notes = task["notes"]
        .as_str()
        .ok_or_else(|| invalid("include notes in the Asana task response"))?;
    let completed = task["completed"]
        .as_bool()
        .ok_or_else(|| invalid("include completed in the Asana task response"))?;
    Ok(Incoming {
        object: format!("asana:{gid}"),
        delivery: None,
        micros: timestamp_date(required(task, "modified_at")?)?,
        title: issue_title(&task["name"])?,
        description: notes.to_owned(),
        closed: completed,
    })
}
fn opaque<'a>(obj: &'a Value, key: &str) -> Result<&'a str> {
    let s = required(obj, key)?;
    if s.len() > 512 || s.chars().any(char::is_control) {
        return Err(invalid(&format!("invalid opaque identifier {key}")));
    }
    Ok(s)
}
// Teams and DingTalk identifiers may themselves contain JSON, ':', '/', '+', '=' or '%'.
fn component(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            result.push(char::from(byte));
        } else {
            write!(result, "%{byte:02X}").expect("write to string");
        }
    }
    result
}
fn bounded_delivery(value: &str) -> String {
    if value.len() <= 256 {
        return value.to_owned();
    }
    let mut out = String::from("sha256:");
    for b in Sha256::digest(value.as_bytes()) {
        write!(out, "{b:02x}").expect("write to string");
    }
    out
}
pub(super) fn teams(p: &Value, f: &Value) -> Result<Incoming> {
    let sender = &p["from"];
    if p["type"] != "message"
        || p["channelId"] != "msteams"
        || sender["id"] == p["recipient"]["id"]
        || (!sender["role"].is_null() && sender["role"] != "user")
        || (sender["role"].is_null() && !sender["aadObjectId"].is_string())
    {
        return Err(invalid("expected a Teams Bot Framework user message"));
    }
    opaque(sender, "id")?;
    let conversation = opaque(&p["conversation"], "id")?;
    let id = opaque(p, "id")?;
    matching(f, "chat_id", conversation)?;
    if f.get("tenant_id").is_some() {
        let tenant = p["channelData"]["tenant"]["id"]
            .as_str()
            .or_else(|| p["conversation"]["tenantId"].as_str())
            .ok_or_else(|| invalid("Teams tenant_id scope requires a tenant"))?;
        matching(f, "tenant_id", tenant)?;
    }
    let external = format!("teams:{}:{}", component(conversation), component(id));
    message(
        bounded_delivery(&external),
        external,
        timestamp_date(required(p, "timestamp")?)?,
        required(p, "text")?,
    )
}
pub(super) fn discord(p: &Value, f: &Value) -> Result<Incoming> {
    let d = &p["d"];
    if p["op"] != 0
        || p["t"] != "MESSAGE_CREATE"
        || !matches!(d["type"].as_i64(), Some(0 | 19))
        || d["author"]["bot"] == true
        || d["author"]["system"] == true
        || !d["webhook_id"].is_null()
    {
        return Err(invalid("expected a Discord user MESSAGE_CREATE dispatch"));
    }
    upstream_id(&d["author"], "id")?;
    let channel = upstream_id(d, "channel_id")?;
    let id = upstream_id(d, "id")?;
    matching(f, "channel_id", &channel)?;
    if f.get("guild_id").is_some() {
        matching(f, "guild_id", &upstream_id(d, "guild_id")?)?;
    }
    message(
        id.clone(),
        format!("discord:{channel}:{id}"),
        timestamp_date(required(d, "timestamp")?)?,
        required(d, "content")?,
    )
}
pub(super) fn wecom(p: &Value, f: &Value) -> Result<Incoming> {
    // The relay verifies the signature, decrypts the XML, then sends its original field names.
    if p["MsgType"] != "text" || p.get("Encrypt").is_some() || required(p, "FromUserName")? == "sys"
    {
        return Err(invalid(
            "expected a decrypted WeCom application user text message",
        ));
    }
    let corp = upstream_id(p, "ToUserName")?;
    let app = upstream_id(p, "AgentID")?;
    let msg = upstream_id(p, "MsgId")?;
    matching(f, "corp_id", &corp)?;
    matching(f, "app_id", &app)?;
    let seconds = p["CreateTime"]
        .as_i64()
        .or_else(|| p["CreateTime"].as_str()?.parse().ok())
        .filter(|n| *n >= 0);
    let time = seconds
        .and_then(|n| n.checked_mul(1_000_000))
        .ok_or_else(|| invalid("expected WeCom CreateTime seconds"))?;
    let external = format!("wecom:{corp}:{app}:{msg}");
    message(
        bounded_delivery(&external),
        external,
        time,
        required(p, "Content")?,
    )
}
pub(super) fn dingtalk(p: &Value, f: &Value) -> Result<Incoming> {
    let parsed;
    let d = if p.get("specVersion").is_some() {
        if p["specVersion"] != "1.0"
            || p["type"] != "CALLBACK"
            || p["headers"]["topic"] != "/v1.0/im/bot/messages/get"
        {
            return Err(invalid("expected a DingTalk robot message Stream callback"));
        }
        parsed = serde_json::from_str::<Value>(required(p, "data")?)
            .map_err(|_| invalid("invalid DingTalk Stream data JSON"))?;
        &parsed
    } else {
        p
    };
    if d["msgtype"] != "text"
        || !matches!(d["conversationType"].as_str(), Some("1" | "2"))
        || opaque(d, "senderId")? == d["chatbotUserId"].as_str().unwrap_or("")
    {
        return Err(invalid("expected a verified DingTalk user text message"));
    }
    let id = opaque(d, "msgId")?;
    let chat = opaque(d, "conversationId")?;
    matching(f, "chat_id", chat)?;
    if f.get("corp_id").is_some() {
        matching(f, "corp_id", opaque(d, "chatbotCorpId")?)?;
    }
    message(
        bounded_delivery(id),
        format!("dingtalk:{}:{}", component(chat), component(id)),
        timestamp_ms(&d["createAt"])?,
        required(&d["text"], "content")?,
    )
}
