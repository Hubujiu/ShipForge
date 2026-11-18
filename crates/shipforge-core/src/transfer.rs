use super::*;
use crate::query::visible_records;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const FORMAT: &str = "shipforge.workspace.v1";
const PORTABLE: &[&str] = &[
    "field",
    "status",
    "task_type",
    "relation_type",
    "tag_group",
    "tag",
    "project",
    "iteration",
    "task",
    "relation",
    "view",
    "document",
    "customer_request",
    "note",
    "chat",
    "message",
    "comment",
    "artifact",
];
pub(crate) fn export(c: &Context<'_>) -> Result<Value> {
    let mut records = Vec::new();
    for kind in PORTABLE {
        records.extend(
            visible_records(c, kind)?
                .into_iter()
                .filter(|row| row.space == c.space)
                .map(|mut row| {
                    if row.kind == "task"
                        && let Some(f) = row.fields.as_object_mut()
                    {
                        f.remove("following");
                    }
                    row
                })
                .filter(|r| !(crate::defaults::builtin(kind, &r.id) && r.rev == 1)),
        );
    }
    Ok(json!({"format":FORMAT,"space":c.space,"records":records}))
}

pub(crate) fn import(c: &Context<'_>, q: &Value, preview: bool) -> Result<Value> {
    c.require_edit()?;
    let data = &q["data"];
    let mapped;
    let input = if data["format"] == "shipforge.rows.v1" {
        mapped = map_rows(data)?;
        &mapped
    } else if let Some(items) = data.as_array() {
        items
    } else {
        if data["format"] != FORMAT {
            return Err(invalid("unsupported import format"));
        }
        data["records"]
            .as_array()
            .ok_or_else(|| invalid("import data requires a records array"))?
    };
    if input.len() > 10000 {
        return Err(invalid(
            "import supports at most 10000 records in one transaction",
        ));
    }
    // Use the live validation and storage paths under a savepoint for an exact preview.
    c.db.execute_batch("SAVEPOINT portable_import")
        .map_err(storage)?;
    let result = apply(c, input);
    if preview || result.is_err() {
        c.db.execute_batch("ROLLBACK TO portable_import")
            .map_err(storage)?;
    }
    c.db.execute_batch("RELEASE portable_import")
        .map_err(storage)?;
    let mut result = result?;
    if data["format"] == "shipforge.rows.v1" {
        result["format"] = data["format"].clone();
        if preview {
            result["mapping"] = data["mapping"].clone();
            result["sample"] = json!(input.iter().take(5).collect::<Vec<_>>());
        }
    }
    Ok(result)
}

