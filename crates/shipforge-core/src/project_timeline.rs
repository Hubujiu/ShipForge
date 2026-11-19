use super::*;
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};

const PHASES: &[&str] = &["draft", "planned", "active", "paused", "done", "cancelled"];
pub(crate) fn dates(f: &Value) -> Result<()> {
    for key in ["start_date", "due_date", "planned_start", "planned_end"] {
        if !f[key].is_null() {
            crate::domain::valid_date(&f[key])?;
        }
    }
    for (from, to) in [("start_date", "due_date"), ("planned_start", "planned_end")] {
        if let (Some(a), Some(b)) = (f[from].as_str(), f[to].as_str())
            && a > b
        {
            return Err(invalid("a schedule cannot finish before its start date"));
        }
    }
    Ok(())
}
pub(crate) fn validate(c: &Context<'_>, old: Option<&Record>, f: &mut Value) -> Result<()> {
    dates(f)?;
    if f.get("status").is_none() {
        f["status"] = json!("draft");
    }
    if f.get("priority").is_none() {
        f["priority"] = json!("none");
    }
    if old.is_none_or(|r| r.fields["status"] != f["status"])
        && !f["status"].as_str().is_some_and(|s| PHASES.contains(&s))
    {
        return Err(invalid("unknown project phase"));
    }
    if old.is_none_or(|r| r.fields["priority"] != f["priority"])
        && !matches!(
            f["priority"].as_str(),
            Some("none" | "low" | "medium" | "high" | "urgent")
        )
    {
        return Err(invalid("unknown project priority"));
    }
    let kind = f["owner_kind"].as_str().unwrap_or("member");
    if !matches!(kind, "member" | "agent") {
        return Err(invalid("project owner must be a member or Agent"));
    }
    if let Some(owner) = f["owner"].as_str().filter(|s| !s.is_empty())
        && old
            .is_none_or(|r| r.fields["owner"] != owner || r.fields["owner_kind"] != f["owner_kind"])
        && !(kind == "member" && owner == "owner")
    {
        c.readable(kind, owner)?;
    }
    if let Some(main) = f["primary_space"].as_str()
        && main != c.space
        && !f["spaces"]
            .as_array()
            .is_some_and(|ls| ls.iter().any(|s| s == main))
    {
        return Err(invalid(
            "project primary space must be included in its scope",
        ));
    }
    Ok(())
}
pub(crate) fn visible_scope(c: &Context<'_>, fields: &mut Value) {
    if let Some(spaces) = fields["spaces"].as_array_mut() {
        spaces.retain(|id| {
            id.as_str()
                .is_some_and(|space| (Context { space, ..*c }).metadata_role().is_ok())
        });
    }
    if let Some(space) = fields["primary_space"].as_str()
        && (Context { space, ..*c }).metadata_role().is_err()
    {
        fields["primary_space"] = Value::Null;
    }
}
fn parse(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| invalid("date must be YYYY-MM-DD"))
}
pub(crate) fn timeline(c: &Context<'_>, q: &Value) -> Result<Value> {
    let today = match q.get("as_of") {
        Some(v) => parse(
            v.as_str()
                .ok_or_else(|| invalid("as_of must be a calendar date"))?,
        )?,
        None => crate::iterations::local_today(c)?,
    };
    let mode = q.get("basis").map_or(Ok("schedule"), |v| {
        v.as_str()
            .filter(|s| matches!(*s, "schedule" | "start" | "due" | "plan"))
            .ok_or_else(|| invalid("unknown timeline basis"))
    })?;
    let progress = crate::query::project_progress(c, required(q, "id")?)?;
    let project = &progress["project"];
    let tasks: Vec<Record> = serde_json::from_value(progress["tasks"].clone()).map_err(storage)?;
    let mut scheduled = Vec::new();
    let mut undated = Vec::new();
    let mut earliest = None;
    let mut latest = None;
    let mut links: HashMap<(String, String), Vec<Value>> = HashMap::new();
    let visible: HashSet<_> = tasks
        .iter()
        .map(|t| (t.space.as_str(), t.id.as_str()))
        .collect();
    for space in tasks
        .iter()
        .map(|t| t.space.as_str())
        .collect::<HashSet<_>>()
    {
        let scope = Context { space, ..*c };
        for rel in scope.records("relation")? {
            if rel.fields["type"] == "blocks"
                && let (Some(from), Some(to)) =
                    (rel.fields["from"].as_str(), rel.fields["to"].as_str())
                && visible.contains(&(space, to))
                && scope.visible(&rel)?
            {
                let dep = scope.readable("task", from)?;
                links
                    .entry((space.into(), to.into()))
                    .or_default()
                    .push(json!({"id":from,"title":dep.title,"number":dep.fields["number"]}));
            }
        }
    }
    for task in tasks {
        let f = &task.fields;
        let start = f["start_date"].as_str();
        let due = f["due_date"].as_str();
        let ps = f["planned_start"].as_str();
        let pe = f["planned_end"].as_str();
        let (first, last, source) = match mode {
            "start" => (start, start, "start"),
            "due" => (due, due, "due"),
            "plan" => (ps.or(pe), pe.or(ps), "plan"),
            _ if start.is_some() || due.is_some() => (start.or(due), due.or(start), "task"),
            _ => (ps.or(pe), pe.or(ps), "plan"),
        };
        let scope = Context {
            space: &task.space,
            ..*c
        };
        let terminal = matches!(
            scope.status_category(&task)?.as_str(),
            "done" | "cancelled" | "duplicate"
        );
        let dependencies = links
            .remove(&(task.space.clone(), task.id.clone()))
            .unwrap_or_default();
        let entry = json!({"task":task,"start":first,"end":last,"source":source,"overdue":!terminal&&due.and_then(|s|parse(s).ok()).is_some_and(|d|d<today),"blocked_by":dependencies});
        if let (Some(a), Some(b)) = (first, last) {
            let (a, b) = (parse(a)?, parse(b)?);
            earliest = Some(earliest.map_or(a, |v: NaiveDate| v.min(a)));
            latest = Some(latest.map_or(b, |v: NaiveDate| v.max(b)));
            scheduled.push(entry);
        } else {
            undated.push(entry);
        }
    }
    scheduled.sort_by(|a, b| {
        a["start"]
            .as_str()
            .cmp(&b["start"].as_str())
            .then_with(|| a["end"].as_str().cmp(&b["end"].as_str()))
            .then_with(|| a["task"]["id"].as_str().cmp(&b["task"]["id"].as_str()))
    });
    Ok(
        json!({"project":project,"total":progress["total"],"completed":progress["completed"],"basis":mode,"as_of":today,"start":earliest,"end":latest,"scheduled":scheduled,"undated":undated}),
    )
}
