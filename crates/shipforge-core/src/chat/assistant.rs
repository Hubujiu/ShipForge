//! A private, system-managed direct assistant for one member in one organization.
use super::*;
use crate::{as_value, now, worker};
use std::collections::BTreeMap;

pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS sf_personal_assistant ON records(space,json_extract(fields,'$.owner')) WHERE kind='assistant';
        CREATE INDEX IF NOT EXISTS sf_assistant_rooms ON records(space,json_extract(fields,'$.handler_id'),updated_at) WHERE kind='chat' AND json_extract(fields,'$.handler_kind')='assistant';").map_err(storage)
}
fn profile(c: &Context<'_>) -> Result<Option<Record>> {
    let id=c.db.query_row("SELECT id FROM records WHERE space=? AND kind='assistant' AND json_extract(fields,'$.owner')=?",params![c.space,c.actor],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    id.map(|id| c.readable("assistant", &id)).transpose()
}
fn ensure(c: &Context<'_>) -> Result<Record> {
    if let Some(p) = profile(c)? {
        return Ok(p);
    }
    c.create("assistant",None,"小助手",json!({"visibility":"private","owner":c.actor,"creator":c.actor,"instructions":"Help the current member with their direct request. Use only the context supplied by this conversation.","directory_mode":"isolated","agent_type":"office","max_parallel":1,"chat_parallel":1,"skills":[],"tools":[]}))
}
fn machines(
    c: &Context<'_>,
    room_scope: Option<&str>,
) -> Result<BTreeMap<String, Vec<(String, bool)>>> {
    let mut installed = BTreeMap::<String, Vec<(String, bool)>>::new();
    let at = now();
    let mut stmt=c.db.prepare("SELECT m.space,m.fields FROM records m JOIN records s ON s.kind='space' AND s.id=s.space AND s.id=m.space WHERE m.kind='machine' AND json_extract(m.fields,'$.owner')=?1 AND json_extract(s.fields,'$.organization')=?2 AND (?3 IS NULL OR m.space=?3) ORDER BY m.space,m.id").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.actor, c.space, room_scope], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?;
    for row in rows {
        let (scope, raw) = row.map_err(storage)?;
        let local = Context {
            space: &scope,
            ..*c
        };
        if local.require_edit().is_err() || crate::spaces::ensure_active(&local).is_err() {
            continue;
        }
        let f: Value = serde_json::from_str(&raw).map_err(storage)?;
        if f["disabled"] == true {
            continue;
        }
        let online = worker::machine_is_online_at(&f, at);
        for name in f["runtimes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            installed
                .entry(name.to_owned())
                .or_default()
                .push((scope.clone(), online));
        }
    }
    Ok(installed)
}
pub(crate) fn is_room(room: &Record) -> bool {
    room.fields["handler_kind"] == "assistant"
}
fn rooms(c: &Context<'_>, profile: Option<&Record>) -> Result<Vec<Value>> {
    let Some(profile) = profile else {
        return Ok(Vec::new());
    };
    let mut stmt=c.db.prepare("SELECT r.space,r.id,coalesce((SELECT max(m.updated_at) FROM records m WHERE m.space=r.space AND m.kind='message' AND json_extract(m.fields,'$.chat')=r.id),r.updated_at) AS activity FROM records r JOIN records s ON s.kind='space' AND s.id=s.space AND s.id=r.space WHERE json_extract(s.fields,'$.organization')=? AND r.kind='chat' AND json_extract(r.fields,'$.handler_kind')='assistant' AND json_extract(r.fields,'$.handler_id')=? ORDER BY activity DESC,r.id").map_err(storage)?;
    let ids = stmt
        .query_map(params![c.space, profile.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?;
    let mut recent = Vec::new();
    for result in ids {
        let (scope, id) = result.map_err(storage)?;
        let local = Context {
            space: &scope,
            ..*c
        };
        if local.role().is_ok()
            && let Ok(room) = local.readable("chat", &id)
        {
            recent.push(as_value(room));
            if recent.len() == 200 {
                break;
            }
        }
    }
    Ok(recent)
}
fn summary(c: &Context<'_>, p: Option<&Record>, room_scope: Option<&str>) -> Result<Value> {
    let runtimes = machines(c, room_scope)?
        .into_iter()
        .map(|(name, scopes)| (name, scopes.iter().any(|(_, online)| *online)))
        .collect::<BTreeMap<_, _>>();
    let previous = p.and_then(|r| r.fields["runtime"].as_str());
    let selected = previous
        .filter(|id| runtimes.get(*id) == Some(&true))
        .or_else(|| {
            runtimes
                .iter()
                .find(|(_, online)| **online)
                .map(|(id, _)| id.as_str())
        });
    let model = p
        .filter(|_| previous == selected)
        .and_then(|r| r.fields["model"].as_str());
    Ok(
        json!({"organization":c.space,"id":p.map(|r|&r.id),"title":"小助手","model":model,"runtime":selected,"runtimes":runtimes.iter().map(|(name,online)|json!({"id":name,"online":online})).collect::<Vec<_>>(),"rooms":rooms(c,p)?}),
    )
}
fn model(q: &Value) -> Result<Option<String>> {
    if q.is_null() {
        return Ok(None);
    }
    let value = q
        .as_str()
        .filter(|s| s.chars().count() <= 256 && !s.chars().any(char::is_control))
        .ok_or_else(|| invalid("model must be a single name up to 256 characters"))?
        .trim();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}
pub(crate) fn send(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    if [
        "task",
        "task_id",
        "target",
        "instructions",
        "handler_id",
        "members",
    ]
    .iter()
    .any(|key| q.get(key).is_some())
    {
        return Err(invalid(
            "the system assistant accepts direct messages and Runtime settings only",
        ));
    }
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    root.role()?;
    let profile = ensure(&root)?;
    let prior = if let Some(id) = q.get("id") {
        let id = id
            .as_str()
            .ok_or_else(|| invalid("invalid assistant conversation"))?;
        let scope:String=root.db.query_row("SELECT space FROM records WHERE kind='chat' AND id=? AND json_extract(fields,'$.handler_kind')='assistant' AND json_extract(fields,'$.handler_id')=?",params![id,profile.id],|r|r.get(0)).optional().map_err(storage)?.ok_or_else(||format!("not_found: chat {id}"))?;
        let local = Context {
            space: &scope,
            ..*c
        };
        if local.org_id()? != org {
            return Err(forbidden("assistant conversation unavailable"));
        }
        local.require_edit()?;
        Some(local.readable("chat", id)?)
    } else {
        None
    };
    if prior.as_ref().is_some_and(|r| {
        !is_room(r) || r.fields["handler_id"] != profile.id || r.fields["creator"] != c.actor
    }) {
        return Err(forbidden(
            "this is not your personal assistant conversation",
        ));
    }
    let body = required(q, "body")?;
    let locations = machines(&root, prior.as_ref().map(|room| room.space.as_str()))?;
    let installed = locations
        .iter()
        .map(|(name, scopes)| (name.as_str(), scopes.iter().any(|(_, online)| *online)))
        .collect::<BTreeMap<_, _>>();
    let previous = prior
        .as_ref()
        .and_then(|r| r.fields["runtime"].as_str())
        .or(profile.fields["runtime"].as_str());
    let explicit = q
        .get("runtime")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_str()
                .ok_or_else(|| invalid("select an installed Runtime"))
        })
        .transpose()?;
    let runtime=explicit.or_else(||previous.filter(|r|installed.get(*r)==Some(&true))).or_else(||installed.iter().find(|(_,online)|**online).map(|(r,_)|*r)).filter(|rt|installed.get(*rt)==Some(&true)).ok_or_else(||conflict("connect your machine in this conversation's execution space and select an available Runtime"))?.to_owned();
    let scope = locations[&runtime]
        .iter()
        .filter(|(_, online)| *online)
        .min_by_key(|(scope, _)| scope != c.space)
        .expect("online Runtime has a location")
        .0
        .as_str();
    let local = Context { space: scope, ..*c };
    let c = &local;
    let selected = if let Some(value) = q.get("model") {
        model(value)?
    } else if previous == Some(runtime.as_str()) {
        model(
            prior
                .as_ref()
                .map_or(&profile.fields["model"], |r| &r.fields["model"]),
        )?
    } else {
        None
    };
    let room = if let Some(room) = prior {
        if room.fields["runtime"] != runtime || room.fields["model"] != json!(selected) {
            let mut f = room.fields.clone();
            f["runtime"] = json!(runtime);
            f["model"] = json!(selected);
            c.update(&room, &room.title, f)?
        } else {
            room
        }
    } else {
        let title = body
            .trim()
            .lines()
            .next()
            .unwrap_or(body)
            .chars()
            .take(55)
            .collect::<String>();
        c.create("chat",None,&title,json!({"visibility":"private","chat_type":"conversation","creator":c.actor,"handler_kind":"assistant","handler_id":profile.id,"runtime":runtime,"model":selected}))?
    };
    if profile.fields["runtime"] != runtime || profile.fields["model"] != json!(selected) {
        let mut f = profile.fields.clone();
        f["runtime"] = json!(runtime);
        f["model"] = json!(selected);
        root.update(&profile, &profile.title, f)?;
    }
    let mut result = workflows::send(c, &room, q, "chat")?;
    result["space"] = json!(c.space);
    Ok(result)
}
pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    root.role()?;
    match op {
        "assistant.open" => {
            let p = ensure(&root)?;
            summary(&root, Some(&p), None)
        }
        "assistant.history" => summary(&root, profile(&root)?.as_ref(), q["room_scope"].as_str()),
        "assistant.send" => send(c, q),
        _ => Err(invalid("unknown assistant operation")),
    }
}
pub(crate) fn replay(c: &Context<'_>, data: &Value) -> Result<()> {
    let org = c.org_id()?;
    let scope = required(data, "space")?;
    let local = Context { space: scope, ..*c };
    local.require_edit()?;
    if local.org_id()? != org {
        return Err(forbidden("assistant conversation unavailable"));
    }
    let room = local.readable("chat", required(data, "id")?)?;
    if !is_room(&room) || room.fields["creator"] != c.actor {
        return Err(forbidden("assistant conversation unavailable"));
    }
    local.readable("run", required(data, "run")?)?;
    Ok(())
}
pub(crate) fn configuration(c: &Context<'_>, id: &str, fields: &Value) -> Result<Value> {
    let profile = c.readable("assistant", id)?;
    let room = c.readable("chat", required(fields, "chat_id")?)?;
    if profile.fields["owner"] != c.actor
        || !is_room(&room)
        || room.fields["handler_id"] != id
        || fields["task_id"].is_string()
    {
        return Err(forbidden(
            "the personal assistant only runs in your own direct conversation",
        ));
    }
    let mut cfg = profile.fields;
    cfg["agent_id"] = json!(id);
    cfg["runtime"] = room.fields["runtime"].clone();
    cfg["model"] = room.fields["model"].clone();
    cfg["private_source"] = json!(true);
    Ok(cfg)
}
