use super::*;
use std::collections::HashSet;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS task_shortcuts(space TEXT NOT NULL,id TEXT NOT NULL,definition TEXT NOT NULL,PRIMARY KEY(space,id));
        CREATE TABLE IF NOT EXISTS shortcut_stars(space TEXT NOT NULL,actor TEXT NOT NULL,id TEXT NOT NULL,rank INTEGER NOT NULL,PRIMARY KEY(space,actor,id));
        CREATE TABLE IF NOT EXISTS shortcut_sessions(seq INTEGER PRIMARY KEY,space TEXT NOT NULL,actor TEXT NOT NULL,task TEXT NOT NULL,shortcut TEXT NOT NULL,chat TEXT NOT NULL,UNIQUE(space,chat));").map_err(storage)
}
fn definition(c: &Context<'_>, id: &str) -> Result<Value> {
    let raw: Option<String> =
        c.db.query_row(
            "SELECT definition FROM task_shortcuts WHERE space=? AND id=?",
            params![c.space, id],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let item: Value = serde_json::from_str(&raw.ok_or_else(|| "not_found: shortcut".to_owned())?)
        .map_err(storage)?;
    if item["visibility"] != "space" && item["owner"] != c.actor {
        return Err("not_found: shortcut".into());
    }
    Ok(item)
}
fn rank(c: &Context<'_>, id: &str) -> Result<()> {
    c.db.execute("INSERT INTO shortcut_stars(space,actor,id,rank) SELECT ?1,?2,?3,COALESCE(MIN(rank),0)-1 FROM shortcut_stars WHERE space=?1 AND actor=?2 ON CONFLICT(space,actor,id) DO NOTHING",params![c.space,c.actor,id]).map_err(storage)?;
    Ok(())
}
fn catalog(c: &Context<'_>) -> Result<Value> {
    let mut stmt=c.db.prepare("SELECT s.definition,f.rank FROM task_shortcuts s LEFT JOIN shortcut_stars f ON f.space=s.space AND f.id=s.id AND f.actor=?2 WHERE s.space=?1 AND (json_extract(s.definition,'$.owner')=?2 OR json_extract(s.definition,'$.visibility')='space') ORDER BY f.rank IS NULL,f.rank,json_extract(s.definition,'$.title'),s.id").map_err(storage)?;
    let mut items = Vec::new();
    for row in stmt
        .query_map(params![c.space, c.actor], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .map_err(storage)?
    {
        let (raw, rank) = row.map_err(storage)?;
        let mut d: Value = serde_json::from_str(&raw).map_err(storage)?;
        d["pinned"] = json!(rank.is_some());
        d["can_edit"] = json!(d["owner"] == c.actor);
        d["can_run"] = json!(
            crate::agents::usable(
                c,
                required(&d, "handler_kind")?,
                required(&d, "handler_id")?,
                None
            )
            .is_ok()
                && c.require_edit().is_ok()
        );
        items.push(d);
    }
    Ok(json!(items))
}
fn save(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let (id, old) = match q.get("id") {
        None => (uuid::Uuid::new_v4().to_string(), None),
        Some(_) => {
            let id = required(q, "id")?;
            (id.into(), Some(definition(c, id)?))
        }
    };
    if let Some(old) = &old {
        if old["owner"] != c.actor {
            return Err(forbidden("only the shortcut creator can edit it"));
        }
        if q.get("expected_rev").is_some_and(|v| *v != old["rev"]) {
            return Err(conflict("shortcut changed"));
        }
    }
    let mut item=old.clone().unwrap_or_else(||json!({"owner":c.actor,"id":id,"visibility":"private","prompt":"","handler_kind":"agent"}));
    for k in [
        "title",
        "prompt",
        "handler_kind",
        "handler_id",
        "visibility",
    ] {
        if let Some(v) = q.get(k) {
            item[k] = v.clone();
        }
    }
    if required(&item, "title")?.trim().is_empty()
        || required(&item, "title")?.chars().count() > 100
        || !item["prompt"].as_str().is_some_and(|s| s.len() <= 65536)
    {
        return Err(invalid("supply a short shortcut name and prompt"));
    }
    if !matches!(item["visibility"].as_str(), Some("space" | "private"))
        || !matches!(item["handler_kind"].as_str(), Some("agent" | "crew"))
    {
        return Err(invalid("invalid shortcut visibility or handler"));
    }
    let kind = required(&item, "handler_kind")?;
    let handler = required(&item, "handler_id")?;
    crate::agents::usable(c, kind, handler, None)?;
    if item["visibility"] == "space" && c.readable(kind, handler)?.fields["visibility"] == "private"
    {
        return Err(invalid("a shared shortcut requires a shared handler"));
    }
    item["rev"] = json!(old.as_ref().and_then(|s| s["rev"].as_i64()).unwrap_or(0) + 1);
    c.db.execute("INSERT INTO task_shortcuts(space,id,definition) VALUES(?,?,?) ON CONFLICT(space,id) DO UPDATE SET definition=excluded.definition",params![c.space,id,item.to_string()]).map_err(storage)?;
    if old.is_none() {
        rank(c, &id)?;
    }
    Ok(item)
}
fn organize(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let id = required(q, "id")?;
    definition(c, id)?;
    if let Some(pinned) = q.get("pinned") {
        match pinned.as_bool() {
            Some(true) => rank(c, id)?,
            Some(false) => {
                c.db.execute(
                    "DELETE FROM shortcut_stars WHERE space=? AND actor=? AND id=?",
                    params![c.space, c.actor, id],
                )
                .map_err(storage)?;
            }
            None => return Err(invalid("pinned must be boolean")),
        }
    }
    if let Some(v) = q.get("order") {
        let ids = crate::domain::strings(v, "order")?;
        let mut seen = HashSet::new();
        let current = catalog(c)?;
        let existing: HashSet<_> = current
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["pinned"] == true)
            .filter_map(|i| i["id"].as_str())
            .collect();
        if ids.len() != existing.len()
            || ids
                .iter()
                .any(|id| !seen.insert(*id) || !existing.contains(id))
        {
            return Err(conflict("provide every current visible favorite once"));
        }
        for (i, id) in ids.into_iter().enumerate() {
            c.db.execute(
                "UPDATE shortcut_stars SET rank=? WHERE space=? AND actor=? AND id=?",
                params![i as i64, c.space, c.actor, id],
            )
            .map_err(storage)?;
        }
    }
    Ok(json!({"id":id,"organized":true}))
}
fn history(c: &Context<'_>, q: &Value) -> Result<Value> {
    let task = c.readable("task", required(q, "task")?)?;
    let mut stmt=c.db.prepare("SELECT shortcut,chat FROM shortcut_sessions WHERE space=? AND actor=? AND task=? ORDER BY seq DESC").map_err(storage)?;
    let mut sessions = Vec::new();
    for row in stmt
        .query_map(params![c.space, c.actor, task.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?
    {
        let (shortcut, chat) = row.map_err(storage)?;
        if let Ok(room) = c.readable("chat", &chat) {
            sessions.push(json!({"shortcut":shortcut,"chat":chat,"title":room.title,"created_at":room.created_at}));
        }
    }
    Ok(json!({"selected":sessions.first(),"sessions":sessions}))
}
fn run(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let task = c.readable("task", required(q, "task")?)?;
    let cmd = definition(c, required(q, "id")?)?;
    let kind = required(&cmd, "handler_kind")?;
    let handler = required(&cmd, "handler_id")?;
    if super::slash::catalog(c, kind, handler, "")?["online"] != true {
        return Err(conflict("shortcut runtime is offline"));
    }
    let mut f =
        json!({"task":task.id,"handler_kind":kind,"handler_id":handler,"visibility":"private"});
    if kind == "agent" {
        f["agent"] = json!(handler);
    }
    let chat = crate::domain::write(
        c,
        &json!({"op":"create","kind":"chat","title":format!("{} · {}",task.title,cmd["title"].as_str().unwrap_or("")),"fields":f}),
    )?;
    let id = required(&chat, "id")?;
    let prompt = cmd["prompt"].as_str().unwrap_or("").trim();
    let fallback = format!(
        "{} · {}",
        task.fields["number"].as_str().unwrap_or(&task.id),
        task.title
    );
    let body = if prompt.is_empty() { &fallback } else { prompt };
    let result = crate::chat::execute(c, &json!({"id":id,"body":body}), "chat.send")?;
    c.db.execute(
        "INSERT INTO shortcut_sessions(space,actor,task,shortcut,chat) VALUES(?,?,?,?,?)",
        params![c.space, c.actor, task.id, cmd["id"].as_str(), id],
    )
    .map_err(storage)?;
    Ok(
        json!({"id":cmd["id"],"task":task.id,"chat":id,"message":result["message"],"run":result["run"]}),
    )
}
pub(super) fn check_receipt(c: &Context<'_>, q: &Value, v: &Value) -> Result<()> {
    let d = definition(c, required(v, "id")?)?;
    if q["op"] == "shortcut.save" && d["owner"] != c.actor {
        return Err(forbidden("shortcut unavailable"));
    }
    Ok(())
}
pub(super) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    match op {
        "shortcut.catalog" => catalog(c),
        "shortcut.save" => save(c, q),
        "shortcut.organize" => organize(c, q),
        "shortcut.history" => history(c, q),
        "shortcut.run" => run(c, q),
        "shortcut.delete" => {
            c.require_edit()?;
            let id = required(q, "id")?;
            let d = definition(c, id)?;
            if d["owner"] != c.actor {
                return Err(forbidden("only the shortcut creator can delete it"));
            }
            if q.get("expected_rev").is_some_and(|r| *r != d["rev"]) {
                return Err(conflict("shortcut changed"));
            }
            c.db.execute(
                "DELETE FROM task_shortcuts WHERE space=? AND id=?",
                params![c.space, id],
            )
            .map_err(storage)?;
            c.db.execute(
                "DELETE FROM shortcut_stars WHERE space=? AND id=?",
                params![c.space, id],
            )
            .map_err(storage)?;
            Ok(json!({"id":id,"owner":c.actor,"deleted":true}))
        }
        _ => Err(invalid("unknown shortcut operation")),
    }
}
