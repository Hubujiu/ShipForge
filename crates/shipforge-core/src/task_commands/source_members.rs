use crate::{Context, Result, domain, invalid};
use serde_json::{Value, json};

fn source_ids(value: &Value) -> Result<Vec<String>> {
    let values: Vec<&Value> = match value {
        Value::Array(list) => list.iter().collect(),
        Value::Null => vec![],
        item => vec![item],
    };
    let mut ids = Vec::new();
    for item in values {
        let key = if item.is_object() {
            ["accountId", "id", "gid", "username", "login", "name"]
                .iter()
                .find_map(|field| item.get(field).filter(|v| !v.is_null()))
                .ok_or_else(|| invalid("an external assignee needs an identity"))?
        } else {
            item
        };
        let id = if let Some(id) = key.as_str() {
            id.to_owned()
        } else if key.is_u64() {
            key.to_string()
        } else {
            return Err(invalid("invalid external assignee"));
        };
        if id.trim().is_empty() || id.chars().any(char::is_control) {
            return Err(invalid("invalid external assignee"));
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub(crate) fn provider(
    c: &Context<'_>,
    provider: &str,
    p: &Value,
    config: &Value,
    mapped: &mut Value,
) -> Result<()> {
    let value = match provider {
        "gitlab" => p
            .get("assignees")
            .or_else(|| p["object_attributes"].get("assignee_ids")),
        "jira" => p["issue"]["fields"].get("assignee"),
        "linear" => p["data"]
            .get("assigneeId")
            .or_else(|| p["data"].get("assignee")),
        "asana" => p["task"]["data"]
            .get("assignee")
            .or_else(|| p["data"].get("assignee")),
        _ => None,
    };
    let Some(value) = value else { return Ok(()) };
    let mut members = Vec::new();
    let mut unresolved = Vec::new();
    for id in source_ids(value)? {
        if let Some(actor) = config["assignee_map"][&id].as_str()
            && c.find("member", actor)?.is_some()
        {
            if !members.contains(&actor) {
                members.push(actor);
            }
        } else {
            unresolved.push(id);
        }
    }
    mapped["assignees"] = json!(members);
    mapped["source_assignees"] = json!(unresolved);
    Ok(())
}

pub(crate) fn validate_map(c: &Context<'_>, config: &Value) -> Result<()> {
    if let Some(mapping) = config.get("assignee_map") {
        for (external, id) in mapping
            .as_object()
            .ok_or_else(|| invalid("assignee_map must be an object"))?
        {
            if external.trim().is_empty() || external.chars().any(char::is_control) {
                return Err(invalid("invalid external assignee"));
            }
            c.readable(
                "member",
                id.as_str()
                    .ok_or_else(|| invalid("map external assignees to member ids"))?,
            )?;
        }
    }
    Ok(())
}

/// Portable bundles do not carry member accounts. Keep unavailable identities
/// for display without assigning an unrelated local account.
pub(crate) fn imported(c: &Context<'_>, fields: &mut Value) -> Result<()> {
    if let Some(value) = fields.get("assignees") {
        let mut known = Vec::new();
        let mut external = fields
            .get("source_assignees")
            .map(source_ids)
            .transpose()?
            .unwrap_or_default();
        for id in domain::strings(value, "assignees")? {
            if id == "owner" || c.find("member", id)?.is_some() {
                if !known.contains(&id) {
                    known.push(id);
                }
            } else if !external.iter().any(|s| s == id) {
                external.push(id.to_owned());
            }
        }
        let known = json!(known);
        fields["assignees"] = known;
        if !external.is_empty() {
            fields["source_assignees"] = json!(external);
        }
    }
    if fields.get("executor").is_some_and(|v| !v.is_null()) {
        let worker = &fields["executor"];
        if let (Some(kind), Some(id)) = (worker["kind"].as_str(), worker["id"].as_str())
            && matches!(kind, "agent" | "crew")
            && c.find(kind, id)?.is_none()
        {
            fields["executor"] = Value::Null;
        }
    }
    Ok(())
}
