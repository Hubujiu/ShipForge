use super::*;
use std::collections::BTreeSet;
pub(crate) mod intake;
mod rules;
mod schema;
pub(crate) use schema::extend as schema;

pub(crate) fn stamp(c: &Context<'_>, old: Option<&Record>, r: &mut Record) -> Result<bool> {
    if r.kind != "task" || c.status_category(r)? != "triage" {
        return Ok(false);
    }
    if let Some(prev) = old
        && c.status_category(prev)? == "triage"
    {
        return Ok(false);
    }
    r.fields["triage_entered_at"] = json!(now());
    r.fields.as_object_mut().unwrap().remove("deferred_until");
    for key in [
        "triage_decision",
        "triage_rule_failed",
        "triage_applied_rules",
    ] {
        r.fields.as_object_mut().unwrap().remove(key);
    }
    c.db.execute(
        "DELETE FROM triage_duplicates WHERE space=? AND task=?",
        params![c.space, r.id],
    )
    .map_err(storage)?;
    Ok(true)
}
pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS triage_policy(space TEXT PRIMARY KEY,actor TEXT NOT NULL,policy TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS triage_conversions(space TEXT NOT NULL,request TEXT NOT NULL,target_space TEXT NOT NULL,task TEXT NOT NULL,PRIMARY KEY(space,request));
        CREATE TABLE IF NOT EXISTS triage_duplicates(space TEXT NOT NULL,task TEXT NOT NULL,target_space TEXT NOT NULL,target TEXT NOT NULL,PRIMARY KEY(space,task));").map_err(storage)
}
fn policy(c: &Context<'_>) -> Result<Option<(String, Value)>> {
    let source: Option<(String, String)> =
        c.db.query_row(
            "SELECT actor,policy FROM triage_policy WHERE space=?",
            [c.space],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    source
        .map(|(actor, s)| Ok((actor, serde_json::from_str(&s).map_err(storage)?)))
        .transpose()
}
fn duplicate(c: &Context<'_>, id: &str) -> Result<Option<(String, String)>> {
    c.db.query_row(
        "SELECT target_space,target FROM triage_duplicates WHERE space=? AND task=?",
        params![c.space, id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(storage)
}
pub(crate) fn moved(c: &Context<'_>, id: &str, to: &str) -> Result<()> {
    for sql in [
        "UPDATE triage_duplicates SET space=?3 WHERE space=?1 AND task=?2",
        "UPDATE triage_duplicates SET target_space=?3 WHERE target_space=?1 AND target=?2",
        "UPDATE triage_conversions SET target_space=?3 WHERE target_space=?1 AND task=?2",
    ] {
        c.db.execute(sql, params![c.space, id, to])
            .map_err(storage)?;
    }
    Ok(())
}
pub(crate) fn forget(c: &Context<'_>, id: &str) -> Result<()> {
    c.db.execute(
        "DELETE FROM triage_conversions WHERE target_space=?1 AND task=?2",
        params![c.space, id],
    )
    .map_err(storage)?;
    c.db.execute("DELETE FROM triage_duplicates WHERE (space=?1 AND task=?2) OR (target_space=?1 AND target=?2)",params![c.space,id]).map_err(storage)?;
    Ok(())
}
pub(crate) fn redact(c: &Context<'_>, data: &mut Value) -> Result<()> {
    if let Some(rows) = data.as_array_mut() {
        for row in rows {
            redact(c, row)?;
        }
        return Ok(());
    }
    if data["kind"] == "space"
        && let Some(id) = data["id"].as_str()
    {
        let target = Context { space: id, ..*c };
        if !matches!(
            target.role().as_deref(),
            Ok("owner" | "admin" | "member" | "editor" | "viewer")
        ) && let Some(f) = data["fields"].as_object_mut()
        {
            f.remove("triage_policy");
            f.remove("triage_configured_by");
        }
    }
    if data["kind"] == "task"
        && data["fields"]["triage_decision"] == "duplicate"
        && let (Some(space), Some(id)) = (data["space"].as_str(), data["id"].as_str())
        && let Some((dest, task)) = duplicate(&Context { space, ..*c }, id)?
    {
        let target = Context { space: &dest, ..*c };
        if target.role().is_ok() && target.readable("task", &task).is_ok() {
            data["fields"]["triage_duplicate_of"] = json!({"space":dest,"task":task});
        }
    }
    Ok(())
}
pub(crate) fn protect_space(p: &serde_json::Map<String, Value>) -> Result<()> {
    if p.contains_key("triage_policy") || p.contains_key("triage_configured_by") {
        return Err(invalid(
            "use triage.configure to change the duty reviewer or rules",
        ));
    }
    Ok(())
}
fn enabled(c: &Context<'_>) -> Result<Record> {
    let space = c.get("space", c.space)?;
    if space.fields["triage_enabled"] != true {
        return Err(invalid("triage is disabled for this space"));
    }
    Ok(space)
}
pub(crate) fn settings(c: &Context<'_>) -> Result<Value> {
    c.metadata_role()?;
    let space = c.get("space", c.space)?;
    let policy = policy(c)?
        .map(|(_, p)| p)
        .unwrap_or_else(|| json!({"duty":{"mode":"none"},"rules":[]}));
    let live = matches!(
        c.role().as_deref(),
        Ok("owner" | "admin" | "member" | "editor" | "viewer")
    );
    Ok(
        json!({"space":c.space,"rev":space.rev,"enabled":space.fields["triage_enabled"]==true,"priority_required":space.fields["triage_priority_required"]==true,"sources":space.fields["triage_sources"],"types":space.fields["triage_types"],
        "duty":if live{policy["duty"].clone()}else{Value::Null},"rules":if live{policy["rules"].clone()}else{Value::Null},"conflicts":if live{rules::conflicts(&policy["rules"])}else{vec![]},
        "can_configure":matches!(c.metadata_role()?.as_str(),"owner"|"admin")&&space.fields["deactivated"]!=true}),
    )
}
pub(crate) fn configure(c: &Context<'_>, q: &Value) -> Result<Value> {
    if !matches!(c.metadata_role()?.as_str(), "owner" | "admin") {
        return Err(forbidden("space or organization administrator required"));
    }
    let space = c.get("space", c.space)?;
    crate::domain::check_rev(q, &space)?;
    let mut f = space.fields.clone();
    let mut policy = policy(c)?
        .map(|(_, p)| p)
        .unwrap_or_else(|| json!({"duty":{"mode":"none"},"rules":[]}));
    for (k, target) in [
        ("enabled", "triage_enabled"),
        ("priority_required", "triage_priority_required"),
        ("sources", "triage_sources"),
        ("types", "triage_types"),
    ] {
        if let Some(v) = q.get(k) {
            if v.is_null() && matches!(k, "types" | "sources") {
                f.as_object_mut().unwrap().remove(target);
            } else {
                f[target] = v.clone();
            }
        }
    }
    for k in ["duty", "rules"] {
        if let Some(v) = q.get(k) {
            policy[k] = v.clone();
        }
    }
    if c.role().is_err() && (q.get("duty").is_some() || q.get("rules").is_some()) {
        return Err(forbidden(
            "join the space to manage member-based triage rules",
        ));
    }
    if q.get("duty").is_some() || q.get("rules").is_some() {
        rules::validate(c, &policy)?;
        c.db.execute("INSERT INTO triage_policy(space,actor,policy) VALUES (?,?,?) ON CONFLICT(space) DO UPDATE SET actor=excluded.actor,policy=excluded.policy",params![c.space,c.actor,policy.to_string()]).map_err(storage)?;
    }
    if let Some(types) = q.get("types").filter(|v| !v.is_null()) {
        for id in crate::domain::strings(types, "types")? {
            if c.find("task_type", id)?.is_none() {
                return Err(invalid("unknown triage task type"));
            }
        }
    }
    if space.fields["triage_enabled"] == true
        && f["triage_enabled"] == false
        && q["confirm"] != true
    {
        return Err(conflict(
            "confirm moving pending tasks to backlog before disabling triage",
        ));
    }

    crate::domain::validate_space(&f)?;
    let updated = c.update(&space, &space.title, f)?;
    crate::actions::after(c, Some(&space), &updated)?;
    settings(c)
}
fn effective_status(c: &Context<'_>, task: &Record) -> Result<String> {
    let default = crate::defaults::selected(c, "status", "todo")?;
    let ty = c.find("task_type", required(&task.fields, "type")?)?;
    let allowed = ty
        .as_ref()
        .and_then(|t| t.fields["statuses"].as_array())
        .filter(|a| !a.is_empty());
    let usable = |id: &str| {
        c.category_for(id)
            .is_ok_and(|s| !matches!(s.as_str(), "triage" | "done" | "cancelled" | "duplicate"))
    };
    if let Some(id) = ty
        .as_ref()
        .and_then(|ty| ty.fields["default_status"].as_str())
        && usable(id)
        && allowed.is_none_or(|ls| ls.iter().any(|v| v == id))
    {
        return Ok(id.to_owned());
    }
    if usable(&default) && allowed.is_none_or(|ls| ls.iter().any(|v| v == &default)) {
        return Ok(default);
    }
    if let Some(list) = allowed {
        return list
            .iter()
            .filter_map(Value::as_str)
            .find(|id| usable(id))
            .map(str::to_owned)
            .ok_or_else(|| invalid("task type has no normal working status"));
    }
    crate::tasks::target_status(c, task, "todo")
}
pub(crate) fn decision(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    enabled(c)?;
    let task = c.readable("task", required(q, "id")?)?;
    crate::domain::check_rev(q, &task)?;
    if c.status_category(&task)? != "triage" {
        return Err(invalid("task is outside triage"));
    }
    let choice = required(q, "decision")?;
    let mut f = json!({"triage_decision":choice,"deferred_until":null});
    match choice {
        "accept" => {
            f["status"] = json!(effective_status(c, &task)?);
            for key in [
                "priority",
                "assignees",
                "executor",
                "labels",
                "project",
                "start_date",
                "due_date",
            ] {
                if let Some(v) = q.get(key) {
                    f[key] = v.clone();
                }
            }
        }
        "reject" => {
            f["status"] = json!(crate::tasks::target_status(c, &task, "cancelled")?);
        }
        "duplicate" => {
            let target = required(q, "target")?;
            let (scope, id) =
                intake::duplicate_target(c, target, q.get("target_space").and_then(Value::as_str))?;
            if scope == c.space && id == task.id {
                return Err(invalid("select a different task"));
            }
            let dest = Context {
                space: &scope,
                ..*c
            };
            let linked = dest.readable("task", &id)?;
            if duplicate(&dest, &linked.id)?.is_some() {
                return Err(invalid("choose the original task rather than a duplicate"));
            }
            let custom = c.find("task_type", required(&task.fields, "type")?)?;
            let list = custom
                .as_ref()
                .and_then(|t| t.fields["statuses"].as_array());
            let status = list
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find(|s| c.category_for(s).ok().as_deref() == Some("duplicate"))
                .unwrap_or("duplicate");
            f["status"] = json!(status);
            c.db.execute("INSERT INTO triage_duplicates(space,task,target_space,target) VALUES(?,?,?,?) ON CONFLICT(space,task) DO UPDATE SET target_space=excluded.target_space,target=excluded.target",params![c.space,task.id,scope,id]).map_err(storage)?;
            if scope == c.space {
                write_request(
                    c,
                    &json!({"op":"create","kind":"relation","title":"重复任务","fields":{"from":task.id,"to":id,"type":"duplicate"}}),
                )?;
            }
        }
        "defer" => {
            let until = q["until"]
                .as_i64()
                .filter(|v| *v > now())
                .ok_or_else(|| invalid("until must be a future Unix timestamp in milliseconds"))?;
            f["deferred_until"] = json!(until);
        }
        "restore" => {}
        _ => return Err(invalid("unknown triage decision")),
    }
    let result = crate::domain::write_review(
        c,
        &json!({"op":"update","kind":"task","id":task.id,"fields":f}),
    )?;
    if matches!(choice, "accept" | "reject")
        && let Some(body) = q
            .get(if choice == "reject" {
                "reason"
            } else {
                "comment"
            })
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        write_request(
            c,
            &json!({"op":"create","kind":"comment","title":body.chars().take(80).collect::<String>(),"fields":{"task":task.id,"body":body}}),
        )?;
    }
    if choice == "accept" && q["subscribe"] == true {
        crate::actions::execute(
            c,
            &json!({"op":"follow","id":task.id,"enabled":true}),
            "follow",
        )?;
    }
    Ok(result)
}
fn bulk(c: &Context<'_>, q: &Value) -> Result<Value> {
    if !matches!(q["decision"].as_str(), Some("accept" | "reject" | "defer")) {
        return Err(invalid("invalid bulk decision"));
    }
    let tasks = crate::domain::strings(&q["tasks"], "tasks")?;
    let mut ids = BTreeSet::new();
    if tasks.is_empty() || tasks.len() > 500 {
        return Err(invalid("select 1–500 pending tasks"));
    }
    for id in tasks {
        if !ids.insert(id) {
            return Err(invalid("duplicate bulk task ID"));
        }
        let mut item = q.clone();
        item["id"] = json!(id);
        if let Some(rev) = q["revisions"].get(id) {
            item["expected_rev"] = rev.clone();
        }
        decision(c, &item)?;
    }
    Ok(json!({"tasks":ids}))
}
fn queue(c: &Context<'_>, q: &Value) -> Result<Value> {
    enabled(c)?;
    let filter = q.get("filter").cloned().unwrap_or_else(|| json!({}));
    if !filter.is_object() {
        return Err(invalid("filter must be an object"));
    }
    let mut filter = filter;
    filter["status_category"] = json!("triage");
    let mut rows = crate::query::filtered(c, "task", &json!({"filter":filter,"query":q["query"]}))?;
    let at = now();
    let snoozed = |r: &Record| r.fields["deferred_until"].as_i64().is_some_and(|t| t > at);
    let total = rows.len();
    let hidden = rows.iter().filter(|t| snoozed(t)).count();
    if q["include_deferred"] != true {
        rows.retain(|r| !snoozed(r));
    }
    let order = q["order"].as_str().unwrap_or("oldest");
    if !matches!(order, "oldest" | "newest") {
        return Err(invalid("order must be oldest or newest"));
    }
    rows.sort_by(|a, b| {
        let key = |r: &Record| {
            r.fields["triage_entered_at"]
                .as_i64()
                .unwrap_or(r.created_at)
        };
        let d = key(a).cmp(&key(b)).then_with(|| a.id.cmp(&b.id));
        if order == "newest" { d.reverse() } else { d }
    });
    for r in &mut rows {
        r.fields["triage_snoozed"] = json!(snoozed(r));
    }
    let rows = crate::query::paginate(rows, q)?;
    Ok(json!({"total":total,"deferred":hidden,"items":rows}))
}
fn converted(c: &Context<'_>, request: &str) -> Result<Option<Value>> {
    let existing: Option<(String, String)> =
        c.db.query_row(
            "SELECT target_space,task FROM triage_conversions WHERE space=? AND request=?",
            params![c.space, request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((space, id)) = existing {
        let dest = Context {
            space: &space,
            ..*c
        };
        let task = if dest.role().is_ok()
            && let Ok(task) = dest.readable("task", &id)
        {
            as_value(task)
        } else {
            json!({"submitted":true,"id":id})
        };
        return Ok(Some(task));
    }
    Ok(None)
}
fn convert_request(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    crate::spaces::require_module(c, "customer_requests")?;
    let request = c.readable("customer_request", required(q, "id")?)?;
    crate::domain::check_rev(q, &request)?;
    if let Some(result) = converted(c, &request.id)? {
        return Ok(result);
    }
    let mut f = json!({"source":"customer_request","description":request.fields["description"].as_str().unwrap_or("")});
    if let Some(visibility) = request.fields["visibility"].as_str() {
        f["visibility"] = json!(visibility);
    }
    for k in ["type", "custom"] {
        if let Some(v) = q.get(k) {
            f[k] = v.clone();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    c.db.execute(
        "INSERT INTO triage_conversions(space,request,target_space,task) VALUES(?,?,?,?)",
        params![c.space, request.id, c.space, id],
    )
    .map_err(storage)?;
    write_request(
        c,
        &json!({"op":"create","kind":"task","id":id,"title":request.title,"fields":f}),
    )?;
    converted(c, &request.id)?.ok_or_else(|| storage("missing conversion after task creation"))
}
pub(crate) fn entered(c: &Context<'_>, row: &Record) -> Result<Record> {
    rules::apply(c, row)
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    match op {
        "triage.settings" => settings(c),
        "triage.configure" => configure(c, q),
        "triage.queue" => queue(c, q),
        "triage.batch" => bulk(c, q),
        "triage.submit" => intake::submit(c, q),
        "customer_request.convert" => convert_request(c, q),
        "task.share" | "task.open" => intake::link(c, op, q),
        _ => Err(invalid("unknown planning operation")),
    }
}
