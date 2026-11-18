mod shares;
mod sidebar;
mod workflows;
use crate::{Context, Record, Result, conflict, forbidden, invalid, required, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
pub(crate) use shares::read as read_share;
pub use shares::share_text;
pub(crate) use sidebar::new_message;
pub(crate) use workflows::{
    apply_conversion, check_binding, check_inherited, claim_still_visible, content_context,
    may_share_inherited,
};

pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("
        CREATE TABLE IF NOT EXISTS chat_management(space TEXT,chat TEXT,owner TEXT NOT NULL,dissolved INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(space,chat));
        CREATE TABLE IF NOT EXISTS chat_people(space TEXT,chat TEXT,actor TEXT,state TEXT NOT NULL,inviter TEXT NOT NULL,PRIMARY KEY(space,chat,actor));
        CREATE INDEX IF NOT EXISTS chat_person ON chat_people(space,actor,state);
        CREATE TABLE IF NOT EXISTS chat_agents(space TEXT,chat TEXT,agent TEXT,PRIMARY KEY(space,chat,agent));
        CREATE TABLE IF NOT EXISTS chat_user_settings(space TEXT,chat TEXT,actor TEXT,muted INTEGER NOT NULL DEFAULT 0,ignore_all INTEGER NOT NULL DEFAULT 0,clear_seq INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(space,chat,actor));
    ").map_err(storage)?;
    sidebar::init(db)?;
    workflows::init(db)?;
    shares::init(db)
}

fn owner(c: &Context<'_>, room: &Record) -> Result<(String, bool)> {
    c.db.query_row(
        "SELECT owner,dissolved FROM chat_management WHERE space=? AND chat=?",
        params![c.space, room.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(storage)
    .map(|s| {
        s.unwrap_or_else(|| {
            (
                room.fields["creator"].as_str().unwrap_or("").to_owned(),
                false,
            )
        })
    })
}

fn group(room: &Record) -> bool {
    room.fields["chat_type"] == "group"
}

fn state(c: &Context<'_>, chat: &str, actor: &str) -> Result<Option<String>> {
    c.db.query_row(
        "SELECT state FROM chat_people WHERE space=? AND chat=? AND actor=?",
        params![c.space, chat, actor],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}

fn set_member(c: &Context<'_>, room: &Record, actor: &str, level: &str) -> Result<()> {
    c.db.execute(
        "INSERT INTO chat_people(space,chat,actor,state,inviter) VALUES(?,?,?,?,?)
        ON CONFLICT(space,chat,actor) DO UPDATE SET state=excluded.state,inviter=excluded.inviter",
        params![c.space, room.id, actor, level, c.actor],
    )
    .map_err(storage)?;
    Ok(())
}

pub(crate) fn can_read(c: &Context<'_>, room: &Record) -> Result<bool> {
    if !task_visible(c, room) {
        return Ok(false);
    }
    let (leader, dissolved) = owner(c, room)?;
    if dissolved {
        return Ok(false);
    }
    if !group(room) && room.fields["visibility"] != "private" {
        return Ok(true);
    }
    Ok(c.actor == leader
        || (c.find("member", c.actor)?.is_some()
            && state(c, &room.id, c.actor)?.as_deref() == Some("member")))
}

pub(crate) fn can_edit(c: &Context<'_>, room: &Record) -> Result<()> {
    if (group(room) || room.fields["visibility"] == "private") && c.actor != owner(c, room)?.0 {
        return Err(forbidden("conversation owner required"));
    }
    Ok(())
}

pub(crate) fn require_owner(c: &Context<'_>, room: &Record) -> Result<()> {
    if owner(c, room)?.0 != c.actor {
        return Err(forbidden("conversation owner required"));
    }
    c.require_edit()
}

pub(crate) fn validate(old: Option<&Record>, room: &mut Record) -> Result<()> {
    if let Some(kind) = room.fields.get("chat_type")
        && !matches!(kind.as_str(), Some("group" | "conversation"))
    {
        return Err(invalid("chat_type must be group or conversation"));
    }
    if old.is_some_and(|existing| group(existing) != group(room)) {
        return Err(invalid("conversation type is immutable"));
    }
    if group(room) {
        if room.fields["visibility"] != "private" {
            return Err(invalid("group conversations require private visibility"));
        }
        if room.fields["agent"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        {
            return Err(invalid("use group membership to add an Agent"));
        }
    }
    Ok(())
}

pub(crate) fn before_delete(c: &Context<'_>, room: &Record) -> Result<()> {
    can_edit(c, room)?;
    for table in [
        "chat_shares",
        "chat_people",
        "chat_agents",
        "chat_user_settings",
        "chat_management",
        "chat_sidebar",
    ] {
        c.db.execute(
            &format!("DELETE FROM {table} WHERE space=? AND chat=?"),
            params![c.space, room.id],
        )
        .map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn forget_grants(c: &Context<'_>, actor: &str) -> Result<()> {
    for table in ["chat_people", "chat_user_settings", "chat_sidebar"] {
        c.db.execute(&format!("DELETE FROM {table} WHERE actor=?1 AND (space=?2 OR space IN (SELECT space FROM records WHERE kind='space' AND id=space AND json_extract(fields,'$.organization')=?2))"),params![actor,c.space]).map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn retained(c: &Context<'_>, row: &Record, room: &str) -> Result<bool> {
    let cutoff: Option<i64> =
        c.db.query_row(
            "SELECT clear_seq FROM chat_user_settings WHERE space=? AND chat=? AND actor=?",
            params![c.space, room, c.actor],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let Some(cutoff) = cutoff.filter(|s| *s > 0) else {
        return Ok(true);
    };
    let first:Option<i64>=c.db.query_row("SELECT max(seq) FROM event_log WHERE space=? AND kind=? AND id=? AND action='create' AND rev<=?",
        params![c.space,row.kind,row.id,row.rev],|r|r.get(0)).map_err(storage)?;
    Ok(first.is_some_and(|seq| seq > cutoff))
}

fn people(c: &Context<'_>, id: &str, level: &str) -> Result<Vec<String>> {
    let mut stmt =
        c.db.prepare(
            "SELECT actor FROM chat_people WHERE space=? AND chat=? AND state=? ORDER BY actor",
        )
        .map_err(storage)?;
    stmt.query_map(params![c.space, id, level], |r| r.get(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
}

fn settings(c: &Context<'_>, room: &str, actor: &str) -> Result<(bool, bool)> {
    c.db.query_row(
        "SELECT muted,ignore_all FROM chat_user_settings WHERE space=? AND chat=? AND actor=?",
        params![c.space, room, actor],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(storage)
    .map(Option::unwrap_or_default)
}

fn info(c: &Context<'_>, room: &Record) -> Result<Value> {
    let (leader, _) = owner(c, room)?;
    let is_owner = c.actor == leader;
    let (muted, ignore_all) = settings(c, &room.id, c.actor)?;
    let mut value = json!({"id":room.id,"owner":leader,"is_owner":is_owner,"group":group(room),"muted":muted,"ignore_all":ignore_all});
    if group(room) || is_owner {
        let mut members = people(c, &room.id, "member")?;
        members.retain(|s| s != &leader);
        members.insert(0, leader);
        value["members"] = json!(members);
        let mut stmt =
            c.db.prepare("SELECT agent FROM chat_agents WHERE space=? AND chat=? ORDER BY agent")
                .map_err(storage)?;
        let ids = stmt
            .query_map(params![c.space, room.id], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        value["agents"] = json!(
            ids.into_iter()
                .filter(|id| c.readable("agent", id).is_ok())
                .collect::<Vec<_>>()
        );
    }
    if is_owner {
        value["pending"] = json!(people(c, &room.id, "invited")?);
        value["shares"] = shares::list(c, room)?;
    }
    Ok(value)
}

fn invitations(c: &Context<'_>) -> Result<Value> {
    if c.find("member", c.actor)?.is_none() {
        return Ok(json!([]));
    }
    let mut stmt=c.db.prepare("SELECT chat,inviter FROM chat_people WHERE space=? AND actor=? AND state='invited' ORDER BY chat").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, c.actor], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?;
    let mut result = Vec::new();
    for entry in rows {
        let (id, inviter) = entry.map_err(storage)?;
        if let Some(room) = c.find("chat", &id)?
            && !owner(c, &room)?.1
            && task_visible(c, &room)
        {
            result.push(json!({"id":id,"title":room.title,"kind":"chat_invitation","space":c.space,"fields":{"inviter":inviter}}));
        }
    }
    Ok(json!(result))
}

fn task_visible(c: &Context<'_>, room: &Record) -> bool {
    room.fields["task"]
        .as_str()
        .or(room.fields["task_id"].as_str())
        .is_none_or(|id| c.readable("task", id).is_ok())
}

pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    if op == "chat.sidebar" {
        return sidebar::list(c);
    }
    if op == "chat.invitations" {
        return invitations(c);
    }
    if op == "chat.share.read" {
        return shares::read(c.db, required(q, "share")?, Some(c.actor));
    }
    let id = required(q, "id")?;
    if op == "chat.manage" && matches!(q["action"].as_str(), Some("accept" | "decline")) {
        let room = c
            .find("chat", id)?
            .ok_or_else(|| forbidden("invitation unavailable"))?;
        if c.find("member", c.actor)?.is_none()
            || state(c, id, c.actor)?.as_deref() != Some("invited")
            || owner(c, &room)?.1
            || !task_visible(c, &room)
        {
            return Err(forbidden("invitation unavailable"));
        }
        if q["action"] == "accept" {
            set_member(c, &room, c.actor, "member")?;
        } else {
            c.db.execute(
                "DELETE FROM chat_people WHERE space=? AND chat=? AND actor=?",
                params![c.space, id, c.actor],
            )
            .map_err(storage)?;
        }
        c.update(&room, &room.title, room.fields.clone())?;
        return Ok(json!({"id":id,"action":q["action"]}));
    }
    let room = c.readable("chat", id)?;
    if op == "chat.manage" {
        crate::domain::check_rev(q, &room)?;
    }
    match op {
        "chat.organize" => sidebar::organize(c, &room, q),
        "chat.fork" => workflows::fork(c, &room, q),
        "chat.bind_task" => workflows::bind(c, &room, q),
        "chat.convert" => workflows::convert(c, &room, q),
        "chat.send" => workflows::send(c, &room, q),
        "chat.info" => info(c, &room),
        "chat.history" => Ok(json!(workflows::ordered(c, &room)?)),
        "chat.share.create" => shares::create(c, &room, q),
        "chat.share.revoke" => shares::revoke(c, &room, required(q, "share")?),
        "chat.preferences" => {
            let flag = |name: &str| {
                q.get(name)
                    .map(|v| {
                        v.as_bool()
                            .ok_or_else(|| invalid("conversation preference must be boolean"))
                    })
                    .transpose()
            };
            let (mute, only_direct) = (flag("muted")?, flag("ignore_all")?);
            if mute.is_none() && only_direct.is_none() {
                return Err(invalid("supply muted or ignore_all"));
            }
            c.db.execute("INSERT INTO chat_user_settings(space,chat,actor,muted,ignore_all) VALUES(?1,?2,?3,coalesce(?4,0),coalesce(?5,0))
                ON CONFLICT(space,chat,actor) DO UPDATE SET muted=coalesce(?4,muted),ignore_all=coalesce(?5,ignore_all)",
                params![c.space,id,c.actor,mute,only_direct]).map_err(storage)?;
            let (muted, ignore_all) = settings(c, id, c.actor)?;
            Ok(json!({"id":id,"muted":muted,"ignore_all":ignore_all}))
        }
        "chat.manage" => manage(c, q, &room),
        _ => Err(invalid("unknown conversation operation")),
    }
}

pub(crate) fn annotate(c: &Context<'_>, result: &mut Value) -> Result<()> {
    let annotate = |row: &mut Value| -> Result<()> {
        if let Some(id) = row["id"].as_str() {
            let room = c.readable("chat", id)?;
            row["fields"]["conversation"] = info(c, &room)?;
        }
        Ok(())
    };
    if let Some(rows) = result.as_array_mut() {
        for r in rows {
            annotate(r)?;
        }
    } else {
        annotate(result)?;
    }
    Ok(())
}

fn manage(c: &Context<'_>, q: &Value, room: &Record) -> Result<Value> {
    let id = &room.id;
    let action = required(q, "action")?;
    let (leader, _) = owner(c, room)?;
    let is_owner = leader == c.actor;
    if action == "clear" {
        c.db.execute("INSERT INTO chat_user_settings(space,chat,actor,clear_seq) VALUES(?1,?2,?3,(SELECT coalesce(max(seq),0) FROM event_log))
            ON CONFLICT(space,chat,actor) DO UPDATE SET clear_seq=excluded.clear_seq",params![c.space,id,c.actor]).map_err(storage)?;
        return Ok(json!({"id":id,"action":action}));
    }
    if !group(room) && room.fields["visibility"] != "private" {
        return Err(invalid(
            "member management requires a group or private conversation",
        ));
    }
    if !is_owner && !matches!(action, "invite" | "leave") {
        return Err(forbidden("conversation owner required"));
    }
    if !matches!(action, "leave" | "invite") {
        c.require_edit()?;
    }
    if action == "leave" {
        if is_owner {
            return Err(conflict(
                "transfer ownership or dissolve the group before leaving",
            ));
        }
        c.db.execute(
            "DELETE FROM chat_people WHERE space=? AND chat=? AND actor=?",
            params![c.space, id, c.actor],
        )
        .map_err(storage)?;
    } else if action == "dissolve" {
        if !group(room) {
            return Err(invalid("only a group can be dissolved"));
        }
        crate::worker::allow_delete(c, room)?;
        c.db.execute(
            "DELETE FROM chat_shares WHERE space=? AND chat=?",
            params![c.space, id],
        )
        .map_err(storage)?;
        c.db.execute(
            "INSERT INTO chat_management(space,chat,owner,dissolved) VALUES(?,?,?,1)
            ON CONFLICT(space,chat) DO UPDATE SET dissolved=1",
            params![c.space, id, leader],
        )
        .map_err(storage)?;
    } else {
        let target = required(q, "target")?;
        if q["target_kind"] == "agent" {
            if !group(room) || !matches!(action, "add" | "remove") || !is_owner {
                return Err(forbidden("only the group owner can manage its Agents"));
            }
            if action == "add" {
                c.readable("agent", target)?;
                c.db.execute(
                    "INSERT OR IGNORE INTO chat_agents VALUES(?,?,?)",
                    params![c.space, id, target],
                )
                .map_err(storage)?;
            } else {
                c.db.execute(
                    "DELETE FROM chat_agents WHERE space=? AND chat=? AND agent=?",
                    params![c.space, id, target],
                )
                .map_err(storage)?;
            }
        } else {
            if q.get("target_kind").is_some_and(|k| k != "member") {
                return Err(invalid("target_kind must be member or agent"));
            }
            if target == leader {
                return Err(invalid("choose another member"));
            }
            if !matches!(action, "remove" | "revoke") {
                c.readable("member", target)?;
                let other = Context {
                    actor: target,
                    ..*c
                };
                if action == "transfer" {
                    other.require_edit()?;
                } else {
                    other.role()?;
                }
                if !task_visible(&other, room) {
                    return Err(forbidden("recipient cannot access the associated task"));
                }
            }
            match action {
                "add" if group(room) => set_member(c, room, target, "member")?,
                "invite" if !group(room) => {
                    if state(c, id, target)?.as_deref() != Some("member") {
                        set_member(c, room, target, "invited")?;
                    }
                }
                "remove" | "revoke" => {
                    if (action == "remove") != group(room) {
                        return Err(invalid(
                            "use remove for a group or revoke for a private conversation",
                        ));
                    }
                    c.db.execute(
                        "DELETE FROM chat_people WHERE space=? AND chat=? AND actor=?",
                        params![c.space, id, target],
                    )
                    .map_err(storage)?;
                }
                "transfer" => {
                    if group(room) && state(c, id, target)?.as_deref() != Some("member") {
                        return Err(invalid(
                            "new group owner must already be an active group member",
                        ));
                    }
                    c.db.execute("INSERT INTO chat_management(space,chat,owner) VALUES(?,?,?) ON CONFLICT(space,chat) DO UPDATE SET owner=excluded.owner",
                        params![c.space,id,target]).map_err(storage)?;
                    set_member(c, room, &leader, "member")?;
                }
                _ => return Err(invalid("unknown or unavailable conversation action")),
            }
        }
    }
    c.update(room, &room.title, room.fields.clone())?;
    Ok(json!({"id":id,"action":action}))
}

pub(crate) fn validate_mentions(c: &Context<'_>, room: &Record, f: &Value) -> Result<()> {
    if f.get("mention_all").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("mention_all must be boolean"));
    }
    if f["mention_all"] == true && !group(room) {
        return Err(invalid("mention_all requires a group"));
    }
    if let Some(ids) = f.get("mentions") {
        for id in crate::domain::strings(ids, "mentions")? {
            c.readable("member", id)?;
            let other = Context { actor: id, ..*c };
            if other.role().is_err() || !can_read(&other, room)? {
                return Err(forbidden(
                    "mention requires an active conversation participant",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn notify(c: &Context<'_>, message: &Record) -> Result<()> {
    let room = c.get("chat", required(&message.fields, "chat")?)?;
    let mentions = message.fields["mentions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let everyone = message.fields["mention_all"] == true;
    let mut recipients: std::collections::BTreeSet<_> = mentions
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if group(&room) || room.fields["visibility"] == "private" {
        recipients.extend(people(c, &room.id, "member")?);
        recipients.insert(owner(c, &room)?.0);
    }
    for recipient in recipients {
        if recipient == c.actor {
            continue;
        }
        let target = Context {
            actor: &recipient,
            ..*c
        };
        if target.role().is_err() || !target.visible(message)? {
            continue;
        }
        let (muted, ignore_all) = settings(c, &room.id, &recipient)?;
        let direct = mentions.iter().any(|v| v == &recipient);
        if !direct && (muted || (everyone && ignore_all)) {
            continue;
        }
        c.create("notification",None,&room.title,json!({"recipient":recipient,"chat":room.id,"message":message.id,"read":false,"state":"inbox","important":direct,"mention":direct,"mention_all":everyone,"creator":c.actor}))?;
    }
    Ok(())
}

pub(crate) fn extend_schema(schema: &mut Value) {
    workflows::schema(schema);
    let rules = schema["allOf"].as_array_mut().expect("schema rules");
    rules.push(json!({"if":{"properties":{"op":{"enum":["chat.info","chat.manage","chat.preferences"]}}},"then":{"required":["id"]}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"chat.manage"}}},"then":{
        "required":["action"],"properties":{"action":{"enum":["add","remove","invite","accept","decline","revoke","transfer","leave","dissolve","clear"]},"target_kind":{"enum":["member","agent"]}},
        "if":{"properties":{"action":{"enum":["add","remove","invite","revoke","transfer"]}}},"then":{"required":["target"],"properties":{"target":{"type":"string","minLength":1}}}
    }}));
    rules.push(json!({"if":{"properties":{"op":{"const":"chat.preferences"}}},"then":{"anyOf":[{"required":["muted"]},{"required":["ignore_all"]}],"properties":{"muted":{"type":"boolean"},"ignore_all":{"type":"boolean"}}}}));
    shares::extend_schema(rules);
}
