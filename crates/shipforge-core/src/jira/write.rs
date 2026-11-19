use super::{Site, fields};
use crate::{Context, Record, Result, domain, invalid, required};
use serde_json::{Value, json};

pub(super) fn apply(c: &Context<'_>, request: Value) -> Result<Value> {
    crate::spaces::ensure_active(c)?;
    let result = domain::write(c, &request)?;
    crate::onboarding::successful(c, required(&request, "op")?, &request, &result)?;
    Ok(result)
}

fn body(patch: &mut Value, key: &str, input: &Value) -> Result<()> {
    let plain = if input.is_null() {
        Value::Null
    } else {
        json!(fields::plain(input)?)
    };
    patch[key] = plain;
    patch[format!("jira_{key}_adf")] = if input.is_object() {
        input.clone()
    } else {
        Value::Null
    };
    Ok(())
}
pub(super) fn issue(
    site: &Site<'_, '_>,
    space: &Record,
    task: Option<&Record>,
    payload: &Value,
    transition: Option<&str>,
) -> Result<Record> {
    let mut comments = Vec::new();
    if let Some(update) = payload.get("update") {
        for (field, edits) in update
            .as_object()
            .ok_or_else(|| invalid("update must be an object"))?
        {
            if field != "comment" || task.is_none() {
                return Err(invalid("provide field replacements under fields"));
            }
            for edit in edits
                .as_array()
                .ok_or_else(|| invalid("comment updates must be a list"))?
            {
                if edit.as_object().is_none_or(|v| v.len() != 1) || !edit["add"].is_object() {
                    return Err(invalid("comment update requires add.body"));
                }
                comments.push(&edit["add"]);
            }
        }
    }
    let c = site.scope(space);
    c.authorize_write("task", task)?;
    let empty = json!({});
    let map = payload
        .get("fields")
        .unwrap_or(&empty)
        .as_object()
        .ok_or_else(|| invalid("fields must be an object"))?;
    if map.is_empty() && transition.is_none() && comments.is_empty() {
        return Err(invalid("supply at least one issue field"));
    }
    let mut request =
        json!({"op":if task.is_some(){"update"}else{"create"},"kind":"task","fields":{}});
    if let Some(row) = task {
        request["id"] = json!(row.id);
        for update in comments {
            comment(site, &c, row, update)?;
        }
    }
    if let Some(state) = transition {
        request["fields"]["status"] = json!(state);
    }
    for (name, value) in map {
        match name.as_str() {
            "project" if task.is_none() => {}
            "summary" => {
                request["title"] = json!(
                    value
                        .as_str()
                        .ok_or_else(|| invalid("summary must be text"))?
                )
            }
            "description" => body(&mut request["fields"], "description", value)?,
            "issuetype" => {
                request["fields"]["type"] = json!(fields::resolve(&c, "task_type", value)?)
            }
            "duedate" => request["fields"]["due_date"] = value.clone(),
            "priority" if value.is_null() => request["fields"]["priority"] = json!("none"),
            "priority" => {
                let p = fields::reference(value, &["id", "name"])?;
                let (_, _, level) = fields::PRIORITIES
                    .iter()
                    .find(|(id, name, _)| p == *id || p.eq_ignore_ascii_case(name))
                    .ok_or_else(|| invalid("unknown priority"))?;
                crate::organization::imported_priority(&c, level)?;
                request["fields"]["priority"] = json!(level);
            }
            "assignee" => {
                request["fields"]["assignees"] = if value.is_null() {
                    json!([])
                } else {
                    json!([fields::reference(value, &["accountId", "name", "key"])?])
                };
            }
            "labels" => request["fields"]["labels"] = value.clone(),
            "parent" => {
                request["fields"]["parent"] = if value.is_null() {
                    Value::Null
                } else {
                    let (project, parent) = site.issue(fields::reference(value, &["key"])?)?;
                    if project.id != space.id {
                        return Err(invalid("parent issue must belong to the same project"));
                    }
                    json!(parent.id)
                };
            }
            _ => {
                return Err(invalid(&format!(
                    "field {name} is unavailable for this operation"
                )));
            }
        }
    }
    let row = apply(&c, request)?;
    serde_json::from_value(row).map_err(crate::storage)
}

pub(super) fn comment(
    site: &Site<'_, '_>,
    c: &Context<'_>,
    task: &Record,
    payload: &Value,
) -> Result<Value> {
    let mut patch = json!({"task":task.id});
    body(&mut patch, "body", &payload["body"])?;
    let text = required(&patch, "body")?;
    let title = text.chars().take(120).collect::<String>();
    let row = apply(
        c,
        json!({"op":"create","kind":"comment","title":title,"fields":patch}),
    )?;
    site.render_comment(
        c,
        task,
        &serde_json::from_value(row).map_err(crate::storage)?,
    )
}