fn map_rows(data: &Value) -> Result<Vec<Value>> {
    let kind = required(data, "kind")?;
    if !PORTABLE.contains(&kind) {
        return Err(invalid("this record kind cannot be imported"));
    }
    let rows = data["rows"]
        .as_array()
        .filter(|r| r.len() <= 10000)
        .ok_or_else(|| invalid("rows must be an array with up to 10000 objects"))?;
    let mapping = data["mapping"]
        .as_object()
        .filter(|m| !m.is_empty() && m.len() <= 256)
        .ok_or_else(|| invalid("mapping requires 1–256 source columns"))?;
    let mut targets = HashSet::new();
    for (column, value) in mapping {
        let path = value
            .as_str()
            .ok_or_else(|| invalid("mapping targets must be strings"))?;
        if column.trim().is_empty() || !targets.insert(path) {
            return Err(invalid("empty source column or repeated mapping target"));
        }
        if matches!(path, "id" | "title") {
            continue;
        }
        let key = path
            .strip_prefix("fields.custom.")
            .or_else(|| path.strip_prefix("fields."))
            .ok_or_else(|| invalid("map columns to id, title or fields.<name>"))?;
        if key.is_empty()
            || key.contains('.')
            || matches!(
                path,
                "fields.creator"
                    | "fields.number"
                    | "fields.system"
                    | "fields.connector"
                    | "fields.custom"
            )
        {
            return Err(invalid("invalid or reserved mapping target"));
        }
    }
    if !targets.contains("title")
        || !(targets.contains("id") || targets.contains("fields.external_id"))
    {
        return Err(invalid("map a title and a stable id or fields.external_id"));
    }
    let connector = if targets.contains("fields.external_id") {
        Some(required(data, "connector")?)
    } else {
        None
    };
    let mut records = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let cells = row
            .as_object()
            .ok_or_else(|| invalid(&format!("row {} must be an object", index + 1)))?;
        let mut result = json!({"kind":kind,"fields":{}});
        for (column, target) in mapping {
            let target = target.as_str().expect("validated mapping");
            let Some(value) = cells.get(column).filter(|v| !v.is_null()) else {
                continue;
            };
            match target {
                "id" | "fields.external_id" => {
                    let id = match value {
                        Value::String(s) if !s.trim().is_empty() => s.to_owned(),
                        Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
                        _ => {
                            return Err(invalid(&format!(
                                "row {} has an invalid identity",
                                index + 1
                            )));
                        }
                    };
                    if target == "id" {
                        result["id"] = json!(id);
                    } else {
                        result["fields"]["external_id"] = json!(id);
                    }
                }
                "title" => result["title"] = value.clone(),
                target if target.starts_with("fields.custom.") => {
                    result["fields"]["custom"][&target[14..]] = value.clone()
                }
                target => result["fields"][&target[7..]] = value.clone(),
            }
        }
        if required(&result, "title").is_err() {
            return Err(invalid(&format!("row {} requires a text title", index + 1)));
        }
        if let Some(connector) = connector {
            let external = required(&result["fields"], "external_id")
                .map_err(|_| invalid(&format!("row {} requires an external id", index + 1)))?
                .to_owned();
            result["fields"]["connector"] = json!(connector);
            if result["id"].is_null() {
                let digest =
                    Sha256::digest(json!([kind, connector, external]).to_string().as_bytes());
                let hex = digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                result["id"] = json!(format!("import-{hex}"));
            }
        }
        required(&result, "id")
            .map_err(|_| invalid(&format!("row {} requires an id", index + 1)))?;
        records.push(result);
    }
    Ok(records)
}

