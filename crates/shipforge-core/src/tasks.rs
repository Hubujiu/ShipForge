use super::*;
use crate::domain::{BASE_STATUS, ref_record, strings, valid_date};
use std::collections::{HashMap, HashSet};

pub(crate) fn validate_task(
    c: &Context<'_>,
    old: Option<&Record>,
    id: &str,
    f: &mut Value,
) -> Result<()> {
    if old.is_none() {
        if f["type"].is_null() {
            f["type"] = json!(crate::defaults::selected(c, "task_type", "task")?);
        }
        if f["status"].is_null() {
            let default = crate::defaults::selected(c, "status", "todo")?;
            let config = c.find("task_type", required(f, "type")?)?;
            let workflow = config
                .as_ref()
                .and_then(|r| r.fields["statuses"].as_array())
                .filter(|r| !r.is_empty());
            let chosen = workflow
                .filter(|r| !r.iter().any(|v| v == &default))
                .and_then(|r| r.first())
                .and_then(Value::as_str)
                .unwrap_or(&default);
            f["status"] = json!(chosen);
        }
        for (k, v) in [
            ("priority", json!("none")),
            ("owner", json!(c.actor)),
            ("assignee_kind", json!("member")),
            ("labels", json!([])),
        ] {
            if f[k].is_null() {
                f[k] = v;
            }
        }
        f["number"] = json!(c.task_number()?);
    }
    let task_type = required(f, "type")?.to_owned();
    let config = c.find("task_type", &task_type)?;
    if config.is_none() && !matches!(task_type.as_str(), "task" | "requirement" | "bug") {
        return Err(invalid("unknown task type"));
    }
    let space = c.get("space", c.space)?;
    let guest = c.role()? == "guest";
    let external = matches!(
        f["source"].as_str(),
        Some("external" | "integration" | "guest")
    ) || f["external_id"].is_string();
    let inbox = space.fields["triage_enabled"] == true;
    let allowed = config
        .as_ref()
        .and_then(|r| r.fields["statuses"].as_array());
    let terminal = matches!(
        c.category_for(required(f, "status")?)?.as_str(),
        "done" | "cancelled"
    );
    if old.is_none() && inbox && (guest || external && !terminal) {
        let candidate = match allowed.filter(|list| !list.is_empty()) {
            None => Some("triage".into()),
            Some(list) => list
                .iter()
                .filter_map(Value::as_str)
                .find(|s| c.category_for(s).as_deref() == Ok("triage"))
                .map(str::to_owned),
        };
        if let Some(status) = candidate {
            f["status"] = json!(status);
        } else if guest {
            return Err(forbidden("guest submission requires a triage workflow"));
        }
    }
    let status = required(f, "status")?;
    let category = c.category_for(status)?;
    if category == "triage" && !inbox {
        return Err(invalid("triage is disabled for this space"));
    }
    if allowed.is_some_and(|states| !states.is_empty() && !states.iter().any(|v| v == status)) {
        return Err(invalid("status is outside this task type's workflow"));
    }
    if !matches!(
        f["priority"].as_str(),
        Some("none" | "low" | "medium" | "high" | "urgent")
    ) {
        return Err(invalid("unknown task priority"));
    }
    if let Some(previous) = old
        && c.status_category(previous)? == "triage"
        && (category != "triage" || !f["iteration"].is_null())
        && space.fields["triage_priority_required"] == true
        && f["priority"] == "none"
    {
        return Err(invalid(
            "set a priority before accepting or planning this task",
        ));
    }
    if guest
        && (old.is_some()
            || category != "triage"
            || !f["assignee"].is_null()
            || !f["iteration"].is_null()
            || !f["project"].is_null())
    {
        return Err(forbidden(
            "guest tasks must enter triage without a handler or planning assignments",
        ));
    }
    let owner = required(f, "owner")?;
    if owner != "owner" && c.find("member", owner)?.is_none() {
        let principal = Context {
            db: c.db,
            actor: owner,
            space: c.space,
        };
        principal.require_admin()?;
    }
    if let Some(actor) = f["assignee"].as_str() {
        let k = f["assignee_kind"].as_str().unwrap_or("member");
        if !matches!(k, "member" | "agent" | "crew") {
            return Err(invalid("unknown assignee kind"));
        }
        if k != "member" || actor != "owner" {
            c.readable(k, actor)?;
        }
    } else if !f["assignee"].is_null() {
        return Err(invalid("assignee must be an id or null"));
    }
    if let Some(it) = ref_record(c, f, "iteration", "iteration")?
        && it.fields["status"] == "closed"
        && old.is_none_or(|r| r.fields["iteration"] != f["iteration"])
    {
        return Err(invalid("cannot add work to a closed iteration"));
    }
    if let Some(project) = f["project"].as_str() {
        resolve_project(c, project)?;
    } else if !f["project"].is_null() {
        return Err(invalid("project must be a single identifier"));
    }
    if let Some(parent) = f["parent"].as_str() {
        c.readable("task", parent)?;
        if reaches(c, parent, id, "parent", None)? {
            return Err(invalid("parent relationship would form a cycle"));
        }
    } else if !f["parent"].is_null() {
        return Err(invalid("parent must be a task identifier"));
    }
    if !f["due_date"].is_null() {
        valid_date(&f["due_date"])?;
    }
    strings(&f["labels"], "labels")?;
    if !f["custom"].is_null() && !f["custom"].is_object() {
        return Err(invalid("custom must be an object"));
    }
    if let Some(type_record) = config
        && let Some(required_fields) = type_record.fields["required_fields"].as_array()
    {
        for key in required_fields.iter().filter_map(Value::as_str) {
            if f["custom"][key].is_null()
                || f["custom"][key]
                    .as_str()
                    .is_some_and(|s| s.trim().is_empty())
            {
                return Err(invalid(&format!("missing required custom field {key}")));
            }
        }
    }
    if let Some(items) = f["custom"].as_object() {
        for (key, v) in items {
            let field = c.readable("field", key)?;
            if v.is_null() {
                continue;
            }
            let good = match field.fields["value_type"].as_str().unwrap_or("text") {
                "number" => v.is_number(),
                "boolean" => v.is_boolean(),
                "date" => valid_date(v).is_ok(),
                "select" => field.fields["options"]
                    .as_array()
                    .is_some_and(|o| o.contains(v)),
                "text" => v.is_string(),
                _ => false,
            };
            if !good {
                return Err(invalid(&format!("incorrect value for custom field {key}")));
            }
        }
    }
    Ok(())
}

