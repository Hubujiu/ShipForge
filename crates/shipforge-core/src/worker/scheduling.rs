use super::*;
use crate::{as_value, conflict, invalid, storage};

pub(super) fn validate(f: &Value) -> Result<()> {
    for (key, low, high) in [
        ("queue_timeout_hours", 1, 168),
        ("run_timeout_minutes", 15, 43_200),
    ] {
        if f.get(key)
            .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| !(low..=high).contains(&n)))
        {
            return Err(invalid(&format!("{key} must be {low}–{high}")));
        }
    }
    if f.get("chat_capacity_mode")
        .is_some_and(|v| !v.is_null() && !matches!(v.as_str(), Some("independent" | "shared")))
    {
        return Err(invalid("unknown chat_capacity_mode"));
    }
    if let Some(triggers) = f["working_hours"].get("triggers")
        && triggers.as_object().is_none_or(|keys| {
            keys.iter().any(|(k, v)| {
                !matches!(k.as_str(), "assignment" | "comment" | "chat") || !v.is_boolean()
            })
        })
    {
        return Err(invalid(
            "working_hours.triggers accepts boolean assignment, comment and chat switches",
        ));
    }
    Ok(())
}
pub(super) fn parse_date(v: &Value) -> Result<Value> {
    if v.is_null() || v.as_str() == Some("") {
        return Ok(Value::Null);
    }
    if let Some(n) = v
        .as_i64()
        .filter(|n| chrono::DateTime::from_timestamp_millis(*n).is_some())
    {
        return Ok(json!(n));
    }
    if let Some(d) = v
        .as_str()
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
    {
        return Ok(json!(d.timestamp_millis()));
    }
    Err(invalid(
        "scheduled_at must be UTC milliseconds or an RFC 3339 timestamp with an offset",
    ))
}
fn trigger(f: &Value) -> &'static str {
    match f["origin_kind"].as_str() {
        Some("comment") => "comment",
        Some("chat") => "chat",
        _ if f["chat_id"].is_string() && f["origin_kind"] != "assignment" => "chat",
        _ => "assignment",
    }
}
fn schedule(f: &Value) -> Result<working_hours::WorkingHours> {
    let selection = trigger(f);
    let hours = &f["config"]["working_hours"];
    if f["trigger_now"] == true
        || !hours["triggers"][selection]
            .as_bool()
            .unwrap_or(selection != "chat")
    {
        return Ok(working_hours::WorkingHours::Always);
    }
    working_hours::WorkingHours::parse(hours)
}
pub(super) fn initialize(f: &mut Value, at: i64) -> Result<()> {
    let earliest = if f["trigger_now"] == true {
        at
    } else {
        f["scheduled_at"].as_i64().unwrap_or(at).max(at)
    };
    let start = schedule(f)?.next_start(earliest);
    f["queue_clock_at"] = json!(at);
    f["eligible_at"] = json!(start);
    f["next_start_at"] = json!(start);
    let timeout = f["config"]["queue_timeout_hours"]
        .as_i64()
        .unwrap_or(24)
        .clamp(1, 168)
        * 3_600_000;
    f["queue_deadline"] = json!(start.map(|s| s.saturating_add(timeout)));
    f["trigger_source"] = json!(trigger(f));
    Ok(())
}
pub(super) fn reset(f: &mut Value, at: i64) -> Result<()> {
    for k in [
        "trigger_now",
        "run_deadline",
        "stop_reason",
        "failure",
        "started_at",
        "scheduled_at",
        "assigned_directory",
        "isolated_workspace",
    ] {
        f.as_object_mut().unwrap().remove(k);
    }
    initialize(f, at)?;
    f["waiting_reason"] = json!(time_reason(f, at).unwrap_or("queued"));
    Ok(())
}
pub(super) fn time_reason(f: &Value, at: i64) -> Option<&'static str> {
    if f["trigger_now"] == true {
        return None;
    }
    if f["scheduled_at"].as_i64().is_some_and(|t| at < t) {
        return Some("scheduled_start");
    }
    if !schedule(f).is_ok_and(|s| s.allows(at)) {
        return Some("outside_working_hours");
    }
    None
}
pub(super) fn next_start(f: &Value, at: i64) -> Option<i64> {
    schedule(f)
        .ok()?
        .next_start(f["scheduled_at"].as_i64().unwrap_or(at).max(at))
}
pub(super) fn run_limit(f: &Value) -> u64 {
    f["config"]["run_timeout_minutes"]
        .as_u64()
        .map(|v| v.clamp(15, 43_200) * 60)
        .or_else(|| f["config"]["timeout_seconds"].as_u64())
        .unwrap_or(180 * 60)
        .clamp(1, 30 * 86400)
}
pub(super) fn has_timed_out(f: &Value, at: i64) -> bool {
    f["stop_reason"] == "run_timeout"
        || (f["status"] == "running" && f["run_deadline"].as_i64().is_some_and(|t| t <= at))
}
fn agent(f: &Value) -> &Value {
    f["config"].get("agent_id").unwrap_or(&f["handler_id"])
}
pub(super) fn capacity(run: &Record, machine: &Record, runs: &[Record]) -> Option<&'static str> {
    let f = &run.fields;
    let cfg = &f["config"];
    let chat = trigger(f) == "chat";
    let shared = cfg["chat_capacity_mode"] == "shared";
    let limit = cfg[if chat && !shared {
        "chat_parallel"
    } else {
        "max_parallel"
    }]
    .as_u64()
    .unwrap_or(1);
    let mut occupied = 0;
    for other in runs
        .iter()
        .map(|r| &r.fields)
        .filter(|o| matches!(o["status"].as_str(), Some("running" | "cancelling")))
    {
        if f["crew_workspace"]["token"].is_string()
            && other["crew_workspace"]["token"] == f["crew_workspace"]["token"]
            && other["machine_id"] == machine.id
        {
            return Some("crew_workspace_busy");
        }
        if agent(other) != agent(f) {
            continue;
        }
        if f["task_id"].is_string()
            && other["task_id"] == f["task_id"]
            && (f["chat_id"].is_null()
                || other["chat_id"].is_null()
                || f["crew_workspace"].is_object()
                || other["crew_workspace"].is_object())
        {
            return Some("task_busy");
        }
        if shared || (trigger(other) == "chat") == chat {
            occupied += 1;
        }
    }
    (occupied >= limit).then_some("agent_capacity")
}
pub(super) fn trigger_now(c: &Context<'_>, q: &Value) -> Result<Value> {
    let r = c.readable("run", req(q, "id")?)?;
    crate::domain::check_rev(q, &r)?;
    if r.fields["status"] != "queued" {
        return Err(conflict("only a queued run can start now"));
    }
    crate::agents::usable(
        c,
        req(&r.fields, "handler_kind")?,
        req(&r.fields, "handler_id")?,
        r.fields["parent_id"].as_str(),
    )?;
    if !crate::agents::requester_can_continue(c, &r)? {
        return Err(crate::forbidden("requester no longer has access"));
    }
    if r.fields["trigger_now"] == true {
        return Ok(as_value(r));
    }
    let at = now();
    if time_reason(&r.fields, at).is_none() {
        return Err(conflict("this run is already in the executable queue"));
    }
    let mut f = r.fields.clone();
    f["trigger_now"] = json!(true);
    initialize(&mut f, at)?;
    f["waiting_reason"] = json!("queued");
    Ok(as_value(c.update(&r, &r.title, f)?))
}

