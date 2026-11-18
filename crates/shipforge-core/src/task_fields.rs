use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Map;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    field: String,
    #[serde(default = "enabled")]
    enabled: bool,
    #[serde(default)]
    required: bool,
}
fn enabled() -> bool {
    true
}

fn bindings(c: &Context<'_>, value: &Value) -> Result<Vec<Binding>> {
    let entries: Vec<Binding> = serde_json::from_value(value.clone())
        .map_err(|_| invalid("field_bindings must list {field, enabled?, required?}"))?;
    let mut seen = BTreeSet::new();
    for binding in &entries {
        if !seen.insert(&binding.field) {
            return Err(invalid("duplicate field binding"));
        }
        c.readable("field", &binding.field)?;
    }
    Ok(entries)
}

fn populated(v: &Value) -> bool {
    !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty())
}

fn removal(fields: &Value, bound: &[Binding]) -> BTreeSet<String> {
    fields
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, value)| populated(value) && !bound.iter().any(|b| &b.field == *id))
        .map(|(id, _)| id.clone())
        .collect()
}

fn confirm(q: &Value, record: &Record, loss: &BTreeSet<String>) -> Result<()> {
    if loss.is_empty() {
        return Ok(());
    }
    let confirmed = q
        .get("confirm_remove")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<BTreeSet<_>>>()
        });
    if q["expected_rev"].as_i64() != Some(record.rev) || confirmed.as_ref() != Some(loss) {
        return Err(conflict(
            "preview the change, then submit expected_rev and the exact confirm_remove field IDs",
        ));
    }
    Ok(())
}

fn target(c: &Context<'_>, id: &str) -> Result<Record> {
    let dst = c.readable("task_type", id)?;
    if dst.fields["enabled"] == false {
        return Err(invalid("this task type is disabled in the space"));
    }
    Ok(dst)
}

pub(crate) fn preview_switch(c: &Context<'_>, q: &Value) -> Result<Value> {
    let task = c.readable("task", required(q, "id")?)?;
    c.authorize_write("task", Some(&task))?;
    crate::domain::check_rev(q, &task)?;
    let dst = target(c, required(q, "target")?)?;
    let bound = dst
        .fields
        .get("field_bindings")
        .map(|v| bindings(c, v))
        .transpose()?;
    let loss = bound
        .as_ref()
        .map(|b| removal(&task.fields["custom"], b))
        .unwrap_or_default();
    let supplied = q.get("custom").and_then(Value::as_object);
    let missing: Vec<_> = dst.fields["required_fields"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|key| {
            !populated(
                supplied
                    .and_then(|m| m.get(*key))
                    .unwrap_or(&task.fields["custom"][*key]),
            )
        })
        .collect();
    let status = q.get("status").unwrap_or(&task.fields["status"]);
    let status_allowed = dst.fields["statuses"]
        .as_array()
        .is_none_or(|ls| ls.is_empty() || ls.contains(status));
    Ok(
        json!({"id":task.id,"expected_rev":task.rev,"target":dst.id,"expected_type_rev":dst.rev,"remove":loss,"missing":missing,"status_allowed":status_allowed,"statuses":dst.fields["statuses"]}),
    )
}