pub(crate) fn resolve_project(c: &Context<'_>, id: &str) -> Result<Record> {
    if let Some(r) = c.find("project", id)?
        && c.visible(&r)?
    {
        return Ok(r);
    }
    for space in c.all_spaces()? {
        if space.id == c.space {
            continue;
        }
        let other = Context {
            db: c.db,
            actor: c.actor,
            space: &space.id,
        };
        if other.role().is_err() {
            continue;
        }
        if let Some(r) = other.find("project", id)?
            && other.visible(&r)?
            && r.fields["spaces"]
                .as_array()
                .is_some_and(|a| a.iter().any(|s| s == c.space))
        {
            return Ok(r);
        }
    }
    Err(format!("not_found: project {id}"))
}

fn reaches(
    c: &Context<'_>,
    from: &str,
    to: &str,
    edge: &str,
    excluded: Option<&str>,
) -> Result<bool> {
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for r in c.records("relation")? {
        if Some(r.id.as_str()) == excluded || r.fields["type"] != edge {
            continue;
        }
        let (Some(start), Some(end)) = (r.fields["from"].as_str(), r.fields["to"].as_str()) else {
            continue;
        };
        adjacency.entry(start.into()).or_default().push(end.into());
    }
    if edge == "parent" {
        for r in c.records("task")? {
            if let Some(parent) = r.fields["parent"].as_str() {
                adjacency.entry(r.id).or_default().push(parent.into());
            }
        }
    }
    let mut queue = vec![from.to_owned()];
    let mut visited = HashSet::new();
    while let Some(node) = queue.pop() {
        if node == to {
            return Ok(true);
        }
        if visited.insert(node.clone())
            && let Some(edges) = adjacency.get(&node)
        {
            queue.extend(edges.iter().cloned());
        }
    }
    Ok(false)
}

pub(crate) fn validate_relation(c: &Context<'_>, id: &str, f: &Value) -> Result<()> {
    let from = required(f, "from")?;
    let to = required(f, "to")?;
    let typ = required(f, "type")?;
    if from == to {
        return Err(invalid("a task cannot relate to itself"));
    }
    c.readable("task", from)?;
    c.readable("task", to)?;
    if !matches!(typ, "related" | "blocks" | "duplicate" | "parent") {
        c.readable("relation_type", typ)?;
    }
    if matches!(typ, "blocks" | "parent" | "duplicate") && reaches(c, to, from, typ, Some(id))? {
        return Err(invalid("relationship would form a cycle"));
    }
    if c.records("relation")?.iter().any(|r| {
        r.id != id
            && r.fields["type"] == typ
            && ((r.fields["from"] == from && r.fields["to"] == to)
                || (typ == "related" && r.fields["from"] == to && r.fields["to"] == from))
    }) {
        return Err(conflict("relationship already exists"));
    }
    Ok(())
}