pub(crate) fn maintain_queue(db: &mut Connection, space: Option<&str>, at: i64) -> Result<()> {
    let sql="SELECT space,id FROM records WHERE kind='run' AND json_extract(fields,'$.status')='queued' AND json_extract(fields,'$.queue_deadline')<=?1 AND (?2 IS NULL OR space=?2)
        UNION ALL SELECT space,id FROM records WHERE kind='run' AND json_extract(fields,'$.status')='running' AND json_extract(fields,'$.run_deadline')<=?1 AND (?2 IS NULL OR space=?2)
        UNION ALL SELECT l.space,l.run FROM sf_run_leases l JOIN records r ON r.space=l.space AND r.kind='run' AND r.id=l.run WHERE l.response_hash IS NULL AND l.expires<=?1 AND (?2 IS NULL OR l.space=?2) AND json_extract(r.fields,'$.status') IN ('running','cancelling') LIMIT 256";
    let due = db
        .prepare_cached(sql)
        .map_err(storage)?
        .query_map(params![at, space], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if due.is_empty() {
        return Ok(());
    }
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(storage)?;
    for (space, id) in due {
        let c = Context {
            db: &tx,
            space: &space,
            actor: "owner",
        };
        let r = c.get("run", &id)?;
        if matches!(r.fields["status"].as_str(), Some("running" | "cancelling")) {
            let lease: Option<i64> =
                c.db.query_row(
                    "SELECT expires FROM sf_run_leases WHERE space=? AND run=?",
                    params![space, id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?;
            if lease.is_none_or(|t| t <= at) {
                expire(&c, std::slice::from_ref(&r), at)?;
            } else if r.fields["status"] == "running" && has_timed_out(&r.fields, at) {
                let mut f = r.fields.clone();
                f["status"] = json!("cancelling");
                f["stop_reason"] = json!("run_timeout");
                c.update(&r, &r.title, f)?;
            }
            continue;
        }
        if r.fields["status"] != "queued"
            || r.fields["queue_deadline"].as_i64().is_none_or(|t| t > at)
        {
            continue;
        }
        let mut f = r.fields.clone();
        f["status"] = json!("failed");
        f["waiting_reason"] = Value::Null;
        f["finished_at"] = json!(at);
        f["failure"] = json!({"category":"queue_timeout","message":"排队等待已超时"});
        f["result"] = json!("排队等待已超时");
        let done = c.update(&r, &r.title, f)?;
        on_terminal(&c, &done)?;
    }
    tx.commit().map_err(storage)
}
impl crate::Store {
    pub fn maintain_run_queue(&mut self) -> Result<()> {
        maintain_queue(&mut self.db, None, now())
    }
}
pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE INDEX IF NOT EXISTS sf_queue_deadlines ON records(json_extract(fields,'$.queue_deadline'),space) WHERE kind='run' AND json_extract(fields,'$.status')='queued'; CREATE INDEX IF NOT EXISTS sf_running_deadlines ON records(json_extract(fields,'$.run_deadline'),space) WHERE kind='run' AND json_extract(fields,'$.status')='running';").map_err(storage)?;
    let old=db.prepare("SELECT space,id,fields,created_at FROM records WHERE kind='run' AND json_extract(fields,'$.status')='queued' AND json_extract(fields,'$.queue_clock_at') IS NULL").map_err(storage)?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?))).map_err(storage)?.collect::<std::result::Result<Vec<_>,_>>().map_err(storage)?;
    for (space, id, raw, _) in old {
        let mut f: Value = serde_json::from_str(&raw).map_err(storage)?;
        initialize(&mut f, now())?;
        db.execute("UPDATE records SET fields=? WHERE space=? AND kind='run' AND id=? AND json_extract(fields,'$.status')='queued' AND json_extract(fields,'$.queue_clock_at') IS NULL",params![f.to_string(),space,id]).map_err(storage)?;
    }
    Ok(())
}
pub(super) fn schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"enum":["run.trigger_now","run.diagnostics","machine.directories","machine.diagnostics"]}}},"then":{"required":["id"]}}));
    rules.push(json!({"if":{"required":["kind"],"properties":{"op":{"enum":["create","update"]},"kind":{"const":"agent"}}},"then":{"properties":{"fields":{"properties":{"run_timeout_minutes":{"type":["integer","null"],"minimum":15,"maximum":43200},"queue_timeout_hours":{"type":["integer","null"],"minimum":1,"maximum":168},"chat_capacity_mode":{"enum":["independent","shared",null]},"profiles":{"type":["array","null"],"maxItems":16,"uniqueItems":true,"items":{"type":"string"}}}}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"run.enqueue"}}},"then":{"properties":{"scheduled_at":{"type":["integer","string","null"]}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"run.claim"}}},"then":{"properties":{"busy_directories":{"type":"array","maxItems":128,"items":{"type":"string","pattern":"^[a-f0-9]{64}$"}}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"machine.register"}}},"then":{"properties":{"directories":{"type":["array","null"],"maxItems":128,"items":{"type":"object","required":["id","identity"],"properties":{"id":{"type":"string","minLength":1,"maxLength":160},"identity":{"type":"string","pattern":"^[A-Fa-f0-9]{64}$"},"git":{"type":"boolean"},"available":{"type":"boolean"},"direct":{"type":"boolean"}}}}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"run.report"}}},"then":{"properties":{"error":{"type":["object","string","null"],"description":"Recognized provider code, HTTP status or sanitized worker classification"}}}}));
}
