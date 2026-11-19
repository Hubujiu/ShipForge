use crate::{Context, Record, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;

#[path = "automation.rs"]
mod automation;
mod working_hours;

pub(crate) const OPS: &[&str] = &[
    "agent.resolve",
    "catalog.list",
    "catalog.install",
    "tool.authorize",
    "machine.register",
    "machine.heartbeat",
    "machine.disable",
    "machine.enable",
    "run.enqueue",
    "run.claim",
    "run.heartbeat",
    "run.report",
    "run.cancel",
    "run.retry",
    "run.reply",
    "run.tool",
    "run.discussion",
    "automation.fire",
    "automation.tick",
    "connector.ingest",
    "connector.export",
];
pub(crate) const READ_OPS: &[&str] = &[
    "agent.resolve",
    "catalog.list",
    "tool.authorize",
    "connector.export",
];
const LEASE_MS: i64 = 120_000;

pub(crate) fn extend_schema(schema: &mut Value) {
    let props = schema["properties"]
        .as_object_mut()
        .expect("schema properties");
    for field in [
        "machine_id",
        "tool_id",
        "call_id",
        "handler_id",
        "task_id",
        "chat_id",
        "external_id",
        "source_version",
        "lease",
    ] {
        props.insert(
            field.into(),
            json!({"type":"string","minLength":1,"maxLength":4096}),
        );
    }
    props.insert(
        "machine_key".into(),
        json!({"type":"string","minLength":32,"maxLength":256,"writeOnly":true}),
    );
    props.insert("handler_kind".into(), json!({"enum":["agent","crew"]}));
    props.insert(
        "capacity".into(),
        json!({"type":"integer","minimum":1,"maximum":64}),
    );
    props.insert(
        "runtimes".into(),
        json!({"type":"array","items":{"type":"string"},"maxItems":128}),
    );
    let rules = schema["allOf"].as_array_mut().expect("schema rules");
    rules.push(json!({
        "if":{"required":["kind"],"properties":{"op":{"enum":["create","update"]},"kind":{"const":"agent"}}},
        "then":{"properties":{"fields":{"properties":{"working_hours":working_hours::schema()}}}}
    }));
    for (ops, required) in [
        (
            &[
                "agent.resolve",
                "catalog.install",
                "machine.enable",
                "machine.disable",
                "run.cancel",
                "run.retry",
            ][..],
            &["id"][..],
        ),
        (&["tool.authorize"], &["id", "tool_id"]),
        (
            &["machine.register"],
            &["machine_id", "machine_key", "title"],
        ),
        (
            &["machine.heartbeat", "run.claim"],
            &["machine_id", "machine_key"],
        ),
        (
            &["run.heartbeat", "run.report", "run.tool", "run.discussion"],
            &["machine_id", "machine_key", "id", "lease"],
        ),
        (&["run.tool"], &["tool_id", "call_id"]),
        (&["run.report"], &["status"]),
        (&["run.reply"], &["id", "text"]),
        (&["run.enqueue"], &["handler_id"]),
        (&["automation.fire"], &["id", "event_id"]),
        (
            &["connector.ingest"],
            &["id", "external_id", "event_id", "source_version", "title"],
        ),
        (&["connector.export"], &["id", "task_id"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"enum":ops}}},"then":{"required":required}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"const":"run.report"}}},"then":{"properties":{"status":{"enum":["done","failed","cancelled","waiting"]},"artifacts":{"type":"array","maxItems":100}}}}));
}

fn now() -> i64 {
    Utc::now().timestamp_millis()
}
fn db_error(e: rusqlite::Error) -> String {
    e.to_string()
}
fn token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
pub(crate) fn digest(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn req<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
        .ok_or_else(|| format!("required string: {key}"))
}
fn array_ids(v: &Value) -> Result<Vec<&str>> {
    if v.is_null() {
        return Ok(Vec::new());
    }
    let items = v
        .as_array()
        .filter(|a| a.len() <= 128)
        .ok_or("expected a list of at most 128 IDs")?;
    items
        .iter()
        .map(|s| {
            s.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 256)
                .ok_or_else(|| "invalid reference in list".to_owned())
        })
        .collect()
}
fn read(c: &Context<'_>, kind: &str, id: &str) -> Result<Record> {
    let r = c.get(kind, id)?;
    if !c.visible(&r)? {
        return Err("record unavailable to this actor".into());
    }
    Ok(r)
}
fn encoded(record: Record) -> Result<Value> {
    serde_json::to_value(record).map_err(|e| e.to_string())
}
fn sql_init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_machine_keys(space TEXT NOT NULL,machine TEXT NOT NULL,owner TEXT NOT NULL,key_hash TEXT NOT NULL,PRIMARY KEY(space,machine));
       CREATE TABLE IF NOT EXISTS sf_run_keys(space TEXT NOT NULL,origin TEXT NOT NULL,run TEXT NOT NULL,PRIMARY KEY(space,origin));
       CREATE TABLE IF NOT EXISTS sf_run_leases(space TEXT NOT NULL,run TEXT NOT NULL,machine TEXT NOT NULL,lease_hash TEXT NOT NULL,expires INTEGER NOT NULL,response_hash TEXT,PRIMARY KEY(space,run));
       CREATE INDEX IF NOT EXISTS sf_lease_machine ON sf_run_leases(space,machine,expires);").map_err(db_error)
}
pub(crate) fn init(db: &Connection) -> Result<()> {
    sql_init(db)?;
    crate::agents::init(db)?;
    automation::init(db)
}