pub(crate) fn target_status(c: &Context<'_>, task: &Record, category: &str) -> Result<String> {
    if !BASE_STATUS.contains(&category) {
        return Err(invalid("unknown status category"));
    }
    if let Some(config) = c.find("task_type", required(&task.fields, "type")?)?
        && let Some(allowed) = config.fields["statuses"]
            .as_array()
            .filter(|a| !a.is_empty())
    {
        return allowed
            .iter()
            .filter_map(Value::as_str)
            .find(|id| c.category_for(id).as_deref() == Ok(category))
            .map(str::to_owned)
            .ok_or_else(|| invalid(&format!("workflow has no {category} status")));
    }
    Ok(category.into())
}

pub(crate) fn cleanup(c: &Context<'_>, r: &Record) -> Result<()> {
    if crate::defaults::builtin(&r.kind, &r.id) {
        return Err(invalid("built-in configuration is permanent"));
    }
    match r.kind.as_str() {
        "space" | "organization" => {
            return Err(invalid(
                "workspace ownership records cannot be deleted here",
            ));
        }
        "member" => {
            if r.id == "owner" || c.get("space", c.space)?.fields["creator"] == r.id {
                return Err(forbidden("space owner membership is permanent"));
            }
            let org = c.org_id()?;
            if org == c.space {
                for child in c.all_spaces()? {
                    if child.id != org && child.fields["organization"] == org {
                        let scope = Context {
                            db: c.db,
                            actor: c.actor,
                            space: &child.id,
                        };
                        if let Some(m) = scope.find("member", &r.id)? {
                            if child.fields["creator"] == r.id {
                                return Err(conflict(
                                    "transfer ownership of the member's space before removal",
                                ));
                            }
                            cleanup(&scope, &m)?;
                            scope.remove(&m)?;
                        }
                    }
                }
            }
            c.db.execute(
                "DELETE FROM followers WHERE space=?1 AND actor=?2",
                rusqlite::params![c.space, r.id],
            )
            .map_err(storage)?;
            for task in c.records("task")? {
                let mut f = task.fields.clone();
                if f["assignee_kind"] == "member" && f["assignee"] == r.id {
                    f["assignee"] = Value::Null;
                }
                if f["owner"] == r.id {
                    f["owner"] = json!("owner");
                }
                if f != task.fields {
                    c.update(&task, &task.title, f)?;
                }
            }
        }
        "task" => {
            for kind in ["relation", "comment", "notification", "artifact"] {
                for child in c.records(kind)? {
                    if ["from", "to", "task"]
                        .iter()
                        .any(|key| child.fields[key] == r.id)
                    {
                        c.remove(&child)?;
                    }
                }
            }
            for sub in c.records("task")? {
                if sub.fields["parent"] == r.id {
                    let mut f = sub.fields.clone();
                    f["parent"] = Value::Null;
                    c.update(&sub, &sub.title, f)?;
                }
            }
            c.db.execute(
                "DELETE FROM followers WHERE space=?1 AND task=?2",
                rusqlite::params![c.space, r.id],
            )
            .map_err(storage)?;
        }
        "chat" => {
            for child in c.records("message")? {
                if child.fields["chat"] == r.id {
                    c.remove(&child)?;
                }
            }
        }
        "iteration" | "status" | "project" | "task_type" | "field" | "relation_type" | "tag" => {
            let key = match r.kind.as_str() {
                "iteration" => "iteration",
                "status" => "status",
                "project" => "project",
                "task_type" => "type",
                "field" => "custom",
                "tag" => "labels",
                _ => "type",
            };
            let linked_kind = if r.kind == "relation_type" {
                "relation"
            } else {
                "task"
            };
            let spaces = if r.kind == "project" {
                c.all_spaces()?.into_iter().map(|s| s.id).collect()
            } else {
                vec![c.space.to_string()]
            };
            for space in spaces {
                let scope = Context {
                    db: c.db,
                    space: &space,
                    actor: c.actor,
                };
                if scope.records(linked_kind)?.iter().any(|x| {
                    x.fields[key] == r.id
                        || x.fields[key].get(&r.id).is_some()
                        || x.fields[key]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|s| s == &r.id))
                }) {
                    return Err(conflict("record is referenced by existing work"));
                }
            }
            if matches!(r.kind.as_str(), "field" | "status") {
                let attr = if r.kind == "field" {
                    "required_fields"
                } else {
                    "statuses"
                };
                if c.records("task_type")?.iter().any(|t| {
                    t.fields[attr]
                        .as_array()
                        .is_some_and(|ls| ls.iter().any(|x| x == &r.id))
                }) {
                    return Err(conflict("record is referenced by a task type"));
                }
            }
        }
        "document" => {
            c.db.execute(
                "DELETE FROM versions WHERE space=?1 AND id=?2",
                rusqlite::params![c.space, r.id],
            )
            .map_err(storage)?;
        }
        _ => {}
    }
    Ok(())
}
