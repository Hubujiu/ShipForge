use super::*;
use chrono::{DateTime, NaiveDateTime};
use std::collections::BTreeMap;

fn canonical_id(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|v| compact_id(v) && *v != "." && *v != "..")
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|id| id.to_string()))
}

/// Canonical installation root used for scope ownership and local credential isolation.
pub fn zentao_installation(input: &str) -> Result<String> {
    use url::{Host, Url};
    if input.len() > 1024 || input.contains(['@', '?', '#', '\\']) || input.trim() != input {
        return Err(invalid(
            "enter the ZenTao installation root without credentials or a page suffix",
        ));
    }
    let url = Url::parse(input).map_err(|_| invalid("invalid ZenTao installation root"))?;
    let local = matches!(url.host(),Some(Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(url.host(),Some(Host::Ipv6(ip)) if ip.is_loopback())
        || url.host_str() == Some("localhost");
    if url.host().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || url.path().contains("//")
        || url.path_segments().into_iter().flatten().any(|s| {
            !s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
                || s.ends_with(".php")
                || s.ends_with(".html")
        })
    {
        return Err(invalid(
            "use an HTTPS installation root or a loopback HTTP installation",
        ));
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

pub(super) fn validate(f: &Value) -> Result<()> {
    let scope = f
        .get("remote_scope")
        .filter(|v| !v.is_null() && v.as_str() != Some(""));
    if f["provider"] == "pingcode" {
        if canonical_id(&f["enterprise_id"]).is_none()
            || !f["enterprise_id"].is_string()
            || scope.is_some_and(|v| canonical_id(v).is_none() || !v.is_string())
        {
            return Err(invalid(
                "enter a PingCode enterprise ID and optional project ID as strings",
            ));
        }
        if f["pingcode_auth"]
            .as_str()
            .is_some_and(|v| !matches!(v, "token" | "application" | "authorization"))
        {
            return Err(invalid("unsupported PingCode credential mode"));
        }
    } else {
        zentao_installation(required(f, "instance_url")?)?;
        if !matches!(f["scope_kind"].as_str(), Some("product" | "project"))
            || scope.is_some_and(|v| v.as_str().is_none_or(|v| !digits(v)))
        {
            return Err(invalid(
                "enter the ZenTao installation root and a product or project scope",
            ));
        }
        if let Some(v) = f["zentao_version"].as_str().filter(|v| !v.is_empty()) {
            let mut parts = v.split('.');
            let version = parts
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .zip(parts.next().and_then(|v| v.parse::<u32>().ok()))
                .ok_or_else(|| invalid("enter a ZenTao version in major.minor form"))?;
            let min = match f["zentao_edition"].as_str().unwrap_or("opensource") {
                "opensource" => (16, 5),
                "enterprise" => (6, 5),
                "ultimate" => (3, 0),
                _ => return Err(invalid("unknown ZenTao edition")),
            };
            if version < min {
                return Err(invalid(
                    "this ZenTao edition and version predates REST API v1",
                ));
            }
        }
    }
    if f.get("vendor_write").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("vendor_write must be boolean"));
    }
    Ok(())
}

fn source_id(v: &Value) -> String {
    if v.is_object() {
        canonical_id(&v["id"]).unwrap_or_default()
    } else {
        canonical_id(v).unwrap_or_default()
    }
}

fn time(v: &Value, seconds: bool) -> Result<Option<i64>> {
    if v.is_null()
        || v.as_str()
            .is_some_and(|t| t.is_empty() || t.starts_with("0000-00-00"))
    {
        return Ok(None);
    }
    if let Some(n) = v.as_i64() {
        return Ok(Some(if seconds || n.unsigned_abs() < 100_000_000_000 {
            n.checked_mul(1000)
                .ok_or_else(|| invalid("source time exceeds range"))?
        } else {
            n
        }));
    }
    let s = v
        .as_str()
        .ok_or_else(|| invalid("invalid source modified time"))?;
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Ok(Some(d.timestamp_millis()));
    }
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|d| Some(d.and_utc().timestamp_millis()))
        .map_err(|_| invalid("invalid source modified time"))
}

