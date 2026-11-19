use crate::{
    Context, Record, Result, as_value, conflict, domain, invalid, required, storage, tasks, worker,
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
pub(crate) mod assignees;
pub(crate) mod source_members;

pub(crate) fn conversation(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let task = c.readable("task", required(q, "id")?)?;
    let kind = required(&task.fields, "assignee_kind")?;
    if !matches!(kind, "agent" | "crew") {
        return Err(invalid("assign this task to an agent or crew first"));
    }
    let handler = required(&task.fields, "assignee")?;
    conversation_for(c, &task, kind, handler)
}
pub(crate) fn conversation_for(
    c: &Context<'_>,
    task: &Record,
    kind: &str,
    handler: &str,
) -> Result<Value> {
    c.readable(kind, handler)?;
    let key = params![c.space, c.actor, task.id, kind, handler];
    let chat_id:Option<String>=c.db.query_row("SELECT chat FROM task_conversations WHERE space=?1 AND actor=?2 AND task=?3 AND kind=?4 AND handler=?5",key,|r|r.get(0)).optional().map_err(storage)?;
    if let Some(id) = chat_id
        && let Some(chat) = c.find("chat", &id)?
    {
        return Ok(as_value(c.readable("chat", &chat.id)?));
    }
    let mut fields =
        json!({"visibility":"private","task":task.id,"handler_kind":kind,"handler_id":handler});
    if kind == "agent" {
        fields["agent"] = json!(handler);
    }
    let result = domain::write(
        c,
        &json!({"op":"create","kind":"chat","title":task.title,"fields":fields}),
    )?;
    c.db.execute("INSERT INTO task_conversations(space,actor,task,kind,handler,chat) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(space,actor,task,kind,handler) DO UPDATE SET chat=excluded.chat",params![c.space,c.actor,task.id,kind,handler,required(&result,"id")?]).map_err(storage)?;
    Ok(result)
}
pub(crate) fn command(c: &Context<'_>, q: &Value) -> Result<Value> {
    let chat = conversation(c, q)?;
    let mut request = json!({"op":"run.enqueue","task_id":q["id"],"chat_id":chat["id"],"handler_id":chat["fields"]["handler_id"],"handler_kind":chat["fields"]["handler_kind"],"instruction":q["instruction"]});
    if let Some(id) = q.get("request_id") {
        request["request_id"] = id.clone();
    }
    let run = worker::dispatch(c, &request).expect("run.enqueue is registered")?;
    Ok(json!({"chat":chat,"run":run}))
}

pub(crate) fn move_task(c: &Context<'_>, q: &Value) -> Result<Value> {
    move_with_status(c, q, None)
}
pub(crate) fn move_with_status(c: &Context<'_>, q: &Value, status: Option<&str>) -> Result<Value> {
    let old = c.readable("task", required(q, "id")?)?;
    c.authorize_write("task", Some(&old))?;
    domain::check_rev(q, &old)?;
    let target = Context {
        db: c.db,
        actor: c.actor,
        space: required(q, "target")?,
    };
    target.require_edit()?;
    if target.space == c.space || target.org_id()? != c.org_id()? {
        return Err(invalid("select another space in the same organization"));
    }
    if target.find("task", &old.id)?.is_some() {
        return Err(conflict("target already contains this task identifier"));
    }
    let linked:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE space=?1 AND ((kind='run' AND json_extract(fields,'$.task_id')=?2) OR (kind='artifact' AND (json_extract(fields,'$.task_id')=?2 OR json_extract(fields,'$.task')=?2)) OR (kind='relation' AND (json_extract(fields,'$.from')=?2 OR json_extract(fields,'$.to')=?2)) OR (kind='task' AND json_extract(fields,'$.parent')=?2)))",params![c.space,old.id],|r|r.get(0)).map_err(storage)?;
    if linked || old.fields["parent"].is_string() || old.fields["external_id"].is_string() {
        return Err(conflict(
            "this task has space-bound relationships, execution history or an external source",
        ));
    }
    let mut f = old.fields.clone();
    let retained_labels = crate::tags::move_labels(c, &old)?;
    f["labels"] = json!([]);
    if let Some(id) = f["iteration"].as_str()
        && target.find("iteration", id)?.is_none()
    {
        f["iteration"] = Value::Null;
    }
    if let Some(id) = f["project"].as_str()
        && tasks::resolve_project(&target, id).is_err()
    {
        f["project"] = Value::Null;
    }
    if let Some(people) = f["assignees"].as_array_mut() {
        let mut kept = Vec::new();
        for id in people.iter().filter_map(Value::as_str) {
            if id == "owner" || target.find("member", id)?.is_some() {
                kept.push(json!(id));
            }
        }
        *people = kept;
    }
    if let (Some(kind), Some(id)) = (f["executor"]["kind"].as_str(), f["executor"]["id"].as_str())
        && target.readable(kind, id).is_err()
    {
        f["executor"] = Value::Null;
    }
    assignees::project(&mut f);
    if let Some(owner) = f["owner"].as_str()
        && owner != "owner"
        && target.find("member", owner)?.is_none()
    {
        f["owner"] = json!(c.actor);
    }
    if let Some(id) = f["type"].as_str()
        && target.find("task_type", id)?.is_none()
    {
        f["type"] = Value::Null;
    }
    if let Some(id) = f["status"].as_str()
        && target.find("status", id)?.is_none()
    {
        f["status"] = Value::Null;
    }
    if let Some(status) = status {
        f["status"] = json!(status);
    }
    tasks::validate_task(&target, None, &old.id, &mut f, true)?;
    crate::task_fields::validate_destination(&target, &f)?;
    f["labels"] = retained_labels;
    let retired: i64 =
        c.db.query_row(
            "SELECT coalesce(max(rev),0) FROM event_log WHERE space=?1 AND kind='task' AND id=?2",
            params![target.space, old.id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let moved = Record {
        space: target.space.into(),
        fields: f,
        rev: old.rev.max(retired) + 1,
        updated_at: crate::now(),
        ..old.clone()
    };
    if let Some(files) = moved.fields["attachments"].as_array() {
        for file in files {
            let id = required(file, "id")?;
            let n=c.db.execute("INSERT INTO attachments(space,id,content) SELECT ?1,id,content FROM attachments WHERE space=?2 AND id=?3 ON CONFLICT(space,id) DO UPDATE SET content=excluded.content WHERE content=excluded.content",params![target.space,c.space,id]).map_err(storage)?;
            if n != 1 {
                return Err(conflict(
                    "attachment is absent or conflicts in the target space",
                ));
            }
        }
    }
    target.insert(&moved)?;
    for child in c
        .records("comment")?
        .into_iter()
        .filter(|r| r.fields["task"] == old.id)
    {
        if !c.visible(&child)? || target.find("comment", &child.id)?.is_some() {
            return Err(conflict(
                "a comment is private or has an identifier conflict in the target",
            ));
        }
        target.insert(&Record {
            space: target.space.into(),
            rev: child.rev + 1,
            ..child.clone()
        })?;
        c.db.execute("INSERT INTO comment_reactions(space,comment,actor,emoji) SELECT ?1,comment,actor,emoji FROM comment_reactions WHERE space=?2 AND comment=?3",params![target.space,c.space,child.id]).map_err(storage)?;
        c.remove(&child)?;
    }
    c.db.execute("INSERT OR IGNORE INTO followers(space,task,actor) SELECT ?1,task,actor FROM followers WHERE space=?2 AND task=?3",params![target.space,c.space,old.id]).map_err(storage)?;
    tasks::cleanup(c, &old)?;
    c.remove(&old)?;
    crate::triage::moved(c, &old.id, target.space)?;
    Ok(as_value(moved))
}
