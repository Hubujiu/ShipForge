use super::*;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS direct_chats(space TEXT NOT NULL, first_actor TEXT NOT NULL, second_actor TEXT NOT NULL, chat TEXT NOT NULL, PRIMARY KEY(space,first_actor,second_actor),UNIQUE(space,chat));").map_err(storage)
}

pub(super) fn direct(c: &Context<'_>, room: &Record) -> Result<bool> {
    c.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM direct_chats WHERE space=? AND chat=?)",
        params![c.space, room.id],
        |r| r.get(0),
    )
    .map_err(storage)
}

pub(super) fn open(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let mode = required(q, "mode")?;
    let make = |title: &str, fields: Value| {
        crate::write_request(
            c,
            &json!({"op":"create","kind":"chat","title":title,"fields":fields}),
        )
    };
    let mut fields = json!({"visibility":"private","chat_type":"conversation"});
    match mode {
        "agent" | "crew" => {
            let handler = crate::agents::usable(c, mode, required(q, "target")?, None)?;
            fields["handler_kind"] = json!(mode);
            fields["handler_id"] = json!(handler.id);
            if mode == "agent" {
                fields["agent"] = json!(handler.id);
            }
            make(&handler.title, fields)
        }
        "member" => {
            let target = required(q, "target")?;
            if target == c.actor {
                return Err(invalid("choose another member"));
            }
            let other = c.readable("member", target)?;
            Context {
                actor: target,
                ..*c
            }
            .role()?;
            let (a, b) = if c.actor < target {
                (c.actor, target)
            } else {
                (target, c.actor)
            };
            let existing: Option<String> = c.db.query_row("SELECT chat FROM direct_chats WHERE space=? AND first_actor=? AND second_actor=?",params![c.space,a,b],|r|r.get(0)).optional().map_err(storage)?;
            if let Some(id) = existing {
                return c.readable("chat", &id).map(crate::as_value);
            }
            let me = c.readable("member", c.actor)?;
            let room = make(&format!("{} · {}", me.title, other.title), fields)?;
            let id = required(&room, "id")?;
            let record = c.get("chat", id)?;
            set_member(c, &record, target, "member")?;
            c.db.execute(
                "INSERT INTO direct_chats VALUES(?,?,?,?)",
                params![c.space, a, b, id],
            )
            .map_err(storage)?;
            Ok(room)
        }
        "group" => {
            fields["chat_type"] = json!("group");
            let room = make(required(q, "title")?, fields)?;
            let id = required(&room, "id")?;
            for (key, kind) in [("members", "member"), ("agents", "agent")] {
                if let Some(targets) = q.get(key) {
                    for target in crate::domain::strings(targets, key)? {
                        if kind == "member" && target == c.actor {
                            continue;
                        }
                        if kind == "agent" {
                            crate::agents::usable(c, kind, target, None)?;
                        }
                        manage(
                            c,
                            &json!({"action":"add","target_kind":kind,"target":target}),
                            &c.get("chat", id)?,
                        )?;
                    }
                }
            }
            Ok(crate::as_value(c.get("chat", id)?))
        }
        _ => Err(invalid("mode must be member, agent, crew or group")),
    }
}
