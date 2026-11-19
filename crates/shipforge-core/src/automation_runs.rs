//! A rule owns its history. Each execution has a private conversation belonging to its designated member.
use super::{automation, encoded};
use crate::{Context, Record, Result, conflict, forbidden, invalid, required, storage};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

pub(super) fn init(c: &rusqlite::Connection) -> Result<()> {
    c.execute_batch("CREATE INDEX IF NOT EXISTS sf_rule_runs ON event_log(space,json_extract(snapshot,'$.fields.automation_id'),seq DESC) WHERE kind='run' AND action='create';
    CREATE TABLE IF NOT EXISTS sf_rule_action_receipts(space TEXT NOT NULL,actor TEXT NOT NULL,key TEXT NOT NULL,hash TEXT NOT NULL,PRIMARY KEY(space,actor,key));").map_err(storage)
}
pub(super) fn active(rule: &Record) -> bool {
    rule.fields["archived"] != true && rule.fields["enabled"] == true
}
fn state(rule: &Record) -> &'static str {
    if rule.fields["archived"] == true {
        "archived"
    } else if active(rule) {
        "enabled"
    } else {
        "paused"
    }
}

pub(super) fn validate(c: &Context<'_>, f: &Value) -> Result<()> {
    if f.get("archived").is_some_and(|v| !v.is_boolean())
        || (f["archived"] == true && f["enabled"] == true)
    {
        return Err(invalid("an archived automation cannot be enabled"));
    }
    if let Some(id) = f["project"].as_str().filter(|s| !s.is_empty()) {
        c.readable("project", id)?;
    }
    for value in std::iter::once(&f["run_owner"]).chain(
        f["triggers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| &t["run_owner"]),
    ) {
        if value.is_null() {
            continue;
        }
        let owner = value
            .as_str()
            .ok_or_else(|| invalid("run_owner must name a space member or triggerer"))?;
        if !matches!(owner, "" | "triggerer") {
            c.readable("member", owner)?;
            if (Context { actor: owner, ..*c }).require_edit().is_err() {
                return Err(invalid("run owner must be a current space editor"));
            }
        }
    }
    Ok(())
}

pub(super) fn eligible(c: &Context<'_>, rule: &Record, task: Option<&Record>) -> Result<bool> {
    Ok(c.require_edit().is_ok()
        && c.visible(rule)?
        && project_visible(c, &rule.fields)
        && task.map(|t| c.visible(t)).transpose()?.unwrap_or(true)
        && crate::agents::usable(
            c,
            rule.fields["handler_kind"].as_str().unwrap_or("agent"),
            required(&rule.fields, "handler_id")?,
            None,
        )
        .is_ok())
}
fn project_visible(c: &Context<'_>, f: &Value) -> bool {
    f["project"]
        .as_str()
        .filter(|s| !s.is_empty())
        .is_none_or(|id| c.readable("project", id).is_ok())
}
pub(crate) fn room_visible(c: &Context<'_>, room: &Record) -> Result<bool> {
    let Some(rule) = room.fields["automation_rule"].as_str() else {
        return Ok(true);
    };
    if room.fields["owner"] != c.actor
        || c.role().is_err()
        || c.readable("automation", rule).is_err()
        || !project_visible(c, &room.fields)
    {
        return Ok(false);
    }
    let source = c.get("run", required(&room.fields, "automation_run")?)?;
    Ok(source.fields["source"] != "comment"
        || source.fields["trigger_record"]
            .as_str()
            .is_none_or(|id| c.readable("comment", id).is_ok()))
}
pub(crate) fn run_visible(c: &Context<'_>, r: &Record) -> Result<bool> {
    let Some(rule) = r.fields["automation_id"].as_str() else {
        return Ok(true);
    };
    if r.fields["requested_by"] != c.actor
        || c.role().is_err()
        || c.readable("automation", rule).is_err()
        || !project_visible(c, &r.fields)
    {
        return Ok(false);
    }
    Ok(r.fields["source"] != "comment"
        || r.fields["trigger_record"]
            .as_str()
            .is_none_or(|id| c.readable("comment", id).is_ok()))
}
pub(super) fn continuation(c: &Context<'_>, room: &Record, fields: &mut Value) -> Result<()> {
    if let Some(id) = room.fields["automation_run"].as_str() {
        let run = c.readable("run", id)?;
        let mut context = json!({"rule":run.fields["automation_id"],"original_run":run.id});
        for k in [
            "instruction",
            "source",
            "trigger_event",
            "trigger_actor",
            "trigger_id",
            "trigger_record",
            "from_status",
            "to_status",
            "scheduled_for",
            "project",
            "task_id",
            "webhook_payload",
        ] {
            if let Some(value) = run.fields.get(k) {
                context[k] = value.clone();
            }
        }
        fields["automation_context"] = context;
    }
    Ok(())
}
pub(crate) fn owned(c: &Context<'_>, r: &Record) -> Result<()> {
    c.require_edit()?;
    if r.fields["requested_by"] != c.actor || !c.visible(r)? || !project_visible(c, &r.fields) {
        return Err(forbidden("automation run owner required"));
    }
    c.readable("automation", required(&r.fields, "automation_id")?)?;
    Ok(())
}

pub(super) fn link(c: &Context<'_>, run: &Record) -> Result<Record> {
    if run.fields["chat_id"].is_string() {
        return Ok(run.clone());
    }
    let f = &run.fields;
    let who = required(f, "requested_by")?;
    let owner = Context { actor: who, ..*c };
    let room=owner.create("chat",None,&run.title,json!({"visibility":"private","chat_type":"conversation","creator":who,"owner":who,"origin_kind":"automation","automation_rule":f["automation_id"],"automation_run":run.id,"project":f["project"],"handler_kind":f["handler_kind"],"handler_id":f["handler_id"],"agent":if f["handler_kind"]=="agent"{f["handler_id"].clone()}else{Value::Null},"task":f["task_id"]}))?;
    let prompt = f["instruction"].as_str().unwrap_or(&run.title);
    let message=owner.create("message",None,&run.title,json!({"chat":room.id,"creator":who,"author":f["trigger_actor"],"author_kind":"automation","body":prompt,"source":f["source"],"event":f["trigger_event"]}))?;
    let mut next = f.clone();
    next["chat_id"] = json!(room.id);
    next["chat"] = json!(room.id);
    next["chat_message"] = json!(message.id);
    next["visibility"] = json!("private");
    next["owner"] = json!(who);
    next["unattended"] = json!(true);
    if matches!(
        f["status"].as_str(),
        Some("done" | "failed" | "cancelled" | "interrupted" | "skipped")
    ) {
        let body = f["result"]
            .as_str()
            .unwrap_or("This execution has no recorded output.");
        owner.create("message",None,&run.title,json!({"chat":room.id,"run":run.id,"creator":who,"author":f["handler_id"],"author_kind":"agent","body":body}))?;
    }
    c.update(run, &run.title, next)
}

pub(super) fn skip(
    c: &Context<'_>,
    rule: &Record,
    event: &str,
    source: &str,
    task: &Record,
    owner: &str,
    mut fields: Value,
) -> Result<()> {
    let key = format!("rule:{}:{event}", rule.id);
    if c.db
        .query_row(
            "SELECT 1 FROM sf_run_keys WHERE space=? AND origin=?",
            params![c.space, key],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage)?
        .is_some()
    {
        return Ok(());
    }
    fields.as_object_mut().unwrap().extend(json!({"status":"skipped","origin_kind":"automation","automation_id":rule.id,"source":source,"trigger_event":event,"requested_by":owner,"visibility":"private","owner":owner,"unattended":true,"task_id":task.id,"project":rule.fields["project"],"handler_id":rule.fields["handler_id"],"handler_kind":rule.fields["handler_kind"].as_str().unwrap_or("agent"),"finished_at":crate::now(),"result":"The designated owner cannot access the automation, executor, or triggering context.","skip_reason":"owner_access"}).as_object().unwrap().clone());
    let r = c.create("run", None, &rule.title, fields)?;
    c.db.execute(
        "INSERT INTO sf_run_keys(space,origin,run) VALUES(?,?,?)",
        params![c.space, key, r.id],
    )
    .map_err(storage)?;
    automation::notify(c, &r)
}

pub(super) fn unattended(f: &Value) -> bool {
    f["unattended"] == true
        || f["automation_id"].is_string()
        || f["handler_kind"] == crate::task_assistants::KIND
}
pub(super) fn stop_scheduled(c: &Context<'_>, rule: &Record) -> Result<()> {
    let mut query=c.db.prepare_cached("SELECT id FROM records WHERE space=? AND kind='run' AND json_extract(fields,'$.automation_id')=? AND json_extract(fields,'$.source')='schedule' AND json_extract(fields,'$.status')='queued'").map_err(storage)?;
    let ids = query
        .query_map(params![c.space, rule.id], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for id in ids {
        let run = c.get("run", &id)?;
        let mut fields = run.fields.clone();
        fields["status"] = json!("skipped");
        fields["finished_at"] = json!(crate::now());
        fields["skip_reason"] = json!(if rule.fields["archived"] == true {
            "rule_archived"
        } else {
            "rule_paused"
        });
        fields["result"] = json!("The automation was stopped before this scheduled run started.");
        let stopped = c.update(&run, &run.title, fields)?;
        super::on_terminal(c, &stopped)?;
    }
    Ok(())
}
pub(super) fn asks_for_input(q: &Value) -> bool {
    crate::runtime_diagnostics::requests_input(q)
}
pub(super) fn input_failure() -> Value {
    json!({"status":"failed","result":"This unattended run requested a human answer or approval. Adjust its instructions or continue in an attended conversation.","error":{"code":"unattended_input"},"artifacts":[]})
}

fn observer(c: &Context<'_>, r: &Record) -> Result<bool> {
    let rule = c.readable("automation", required(&r.fields, "automation_id")?)?;
    if !project_visible(c, &r.fields)
        || r.fields["task_id"]
            .as_str()
            .is_some_and(|id| c.readable("task", id).is_err())
    {
        return Ok(false);
    }
    if let Some(id) = r.fields["trigger_record"].as_str()
        && r.fields["source"] == "comment"
        && c.readable("comment", id).is_err()
    {
        return Ok(false);
    }
    Ok(c.visible(r)?
        || rule.fields["creator"] == c.actor
        || rule.fields["notify"]
            .as_array()
            .is_some_and(|list| list.iter().any(|id| id == c.actor)))
}
pub(super) fn summary_allowed(c: &Context<'_>, r: &Record) -> Result<bool> {
    if c.role().is_err()
        || r.fields["automation_id"]
            .as_str()
            .is_none_or(|id| c.readable("automation", id).is_err())
    {
        return Ok(false);
    }
    observer(c, r)
}
fn row(c: &Context<'_>, r: &Record) -> Result<Value> {
    let f = &r.fields;
    let owner = f["requested_by"] == c.actor && c.require_edit().is_ok() && c.visible(r)?;
    let mut v = json!({"id":r.id,"title":r.title,"created_at":r.created_at,"updated_at":r.updated_at,"status":f["status"],"source":f["source"],"finished_at":f["finished_at"],"owner":f["requested_by"],"attempt":f["attempt"],"can_open":owner,"can_retry":owner && retryable(f) && can_retry(c,r).is_ok()});
    if owner {
        for k in [
            "trigger_id",
            "trigger_index",
            "trigger_event",
            "trigger_actor",
            "trigger_record",
            "scheduled_for",
            "from_status",
            "to_status",
            "task_id",
            "project",
            "waiting_reason",
            "failure",
            "skip_reason",
            "chat_id",
        ] {
            v[k] = f[k].clone();
        }
    }
    Ok(v)
}
fn history(c: &Context<'_>, rule: &Record, q: &Value, health: bool) -> Result<Value> {
    let limit = q
        .get("limit")
        .map(|v| {
            v.as_i64()
                .filter(|i| (1..=100).contains(i))
                .ok_or_else(|| invalid("limit must be 1 to 100"))
        })
        .transpose()?
        .unwrap_or(25) as usize;
    let anchor=q.get("cursor").map(|v|{
        let r=c.readable("automation",&rule.id)?;
        let last=c.find("run",v.as_str().ok_or_else(||invalid("invalid history cursor"))?)?.ok_or_else(||invalid("invalid history cursor"))?;
        if last.fields["automation_id"]!=r.id || !observer(c,&last)?{return Err(invalid("cursor is outside the accessible rule history"));}
        c.db.query_row("SELECT seq FROM event_log WHERE space=? AND kind='run' AND id=? AND action='create' ORDER BY seq LIMIT 1",params![c.space,last.id],|r|r.get::<_,i64>(0)).map_err(storage)
    }).transpose()?;
    let mut stmt=c.db.prepare_cached("SELECT e.id FROM event_log e JOIN records r ON r.space=e.space AND r.kind='run' AND r.id=e.id WHERE e.space=?1 AND e.kind='run' AND e.action='create' AND json_extract(e.snapshot,'$.fields.automation_id')=?2 AND e.seq<?3 ORDER BY e.seq DESC").map_err(storage)?;
    let ids = stmt
        .query_map(params![c.space, rule.id, anchor.unwrap_or(i64::MAX)], |r| {
            r.get::<_, String>(0)
        })
        .map_err(storage)?;
    let mut results = Vec::new();
    let mut more = false;
    for id in ids {
        let rec = c.get("run", &id.map_err(storage)?)?;
        if !observer(c, &rec)? {
            continue;
        }
        if health {
            let latest = row(c, &rec)?;
            let label = match rec.fields["status"].as_str() {
                Some("failed" | "skipped" | "cancelled" | "interrupted") => "attention",
                Some("running" | "waiting_children" | "cancelling") => "running",
                Some("queued" | "waiting") => "pending",
                _ => "healthy",
            };
            return Ok(
                json!({"id":rule.id,"state":state(rule),"rev":rule.rev,"health":label,"latest":latest}),
            );
        }
        if results.len() == limit {
            more = true;
            break;
        }
        results.push(row(c, &rec)?);
    }
    if health {
        return Ok(
            json!({"id":rule.id,"state":state(rule),"rev":rule.rev,"health":"no_runs","latest":null}),
        );
    }
    let cursor = if more {
        results.last().map(|v| &v["id"])
    } else {
        None
    };
    Ok(json!({"id":rule.id,"items":results,"next_cursor":cursor,"state":state(rule)}))
}
fn retryable(f: &Value) -> bool {
    matches!(
        f["status"].as_str(),
        Some("failed" | "cancelled" | "interrupted")
    )
}
pub(super) fn can_retry(c: &Context<'_>, run: &Record) -> Result<()> {
    owned(c, run)?;
    let rule = c.readable("automation", required(&run.fields, "automation_id")?)?;
    if !active(&rule) {
        return Err(conflict("resume the rule before retrying a historical run"));
    }
    crate::agents::usable(
        c,
        required(&run.fields, "handler_kind")?,
        required(&run.fields, "handler_id")?,
        None,
    )?;
    if !retryable(&run.fields) {
        return Err(conflict(
            "only failed, cancelled or interrupted runs may be retried",
        ));
    }
    if let Some(id) = run.fields["trigger_record"].as_str()
        && run.fields["source"] == "comment"
    {
        c.readable("comment", id)?;
    }
    Ok(())
}
pub(super) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let rule = c.readable("automation", required(q, "id")?)?;
    let key = q
        .get("request_id")
        .filter(|_| matches!(op, "automation.lifecycle" | "automation.retry"))
        .map(|_| required(q, "request_id"))
        .transpose()?;
    if let Some(key) = key {
        crate::identifier(key)?;
        let hash = super::digest(&q.to_string());
        let prior: Option<String> =
            c.db.query_row(
                "SELECT hash FROM sf_rule_action_receipts WHERE space=? AND actor=? AND key=?",
                params![c.space, c.actor, key],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(prior) = prior {
            if prior != hash {
                return Err(conflict(
                    "request ID was already used for a different automation action",
                ));
            }
            replay(c, op, q, &Value::Null)?;
            return if op == "automation.lifecycle" {
                encoded(rule)
            } else {
                encoded(c.readable("run", required(q, "run")?)?)
            };
        }
        c.db.execute(
            "INSERT INTO sf_rule_action_receipts VALUES(?,?,?,?)",
            params![c.space, c.actor, key, hash],
        )
        .map_err(storage)?;
    }
    match op {
        "automation.history" | "automation.status" => history(c, &rule, q, op.ends_with(".status")),
        "automation.lifecycle" => {
            c.authorize_write("automation", Some(&rule))?;
            if q["expected_rev"].as_i64() != Some(rule.rev) {
                return Err(conflict("refresh the automation before changing its state"));
            }
            let target = match (required(q, "action")?, state(&rule)) {
                ("pause", "enabled") => "paused",
                ("resume", "paused") => "enabled",
                ("archive", "enabled" | "paused") => "archived",
                _ => return Err(conflict("invalid automation state transition")),
            };
            let mut next = rule.clone();
            next.fields["enabled"] = json!(target == "enabled");
            next.fields["archived"] = json!(target == "archived");
            if target == "enabled" {
                automation::validate(c, &next)?;
            }
            let updated = c.update(&rule, &rule.title, next.fields)?;
            automation::changed(c, Some(&rule), &updated)?;
            encoded(updated)
        }
        "automation.conversation" | "automation.retry" => {
            let r = c.readable("run", required(q, "run")?)?;
            if r.fields["automation_id"] != rule.id {
                return Err(invalid("run belongs to a different automation"));
            }
            owned(c, &r)?;
            if op == "automation.retry" {
                can_retry(c, &r)?;
            }
            let r = link(c, &r)?;
            if op == "automation.retry" {
                super::retry(c, &json!({"id":r.id}))
            } else {
                encoded(c.readable("chat", required(&r.fields, "chat_id")?)?)
            }
        }
        _ => Err(invalid("unknown automation history operation")),
    }
}
pub(crate) fn replay(c: &Context<'_>, op: &str, q: &Value, result: &Value) -> Result<()> {
    let rule = c.readable("automation", required(q, "id")?)?;
    if op == "automation.lifecycle" {
        return c.authorize_write("automation", Some(&rule));
    }
    let r = c.readable("run", required(q, "run")?)?;
    owned(c, &r)?;
    if r.fields["automation_id"] != rule.id {
        return Err(forbidden("run belongs to a different rule"));
    }
    if op == "automation.conversation" {
        c.readable("chat", required(result, "id")?)?;
    }
    Ok(())
}
pub(super) fn schema(s: &mut Value) {
    let rows = s["allOf"].as_array_mut().unwrap();
    rows.extend([
        json!({"if":{"properties":{"op":{"enum":["automation.status","automation.history"]}}},"then":{"required":["id"],"properties":{"cursor":{"type":"string","minLength":1},"limit":{"type":"integer","minimum":1,"maximum":100}}}}),
        json!({"if":{"properties":{"op":{"const":"automation.lifecycle"}}},"then":{"required":["id","action","expected_rev"],"properties":{"action":{"enum":["pause","resume","archive"]}}}}),
        json!({"if":{"properties":{"op":{"enum":["automation.conversation","automation.retry"]}}},"then":{"required":["id","run"],"properties":{"run":{"type":"string","minLength":1}}}}),
        json!({"if":{"properties":{"op":{"enum":["create","update"]},"kind":{"const":"automation"}},"required":["kind"]},"then":{"properties":{"fields":{"properties":{"run_owner":{"type":["string","null"]},"project":{"type":["string","null"]},"archived":{"type":"boolean"}}}}}})
    ]);
}
