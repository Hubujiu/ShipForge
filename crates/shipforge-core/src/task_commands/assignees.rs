use crate::{Context, Record, Result, domain, identifier, invalid, required};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

/// Shape legacy records at the read boundary. This leaves revisioned history intact.
pub(crate) fn upgrade(fields: &mut Value) {
    if fields.get("assignees").is_none() {
        fields["assignees"] = if fields["assignee"].is_string()
            && !matches!(fields["assignee_kind"].as_str(), Some("agent" | "crew"))
        {
            json!([fields["assignee"]])
        } else {
            json!([])
        };
    }
    if fields.get("executor").is_none() {
        fields["executor"] = if fields["assignee"].is_string()
            && matches!(fields["assignee_kind"].as_str(), Some("agent" | "crew"))
        {
            json!({"kind":fields["assignee_kind"],"id":fields["assignee"]})
        } else {
            Value::Null
        };
    }
}

/// The legacy handler remains the single worker when present, or the first human.
pub(crate) fn project(fields: &mut Value) {
    let worker = &fields["executor"];
    let (kind, id) = match worker["id"].as_str() {
        Some(_) => (worker["kind"].clone(), worker["id"].clone()),
        None => (json!("member"), fields["assignees"][0].clone()),
    };
    fields["assignee"] = id;
    fields["assignee_kind"] = kind;
}

pub(crate) fn people(fields: &Value) -> impl Iterator<Item = &str> {
    fields["assignees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn checked_members(c: &Context<'_>, members: &Value) -> Result<()> {
    let members = domain::strings(members, "assignees")?;
    let mut unique = HashSet::new();
    for id in members {
        identifier(id)?;
        if !unique.insert(id) {
            return Err(invalid("assignees must be distinct"));
        }
        if id != "owner" {
            c.readable("member", id)?;
        }
    }
    Ok(())
}

pub(crate) fn prepare(
    c: &Context<'_>,
    old: Option<&Record>,
    patch: &mut Map<String, Value>,
    from_source: bool,
) -> Result<()> {
    let empty = json!({});
    let previous = old.map_or(&empty, |r| &r.fields);
    let mut assignment = json!({
        "assignees":previous.get("assignees").cloned().unwrap_or_else(||json!([])),
        "executor":previous["executor"]
    });
    if patch.contains_key("assignee") || patch.contains_key("assignee_kind") {
        let id = patch.get("assignee").unwrap_or(&previous["assignee"]);
        if !id.is_null() && !id.is_string() {
            return Err(invalid("assignee must be an id or null"));
        }
        let kind = patch
            .get("assignee_kind")
            .or_else(|| previous.get("assignee_kind"))
            .and_then(Value::as_str)
            .unwrap_or("member");
        match kind {
            "member" => {
                assignment["assignees"] = if id.is_string() {
                    json!([id])
                } else {
                    json!([])
                }
            }
            "agent" | "crew" => {
                assignment["executor"] = if id.is_string() {
                    json!({"kind":kind,"id":id})
                } else {
                    Value::Null
                }
            }
            _ => return Err(invalid("unknown assignee kind")),
        }
    }
    for key in ["assignees", "executor"] {
        if let Some(value) = patch.get(key) {
            assignment[key] = value.clone();
        }
    }
    // When a collaborator cannot see another owner's private executor, allow an
    // unrelated human assignment edit without giving them access to that executor.
    let participants = assignment["assignees"]
        .as_array()
        .ok_or_else(|| invalid("assignees must be an array"))?;
    if previous["assignees"] != assignment["assignees"] {
        checked_members(c, &assignment["assignees"])?;
        if participants.len() > 1
            && !from_source
            && previous["assignees"]
                .as_array()
                .is_none_or(|ids| ids.len() <= 1)
            && c.get("space", c.space)?.fields["allow_multiple_assignees"] != true
        {
            return Err(invalid(
                "enable multiple task assignees in this space first",
            ));
        }
    }
    let worker = &assignment["executor"];
    if !worker.is_null() && (previous["executor"] != *worker || old.is_none()) {
        let kind = required(worker, "kind")?;
        if !matches!(kind, "agent" | "crew") {
            return Err(invalid("executor must be an agent or crew"));
        }
        if worker.as_object().is_none_or(|o| o.len() != 2) {
            return Err(invalid("executor accepts kind and id"));
        }
        c.readable(kind, required(worker, "id")?)?;
    }
    project(&mut assignment);
    patch.extend(assignment.as_object().unwrap().clone());
    Ok(())
}

pub(crate) fn assign(c: &Context<'_>, q: &Value) -> Result<Value> {
    let task = c.readable("task", required(q, "id")?)?;
    c.authorize_write("task", Some(&task))?;
    domain::check_rev(q, &task)?;
    let mut patch = Map::new();
    if let Some(members) = q.get("assignees") {
        checked_members(c, members)?;
        patch.insert(
            "assignees".into(),
            match q.get("mode").map(Value::as_str) {
                None | Some(Some("replace")) => members.clone(),
                Some(Some("append")) => {
                    let mut ids = task.fields["assignees"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for id in members.as_array().unwrap() {
                        if !ids.contains(id) {
                            ids.push(id.clone());
                        }
                    }
                    Value::Array(ids)
                }
                _ => return Err(invalid("mode must be replace or append")),
            },
        );
    } else if q.get("mode").is_some() {
        return Err(invalid("mode requires assignees"));
    }
    if let Some(worker) = q.get("executor") {
        patch.insert("executor".into(), worker.clone());
    }
    if patch.is_empty() {
        return Err(invalid("provide assignees or executor"));
    }
    domain::write(
        c,
        &json!({"op":"update","kind":"task","id":task.id,"expected_rev":task.rev,"fields":patch}),
    )
}

pub(crate) fn schema(schema: &mut Value) {
    let people = json!({"type":"array","items":{"type":"string","minLength":1},"uniqueItems":true});
    let executor = json!({"oneOf":[{"type":"null"},{"type":"object","required":["kind","id"],"properties":{"kind":{"enum":["agent","crew"]},"id":{"type":"string","minLength":1}},"additionalProperties":false}]});
    schema["properties"]["assignees"] = people.clone();
    schema["properties"]["executor"] = executor.clone();
    schema["properties"]["source_assignees"] = people.clone();
    let rules = schema["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"const":"task.assign"}}},"then":{"required":["id"],"anyOf":[{"required":["assignees"]},{"required":["executor"]}],"properties":{"mode":{"enum":["append","replace"]}},"dependentRequired":{"mode":["assignees"]}}}));
    rules.push(json!({"if":{"properties":{"op":{"enum":["create","update"]},"kind":{"const":"task"}},"required":["kind"]},"then":{"properties":{"fields":{"properties":{"assignees":people,"executor":executor}}}}}));
}
