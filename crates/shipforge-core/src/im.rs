//! Thread bindings, inbox receipts and outbox rows share the transaction that writes the task.
mod cards;
mod origin;
mod outbox;
mod replies;
use crate::{Context, Record, Result, conflict, forbidden, invalid, required, storage};
pub(crate) use origin::Origin;
pub(crate) use origin::origin;
pub(crate) use replies::{decorate, options};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) fn init(db: &Connection) -> Result<()> {
    cards::init(db)?;
    db.execute_batch("
        CREATE TABLE IF NOT EXISTS sf_im_threads (
            space TEXT NOT NULL, connector TEXT NOT NULL, route TEXT NOT NULL, task TEXT NOT NULL,
            active INTEGER NOT NULL, PRIMARY KEY(space,connector,route));
        CREATE INDEX IF NOT EXISTS sf_im_task ON sf_im_threads(space,task,active);
        CREATE TABLE IF NOT EXISTS sf_im_messages (
            space TEXT NOT NULL, connector TEXT NOT NULL, message TEXT NOT NULL, route TEXT NOT NULL,
            hash TEXT NOT NULL, response TEXT NOT NULL, comment TEXT,
            PRIMARY KEY(space,connector,message));
        CREATE INDEX IF NOT EXISTS sf_im_comment ON sf_im_messages(space,comment);
        CREATE TABLE IF NOT EXISTS sf_im_replies (
            space TEXT NOT NULL,actor TEXT NOT NULL,request TEXT NOT NULL,hash TEXT NOT NULL,comment TEXT NOT NULL,
            connector TEXT NOT NULL,route TEXT NOT NULL,PRIMARY KEY(space,actor,request),UNIQUE(space,comment));
        CREATE TABLE IF NOT EXISTS sf_im_events (
            space TEXT NOT NULL, connector TEXT NOT NULL, event TEXT NOT NULL, message TEXT NOT NULL,
            PRIMARY KEY(space,connector,event));
        CREATE TABLE IF NOT EXISTS sf_im_outbox (
            seq INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, space TEXT NOT NULL, connector TEXT NOT NULL,
            route TEXT NOT NULL, task TEXT NOT NULL, kind TEXT NOT NULL, source TEXT NOT NULL, rev INTEGER NOT NULL,
            body TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', available INTEGER NOT NULL DEFAULT 0,
            lease TEXT, expires INTEGER, attempts INTEGER NOT NULL DEFAULT 0, remote TEXT, error TEXT,
            UNIQUE(space,connector,route,kind,source,rev));
        CREATE INDEX IF NOT EXISTS sf_im_pending ON sf_im_outbox(space,connector,state,available,seq);
        CREATE TABLE IF NOT EXISTS sf_im_rate_limits (
            workspace TEXT NOT NULL, app TEXT NOT NULL, available INTEGER NOT NULL, PRIMARY KEY(workspace,app));
    ").map_err(storage)
}

fn thread(c: &Context<'_>, connector: &str, route: &str) -> Result<Option<(String, bool)>> {
    c.db.query_row(
        "SELECT task,active FROM sf_im_threads WHERE space=? AND connector=? AND route=?",
        params![c.space, connector, route],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(storage)
}

fn shared_task(c: &Context<'_>, id: &str) -> Result<Record> {
    let task = c.readable("task", id)?;
    if task.fields["visibility"] == "private" {
        return Err(forbidden(
            "private tasks cannot participate in an external conversation",
        ));
    }
    Ok(task)
}

fn canonical(c: &Context<'_>, connector: &str, origin: &Origin) -> Result<String> {
    let parent: Option<String> =
        c.db.query_row(
            "SELECT route FROM sf_im_messages WHERE space=? AND connector=? AND message=?",
            params![c.space, connector, origin.message_key(&origin.root)],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    Ok(parent.unwrap_or_else(|| origin.route().to_string()))
}

fn legacy_task(
    c: &Context<'_>,
    connector: &str,
    origin: &Origin,
    mapped: &Value,
) -> Result<Option<String>> {
    // Adopt the pre-threading root when the first event after an upgrade is a reply.
    if origin.root == origin.message {
        return Ok(None);
    }
    let external = required(mapped, "external_id")?;
    let Some((prefix, _)) = external.rsplit_once(':') else {
        return Ok(None);
    };
    let root = if origin.provider == "teams" {
        crate::providers::teams_component(&origin.root)
    } else {
        origin.root.clone()
    };
    c.db.query_row(
        "SELECT task FROM sf_connector_objects WHERE space=? AND connector=? AND external=?",
        params![c.space, connector, format!("{prefix}:{root}")],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}

pub(crate) fn receive(
    c: &Context<'_>,
    connector: &Record,
    mapped: &Value,
    origin: Origin,
) -> Result<Value> {
    let id = &connector.id;
    let event = required(mapped, "event_id")?;
    let message = origin.message_key(&origin.message);
    let hash = crate::worker::digest(
        &json!([
            origin.route(),
            origin.author,
            mapped["source_version"],
            mapped["description"],
            mapped["external_id"]
        ])
        .to_string(),
    );
    let previous: Option<String> =
        c.db.query_row(
            "SELECT message FROM sf_im_events WHERE space=? AND connector=? AND event=?",
            params![c.space, id, event],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if previous.as_ref().is_some_and(|key| key != &message) {
        return Err(conflict("event_id was reused for another message"));
    }
    let seen: Option<(String, String)> =
        c.db.query_row(
            "SELECT hash,response FROM sf_im_messages WHERE space=? AND connector=? AND message=?",
            params![c.space, id, message],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((original, response)) = seen {
        if hash != original {
            return Err(conflict("message was reused with different content"));
        }
        let response: Value = serde_json::from_str(&response).map_err(storage)?;
        if let Some(task) = response["task"].as_str() {
            shared_task(c, task)?;
        }
        record_event(c, id, event, &message)?;
        return Ok(response);
    }

    let route = canonical(c, id, &origin)?;
    let bound = thread(c, id, &route)?;
    if bound.as_ref().is_some_and(|(_, active)| !active) {
        return Ok(json!({"action":"ignored","reason":"thread unbound"}));
    }
    let old_delivery:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_connector_events WHERE space=? AND connector=? AND event=?)",
        params![c.space,id,event],|r|r.get(0)).map_err(storage)?;
    let bound = match bound {
        Some((task, _)) => Some(task),
        None => legacy_task(c, id, &origin, mapped)?,
    };
    let comment_id = bound
        .as_ref()
        .filter(|_| !old_delivery)
        .map(|_| Uuid::new_v4().to_string());
    let response = if let Some(task) = bound.filter(|_| !old_delivery) {
        shared_task(c, &task)?;
        json!({"action":"commented","task":task,"comment":comment_id,"external_id":mapped["external_id"]})
    } else {
        crate::worker::dispatch(c, mapped)
            .ok_or_else(|| invalid("connector ingestion unavailable"))??
    };
    let task = required(&response, "task")?;
    let shared = shared_task(c, task)?;
    let added=c.db.execute(
        "INSERT OR IGNORE INTO sf_im_threads(space,connector,route,task,active) VALUES(?,?,?,?,1)",
        params![c.space, id, route, task],
    )
    .map_err(storage)?;
    if added == 1 {
        cards::initial(c, connector, &route, &shared)?;
    }
    c.db.execute("INSERT INTO sf_im_messages(space,connector,message,route,hash,response,comment) VALUES(?,?,?,?,?,?,?)",
        params![c.space,id,message,route,hash,response.to_string(),comment_id]).map_err(storage)?;
    record_event(c, id, event, &message)?;
    if let Some(comment) = comment_id {
        crate::write_request(
            c,
            &json!({"op":"create","kind":"comment","id":comment,"title":mapped["title"],
            "fields":{"task":task,"body":mapped["description"],"external_provider":origin.provider,
            "external_author":origin.author,"external_message":origin.message,"external_event":event,
            "external_thread":serde_json::from_str::<Value>(&route).map_err(storage)?}}),
        )?;
    }
    Ok(response)
}

fn record_event(c: &Context<'_>, id: &str, event: &str, message: &str) -> Result<()> {
    c.db.execute(
        "INSERT OR IGNORE INTO sf_im_events(space,connector,event,message) VALUES(?,?,?,?)",
        params![c.space, id, event, message],
    )
    .map_err(storage)?;
    Ok(())
}

pub(crate) fn execute(c: &Context<'_>, q: &Value) -> Result<Value> {
    if q["op"] == "im.threads" {
        let task = c.readable("task", required(q, "task_id")?)?;
        return options(c, &task.id);
    }
    if q["op"] == "im.reply" {
        return replies::send(c, q);
    }
    c.require_admin()?;
    let connector = c.readable("connector", required(q, "id")?)?;
    match required(q, "op")? {
        "im.bind" | "im.unbind" => {
            let preview = crate::providers::execute(c, q, true)?;
            let origin = origin(required(q, "provider")?, &q["payload"])?
                .ok_or_else(|| invalid("provider has no thread routing"))?;
            origin::validate_route(required(q, "provider")?, &preview["thread"])?;
            let task = shared_task(c, required(q, "task_id")?)?;
            let route = canonical(c, &connector.id, &origin)?;
            let bound = thread(c, &connector.id, &route)?;
            let bind = q["op"] == "im.bind";
            if bound
                .as_ref()
                .is_some_and(|(id, active)| id != &task.id && (*active || !bind))
            {
                return Err(conflict("thread is bound to a different task"));
            }
            if !bind && bound.is_none() {
                return Err(invalid("thread has no binding"));
            }
            c.db.execute("INSERT INTO sf_im_threads(space,connector,route,task,active) VALUES(?,?,?,?,?) ON CONFLICT(space,connector,route) DO UPDATE SET active=excluded.active,task=excluded.task",
                params![c.space,connector.id,route,task.id,bind]).map_err(storage)?;
            if !bind {
                outbox::cancel_route(c, &connector.id, &route)?;
            } else if bound.as_ref().is_none_or(|(_, active)| !*active) {
                cards::initial(c, &connector, &route, &task)?;
            }
            Ok(
                json!({"task":task.id,"active":bind,"thread":serde_json::from_str::<Value>(&route).map_err(storage)?}),
            )
        }
        _ => outbox::execute(c, &connector, q),
    }
}

pub(crate) use cards::click;
pub(crate) use outbox::{changed, echo, removed};

pub(crate) fn extend_schema(schema: &mut Value) {
    let clauses = schema["allOf"].as_array_mut().unwrap();
    clauses.push(json!({"if":{"required":["kind"],"properties":{"op":{"enum":["create","update"]},"kind":{"const":"connector"}}},"then":{"properties":{"fields":{"properties":{"slack_assignment":{"type":"boolean"},"assignee_map":{"type":"object","additionalProperties":{"type":"string"}}}}}}}));
    for (ops, fields) in [
        (
            vec!["im.bind", "im.unbind"],
            vec!["id", "provider", "payload", "task_id"],
        ),
        (vec!["im.outbox", "im.claim"], vec!["id"]),
        (vec!["im.threads"], vec!["task_id"]),
        (
            vec!["im.reply"],
            vec!["id", "task_id", "thread", "body", "request_id"],
        ),
        (
            vec!["im.ack"],
            vec!["id", "delivery_id", "lease", "outcome"],
        ),
        (vec!["im.retry"], vec!["id", "delivery_id", "decision"]),
    ] {
        clauses.push(json!({"if":{"properties":{"op":{"enum":ops}}},"then":{"required":fields}}));
    }
    clauses.push(json!({"if":{"properties":{"op":{"const":"im.ack"}}},"then":{"properties":{"outcome":{"enum":["sent","retry","uncertain","failed"]},"retry_after":{"type":"integer","minimum":1,"maximum":86400}}}}));
    clauses.push(json!({"if":{"properties":{"op":{"const":"im.retry"}}},"then":{"properties":{"decision":{"enum":["resend","confirm"]}}}}));
    clauses.push(json!({"if":{"properties":{"op":{"const":"im.outbox"}}},"then":{"properties":{"state":{"enum":["pending","leased","uncertain","failed","sent","cancelled"]},"available_only":{"type":"boolean"}}}}));
}
