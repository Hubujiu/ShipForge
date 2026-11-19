use super::require_owner;
use crate::{Context, Record, Result, domain, forbidden, invalid, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS chat_shares(
        share TEXT PRIMARY KEY, space TEXT NOT NULL, chat TEXT NOT NULL, organization TEXT NOT NULL,
        scope TEXT NOT NULL, selection TEXT NOT NULL, snapshot TEXT NOT NULL, created INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS chat_share_selection ON chat_shares(space,chat,selection);").map_err(storage)
}

fn missing() -> String {
    "not_found: conversation share unavailable".to_owned()
}

/// A printable version of the selected messages. Markdown and line breaks remain intact.
pub fn share_text(snapshot: &Value) -> String {
    let mut sections = vec![snapshot["title"].as_str().unwrap_or_default().to_owned()];
    for msg in snapshot["messages"].as_array().into_iter().flatten() {
        let author = msg["author"].as_str().unwrap_or_default();
        let body = msg["body"].as_str().unwrap_or_default();
        let mut block = format!("{author}\n{body}");
        for name in msg["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            block.push('\n');
            block.push_str(name);
        }
        sections.push(block);
    }
    sections.join("\n\n")
}

pub(super) fn list(c: &Context<'_>, room: &Record) -> Result<Value> {
    let mut stmt=c.db.prepare("SELECT share,scope,created,selection FROM chat_shares WHERE space=? AND chat=? ORDER BY created DESC,rowid DESC").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, room.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(storage)?;
    let mut result = vec![];
    for row in rows {
        let (share, scope, created, selection) = row.map_err(storage)?;
        let ids: Vec<String> = serde_json::from_str(&selection).map_err(storage)?;
        result.push(json!({"share":share,"path":format!("/share/{share}"),"scope":scope,"created_at":created,"message_ids":ids}));
    }
    Ok(json!(result))
}

pub(super) fn create(c: &Context<'_>, room: &Record, q: &Value) -> Result<Value> {
    require_owner(c, room)?;
    let scope = match q.get("scope") {
        None => "organization",
        Some(v) => v
            .as_str()
            .filter(|s| matches!(*s, "organization" | "link"))
            .ok_or_else(|| invalid("scope must be organization or link"))?,
    };
    let ids = domain::strings(&q["message_ids"], "message_ids")?;
    let selection = ids.iter().copied().collect::<BTreeSet<_>>();
    if ids.is_empty() || ids.len() > 100 || ids.len() != selection.len() {
        return Err(invalid("select 1 to 100 distinct messages"));
    }
    let mut included = selection.clone();
    let mut parent_ids = Vec::new();
    if q["include_questions"] == true {
        for id in &ids {
            let message = c.readable("message", id)?;
            if message.fields["chat"] == room.id
                && let Some(run) = message.fields["run"].as_str()
            {
                let run = c.readable("run", run)?;
                if let Some(origin) = run.fields["chat_message"].as_str() {
                    parent_ids.push(origin.to_owned());
                }
            }
        }
    }
    included.extend(parent_ids.iter().map(String::as_str));
    let mut total_image_bytes = 0usize;
    let mut messages = Vec::new();
    for id in included {
        let message = c.readable("message", id)?;
        super::may_share_inherited(c, id)?;
        let f = &message.fields;
        if f["chat"] != room.id {
            return Err(invalid(
                "every selected message must belong to this conversation",
            ));
        }
        if let Some(run) = f["run"].as_str() {
            let run = c.readable("run", run)?;
            if run.fields["chat_id"] != room.id || run.fields["status"] != "done" {
                return Err(invalid("only completed replies can be shared"));
            }
        }
        let author = if f["author_kind"] == "agent" {
            let id = f["author"].as_str().unwrap_or("");
            c.readable("agent", id)
                .map_or_else(|_| "Agent".to_owned(), |r| r.title)
        } else {
            let id = f["creator"].as_str().unwrap_or("");
            c.find("member", id)?
                .map_or_else(|| "成员".to_owned(), |r| r.title)
        };
        let body = f["body"].as_str().unwrap_or(&message.title);
        let filenames: Vec<_> = f["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["filename"].as_str())
            .collect();
        let seq:i64=c.db.query_row("SELECT coalesce(max(seq),0) FROM event_log WHERE space=? AND kind='message' AND id=? AND action='create' AND rev<=?",params![c.space,id,message.rev],|r|r.get(0)).map_err(storage)?;
        let mut images = Vec::new();
        for file in f["attachments"].as_array().into_iter().flatten() {
            let Some(key) = file["id"].as_str() else {
                continue;
            };
            let bytes: Option<Vec<u8>> =
                c.db.query_row(
                    "SELECT content FROM attachments WHERE space=? AND id=? AND (
                        substr(content,1,8)=X'89504E470D0A1A0A'
                        OR substr(content,1,3)=X'FFD8FF'
                        OR substr(content,1,6) IN (X'474946383761',X'474946383961')
                        OR (substr(content,1,4)=X'52494646' AND substr(content,9,4)=X'57454250'))",
                    params![c.space, key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            if let Some(image) = bytes.as_deref().and_then(|b| {
                super::share_image::thumbnail(b, file["filename"].as_str().unwrap_or("image"))
            }) {
                total_image_bytes += image["png"].as_str().map_or(0, str::len);
                if total_image_bytes > 768 * 1024 {
                    return Err(invalid("select fewer image attachments for one share"));
                }
                images.push(image);
            }
        }
        let mut selected = json!({"author":author,"body":body,"attachments":filenames});
        if !images.is_empty() {
            selected["images"] = json!(images);
        }
        messages.push((seq, selected));
    }
    messages.sort_by_key(|(seq, _)| *seq);
    let messages = messages.into_iter().map(|(_, m)| m).collect::<Vec<_>>();
    let snapshot = json!({"title":room.title,"messages":messages}).to_string();
    if snapshot.len() > 1024 * 1024 {
        return Err(invalid("conversation share exceeds 1 MiB"));
    }
    let share = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let selection = serde_json::to_string(&selection).map_err(storage)?;
    c.db.execute("INSERT INTO chat_shares(share,space,chat,organization,scope,selection,snapshot,created) VALUES(?,?,?,?,?,?,?,unixepoch())",
        params![share,c.space,room.id,c.org_id()?,scope,selection,snapshot]).map_err(storage)?;
    Ok(json!({"share":share,"scope":scope,"path":format!("/share/{share}")}))
}

pub(crate) fn read(db: &Connection, key: &str, actor: Option<&str>) -> Result<Value> {
    if key.len() != 64
        || !key
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(missing());
    }
    let (scope, org, snapshot): (String, String, String) = db
        .query_row(
            "SELECT scope,organization,snapshot FROM chat_shares WHERE share=?",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(missing)?;
    if scope == "organization" {
        let actor =
            actor.ok_or_else(|| "unauthorized: organization member sign-in required".to_owned())?;
        let org = Context {
            db,
            actor,
            space: &org,
        };
        if org.find("member", actor)?.is_none() {
            return Err(forbidden("organization membership required"));
        }
    }
    let mut snapshot: Value = serde_json::from_str(&snapshot).map_err(storage)?;
    snapshot["scope"] = json!(scope);
    Ok(snapshot)
}

pub(super) fn revoke(c: &Context<'_>, room: &Record, key: &str) -> Result<Value> {
    require_owner(c, room)?;
    let selection: String =
        c.db.query_row(
            "SELECT selection FROM chat_shares WHERE space=? AND chat=? AND share=?",
            params![c.space, room.id, key],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(missing)?;
    let count =
        c.db.execute(
            "DELETE FROM chat_shares WHERE space=? AND chat=? AND selection=?",
            params![c.space, room.id, selection],
        )
        .map_err(storage)?;
    Ok(json!({"id":room.id,"revoked":count}))
}

pub(super) fn extend_schema(rules: &mut Vec<Value>) {
    rules.push(json!({"if":{"properties":{"op":{"const":"chat.share.create"}}},"then":{"required":["id","message_ids"],"properties":{"scope":{"enum":["organization","link"]},"message_ids":{"type":"array","minItems":1,"maxItems":100,"uniqueItems":true,"items":{"type":"string","minLength":1}}}}}));
    rules.push(json!({"if":{"properties":{"op":{"enum":["chat.share.read","chat.share.revoke"]}}},"then":{"required":["share"],"properties":{"share":{"type":"string","pattern":"^[0-9a-f]{64}$"}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"chat.share.revoke"}}},"then":{"required":["id"]}}));
}
