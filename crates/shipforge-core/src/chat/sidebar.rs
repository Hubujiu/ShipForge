use super::*;
use crate::{as_value, now};

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS chat_sidebar(space TEXT NOT NULL,chat TEXT NOT NULL,actor TEXT NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0, undo TEXT, prior_pin INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(space,chat,actor));
        CREATE INDEX IF NOT EXISTS chat_hidden ON chat_sidebar(space,chat) WHERE undo IS NOT NULL;
        CREATE INDEX IF NOT EXISTS chat_message_activity ON records(space,json_extract(fields,'$.chat'),created_at DESC,id) WHERE kind='message';").map_err(storage)
}
fn state(c: &Context<'_>, id: &str) -> Result<(i64, Option<String>, i64)> {
    c.db.query_row(
        "SELECT pinned,undo,prior_pin FROM chat_sidebar WHERE space=? AND chat=? AND actor=?",
        params![c.space, id, c.actor],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(storage)
    .map(Option::unwrap_or_default)
}
pub(super) fn organize(c: &Context<'_>, room: &Record, q: &Value) -> Result<Value> {
    let (pinned, undo, prior) = state(c, &room.id)?;
    let (pin, token, old) = match required(q, "action")? {
        "pin" if undo.is_none() => (if pinned > 0 { pinned } else { now() }, None, 0),
        "unpin" if undo.is_none() => (0, None, 0),
        "hide" if undo.is_some() => (0, undo, prior),
        "hide" => (0, Some(uuid::Uuid::new_v4().to_string()), pinned),
        "undo" if undo.as_deref() == Some(required(q, "undo_token")?) => (prior, None, 0),
        "undo" => return Err(conflict("the hide action is no longer current")),
        _ => {
            return Err(invalid(
                "choose pin, unpin, hide or undo on an available sidebar row",
            ));
        }
    };
    c.db.execute("INSERT INTO chat_sidebar(space,chat,actor,pinned,undo,prior_pin) VALUES(?,?,?,?,?,?)
        ON CONFLICT(space,chat,actor) DO UPDATE SET pinned=excluded.pinned,undo=excluded.undo,prior_pin=excluded.prior_pin",
        params![c.space,room.id,c.actor,pin,token,old]).map_err(storage)?;
    Ok(json!({"id":room.id,"pinned":pin>0,"hidden":token.is_some(),"undo_token":token}))
}
pub(crate) fn new_message(c: &Context<'_>, message: &Record) -> Result<()> {
    let chat = required(&message.fields, "chat")?;
    crate::conversation_tools::reviews::new_message(c, chat)?;
    let mut stmt =
        c.db.prepare_cached(
            "SELECT actor FROM chat_sidebar WHERE space=? AND chat=? AND undo IS NOT NULL",
        )
        .map_err(storage)?;
    let actors = stmt
        .query_map(params![c.space, chat], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for actor in actors {
        let reader = Context {
            actor: &actor,
            ..*c
        };
        if reader.role().is_ok() && reader.visible(message)? {
            c.db.execute("UPDATE chat_sidebar SET undo=NULL,prior_pin=0 WHERE space=? AND chat=? AND actor=?",params![c.space,chat,actor]).map_err(storage)?;
        }
    }
    Ok(())
}
fn task_participant(c: &Context<'_>, room: &Record) -> Result<bool> {
    if !room.fields["task"].is_string() && !room.fields["task_id"].is_string() {
        return Ok(true);
    }
    if room.fields["origin_kind"] != "automation" && super::owner(c, room)?.0 == c.actor {
        return Ok(true);
    }
    c.db.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE space=? AND kind='message'
        AND json_extract(fields,'$.chat')=? AND coalesce(json_extract(fields,'$.author_kind'),'human')='human'
        AND coalesce(json_extract(fields,'$.inherited'),0)=0 AND coalesce(json_extract(fields,'$.author'),json_extract(fields,'$.creator'))=?)",
        params![c.space,room.id,c.actor],|r|r.get(0)).map_err(storage)
}
pub(super) fn list(c: &Context<'_>) -> Result<Value> {
    let mut result = Vec::new();
    let mut activity=c.db.prepare_cached("SELECT id,created_at FROM records WHERE space=? AND kind='message' AND json_extract(fields,'$.chat')=? ORDER BY created_at DESC,id").map_err(storage)?;
    for room in c.records("chat")? {
        if !c.visible(&room)? {
            continue;
        }
        let (pin, hidden, _) = state(c, &room.id)?;
        if hidden.is_some() || (pin == 0 && !task_participant(c, &room)?) {
            continue;
        }
        let mut at = room.created_at;
        let candidates = activity
            .query_map(params![c.space, room.id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(storage)?;
        for candidate in candidates {
            let (id, time) = candidate.map_err(storage)?;
            if c.visible(&c.get("message", &id)?)? {
                at = time;
                break;
            }
        }
        let id = room.id.clone();
        let mut row = as_value(room);
        row["fields"]["sidebar"] = json!({"pinned":pin>0,"last_activity":at});
        result.push((std::cmp::Reverse(pin), std::cmp::Reverse(at), id, row));
    }
    result.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    Ok(Value::Array(
        result.into_iter().map(|item| item.3).collect(),
    ))
}
