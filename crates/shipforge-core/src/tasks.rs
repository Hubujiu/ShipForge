use super::*;
use crate::domain::{BASE_STATUS, ref_record, valid_date};
use std::collections::{HashMap, HashSet};

pub(crate) fn validate_task(
    c: &Context<'_>,
    old: Option<&Record>,
    id: &str,
    f: &mut Value,
    historical: bool,
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
                .and_then(|ls| {
                    ls.iter()
                        .filter_map(Value::as_str)
                        .find(|id| {
                            c.category_for(id).is_ok_and(|v| {
                                !matches!(v.as_str(), "triage" | "duplicate" | "cancelled" | "done")
                            })
                        })
                        .or_else(|| ls.first().and_then(Value::as_str))
                })
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
    if config
        .as_ref()
        .is_some_and(|r| r.fields["enabled"] == false)
        && old.is_none_or(|r| r.fields["type"] != task_type)
    {
        return Err(invalid("this task type is disabled in the space"));
    }
    if !f["estimate"].is_null() && old.is_none_or(|r| r.fields["estimate"] != f["estimate"]) {
        if c.get("space", c.space)?.fields["estimates_enabled"] != true {
            return Err(invalid("enable task estimates in the space first"));
        }
        if !f["estimate"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v >= 0.0)
        {
            return Err(invalid("estimate must be a non-negative number"));
        }
    }
    if config.is_none() && !matches!(task_type.as_str(), "task" | "requirement" | "bug") {
        return Err(invalid("unknown task type"));
    }
    let space = c.get("space", c.space)?;
    let role = match c.role() {
        Ok(role) => role,
        Err(_) if crate::triage::intake::outside(c)? => "outsider".into(),
        Err(e) => return Err(e),
    };
    let guest = role == "guest";
    let intake = guest || role == "outsider";
    if guest {
        f["source"] = json!("guest");
    }
    let external = matches!(
        f["source"].as_str(),
        Some("external" | "integration" | "guest" | "customer_request")
    ) || f["external_id"].is_string();
    let inbox = space.fields["triage_enabled"] == true;
    let channel = if guest { "guest" } else { "external" };
    let source_allowed = space.fields["triage_sources"]
        .as_array()
        .is_none_or(|sources| sources.iter().any(|s| s == channel));
    let type_allowed = space.fields["triage_types"]
        .as_array()
        .is_none_or(|types| types.iter().any(|s| s == &task_type));
    if intake && !(inbox && source_allowed && type_allowed) {
        return Err(forbidden(
            "this submission source or task type is outside triage",
        ));
    }
    let allowed = config
        .as_ref()
        .and_then(|r| r.fields["statuses"].as_array());
    let terminal = matches!(
        c.category_for(required(f, "status")?)?.as_str(),
        "done" | "cancelled" | "duplicate"
    );
    if old.is_none()
        && inbox
        && source_allowed
        && type_allowed
        && (intake || external && !historical && !terminal)
    {
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
        } else if intake {
            return Err(forbidden("guest submission requires a triage workflow"));
        }
    }
    let status = required(f, "status")?;
    let category = c.category_for(status)?;
    if category == "triage" && !inbox {
        return Err(invalid("triage is disabled for this space"));
    }
    if !(category == "duplicate" && f["triage_decision"] == "duplicate")
        && allowed.is_some_and(|states| !states.is_empty() && !states.iter().any(|v| v == status))
    {
        return Err(invalid("status is outside this task type's workflow"));
    }
    if !matches!(
        f["priority"].as_str(),
        Some("none" | "low" | "medium" | "high" | "urgent")
    ) {
        return Err(invalid("unknown task priority"));
    }
    let in_triage = category == "triage"
        || old.map(|r| c.status_category(r)).transpose()?.as_deref() == Some("triage");
    if in_triage
        && ((!matches!(category.as_str(), "triage" | "cancelled" | "duplicate") && old.is_some())
            || !f["iteration"].is_null())
        && space.fields["triage_priority_required"] == true
        && f["priority"] == "none"
    {
        return Err(invalid(
            "set a priority before accepting or planning this task",
        ));
    }
    if intake
        && (old.is_some()
            || f["owner"] != c.actor
            || !f["executor"].is_null()
            || !f["parent"].is_null()
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
    if owner != "owner"
        && old.is_none_or(|t| t.fields["owner"] != owner)
        && !(role == "outsider" && owner == c.actor)
        && c.find("member", owner)?.is_none()
    {
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
        if (k != "member" || actor != "owner")
            && old.is_none_or(|r| r.fields["assignee"] != actor || r.fields["assignee_kind"] != k)
        {
            c.readable(k, actor)?;
            if matches!(k, "agent" | "crew") {
                crate::agents::usable(c, k, actor, None)?;
            }
        }
    } else if !f["assignee"].is_null() {
        return Err(invalid("assignee must be an id or null"));
    }
    if f["iteration"].is_string() && old.is_none_or(|r| r.fields["iteration"] != f["iteration"]) {
        crate::spaces::require_module(c, "iterations")?;
    }
    if let Some(it) = ref_record(c, f, "iteration", "iteration")?
        && (it.fields["status"] == "cancelled"
            || (it.fields["status"] == "closed" && category != "done"))
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
    crate::project_timeline::dates(f)?;
    f["labels"] = crate::tags::normalize(
        c,
        old.map(|r| &r.fields["labels"]).unwrap_or(&Value::Null),
        &f["labels"],
    )?;
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
                "member" => v
                    .as_str()
                    .is_some_and(|id| c.readable("member", id).is_ok()),
                "members" => v.as_array().is_some_and(|ids| {
                    let mut seen = std::collections::BTreeSet::new();
                    ids.iter().all(|v| {
                        v.as_str()
                            .is_some_and(|id| seen.insert(id) && c.readable("member", id).is_ok())
                    })
                }),
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
    let org = c.org_id()?;
    for space in c.all_spaces()? {
        if space.id == c.space || space.fields["organization"].as_str().unwrap_or("default") != org
        {
            continue;
        }
        let other = Context {
            db: c.db,
            actor: c.actor,
            space: &space.id,
        };
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
    if r.kind == "iteration" {
        return crate::iterations::remove_manual(c, r);
    }
    if matches!(r.kind.as_str(), "tag" | "tag_group") {
        return crate::tags::cleanup(c, r);
    }
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
                            if child.fields["deactivated"] != true {
                                cleanup(&scope, &m)?;
                            }
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
                if let Some(people) = f["assignees"].as_array_mut() {
                    people.retain(|id| id != &r.id);
                }
                crate::task_commands::assignees::project(&mut f);
                if f["owner"] == r.id {
                    f["owner"] = json!("owner");
                }
                if f != task.fields {
                    c.update(&task, &task.title, f)?;
                }
            }
        }
        "task" => {
            c.db.execute(
                "DELETE FROM follower_objects WHERE space=?1 AND task=?2",
                rusqlite::params![c.space, r.id],
            )
            .map_err(storage)?;
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
        "iteration" | "status" | "project" | "task_type" | "field" | "relation_type" => {
            let key = match r.kind.as_str() {
                "iteration" => "iteration",
                "status" => "status",
                "project" => "project",
                "task_type" => "type",
                "field" => "custom",
                _ => "type",
            };
            let linked_kind = if r.kind == "relation_type" {
                "relation"
            } else {
                "task"
            };
            let spaces = if r.kind == "project" {
                let org = c.org_id()?;
                c.all_spaces()?
                    .into_iter()
                    .filter(|s| s.fields["organization"].as_str().unwrap_or("default") == org)
                    .map(|s| s.id)
                    .collect()
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
                    r.kind == "field" && crate::task_fields::references(t, &r.id)
                        || t.fields[attr]
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