pub(crate) fn prepare_task(
    c: &Context<'_>,
    old: Option<&Record>,
    patch: &mut Map<String, Value>,
    q: &Value,
    importing: bool,
) -> Result<()> {
    let requested = patch
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| old.and_then(|r| r.fields["type"].as_str()));
    let fallback;
    let id = if let Some(v) = requested {
        v
    } else {
        fallback = crate::defaults::selected(c, "task_type", "task")?;
        &fallback
    };
    let Some(config) = c.find("task_type", id)? else {
        return Ok(());
    };
    let switching = old.is_some_and(|r| r.fields["type"] != id);
    if let Some(v) = q.get("expected_type_rev")
        && v.as_i64() != Some(config.rev)
    {
        return Err(conflict("target type changed; preview it again"));
    }
    let bound = config
        .fields
        .get("field_bindings")
        .map(|v| bindings(c, v))
        .transpose()?;
    if bound.is_none() && !switching {
        return Ok(());
    }
    let mut values = old
        .and_then(|r| r.fields["custom"].as_object())
        .cloned()
        .unwrap_or_default();
    let old_values = values.clone();
    if let Some(bound) = &bound
        && let Some(row) = old
        && switching
    {
        let loss = removal(&row.fields["custom"], bound);
        if !loss.is_empty() && q["expected_type_rev"].as_i64() != Some(config.rev) {
            return Err(conflict(
                "preview the target type before confirming removal",
            ));
        }
        confirm(q, row, &loss)?;
        values.retain(|key, _| bound.iter().any(|b| &b.field == key));
    }
    if let Some(custom) = patch.get("custom") {
        let supplied = custom
            .as_object()
            .ok_or_else(|| invalid("custom must be an object"))?;
        for (key, v) in supplied {
            if let Some(bindings) = &bound
                && !bindings
                    .iter()
                    .any(|b| &b.field == key && (b.enabled || importing && old.is_none()))
            {
                if old_values.get(key) == Some(v) {
                    continue;
                }
                return Err(invalid(&format!(
                    "custom field {key} is unavailable for this type"
                )));
            }
            values.insert(key.clone(), v.clone());
        }
    }
    if patch.contains_key("custom") || switching {
        patch.insert("custom".into(), Value::Object(values));
    }
    Ok(())
}

fn impact(
    c: &Context<'_>,
    ty: &Record,
    bound: &[Binding],
) -> Result<(BTreeMap<String, u64>, Vec<Record>)> {
    let mut counts = BTreeMap::new();
    let mut affected = Vec::new();
    for task in c
        .records("task")?
        .into_iter()
        .filter(|r| r.fields["type"] == ty.id)
    {
        let removed = removal(&task.fields["custom"], bound);
        for key in removed {
            *counts.entry(key).or_default() += 1;
        }
        if task.fields["custom"]
            .as_object()
            .is_some_and(|m| m.keys().any(|key| !bound.iter().any(|b| &b.field == key)))
        {
            affected.push(task);
        }
    }
    Ok((counts, affected))
}
fn impact_token(ty: &Record, bound: &[Binding], affected: &[Record]) -> String {
    let rows: BTreeMap<_, _> = affected.iter().map(|r| (&r.id, r.rev)).collect();
    let value = json!([ty.id, ty.rev, bound, rows]).to_string();
    URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        value.as_bytes(),
    ))
}

pub(crate) fn preview_bindings(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_admin()?;
    let ty = c.readable("task_type", required(q, "id")?)?;
    crate::domain::check_rev(q, &ty)?;
    let bound = bindings(c, &q["field_bindings"])?;
    let (counts, affected) = impact(c, &ty, &bound)?;
    Ok(
        json!({"id":ty.id,"expected_rev":ty.rev,"remove":counts.keys().collect::<Vec<_>>(),"counts":counts,"impact_token":impact_token(&ty,&bound,&affected)}),
    )
}

pub(crate) fn prepare_type(
    c: &Context<'_>,
    old: Option<&Record>,
    patch: &mut Map<String, Value>,
    q: &Value,
) -> Result<()> {
    let Some(value) = patch.get("field_bindings") else {
        if old.is_some_and(|r| r.fields.get("field_bindings").is_some())
            && patch
                .get("required_fields")
                .is_some_and(|v| Some(v) != old.and_then(|r| r.fields.get("required_fields")))
        {
            return Err(invalid("edit required fields through field_bindings"));
        }
        return Ok(());
    };
    let bound = bindings(c, value)?;
    if let Some(ty) = old {
        let (counts, affected) = impact(c, ty, &bound)?;
        if !counts.is_empty()
            && q["impact_token"].as_str() != Some(&impact_token(ty, &bound, &affected))
        {
            return Err(conflict(
                "affected tasks changed or removal was not previewed",
            ));
        }
        confirm(q, ty, &counts.into_keys().collect())?;
        for task in affected {
            let mut f = task.fields.clone();
            if let Some(m) = f["custom"].as_object_mut() {
                m.retain(|key, _| bound.iter().any(|b| &b.field == key));
            }
            c.update_mode(&task, &task.title, f, false)?;
        }
    }
    patch.insert(
        "required_fields".into(),
        json!(
            bound
                .iter()
                .filter(|b| b.enabled && b.required)
                .map(|b| &b.field)
                .collect::<Vec<_>>()
        ),
    );
    patch.insert("field_bindings".into(), json!(bound));
    Ok(())
}

