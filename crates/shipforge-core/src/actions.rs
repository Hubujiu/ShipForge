use super::*;
use crate::domain::{check_rev, ref_record};
use crate::tasks::target_status;
use std::collections::BTreeSet;

pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    match op {
        "follow" => follow(c, q),
        "notification.act" => notification(c, q),
        "iteration.close" => finish_iteration(c, q),
        "triage" => triage(c, q),
        "document.restore" => restore(c, q),
        "artifact.save" => save_artifact(c, q),
        "import" => crate::transfer::import(c, q, false),
        _ => Err(invalid("unknown command")),
    }
}
pub(crate) fn after(c: &Context<'_>, old: Option<&Record>, row: &Record) -> Result<()> {
    if row.kind == "space"
        && old.is_some_and(|r| r.fields["triage_enabled"] == true)
        && row.fields["triage_enabled"] != true
    {
        for task in c.records("task")? {
            if c.status_category(&task)? == "triage" {
                let status = target_status(c, &task, "backlog")?;
                let mut f = task.fields.clone();
                f["status"] = json!(status);
                c.update(&task, &task.title, f)?;
            }
        }
    }
    let task = match row.kind.as_str() {
        "task" => row.clone(),
        "comment" if old.is_none() => c.get("task", required(&row.fields, "task")?)?,
        _ => return Ok(()),
    };
    let mut recipients = BTreeSet::new();
    for key in ["owner", "creator"] {
        if let Some(s) = task.fields[key].as_str() {
            recipients.insert(s.to_string());
        }
    }
    if task.fields["assignee_kind"] == "member"
        && let Some(s) = task.fields["assignee"].as_str()
    {
        recipients.insert(s.into());
    }
    let mut stmt =
        c.db.prepare("SELECT actor FROM followers WHERE space=?1 AND task=?2")
            .map_err(storage)?;
    for actor in stmt
        .query_map(params![c.space, task.id], |r| r.get::<_, String>(0))
        .map_err(storage)?
    {
        recipients.insert(actor.map_err(storage)?);
    }
    let mentions = row.fields["mentions"].as_array();
    if let Some(people) = mentions {
        for text in people.iter().filter_map(Value::as_str) {
            recipients.insert(text.into());
        }
    }
    for actor in recipients {
        if actor == c.actor {
            continue;
        }
        let target = Context {
            db: c.db,
            space: c.space,
            actor: &actor,
        };
        if target.role().is_err() || !target.visible(&task)? {
            continue;
        }
        let important = mentions.is_some_and(|a| a.iter().any(|s| s == &actor))
            || task.fields["assignee"] == actor;
        notify(c, &actor, &task, &row.id, important)?;
    }
    Ok(())
}
fn notify(
    c: &Context<'_>,
    recipient: &str,
    task: &Record,
    source: &str,
    important: bool,
) -> Result<()> {
    let previous=c.db.query_row("SELECT id FROM records WHERE space=?1 AND kind='notification' AND json_extract(fields,'$.task')=?2 AND json_extract(fields,'$.recipient')=?3",params![c.space,task.id,recipient],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    let fields = json!({"recipient":recipient,"task":task.id,"importance":if important{"important"}else{"other"},"latest":source,"actor":c.actor,"state":"inbox","read":false});
    if let Some(id) = previous {
        let old = c.get("notification", &id)?;
        let mut next = fields;
        next["count"] = json!(old.fields["count"].as_u64().unwrap_or(1) + 1);
        if old.fields["state"] == "snoozed"
            && old.fields["until"].as_i64().is_some_and(|t| t > now())
        {
            next["state"] = json!("snoozed");
            next["until"] = old.fields["until"].clone();
        }
        c.update(&old, &task.title, next)?;
    } else {
        let mut next = fields;
        next["count"] = json!(1);
        c.create("notification", None, &task.title, next)?;
    }
    Ok(())
}
fn follow(c: &Context<'_>, q: &Value) -> Result<Value> {
    let id = required(q, "id")?;
    let task = c.readable("task", id)?;
    let enabled = q.get("enabled").map_or(Ok(true), |v| {
        v.as_bool()
            .ok_or_else(|| invalid("enabled must be boolean"))
    })?;
    if enabled {
        c.db.execute(
            "INSERT OR IGNORE INTO followers(space,task,actor) VALUES(?1,?2,?3)",
            params![c.space, task.id, c.actor],
        )
        .map_err(storage)?;
    } else {
        c.db.execute(
            "DELETE FROM followers WHERE space=?1 AND task=?2 AND actor=?3",
            params![c.space, task.id, c.actor],
        )
        .map_err(storage)?;
    }
    Ok(json!({"id":id,"actor":c.actor,"following":enabled}))
}
fn notification(c: &Context<'_>, q: &Value) -> Result<Value> {
    let n = c.readable("notification", required(q, "id")?)?;
    check_rev(q, &n)?;
    let mut f = n.fields.clone();
    match required(q, "action")? {
        "read" => f["read"] = json!(true),
        "unread" => f["read"] = json!(false),
        "clear" => {
            f["state"] = json!("cleared");
            f["read"] = json!(true);
        }
        "snooze" => {
            let until = q["until"]
                .as_i64()
                .filter(|t| *t > now())
                .ok_or_else(|| invalid("snooze needs a future until value in milliseconds"))?;
            f["state"] = json!("snoozed");
            f["until"] = json!(until);
        }
        "restore" => {
            f["state"] = json!("inbox");
            f["until"] = Value::Null;
        }
        _ => return Err(invalid("unknown notification action")),
    }
    Ok(as_value(c.update(&n, &n.title, f)?))
}
fn finish_iteration(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let it = c.readable("iteration", required(q, "id")?)?;
    check_rev(q, &it)?;
    if it.fields["status"] == "closed" {
        return Err(conflict("iteration already closed"));
    }
    let target = ref_record(c, q, "target", "iteration")?;
    if let Some(next) = &target
        && (next.id == it.id || next.fields["status"] == "closed")
    {
        return Err(invalid("choose a different open iteration"));
    }
    let mut moved = Vec::new();
    for task in c.records("task")? {
        if task.fields["iteration"] != it.id
            || matches!(c.status_category(&task)?.as_str(), "done" | "cancelled")
        {
            continue;
        }
        if !c.visible(&task)? {
            return Err(forbidden("cannot move all tasks in the iteration"));
        }
        let mut f = json!({"iteration":target.as_ref().map(|r|&r.id)});
        if target.is_none() {
            f["status"] = json!(target_status(c, &task, "backlog")?);
        }
        write_request(
            c,
            &json!({"op":"update","kind":"task","id":task.id,"fields":f}),
        )?;
        moved.push(task.id);
    }
    let mut f = it.fields.clone();
    f["status"] = json!("closed");
    f["closed_at"] = json!(now());
    Ok(json!({"iteration":c.update(&it,&it.title,f)?,"moved":moved}))
}
fn triage(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let task = c.readable("task", required(q, "id")?)?;
    check_rev(q, &task)?;
    if c.status_category(&task)? != "triage" {
        return Err(invalid("task is outside triage"));
    }
    let choice = required(q, "decision")?;
    let mut patch = json!({"triage_decision":choice});
    match choice {
        "accept" => {
            patch["status"] = json!(target_status(c, &task, "todo")?);
        }
        "defer" => {
            if let Some(until) = q.get("until") {
                patch["deferred_until"] = until.clone();
            }
        }
        "reject" | "duplicate" => {
            patch["status"] = json!(target_status(c, &task, "cancelled")?);
            if choice == "duplicate" {
                write_request(
                    c,
                    &json!({"op":"create","kind":"relation","title":"重复任务","fields":{"from":task.id,"to":required(q,"target")?,"type":"duplicate"}}),
                )?;
            }
        }
        _ => return Err(invalid("unknown triage decision")),
    }
    if let Some(p) = q.get("priority") {
        patch["priority"] = p.clone();
    }
    write_request(
        c,
        &json!({"op":"update","kind":"task","id":task.id,"fields":patch}),
    )
}
fn restore(c: &Context<'_>, q: &Value) -> Result<Value> {
    let doc = c.readable("document", required(q, "id")?)?;
    c.authorize_write("document", Some(&doc))?;
    check_rev(q, &doc)?;
    let version = q["version"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("missing document version"))?;
    let saved: Option<(String, String)> =
        c.db.query_row(
            "SELECT title,fields FROM versions WHERE space=?1 AND id=?2 AND rev=?3",
            params![c.space, doc.id, version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let (title, raw) = saved.ok_or_else(|| "not_found: document version".to_string())?;
    let mut past: Value = serde_json::from_str(&raw).map_err(storage)?;
    if !c.visible(&Record {
        fields: past.clone(),
        ..doc.clone()
    })? {
        return Err(forbidden("document version is private"));
    }
    // Restoration changes content while preserving the current sharing policy.
    for k in ["visibility", "owner", "creator"] {
        past[k] = doc.fields[k].clone();
    }
    let result = c.update(&doc, &title, past)?;
    Ok(as_value(result))
}
fn save_artifact(c: &Context<'_>, q: &Value) -> Result<Value> {
    let item = c.readable("artifact", required(q, "id")?)?;
    let body = item.fields["body"]
        .as_str()
        .or_else(|| item.fields["url"].as_str())
        .or_else(|| item.fields["path"].as_str())
        .unwrap_or(&item.title);
    let request = json!({"op":"create","kind":"document","title":q["title"].as_str().unwrap_or(&item.title),"fields":{"body":body,"artifact":item.id,"visibility":q["visibility"].as_str().unwrap_or("private")}});
    if let Some(files) = item.fields.get("attachments") {
        domain::write_with_files(c, &request, files.clone())
    } else {
        write_request(c, &request)
    }
}
