//! Convert authenticated relay payloads to the same issue synchronization contract used by the CLI.
mod extended;
use crate::{Context, Result, invalid, required, worker};
use chrono::{DateTime, NaiveDateTime};
pub(crate) use extended::component as teams_component;
use serde_json::{Value, json};
pub(crate) const NAMES: &[&str] = &[
    "github", "gitlab", "jira", "feishu", "lark", "slack", "telegram", "linear", "asana", "teams",
    "discord", "wecom", "dingtalk",
];

pub(crate) fn execute(c: &Context<'_>, q: &Value, preview: bool) -> Result<Value> {
    let id = required(q, "id")?;
    let connector = c.readable("connector", id)?;
    if !preview {
        crate::vcs::forge::require_legacy(c, id)?;
        c.require_admin()?;
    }
    if connector.fields["enabled"] == false {
        return Err(invalid("connector is disabled"));
    }
    let provider = required(q, "provider")?;
    if connector.fields["provider"].as_str() != Some(provider) {
        return Err(invalid("provider does not match the configured connector"));
    }
    let p = &q["payload"];
    if !p.is_object() {
        return Err(invalid("payload must be an event object"));
    }
    let f = &connector.fields;
    let assistant = crate::task_assistants::route(c, id)?;
    if assistant.is_none() && provider == "slack" && p["event"]["type"] == "app_mention" {
        return Ok(json!({"action":"ignored","reason":"no task assistant"}));
    }
    if provider == "github"
        || (provider == "gitlab"
            && matches!(p["object_kind"].as_str(), Some("push" | "merge_request")))
    {
        return crate::vcs::ingest(c, &connector, q, preview);
    }
    let event = match provider {
        "gitlab" => gitlab(p, f)?,
        "jira" => jira(p, f)?,
        "slack" => slack(p, f)?,
        "telegram" => telegram(p, f)?,
        "feishu" | "lark" => feishu(p, f, assistant.is_some())?,
        "linear" => extended::linear(p, f)?,
        "asana" => extended::asana(p, f)?,
        "teams" => extended::teams(p, f)?,
        "discord" => extended::discord(p, f)?,
        "wecom" => extended::wecom(p, f)?,
        "dingtalk" => extended::dingtalk(p, f)?,
        _ => return Err(invalid("no inbound parser for this provider")),
    };
    let supplied = q
        .get("event_id")
        .map(|_| required(q, "event_id"))
        .transpose()?;
    if let (Some(upstream), Some(supplied)) = (&event.delivery, supplied)
        && upstream != supplied
    {
        return Err(invalid("event_id must match the upstream delivery id"));
    }
    let delivery = event
        .delivery
        .as_deref()
        .or(supplied)
        .ok_or_else(|| invalid("event_id is required from the upstream delivery header"))?;
    if delivery.len() > 256 || delivery.chars().any(char::is_control) {
        return Err(invalid("invalid upstream event_id"));
    }
    if event.description.len() > 1_048_576 {
        return Err(invalid("external description exceeds 1 MiB"));
    }
    if matches!(provider, "linear" | "asana") && crate::vendors::uses_project(f) {
        return crate::vendors::project_relay(c, &connector, p, delivery, preview);
    }
    let mut mapped = json!({"id":id,"provider":provider,"external_id":event.object,"event_id":delivery,"source_version":format!("{:020}",event.micros),"title":event.title,"description":event.description,"closed":event.closed});
    if provider == "jira"
        && let Some(value) = p["issue"]["fields"].get("priority")
        && let Some(level) = crate::jira::webhook_priority(value, &f["mapping"]["priority"])
    {
        mapped["priority"] = json!(level);
    }
    crate::task_commands::source_members::provider(c, provider, p, f, &mut mapped)?;
    let origin = crate::im::origin(provider, p)?;
    if let Some(origin) = &origin {
        mapped["thread"] = origin.route();
    }
    if let Some(binding) = assistant {
        return crate::task_assistants::receive(
            c, &connector, &binding, p, &mapped, origin, preview,
        );
    }
    if preview {
        return Ok(mapped);
    }
    if let Some(choice) = q.get("resolve") {
        if !matches!(choice.as_str(), Some("local" | "source")) {
            return Err(invalid("resolve must be local or source"));
        }
        mapped["resolve"] = choice.clone();
    }
    mapped["op"] = json!("connector.ingest");
    if let Some(origin) = origin {
        return crate::im::receive(c, &connector, &mapped, origin);
    }
    worker::dispatch(c, &mapped).ok_or_else(|| invalid("connector ingestion unavailable"))?
}

