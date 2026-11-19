use super::{array_ids, db_error, digest, encoded, enqueue, now, read, req};
use crate::{Context, Record, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::HashSet;
#[path = "automation/schedule.rs"]
mod schedule;
use schedule::Schedule;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_connector_objects(space TEXT NOT NULL,connector TEXT NOT NULL,external TEXT NOT NULL,task TEXT NOT NULL,base TEXT NOT NULL,source_version TEXT NOT NULL,PRIMARY KEY(space,connector,external),UNIQUE(space,connector,task));
    CREATE TABLE IF NOT EXISTS sf_connector_events(space TEXT NOT NULL,connector TEXT NOT NULL,event TEXT NOT NULL,payload TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(space,connector,event));
    CREATE TABLE IF NOT EXISTS sf_schedule_cursors(space TEXT NOT NULL,rule TEXT NOT NULL,slot TEXT NOT NULL,seen INTEGER NOT NULL,last_run TEXT,PRIMARY KEY(space,rule,slot));
    CREATE TABLE IF NOT EXISTS sf_automation_deliveries(space TEXT NOT NULL,rule TEXT NOT NULL,source TEXT NOT NULL,event TEXT NOT NULL,payload TEXT NOT NULL,run TEXT NOT NULL,PRIMARY KEY(space,rule,source,event));").map_err(db_error)
}
pub(super) fn validate(c: &Context<'_>, r: &Record) -> Result<()> {
    let f = &r.fields;
    if r.kind == "connector" {
        c.require_admin()?;
        crate::vcs::validate(f)?;
        crate::vendors::validate(c, r)?;
        crate::task_commands::source_members::validate_map(c, f)?;
        if f.get("slack_assignment")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(crate::invalid("slack_assignment must be a boolean"));
        }
        if f["slack_assignment"] == true
            && (f["provider"] != "slack"
                || f["outbound"] != true
                || !f["assignee_map"].as_object().is_some_and(|map| {
                    !map.is_empty()
                        && map.keys().all(|id| {
                            id.starts_with(['U', 'W'])
                                && id.len() <= 160
                                && id.bytes().all(|b| b.is_ascii_alphanumeric())
                        })
                }))
        {
            return Err(crate::invalid(
                "Slack assignment requires outbound messages and a mapping from Slack user IDs to space members",
            ));
        }
        if f["link_tasks"] == true
            && let Some(id) = f["review_rule_id"].as_str().filter(|s| !s.is_empty())
        {
            read(c, "automation", id)?;
        }
        if f["provider"] == "slack" && f["outbound"] == true {
            for field in ["workspace_id", "channel_id", "app_id", "bot_id"] {
                let id = crate::required(f, field)?;
                if id.len() > 160 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err(crate::invalid("Slack outbound requires scoped Slack IDs"));
                }
            }
        }
        if f["provider"] == "github" {
            let repo = req(f, "repository")?;
            let parts = repo.split('/').collect::<Vec<_>>();
            if parts.len() != 2
                || parts.iter().any(|p| {
                    p.is_empty()
                        || *p == "."
                        || *p == ".."
                        || !p
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                })
            {
                return Err("expected GitHub owner/repository".into());
            }
        }
        return Ok(());
    }
    let enabled = f["enabled"].as_bool().unwrap_or(false);
    if !enabled && f["handler_id"].is_null() {
        return Ok(());
    }
    let handler = req(f, "handler_id")?;
    let kind = f["handler_kind"].as_str().unwrap_or("agent");
    if !matches!(kind, "agent" | "crew") {
        return Err("automation requires an agent or crew".into());
    }
    read(c, kind, handler)?;
    if enabled {
        crate::agents::usable(c, kind, handler, None)?;
        req(f, "instruction")?;
    }
    for contact in array_ids(&f["notify"])? {
        read(c, "member", contact)?;
    }
    if let Some(triggers) = f.get("triggers") {
        let mut schedules = HashSet::new();
        for trigger in triggers
            .as_array()
            .filter(|v| v.len() <= 32)
            .ok_or("expected up to 32 triggers")?
        {
            match req(trigger, "type")? {
                "status" | "comment" | "webhook" => {}
                "schedule" => {
                    let schedule = Schedule::parse(trigger)?;
                    if !schedules.insert(schedule.signature(trigger)) {
                        return Err("duplicate schedule; use different trigger IDs to configure identical times".into());
                    }
                }
                _ => return Err("unknown trigger type".into()),
            }
        }
    }
    Ok(())
}
pub(super) fn dispatch(c: &Context<'_>, q: &Value) -> Result<Value> {
    match q["op"].as_str() {
        Some("tool.authorize") => {
            let agent = req(q, "id")?;
            let tool = req(q, "tool_id")?;
            let config = super::resolved(c, agent)?;
            if !array_ids(&config["tools"])?.contains(&tool) {
                return Err("forbidden: tool is not granted to this agent".into());
            }
            let t = crate::agents::resource(c, &config, "tools", "tool", tool)?;
            if t.fields["enabled"] == false {
                return Err("forbidden: tool has been disabled".into());
            }
            if let Some(doc) = t.fields["document"].as_str() {
                let d = crate::agents::resource(c, &config, "tools", "document", doc)?;
                return Ok(
                    json!({"tool":t.id,"type":"document","document":d.id,"title":d.title,"body":d.fields["body"],"rev":d.rev}),
                );
            }
            Ok(json!({"tool":t.id,"type":t.fields["type"],"title":t.title,"rev":t.rev}))
        }
        Some("automation.fire") => {
            let rule = read(c, "automation", req(q, "id")?)?;
            let event = req(q, "event_id")?;
            if event.len() > 256 || event.chars().any(char::is_control) {
                return Err("invalid automation event ID".into());
            }
            let source = q["source"].as_str().unwrap_or("manual");
            if rule.fields["enabled"] != true {
                return Err("automation is disabled".into());
            }
            if source == "webhook"
                && !enabled_triggers(&rule).any(|(_, trigger)| trigger["type"] == "webhook")
            {
                return Err("no enabled webhook trigger".into());
            }
            if !matches!(source, "manual" | "webhook") {
                return Err("unknown automation source".into());
            }
            let task = q["task_id"]
                .as_str()
                .map(|id| read(c, "task", id))
                .transpose()?;
            let payload = &q["payload"];
            if !payload.is_null() && (!payload.is_object() || payload.to_string().len() > 131072) {
                return Err("webhook payload must be an object under 128 KiB".into());
            }
            let content = digest(&json!([q["task_id"], payload]).to_string());
            let receipt:Option<(String,String)>=c.db.query_row("SELECT payload,run FROM sf_automation_deliveries WHERE space=? AND rule=? AND source=? AND event=?",params![c.space,rule.id,source,event],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(db_error)?;
            if let Some((delivered, id)) = receipt {
                if delivered != content {
                    return Err(
                        "conflict: automation event ID was reused with different content".into(),
                    );
                }
                return encoded(read(c, "run", &id)?);
            }
            let run = fire(
                c,
                &rule,
                &format!("{source}:{event}"),
                source,
                task.as_ref(),
                json!({"webhook_payload":payload}),
            )?;
            c.db.execute("INSERT INTO sf_automation_deliveries(space,rule,source,event,payload,run) VALUES(?,?,?,?,?,?)",params![c.space,rule.id,source,event,content,run.id]).map_err(db_error)?;
            encoded(run)
        }
        Some("automation.tick") => tick(c, q),
        Some("connector.ingest") => ingest(c, q),
        Some("connector.export") => export(c, q),
        _ => Err("unknown automation operation".into()),
    }
}
fn enabled_triggers(r: &Record) -> impl Iterator<Item = (usize, &Value)> {
    r.fields["triggers"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, t)| t["enabled"] != false)
}
fn fire(
    c: &Context<'_>,
    rule: &Record,
    event: &str,
    source: &str,
    task: Option<&Record>,
    extra: Value,
) -> Result<Record> {
    let handler = req(&rule.fields, "handler_id")?;
    let kind = rule.fields["handler_kind"].as_str().unwrap_or("agent");
    let mut fields = json!({"origin_kind":"automation","automation_id":rule.id,"instruction":rule.fields["instruction"],"source":source});
    if let Some(values) = extra.as_object() {
        fields.as_object_mut().unwrap().extend(values.clone());
    }
    if rule.fields["visibility"] == "private" {
        fields["visibility"] = json!("private");
    }
    if let Some(t) = task {
        fields["task_id"] = json!(t.id);
        fields["task"] = super::task_context(c, t)?;
    }
    enqueue(
        c,
        &format!("rule:{}:{event}", rule.id),
        handler,
        kind,
        &rule.title,
        fields,
    )
}
pub(super) fn changed(c: &Context<'_>, before: Option<&Record>, item: &Record) -> Result<()> {
    if item.kind == "automation" {
        return reset_schedules(c, before, item);
    }
    let (trigger_kind, task) = match item.kind.as_str() {
        "task" if before.is_some_and(|b| b.fields["status"] != item.fields["status"]) => {
            ("status", item.clone())
        }
        "comment" if before.is_none() => {
            let id = req(&item.fields, "task")?;
            ("comment", c.get("task", id)?)
        }
        _ => return Ok(()),
    };
    for rule in c
        .records("automation")?
        .into_iter()
        .filter(|r| r.fields["enabled"] == true)
    {
        let Some(owner) = rule.fields["creator"].as_str() else {
            continue;
        };
        let engine = Context {
            db: c.db,
            space: c.space,
            actor: owner,
        };
        if engine.require_edit().is_err() || !engine.visible(&task)? || !engine.visible(&rule)? {
            continue;
        }
        if crate::agents::usable(
            &engine,
            rule.fields["handler_kind"].as_str().unwrap_or("agent"),
            req(&rule.fields, "handler_id")?,
            None,
        )
        .is_err()
        {
            continue;
        }
        for (idx, t) in enabled_triggers(&rule) {
            if t["type"] != trigger_kind {
                continue;
            }
            if trigger_kind == "status"
                && t["status"].as_str().is_some_and(|s| {
                    !s.is_empty() && s != task.fields["status"].as_str().unwrap_or_default()
                })
            {
                continue;
            }
            if trigger_kind == "comment"
                && t["keyword"]
                    .as_str()
                    .is_some_and(|s| !item.fields["body"].as_str().unwrap_or_default().contains(s))
            {
                continue;
            }
            fire(
                &engine,
                &rule,
                &format!("{trigger_kind}:{}:{}:{idx}", item.id, item.rev),
                trigger_kind,
                Some(&task),
                Value::Null,
            )?;
        }
    }
    Ok(())
}
fn schedules(r: &Record) -> Result<Vec<(usize, String, Schedule)>> {
    if r.fields["enabled"] != true {
        return Ok(Vec::new());
    }
    enabled_triggers(r)
        .filter(|(_, t)| t["type"] == "schedule")
        .map(|(idx, t)| {
            let schedule = Schedule::parse(t)?;
            Ok((idx, schedule.signature(t), schedule))
        })
        .collect()
}
fn reset_schedules(c: &Context<'_>, before: Option<&Record>, r: &Record) -> Result<()> {
    let prior = before.map(schedules).transpose()?.unwrap_or_default();
    let current = schedules(r)?;
    for (_, key, _) in &prior {
        if !current.iter().any(|(_, k, _)| k == key) {
            c.db.execute(
                "DELETE FROM sf_schedule_cursors WHERE space=? AND rule=? AND slot=?",
                params![c.space, r.id, key],
            )
            .map_err(db_error)?;
        }
    }
    for (_, key, schedule) in current {
        if !prior.iter().any(|(_, k, _)| k == &key) {
            c.db.execute("INSERT INTO sf_schedule_cursors(space,rule,slot,seen,last_run) VALUES(?,?,?,?,NULL) ON CONFLICT(space,rule,slot) DO UPDATE SET seen=excluded.seen,last_run=NULL",params![c.space,r.id,key,schedule.start_cursor(r.updated_at)]).map_err(db_error)?;
        }
    }
    Ok(())
}
fn tick(c: &Context<'_>, q: &Value) -> Result<Value> {
    if q.get("machine_id").is_some() {
        super::authenticate(c, q)?;
    } else {
        c.require_admin()?;
    }
    tick_at(c, now())
}
fn tick_at(c: &Context<'_>, timestamp: i64) -> Result<Value> {
    let mut results = Vec::new();
    for rule in c
        .records("automation")?
        .into_iter()
        .filter(|r| r.fields["enabled"] == true)
    {
        let Some(owner) = rule.fields["creator"].as_str() else {
            continue;
        };
        let engine = Context {
            db: c.db,
            space: c.space,
            actor: owner,
        };
        if engine.require_edit().is_err()
            || crate::agents::usable(
                &engine,
                rule.fields["handler_kind"].as_str().unwrap_or("agent"),
                req(&rule.fields, "handler_id")?,
                None,
            )
            .is_err()
        {
            continue;
        }
        for (idx, key, schedule) in schedules(&rule)? {
            let cursor:Option<(i64,Option<String>)>=c.db.query_row("SELECT seen,last_run FROM sf_schedule_cursors WHERE space=? AND rule=? AND slot=?",params![c.space,rule.id,key],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(db_error)?;
            let (seen, mut last_id) =
                cursor.unwrap_or_else(|| (schedule.start_cursor(rule.updated_at), None));
            let due = schedule.occurrences(seen, timestamp, idx, &key)?;
            if due.is_empty() {
                if let Some(id) = &last_id
                    && let Some(run) = c.find("run", id)?
                    && c.visible(&run)?
                {
                    results.push(run.id);
                }
                continue;
            }
            for occurrence in &due {
                let run = fire(
                    &engine,
                    &rule,
                    &occurrence.event,
                    "schedule",
                    None,
                    json!({"scheduled_for":occurrence.at}),
                )?;
                if c.visible(&run)? {
                    results.push(run.id.clone());
                }
                last_id = Some(run.id);
            }
            let processed = if due.len() == schedule::BATCH {
                due.last().unwrap().at
            } else {
                timestamp
            };
            c.db.execute("INSERT INTO sf_schedule_cursors(space,rule,slot,seen,last_run) VALUES(?,?,?,?,?) ON CONFLICT(space,rule,slot) DO UPDATE SET seen=excluded.seen,last_run=excluded.last_run",params![c.space,rule.id,key,processed,last_id]).map_err(db_error)?;
        }
    }
    Ok(json!({"runs":results}))
}
pub(super) fn notify(c: &Context<'_>, run: &Record) -> Result<()> {
    if run.fields["automation_id"].is_null() && run.fields["task_id"].is_string() {
        return crate::inbox::task_run(c, run);
    }
    let mut recipients = Vec::new();
    if let Some(id) = run.fields["automation_id"].as_str() {
        if let Some(rule) = c.find("automation", id)? {
            recipients.extend(
                array_ids(&rule.fields["notify"])?
                    .into_iter()
                    .map(str::to_owned),
            );
        }
    } else if let Some(actor) = run.fields["requested_by"].as_str() {
        recipients.push(actor.to_owned());
    }
    for recipient in recipients {
        let receiver = Context {
            db: c.db,
            space: c.space,
            actor: &recipient,
        };
        let event = if run.fields["automation_id"].is_string() {
            "automation_result"
        } else if run.fields["status"] == "failed" {
            "run_failed"
        } else {
            "run_done"
        };
        if receiver.role().is_err() || !receiver.visible(run)? {
            continue;
        }
        // Task runs also emit the task notification through inbox::task_run.
        if run.fields["task_id"].is_null() {
            crate::inbox::deliver_mail(c, &recipient, event, ("run", &run.id), None)?;
        }
        if !crate::inbox::channel_enabled(c, &recipient, event)? {
            continue;
        }
        let group = run.fields["automation_id"]
            .as_str()
            .or(run.fields["task_id"].as_str())
            .unwrap_or(&run.id);
        let id = format!("run-{}", digest(&format!("{recipient}:{group}")));
        let mut fields = json!({"recipient":recipient,"run":run.id,"task":run.fields["task_id"],"automation":run.fields["automation_id"],"state":"inbox","importance":if run.fields["status"]=="done"{"other"}else{"important"},"status":run.fields["status"],"subject":group,"count":1,"read":false});
        fields["event"] = json!(event);
        let notice = if let Some(old) = c.find("notification", &id)? {
            fields["count"] = json!(old.fields["count"].as_i64().unwrap_or(1) + 1);
            if old.fields["state"] == "snoozed"
                && old.fields["until"].as_i64().is_some_and(|t| t > now())
            {
                fields["state"] = json!("snoozed");
                fields["until"] = old.fields["until"].clone();
            }
            c.update(&old, &run.title, fields)?
        } else {
            c.create("notification", Some(&id), &run.title, fields)?
        };
        crate::inbox::notify_record(c, &notice, event, Some(("run", &run.id)))?;
    }
    Ok(())
}

pub(super) fn catalog() -> Value {
    json!([
        {"id":"change-summary","kind":"skill","title":"变更摘要","description":"把讨论和产物整理为可核对的交付说明。","fields":{"version":"1","instructions":"逐项说明已完成的变化、验证方式、产物位置和需要人工确认的问题。仅引用实际观察到的结果。"}},
        {"id":"acceptance-check","kind":"skill","title":"验收检查","description":"将任务描述整理成带验证结果的检查清单。","fields":{"version":"1","instructions":"从任务描述提取验收条件。逐项列出依据和实际结果，把尚未验证的项目单列。"}},
        {"id":"documentation-agent","kind":"agent","title":"文档助手","description":"整理团队已有资料，编写用户及维护者说明。","fields":{"runtime":"local-tool","instructions":"根据提供的任务与资料编写可独立阅读的说明。在交付中标出示例和未核对的信息。","skills":[]}},
        {"id":"delivery-agent","kind":"agent","title":"交付检查员","description":"汇总执行结果和交付风险，协助负责人验收。","fields":{"runtime":"local-tool","instructions":"对照任务的验收条件和已有运行记录，列出已通过的检查、还缺少的依据以及交付物。","skills":[]}}
    ])
}
fn mapped_task(c: &Context<'_>, record: &Record) -> Result<Value> {
    Ok(
        json!({"title":record.title,"description":record.fields["description"].as_str().unwrap_or_default(),"closed":matches!(c.status_category(record)?.as_str(),"done"|"cancelled"|"duplicate"|"completed"|"canceled")}),
    )
}
fn ingest(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_admin()?;
    let connector = read(c, "connector", req(q, "id")?)?;
    if crate::vendors::uses_project(&connector.fields) {
        return Err(crate::invalid(
            "use the bound project's native tracker sync or provider event",
        ));
    }
    if connector.fields["enabled"] == false {
        return Err("connector is disabled".into());
    }
    let external = req(q, "external_id")?;
    let event = req(q, "event_id")?;
    let source_version = req(q, "source_version")?;
    let title = req(q, "title")?;
    let description = q["description"].as_str().unwrap_or_default();
    if description.len() > 1_048_576 {
        return Err("external description exceeds 1 MiB".into());
    }
    let mut incoming = json!({"title":title,"description":description,"closed":q["closed"].as_bool().unwrap_or(false)});
    if let Some(level) = q.get("priority") {
        if !matches!(
            level.as_str(),
            Some("urgent" | "high" | "medium" | "low" | "lowest" | "none")
        ) {
            return Err(crate::invalid("invalid source priority"));
        }
        incoming["priority"] = level.clone();
    }
    for key in ["assignees", "source_assignees"] {
        if let Some(value) = q.get(key) {
            crate::domain::strings(value, key)?;
            incoming[key] = value.clone();
        }
    }
    let payload = digest(&json!([external, source_version, incoming]).to_string());
    let previous:Option<(String,String)>=c.db.query_row("SELECT payload,response FROM sf_connector_events WHERE space=? AND connector=? AND event=?",params![c.space,connector.id,event],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
    if let Some((original, response)) = previous {
        if original != payload {
            return Err("event_id was reused with different content".into());
        }
        let receipt: Value = serde_json::from_str(&response).map_err(|e| e.to_string())?;
        read(c, "task", req(&receipt, "task")?)?;
        return Ok(receipt);
    }
    let base:Option<(String,String,String)>=c.db.query_row("SELECT task,base,source_version FROM sf_connector_objects WHERE space=? AND connector=? AND external=?",params![c.space,connector.id,external],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
    let response = if let Some((task_id, base, previous_version)) = base {
        let task = read(c, "task", &task_id)?;
        let base: Value = serde_json::from_str(&base).map_err(|e| e.to_string())?;
        if source_version < previous_version.as_str()
            || (source_version == previous_version && incoming == base && q["resolve"].is_null())
        {
            json!({"action":"unchanged","task":task.id,"external_id":external})
        } else {
            let mut local = mapped_task(c, &task)?;
            let mut base = base;
            if incoming.get("priority").is_some() {
                local["priority"] = task.fields["priority"].as_str().unwrap_or("none").into();
                if base.get("priority").is_none() {
                    base["priority"] = json!("none");
                }
            }
            for key in ["assignees", "source_assignees"] {
                if incoming.get(key).is_some() {
                    local[key] = task.fields.get(key).cloned().unwrap_or_else(|| json!([]));
                    if base.get(key).is_none() {
                        base[key] = json!([]);
                    }
                }
            }
            let mut merged = local.clone();
            let mut conflicts = Vec::new();
            for key in [
                "title",
                "description",
                "closed",
                "priority",
                "assignees",
                "source_assignees",
            ] {
                if incoming.get(key).is_none() || incoming[key] == base[key] {
                    continue;
                }
                if incoming[key] != local[key] && local[key] != base[key] {
                    if q["resolve"] == "local" {
                        continue;
                    }
                    if q["resolve"] != "source" {
                        conflicts.push(key);
                    }
                }
                merged[key] = incoming[key].clone();
            }
            if !conflicts.is_empty() {
                return Ok(
                    json!({"action":"conflict","task":task.id,"fields":conflicts,"local":local,"source":incoming,"source_version":source_version}),
                );
            }
            let mut patch = json!({"description":merged["description"]});
            if merged["priority"] != local["priority"] {
                patch["priority"] = merged["priority"].clone();
            }
            for key in ["assignees", "source_assignees"] {
                if let Some(value) = merged.get(key) {
                    patch[key] = value.clone();
                }
            }
            if merged["closed"] != local["closed"] {
                patch["status"] = json!(if merged["closed"] == true {
                    "done"
                } else {
                    "todo"
                });
            }
            if merged != local {
                crate::domain::write_import(
                    c,
                    &json!({"op":"update","kind":"task","id":task.id,"expected_rev":task.rev,"title":merged["title"],"fields":patch}),
                    true,
                )?;
            }
            for (key, value) in incoming.as_object().unwrap() {
                base[key] = value.clone();
            }
            c.db.execute("UPDATE sf_connector_objects SET base=?,source_version=? WHERE space=? AND connector=? AND external=?",params![base.to_string(),source_version,c.space,connector.id,external]).map_err(db_error)?;
            json!({"action":if merged==local{"unchanged"}else{"updated"},"task":task.id,"external_id":external})
        }
    } else {
        let mut record = json!({"description":description,"external_id":external,"connector":connector.id,"source":"integration"});
        for key in ["assignees", "source_assignees", "priority"] {
            if let Some(value) = incoming.get(key) {
                record[key] = value.clone();
            }
        }
        if incoming["closed"] == true {
            record["status"] = json!("done");
        }
        let task = crate::domain::write_import(
            c,
            &json!({"op":"create","kind":"task","title":title,"fields":record}),
            true,
        )?;
        let tid = req(&task, "id")?;
        c.db.execute("INSERT INTO sf_connector_objects(space,connector,external,task,base,source_version) VALUES(?,?,?,?,?,?)",params![c.space,connector.id,external,tid,incoming.to_string(),source_version]).map_err(db_error)?;
        json!({"action":"created","task":tid,"external_id":external})
    };
    c.db.execute(
        "INSERT INTO sf_connector_events(space,connector,event,payload,response) VALUES(?,?,?,?,?)",
        params![c.space, connector.id, event, payload, response.to_string()],
    )
    .map_err(db_error)?;
    Ok(response)
}
fn export(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_admin()?;
    let connector = read(c, "connector", req(q, "id")?)?;
    let task = read(c, "task", req(q, "task_id")?)?;
    let (external,source_version):(String,String)=c.db.query_row("SELECT external,source_version FROM sf_connector_objects WHERE space=? AND connector=? AND task=?",params![c.space,connector.id,task.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"no external mapping for this task".to_owned())?;
    Ok(
        json!({"external_id":external,"source_version":source_version,"value":mapped_task(c,&task)?,"connector":connector.id}),
    )
}
