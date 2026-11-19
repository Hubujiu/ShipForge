use super::*;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

pub(super) fn supported(f: &Value) -> bool {
    matches!(f["provider"].as_str(), Some("linear" | "clickup" | "ones"))
}
fn resource(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| compact_id(s) && *s != "." && *s != "..")
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
}
fn nested(v: &Value) -> Option<String> {
    if v.is_object() {
        resource(&v["id"])
    } else {
        resource(v)
    }
}

pub(super) fn validate(f: &Value) -> Result<()> {
    for key in ["remote_scope", "enterprise_id"] {
        if let Some(v) = f.get(key).filter(|v| !v.is_null() && **v != "")
            && (resource(v).is_none() || !v.is_string())
        {
            return Err(invalid(
                "the remote account and project/List must be stable IDs",
            ));
        }
    }
    if f.get("cloud_auth").is_some_and(|mode| {
        !matches!(
            (f["provider"].as_str(), mode.as_str()),
            (Some("linear" | "clickup"), Some("key" | "oauth")) | (Some("ones"), Some("key"))
        )
    }) {
        return Err(invalid("unsupported tracker credential mode"));
    }
    if f["provider"] == "ones" {
        let base = trackers::zentao_installation(required(f, "ones_url")?)?;
        let url = url::Url::parse(&base).map_err(|_| invalid("invalid ONES tenant address"))?;
        if matches!(
            url.host_str(),
            Some("open.ones.cn" | "open.ones.com" | "docs.ones.cn" | "docs.ones.com")
        ) || url.path() != "/"
        {
            return Err(invalid(
                "use the ONES regional or self-hosted tenant origin",
            ));
        }
    }
    if f.get("vendor_write").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("vendor_write must be boolean"));
    }
    Ok(())
}

pub(super) fn namespace(f: &Value) -> Result<String> {
    let base = if f["provider"] == "ones" {
        trackers::zentao_installation(required(f, "ones_url")?)?
    } else {
        String::new()
    };
    Ok(format!(
        "{}|{base}|{}",
        required(f, "provider")?,
        required(f, "enterprise_id")?
    ))
}
pub(super) fn sync_control(c: &Context<'_>, r: &Record, action: &str) -> Result<Value> {
    if !supported(&r.fields) {
        return Err(invalid("select a cloud tracker connector"));
    }
    let paused = match action {
        "pause" => true,
        "resume" => false,
        _ => return Err(invalid("select pause or resume")),
    };
    if r.fields["sync_paused"].as_bool().unwrap_or(false) != paused {
        crate::domain::write(
            c,
            &json!({"op":"update","kind":"connector","id":r.id,"expected_rev":r.rev,"fields":{"sync_paused":paused}}),
        )?;
    }
    Ok(json!({"paused":paused}))
}

