use super::*;
use std::collections::BTreeMap;

fn id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| digits(s))
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))
}

fn optional_text(value: &Value, limit: usize) -> Result<String> {
    match value {
        Value::Null => Ok(String::new()),
        Value::String(s) if s.len() <= limit => Ok(s.to_owned()),
        _ => Err(invalid("invalid Zoho text field")),
    }
}

pub(super) fn normalize(c: &Context<'_>, r: &Record, issues: &Value) -> Result<Vec<Value>> {
    let rows = issues
        .as_array()
        .filter(|items| items.len() <= 5000)
        .ok_or_else(|| invalid("expected at most 5000 Zoho issues"))?;
    let mut normalized = BTreeMap::new();
    for data in rows {
        let external = id(&data["id"])
            .or_else(|| id(&data["id_string"]))
            .ok_or_else(|| invalid("Zoho issue needs a lossless numeric ID"))?;
        let title = required(data, "title")?.trim();
        if title.chars().count() > 500 {
            return Err(invalid("Zoho title exceeds 500 characters"));
        }
        let label = optional_text(&data["status"]["type"], 256)?;
        let severity = optional_text(&data["severity"]["type"], 256)?;
        let status = r.fields["mapping"]["status"][&label]
            .as_str()
            .unwrap_or_else(|| {
                if data["closed"] == true {
                    "done"
                } else if matches!(
                    label.to_ascii_lowercase().as_str(),
                    "inprogress" | "in progress" | "in progress phase"
                ) {
                    "in_progress"
                } else {
                    "todo"
                }
            });
        c.readable("status", status)?;
        let priority = r.fields["mapping"]["priority"][&severity]
            .as_str()
            .unwrap_or_else(|| match severity.to_ascii_lowercase().as_str() {
                "critical" | "show stopper" | "blocker" => "urgent",
                "major" | "high" => "high",
                "medium" | "normal" => "medium",
                "minor" | "low" => "low",
                "lowest" | "最低" => "lowest",
                _ => "none",
            });
        if !matches!(
            priority,
            "urgent" | "high" | "medium" | "low" | "lowest" | "none"
        ) {
            return Err(invalid("unknown mapped priority"));
        }
        let name = optional_text(&data["assignee_name"], 256)?;
        let assignee = id(&data["assignee_id"]).or_else(|| id(&data["assignee_zpuid"]));
        let local = assignee
            .as_deref()
            .and_then(|id| r.fields["assignee_map"][id].as_str());
        if let Some(member) = local {
            c.readable("member", member)?;
        }
        let unresolved = if local.is_none() {
            assignee.as_deref().or_else(|| {
                (!name.is_empty() && !name.eq_ignore_ascii_case("Not Assigned"))
                    .then_some(name.as_str())
            })
        } else {
            None
        };
        let modified = match data.get("last_modified_time_long").filter(|v| !v.is_null()) {
            Some(v) => Some(v.as_i64().filter(|n| *n >= 0).ok_or_else(|| {
                invalid("Zoho modification time must be a millisecond timestamp")
            })?),
            None => None,
        };
        let fields = json!({"title":title,"description":optional_text(&data["description"],1_048_576)?,"priority":priority,
            "status":status,"source_status":label,"source_priority":severity,"source_key":id(&data["key"]),
            "source_reporter":optional_text(&data["reported_person"],256)?,"source_reporter_id":id(&data["reporter_id"]),
            "assignees":local.into_iter().collect::<Vec<_>>(),"source_assignees":unresolved.into_iter().collect::<Vec<_>>()});
        let row = json!({"external":external,"modified":modified,"fields":fields});
        if normalized
            .insert(external, row.clone())
            .is_some_and(|old| old != row)
        {
            return Err(conflict(
                "Zoho returned incompatible versions of the same issue; preview again",
            ));
        }
    }
    Ok(normalized.into_values().collect())
}
