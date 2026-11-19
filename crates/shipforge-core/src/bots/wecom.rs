use super::*;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS wecom_reservations(id TEXT PRIMARY KEY, space TEXT NOT NULL, agent TEXT NOT NULL, owner TEXT NOT NULL, account TEXT NOT NULL UNIQUE, device TEXT NOT NULL, expires INTEGER NOT NULL, UNIQUE(space,agent));").map_err(storage)
}
pub(super) fn reserve(c: &Context<'_>, q: &Value, release: bool) -> Result<Value> {
    c.require_edit()?;
    let id = short(q, "id")?;
    let key = device(q)?;
    if release {
        c.db.execute(
            "DELETE FROM wecom_reservations WHERE id=? AND space=? AND owner=? AND device=?",
            params![id, c.space, c.actor, key],
        )
        .map_err(storage)?;
        return Ok(json!({"released":true}));
    }
    identifier(id)?;
    let account = short(q, "account")?;
    let agent = short(q, "agent")?;
    crate::agents::usable(c, "agent", agent, None)?;
    c.db.execute(
        "DELETE FROM wecom_reservations WHERE expires<=?",
        [crate::now()],
    )
    .map_err(storage)?;
    let busy=c.db.query_row("SELECT EXISTS(SELECT 1 FROM external_bots WHERE provider='wecom' AND state='connected' AND (account=?1 OR space=?2 AND agent=?3)) OR EXISTS(SELECT 1 FROM wecom_reservations WHERE account=?1 OR space=?2 AND agent=?3)",params![account,c.space,agent],|r|r.get::<_,bool>(0)).map_err(storage)?;
    if busy {
        return Err(conflict(
            "this WeCom bot or Agent is already connected or being verified",
        ));
    }
    c.db.execute(
        "INSERT INTO wecom_reservations VALUES(?,?,?,?,?,?,?)",
        params![
            id,
            c.space,
            agent,
            c.actor,
            account,
            key,
            crate::now() + 90000
        ],
    )
    .map_err(storage)?;
    Ok(json!({"reserved":true}))
}
pub(super) fn consume(c: &Context<'_>, q: &Value) -> Result<()> {
    let changed=c.db.execute("DELETE FROM wecom_reservations WHERE id=? AND space=? AND owner=? AND agent=? AND account=? AND device=? AND expires>?",params![short(q,"id")?,c.space,c.actor,short(q,"agent")?,short(q,"account")?,device(q)?,crate::now()]).map_err(storage)?;
    if changed != 1 {
        return Err(conflict(
            "WeCom credential verification reservation expired",
        ));
    }
    Ok(())
}
pub(super) fn event(c: &Context<'_>, b: &Binding, q: &Value) -> Result<Value> {
    if b.provider != "wecom" || q["bot_id"] != b.account {
        return Err(forbidden("event belongs to another WeCom bot"));
    }
    let kind = short(q, "chattype")?;
    let user = short(q, "userid")?;
    let source = short(q, "msgid")?;
    let (scene, peer) = match kind {
        "single" => ("direct", user),
        "group" => ("group", short(q, "chatid")?),
        _ => return Ok(json!({"ignored":"unsupported_scene"})),
    };
    if !b.permits(scene) {
        return Ok(json!({"ignored":"disabled"}));
    }
    // API-mode group callbacks are addressed to this bot by WeCom. Other group activity has no message callback.
    turns::receive(
        c,
        b,
        &json!({"scene":scene,"peer":peer,"sender":user,"remote":source,"body":q["body"],"files":q["files"]}),
    )
}
pub(super) fn schema(s: &mut Value) {
    for (op, keys) in [
        (
            "bot.wecom.reserve",
            vec!["id", "agent", "account", "device_key"],
        ),
        ("bot.wecom.release", vec!["id", "device_key"]),
        (
            "bot.wecom.event",
            vec![
                "id",
                "bot_id",
                "device_key",
                "chattype",
                "userid",
                "msgid",
                "body",
            ],
        ),
    ] {
        s["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":keys,"properties":{"chattype":{"enum":["single","group"]},"device_key":{"type":"string","minLength":32,"maxLength":256}}}}));
    }
    s["allOf"].as_array_mut().unwrap().push(json!({"if":{"required":["chattype"],"properties":{"op":{"const":"bot.wecom.event"},"chattype":{"const":"group"}}},"then":{"required":["chatid"],"properties":{"chatid":{"type":"string","minLength":1,"maxLength":512}}}}));
}