fn ms_date(v: &Value) -> Option<String> {
    let ms = v.as_i64().or_else(|| v.as_str()?.parse::<i64>().ok())?;
    DateTime::<Utc>::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

fn edges(provider: &str, native: &Value) -> Vec<Value> {
    let mut items = BTreeSet::new();
    let id = nested(&native["id"]).unwrap_or_default();
    let mut add = |kind: &str, from: Option<String>, to: Option<String>| {
        if let (Some(from), Some(to)) = (from, to)
            && from != to
            && (from == id || to == id)
        {
            items.insert((kind.to_owned(), from, to));
        }
    };
    let parent = nested(&native["parent"]);
    if parent.is_some() {
        add("parent", parent, Some(id.clone()));
    }
    if provider == "linear" {
        for edge in native["relations"].as_array().into_iter().flatten() {
            if let Some(kind) = edge["type"]
                .as_str()
                .filter(|k| matches!(*k, "blocks" | "related" | "duplicate"))
            {
                add(kind, Some(id.clone()), nested(&edge["relatedIssue"]));
            }
        }
    } else if provider == "clickup" {
        for edge in native["dependencies"].as_array().into_iter().flatten() {
            add(
                "blocks",
                nested(&edge["depends_on"]),
                nested(&edge["task_id"]),
            );
        }
        for edge in native["linked_tasks"].as_array().into_iter().flatten() {
            add(
                "related",
                nested(&edge["task_id"]),
                nested(&edge["link_id"]),
            );
        }
    }
    items
        .into_iter()
        .map(|(kind, from, to)| json!({"type":kind,"from":from,"to":to}))
        .collect()
}

pub(super) fn normalize(c: &Context<'_>, r: &Record, q: &Value) -> Result<Vec<Value>> {
    if q["source_account"] != r.fields["enterprise_id"]
        || r.fields["enterprise_id"].as_str().is_none_or(str::is_empty)
    {
        return Err(crate::forbidden(
            "the verified source account differs from this binding",
        ));
    }
    let provider = required(&r.fields, "provider")?;
    let rows = q["issues"]
        .as_array()
        .filter(|rows| rows.len() <= 5000)
        .ok_or_else(|| invalid("expected up to 5000 native tasks"))?;
    let mut converted = Vec::new();
    for native in rows {
        let key = resource(&native["id"]).ok_or_else(|| invalid("missing native issue ID"))?;
        let project = if provider == "clickup" {
            &native["list"]
        } else {
            &native["project"]
        };
        if nested(project).as_deref() != r.fields["remote_scope"].as_str() {
            return Err(crate::forbidden(
                "upstream task is outside the bound project or List",
            ));
        }
        let mut row = json!({"id":key,"project":{"id":r.fields["remote_scope"]}});
        let mut warnings = Vec::new();
        match provider {
            "linear" => {
                for (to, from) in [
                    ("title", "title"),
                    ("description", "description"),
                    ("whole_identifier", "identifier"),
                    ("html_url", "url"),
                    ("updated_at", "updatedAt"),
                    ("state", "state"),
                    ("assignee", "assignee"),
                    ("created_by", "creator"),
                ] {
                    row[to] = native[from].clone();
                }
                let p = native["priority"].as_u64().unwrap_or(0);
                row["priority"] = json!({"id":p.to_string(),"name":match p{1=>"Urgent",2=>"High",3=>"Medium",4=>"Low",_=>"None"}});
                row["type"] = json!({"id":"issue"});
            }
            "clickup" => {
                row["title"] = native["name"].clone();
                row["description"] = native["description"].clone();
                row["html_url"] = native["url"].clone();
                row["whole_identifier"] = native["custom_id"]
                    .as_str()
                    .map_or_else(|| json!(key), |id| json!(id));
                row["updated_at"] = json!(
                    ms_date(&native["date_updated"])
                        .ok_or_else(|| invalid("invalid ClickUp modification time"))?
                );
                row["state"] = json!({"id":native["status"]["id"],"name":native["status"]["status"],"type":native["status"]["type"]});
                let p = nested(&native["priority"]).unwrap_or_default();
                row["priority"] = json!({"id":p,"name":match p.as_str(){"1"=>"Urgent","2"=>"High","3"=>"Medium","4"=>"Low",_=>"None"}});
                row["created_by"] = native["creator"].clone();
                if native["time_estimate"].as_i64().is_some_and(|v| v > 0) {
                    warnings.push(
                        json!({"field":"time_estimate","reason":"skipped_external_estimate"}),
                    );
                }
                for field in native["custom_fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| !f["value"].is_null())
                {
                    warnings.push(json!({"field":field["id"],"reason":"unsupported_custom_field"}));
                }
                if native["custom_item_id"].as_i64().is_some_and(|n| n != 0) {
                    warnings.push(
                        json!({"field":"custom_item_id","reason":"using_space_default_type"}),
                    );
                }
            }
            "ones" => {
                row["title"] = native["title"].clone();
                row["description"] = native["descriptionText"].clone();
                row["type"] = native["issueType"].clone();
                row["state"] = json!({"id":native["status"]["id"],"name":native["status"]["name"],"type":native["status"]["category"]});
                row["priority"] = native["priority"].clone();
                row["assignee"] = native["assignee"].clone();
                row["created_by"] = native["creator"].clone();
                row["whole_identifier"] =
                    resource(&native["number"]).map_or_else(|| json!(key), |n| json!(n));
                // ONES listing has no modified-time filter; the confirmed native snapshot is compared field by field.
            }
            _ => return Err(invalid("unknown tracker")),
        }
        converted.push((row, native, warnings));
    }
    let mut transformed = q.clone();
    transformed["issues"] = json!(converted.iter().map(|(r, _, _)| r).collect::<Vec<_>>());
    let mut result = trackers::normalize(c, r, &transformed)?;
    let native_by_key = converted
        .into_iter()
        .map(|(row, native, warnings)| {
            (
                format!("workitem:{}", row["id"].as_str().unwrap()),
                (native, warnings),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    for item in &mut result {
        let (source, extra) = &native_by_key[required(item, "external")?];
        let fields = &mut item["fields"];
        let mut warnings = fields["source_warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        warnings.extend(extra.iter().cloned());
        if provider == "clickup" {
            let mut assigned = BTreeSet::new();
            let mut external = BTreeSet::new();
            for v in source["assignees"].as_array().into_iter().flatten() {
                let key = nested(v).ok_or_else(|| invalid("invalid ClickUp assignee ID"))?;
                if let Some(local) = r.fields["assignee_map"][&key].as_str() {
                    c.readable("member", local)?;
                    assigned.insert(local.to_owned());
                } else {
                    external.insert(key);
                }
            }
            if !external.is_empty() {
                warnings.push(json!({"field":"assignees","reason":"unmapped_identity"}));
            }
            fields["assignees"] = json!(assigned);
            fields["source_assignees"] = json!(external);
        }
        fields["source_warnings"] = json!(warnings);
        fields["source_relations"] = json!(edges(provider, source));
    }
    Ok(result)
}

pub(super) fn connect_relations(c: &Context<'_>, r: &Record) -> Result<Value> {
    let mut stmt=c.db.prepare("SELECT i.connector,i.task,i.birth,i.external FROM sf_import_item i INNER JOIN sf_import_binding b ON b.space=i.space AND b.connector=i.connector WHERE b.org=? AND b.space=? AND b.portal=?").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.org_id()?, c.space, namespace(&r.fields)?], |v| {
            Ok((
                v.get::<_, String>(0)?,
                v.get::<_, String>(1)?,
                v.get::<_, i64>(2)?,
                v.get::<_, String>(3)?,
            ))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let mut mapped = std::collections::HashMap::new();
    let mut desired = BTreeSet::new();
    for (connector, id, birth, external) in rows {
        let Some(source) = c.find("connector", &connector)? else {
            continue;
        };
        let Some(task) = batches::owned(c, &source, &id, birth, &external)? else {
            continue;
        };
        if c.readable("task", &task.id).is_err() {
            continue;
        }
        if let Some(key) = external.strip_prefix("workitem:") {
            mapped.insert(key.to_owned(), task.id);
        }
        for edge in task.fields["source_relations"]
            .as_array()
            .into_iter()
            .flatten()
        {
            desired.insert((
                required(edge, "type")?.to_owned(),
                required(edge, "from")?.to_owned(),
                required(edge, "to")?.to_owned(),
            ));
        }
    }
    let mut existing = c
        .records("relation")?
        .into_iter()
        .filter_map(|r| {
            Some((
                r.fields["type"].as_str()?.to_owned(),
                r.fields["from"].as_str()?.to_owned(),
                r.fields["to"].as_str()?.to_owned(),
            ))
        })
        .collect::<BTreeSet<_>>();
    let mut created = 0;
    let mut skipped = 0;
    for (kind, from, to) in desired {
        let (Some(from), Some(to)) = (mapped.get(&from), mapped.get(&to)) else {
            skipped += 1;
            continue;
        };
        if existing.contains(&(kind.clone(), from.clone(), to.clone()))
            || kind == "related" && existing.contains(&(kind.clone(), to.clone(), from.clone()))
        {
            continue;
        }
        let cmd = json!({"op":"create","kind":"relation","title":"Imported task relation","fields":{"type":kind,"from":from,"to":to}});
        match crate::domain::write(c, &cmd) {
            Ok(_) => {
                existing.insert((kind, from.clone(), to.clone()));
                created += 1;
            }
            Err(err) if err.starts_with("invalid:") || err.starts_with("conflict:") => {
                skipped += 1;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(json!({"created":created,"unavailable_or_conflicting":skipped}))
}
