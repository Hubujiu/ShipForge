use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn valid_token(s: &str) -> bool {
    let value = s.strip_prefix('/').unwrap_or(s);
    !value.is_empty()
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}
pub(crate) fn advertised_commands(value: &Value, runtimes: &[&str]) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let all = value
        .as_object()
        .filter(|m| m.len() <= 64)
        .ok_or_else(|| invalid("slash_commands must be a map of enabled runtimes"))?;
    let mut out = json!({});
    for (runtime, names) in all {
        if !runtimes.contains(&runtime.as_str()) {
            return Err(invalid("commands must belong to an enabled runtime"));
        }
        let entries = crate::domain::strings(names, "slash_commands")?;
        if entries.len() > 200 {
            return Err(invalid("too many Runtime commands"));
        }
        let mut listing = BTreeSet::new();
        for token in entries {
            if !valid_token(token) {
                return Err(invalid("invalid slash command name"));
            }
            listing.insert(format!("/{}", token.trim_start_matches('/')));
        }
        out[runtime] = json!(listing);
    }
    Ok(out)
}
pub(super) fn catalog(c: &Context<'_>, kind: &str, handler: &str, search: &str) -> Result<Value> {
    crate::agents::usable(c, kind, handler, None)?;
    let cfg = crate::worker::snapshot(c, handler, kind)?;
    let runtime = required(&cfg, "runtime")?;
    let at = now();
    let mut machines = Vec::new();
    for m in c.records("machine")? {
        if crate::worker::machine_is_online_at(&m.fields, at)
            && (m.fields["owner"] == c.actor || m.fields["shared"] == true)
            && c.visible(&m)?
            && cfg["machine_id"]
                .as_str()
                .is_none_or(|id| id.is_empty() || id == m.id)
            && m.fields["runtimes"]
                .as_array()
                .is_some_and(|ls| ls.contains(&json!(runtime)))
        {
            machines.push(m);
        }
    }
    let search = search.trim_start_matches('/').trim().to_lowercase();
    let busy = c.records("run")?.iter().any(|r| {
        r.fields["handler_kind"] == kind
            && r.fields["handler_id"] == handler
            && matches!(r.fields["status"].as_str(), Some("running" | "cancelling"))
    });
    let mut items = BTreeMap::new();
    let mut known = true;
    let mut common: Option<BTreeSet<String>> = None;
    for m in &machines {
        let Some(names) = m.fields["slash_commands"][runtime].as_array() else {
            known = false;
            common = Some(BTreeSet::new());
            continue;
        };
        let names: BTreeSet<String> = names
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        common = Some(match common.take() {
            None => names,
            Some(old) => old.intersection(&names).cloned().collect(),
        });
    }
    if !machines.is_empty() {
        for token in common.unwrap_or_default() {
            items.insert(
                ("command", token.clone()),
                json!({"group":"command","token":token,"name":token}),
            );
        }
        for skill in cfg["skill_versions"].as_array().into_iter().flatten() {
            let id = required(skill, "id")?;
            if !valid_token(id) {
                continue;
            }
            let token = format!("/{id}");
            items.insert(("skill",token.clone()),json!({"group":"skill","id":id,"token":token,"name":skill["title"],"rev":skill["rev"]}));
        }
    }
    let mut items: Vec<_> = items
        .into_values()
        .filter(|v| {
            v["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&search)
                || v["token"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&search)
        })
        .collect();
    items.sort_by_key(|v| {
        let folded = |key: &str| {
            v[key]
                .as_str()
                .unwrap_or("")
                .trim_start_matches('/')
                .to_lowercase()
        };
        let name = folded("name");
        let token = folded("token");
        (
            v["group"] != "skill",
            name != search && token != search,
            !name.starts_with(&search) && !token.starts_with(&search),
            name,
            token,
        )
    });
    Ok(
        json!({"handler":handler,"handler_kind":kind,"runtime":runtime,"online":!machines.is_empty(),"busy":busy,"commands_reported":!machines.is_empty()&&known,"items":items}),
    )
}
pub(super) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let room = c.readable("chat", required(q, "id")?)?;
    let (kind, id) = crate::chat::handler(c, &room)?;
    let menu = catalog(c, kind, id, q["query"].as_str().unwrap_or(""))?;
    if op == "chat.slash" {
        return Ok(menu);
    }
    c.require_edit()?;
    if menu["online"] != true {
        return Err(conflict("the Agent runtime is offline"));
    }
    let body = required(q, "body")?;
    let name = body.split_whitespace().next().unwrap_or("");
    if !menu["items"]
        .as_array()
        .is_some_and(|items| items.iter().any(|r| r["token"] == name))
    {
        return Err(invalid(
            "the Runtime no longer advertises this command or skill",
        ));
    }
    let mut result = crate::chat::execute(c, &json!({"id":room.id,"body":body}), "chat.send")?;
    result["chat"] = json!(room.id);
    Ok(result)
}
