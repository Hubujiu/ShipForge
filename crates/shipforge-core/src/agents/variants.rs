use super::*;
use std::collections::HashSet;

pub(crate) const FIELDS: &[&str] = &[
    "runtime",
    "model",
    "environment",
    "arguments",
    "timeout_seconds",
    "instructions",
    "description",
    "avatar",
    "agent_type",
    "directory_mode",
    "working_hours",
    "max_parallel",
    "chat_parallel",
    "machine_id",
    "profile",
    "repository",
    "tools",
    "skills",
    "visibility",
];
pub(super) fn source(c: &Context<'_>, r: &Record) -> Result<(String, Record)> {
    let id = required(&r.fields, "parent_id")?;
    let space = r.fields["parent_space"].as_str().unwrap_or(&r.space);
    let origin = Context { space, ..*c };
    if origin.org_id()? != c.org_id()? {
        return Err(forbidden("variant source unavailable"));
    }
    let parent = origin.get("agent", id)?;
    if !base_visible(&origin, &parent)? {
        return Err(forbidden("variant source unavailable"));
    }
    if origin.role().is_err()
        && (parent.fields["visibility"] == "private"
            || origin.get("space", space)?.fields["visibility"] != "public")
    {
        return Err(forbidden("source space unavailable"));
    }
    if parent.fields["exclusive_crew"].is_string() {
        return Err(forbidden("source Agent is crew-exclusive"));
    }
    Ok((space.to_owned(), parent))
}
pub(super) fn can_create(c: &Context<'_>, r: &Record) -> Result<()> {
    if r.kind != "agent" || !discoverable(r) || r.fields["parent_id"].is_string() {
        return Err(invalid("select an active original Agent"));
    }
    if r.fields["variant_permission"] == "manage" && !manager(c, r)? {
        return Err(forbidden("Agent managers create its variants"));
    }
    Ok(())
}
pub(super) fn validate(c: &Context<'_>, old: Option<&Record>, row: &Record) -> Result<()> {
    let f = &row.fields;
    for field in [
        "runtime",
        "model",
        "instructions",
        "description",
        "avatar",
        "agent_type",
        "directory_mode",
        "machine_id",
        "profile",
        "repository",
        "visibility",
    ] {
        if f.get(field).is_some_and(|v| !v.is_null() && !v.is_string()) {
            return Err(invalid(&format!("{field} must be a string")));
        }
    }
    if f.get("agent_type")
        .is_some_and(|v| !v.is_null() && !matches!(v.as_str(), Some("office" | "code")))
    {
        return Err(invalid("unknown Agent type"));
    }
    if f.get("timeout_seconds")
        .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| n == 0))
    {
        return Err(invalid("timeout must be a positive integer"));
    }
    if f.get("variant_permission")
        .is_some_and(|v| !matches!(v.as_str(), Some("use" | "manage")))
    {
        return Err(invalid("variant permission must be use or manage"));
    }
    if f.get("allow_variant_environment")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err(invalid("allow_variant_environment must be boolean"));
    }
    if f.get("environment").is_some_and(|v| {
        !v.is_null()
            && !v
                .as_object()
                .is_some_and(|xs| xs.iter().all(|(k, v)| !k.is_empty() && v.is_string()))
    }) {
        return Err(invalid("environment must contain named strings"));
    }
    if f.get("arguments").is_some_and(|v| {
        !v.is_null()
            && !v
                .as_array()
                .is_some_and(|xs| xs.iter().all(Value::is_string))
    }) {
        return Err(invalid("arguments must be strings"));
    }
    if let Some(parent) = f["parent_id"].as_str() {
        if old.is_none() {
            let (space, base) = source(c, row)?;
            let origin = Context {
                space: &space,
                ..*c
            };
            can_create(&origin, &base)?;
            if parent == row.id && space == row.space {
                return Err(invalid("cyclic Agent variant"));
            }
        }
        if f["skills"].is_array() && old.is_none_or(|r| r.fields["skills"] != f["skills"]) {
            return Err(invalid(
                "variant skills are inherited from the original Agent",
            ));
        }
    }
    Ok(())
}
pub(crate) fn effective(c: &Context<'_>, id: &str) -> Result<Value> {
    let mut chain = vec![c.readable("agent", id)?];
    let mut seen = HashSet::new();
    while let Some(r) = chain.last() {
        if !seen.insert((r.space.clone(), r.id.clone())) || seen.len() > 32 {
            return Err(invalid("cyclic Agent inheritance"));
        }
        if !r.fields["parent_id"].is_string() {
            break;
        }
        let ctx = Context {
            space: &r.space,
            ..*c
        };
        let (_, root) = source(&ctx, r)?;
        chain.push(root);
    }
    let mut data = json!({"runtime":"local-tool","directory_mode":"isolated","skills":[],"tools":[],"instructions":"","max_parallel":1,"chat_parallel":1,"field_spaces":{}});
    if chain.len() > 1 {
        data["variant_id"] = json!(id);
    }
    for item in chain.iter().rev() {
        if item.fields["visibility"] == "private" {
            data["private_source"] = json!(true);
        }
        if item.fields["parent_id"].is_string()
            && let Some(parent) = chain.iter().find(|v| {
                v.id == item.fields["parent_id"]
                    && v.space == item.fields["parent_space"].as_str().unwrap_or(&item.space)
            })
            && parent.fields["allow_variant_environment"] == false
            && item.fields["variant_creator"]
                .as_str()
                .or(item.fields["creator"].as_str())
                != Some(owner(parent))
        {
            data.as_object_mut().unwrap().remove("environment");
        }
        for &key in FIELDS {
            if key == "skills" && item.fields["parent_id"].is_string() {
                continue;
            }
            if let Some(v) = item.fields.get(key).filter(|v| !v.is_null()) {
                data[key] = v.clone();
                data["field_spaces"][key] = json!(item.space);
            }
        }
    }
    Ok(data)
}
pub(crate) fn resource(
    c: &Context<'_>,
    config: &Value,
    field: &str,
    kind: &str,
    id: &str,
) -> Result<Record> {
    let origin = config["field_spaces"][field].as_str().unwrap_or(c.space);
    let src = Context {
        space: origin,
        ..*c
    };
    if src.org_id()? != c.org_id()? {
        return Err(forbidden("resource space unavailable"));
    }
    src.role()?;
    src.readable(kind, id)
}
pub(super) fn settings(c: &Context<'_>, r: &Record) -> Result<Value> {
    let record = decorate(c, r.clone())?;
    let effective = if r.kind == "agent" {
        effective(c, &r.id)?
    } else {
        Value::Null
    };
    let overrides = FIELDS
        .iter()
        .filter(|k| r.fields.get(**k).is_some_and(|v| !v.is_null()))
        .collect::<Vec<_>>();
    let mut access = Vec::new();
    if r.kind == "agent" && manager(c, r)? {
        let mut stmt=c.db.prepare_cached("SELECT actor,permission FROM agent_grants WHERE space=? AND agent=? ORDER BY actor").map_err(storage)?;
        for entry in stmt
            .query_map(params![c.space, r.id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?
        {
            let (a, p) = entry.map_err(storage)?;
            access.push(json!({"actor":a,"permission":p}));
        }
    }
    let usage = if !is_private(c, r)? || owner(r) == c.actor {
        let mut total = None;
        for run in c.records("run")? {
            if run.fields["handler_id"] == r.id
                && run.fields["handler_kind"] == r.kind
                && let Some(n) = run.fields["usage"]["total_tokens"].as_u64()
            {
                total = Some(total.unwrap_or(0_u64).saturating_add(n));
            }
        }
        total
    } else {
        None
    };
    let mut members = Vec::new();
    for id in r.fields["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if let Ok(agent) = c.readable("agent", id) {
            members.push(json!({"id":id,"title":agent.title,"exclusive":agent.fields["exclusive_crew"]==r.id,"leader":r.fields["leader_id"]==id}));
        }
    }
    Ok(
        json!({"record":record,"effective":effective,"overrides":overrides,"access":access,"members":members,"total_tokens":usage}),
    )
}
pub(super) fn create(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    can_create(c, r)?;
    let target = required(q, "target_space")?;
    let dst = Context {
        space: target,
        ..*c
    };
    dst.require_edit()?;
    if dst.org_id()? != c.org_id()? {
        return Err(forbidden("variants must stay in the same organization"));
    }
    let title = q["title"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{} · 分身", r.title));
    if title.trim().is_empty() || title.chars().count() > 500 {
        return Err(invalid("invalid variant name"));
    }
    let mut fields = json!({"creator":c.actor,"owner":c.actor,"parent_id":r.id,"parent_space":c.space,"variant_creator":c.actor});
    if let Some(v) = q.get("visibility") {
        if !matches!(v.as_str(), Some("space" | "private" | "public")) {
            return Err(invalid("unknown Agent visibility"));
        }
        fields["visibility"] = v.clone();
    }
    let id = q
        .get("variant_id")
        .map(|v| {
            v.as_str()
                .ok_or_else(|| invalid("variant_id must be an identifier"))
        })
        .transpose()?;
    if let Some(id) = id {
        crate::identifier(id)?;
    }
    let variant = dst.create("agent", id, &title, fields)?;
    worker::validate(&dst, None, &variant)?;
    Ok(as_value(variant))
}
pub(super) fn change(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    require_manage(c, r)?;
    if !r.fields["parent_id"].is_string() {
        return Err(invalid("choose an Agent variant"));
    }
    let key = required(q, "field")?;
    if key == "skills" || !FIELDS.contains(&key) {
        return Err(invalid("choose an individually overridable setting"));
    }
    let mut fields = r.fields.clone();
    if q["value"].is_null() {
        fields.as_object_mut().unwrap().remove(key);
    } else {
        fields[key] = q["value"].clone();
    }
    if key == "visibility"
        && !fields[key].is_null()
        && !matches!(fields[key].as_str(), Some("private" | "space" | "public"))
    {
        return Err(invalid("unknown variant visibility"));
    }
    let candidate = Record {
        fields: fields.clone(),
        ..r.clone()
    };
    worker::validate(c, Some(r), &candidate)?;
    Ok(as_value(c.update(r, &r.title, fields)?))
}