pub(crate) fn dispatch(c: &Context<'_>, q: &Value) -> Option<Result<Value>> {
    let op = q["op"].as_str()?;
    if !OPS.contains(&op) {
        return None;
    }
    Some((|| {
        if !READ_OPS.contains(&op) {
            c.require_edit()?;
        }
        match op {
            "agent.resolve" => resolved(c, req(q, "id")?),
            "machine.register" => register(c, q),
            "machine.heartbeat" => {
                let machine = authenticate(c, q)?;
                let mut fields = machine.fields.clone();
                fields["last_seen"] = json!(now());
                encoded(c.update(&machine, &machine.title, fields)?)
            }
            "machine.disable" => disable(c, q),
            "machine.enable" => {
                let record = read(c, "machine", req(q, "id")?)?;
                if record.fields["owner"] != c.actor {
                    c.require_admin()?;
                }
                let mut f = record.fields.clone();
                f["disabled"] = json!(false);
                encoded(c.update(&record, &record.title, f)?)
            }
            "run.tool" => authorize_run_tool(c, q),
            "run.discussion" => {
                let (run, expires, last) = lease_row(c, q)?;
                if expires <= now() || last.is_some() || run.fields["status"] != "running" {
                    return Err(crate::forbidden("an active run lease is required"));
                }
                crate::discussions::run(c, q, &run)
            }
            "run.enqueue" => manual_enqueue(c, q),
            "run.claim" => claim(c, q),
            "run.heartbeat" => heartbeat(c, q),
            "run.report" => report(c, q),
            "run.cancel" => {
                let r = read(c, "run", req(q, "id")?)?;
                encoded(cancel(c, &r)?)
            }
            "run.retry" => retry(c, q),
            "run.reply" => reply(c, q),
            _ => automation::dispatch(c, q),
        }
    })())
}

