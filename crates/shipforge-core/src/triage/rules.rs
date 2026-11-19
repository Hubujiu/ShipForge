use super::*;
fn keys(v: &Value, allowed: &[&str]) -> Result<()> {
    if !v
        .as_object()
        .is_some_and(|m| m.keys().all(|k| allowed.contains(&k.as_str())))
    {
        return Err(invalid("unknown triage rule property"));
    }
    Ok(())
}
pub(super) fn validate(c: &Context<'_>, policy: &Value) -> Result<()> {
    let duty = &policy["duty"];
    keys(duty, &["mode", "member"])?;
    match required(duty, "mode")? {
        "none" => {}
        "notify" | "assign" => {
            let m = c.readable("member", required(duty, "member")?)?;
            if !matches!(
                m.fields["role"].as_str(),
                Some("member" | "editor" | "admin" | "owner")
            ) {
                return Err(invalid("select a space reviewer with editing access"));
            }
        }
        _ => return Err(invalid("unknown duty action")),
    }
    let rules = policy["rules"]
        .as_array()
        .filter(|ls| ls.len() <= 50)
        .ok_or_else(|| invalid("rules must list up to 50 entries"))?;
    let mut ids = BTreeSet::new();
    for rule in rules {
        keys(rule, &["id", "title", "enabled", "when", "set"])?;
        let id = required(rule, "id")?;
        identifier(id)?;
        if !ids.insert(id) {
            return Err(invalid("duplicate triage rule id"));
        }
        if rule.get("enabled").is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("rule enabled must be boolean"));
        }
        let conditions = &rule["when"];
        keys(
            conditions,
            &[
                "status",
                "priority",
                "assignee",
                "creator",
                "project",
                "labels",
                "type",
                "source",
                "due_before",
                "due_after",
                "created_after",
                "created_before",
                "contains",
            ],
        )?;
        for (k, v) in conditions.as_object().unwrap() {
            if matches!(k.as_str(), "due_before" | "due_after") {
                crate::domain::valid_date(v)?;
            } else if matches!(k.as_str(), "created_after" | "created_before") {
                if !v.is_i64() {
                    return Err(invalid("rule creation times use Unix milliseconds"));
                }
            } else if k == "labels" {
                crate::domain::strings(v, "labels")?;
            } else if !v.is_string() {
                return Err(invalid("rule condition must be a string"));
            }
        }
        let set = &rule["set"];
        keys(
            set,
            &[
                "space", "status", "assignee", "labels", "project", "priority",
            ],
        )?;
        if set.as_object().unwrap().is_empty() {
            return Err(invalid("a rule needs at least one action"));
        }
        let dest = set
            .get("space")
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| invalid("rule space must be an ID"))
            })
            .transpose()?
            .unwrap_or(c.space);
        let target = Context { space: dest, ..*c };
        if dest != c.space {
            target.require_edit()?;
            if c.org_id()? != target.org_id()? {
                return Err(forbidden("routing target must be in the same organization"));
            }
        }
        for (k, v) in set.as_object().unwrap() {
            match k.as_str() {
                "priority"
                    if !matches!(
                        v.as_str(),
                        Some("none" | "low" | "medium" | "high" | "urgent")
                    ) =>
                {
                    return Err(invalid("unknown rule priority"));
                }
                "status" => {
                    target.category_for(required(set, k)?)?;
                }
                "assignee" if !v.is_null() => {
                    target.readable("member", required(set, k)?)?;
                }
                "labels" => {
                    for id in crate::domain::strings(v, k)? {
                        crate::tags::normalize(&target, &Value::Null, &json!([id]))?;
                    }
                }
                "project" if !v.is_null() => {
                    crate::tasks::resolve_project(&target, required(set, k)?)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}
pub(super) fn conflicts(rules: &Value) -> Vec<Value> {
    let mut found = Vec::new();
    let ls = rules.as_array().map(|v| v.as_slice()).unwrap_or_default();
    for (i, r) in ls.iter().enumerate().filter(|(_, r)| r["enabled"] != false) {
        for prior in &ls[..i] {
            if prior["enabled"] == false {
                continue;
            }
            for (key, v) in r["set"].as_object().into_iter().flatten() {
                if prior["set"].get(key).is_some_and(|old| old != v) {
                    found.push(json!({"field":key,"earlier":prior["id"],"later":r["id"]}));
                }
            }
        }
    }
    found
}
fn matches(task: &Record, f: &Value) -> bool {
    f.as_object().is_some_and(|m| {
        m.iter().all(|(key, val)| match key.as_str() {
            "contains" => format!(
                "{} {}",
                task.title,
                task.fields["description"].as_str().unwrap_or("")
            )
            .to_lowercase()
            .contains(&val.as_str().unwrap_or("").to_lowercase()),
            "due_before" | "due_after" => task.fields["due_date"].as_str().is_some_and(|d| {
                if key == "due_before" {
                    d <= val.as_str().unwrap_or("")
                } else {
                    d >= val.as_str().unwrap_or("")
                }
            }),
            "created_after" => task.created_at >= val.as_i64().unwrap_or(0),
            "created_before" => task.created_at <= val.as_i64().unwrap_or(0),
            "labels" => val.as_array().is_some_and(|ids| {
                ids.iter().all(|id| {
                    task.fields["labels"]
                        .as_array()
                        .is_some_and(|ls| ls.contains(id))
                })
            }),
            "assignee" => {
                task.fields["assignees"]
                    .as_array()
                    .is_some_and(|p| p.contains(val))
                    || task.fields["assignee"] == *val
            }
            _ => task.fields[key] == *val,
        })
    })
}
pub(super) fn apply(c: &Context<'_>, row: &Record) -> Result<Record> {
    let Some((manager, policy)) = super::policy(c)? else {
        return Ok(row.clone());
    };
    let admin = Context {
        actor: &manager,
        ..*c
    };
    let duty = &policy["duty"];
    if admin.require_edit().is_err() || !admin.visible(row)? {
        return Ok(row.clone());
    }
    if duty["mode"] == "notify" {
        crate::inbox::task_notice(
            &admin,
            row,
            required(duty, "member")?,
            "triage_duty",
            None,
            true,
        )?;
    }
    let mut current = row.clone();
    let mut patch = json!({});
    let mut dest = c.space;
    let mut applied = Vec::new();
    if duty["mode"] == "assign" {
        patch["assignees"] = json!([duty["member"]]);
        current.fields["assignees"] = patch["assignees"].clone();
    }
    for rule in policy["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["enabled"] != false)
    {
        if !matches(&current, &rule["when"]) {
            continue;
        }
        applied.push(&rule["id"]);
        for (k, v) in rule["set"].as_object().into_iter().flatten() {
            match k.as_str() {
                "space" => dest = v.as_str().unwrap_or(dest),
                "assignee" => {
                    let people = v.as_str().map(|s| vec![s]).unwrap_or_default();
                    patch["assignees"] = json!(people);
                    current.fields["assignees"] = json!(people);
                }
                _ => {
                    patch[k] = v.clone();
                    current.fields[k] = v.clone();
                }
            }
        }
    }
    if patch.as_object().unwrap().is_empty() && applied.is_empty() {
        return Ok(row.clone());
    }
    let mut run = || -> Result<Record> {
        let mut current = row.clone();
        if dest != c.space {
            current = serde_json::from_value(crate::task_commands::move_with_status(
                &admin,
                &json!({"id":row.id,"target":dest,"expected_rev":row.rev}),
                patch["status"].as_str(),
            )?)
            .map_err(storage)?;
        }
        let target = Context {
            space: dest,
            ..admin
        };
        patch["triage_applied_rules"] = json!(applied);
        serde_json::from_value(crate::domain::write_review(
            &target,
            &json!({"op":"update","kind":"task","id":current.id,"fields":patch}),
        )?)
        .map_err(storage)
    };
    c.db.execute_batch("SAVEPOINT triage_routing")
        .map_err(storage)?;
    match run() {
        Ok(r) => {
            c.db.execute_batch("RELEASE triage_routing")
                .map_err(storage)?;
            Ok(r)
        }
        Err(e) => {
            c.db.execute_batch("ROLLBACK TO triage_routing; RELEASE triage_routing")
                .map_err(storage)?;
            if e.starts_with("storage:") {
                return Err(e);
            }
            let mut f = row.fields.clone();
            f["triage_rule_failed"] = json!(true);
            c.update_mode(row, &row.title, f, false)
        }
    }
}