struct Incoming {
    delivery: Option<String>,
    object: String,
    micros: i64,
    title: String,
    description: String,
    closed: bool,
}
fn message(delivery: String, object: String, micros: i64, text: &str) -> Result<Incoming> {
    let title = text
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .ok_or_else(|| invalid("message is empty"))?;
    Ok(Incoming {
        delivery: Some(delivery),
        object,
        micros,
        title: title.chars().take(160).collect(),
        description: text.to_owned(),
        closed: false,
    })
}
fn issue_title(value: &Value) -> Result<String> {
    let text = value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 500)
        .ok_or_else(|| invalid("issue title must have 1–500 characters"))?;
    Ok(text.to_owned())
}
fn upstream_id(v: &Value, key: &str) -> Result<String> {
    let out = if let Some(id) = v[key].as_i64() {
        id.to_string()
    } else if let Some(id) = v[key].as_u64() {
        id.to_string()
    } else {
        required(v, key)?.to_owned()
    };
    if out.len() > 160
        || !out
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(invalid(&format!("invalid identifier {key}")));
    }
    Ok(out)
}
fn matching(config: &Value, key: &str, actual: &str) -> Result<()> {
    if config.get(key).is_some_and(|v| v.as_str() != Some(actual)) {
        return Err(invalid(&format!("event is outside the configured {key}")));
    }
    Ok(())
}
fn timestamp_ms(value: &Value) -> Result<i64> {
    let n = value.as_i64().or_else(|| value.as_str()?.parse().ok());
    n.filter(|n| *n >= 0)
        .and_then(|n| n.checked_mul(1000))
        .ok_or_else(|| invalid("expected a Unix timestamp in milliseconds"))
}
fn timestamp_date(s: &str) -> Result<i64> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .map(|v| v.timestamp_micros())
        .or_else(|_| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f UTC")
                .map(|v| v.and_utc().timestamp_micros())
        })
        .map_err(|_| invalid("unsupported issue timestamp"))
        .and_then(|v| {
            if v >= 0 {
                Ok(v)
            } else {
                Err(invalid("issue timestamp precedes 1970"))
            }
        })
}
fn gitlab(p: &Value, f: &Value) -> Result<Incoming> {
    if p["object_kind"] != "issue" {
        return Err(invalid("expected a GitLab issue event"));
    }
    let a = &p["object_attributes"];
    if a.get("description").is_none() {
        return Err(invalid("GitLab issue payload is missing description"));
    }
    if a["confidential"] == true
        || !matches!(
            a["action"].as_str(),
            Some("open" | "reopen" | "update" | "close")
        )
    {
        return Err(invalid(
            "unsupported GitLab issue action or confidential issue",
        ));
    }
    let project = upstream_id(&p["project"], "id")?;
    matching(f, "project_id", &project)?;
    if f.get("repository").is_some() {
        matching(
            f,
            "repository",
            required(&p["project"], "path_with_namespace")?,
        )?;
    }
    let issue = upstream_id(a, "iid")?;
    let closed = match required(a, "state")? {
        "closed" => true,
        "opened" => false,
        _ => return Err(invalid("unknown GitLab issue state")),
    };
    let description = if a["description"].is_null() {
        String::new()
    } else {
        a["description"]
            .as_str()
            .ok_or_else(|| invalid("expected a GitLab description"))?
            .to_owned()
    };
    Ok(Incoming {
        delivery: None,
        object: format!("gitlab:{project}:{issue}"),
        micros: timestamp_date(required(a, "updated_at")?)?,
        title: issue_title(&a["title"])?,
        description,
        closed,
    })
}
fn jira(p: &Value, f: &Value) -> Result<Incoming> {
    if !matches!(
        p["webhookEvent"].as_str(),
        Some("jira:issue_created" | "jira:issue_updated")
    ) {
        return Err(invalid(
            "expected a Jira issue_created or issue_updated event",
        ));
    }
    let issue = &p["issue"];
    let fields = &issue["fields"];
    if fields.get("description").is_none() {
        return Err(invalid("Jira issue payload is missing description"));
    }
    if !fields["security"].is_null() {
        return Err(invalid(
            "issue-level security requires an explicitly mapped relay",
        ));
    }
    let id = upstream_id(issue, "id")?;
    if f.get("project_key").is_some() {
        matching(f, "project_key", required(&fields["project"], "key")?)?;
    }
    let version = if let Some(s) = fields["updated"].as_str() {
        timestamp_date(s)?
    } else {
        timestamp_ms(&p["timestamp"])?
    };
    let mut description = String::new();
    match &fields["description"] {
        Value::Null => (),
        Value::String(v) => description.push_str(v),
        v if v["type"] == "doc" => {
            adf_text(v, &mut description, 0)?;
            description.truncate(description.trim_end_matches('\n').len());
        }
        _ => return Err(invalid("unsupported Jira description")),
    }
    let closed = match required(&fields["status"]["statusCategory"], "key")? {
        "done" => true,
        "new" | "indeterminate" => false,
        _ => return Err(invalid("unknown Jira status category")),
    };
    Ok(Incoming {
        delivery: None,
        object: format!("jira:{id}"),
        micros: version,
        title: issue_title(&fields["summary"])?,
        description,
        closed,
    })
}
fn adf_text(node: &Value, out: &mut String, depth: usize) -> Result<()> {
    if depth > 48 || out.len() > 1_048_576 {
        return Err(invalid("Jira description is too large or deeply nested"));
    }
    let kind = required(node, "type")?;
    match kind {
        "text" => out.push_str(
            node["text"]
                .as_str()
                .ok_or_else(|| invalid("invalid Jira text node"))?,
        ),
        "hardBreak" => out.push('\n'),
        "mention" | "emoji" => {
            if let Some(text) = node["attrs"]["text"]
                .as_str()
                .or_else(|| node["attrs"]["shortName"].as_str())
            {
                out.push_str(text);
            }
        }
        "inlineCard" => {
            if let Some(text) = node["attrs"]["url"].as_str() {
                out.push_str(text);
            }
        }
        _ => {
            if let Some(children) = node.get("content") {
                for child in children
                    .as_array()
                    .ok_or_else(|| invalid("invalid Jira content array"))?
                {
                    adf_text(child, out, depth + 1)?;
                }
            }
            if matches!(kind, "paragraph" | "heading" | "codeBlock") {
                out.push('\n');
            }
        }
    }
    Ok(())
}
fn slack(p: &Value, f: &Value) -> Result<Incoming> {
    let e = &p["event"];
    if p["type"] != "event_callback"
        || !matches!(e["type"].as_str(), Some("message" | "app_mention"))
        || !e["subtype"].is_null()
        || !e["bot_id"].is_null()
        || !e["bot_profile"].is_null()
    {
        return Err(invalid("expected an ordinary Slack message"));
    }
    upstream_id(e, "user")?;
    let team = upstream_id(p, "team_id")?;
    let channel = upstream_id(e, "channel")?;
    matching(f, "workspace_id", &team)?;
    matching(f, "channel_id", &channel)?;
    let ts = required(e, "ts")?;
    let (sec, frac) = ts
        .split_once('.')
        .ok_or_else(|| invalid("invalid Slack message timestamp"))?;
    if sec.is_empty()
        || !sec.bytes().all(|b| b.is_ascii_digit())
        || frac.is_empty()
        || frac.len() > 6
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid("invalid Slack message timestamp"));
    }
    let micros = sec
        .parse::<i64>()
        .ok()
        .and_then(|n| n.checked_mul(1_000_000))
        .and_then(|n| n.checked_add(frac.parse::<i64>().ok()? * 10_i64.pow(6 - frac.len() as u32)))
        .ok_or_else(|| invalid("invalid Slack message timestamp"))?;
    message(
        upstream_id(p, "event_id")?,
        format!("slack:{team}:{channel}:{ts}"),
        micros,
        required(e, "text")?,
    )
}
fn telegram(p: &Value, f: &Value) -> Result<Incoming> {
    let m = &p["message"];
    if m["from"]["is_bot"] != false
        || !m["sender_chat"].is_null()
        || !matches!(
            m["chat"]["type"].as_str(),
            Some("private" | "group" | "supergroup")
        )
    {
        return Err(invalid("expected an ordinary Telegram user message"));
    }
    let chat = upstream_id(&m["chat"], "id")?;
    matching(f, "chat_id", &chat)?;
    let msg = upstream_id(m, "message_id")?;
    let micros = m["date"]
        .as_i64()
        .filter(|n| *n >= 0)
        .and_then(|n| n.checked_mul(1_000_000))
        .ok_or_else(|| invalid("invalid Telegram message date"))?;
    message(
        upstream_id(p, "update_id")?,
        format!("telegram:{chat}:{msg}"),
        micros,
        required(m, "text")?,
    )
}
fn feishu(p: &Value, f: &Value, assistant: bool) -> Result<Incoming> {
    let h = &p["header"];
    let e = &p["event"];
    let m = &e["message"];
    if p["schema"] != "2.0"
        || h["event_type"] != "im.message.receive_v1"
        || e["sender"]["sender_type"] != "user"
        || !(m["message_type"] == "text" || assistant && m["message_type"] == "post")
    {
        return Err(invalid("expected a Feishu v2 user text message"));
    }
    let chat = upstream_id(m, "chat_id")?;
    matching(f, "chat_id", &chat)?;
    let msg = upstream_id(m, "message_id")?;
    let body: Value = serde_json::from_str(required(m, "content")?)
        .map_err(|_| invalid("invalid Feishu message content"))?;
    let micros = timestamp_ms(&m["create_time"])?;
    let text = if m["message_type"] == "post" {
        crate::task_assistants::post(&body)?
    } else {
        required(&body, "text")?.to_owned()
    };
    message(
        upstream_id(h, "event_id")?,
        format!("feishu:{chat}:{msg}"),
        micros,
        &text,
    )
}