fn apply(c: &Context<'_>, input: &[Value]) -> Result<Value> {
    let mut pending = Vec::new();
    let mut ids = HashSet::new();
    let mut aliases = HashMap::new();
    let mut skipped = 0;
    for raw in input {
        let kind = required(raw, "kind")?;
        let id = required(raw, "id")?;
        if !PORTABLE.contains(&kind) {
            return Err(invalid(&format!("cannot import kind {kind}")));
        }
        identifier(id)?;
        let title = required(raw, "title")?;
        if !ids.insert((kind.to_owned(), id.to_owned())) {
            return Err(conflict("duplicate identifier in import file"));
        }
        let mut op = "create";
        if let Some(existing) = c.find(kind, id)? {
            if !c.visible(&existing)? {
                return Err(forbidden("import identifier unavailable"));
            }
            if crate::defaults::builtin(kind, id)
                && existing.rev == 1
                && raw["fields"]["system"] == true
            {
                op = "update";
            } else {
                skipped += 1;
                continue;
            }
        }
        let mut f = crate::domain::fields_object(raw.get("fields"))?;
        if let (Some(provider), Some(external)) = (
            f.get("connector").and_then(Value::as_str),
            f.get("external_id").and_then(Value::as_str),
        ) {
            let mapped:Option<String>=c.db.query_row("SELECT id FROM records WHERE space=?1 AND kind=?2 AND json_extract(fields,'$.connector')=?3 AND json_extract(fields,'$.external_id')=?4",params![c.space,kind,provider,external],|r|r.get(0)).optional().map_err(storage)?;
            if let Some(found) = mapped {
                c.readable(kind, &found)?;
                aliases.insert((kind.to_owned(), id.to_owned()), found);
                skipped += 1;
                continue;
            }
        }
        f.remove("creator");
        f.remove("number");
        f.remove("system");
        if let Some(files) = f.remove("attachments") {
            let mut metadata = files;
            if let Some(entries) = metadata.as_array_mut() {
                for entry in entries {
                    if let Some(item) = entry.as_object_mut() {
                        item.remove("id");
                    }
                }
            }
            f.insert("source_attachments".into(), metadata);
        }
        let entry = json!({"op":op,"kind":kind,"id":id,"title":title,"fields":f});
        pending.push(entry);
    }
    pending.sort_by_key(|e| PORTABLE.iter().position(|k| Some(*k) == e["kind"].as_str()));
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut warnings = Vec::new();
    let mut closing = Vec::new();
    while !pending.is_empty() {
        let mut deferred = Vec::new();
        let mut last = String::new();
        let start = pending.len();
        for mut request in pending {
            let kind = required(&request, "kind")?.to_owned();
            let id = required(&request, "id")?.to_owned();
            let f = &mut request["fields"];
            for (key, target) in [
                ("task", "task"),
                ("from", "task"),
                ("to", "task"),
                ("parent", if kind == "comment" { "comment" } else { "task" }),
                ("iteration", "iteration"),
                ("project", "project"),
                ("chat", "chat"),
                ("message", "message"),
                ("document", "document"),
                ("artifact", "artifact"),
            ] {
                if let Some(original) = f[key].as_str()
                    && let Some(mapped) = aliases.get(&(target.to_owned(), original.to_owned()))
                {
                    f[key] = json!(mapped);
                }
            }
            if kind == "iteration"
                && let Some(owner) = f["owner"].as_str()
                && c.find("member", owner)?.is_none()
            {
                f["owner"] = json!(c.actor);
            }
            if kind == "task" {
                crate::task_commands::source_members::imported(c, f)?;
                if let Some(actor) = f["owner"].as_str()
                    && actor != "owner"
                    && c.find("member", actor)?.is_none()
                {
                    f["owner"] = json!(c.actor);
                    warnings.push(format!(
                        "{id}: imported task owner mapped to the importing member"
                    ));
                }
                if let Some(assignee) = f["assignee"].as_str() {
                    let k = f["assignee_kind"].as_str().unwrap_or("member");
                    if assignee != "owner" && c.find(k, assignee)?.is_none() {
                        f["assignee"] = Value::Null;
                        warnings.push(format!("{id}: absent handler cleared"));
                    }
                }
            }
            if kind == "iteration" && f["status"] == "closed" {
                f["status"] = json!("planned");
                closing.push(id.clone());
            }
            if kind == "space" || f.get("spaces").is_some() {
                f["spaces"] = json!([c.space]);
            }
            if kind == "artifact" && f["run"].is_string() {
                f["source_run"] = f["run"].clone();
                f["run"] = Value::Null;
            }
            c.db.execute_batch("SAVEPOINT import_row")
                .map_err(storage)?;
            match crate::domain::write_import(c, &request, false) {
                Ok(_) => {
                    if kind == "task" {
                        crate::onboarding::imported_tasks(c)?;
                    }
                    c.db.execute_batch("RELEASE import_row").map_err(storage)?;
                    if request["op"] == "update" {
                        updated += 1;
                    } else {
                        created += 1;
                    }
                }
                Err(e) if e.starts_with("not_found:") => {
                    c.db.execute_batch("ROLLBACK TO import_row; RELEASE import_row")
                        .map_err(storage)?;
                    last = e;
                    deferred.push(request);
                }
                Err(e) => return Err(e),
            }
        }
        if deferred.len() == start {
            return Err(invalid(&format!("unresolved import reference ({last})")));
        }
        pending = deferred;
    }
    for id in closing {
        let row = c.get("iteration", &id)?;
        let mut fields = row.fields.clone();
        fields["status"] = json!("closed");
        c.update(&row, &row.title, fields)?;
    }
    Ok(
        json!({"format":FORMAT,"total":input.len(),"created":created,"updated":updated,"skipped":skipped,"warnings":warnings}),
    )
}
