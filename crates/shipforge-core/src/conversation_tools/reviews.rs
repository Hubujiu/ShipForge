use super::*;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS task_reviews(space TEXT NOT NULL,task TEXT NOT NULL,room TEXT NOT NULL,run TEXT NOT NULL,PRIMARY KEY(space,task,room));
        CREATE TABLE IF NOT EXISTS conversation_completions(space TEXT NOT NULL,room TEXT NOT NULL,finished INTEGER NOT NULL,PRIMARY KEY(space,room));").map_err(storage)
}
pub(crate) fn finished(c: &Context<'_>, run: &Record, report: &Value) -> Result<()> {
    if run.fields["status"] != "done"
        || run.fields["parent_id"].is_string()
        || report["needs_review"] == false
    {
        return Ok(());
    }
    let Some(task) = run.fields["task_id"].as_str() else {
        return Ok(());
    };
    c.db.execute("INSERT INTO task_reviews(space,task,room,run) VALUES(?,?,?,?) ON CONFLICT(space,task,room) DO UPDATE SET run=excluded.run",params![c.space,task,run.fields["chat_id"].as_str().unwrap_or(""),run.id]).map_err(storage)?;
    Ok(())
}
pub(crate) fn completed(c: &Context<'_>, room: &str) -> Result<bool> {
    c.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversation_completions WHERE space=? AND room=?)",
        params![c.space, room],
        |r| r.get(0),
    )
    .map_err(storage)
}
pub(crate) fn new_message(c: &Context<'_>, room: &str) -> Result<()> {
    c.db.execute(
        "DELETE FROM conversation_completions WHERE space=? AND room=?",
        params![c.space, room],
    )
    .map_err(storage)?;
    Ok(())
}
fn pending(c: &Context<'_>, task: &str) -> Result<Vec<String>> {
    let mut stmt =
        c.db.prepare_cached("SELECT run FROM task_reviews WHERE space=? AND task=? ORDER BY room")
            .map_err(storage)?;
    let mut seen = Vec::new();
    for run in stmt
        .query_map(params![c.space, task], |r| r.get::<_, String>(0))
        .map_err(storage)?
    {
        let run = run.map_err(storage)?;
        if c.readable("run", &run).is_ok() {
            seen.push(run);
        }
    }
    Ok(seen)
}
pub(crate) fn awaiting(c: &Context<'_>, id: &str) -> Result<bool> {
    Ok(!pending(c, id)?.is_empty())
}
pub(crate) fn decorate(c: &Context<'_>, record: &mut Record) -> Result<()> {
    let waiting = pending(c, &record.id)?;
    record.fields["awaiting_review"] = json!(!waiting.is_empty());
    Ok(())
}
pub(crate) fn forget(c: &Context<'_>, kind: &str, id: &str) -> Result<()> {
    if kind == "task" {
        c.db.execute(
            "DELETE FROM task_reviews WHERE space=? AND task=?",
            params![c.space, id],
        )
        .map_err(storage)?;
    }
    if kind == "chat" {
        new_message(c, id)?;
        c.db.execute(
            "DELETE FROM task_reviews WHERE space=? AND room=?",
            params![c.space, id],
        )
        .map_err(storage)?;
    }
    Ok(())
}
pub(super) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op == "chat.complete" {
        c.require_edit()?;
        let room = c.readable("chat", required(q, "id")?)?;
        crate::chat::can_edit(c, &room)?;
        crate::domain::check_rev(q, &room)?;
        if room.fields["chat_type"] == "group"
            || !(room.fields["handler_id"].is_string() || room.fields["agent"].is_string())
        {
            return Err(invalid(
                "completion applies to an Agent or crew conversation",
            ));
        }
        let task = room.fields["task"]
            .as_str()
            .or(room.fields["task_id"].as_str());
        if let Some(id) = task {
            let r = c.readable("task", id)?;
            let status = crate::tasks::target_status(c, &r, "done")?;
            if r.fields["status"] != status {
                crate::domain::write(
                    c,
                    &json!({"op":"update","kind":"task","id":id,"fields":{"status":status},"expected_rev":r.rev}),
                )?;
            }
            c.db.execute(
                "DELETE FROM task_reviews WHERE space=? AND task=? AND room=?",
                params![c.space, id, room.id],
            )
            .map_err(storage)?;
        }
        c.db.execute(
            "INSERT OR IGNORE INTO conversation_completions(space,room,finished) VALUES(?,?,?)",
            params![c.space, room.id, now()],
        )
        .map_err(storage)?;
        return Ok(json!({"chat":room.id,"task":task,"completed":true}));
    }
    let task = c.readable("task", required(q, "id")?)?;
    let visible = pending(c, &task.id)?;
    if op == "task.review.state" {
        return Ok(json!({"task":task.id,"awaiting_review":!visible.is_empty(),"runs":visible}));
    }
    c.require_edit()?;
    if q.get("expected_rev").is_some() {
        crate::domain::check_rev(q, &task)?;
    }
    let selected = if let Some(val) = q.get("run") {
        let run = val.as_str().ok_or_else(|| invalid("run must be an ID"))?;
        if !visible.iter().any(|v| v == run) {
            return Err(conflict("that review is no longer pending or visible"));
        }
        vec![run.to_string()]
    } else {
        visible
    };
    let text = q
        .get("body")
        .map_or(Ok("已审阅，没问题"), |_| required(q, "body"))?
        .trim();
    if text.is_empty() || text.len() > 262144 {
        return Err(invalid("supply a review reply under 256 KiB"));
    }
    let comment = crate::domain::write(
        c,
        &json!({"op":"create","kind":"comment","title":text.chars().take(80).collect::<String>(),"fields":{"task":task.id,"body":text}}),
    )?;
    let reviewed = if text == "已审阅，没问题" {
        for run in &selected {
            c.db.execute(
                "DELETE FROM task_reviews WHERE space=? AND task=? AND run=?",
                params![c.space, task.id, run],
            )
            .map_err(storage)?;
        }
        selected
    } else {
        vec![]
    };
    Ok(
        json!({"task":task.id,"comment":comment["id"],"cleared":!reviewed.is_empty(),"reviewed":reviewed,"status":task.fields["status"]}),
    )
}