fn identity(v: &Value, zen: bool) -> String {
    if zen {
        v["account"]
            .as_str()
            .or(v.as_str())
            .unwrap_or("")
            .to_owned()
    } else {
        source_id(v)
    }
}

fn source_label(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_owned()
    } else if let Some(n) = v.as_i64() {
        n.to_string()
    } else {
        String::new()
    }
}

pub(super) fn normalize(c: &Context<'_>, r: &Record, q: &Value) -> Result<Vec<Value>> {
    let project = required(&r.fields, "remote_scope")?;
    let mode = q["import_mode"].as_str().unwrap_or("history");
    if !matches!(mode, "history" | "changes" | "refresh") {
        return Err(invalid("unknown import mode"));
    }
    let zen = r.fields["provider"] == "zentao";
    let mut items = BTreeMap::new();
    for entry in q["issues"]
        .as_array()
        .filter(|v| v.len() <= 5000)
        .ok_or_else(|| invalid("expected up to 5000 remote issues"))?
    {
        let (row, kind) = if zen {
            (&entry["record"], required(entry, "entity")?)
        } else {
            (entry, "workitem")
        };
        let key =
            canonical_id(&row["id"]).ok_or_else(|| invalid("source work item ID is invalid"))?;
        if zen {
            if !matches!(kind, "story" | "requirement" | "task" | "bug")
                || !digits(&key)
                || entry["scope"] != project
                || entry["scope_kind"] != r.fields["scope_kind"]
            {
                return Err(crate::forbidden("ZenTao record is outside this binding"));
            }
            let foreign = source_id(&row[required(&r.fields, "scope_kind")?]);
            if !foreign.is_empty() && foreign != "0" && foreign != project {
                return Err(crate::forbidden("ZenTao record belongs to another scope"));
            }
        } else if source_id(&row["project"]) != project {
            return Err(crate::forbidden(
                "PingCode work item belongs to another project",
            ));
        }
        let title = required(row, if kind == "task" { "name" } else { "title" })?.trim();
        if title.chars().count() > 500 {
            return Err(invalid("work item title exceeds 500 characters"));
        }
        let description = if zen {
            match kind {
                "bug" => &row["steps"],
                "task" => &row["desc"],
                _ => &row["spec"],
            }
        } else {
            &row["description"]
        };
        let description = description.as_str().unwrap_or_default();
        if description.len() > 1_048_576 {
            return Err(invalid("work item description exceeds 1 MiB"));
        }
        let state_id = if zen {
            source_label(&row["status"])
        } else if r.fields["provider"] == "clickup" && row["state"]["id"].is_null() {
            required(&row["state"], "name")?.to_owned()
        } else {
            source_id(&row["state"])
        };
        let status_name = if zen {
            state_id.as_str()
        } else {
            row["state"]["name"].as_str().unwrap_or("")
        };
        let category = if zen {
            status_name
        } else {
            row["state"]["type"].as_str().unwrap_or("")
        };
        let map = &r.fields["mapping"];
        let status = map["status"][&state_id]
            .as_str()
            .or_else(|| {
                (r.fields["provider"] != "linear")
                    .then(|| map["status"][status_name].as_str())
                    .flatten()
            })
            .unwrap_or(match category {
                "done" | "closed" | "resolved" | "completed" => "done",
                "doing" | "developing" | "in_progress" | "progressing" | "started"
                | "inProgress" => "in_progress",
                "backlog" => "backlog",
                "triage" => "triage",
                "cancel" | "cancelled" | "canceled" => "cancelled",
                _ => "todo",
            });
        c.readable("status", status)?;
        let priority_id = if zen {
            source_label(&row["pri"])
        } else {
            source_id(&row["priority"])
        };
        let priority_name = if zen {
            priority_id.as_str()
        } else {
            row["priority"]["name"].as_str().unwrap_or("")
        };
        let priority = map["priority"][&priority_id]
            .as_str()
            .or(map["priority"][priority_name].as_str())
            .unwrap_or(match priority_name {
                "1" | "紧急" | "Urgent" => "urgent",
                "2" | "高" | "High" => "high",
                "3" | "中" | "普通" | "Medium" => "medium",
                "4" | "低" | "Low" => "low",
                _ => "none",
            });
        if !matches!(priority, "none" | "low" | "medium" | "high" | "urgent") {
            return Err(invalid("invalid mapped priority"));
        }
        let source_type = if zen {
            kind.to_owned()
        } else {
            source_id(&row["type"])
        };
        let default_type = crate::defaults::selected(c, "task_type", "task")?;
        let task_type = map["type"][&source_type].as_str().unwrap_or(if zen {
            match kind {
                "bug" => "bug",
                "task" => "task",
                _ => "requirement",
            }
        } else if r.fields["provider"] == "ones" {
            match row["type"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .as_str()
            {
                "bug" | "defect" | "缺陷" => "bug",
                "story" | "requirement" | "需求" | "用户故事" => "requirement",
                _ => &default_type,
            }
        } else {
            &default_type
        });
        c.readable("task_type", task_type)?;
        let person = identity(
            if zen {
                &row["assignedTo"]
            } else {
                &row["assignee"]
            },
            zen,
        );
        let local = r.fields["assignee_map"][&person].as_str();
        if let Some(local) = local {
            c.readable("member", local)?;
        }
        let mut warnings = Vec::new();
        let unresolved = if local.is_none() && !person.is_empty() {
            warnings.push(json!({"field":"assignee","reason":"unmapped_identity"}));
            vec![person]
        } else {
            vec![]
        };
        let modified = if zen {
            time(&row["lastEditedDate"], false)?.or(time(&row["openedDate"], false)?)
        } else {
            time(&row["updated_at"], true)?
        };
        let external = format!("{kind}:{key}");
        let mut fields = json!({"title":title,"description":description,"source_key":if zen{json!(key)}else{row["whole_identifier"].clone()},
          "source_type":source_type,"source_status":status_name,"source_status_id":state_id,"status":status,
          "source_priority":priority_name,"priority":priority,"source_reporter":identity(if zen{&row["openedBy"]}else{&row["created_by"]},zen),
          "assignees":local.into_iter().collect::<Vec<_>>(),"source_assignees":unresolved});
        if zen {
            let base = zentao_installation(required(&r.fields, "instance_url")?)?;
            let module = if matches!(kind, "requirement" | "story") {
                "story"
            } else {
                kind
            };
            fields["source_url"] = json!(format!(
                "{base}/index.php?m={module}&f=view&{module}ID={key}"
            ));
        } else {
            if let Some(url) = row["html_url"]
                .as_str()
                .filter(|s| s.starts_with("https://") && s.len() < 2048)
            {
                fields["source_url"] = json!(url);
            }
            if let Some(props) = row["properties"].as_object() {
                let source = props
                    .iter()
                    .filter(|(_, v)| !v.is_null())
                    .collect::<BTreeMap<_, _>>();
                if !source.is_empty() {
                    for field in source.keys() {
                        warnings
                            .push(json!({"field":field,"reason":"retained_as_source_property"}));
                    }
                    fields["source_properties"] = json!(source);
                }
            }
        }
        fields["source_warnings"] = json!(warnings);
        fields["type"] = json!(task_type);
        let record = json!({"external":external,"modified":modified,"fields":fields,"task_type":task_type,"historical":mode!="changes","keep_existing":mode=="history"&&zen});
        if items
            .insert(external, record.clone())
            .is_some_and(|old| old != record)
        {
            return Err(conflict(
                "source returned conflicting copies; preview again",
            ));
        }
    }
    Ok(items.into_values().collect())
}