pub(crate) fn references(ty: &Record, field: &str) -> bool {
    ty.fields["field_bindings"]
        .as_array()
        .is_some_and(|list| list.iter().any(|b| b["field"] == field))
}
pub(crate) fn validate_destination(c: &Context<'_>, f: &Value) -> Result<()> {
    if let Some(config) = c.find("task_type", required(f, "type")?)?
        && let Some(bound) = config.fields.get("field_bindings")
        && !removal(&f["custom"], &bindings(c, bound)?).is_empty()
    {
        return Err(conflict(
            "the target space task type cannot retain the task's custom field values",
        ));
    }
    Ok(())
}

pub(crate) fn schema(s: &mut Value) {
    let ids = json!({"type":"array","items":{"type":"string","minLength":1},"uniqueItems":true});
    let binding = json!({"type":"array","items":{"type":"object","required":["field"],"additionalProperties":false,"properties":{"field":{"type":"string","minLength":1},"required":{"type":"boolean","default":false},"enabled":{"type":"boolean","default":true}}}});
    s["properties"]["confirm_remove"] = ids;
    s["properties"]["expected_type_rev"] = json!({"type":"integer","minimum":1});
    s["properties"]["impact_token"] = json!({"type":"string","minLength":1});
    s["properties"]["with_total"] = json!({"type":"boolean"});
    s["properties"]["field_bindings"] = binding.clone();
    let rules = s["allOf"].as_array_mut().unwrap();
    rules.extend([
        json!({"if":{"properties":{"op":{"const":"task.type.preview"}}},"then":{"required":["id","target"],"properties":{"target":{"type":"string"},"custom":{"type":"object"},"status":{"type":"string"}}}}),
        json!({"if":{"properties":{"op":{"const":"task_type.bindings.preview"}}},"then":{"required":["id","field_bindings"]}}),
        json!({"if":{"required":["kind"],"properties":{"op":{"enum":["create","update"]},"kind":{"const":"task_type"}}},"then":{"properties":{"fields":{"properties":{"field_bindings":binding}}}}}),
        json!({"if":{"properties":{"op":{"enum":["comment.thread","comment.resolve","comment.reopen"]}}},"then":{"required":["task_id","id"]}}),
        json!({"if":{"properties":{"op":{"const":"run.discussion"}}},"then":{"required":["action"],"properties":{"action":{"enum":["comment","reply","resolve","reopen"]},"comment_id":{"type":"string","minLength":1},"body":{"type":"string","minLength":1}},"allOf":[{"if":{"properties":{"action":{"enum":["comment","reply"]}}},"then":{"required":["body","call_id"]}},{"if":{"properties":{"action":{"enum":["reply","resolve","reopen"]}}},"then":{"required":["comment_id"]}}]}}),
        json!({"if":{"properties":{"op":{"const":"view.share"}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"const":"view.open"}}},"then":{"required":["link"],"properties":{"link":{"type":"string","pattern":"^shipforge://view/"}}}}),
        json!({"if":{"properties":{"op":{"const":"view.save_as"}}},"then":{"required":["id","title"],"properties":{"visibility":{"enum":["private","space"]},"changes":{"type":"object"}}}}),
        json!({"if":{"properties":{"op":{"const":"view.organize"}}},"then":{"required":["id"],"anyOf":[{"required":["pinned"]},{"required":["before"]},{"required":["after_view"]}],"not":{"required":["before","after_view"]},"properties":{"pinned":{"type":"boolean"},"before":{"type":"string"},"after_view":{"type":"string"}}}}),
        json!({"if":{"properties":{"op":{"enum":["view.run","view.preview","view.open"]}}},"then":{"properties":{"changes":{"type":"object"}}}})
    ]);
}
