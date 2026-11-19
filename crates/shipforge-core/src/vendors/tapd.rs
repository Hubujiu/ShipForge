use super::*;
use chrono::NaiveDateTime;
use std::collections::BTreeMap;

pub(super) fn validate(f: &Value) -> Result<()> {
    let company = f["company_id"].as_str().filter(|s| !s.is_empty());
    match required(f, "tapd_auth")? {
        "company" if company.is_some_and(digits) => (),
        "personal" if company.is_none() => (),
        _ => {
            return Err(invalid(
                "TAPD company authentication requires a company ID; personal tokens do not use one",
            ));
        }
    }
    if f["tapd_workspace"]
        .as_str()
        .filter(|s| !s.is_empty())
        .is_some_and(|s| !digits(s))
    {
        return Err(invalid("TAPD workspace ID must be a decimal string"));
    }
    if f["realtime"] == true
        || f["outbound"] == true
        || f["direction"]
            .as_str()
            .is_some_and(|v| !v.is_empty() && v != "import")
    {
        return Err(invalid(
            "this TAPD connector supports historical import and on-demand inbound detection",
        ));
    }
    Ok(())
}

fn field(v: &Value, key: &str, max: usize) -> Result<String> {
    match &v[key] {
        Value::Null => Ok(String::new()),
        Value::String(s) if s.len() <= max => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(invalid("invalid TAPD text field")),
    }
}

pub(super) fn normalize(c: &Context<'_>, r: &Record, q: &Value) -> Result<Vec<Value>> {
    let mode = q["import_mode"].as_str().unwrap_or("history");
    if !matches!(mode, "history" | "changes" | "refresh") {
        return Err(invalid("unknown TAPD import mode"));
    }
    let project = required(&r.fields, "tapd_workspace")?;
    let rows = q["issues"]
        .as_array()
        .filter(|rows| rows.len() <= 5000)
        .ok_or_else(|| invalid("expected at most 5000 TAPD entries"))?;
    let mut results = BTreeMap::new();
    for item in rows {
        let (kind, data, title_field, owner_field) = match (item.get("Story"), item.get("Bug")) {
            (Some(s), None) => ("story", s, "name", "owner"),
            (None, Some(b)) => ("bug", b, "title", "current_owner"),
            _ => return Err(invalid("TAPD entries must contain one Story or Bug")),
        };
        if field(data, "workspace_id", 40)? != project {
            return Err(crate::forbidden("TAPD item belongs to another workspace"));
        }
        let key = field(data, "id", 40)?;
        if !digits(&key) {
            return Err(invalid("invalid TAPD item ID"));
        }
        let title = required(data, title_field)?.trim();
        if title.chars().count() > 500 {
            return Err(invalid("TAPD title exceeds 500 characters"));
        }
        let raw_status = field(data, "status", 160)?;
        let raw_priority = if data["priority_label"].is_string() {
            field(data, "priority_label", 160)?
        } else {
            field(data, "priority", 160)?
        };
        let status = r.fields["mapping"]["status"][&raw_status]
            .as_str()
            .unwrap_or(match raw_status.as_str() {
                "closed" | "done" | "resolved" => "done",
                "in_progress" | "progressing" | "developing" | "implementing" | "processing" => {
                    "in_progress"
                }
                "rejected" | "cancelled" => "cancelled",
                _ => "todo",
            });
        c.readable("status", status)?;
        let priority = r.fields["mapping"]["priority"][&raw_priority]
            .as_str()
            .unwrap_or(match raw_priority.to_lowercase().as_str() {
                "urgent" | "紧急" => "urgent",
                "high" | "高" => "high",
                "medium" | "中" => "medium",
                "low" | "低" => "low",
                "lowest" | "最低" => "lowest",
                _ => "none",
            });
        if !matches!(
            priority,
            "urgent" | "high" | "medium" | "low" | "lowest" | "none"
        ) {
            return Err(invalid("invalid mapped TAPD priority"));
        }
        let mut members = Vec::new();
        let mut unresolved = Vec::new();
        let owners = field(data, owner_field, 4096)?;
        for user in owners.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(local) = r.fields["assignee_map"][user].as_str() {
                c.readable("member", local)?;
                if !members.contains(&local) {
                    members.push(local);
                }
            } else if !unresolved.contains(&user) {
                unresolved.push(user);
            }
        }
        let stamp = field(data, "modified", 32)?;
        let modified = if stamp.is_empty() {
            None
        } else {
            Some(
                NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%d %H:%M:%S")
                    .map_err(|_| invalid("invalid TAPD modified timestamp"))?
                    .and_utc()
                    .timestamp_millis(),
            )
        };
        let external = format!("{kind}:{key}");
        let fields = json!({"title":title,"description":field(data,"description",1_048_576)?,"status":status,"priority":priority,
            "source_key":key,"source_status":raw_status,"source_priority":raw_priority,"source_reporter":field(data,"creator",256)?,
            "source_url":format!("https://www.tapd.cn/tapd_fe/{project}/{kind}/detail/{key}"),"assignees":members,"source_assignees":unresolved});
        let record = json!({"external":external,"modified":modified,"fields":fields,"task_type":if kind=="story"{"requirement"}else{"bug"},
            "historical":mode!="changes","keep_existing":mode=="history"});
        if results
            .insert(external, record.clone())
            .is_some_and(|old| old != record)
        {
            return Err(conflict(
                "TAPD records changed during pagination; preview again",
            ));
        }
    }
    Ok(results.into_values().collect())
}
