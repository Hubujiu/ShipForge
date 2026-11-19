use super::*;
use serde_json::Map;
use std::collections::{HashMap, HashSet};

pub(crate) fn reactions(c: &Context<'_>, q: &Value, write: bool) -> Result<Value> {
    let comment = c.readable("comment", required(q, "id")?)?;
    if write {
        c.require_edit()?;
        let symbol = required(q, "emoji")?.trim();
        if symbol.is_empty() || symbol.chars().count() > 32 || symbol.chars().any(char::is_control)
        {
            return Err(invalid("enter a reaction up to 32 characters"));
        }
        let enabled = q["enabled"]
            .as_bool()
            .ok_or_else(|| invalid("enabled must be a boolean"))?;
        if enabled {
            let count=c.db.execute("INSERT OR IGNORE INTO comment_reactions(space,comment,actor,emoji)VALUES(?1,?2,?3,?4)",params![c.space,comment.id,c.actor,symbol]).map_err(storage)?;
            if count == 1
                && let Some(actor) = comment.fields["creator"]
                    .as_str()
                    .filter(|id| *id != c.actor)
                && comment.fields["authored_by_agent"].is_null()
            {
                let task = c.readable("task", required(&comment.fields, "task")?)?;
                crate::inbox::task_notice(
                    c,
                    &task,
                    actor,
                    "reaction",
                    Some(("comment", &comment.id)),
                    false,
                )?;
            }
        } else {
            c.db.execute("DELETE FROM comment_reactions WHERE space=?1 AND comment=?2 AND actor=?3 AND emoji=?4",params![c.space,comment.id,c.actor,symbol]).map_err(storage)?;
        }
    }
    let mut stmt=c.db.prepare("SELECT emoji,COUNT(*),MAX(actor=?3) FROM comment_reactions WHERE space=?1 AND comment=?2 GROUP BY emoji ORDER BY COUNT(*) DESC,emoji").map_err(storage)?;
    let rows=stmt.query_map(params![c.space,comment.id,c.actor],|r|Ok(json!({"emoji":r.get::<_,String>(0)?,"count":r.get::<_,i64>(1)?,"mine":r.get::<_,bool>(2)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
    Ok(json!({"comment":comment.id,"reactions":rows}))
}

pub(crate) fn protect(
    c: &Context<'_>,
    patch: &mut Map<String, Value>,
    old: Option<&Record>,
    import: bool,
) -> Result<()> {
    for key in [
        "resolved",
        "resolution_comment",
        "resolved_at",
        "resolved_by",
        "resolved_by_agent",
        "authored_by_agent",
        "reactions",
    ] {
        if patch.contains_key(key) && !import {
            return Err(invalid(
                "discussion state and Agent authorship are assigned by the workspace",
            ));
        }
        patch.remove(key);
    }
    if let Some(row) = old
        && ["task", "parent"]
            .iter()
            .any(|&k| patch.get(k).is_some_and(|v| v != &row.fields[k]))
    {
        return Err(invalid("a comment cannot change its task or parent"));
    }
    if let Some(row) = old
        && row.fields["parent"].is_string()
        && row.fields["visibility"] != "private"
        && patch.get("visibility") == Some(&json!("private"))
    {
        let (_, root) = locate(c, required(&row.fields, "task")?, &row.id)?;
        if let Some(mark) = root.fields["resolution_comment"].as_str() {
            let mut current = c.get("comment", mark)?;
            while let Some(parent) = current.fields["parent"].as_str() {
                if current.id == row.id {
                    return Err(conflict(
                        "reopen the discussion before making its resolution private",
                    ));
                }
                current = c.get("comment", parent)?;
            }
        }
    }
    Ok(())
}

// An older database may contain malformed parent links. Stop at the first
// missing/private ancestor; a descendant must not reveal a hidden discussion.
pub(crate) fn ancestry_visible(c: &Context<'_>, row: &Record) -> Result<bool> {
    let mut parent = row.fields["parent"].as_str().map(str::to_owned);
    let mut seen = HashSet::from([row.id.clone()]);
    while let Some(id) = parent {
        if !seen.insert(id.clone()) {
            return Ok(false);
        }
        let Some(p) = c.find("comment", &id)? else {
            return Ok(false);
        };
        if p.fields["task"] != row.fields["task"]
            || p.fields["visibility"] == "private"
                && p.fields["creator"] != c.actor
                && p.fields["owner"] != c.actor
        {
            return Ok(false);
        }
        parent = p.fields["parent"].as_str().map(str::to_owned);
    }
    Ok(true)
}

fn locate(c: &Context<'_>, task: &str, comment: &str) -> Result<(Record, Record)> {
    c.readable("task", task)?;
    let item = c.readable("comment", comment)?;
    if item.fields["task"] != task {
        return Err(invalid("comment does not belong to this task"));
    }
    let mut root = item.clone();
    // readable has already checked the entire ancestry for cycles and visibility.
    while let Some(parent) = root.fields["parent"].as_str() {
        root = c.get("comment", parent)?;
    }
    Ok((item, root))
}

pub(crate) fn thread(c: &Context<'_>, q: &Value) -> Result<Value> {
    let task = required(q, "task_id")?;
    let (item, root) = locate(c, task, required(q, "id")?)?;
    let mut by_parent: HashMap<String, Vec<Record>> = HashMap::new();
    for row in c.records("comment")? {
        if row.fields["task"] == task
            && let Some(parent) = row.fields["parent"].as_str()
            && c.visible(&row)?
        {
            by_parent.entry(parent.to_owned()).or_default().push(row);
        }
    }
    for children in by_parent.values_mut() {
        children.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    }
    let mut pending = vec![root.clone()];
    let mut comments = Vec::new();
    while let Some(row) = pending.pop() {
        if let Some(children) = by_parent.remove(&row.id) {
            pending.extend(children);
        }
        comments.push(row);
    }
    Ok(
        json!({"task_id":task,"task":c.readable("task",task)?,"root":root,"selected":item.id,"comments":comments}),
    )
}

pub(crate) fn mark(
    c: &Context<'_>,
    q: &Value,
    agent: Option<&str>,
    resolved: bool,
) -> Result<Value> {
    c.require_edit()?;
    let (item, root) = locate(c, required(q, "task_id")?, required(q, "id")?)?;
    if let Some(agent) = agent
        && !c.db.query_row("SELECT EXISTS(SELECT 1 FROM discussion_authors WHERE space=? AND comment=? AND agent=?)",params![c.space,root.id,agent],|r|r.get::<_,bool>(0)).map_err(storage)?
    {
        return Err(forbidden(
            "Agent execution may change only its own root discussions",
        ));
    }
    crate::domain::check_rev(q, &root)?;
    if resolved {
        let mut current = item.clone();
        while let Some(parent) = current.fields["parent"].as_str() {
            if current.fields["visibility"] == "private" {
                return Err(invalid(
                    "an individually private reply cannot identify the resolution of its parent discussion",
                ));
            }
            current = c.get("comment", parent)?;
        }
    }
    let mut fields = root.fields.clone();
    if resolved {
        if fields["resolved"] == true && fields["resolution_comment"] == item.id {
            return Ok(as_value(root));
        }
        fields["resolved"] = json!(true);
        fields["resolution_comment"] = json!(item.id);
        fields["resolved_at"] = json!(now());
        fields["resolved_by"] = json!(c.actor);
        fields["resolved_by_agent"] = json!(agent);
    } else {
        if fields["resolved"] != true {
            return Ok(as_value(root));
        }
        fields["resolved"] = json!(false);
        for key in [
            "resolution_comment",
            "resolved_at",
            "resolved_by",
            "resolved_by_agent",
        ] {
            fields.as_object_mut().unwrap().remove(key);
        }
    }
    Ok(as_value(c.update_mode(
        &root,
        &root.title,
        fields,
        false,
    )?))
}

pub(crate) fn delete(c: &Context<'_>, row: &Record) -> Result<()> {
    let task = required(&row.fields, "task")?;
    if c.records("comment")?
        .iter()
        .any(|r| r.fields["task"] == task && r.fields["parent"] == row.id)
    {
        return Err(conflict(
            "remove or retain the replies before deleting their parent",
        ));
    }
    let (_, root) = locate(c, task, &row.id)?;
    if root.id != row.id && root.fields["resolution_comment"] == row.id {
        mark(c, &json!({"id":row.id,"task_id":task}), None, false)?;
    }
    Ok(())
}

pub(crate) fn restore(c: &Context<'_>, root_id: &str, requested: &str) -> Result<bool> {
    let root = c.readable("comment", root_id)?;
    let task = required(&root.fields, "task")?;
    let apply = || -> Result<bool> {
        let (_, located) = locate(c, task, requested)?;
        if located.id != root.id {
            return Ok(false);
        }
        mark(c, &json!({"task_id":task,"id":requested}), None, true)?;
        Ok(true)
    };
    match apply() {
        Ok(true) => return Ok(true),
        Err(e) if !e.starts_with("not_found:") && !e.starts_with("invalid:") => return Err(e),
        _ => {}
    }
    mark(c, &json!({"task_id":task,"id":root.id}), None, true)?;
    Ok(false)
}

pub(crate) fn run(c: &Context<'_>, q: &Value, run: &Record) -> Result<Value> {
    if run.fields["handler_kind"] != "agent" {
        return Err(forbidden("an individual Agent run is required"));
    }
    let agent = required(&run.fields, "handler_id")?;
    let task = required(&run.fields, "task_id")?;
    if q.get("task_id").is_some_and(|v| v != task) {
        return Err(invalid("the discussion must belong to the leased task"));
    }
    let actor = required(&run.fields, "requested_by")?;
    let scope = Context { actor, ..*c };
    scope.require_edit()?;
    scope.readable("task", task)?;
    if let Some(chat) = run.fields["chat_id"].as_str() {
        scope.readable("chat", chat)?;
    }
    match required(q, "action")? {
        "comment" | "reply" => {
            let body = required(q, "body")?;
            let call = required(q, "call_id")?;
            identifier(call)?;
            let mut f = json!({"task":task,"body":body});
            if q["action"] == "reply" {
                let (item, _) = locate(&scope, task, required(q, "comment_id")?)?;
                f["parent"] = json!(item.id);
            }
            let payload = json!([q["action"], f["parent"], body]).to_string();
            let digest = ring::digest::digest(&ring::digest::SHA256, payload.as_bytes());
            let prior:Option<(Vec<u8>,String)>=c.db.query_row("SELECT payload,comment FROM discussion_calls WHERE space=? AND run=? AND call_id=?",params![c.space,run.id,call],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
            if let Some((saved, id)) = prior {
                if saved != digest.as_ref() {
                    return Err(conflict(
                        "call_id already identifies another discussion action",
                    ));
                }
                return Ok(as_value(scope.readable("comment", &id)?));
            }
            let value = crate::domain::write_agent_comment(
                &scope,
                &json!({"op":"create","kind":"comment","title":body.chars().take(80).collect::<String>(),"fields":f}),
                agent,
            )?;
            c.db.execute(
                "INSERT INTO discussion_authors(space,comment,agent) VALUES(?,?,?)",
                params![c.space, required(&value, "id")?, agent],
            )
            .map_err(storage)?;
            c.db.execute(
                "INSERT INTO discussion_calls(space,run,call_id,payload,comment) VALUES(?,?,?,?,?)",
                params![
                    c.space,
                    run.id,
                    call,
                    digest.as_ref(),
                    required(&value, "id")?
                ],
            )
            .map_err(storage)?;
            Ok(value)
        }
        "resolve" | "reopen" => {
            let mut change = json!({"id":required(q,"comment_id")?,"task_id":task});
            if let Some(rev) = q.get("expected_rev") {
                change["expected_rev"] = rev.clone();
            }
            mark(&scope, &change, Some(agent), q["action"] == "resolve")
        }
        _ => Err(invalid("unknown run discussion action")),
    }
}