pub(crate) fn validate(c: &Context<'_>, before: Option<&Record>, item: &Record) -> Result<()> {
    let fields = &item.fields;
    crate::agents::validate(c, before, item)?;
    match item.kind.as_str() {
        "run" | "machine" => Err("managed record; use an execution or machine operation".into()),
        "agent" => {
            if let Some(parent) = fields["parent_id"].as_str()
                && fields["parent_space"].is_null()
            {
                let mut next = parent.to_owned();
                let mut seen = HashSet::from([item.id.clone()]);
                while !next.is_empty() {
                    if !seen.insert(next.clone()) || seen.len() > 32 {
                        return Err("cyclic or overly deep agent inheritance".into());
                    }
                    let value = read(c, "agent", &next)?;
                    next = value.fields["parent_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                }
            }
            for (field, kind) in [("skills", "skill"), ("tools", "tool")] {
                for id in array_ids(&fields[field])? {
                    read(c, kind, id)?;
                }
            }
            if let Some(id) = fields["machine_id"].as_str().filter(|s| !s.is_empty()) {
                read(c, "machine", id)?;
            }
            if let Some(mode) = fields["directory_mode"].as_str()
                && !matches!(mode, "direct" | "isolated")
            {
                return Err("unknown directory mode".into());
            }
            working_hours::WorkingHours::parse(&fields["working_hours"])?;
            for key in ["max_parallel", "chat_parallel"] {
                if let Some(n) = fields.get(key)
                    && !n.is_null()
                    && n.as_u64().filter(|n| (1..=64).contains(n)).is_none()
                {
                    return Err(format!("invalid {key}"));
                }
            }
            Ok(())
        }
        "crew" => {
            if let Some(id) = fields["leader_id"].as_str().filter(|id| !id.is_empty()) {
                read(c, "agent", id)?;
            }
            for id in array_ids(&fields["members"])? {
                read(c, "agent", id)?;
            }
            Ok(())
        }
        "automation" | "connector" => automation::validate(c, item),
        _ => Ok(()),
    }
}
pub(crate) fn allow_delete(c: &Context<'_>, r: &Record) -> Result<()> {
    match r.kind.as_str() {
        "run" | "machine" => Err("managed record; cancel a run or disable a machine".into()),
        "connector" => c.require_admin(),
        "task" | "chat"
            if c.records("run")?
                .iter()
                .any(|run| run.fields[&r.kind] == r.id && active(run)) =>
        {
            Err("cancel active runs before deleting their source".into())
        }
        "agent" | "crew" => {
            if r.kind=="agent" && c.db.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='agent' AND json_extract(fields,'$.parent_space')=? AND json_extract(fields,'$.parent_id')=?)",params![c.space,r.id],|row|row.get::<_,bool>(0)).map_err(db_error)?{return Err(crate::conflict("remove cross-space variants before deleting their original Agent"));}
            let used = c
                .records("task")?
                .iter()
                .any(|x| x.fields["assignee_kind"] == r.kind && x.fields["assignee"] == r.id)
                || c.records("automation")?
                    .iter()
                    .any(|x| x.fields["handler_id"] == r.id)
                || c.records("run")?
                    .iter()
                    .any(|x| x.fields["handler_id"] == r.id && active(x))
                || (r.kind == "agent"
                    && (c
                        .records("agent")?
                        .iter()
                        .any(|x| x.fields["parent_id"] == r.id)
                        || c.records("crew")?.iter().any(|x| {
                            x.fields["leader_id"] == r.id
                                || x.fields["members"]
                                    .as_array()
                                    .is_some_and(|a| a.contains(&json!(r.id)))
                        })));
            if used {
                Err(
                    "remove the handler from tasks, rules, variants and crews before deletion"
                        .into(),
                )
            } else {
                Ok(())
            }
        }
        "skill" | "tool" => {
            let field = if r.kind == "skill" { "skills" } else { "tools" };
            for agent in c.records("agent")? {
                let mut f = agent.fields.clone();
                if let Some(items) = f[field].as_array_mut() {
                    let old = items.len();
                    items.retain(|v| *v != r.id);
                    if old != items.len() {
                        c.update(&agent, &agent.title, f)?;
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn resolved(c: &Context<'_>, id: &str) -> Result<Value> {
    crate::agents::effective(c, id)
}

fn snapshot(c: &Context<'_>, handler: &str, kind: &str) -> Result<Value> {
    let (agent, crew) = if kind == "crew" {
        let crew = read(c, "crew", handler)?;
        (req(&crew.fields, "leader_id")?.to_owned(), Some(crew))
    } else if kind == "agent" {
        (handler.to_owned(), None)
    } else {
        return Err("handler must be agent or crew".into());
    };
    let mut data = resolved(c, &agent)?;
    data["agent_id"] = json!(agent);
    if let Some(crew) = crew {
        data["crew_members"] = crew.fields["members"].clone();
        data["crew_instruction"] = crew.fields["instructions"].clone();
    }
    for (field, target, dest) in [
        ("skills", "skill", "skill_versions"),
        ("tools", "tool", "tool_grants"),
    ] {
        let mut entries = Vec::new();
        let mut private = false;
        for id in array_ids(&data[field])? {
            let item = crate::agents::resource(c, &data, field, target, id)?;
            private |= item.fields["visibility"] == "private";
            if target == "tool"
                && let Some(doc) = item.fields["document"].as_str()
            {
                private |= crate::agents::resource(c, &data, field, "document", doc)?.fields["visibility"]
                    == "private";
            }
            let entry = if target == "skill" {
                json!({"id":id,"title":item.title,"rev":item.rev,"instructions":item.fields["instructions"]})
            } else {
                json!({"id":id,"title":item.title,"rev":item.rev,"type":item.fields["type"],"description":item.fields["description"]})
            };
            entries.push(entry);
        }
        data[dest] = json!(entries);
        if private {
            data["private_source"] = json!(true);
        }
    }
    Ok(data)
}

pub(crate) fn enqueue(
    c: &Context<'_>,
    origin: &str,
    handler: &str,
    kind: &str,
    title: &str,
    mut fields: Value,
) -> Result<Record> {
    if let Some(id) =
        c.db.query_row(
            "SELECT run FROM sf_run_keys WHERE space=? AND origin=?",
            params![c.space, origin],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?
    {
        return read(c, "run", &id);
    }
    crate::agents::usable(c, kind, handler, fields["parent_id"].as_str())?;
    let config = snapshot(c, handler, kind)?;
    let handler_record = read(c, kind, handler)?;
    if handler_record.fields["visibility"] == "private"
        || config["private_source"] == true
        || fields["visibility"] == "private"
        || fields["task"]["private_context"] == true
    {
        fields["visibility"] = json!("private");
        fields["owner"] = json!(c.actor);
    }
    if fields["task"].is_object() {
        fields["task_context"] = fields["task"].take();
    }
    fields["task"] = fields["task_id"].clone();
    fields["chat"] = fields["chat_id"].clone();
    fields["handler_id"] = json!(handler);
    fields["handler_kind"] = json!(kind);
    fields["runtime"] = config["runtime"].clone();
    fields["config"] = config;
    fields["status"] = json!("queued");
    fields["requested_by"] = json!(c.actor);
    fields["attempt"] = json!(0);
    let machines = c.records("machine")?;
    let local = machines
        .iter()
        .filter(|m| {
            m.fields["disabled"] != true
                && (m.fields["shared"] == true || m.fields["owner"] == c.actor)
                && fields["config"]["machine_id"]
                    .as_str()
                    .is_none_or(|id| id.is_empty() || id == m.id)
        })
        .collect::<Vec<_>>();
    let at = now();
    fields["waiting_reason"] = json!(if !within_hours(&fields["config"], at) {
        "outside_working_hours"
    } else if local.is_empty() {
        "no_machine"
    } else if !local.iter().any(|m| machine_is_online_at(&m.fields, at)) {
        "machine_offline"
    } else if !local.iter().any(|m| m.fields["runtimes"]
        .as_array()
        .is_some_and(|a| a.contains(&fields["runtime"])))
    {
        "runtime_unavailable"
    } else {
        "queued"
    });
    let run = c.create("run", None, title, fields)?;
    c.db.execute(
        "INSERT INTO sf_run_keys(space,origin,run) VALUES(?,?,?)",
        params![c.space, origin, run.id],
    )
    .map_err(db_error)?;
    Ok(run)
}
fn is_ready(c: &Context<'_>, task: &Record) -> Result<bool> {
    Ok(matches!(
        c.status_category(task)?.as_str(),
        "todo" | "in_progress" | "started" | "unstarted" | "active"
    ))
}
fn task_context(c: &Context<'_>, task: &Record) -> Result<Value> {
    let mut comments = c
        .records("comment")?
        .into_iter()
        .filter(|r| r.fields["task"] == task.id)
        .collect::<Vec<_>>();
    comments.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    let mut discussion = Vec::new();
    let mut private = false;
    for r in comments.into_iter().take(15) {
        if c.visible(&r)? {
            private |= r.fields["visibility"] == "private";
            discussion.push(json!({"title":r.title,"body":r.fields["body"]}));
        }
    }
    Ok(
        json!({"id":task.id,"title":task.title,"description":task.fields["description"],"status":task.fields["status"],"priority":task.fields["priority"],"rev":task.rev,"comments":discussion,"private_context":private}),
    )
}
fn manual_enqueue(c: &Context<'_>, q: &Value) -> Result<Value> {
    let handler = req(q, "handler_id")?;
    let kind = q["handler_kind"].as_str().unwrap_or("agent");
    let mut fields =
        json!({"instruction":q["instruction"],"task_id":q["task_id"],"chat_id":q["chat_id"]});
    let chat = q["chat_id"]
        .as_str()
        .map(|id| read(c, "chat", id))
        .transpose()?;
    if let Some(room) = &chat {
        crate::agents::chat_available(c, room)?;
    }
    if chat
        .as_ref()
        .is_some_and(|r| r.fields["visibility"] == "private")
    {
        fields["visibility"] = json!("private");
    }
    let title = if let Some(id) = q["task_id"].as_str() {
        let task = read(c, "task", id)?;
        if !is_ready(c, &task)? {
            return Err("task is not in an executable status".into());
        }
        fields["task"] = task_context(c, &task)?;
        task.title
    } else if let Some(chat) = chat {
        chat.title
    } else {
        req(q, "instruction")?.to_owned()
    };
    let origin = format!(
        "manual:{}:{}",
        c.actor,
        q["request_id"].as_str().unwrap_or(&token())
    );
    encoded(enqueue(c, &origin, handler, kind, &title, fields)?)
}
pub(crate) fn changed(c: &Context<'_>, before: Option<&Record>, item: &Record) -> Result<()> {
    if item.kind == "comment" {
        crate::agents::mentioned(c, before, item)?;
    }
    if item.kind == "task" {
        let handler = item.fields["assignee"].as_str().unwrap_or_default();
        let kind = item.fields["assignee_kind"].as_str().unwrap_or("member");
        let eligible =
            !handler.is_empty() && matches!(kind, "agent" | "crew") && is_ready(c, item)?;
        let prior_ready = match before {
            Some(b) => is_ready(c, b)?,
            None => false,
        };
        let reassigned = before.is_none_or(|b| {
            b.fields["assignee"] != item.fields["assignee"]
                || b.fields["assignee_kind"] != item.fields["assignee_kind"]
        });
        if before.is_some() && (!eligible || reassigned) {
            for r in c.records("run")?.into_iter().filter(|r| {
                r.fields["task_id"] == item.id
                    && r.fields["origin_kind"] == "assignment"
                    && active(r)
            }) {
                cancel(c, &r)?;
            }
        }
        if eligible
            && (reassigned || !prior_ready)
            && crate::agents::usable(c, kind, handler, None).is_ok()
        {
            enqueue(
                c,
                &format!("assignment:{}:{}", item.id, item.rev),
                handler,
                kind,
                &item.title,
                json!({"task_id":item.id,"task":task_context(c,item)?,"origin_kind":"assignment"}),
            )?;
        }
    }
    automation::changed(c, before, item)
}
pub(crate) fn cancel_space_runs(c: &Context<'_>) -> Result<()> {
    let mut cancelled = Vec::new();
    for run in c.records("run")?.into_iter().filter(active) {
        let mut fields = run.fields.clone();
        fields["status"] = json!("cancelled");
        fields["finished_at"] = json!(now());
        fields["result"] = json!("空间已停用");
        cancelled.push(c.update(&run, &run.title, fields)?);
    }
    c.db.execute(
        "UPDATE sf_run_leases SET expires=0 WHERE space=? AND response_hash IS NULL",
        params![c.space],
    )
    .map_err(db_error)?;
    // Complete all records before notifying parents, so a crew cannot queue a summary in this space.
    for run in cancelled {
        on_terminal(c, &run)?;
    }
    Ok(())
}
fn active(r: &Record) -> bool {
    matches!(
        r.fields["status"].as_str(),
        Some("queued" | "running" | "cancelling" | "waiting" | "waiting_children")
    )
}
pub(crate) fn cancel(c: &Context<'_>, r: &Record) -> Result<Record> {
    if !active(r) {
        return Ok(r.clone());
    }
    let mut f = r.fields.clone();
    f["status"] = json!(
        if matches!(f["status"].as_str(), Some("running" | "cancelling")) {
            "cancelling"
        } else {
            "cancelled"
        }
    );
    if f["status"] == "cancelled" {
        f["finished_at"] = json!(now());
    }
    let updated = c.update(r, &r.title, f)?;
    if r.fields["handler_kind"] == "crew" {
        for child in c
            .records("run")?
            .into_iter()
            .filter(|x| x.fields["parent_id"] == r.id && active(x))
        {
            cancel(c, &child)?;
        }
    }
    if updated.fields["status"] == "cancelled" {
        on_terminal(c, &updated)?;
    }
    Ok(updated)
}
fn retry(c: &Context<'_>, q: &Value) -> Result<Value> {
    let run = read(c, "run", req(q, "id")?)?;
    crate::agents::usable(
        c,
        req(&run.fields, "handler_kind")?,
        req(&run.fields, "handler_id")?,
        run.fields["parent_id"].as_str(),
    )?;
    if !matches!(
        run.fields["status"].as_str(),
        Some("failed" | "cancelled" | "interrupted")
    ) {
        return Err("only a terminal unsuccessful run can be retried".into());
    }
    let mut f = run.fields.clone();
    f["status"] = json!("queued");
    f["previous_result"] = f["result"].take();
    f["previous_artifacts"] = f["artifacts"].take();
    for key in ["finished_at", "machine_id", "waiting_reason"] {
        f.as_object_mut().unwrap().remove(key);
    }
    c.db.execute(
        "DELETE FROM sf_run_leases WHERE space=? AND run=?",
        params![c.space, run.id],
    )
    .map_err(db_error)?;
    encoded(c.update(&run, &run.title, f)?)
}
fn reply(c: &Context<'_>, q: &Value) -> Result<Value> {
    let r = read(c, "run", req(q, "id")?)?;
    if r.fields["status"] != "waiting" {
        return Err("run is not waiting for a reply".into());
    }
    let mut f = r.fields.clone();
    f["status"] = json!("queued");
    f["reply"] = json!({"actor":c.actor,"text":req(q,"text")?,"at":now()});
    encoded(c.update(&r, &r.title, f)?)
}

fn authenticate(c: &Context<'_>, q: &Value) -> Result<Record> {
    let id = req(q, "machine_id")?;
    let key = req(q, "machine_key")?;
    let credentials: Option<(String, String)> =
        c.db.query_row(
            "SELECT owner,key_hash FROM sf_machine_keys WHERE space=? AND machine=?",
            params![c.space, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    if credentials != Some((c.actor.to_owned(), digest(key))) {
        return Err("machine authentication failed".into());
    }
    let r = c.get("machine", id)?;
    if r.fields["disabled"] == true {
        return Err("machine has been disabled".into());
    }
    Ok(r)
}
pub(crate) fn machine_is_online_at(fields: &Value, at: i64) -> bool {
    fields["disabled"] != true
        && fields["last_seen"]
            .as_i64()
            .is_some_and(|seen| seen > at - 90_000)
}
fn register(c: &Context<'_>, q: &Value) -> Result<Value> {
    let id = req(q, "machine_id")?;
    let key = req(q, "machine_key")?;
    if !(32..=256).contains(&key.len()) {
        return Err("machine key must contain 32 to 256 characters".into());
    }
    let title = req(q, "title")?;
    let runtimes = array_ids(&q["runtimes"])?;
    let capacity = q["capacity"].as_u64().unwrap_or(1);
    if capacity == 0 || capacity > 64 {
        return Err("capacity must be 1–64".into());
    }
    let shared = q["shared"].as_bool().unwrap_or(false);
    if shared {
        c.require_admin()?;
    }
    let f = json!({"owner":c.actor,"shared":shared,"visibility":if shared{"space"}else{"private"},"runtimes":runtimes,"capacity":capacity,"last_seen":now(),"disabled":false});
    let r = if c.find("machine", id)?.is_some() {
        let prior = authenticate(c, q)?;
        c.update(&prior, title, f)?
    } else {
        let item = c.create("machine", Some(id), title, f)?;
        c.db.execute(
            "INSERT INTO sf_machine_keys(space,machine,owner,key_hash) VALUES(?,?,?,?)",
            params![c.space, id, c.actor, digest(key)],
        )
        .map_err(db_error)?;
        item
    };
    encoded(r)
}
fn disable(c: &Context<'_>, q: &Value) -> Result<Value> {
    let r = read(c, "machine", req(q, "id")?)?;
    if r.fields["owner"] != c.actor {
        c.require_admin()?;
    }
    let mut f = r.fields.clone();
    f["disabled"] = json!(true);
    for run in c.records("run")?.into_iter().filter(|x| {
        x.fields["machine_id"] == r.id
            && matches!(x.fields["status"].as_str(), Some("running" | "cancelling"))
    }) {
        let mut data = run.fields.clone();
        data["status"] = json!("interrupted");
        data["result"] = json!("机器已停用，重新执行前请核实先前的运行。");
        data["finished_at"] = json!(now());
        let done = c.update(&run, &run.title, data)?;
        on_terminal(c, &done)?;
    }
    c.db.execute(
        "UPDATE sf_run_leases SET expires=0 WHERE space=? AND machine=? AND response_hash IS NULL",
        params![c.space, r.id],
    )
    .map_err(db_error)?;
    encoded(c.update(&r, &r.title, f)?)
}
fn within_hours(config: &Value, at: i64) -> bool {
    working_hours::WorkingHours::parse(&config["working_hours"]).is_ok_and(|h| h.allows(at))
}

fn expire(c: &Context<'_>, runs: &[Record], at: i64) -> Result<()> {
    for r in runs
        .iter()
        .filter(|r| matches!(r.fields["status"].as_str(), Some("running" | "cancelling")))
    {
        let expires: Option<i64> =
            c.db.query_row(
                "SELECT expires FROM sf_run_leases WHERE space=? AND run=?",
                params![c.space, r.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if expires.is_none_or(|expiry| expiry <= at) {
            let mut f = r.fields.clone();
            f["status"] = json!(if f["status"] == "cancelling" {
                "cancelled"
            } else {
                "interrupted"
            });
            f["result"] = json!("机器租约已过期，请核对产物后重试。");
            f["finished_at"] = json!(at);
            let done = c.update(r, &r.title, f)?;
            on_terminal(c, &done)?;
        }
    }
    Ok(())
}
fn current_reason(
    run: &Record,
    machine: &Record,
    runs: &[Record],
    at: i64,
) -> Option<&'static str> {
    let cfg = &run.fields["config"];
    if run.fields["chat_id"].is_string()
        && run.fields["parent_id"].is_null()
        && runs.iter().any(|other| {
            other.id != run.id
                && other.fields["chat_id"] == run.fields["chat_id"]
                && matches!(
                    other.fields["status"].as_str(),
                    Some("running" | "cancelling" | "waiting_children")
                )
        })
    {
        return Some("conversation_busy");
    }
    if machine.fields["shared"] != true && machine.fields["owner"] != run.fields["requested_by"] {
        return Some("machine_visibility");
    }
    if cfg["machine_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty() && id != machine.id)
    {
        return Some("assigned_machine");
    }
    if !machine.fields["runtimes"]
        .as_array()
        .is_some_and(|rt| rt.contains(&cfg["runtime"]))
    {
        return Some("runtime_unavailable");
    }
    if !within_hours(cfg, at) {
        return Some("outside_working_hours");
    }
    let chat = run.fields["chat_id"].is_string();
    let max = cfg[if chat {
        "chat_parallel"
    } else {
        "max_parallel"
    }]
    .as_u64()
    .unwrap_or(1);
    let in_flight = runs
        .iter()
        .filter(|r| {
            r.fields["handler_id"] == run.fields["handler_id"]
                && r.fields["chat_id"].is_string() == chat
                && matches!(r.fields["status"].as_str(), Some("running" | "cancelling"))
        })
        .count() as u64;
    if in_flight >= max {
        return Some("agent_capacity");
    }
    None
}
fn claim(c: &Context<'_>, q: &Value) -> Result<Value> {
    let machine = authenticate(c, q)?;
    let at = now();
    let previous = c.records("run")?;
    expire(c, &previous, at)?;
    let mut runs = c.records("run")?;
    runs.sort_by_key(|r| {
        (
            r.created_at,
            r.fields["chat_order"].as_i64().unwrap_or(0),
            r.id.clone(),
        )
    });
    let capacity = machine.fields["capacity"].as_u64().unwrap_or(1);
    if runs
        .iter()
        .filter(|r| {
            r.fields["machine_id"] == machine.id
                && matches!(r.fields["status"].as_str(), Some("running" | "cancelling"))
        })
        .count() as u64
        >= capacity
    {
        return Ok(Value::Null);
    }
    for run in runs.iter().filter(|r| r.fields["status"] == "queued") {
        if !crate::agents::requester_can_continue(c, run)? {
            cancel(c, run)?;
            continue;
        }
        if !c.visible(run)? {
            continue;
        }
        if !crate::agents::machine_eligible(c, &machine, run)? {
            continue;
        }
        let mut f = run.fields.clone();
        if let Some(reason) = current_reason(run, &machine, &runs, at) {
            if (reason == "outside_working_hours"
                || reason == "conversation_busy"
                || reason == "agent_capacity"
                || f["config"]["machine_id"] == machine.id)
                && f["waiting_reason"] != reason
            {
                f["waiting_reason"] = json!(reason);
                c.update(run, &run.title, f)?;
            }
            continue;
        }
        let context = if run.fields["chat_id"].is_string() {
            match crate::chat::content_context(c, run) {
                Ok(value) => value,
                Err(error)
                    if error.starts_with("forbidden:") || error.starts_with("not_found:") =>
                {
                    f["status"] = json!("cancelled");
                    f["finished_at"] = json!(at);
                    f["result"] = json!("Conversation access changed");
                    c.update(run, &run.title, f)?;
                    continue;
                }
                Err(e) => return Err(e),
            }
        } else {
            Value::Null
        };
        if context["private"] == true {
            if c.actor != f["requested_by"].as_str().unwrap_or_default() {
                continue;
            }
            f["visibility"] = json!("private");
            f["owner"] = f["requested_by"].clone();
        }
        let lease = token();
        f["status"] = json!("running");
        f["attempt"] = json!(f["attempt"].as_u64().unwrap_or(0) + 1);
        f["machine_id"] = json!(machine.id);
        f["started_at"] = json!(at);
        f["waiting_reason"] = Value::Null;
        let mut current = c.update(run, &run.title, f)?;
        if !context.is_null() {
            current.fields["chat_context"] = context;
        }
        c.db.execute("INSERT INTO sf_run_leases(space,run,machine,lease_hash,expires,response_hash) VALUES(?,?,?,?,?,NULL) ON CONFLICT(space,run) DO UPDATE SET machine=excluded.machine,lease_hash=excluded.lease_hash,expires=excluded.expires,response_hash=NULL",
            params![c.space,run.id,machine.id,digest(&lease),at+LEASE_MS]).map_err(db_error)?;
        return Ok(json!({"run":current,"lease":lease,"expires_at":at+LEASE_MS}));
    }
    Ok(Value::Null)
}
fn lease_row(c: &Context<'_>, q: &Value) -> Result<(Record, i64, Option<String>)> {
    let machine = authenticate(c, q)?;
    let id = req(q, "id")?;
    let key = digest(req(q, "lease")?);
    let row:Option<(i64,Option<String>)>=c.db.query_row("SELECT expires,response_hash FROM sf_run_leases WHERE space=? AND run=? AND machine=? AND lease_hash=?",params![c.space,id,machine.id,key],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(db_error)?;
    let Some((expires, last)) = row else {
        return Err("stale lease".into());
    };
    Ok((c.get("run", id)?, expires, last))
}
fn heartbeat(c: &Context<'_>, q: &Value) -> Result<Value> {
    let (run, expires, last) = lease_row(c, q)?;
    let at = now();
    if expires <= at
        || last.is_some()
        || !matches!(
            run.fields["status"].as_str(),
            Some("running" | "cancelling")
        )
    {
        return Err("stale lease".into());
    }
    if let Some(value) = q.get("progress").filter(|v| !v.is_null()) {
        let mut fields = run.fields.clone();
        if apply_progress(&mut fields, value, at)? {
            c.update(&run, &run.title, fields)?;
        }
    }
    c.db.execute(
        "UPDATE sf_run_leases SET expires=? WHERE space=? AND run=?",
        params![at + LEASE_MS, c.space, run.id],
    )
    .map_err(db_error)?;
    Ok(json!({"cancel":run.fields["status"]=="cancelling","expires_at":at+LEASE_MS}))
}
fn apply_progress(fields: &mut Value, progress: &Value, at: i64) -> Result<bool> {
    let message = progress["message"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 2048)
        .ok_or("progress needs a message of 1–2048 bytes")?;
    let percent = &progress["percent"];
    if !percent.is_null() && !percent.as_u64().is_some_and(|p| p <= 100) {
        return Err("progress percent must be 0–100".into());
    }
    if fields["progress"]["message"] == message && fields["progress"]["percent"] == *percent {
        return Ok(false);
    }
    fields["progress"] = json!({"message":message,"percent":percent,"at":at});
    Ok(true)
}
fn authorize_run_tool(c: &Context<'_>, q: &Value) -> Result<Value> {
    let (run, expires, reported) = lease_row(c, q)?;
    if expires <= now() || reported.is_some() || run.fields["status"] != "running" {
        return Err("stale lease".into());
    }
    let tool = req(q, "tool_id")?;
    if !array_ids(&run.fields["config"]["tools"])?.contains(&tool) {
        return Err("forbidden: tool was not present in the accepted run configuration".into());
    }
    let owner = req(&run.fields, "requested_by")?;
    let actor = Context {
        db: c.db,
        space: c.space,
        actor: owner,
    };
    actor.role()?;
    let permitted = automation::dispatch(
        &actor,
        &json!({"op":"tool.authorize","id":run.fields["config"]["agent_id"],"tool_id":tool}),
    )?;
    let mut f = run.fields.clone();
    let mut calls = f["tool_calls"].as_array().cloned().unwrap_or_default();
    let call_id = req(q, "call_id")?;
    if let Some(old) = calls.iter().find(|x| x["id"] == call_id) {
        if old["tool"] != tool {
            return Err("tool call ID conflict".into());
        }
    } else {
        if calls.len() >= 128 {
            return Err("too many tool calls in one run".into());
        }
        calls.push(json!({"id":call_id,"tool":tool,"version":permitted["rev"],"at":now()}));
        f["tool_calls"] = json!(calls);
        c.update(&run, &run.title, f)?;
    }
    Ok(permitted)
}
fn report(c: &Context<'_>, q: &Value) -> Result<Value> {
    let (run, expires, last) = lease_row(c, q)?;
    let at = now();
    let mut fingerprint = vec![&q["status"], &q["result"], &q["artifacts"], &q["delegates"]];
    if let Some(usage) = q.get("usage").filter(|v| !v.is_null()) {
        fingerprint.push(usage);
    }
    if run.fields["chat_conversion"] == true {
        fingerprint.push(&q["task_draft"]);
    }
    if let Some(progress) = q.get("progress").filter(|v| !v.is_null()) {
        fingerprint.push(progress);
    }
    let body_digest = digest(&json!(fingerprint).to_string());
    if let Some(prior) = last {
        if prior != body_digest {
            return Err("lease was already reported with a different result".into());
        }
        return encoded(run);
    }
    if expires <= at
        || !matches!(
            run.fields["status"].as_str(),
            Some("running" | "cancelling")
        )
    {
        return Err("stale lease".into());
    }
    let private_result;
    let q = if crate::chat::claim_still_visible(c, &run)?
        && crate::agents::requester_can_continue(c, &run)?
    {
        q
    } else {
        private_result =
            json!({"status":"cancelled","result":"Conversation access changed","artifacts":[]});
        &private_result
    };
    let outcome = req(q, "status")?;
    if !matches!(outcome, "done" | "failed" | "cancelled" | "waiting") {
        return Err("unknown run outcome".into());
    }
    let text = q["result"].as_str().unwrap_or_default();
    if text.len() > 262_144 {
        return Err("result exceeds 256 KiB".into());
    }
    if !q["artifacts"].is_null() && !q["artifacts"].as_array().is_some_and(|xs| xs.len() <= 100) {
        return Err("invalid artifact list".into());
    }
    let mut f = run.fields.clone();
    f["status"] = json!(if f["status"] == "cancelling" {
        "cancelled"
    } else {
        outcome
    });
    if let Some(usage) = crate::agents::usage(q)? {
        f["usage"] = usage;
    }
    f["result"] = json!(text);
    if let Some(progress) = q.get("progress").filter(|v| !v.is_null()) {
        apply_progress(&mut f, progress, at)?;
    }
    let mut summary = q["artifacts"].clone();
    if let Some(items) = summary.as_array_mut() {
        for entry in items {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("content_base64");
            }
        }
    }
    f["artifacts"] = summary;
    if let Some(ds) = q["delegates"].as_array().filter(|v| !v.is_empty())
        && f["status"] == "done"
    {
        if run.fields["handler_kind"] != "crew" || run.fields["phase"] == "summary" || ds.len() > 32
        {
            return Err("delegation requires an initial crew run".into());
        }
        let allowed = array_ids(&run.fields["config"]["crew_members"])?;
        let requester = Context {
            db: c.db,
            space: c.space,
            actor: req(&run.fields, "requested_by")?,
        };
        requester.require_edit()?;
        let mut children = Vec::new();
        for (idx, d) in ds.iter().enumerate() {
            let target = req(d, "handler_id")?;
            if !allowed.contains(&target) {
                return Err("agent is not a member of this crew".into());
            }
            let instruction = req(d, "instruction")?;
            let context = json!({"instruction":instruction,"visibility":run.fields["visibility"],"parent_id":run.id,"task_id":run.fields["task_id"],"task":run.fields["task_context"],"chat_id":run.fields["chat_id"]});
            children.push(
                enqueue(
                    &requester,
                    &format!("delegate:{}:{idx}", run.id),
                    target,
                    "agent",
                    instruction,
                    context,
                )?
                .id,
            );
        }
        f["children"] = json!(children);
        f["status"] = json!("waiting_children");
    }
    crate::chat::apply_conversion(c, &run, q, &mut f)?;
    if matches!(f["status"].as_str(), Some("done" | "failed" | "cancelled")) {
        f["finished_at"] = json!(at);
    }
    let updated = c.update(&run, &run.title, f)?;
    record_artifacts(c, &updated, &q["artifacts"])?;
    if updated.fields["status"] == "waiting" {
        crate::inbox::task_run(c, &updated)?;
    }
    c.db.execute(
        "UPDATE sf_run_leases SET response_hash=?,expires=? WHERE space=? AND run=?",
        params![body_digest, at, c.space, run.id],
    )
    .map_err(db_error)?;
    if !active(&updated) {
        on_terminal(c, &updated)?;
    }
    encoded(updated)
}
fn record_artifacts(c: &Context<'_>, r: &Record, entries: &Value) -> Result<()> {
    if let Some(artifacts) = entries.as_array() {
        for entry in artifacts {
            let title = entry["name"]
                .as_str()
                .or(entry["path"].as_str())
                .or(entry.as_str())
                .unwrap_or("执行产物");
            let fields = json!({"run":r.id,"run_id":r.id,"task":r.fields["task_id"],"task_id":r.fields["task_id"],"chat":r.fields["chat_id"],"chat_id":r.fields["chat_id"],"machine":r.fields["machine_id"],"path":entry["location"].as_str().or(entry["path"].as_str()).or(entry.as_str()),"bytes":entry["bytes"],"creator":c.actor});
            let artifact = c.create("artifact", None, title, fields)?;
            if entry["content_base64"].is_string() {
                crate::attachments::append(c, &artifact, entry)?;
            }
        }
    }
    Ok(())
}
fn on_terminal(c: &Context<'_>, r: &Record) -> Result<()> {
    if let Some(chat) = r.fields["chat"].as_str() {
        let text = r.fields["result"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("执行已结束");
        c.create("message",None,&r.title,json!({"chat":chat,"run":r.id,"body":text,"visibility":r.fields["visibility"],"owner":r.fields["requested_by"],"author_kind":"agent","author":r.fields["handler_id"],"creator":r.fields["requested_by"]}))?;
    }
    if let Some(parent_id) = r.fields["parent_id"].as_str() {
        let parent = c.get("run", parent_id)?;
        if parent.fields["status"] == "waiting_children" {
            let mut results = Vec::new();
            for child in array_ids(&parent.fields["children"])? {
                let child = c.get("run", child)?;
                if active(&child) {
                    return Ok(());
                }
                results.push(json!({"id":child.id,"status":child.fields["status"],"result":child.fields["result"],"artifacts":child.fields["artifacts"]}));
            }
            let mut f = parent.fields.clone();
            f["child_results"] = json!(results);
            f["phase"] = json!("summary");
            f["status"] = json!("queued");
            c.update(&parent, &parent.title, f)?;
        }
    }
    automation::notify(c, r)
}
