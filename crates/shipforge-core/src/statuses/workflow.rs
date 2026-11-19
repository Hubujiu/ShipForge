use super::*;

pub(crate) fn default_id(c: &Context<'_>, config: Option<&Record>) -> Result<String> {
    let allowed = config
        .and_then(|t| t.fields["statuses"].as_array())
        .filter(|v| !v.is_empty());
    let enabled_triage = c.get("space", c.space)?.fields["triage_enabled"] == true;
    let permitted = |id: &str| -> Result<bool> {
        let category = c.category_for(id)?;
        Ok(!terminal(&category)
            && (category != "triage" || enabled_triage)
            && allowed.is_none_or(|ls| ls.iter().any(|v| v == id)))
    };
    if let Some(id) = config.and_then(|r| r.fields["default_status"].as_str())
        && permitted(id)?
    {
        return Ok(id.to_owned());
    }
    let default = crate::defaults::selected(c, "status", "todo")?;
    if c.find("status", &default)?.is_some() && permitted(&default)? {
        return Ok(default);
    }
    let options = if let Some(allowed) = allowed {
        allowed
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    } else {
        c.records("status")?
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    for id in options {
        if permitted(&id)? {
            return Ok(id);
        }
    }
    Err(invalid(
        "select an initial nonterminal status for this type",
    ))
}
pub(crate) fn validate(
    c: &Context<'_>,
    old: Option<&Record>,
    id: &str,
    fields: &Value,
) -> Result<()> {
    let states = if fields["statuses"].is_null() {
        Vec::new()
    } else {
        domain::strings(&fields["statuses"], "statuses")?
    };
    if states.len() > 256 || states.iter().collect::<BTreeSet<_>>().len() != states.len() {
        return Err(invalid("choose up to 256 distinct workflow statuses"));
    }
    if let Some(default) = fields.get("default_status").filter(|v| !v.is_null()) {
        let default = default
            .as_str()
            .ok_or_else(|| invalid("default_status must name a status"))?;
        let category = c.category_for(default)?;
        if (!states.is_empty() && !states.contains(&default)) || terminal(&category) {
            return Err(invalid("choose a nonterminal default in this workflow"));
        }
        if category == "triage" && c.get("space", c.space)?.fields["triage_enabled"] != true {
            return Err(invalid(
                "enable triage before using it as the initial status",
            ));
        }
    }
    if let Some(old) = old
        && fields["statuses"] != old.fields["statuses"]
        && !states.is_empty()
        && c.records("task")?.iter().any(|r| {
            r.fields["type"] == id
                && r.fields["status"]
                    .as_str()
                    .is_some_and(|s| !states.contains(&s))
        })
    {
        return Err(conflict(
            "move tasks out of the removed statuses before changing this workflow",
        ));
    }
    Ok(())
}
fn choose(c: &Context<'_>, root: &Context<'_>, input: &Value) -> Result<String> {
    if input.as_object().is_some_and(|m| m.len() != 1) {
        return Err(invalid("choose one organization or space status reference"));
    }
    if let Some(library) = input.get("library").and_then(Value::as_str) {
        return attach(c, &root.readable("status", library)?);
    }
    let id = input
        .as_str()
        .or_else(|| input["local"].as_str())
        .ok_or_else(|| invalid("choose a local status ID or an organization library reference"))?;
    Ok(c.readable("status", id)?.id)
}
pub(super) fn execute(c: &Context<'_>, root: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let mut ty = c.readable("task_type", required(q, "id")?)?;
    if op == "task_type.workflow.save" {
        c.require_admin()?;
        if q.get("expected_rev").is_none() {
            return Err(invalid(
                "expected_rev is required when configuring a workflow",
            ));
        }
        domain::check_rev(q, &ty)?;
        let raw = q["statuses"]
            .as_array()
            .filter(|v| v.len() <= 256)
            .ok_or_else(|| invalid("supply an ordered statuses array"))?;
        let ids = raw
            .iter()
            .map(|v| choose(c, root, v))
            .collect::<Result<Vec<_>>>()?;
        let def = q
            .get("default_status")
            .filter(|v| !v.is_null())
            .map(|v| choose(c, root, v))
            .transpose()?;
        let mut f = ty.fields.clone();
        f["statuses"] = json!(ids);
        f["default_status"] = json!(def);
        validate(c, Some(&ty), &ty.id, &f)?;
        let candidate = Record {
            fields: f.clone(),
            ..ty.clone()
        };
        default_id(c, Some(&candidate))?;
        if f != ty.fields {
            ty = c.update(&ty, &ty.title, f)?;
        }
    }
    let language = q["language"].as_str().unwrap_or("zh-CN");
    let selected = ty.fields["statuses"].as_array().filter(|ls| !ls.is_empty());
    let mut states = c.records("status")?;
    states.sort_by_key(|r| (r.fields["order"].as_i64().unwrap_or(0), r.id.clone()));
    if let Some(ls) = selected {
        states.retain(|r| ls.iter().any(|v| v == &r.id));
        states.sort_by_key(|r| ls.iter().position(|v| v == &r.id));
    }
    Ok(
        json!({"id":ty.id,"rev":ty.rev,"inherits":selected.is_none(),"configured_default":ty.fields["default_status"],"default_status":default_id(c,Some(&ty)).ok(),"statuses":states.iter().map(|r|json!({"id":r.id,"name":preferred(r,language),"category":r.fields["category"],"collapsed":r.fields["collapsed"]==true})).collect::<Vec<_>>() }),
    )
}
