use super::*;
use std::collections::BTreeSet;

pub(crate) fn capture(c: &Context<'_>, it: &Record) -> Result<()> {
    let inserted =
        c.db.execute(
            "INSERT OR IGNORE INTO iteration_closures(space,iteration,at) VALUES(?,?,?)",
            params![c.space, it.id, crate::now()],
        )
        .map_err(storage)?;
    if inserted > 0 {
        for task in c
            .records("task")?
            .iter()
            .filter(|r| r.fields["iteration"] == it.id)
        {
            remember(c, &it.id, task)?;
        }
    }
    Ok(())
}

fn remember(c: &Context<'_>, iteration: &str, task: &Record) -> Result<()> {
    c.db.execute("INSERT OR IGNORE INTO iteration_scope_history(space,iteration,task,snapshot,category) VALUES(?,?,?,?,?)",params![c.space,iteration,task.id,serde_json::to_string(task).map_err(storage)?,c.status_category(task)?]).map_err(storage)?;
    Ok(())
}

pub(crate) fn supplement(c: &Context<'_>, old: Option<&Record>, task: &Record) -> Result<()> {
    if task.kind == "task"
        && old.is_none_or(|r| r.fields["iteration"] != task.fields["iteration"])
        && let Some(id) = task.fields["iteration"].as_str()
        && c.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM iteration_closures WHERE space=? AND iteration=?)",
                params![c.space, id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage)?
        && c.status_category(task)? == "done"
    {
        remember(c, id, task)?;
    }
    Ok(())
}
pub(super) fn forget(c: &Context<'_>, it: &str) -> Result<()> {
    for table in ["iteration_closures", "iteration_scope_history"] {
        c.db.execute(
            &format!("DELETE FROM {table} WHERE space=? AND iteration=?"),
            params![c.space, it],
        )
        .map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn record_override(c: &Context<'_>, task: &Record) -> Result<()> {
    // A deliberate removal or a metadata import predates any later live status transition.
    c.db.execute("INSERT INTO iteration_join_overrides(space,task,seq)
        SELECT space,id,seq FROM event_log WHERE space=? AND kind='task' AND id=? AND rev=? ORDER BY seq DESC LIMIT 1
        ON CONFLICT(space,task) DO UPDATE SET seq=excluded.seq",params![c.space,task.id,task.rev]).map_err(storage)?;
    Ok(())
}
pub(crate) fn closed_scope(c: &Context<'_>, it: &str) -> Result<Option<Vec<(Record, String)>>> {
    let exists: bool =
        c.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM iteration_closures WHERE space=? AND iteration=?)",
            params![c.space, it],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !exists {
        return Ok(None);
    }
    let mut stmt=c.db.prepare("SELECT snapshot,category FROM iteration_scope_history WHERE space=? AND iteration=? ORDER BY task").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, it], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?;
    let mut visible = vec![];
    for row in rows {
        let (raw, category) = row.map_err(storage)?;
        let old: Record = serde_json::from_str(&raw).map_err(storage)?;
        if c.visible(&old)? && c.readable("task", &old.id).is_ok() {
            visible.push((old, category));
        }
    }
    Ok(Some(visible))
}
pub(super) fn read(c: &Context<'_>, id: &str) -> Result<Value> {
    c.readable("iteration", id)?;
    let captured = closed_scope(c, id)?;
    let tasks=captured.as_deref().unwrap_or_default().iter().map(|(r,category)|json!({"id":r.id,"title":r.title,"status":r.fields["status"],"category":category,"estimate":r.fields["estimate"]})).collect::<Vec<_>>();
    let at: Option<i64> =
        c.db.query_row(
            "SELECT at FROM iteration_closures WHERE space=? AND iteration=?",
            params![c.space, id],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    Ok(json!({"id":id,"captured":captured.is_some(),"captured_at":at,"tasks":tasks}))
}

pub(super) fn list(c: &Context<'_>) -> Result<Value> {
    let mut result = Vec::new();
    for it in c.records("iteration")? {
        if it.fields["status"] == "closed" && c.visible(&it)? {
            result.push(json!({"id":it.id,"title":it.title,"closed_at":it.fields["closed_at"],"start_date":it.fields["start_date"],"end_date":it.fields["end_date"]}));
        }
    }
    result.sort_by_key(|r| std::cmp::Reverse(r["closed_at"].as_i64().unwrap_or_default()));
    Ok(json!(result))
}

fn target(c: &Context<'_>, s: &Settings, day: NaiveDate) -> Result<Option<(String, NaiveDate)>> {
    if s.mode != Mode::Automatic || !s.rules() {
        return Ok(None);
    }
    let rows = c.records("iteration")?;
    let current = rows
        .iter()
        .filter(|r| r.fields["cadence"] == true)
        .find(|r| r.fields["status"] == "active");
    if let Some(it) = current {
        let begin = date(required(&it.fields, "start_date")?)?;
        let end = date(required(&it.fields, "end_date")?)?;
        return Ok((begin <= day && day <= end).then_some((it.id.clone(), begin)));
    }
    if let Some(it) = rows
        .iter()
        .find(|r| r.fields["cadence"] == true && r.fields["status"] == "upcoming")
    {
        if day < s.anchor_date {
            return Ok(None);
        }
        let next = date(required(&it.fields, "start_date")?)?;
        let start = next - Duration::days(i64::from(s.cooldown_days));
        return Ok((start <= day && day < next).then_some((it.id.clone(), start)));
    }
    Ok(None)
}

fn category(c: &Context<'_>, fields: &Value) -> String {
    fields["status"]
        .as_str()
        .and_then(|s| c.category_for(s).ok())
        .unwrap_or_default()
}
fn matches(c: &Context<'_>, s: &Settings, old: Option<&Record>, r: &Record) -> bool {
    let f = &r.fields;
    let category = category(c, f);
    let transitioned = old.is_none_or(|before| before.fields["status"] != f["status"]);
    if matches!(category.as_str(), "" | "cancelled" | "triage") {
        return false;
    }
    if category == "done" {
        return transitioned && s.auto_join_completed;
    }
    let deadline_changed = f["due_date"].is_string()
        && old.is_none_or(|before| before.fields["due_date"] != f["due_date"]);
    (s.auto_join_in_progress && transitioned && category == "in_progress")
        || (s.auto_join_active
            && (deadline_changed
                || transitioned
                    && (matches!(category.as_str(), "todo" | "in_progress")
                        || f["due_date"].is_string())))
}
pub(crate) fn auto_assign(c: &Context<'_>, old: Option<&Record>, r: &mut Record) -> Result<()> {
    if r.kind != "task" || !r.fields["iteration"].is_null() {
        return Ok(());
    }
    let (s, _) = policy(c)?;
    if s.mode != Mode::Automatic || !s.rules() || !matches(c, &s, old, r) {
        return Ok(());
    }
    if let Some((it, _)) = target(c, &s, Utc::now().with_timezone(&s.zone()?).date_naive())? {
        r.fields["iteration"] = json!(it);
    }
    Ok(())
}

pub(super) fn backfill(c: &Context<'_>, s: &Settings, at: DateTime<Utc>) -> Result<()> {
    let zone = s.zone()?;
    let day = at.with_timezone(&zone).date_naive();
    let Some((it, begin)) = target(c, s, day)? else {
        return Ok(());
    };
    // UTC padding includes civil days whose midnight is skipped or repeated.
    let lower = (begin.and_hms_opt(0, 0, 0).expect("midnight") - Duration::days(2))
        .and_utc()
        .timestamp_millis();
    let mut stmt=c.db.prepare("SELECT e.at,e.snapshot,(SELECT p.snapshot FROM event_log p WHERE p.space=e.space AND p.kind='task' AND p.id=e.id AND p.seq<e.seq ORDER BY p.seq DESC LIMIT 1)
        FROM event_log e LEFT JOIN iteration_join_overrides o ON o.space=e.space AND o.task=e.id
        WHERE e.space=? AND e.kind='task' AND e.at BETWEEN ? AND ? AND e.action IN ('create','update') AND e.seq>coalesce(o.seq,0) ORDER BY e.seq").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, lower, at.timestamp_millis()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(storage)?;
    let mut candidates = BTreeSet::new();
    for row in rows {
        let (when, raw, prior) = row.map_err(storage)?;
        if !DateTime::from_timestamp_millis(when)
            .is_some_and(|t| t.with_timezone(&zone).date_naive() >= begin)
        {
            continue;
        }
        let old: Option<Record> = prior
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(storage)?;
        let task: Record = serde_json::from_str(&raw).map_err(storage)?;
        if !task.fields["iteration"].is_null()
            || old
                .as_ref()
                .is_some_and(|r| r.fields["iteration"] != task.fields["iteration"])
        {
            candidates.remove(&task.id);
        } else if matches(c, s, old.as_ref(), &task) {
            candidates.insert(task.id);
        } else if old.as_ref().is_some_and(|r| {
            r.fields["status"] != task.fields["status"]
                || r.fields["due_date"] != task.fields["due_date"]
        }) {
            candidates.remove(&task.id);
        }
    }
    for id in candidates {
        if let Some(task) = c.find("task", &id)?
            && task.fields["iteration"].is_null()
        {
            let mut f = task.fields.clone();
            f["iteration"] = json!(it);
            c.update(&task, &task.title, f)?;
        }
    }
    Ok(())
}
