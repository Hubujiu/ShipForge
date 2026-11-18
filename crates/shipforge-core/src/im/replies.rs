use super::{outbox, shared_task, thread};
use crate::{Context, Record, Result, conflict, identifier, invalid, required, storage};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

fn authorized(c: &Context<'_>, task: Option<&str>) -> Result<HashMap<String, Vec<Value>>> {
    let mut st=c.db.prepare("SELECT t.task,t.connector,t.route FROM sf_im_threads t JOIN records r ON r.space=t.space AND r.kind='task' AND r.id=t.task WHERE t.space=? AND t.active=1 AND (? IS NULL OR t.task=?) AND COALESCE(json_extract(r.fields,'$.visibility'),'space')!='private' ORDER BY t.task,t.connector,t.route").map_err(storage)?;
    let rows = st
        .query_map(params![c.space, task, task], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?;
    let mut result = HashMap::<String, Vec<Value>>::new();
    let mut known = HashMap::new();
    for row in rows {
        let (task, id, serialized) = row.map_err(storage)?;
        if !known.contains_key(&id) {
            let record = c.find("connector", &id)?;
            let allowed = match record {
                Some(r) if c.visible(&r)? => Some(r),
                _ => None,
            };
            known.insert(id.clone(), allowed);
        }
        let route: Value = serde_json::from_str(&serialized).map_err(storage)?;
        if let Some(connector) = &known[&id]
            && outbox::scoped(connector, &route)
        {
            result
                .entry(task)
                .or_default()
                .push(json!({"id":id,"title":connector.title,"thread":route}));
        }
    }
    Ok(result)
}
pub(crate) fn options(c: &Context<'_>, task: &str) -> Result<Value> {
    Ok(json!(
        authorized(c, Some(task))?.remove(task).unwrap_or_default()
    ))
}
pub(crate) fn decorate(c: &Context<'_>, rows: &mut [Record]) -> Result<()> {
    let mut threads = authorized(c, None)?;
    for task in rows {
        if let Some(items) = threads.remove(&task.id) {
            task.fields["im_threads"] = json!(items)
        }
    }
    Ok(())
}
pub(super) fn send(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let connector = c.readable("connector", required(q, "id")?)?;
    let task = shared_task(c, required(q, "task_id")?)?;
    let route = &q["thread"];
    let route_text = route.to_string();
    if !outbox::scoped(&connector, route)
        || thread(c, &connector.id, &route_text)? != Some((task.id.clone(), true))
    {
        return Err(invalid("select an active authorized thread for this task"));
    }
    let body = required(q, "body")?;
    if body.len() > 1_048_576 {
        return Err(invalid("reply exceeds 1 MiB"));
    }
    let key = required(q, "request_id")?;
    identifier(key)?;
    let hash = crate::worker::digest(&json!([connector.id, task.id, route, body]).to_string());
    let prior: Option<(String, String)> =
        c.db.query_row(
            "SELECT hash,comment FROM sf_im_replies WHERE space=? AND actor=? AND request=?",
            params![c.space, c.actor, key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((old, id)) = prior {
        if old != hash {
            return Err(conflict(
                "request_id already identifies another thread reply",
            ));
        }
        return Ok(crate::as_value(c.readable("comment", &id)?));
    }
    let comment = Uuid::new_v4().to_string();
    c.db.execute("INSERT INTO sf_im_replies(space,actor,request,hash,comment,connector,route) VALUES(?,?,?,?,?,?,?)",params![c.space,c.actor,key,hash,comment,connector.id,route_text]).map_err(storage)?;
    let title: String = body
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("Reply")
        .chars()
        .take(160)
        .collect();
    crate::write_request(
        c,
        &json!({"op":"create","kind":"comment","id":comment,"title":title,"fields":{"task":task.id,"body":body,"reply_provider":"slack","reply_thread":route,"reply_connector":connector.id}}),
    )
}
