//! Space-scoped directories, management permissions, and invocation policy.
mod variants;
use crate::{
    Context, Record, Result, as_value, conflict, domain, forbidden, invalid, required, storage,
    worker,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) use variants::{effective, resource};
pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS agent_grants(space TEXT,agent TEXT,actor TEXT,permission TEXT NOT NULL,PRIMARY KEY(space,agent,actor));
        CREATE TABLE IF NOT EXISTS agent_personal(space TEXT,kind TEXT,id TEXT,actor TEXT,pinned INTEGER NOT NULL,favorite INTEGER NOT NULL,PRIMARY KEY(space,kind,id,actor));").map_err(storage)
}
pub(crate) fn handles(op: &str) -> bool {
    matches!(
        op,
        "agent.directory"
            | "agent.settings"
            | "agent.history"
            | "agent.archive"
            | "agent.restore"
            | "agent.variant"
            | "agent.override"
            | "agent.access"
            | "agent.organize"
            | "crew.exclusive"
    )
}
fn kind(q: &Value) -> Result<&str> {
    match q["kind"].as_str().unwrap_or("agent") {
        k @ ("agent" | "crew") => Ok(k),
        _ => Err(invalid("select an agent or crew")),
    }
}
fn owner(row: &Record) -> &str {
    row.fields["owner"]
        .as_str()
        .or(row.fields["creator"].as_str())
        .unwrap_or("owner")
}
fn permission(c: &Context<'_>, row: &Record) -> Result<Option<String>> {
    if row.kind != "agent" {
        return Ok(None);
    }
    c.db.query_row(
        "SELECT permission FROM agent_grants WHERE space=? AND agent=? AND actor=?",
        params![row.space, row.id, c.actor],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}
fn base_visible(c: &Context<'_>, row: &Record) -> Result<bool> {
    Ok(owner(row) == c.actor
        || row.fields["visibility"] != "private"
        || permission(c, row)?.is_some())
}
fn is_private(c: &Context<'_>, row: &Record) -> Result<bool> {
    if row.fields["visibility"].is_null() && row.fields["parent_space"].is_string() {
        return Ok(variants::source(c, row)?.1.fields["visibility"] == "private");
    }
    Ok(row.fields["visibility"] == "private")
}
pub(crate) fn visible(c: &Context<'_>, row: &Record) -> Result<bool> {
    if !base_visible(c, row)? {
        return Ok(false);
    }
    if let Some(id) = row.fields["exclusive_crew"].as_str() {
        let Some(crew) = c.find("crew", id)? else {
            return Ok(false);
        };
        if !base_visible(c, &crew)? {
            return Ok(false);
        }
    }
    if row.kind == "agent" && row.fields["parent_space"].is_string() {
        return variants::source(c, row)
            .and_then(|(_, parent)| {
                Ok(!row.fields["visibility"].is_null()
                    || parent.fields["visibility"] != "private"
                    || owner(row) == c.actor
                    || permission(c, row)?.is_some())
            })
            .or_else(|e| {
                if e.starts_with("forbidden:") || e.starts_with("not_found:") {
                    Ok(false)
                } else {
                    Err(e)
                }
            });
    }
    Ok(true)
}
fn manager(c: &Context<'_>, row: &Record) -> Result<bool> {
    Ok(owner(row) == c.actor || permission(c, row)?.as_deref() == Some("manage"))
}
pub(crate) fn require_manage(c: &Context<'_>, row: &Record) -> Result<()> {
    c.require_edit()?;
    if manager(c, row)? {
        Ok(())
    } else {
        Err(forbidden("handler management access required"))
    }
}
pub(crate) fn discoverable(row: &Record) -> bool {
    row.fields["archived"] != true && !row.fields["exclusive_crew"].is_string()
}
pub(crate) fn run_source_visible(c: &Context<'_>, run: &Record) -> Result<bool> {
    let Some(id) = run.fields["config"]["variant_id"].as_str() else {
        return Ok(true);
    };
    let Some(r) = c.find("agent", id)? else {
        return Ok(false);
    };
    if !visible(c, &r)? {
        return Ok(false);
    }
    for field in ["skills", "tools"] {
        let kind = if field == "skills" { "skill" } else { "tool" };
        if run.fields["config"]["field_spaces"][field]
            .as_str()
            .is_some_and(|s| s != c.space)
        {
            for item in run.fields["config"][field]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if resource(c, &run.fields["config"], field, kind, item).is_err() {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
pub(crate) fn requester_can_continue(c: &Context<'_>, run: &Record) -> Result<bool> {
    let requester = Context {
        actor: required(&run.fields, "requested_by")?,
        ..*c
    };
    if requester.role().is_err() {
        return Ok(false);
    }
    let Some(handler) = requester.find(
        required(&run.fields, "handler_kind")?,
        required(&run.fields, "handler_id")?,
    )?
    else {
        return Ok(false);
    };
    Ok(visible(&requester, &handler)? && run_source_visible(&requester, run)?)
}
pub(crate) fn machine_eligible(c: &Context<'_>, machine: &Record, run: &Record) -> Result<bool> {
    let config = &run.fields["config"];
    let Some(origin) = config["field_spaces"]["machine_id"]
        .as_str()
        .filter(|space| *space != c.space)
    else {
        return Ok(true);
    };
    if config["machine_id"] != machine.id {
        return Ok(false);
    }
    c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_machine_keys s JOIN sf_machine_keys d ON d.machine=s.machine AND d.owner=s.owner AND d.key_hash=s.key_hash WHERE s.space=? AND d.space=? AND s.machine=?)",params![origin,c.space,machine.id],|r|r.get(0)).map_err(storage)
}
pub(crate) fn usage(report: &Value) -> Result<Option<Value>> {
    let Some(v) = report.get("usage").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    if !v.is_object() {
        return Err(invalid("usage must contain token counts"));
    }
    let mut total = 0_u64;
    for key in ["input_tokens", "output_tokens", "total_tokens"] {
        if let Some(n) = v.get(key) {
            let n = n
                .as_u64()
                .ok_or_else(|| invalid("token counts must be nonnegative integers"))?;
            if key == "total_tokens" {
                total = n;
            } else {
                total = total
                    .checked_add(n)
                    .ok_or_else(|| invalid("token count overflow"))?;
            }
        }
    }
    if !["input_tokens", "output_tokens", "total_tokens"]
        .iter()
        .any(|k| v.get(k).is_some())
    {
        return Err(invalid("no token count supplied"));
    }
    Ok(Some(
        json!({"input_tokens":v["input_tokens"],"output_tokens":v["output_tokens"],"total_tokens":total}),
    ))
}
pub(crate) fn forget(c: &Context<'_>, actor: &str) -> Result<()> {
    for table in ["agent_grants", "agent_personal"] {
        c.db.execute(&format!("DELETE FROM {table} WHERE actor=?1 AND (space=?2 OR space IN (SELECT space FROM records WHERE kind='space' AND id=space AND json_extract(fields,'$.organization')=?2))"),params![actor,c.space]).map_err(storage)?;
    }
    Ok(())
}
pub(crate) fn usable(
    c: &Context<'_>,
    kind: &str,
    id: &str,
    parent_run: Option<&str>,
) -> Result<Record> {
    let agent = c.readable(kind, id)?;
    if agent.fields["archived"] == true {
        return Err(conflict("this handler is archived"));
    }
    if let Some(crew) = agent.fields["exclusive_crew"].as_str() {
        let parent = parent_run.map(|id| c.get("run", id)).transpose()?;
        if !parent.is_some_and(|r| {
            r.fields["handler_kind"] == "crew"
                && r.fields["handler_id"] == crew
                && r.fields["requested_by"] == c.actor
        }) {
            return Err(forbidden("this Agent can only work inside its crew"));
        }
    }
    if kind == "crew" {
        let leader = c.readable("agent", required(&agent.fields, "leader_id")?)?;
        if !discoverable(&leader) {
            return Err(conflict("crew leader unavailable"));
        }
    }
    Ok(agent)
}
pub(crate) fn chat_available(c: &Context<'_>, room: &Record) -> Result<()> {
    if let Some(id) = room.fields["handler_id"]
        .as_str()
        .or(room.fields["agent"].as_str())
        .filter(|s| !s.is_empty())
    {
        usable(
            c,
            room.fields["handler_kind"].as_str().unwrap_or("agent"),
            id,
            None,
        )?;
    }
    Ok(())
}
pub(crate) fn validate(c: &Context<'_>, old: Option<&Record>, row: &Record) -> Result<()> {
    let f = &row.fields;
    match row.kind.as_str() {
        "agent" => variants::validate(c, old, row)?,
        "crew" => {
            for id in f["members"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .chain(f["leader_id"].as_str())
            {
                if let Some(agent) = c.find("agent", id)?
                    && agent.fields["exclusive_crew"]
                        .as_str()
                        .is_some_and(|crew| crew != row.id || f["leader_id"] == id)
                {
                    return Err(conflict(
                        "exclusive Agents may only be members of their own crew",
                    ));
                }
            }
            if let Some(before) = old {
                for id in before.fields["members"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if c.find("agent", id)?
                        .is_some_and(|a| a.fields["exclusive_crew"] == row.id)
                        && !f["members"]
                            .as_array()
                            .is_some_and(|v| v.contains(&json!(id)))
                    {
                        return Err(conflict("remove exclusivity before removing this member"));
                    }
                }
            }
        }
        "chat"
            if old.is_none()
                || old.is_some_and(|r| {
                    r.fields["agent"] != f["agent"]
                        || r.fields["handler_id"] != f["handler_id"]
                        || r.fields["handler_kind"] != f["handler_kind"]
                }) =>
        {
            chat_available(c, row)?
        }
        "comment"
            if old.is_none()
                || old.is_some_and(|r| r.fields["agent_mentions"] != f["agent_mentions"]) =>
        {
            if let Some(ids) = f.get("agent_mentions") {
                for id in domain::strings(ids, "agent_mentions")? {
                    usable(c, "agent", id, None)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn protect(q: &Value, old: Option<&Record>) -> Result<()> {
    let f = &q["fields"];
    for key in [
        "parent_space",
        "variant_creator",
        "exclusive_crew",
        "archived",
        "personal",
        "can_manage",
        "inherited_fields",
    ] {
        if f.get(key).is_some() {
            return Err(invalid(
                "use the directory operation for managed handler state",
            ));
        }
    }
    if let Some(r) = old {
        if f.get("parent_id")
            .is_some_and(|v| v != &r.fields["parent_id"])
        {
            return Err(invalid("a variant cannot change its source"));
        }
        if f.get("owner").is_some_and(|v| v.as_str() != Some(owner(r))) {
            return Err(invalid("use agent.access to transfer ownership"));
        }
    }
    Ok(())
}
pub(crate) fn mentioned(c: &Context<'_>, old: Option<&Record>, comment: &Record) -> Result<()> {
    let Some(ids) = comment.fields["agent_mentions"].as_array() else {
        return Ok(());
    };
    let task = c.readable("task", required(&comment.fields, "task")?)?;
    let mut dispatched = std::collections::HashSet::new();
    for id in ids.iter().filter_map(Value::as_str) {
        if !dispatched.insert(id)
            || old.is_some_and(|r| {
                r.fields["agent_mentions"]
                    .as_array()
                    .is_some_and(|v| v.contains(&json!(id)))
            })
        {
            continue;
        }
        let room = crate::task_commands::conversation_for(c, &task, "agent", id)?;
        crate::chat::execute(
            c,
            &json!({"id":room["id"],"body":comment.fields["body"]}),
            "chat.send",
        )?;
    }
    Ok(())
}
fn personal(c: &Context<'_>, r: &Record) -> Result<(bool, bool)> {
    c.db.query_row(
        "SELECT pinned,favorite FROM agent_personal WHERE space=? AND kind=? AND id=? AND actor=?",
        params![r.space, r.kind, r.id, c.actor],
        |x| Ok((x.get(0)?, x.get(1)?)),
    )
    .optional()
    .map_err(storage)
    .map(Option::unwrap_or_default)
}
fn decorate(c: &Context<'_>, mut r: Record) -> Result<Record> {
    if r.fields["visibility"].is_null() {
        r.fields["visibility"] = json!(if is_private(c, &r)? {
            "private"
        } else {
            "space"
        });
    }
    let (pinned, favorite) = personal(c, &r)?;
    r.fields["personal"] = json!({"pinned":pinned,"favorite":favorite});
    r.fields["can_manage"] = json!(manager(c, &r)?);
    r.fields["can_variant"] = json!(
        r.kind == "agent"
            && discoverable(&r)
            && !r.fields["parent_id"].is_string()
            && variants::can_create(c, &r).is_ok()
    );
    r.fields["directory_owner"] = json!(owner(&r));
    Ok(r)
}
fn directory(c: &Context<'_>, q: &Value) -> Result<Value> {
    let k = kind(q)?;
    let view = q["view"].as_str().unwrap_or("all");
    if !matches!(
        view,
        "all" | "mine" | "shared" | "private" | "archived" | "favorite"
    ) {
        return Err(invalid("unknown directory view"));
    }
    let text = q["query"].as_str().unwrap_or("").trim().to_lowercase();
    let all = q["scope"] == "organization";
    let mut rows = Vec::new();
    for space in c.all_spaces()? {
        if !all && space.id != c.space {
            continue;
        }
        let ctx = Context {
            space: &space.id,
            ..*c
        };
        if ctx.role().is_err() || ctx.org_id()? != c.org_id()? {
            continue;
        }
        for r in ctx.records(k)? {
            if r.fields["exclusive_crew"].is_string()
                || !ctx.visible(&r)?
                || (r.fields["archived"] == true) != (view == "archived")
            {
                continue;
            }
            let private = is_private(&ctx, &r)?;
            if (view == "mine" && r.fields["creator"] != c.actor)
                || (view == "shared" && private)
                || (view == "private" && !private)
            {
                continue;
            }
            if !text.is_empty()
                && !r.title.to_lowercase().contains(&text)
                && !r.fields["description"]
                    .as_str()
                    .is_some_and(|s| s.to_lowercase().contains(&text))
            {
                continue;
            }
            let mut r = decorate(&ctx, r)?;
            if view == "favorite" && r.fields["personal"]["favorite"] != true {
                continue;
            }
            r.fields["space_title"] = json!(space.title);
            rows.push(r);
        }
    }
    rows.sort_by(|a, b| {
        (
            a.fields["personal"]["pinned"] != true,
            &a.space,
            &a.title,
            &a.id,
        )
            .cmp(&(
                b.fields["personal"]["pinned"] != true,
                &b.space,
                &b.title,
                &b.id,
            ))
    });
    Ok(json!(rows))
}
fn monitored(c: &Context<'_>, r: &Record, room: &Record) -> Result<bool> {
    Ok(r.kind == "agent"
        && room.fields["visibility"] == "private"
        && room.fields["handler_kind"].as_str().unwrap_or("agent") == "agent"
        && room.fields["handler_id"]
            .as_str()
            .or(room.fields["agent"].as_str())
            == Some(&r.id)
        && crate::chat::history_owner(c, room)?.is_some()
        && manager(c, r)?)
}
fn history(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    if is_private(c, r)? && owner(r) != c.actor {
        return Err(forbidden("the owner can view a private handler's history"));
    }
    if let Some(id) = q["chat"].as_str() {
        let room = c.get("chat", id)?;
        if !monitored(c,r,&room)? || !c.db.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE space=? AND kind='run' AND json_extract(fields,'$.handler_kind')='agent' AND json_extract(fields,'$.handler_id')=? AND json_extract(fields,'$.chat_id')=?)",params![c.space,r.id,id],|v|v.get::<_,bool>(0)).map_err(storage)?{return Err(forbidden("the Agent cannot inspect this conversation"));}
        return Ok(
            json!({"title":room.title,"messages":crate::chat::manager_transcript(c,&room)?}),
        );
    }
    let mut groups = BTreeMap::<String, Value>::new();
    for run in c.records("run")? {
        if run.fields["handler_kind"] != r.kind || run.fields["handler_id"] != r.id {
            continue;
        }
        let chat = run.fields["chat_id"].as_str();
        let task = run.fields["task_id"].as_str();
        let key = chat
            .map(|v| format!("chat:{v}"))
            .or_else(|| task.map(|v| format!("task:{v}")))
            .unwrap_or_else(|| format!("run:{}", run.id));
        if groups.contains_key(&key) {
            continue;
        }
        let row = if c.visible(&run)? { Some(&run) } else { None };
        let related = chat
            .map(|id| ("chat", id))
            .or_else(|| task.map(|id| ("task", id)))
            .and_then(|(kind, id)| c.readable(kind, id).ok());
        let manage = manager(c, r)?;
        let related = related.filter(|target| {
            target.kind != "chat"
                || target.fields["visibility"] != "private"
                || crate::chat::history_owner(c, target)
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(c.actor)
                || manage
        });
        if row.is_none() && chat.is_none() {
            continue;
        }
        let value = if let Some(target) = related {
            json!({"kind":target.kind,"id":target.id,"title":target.title,"status":run.fields["status"],"at":run.updated_at})
        } else if let Some(id) = chat
            && let Some(room) = c.find("chat", id)?
            && monitored(c, r, &room)?
        {
            json!({"kind":"manager_transcript","id":room.id,"title":room.title,"at":run.updated_at})
        } else if chat.is_some() {
            json!({"masked":true,"title":"私人对话","at":run.updated_at})
        } else if row.is_some() {
            json!({"kind":"run","id":run.id,"title":run.title,"status":run.fields["status"],"at":run.updated_at})
        } else {
            continue;
        };
        groups.insert(key, value);
    }
    let mut rows = groups.into_values().collect::<Vec<_>>();
    rows.sort_by_key(|v| std::cmp::Reverse(v["at"].as_i64().unwrap_or(0)));
    Ok(json!(rows))
}
fn organize(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    let (p, f) = personal(c, r)?;
    let (p, f) = match required(q, "action")? {
        "pin" => (true, f),
        "unpin" => (false, f),
        "favorite" => (true, true),
        "unfavorite" => (p, false),
        _ => return Err(invalid("unknown directory action")),
    };
    c.db.execute("INSERT INTO agent_personal VALUES(?,?,?,?,?,?) ON CONFLICT(space,kind,id,actor) DO UPDATE SET pinned=excluded.pinned,favorite=excluded.favorite",params![c.space,r.kind,r.id,c.actor,p,f]).map_err(storage)?;
    Ok(json!({"id":r.id,"pinned":p,"favorite":f}))
}
fn lifecycle(c: &Context<'_>, r: &Record, archived: bool) -> Result<Value> {
    require_manage(c, r)?;
    if (r.fields["archived"] == true) == archived {
        return Ok(as_value(r.clone()));
    }
    let mut f = r.fields.clone();
    f["archived"] = json!(archived);
    let updated = c.update(r, &r.title, f)?;
    if archived && r.kind == "agent" {
        for row in c.records("run")? {
            if row.fields["handler_id"] == r.id && row.fields["handler_kind"] == r.kind
                || r.kind == "agent" && row.fields["config"]["agent_id"] == r.id
            {
                worker::cancel(c, &c.get("run", &row.id)?)?;
            }
        }
    }
    Ok(as_value(updated))
}
fn access(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    require_manage(c, r)?;
    let actor = required(q, "actor")?;
    if actor != "owner" {
        c.readable("member", actor)?;
    }
    let level = required(q, "permission")?;
    if level == "transfer" {
        if owner(r) != c.actor {
            return Err(forbidden("only the owner can transfer a handler"));
        }
        if r.kind == "agent" {
            c.db.execute(
                "INSERT OR REPLACE INTO agent_grants VALUES(?,?,?,'manage')",
                params![c.space, r.id, c.actor],
            )
            .map_err(storage)?;
        }
        let mut f = r.fields.clone();
        f["owner"] = json!(actor);
        return Ok(as_value(c.update(r, &r.title, f)?));
    }
    if r.kind != "agent" || actor == owner(r) {
        return Err(invalid("choose another Agent member"));
    }
    match level {
        "use" | "manage" => {
            c.db.execute(
                "INSERT OR REPLACE INTO agent_grants VALUES(?,?,?,?)",
                params![c.space, r.id, actor, level],
            )
            .map_err(storage)?;
        }
        "remove" => {
            c.db.execute(
                "DELETE FROM agent_grants WHERE space=? AND agent=? AND actor=?",
                params![c.space, r.id, actor],
            )
            .map_err(storage)?;
        }
        _ => {
            return Err(invalid(
                "permission must be use, manage, remove or transfer",
            ));
        }
    }
    Ok(json!({"id":r.id,"actor":actor,"permission":level}))
}
fn exclusive(c: &Context<'_>, crew: &Record, q: &Value) -> Result<Value> {
    require_manage(c, crew)?;
    let member = required(q, "agent")?;
    let acting = c.readable("agent", member)?;
    require_manage(c, &acting)?;
    let on = q["exclusive"]
        .as_bool()
        .ok_or_else(|| invalid("exclusive must be boolean"))?;
    if crew.fields["leader_id"] == member
        || !crew.fields["members"]
            .as_array()
            .is_some_and(|xs| xs.contains(&json!(member)))
    {
        return Err(invalid("select a non-leader Agent from this crew"));
    }
    if on
        && (acting.fields["exclusive_crew"]
            .as_str()
            .is_some_and(|id| id != crew.id)
            || c.records("crew")?.iter().any(|r| {
                r.id != crew.id
                    && (r.fields["leader_id"] == member
                        || r.fields["members"]
                            .as_array()
                            .is_some_and(|v| v.contains(&json!(member))))
            }))
    {
        return Err(conflict("Agent already belongs to another crew"));
    }
    let mut f = acting.fields.clone();
    f["exclusive_crew"] = if on { json!(crew.id) } else { Value::Null };
    Ok(as_value(c.update(&acting, &acting.title, f)?))
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op == "agent.directory" {
        return directory(c, q);
    }
    let k = if op == "crew.exclusive" {
        "crew"
    } else {
        kind(q)?
    };
    let r = c.readable(k, required(q, "id")?)?;
    if !matches!(op, "agent.settings" | "agent.history") {
        domain::check_rev(q, &r)?;
    }
    match op {
        "agent.settings" => variants::settings(c, &r),
        "agent.history" => history(c, &r, q),
        "agent.variant" => variants::create(c, &r, q),
        "agent.override" => variants::change(c, &r, q),
        "agent.organize" => organize(c, &r, q),
        "agent.access" => access(c, &r, q),
        "agent.archive" | "agent.restore" => lifecycle(c, &r, op == "agent.archive"),
        "crew.exclusive" => exclusive(c, &r, q),
        _ => Err(invalid("unknown directory operation")),
    }
}
pub(crate) fn replay(c: &Context<'_>, q: &Value, result: &Value) -> Result<()> {
    let op = required(q, "op")?;
    let r = c.readable(
        if op == "crew.exclusive" {
            "crew"
        } else {
            kind(q)?
        },
        required(q, "id")?,
    )?;
    if op == "agent.variant" {
        variants::can_create(c, &r)?;
        let target = Context {
            space: required(result, "space")?,
            ..*c
        };
        target.role()?;
        target.readable("agent", required(result, "id")?)?;
    } else if op != "agent.organize" {
        require_manage(c, &r)?;
    }
    Ok(())
}
pub(crate) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"enum":["agent.settings","agent.history","agent.archive","agent.restore","agent.variant","agent.override","agent.organize","agent.access","crew.exclusive"]}}},"then":{"required":["id"]}}));
    for (op, keys) in [
        ("agent.variant", vec!["target_space"]),
        ("agent.override", vec!["field"]),
        ("agent.organize", vec!["action"]),
        ("agent.access", vec!["actor", "permission"]),
        ("crew.exclusive", vec!["agent", "exclusive"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":keys}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"const":"run.report"}}},"then":{"properties":{"usage":{"type":["object","null"],"properties":{"input_tokens":{"type":"integer","minimum":0},"output_tokens":{"type":"integer","minimum":0},"total_tokens":{"type":"integer","minimum":0}}}}}}));
}
