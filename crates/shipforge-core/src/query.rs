use super::*;
use crate::domain::check_kind;
use crate::tasks::resolve_project;
use chrono::{Days, NaiveDate};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    match op {
        "organization.list" => organizations(c),
        "space.list" => {
            let mut spaces = Vec::new();
            for space in c.all_spaces()? {
                let dest = Context {
                    db: c.db,
                    actor: c.actor,
                    space: &space.id,
                };
                if let Ok(role) = dest.metadata_role() {
                    spaces.push(crate::spaces::listing(c, space, &role)?);
                }
            }
            Ok(json!(spaces))
        }
        "get" => {
            let kind = required(q, "kind")?;
            check_kind(kind)?;
            let id = required(q, "id")?;
            if kind == "project" {
                let mut project = resolve_project(c, id)?;
                crate::project_timeline::visible_scope(c, &mut project.fields);
                return Ok(as_value(project));
            }
            if matches!(kind, "tag" | "tag_group") {
                return crate::tags::accessible(c, kind, id).map(as_value);
            }
            match c.readable(kind, id) {
                Ok(r) => Ok(as_value(r)),
                Err(e) if kind == "task" && e.starts_with("not_found:") => {
                    let mut tasks = visible_records(c, "task")?;
                    tasks.retain(|r| r.fields["number"] == id);
                    tasks.into_iter().next().map(as_value).ok_or(e)
                }
                Err(e) => Err(e),
            }
        }
        "list" => {
            let kind = required(q, "kind")?;
            check_kind(kind)?;
            if kind == "organization" {
                return organizations(c);
            }
            let mut records = matching(c, kind, q)?;
            if kind == "task" {
                crate::im::decorate(c, &mut records)?;
            }
            if matches!(
                kind,
                "status" | "task_type" | "relation_type" | "field" | "tag"
            ) {
                let items = visible_records(
                    c,
                    if kind == "relation_type" {
                        "relation"
                    } else {
                        "task"
                    },
                )?;
                for row in &mut records {
                    let count = items
                        .iter()
                        .filter(|v| match kind {
                            "status" => v.fields["status"] == row.id,
                            "task_type" | "relation_type" => v.fields["type"] == row.id,
                            "field" => v.fields["custom"]
                                .get(&row.id)
                                .is_some_and(|v| !v.is_null()),
                            _ => v.fields["labels"]
                                .as_array()
                                .is_some_and(|ls| ls.iter().any(|label| label == &row.id)),
                        })
                        .count();
                    row.fields["usage"] = json!(count);
                }
            }
            Ok(json!(records))
        }
        "search" => {
            let text = required(q, "query")?.trim().to_lowercase();
            let tags = matching_tags(c, &text)?;
            let kinds = if let Some(k) = q["kind"].as_str() {
                check_kind(k)?;
                vec![k]
            } else {
                let space = c.get("space", c.space)?;
                let mut searchable = vec![
                    "task",
                    "view",
                    "project",
                    "document",
                    "note",
                    "chat",
                    "message",
                    "comment",
                    "member",
                    "agent",
                    "crew",
                    "skill",
                    "automation",
                    "machine",
                    "artifact",
                ];
                if crate::spaces::enabled(&space, "iterations") {
                    searchable.push("iteration");
                }
                if crate::spaces::enabled(&space, "customer_requests") {
                    searchable.push("customer_request");
                }
                searchable
            };
            let mut matches = Vec::new();
            for kind in kinds {
                for r in visible_records(c, kind)? {
                    if matches!(kind, "agent" | "crew") && !crate::agents::discoverable(&r) {
                        continue;
                    }
                    if search_matches(&r, &text, &tags) {
                        matches.push(r);
                    }
                }
            }
            matches.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.kind.cmp(&b.kind))
                    .then_with(|| a.id.cmp(&b.id))
            });
            Ok(json!(paginate(matches, q)?))
        }
        "events" => events(c, q),
        "view.run" => {
            let view = c.readable("view", required(q, "id")?)?;
            crate::saved_views::query(c, &view.fields, q)
        }
        "dashboard" => dashboard(c, q),
        "iteration.timeline" => iteration_timeline(c, q),
        "iteration.preview" => {
            let it = c.readable("iteration", required(q, "id")?)?;
            Ok(json!(unfinished(c, &it.id)?))
        }
        "task.relations" => related(c, required(q, "id")?),
        "project.progress" => project_progress(c, required(q, "id")?),
        "document.history" => {
            let doc = c.readable("document", required(q, "id")?)?;
            let mut sql=c.db.prepare("SELECT rev,title,fields,actor,at FROM versions WHERE space=?1 AND id=?2 ORDER BY rev DESC").map_err(storage)?;
            let rows = sql
                .query_map(params![c.space, doc.id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                })
                .map_err(storage)?;
            let mut history = Vec::new();
            for row in rows {
                let (rev, title, raw, actor, at) = row.map_err(storage)?;
                let fields: Value = serde_json::from_str(&raw).map_err(storage)?;
                let snapshot = Record {
                    fields: fields.clone(),
                    ..doc.clone()
                };
                if c.visible(&snapshot)? {
                    history.push(json!({"version":rev,"rev":rev,"title":title,"fields":fields,"actor":actor,"at":at}));
                }
            }
            Ok(json!(history))
        }
        "export" => crate::transfer::export(c),
        "import.preview" => crate::transfer::import(c, q, true),
        _ => Err(invalid("unknown query")),
    }
}
pub(crate) fn visible_records(c: &Context<'_>, kind: &str) -> Result<Vec<Record>> {
    if matches!(kind, "tag" | "tag_group") {
        return crate::tags::catalog(c, kind);
    }
    let mut rows = Vec::new();
    let mut followed = std::collections::HashSet::new();
    if kind == "task" {
        let mut query =
            c.db.prepare("SELECT task FROM followers WHERE space=? AND actor=?")
                .map_err(storage)?;
        for id in query
            .query_map(params![c.space, c.actor], |r| r.get::<_, String>(0))
            .map_err(storage)?
        {
            followed.insert(id.map_err(storage)?);
        }
    }
    let candidates = if kind == "notification" {
        c.own_notifications()?
    } else {
        c.records(kind)?
    };
    for mut row in candidates {
        if c.visible(&row)? {
            if kind == "task" {
                row.fields["following"] = json!(followed.contains(&row.id));
            }
            if kind == "notification"
                && row.fields["state"] == "snoozed"
                && row.fields["until"].as_i64().is_some_and(|t| t <= now())
            {
                row.fields["state"] = json!("inbox");
            }
            rows.push(row);
        }
    }
    if kind == "project" {
        let org = c.org_id()?;
        for space in c.all_spaces()? {
            if space.id == c.space
                || space.fields["organization"].as_str().unwrap_or("default") != org
            {
                continue;
            }
            let referring = Context {
                actor: c.actor,
                space: &space.id,
                db: c.db,
            };
            for project in referring.records("project")? {
                if project.fields["spaces"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == c.space))
                    && referring.visible(&project)?
                {
                    rows.push(project);
                }
            }
        }
    }
    if kind == "project" {
        for project in &mut rows {
            crate::project_timeline::visible_scope(c, &mut project.fields);
        }
    }
    Ok(rows)
}
fn matching_tags(c: &Context<'_>, text: &str) -> Result<std::collections::HashSet<String>> {
    if text.is_empty() {
        return Ok(Default::default());
    }
    Ok(crate::tags::catalog(c, "tag")?
        .into_iter()
        .filter(|r| r.title.to_lowercase().contains(text))
        .map(|r| r.id)
        .collect())
}
fn search_matches(r: &Record, text: &str, tags: &std::collections::HashSet<String>) -> bool {
    r.title.to_lowercase().contains(text)
        || r.fields["attachments"].as_array().is_some_and(|a| {
            a.iter()
                .filter_map(|f| f["filename"].as_str())
                .any(|s| s.to_lowercase().contains(text))
        })
        || r.id.to_lowercase().contains(text)
        || ["body", "description", "number", "summary"]
            .iter()
            .any(|k| {
                r.fields[k]
                    .as_str()
                    .is_some_and(|s| s.to_lowercase().contains(text))
            })
        || r.fields["labels"].as_array().is_some_and(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .any(|s| s.to_lowercase().contains(text) || tags.contains(s))
        })
}
fn filter_eq(actual: &Value, wanted: &Value, actor: &str) -> bool {
    if let Some(values) = wanted.as_array() {
        return values.iter().any(|w| filter_eq(actual, w, actor));
    }
    if let Some(values) = actual.as_array() {
        return values.iter().any(|v| filter_eq(v, wanted, actor));
    }
    if wanted == "@me" {
        return actual.as_str() == Some(actor);
    }
    actual == wanted
}
fn satisfies(c: &Context<'_>, r: &Record, filter: Option<&Value>) -> Result<bool> {
    let Some(f) = filter else { return Ok(true) };
    let filters = f
        .as_object()
        .ok_or_else(|| invalid("filter must be an object"))?;
    for (key, wanted) in filters {
        let negate = key.ends_with("_not");
        let key = key.strip_suffix("_not").unwrap_or(key);
        if r.kind == "task"
            && key == "labels"
            && let Some(labels) = wanted.as_array()
        {
            let values = &r.fields["labels"];
            let matched = if negate {
                labels.iter().any(|v| filter_eq(values, v, c.actor))
            } else {
                labels.iter().all(|v| filter_eq(values, v, c.actor))
            };
            if matched == negate {
                return Ok(false);
            }
            continue;
        }
        let value = match key {
            "status_category" if r.kind == "task" => json!(c.status_category(r)?),
            "assignee" if r.kind == "task" => {
                let mut handlers = r.fields["assignees"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if let Some(id) = r.fields["executor"]["id"].as_str() {
                    handlers.push(json!(id));
                }
                if handlers.is_empty() {
                    Value::Null
                } else {
                    Value::Array(handlers)
                }
            }
            "title" => json!(r.title),
            "id" => json!(r.id),
            "kind" => json!(r.kind),
            _ => r.fields[key].clone(),
        };
        if filter_eq(&value, wanted, c.actor) == negate {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn filtered(c: &Context<'_>, kind: &str, q: &Value) -> Result<Vec<Record>> {
    let text = q["query"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let mut rows = Vec::new();
    let tags = if kind == "task" {
        matching_tags(c, &text)?
    } else {
        Default::default()
    };
    for r in visible_records(c, kind)? {
        if (text.is_empty() || search_matches(&r, &text, &tags))
            && satisfies(c, &r, q.get("filter"))?
        {
            rows.push(r);
        }
    }
    Ok(rows)
}
pub(crate) fn matching(c: &Context<'_>, kind: &str, q: &Value) -> Result<Vec<Record>> {
    Ok(matching_page(c, kind, q)?.0)
}
pub(crate) fn matching_page(
    c: &Context<'_>,
    kind: &str,
    q: &Value,
) -> Result<(Vec<Record>, usize)> {
    let mut rows = filtered(c, kind, q)?;
    if matches!(kind, "agent" | "crew") {
        rows.retain(crate::agents::discoverable);
    }
    let sort = q["sort"].as_str().unwrap_or(
        if matches!(kind, "status" | "task_type" | "relation_type") {
            "order"
        } else {
            "-updated_at"
        },
    );
    let key = sort.trim_start_matches('-');
    let desc = sort.starts_with('-') || q["direction"] == "desc";
    rows.sort_by(|a, b| {
        let order = match key {
            "updated_at" => a.updated_at.cmp(&b.updated_at),
            "created_at" => a.created_at.cmp(&b.created_at),
            "title" => a.title.cmp(&b.title),
            "id" => a.id.cmp(&b.id),
            "priority" => priority(&a.fields["priority"]).cmp(&priority(&b.fields["priority"])),
            _ if a.fields[key].is_number() || b.fields[key].is_number() => a.fields[key]
                .as_f64()
                .partial_cmp(&b.fields[key].as_f64())
                .unwrap_or(Ordering::Equal),
            _ => a.fields[key]
                .as_str()
                .partial_cmp(&b.fields[key].as_str())
                .unwrap_or(Ordering::Equal),
        };
        (if desc { order.reverse() } else { order }).then_with(|| a.id.cmp(&b.id))
    });
    let total = rows.len();
    Ok((paginate(rows, q)?, total))
}
fn priority(v: &Value) -> u8 {
    match v.as_str() {
        Some("urgent") => 0,
        Some("high") => 1,
        Some("medium") => 2,
        Some("low") => 3,
        _ => 4,
    }
}
fn count(q: &Value, key: &str, default: usize) -> Result<usize> {
    q.get(key).map_or(Ok(default), |v| {
        v.as_u64()
            .and_then(|x| usize::try_from(x).ok())
            .ok_or_else(|| invalid(&format!("{key} must be a nonnegative integer")))
    })
}
pub(crate) fn paginate<T>(rows: Vec<T>, q: &Value) -> Result<Vec<T>> {
    let offset = count(q, "offset", 0)?;
    let limit = count(q, "limit", 500)?;
    if !(1..=5000).contains(&limit) {
        return Err(invalid("limit must be 1–5000"));
    }
    Ok(rows.into_iter().skip(offset).take(limit).collect())
}
pub(crate) fn unfinished(c: &Context<'_>, id: &str) -> Result<Vec<Record>> {
    let mut pending = Vec::new();
    for task in visible_records(c, "task")? {
        if task.fields["iteration"] == id
            && !matches!(
                c.status_category(&task)?.as_str(),
                "done" | "cancelled" | "duplicate"
            )
        {
            pending.push(task);
        }
    }
    Ok(pending)
}
fn iteration_timeline(c: &Context<'_>, q: &Value) -> Result<Value> {
    let today = match q.get("as_of") {
        Some(value) => NaiveDate::parse_from_str(value.as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| invalid("as_of must use YYYY-MM-DD"))?,
        None => crate::iterations::local_today(c)?,
    };
    let today = today.to_string();
    #[derive(Default)]
    struct Scope {
        total: usize,
        completed: usize,
        estimate: f64,
        completed_estimate: f64,
    }
    let estimates_enabled = c.get("space", c.space)?.fields["estimates_enabled"] == true;
    let mut counts: BTreeMap<String, Scope> = BTreeMap::new();
    for task in visible_records(c, "task")? {
        if let Some(iteration) = task.fields["iteration"].as_str() {
            let entry = counts.entry(iteration.to_owned()).or_default();
            let estimate = task.fields["estimate"].as_f64().unwrap_or_default();
            entry.total += 1;
            entry.estimate += estimate;
            if c.status_category(&task)? == "done" {
                entry.completed += 1;
                entry.completed_estimate += estimate;
            }
        }
    }
    let mut rows = visible_records(c, "iteration")?;
    let manual = crate::iterations::configured_manual(c)?;
    let next_manual = if manual {
        rows.iter()
            .filter(|r| r.fields["status"] == "planned")
            .filter_map(|r| {
                let date = r.fields["start_date"]
                    .as_str()
                    .or(r.fields["start"].as_str())?;
                (date >= today.as_str()).then_some((date, r.id.as_str()))
            })
            .min()
            .map(|(_, id)| id.to_owned())
    } else {
        None
    };
    if let Some(owner) = q.get("owner") {
        let id = owner
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("owner must be a member ID or @me"))?;
        let id = if id == "@me" { c.actor } else { id };
        rows.retain(|r| {
            r.fields
                .get("owner")
                .map_or_else(|| r.fields["creator"].as_str(), Value::as_str)
                == Some(id)
        });
    }
    for it in &mut rows {
        let start = it.fields["start_date"]
            .as_str()
            .or(it.fields["start"].as_str());
        let end = it.fields["end_date"].as_str().or(it.fields["end"].as_str());
        let phase = if it.fields["status"] == "closed" {
            "closed"
        } else if it.fields["status"] == "cancelled" {
            "cancelled"
        } else if manual && it.fields["status"] == "active" {
            "active"
        } else if next_manual.as_deref() == Some(it.id.as_str()) {
            "upcoming"
        } else if (manual || it.fields["cadence"] == true) && it.fields["status"] == "planned" {
            "planned"
        } else if start.is_some_and(|s| s > today.as_str()) {
            "upcoming"
        } else if end.is_some_and(|s| s < today.as_str()) {
            "overdue"
        } else if start.is_some() || it.fields["status"] == "active" {
            "active"
        } else {
            "unscheduled"
        };
        let mut scope = counts.remove(&it.id).unwrap_or_default();
        if it.fields["status"] == "closed"
            && let Some(history) = crate::iterations::closed_scope(c, &it.id)?
        {
            scope = Scope::default();
            for (r, category) in history {
                scope.total += 1;
                let estimate = r.fields["estimate"].as_f64().unwrap_or_default();
                scope.estimate += estimate;
                if category == "done" {
                    scope.completed += 1;
                    scope.completed_estimate += estimate;
                }
            }
        }
        it.fields["planning"] = json!({"phase":phase,"start":start,"end":end,"total":scope.total,
            "completed":scope.completed,"capacity":it.fields["capacity"].as_u64()});
        if estimates_enabled {
            it.fields["planning"]["estimate"] = json!(scope.estimate);
            it.fields["planning"]["completed_estimate"] = json!(scope.completed_estimate);
        }
    }
    rows.sort_by(|a, b| {
        b.fields["planning"]["start"]
            .as_str()
            .cmp(&a.fields["planning"]["start"].as_str())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(json!(rows))
}
fn dashboard(c: &Context<'_>, q: &Value) -> Result<Value> {
    let list = filtered(c, "task", q)?;
    let today = q["today"]
        .as_str()
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| invalid("today must use YYYY-MM-DD"))
        })
        .transpose()?
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let week = today
        .checked_add_days(Days::new(7))
        .unwrap_or(NaiveDate::MAX);
    let (mut completed, mut overdue, mut due_soon, mut unassigned) = (0, 0, 0, 0);
    let mut statuses: BTreeMap<String, usize> = BTreeMap::new();
    let mut priorities: BTreeMap<String, usize> = BTreeMap::new();
    for r in &list {
        let category = c.status_category(r)?;
        *statuses
            .entry(r.fields["status"].as_str().unwrap_or("todo").into())
            .or_default() += 1;
        *priorities
            .entry(r.fields["priority"].as_str().unwrap_or("none").into())
            .or_default() += 1;
        if !r.fields["assignee"].is_string() {
            unassigned += 1;
        }
        if category == "done" {
            completed += 1;
        }
        if !matches!(category.as_str(), "done" | "cancelled" | "duplicate")
            && let Some(date) = r.fields["due_date"]
                .as_str()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        {
            if date < today {
                overdue += 1;
            } else if date <= week {
                due_soon += 1;
            }
        }
    }
    Ok(
        json!({"total":list.len(),"completed":completed,"overdue":overdue,"due_soon":due_soon,"unassigned":unassigned,"by_status":statuses,"by_priority":priorities}),
    )
}
fn related(c: &Context<'_>, id: &str) -> Result<Value> {
    let task = c.readable("task", id)?;
    let mut output = Vec::new();
    if let Some(parent) = task.fields["parent"].as_str()
        && let Ok(p) = c.readable("task", parent)
    {
        output.push(json!({"type":"parent","direction":"out","task":p}));
    }
    for child in visible_records(c, "task")? {
        if child.fields["parent"] == id {
            output.push(json!({"type":"parent","direction":"in","task":child}));
        }
    }
    for rel in visible_records(c, "relation")? {
        let edge = if rel.fields["from"] == id {
            Some(("out", "to"))
        } else if rel.fields["to"] == id {
            Some(("in", "from"))
        } else {
            None
        };
        if let Some((direction, key)) = edge
            && let Some(other) = rel.fields[key].as_str()
            && let Ok(r) = c.readable("task", other)
        {
            output.push(
                json!({"relation":rel.id,"type":rel.fields["type"],"direction":direction,"task":r}),
            );
        }
    }
    Ok(json!(output))
}
pub(crate) fn project_progress(c: &Context<'_>, id: &str) -> Result<Value> {
    let mut project = resolve_project(c, id)?;
    let mut tasks = Vec::new();
    let mut done = 0;
    for space in c.all_spaces()? {
        if space.fields["organization"].as_str().unwrap_or("default")
            != project.fields["organization"].as_str().unwrap_or("default")
        {
            continue;
        }
        if space.id != project.space
            && !project.fields["spaces"]
                .as_array()
                .is_some_and(|s| s.iter().any(|v| v == &space.id))
        {
            continue;
        }
        let scope = Context {
            db: c.db,
            actor: c.actor,
            space: &space.id,
        };
        if scope.role().is_err() {
            continue;
        }
        for task in visible_records(&scope, "task")? {
            if task.fields["project"] == id {
                if scope.status_category(&task)? == "done" {
                    done += 1;
                }
                tasks.push(task);
            }
        }
    }
    crate::project_timeline::visible_scope(c, &mut project.fields);
    Ok(json!({"project":project,"total":tasks.len(),"completed":done,"tasks":tasks}))
}
fn events(c: &Context<'_>, q: &Value) -> Result<Value> {
    let after = count(q, "after", 0)?;
    let limit = count(q, "limit", 500)?;
    if !(1..=5000).contains(&limit) {
        return Err(invalid("limit must be 1–5000"));
    }
    let mut sql=c.db.prepare("SELECT seq,action,snapshot FROM event_log WHERE space=?1 AND seq>?2 ORDER BY seq LIMIT ?3").map_err(storage)?;
    let rows = sql
        .query_map(params![c.space, after as i64, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?;
    let mut cursor = after as i64;
    let mut output = Vec::new();
    for row in rows {
        let (seq, action, snapshot) = row.map_err(storage)?;
        cursor = seq;
        let prior: Record = serde_json::from_str(&snapshot).map_err(storage)?;
        if !c.visible(&prior)? {
            continue;
        }
        if let Some(current) = c.find(&prior.kind, &prior.id)?
            && !c.visible(&current)?
        {
            continue;
        }
        output.push(json!({"seq":seq,"space":c.space,"kind":prior.kind,"id":prior.id,"rev":prior.rev,"action":action,"at":prior.updated_at}));
    }
    Ok(json!({"cursor":cursor,"events":output}))
}

fn organizations(c: &Context<'_>) -> Result<Value> {
    let mut result = Vec::new();
    for space in c.all_spaces()? {
        let anchor = Context {
            db: c.db,
            actor: c.actor,
            space: &space.id,
        };
        if anchor.role().is_ok()
            && let Some(org) = anchor.find("organization", &space.id)?
        {
            result.push(org);
        }
    }
    Ok(json!(result))
}
