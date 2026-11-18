use crate::{Result, invalid, required};
use serde_json::{Value, json};

pub(crate) struct Origin {
    pub provider: String,
    pub workspace: String,
    pub channel: String,
    pub message: String,
    pub root: String,
    pub author: Option<String>,
}

impl Origin {
    pub(crate) fn route(&self) -> Value {
        json!({"provider":self.provider,"workspace":self.workspace,"channel":self.channel,"thread":self.root})
    }

    pub(super) fn message_key(&self, id: &str) -> String {
        json!([self.provider, self.workspace, self.channel, id]).to_string()
    }
}

fn id(value: &Value, key: &str) -> Result<String> {
    let value = &value[key];
    let text = if let Some(n) = value.as_i64() {
        n.to_string()
    } else {
        value
            .as_str()
            .ok_or_else(|| invalid(&format!("missing {key}")))?
            .to_owned()
    };
    if text.trim().is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
        return Err(invalid(&format!("invalid {key}")));
    }
    Ok(text)
}

fn optional(value: &Value, key: &str) -> Result<Option<String>> {
    if value[key].is_null() {
        Ok(None)
    } else {
        id(value, key).map(Some)
    }
}

pub(crate) fn slack_ts(value: &str) -> bool {
    value.split_once('.').is_some_and(|(s, f)| {
        !s.is_empty()
            && s.len() <= 13
            && !f.is_empty()
            && f.len() <= 6
            && s.bytes().chain(f.bytes()).all(|b| b.is_ascii_digit())
    })
}

pub(crate) fn origin(provider: &str, p: &Value) -> Result<Option<Origin>> {
    let (workspace, channel, message, root, author) = match provider {
        "slack" => {
            let m = &p["event"];
            let ts = id(m, "ts")?;
            let root = optional(m, "thread_ts")?;
            if !slack_ts(&ts) || root.as_deref().is_some_and(|s| !slack_ts(s)) {
                return Err(invalid("invalid Slack thread timestamp"));
            }
            (
                id(p, "team_id")?,
                id(m, "channel")?,
                ts,
                root,
                Some(id(m, "user")?),
            )
        }
        "telegram" => {
            let m = &p["message"];
            let chat = id(&m["chat"], "id")?;
            let parent = &m["reply_to_message"];
            if !parent["chat"].is_null() && id(&parent["chat"], "id")? != chat {
                return Err(invalid("reply belongs to another Telegram chat"));
            }
            (
                String::new(),
                chat,
                id(m, "message_id")?,
                optional(m, "message_thread_id")?.or(optional(parent, "message_id")?),
                optional(&m["from"], "id")?,
            )
        }
        "lark" | "feishu" => {
            let e = &p["event"];
            let m = &e["message"];
            let parent = |key: &str| {
                if m[key].as_str() == Some("") {
                    Ok(None)
                } else {
                    optional(m, key)
                }
            };
            (
                optional(&p["header"], "tenant_key")?.unwrap_or_default(),
                id(m, "chat_id")?,
                id(m, "message_id")?,
                parent("root_id")?.or(parent("parent_id")?),
                optional(&e["sender"]["sender_id"], "open_id")?,
            )
        }
        "teams" => (
            optional(&p["channelData"]["tenant"], "id")?
                .or(optional(&p["conversation"], "tenantId")?)
                .unwrap_or_default(),
            id(&p["conversation"], "id")?,
            id(p, "id")?,
            optional(p, "replyToId")?,
            Some(id(&p["from"], "id")?),
        ),
        "discord" => {
            let d = &p["d"];
            let reference = &d["message_reference"];
            let channel = id(d, "channel_id")?;
            let guild = optional(d, "guild_id")?.unwrap_or_default();
            if !reference.is_null()
                && (optional(reference, "channel_id")?.is_some_and(|v| v != channel)
                    || optional(reference, "guild_id")?.is_some_and(|v| v != guild))
            {
                return Err(invalid("reply belongs to another Discord channel or guild"));
            }
            (
                guild,
                channel,
                id(d, "id")?,
                if d["type"] == 19 {
                    Some(id(reference, "message_id")?)
                } else {
                    None
                },
                Some(id(&d["author"], "id")?),
            )
        }
        _ => return Ok(None),
    };
    Ok(Some(Origin {
        provider: provider.into(),
        workspace,
        channel,
        root: root.unwrap_or_else(|| message.clone()),
        message,
        author,
    }))
}

pub(super) fn validate_route(provider: &str, route: &Value) -> Result<()> {
    if required(route, "provider")? != provider || route["workspace"].as_str().is_none() {
        return Err(invalid("thread does not match the connector provider"));
    }
    for key in ["channel", "thread"] {
        id(route, key)?;
    }
    Ok(())
}
