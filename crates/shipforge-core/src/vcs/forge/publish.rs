use super::*;
pub(super) fn backlink(c: &Context<'_>, b: &Value, n: i64, task: &str, kind: &str) -> Result<()> {
    let t = c.readable("task", task)?;
    if t.fields["visibility"] == "private" {
        return Ok(());
    }
    let link = crate::triage::execute(c, "task.share", &json!({"id":task}))?;
    let body = format!(
        "{} · {}",
        required(&link, "number")?,
        required(&link, "link")?
    );
    let key = format!("backlink:{kind}:{n}:{task}");
    c.db.execute("INSERT OR IGNORE INTO forge_outbox(id,pair,number,task,author,body,kind,phase,expires,request) VALUES(?,?,?,?,'',?,?,'pending',0,?)",params![Uuid::new_v4().to_string(),b["id"].as_str(),n,task,body,if kind=="review"{"review_link"}else{"backlink"},key]).map_err(storage)?;
    Ok(())
}
pub(super) fn command(c: &Context<'_>, p: &str, q: &Value) -> Result<Value> {
    let b = pair(c, p, required(q, "pair")?)?;
    let id = required(&b, "id")?;
    if q["action"] == "can_publish" {
        let row=c.db.query_row("SELECT task,author,kind FROM forge_outbox WHERE id=? AND pair=? AND lease=? AND phase='sending' AND expires>?",params![required(q,"delivery")?,id,required(q,"lease")?,crate::now()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional().map_err(storage)?.ok_or_else(||conflict("source delivery changed"))?;
        let t = c.readable("task", &row.0)?;
        if t.fields["visibility"] == "private" {
            return Err(forbidden("private tasks cannot publish"));
        }
        if row.2 == "reply" {
            c.require_edit()?;
            if row.1 != c.actor || b["direction"] != "both" {
                return Err(forbidden("source reply is disabled"));
            }
        } else {
            org_admin(c)?;
            if b["backlinks"] != true {
                return Err(forbidden("backlinks disabled"));
            }
        }
        let a = account(c, p, "")?.ok_or_else(|| forbidden("inactive source authorization"))?;
        if a["credential"] != q["credential"] {
            return Err(conflict("source credential changed"));
        }
        return Ok(Value::Null);
    }
    if q["action"] == "settle" {
        let state = required(q, "state")?;
        if !matches!(state, "sent" | "uncertain" | "failed") {
            return Err(invalid("invalid send state"));
        }
        let n=c.db.execute("UPDATE forge_outbox SET phase=?,remote=?,lease=NULL,expires=0 WHERE id=? AND pair=? AND lease=? AND phase='sending'",params![state,q["remote_id"].as_str(),required(q,"delivery")?,id,required(q,"lease")?]).map_err(storage)?;
        if n != 1 {
            return Err(conflict("repository reply lease changed"));
        }
        let mut receipt =
            json!({"state":state,"remote_id":q["remote_id"],"delivery":q["delivery"]});
        if state == "sent" {
            c.db.execute(
                "INSERT INTO forge_sent VALUES(?,?,?)",
                params![
                    required(q, "delivery")?,
                    required(q, "identity")?,
                    q["fallback"].as_bool().unwrap_or(false)
                ],
            )
            .map_err(storage)?;
            receipt["identity"] = q["identity"].clone();
            receipt["fallback"] = q["fallback"].clone();
        }
        return Ok(receipt);
    }
    let delivery = if q["action"] == "reserve" {
        c.require_edit()?;
        let map = events::mapping(c, p, q)?;
        if b["direction"] != "both" {
            return Err(forbidden("this repository receives changes only"));
        }
        let body = required(q, "body")?.trim();
        if body.is_empty() || body.len() > 60000 {
            return Err(invalid("source reply must contain 1 to 60000 bytes"));
        }
        let key = required(q, "request_id")?;
        if key.len() > 160 {
            return Err(invalid("request ID too long"));
        }
        c.db.execute("INSERT INTO forge_outbox(id,pair,number,task,author,body,kind,phase,request,expires) VALUES(?,?,?,?,?,?,'reply','pending',?,0) ON CONFLICT(pair,author,request) DO NOTHING",params![Uuid::new_v4().to_string(),id,map["number"].as_i64(),required(q,"task_id")?,c.actor,body,key]).map_err(storage)?;
        let (ticket, prior, task): (String, String, String) =
            c.db.query_row(
                "SELECT id,body,task FROM forge_outbox WHERE pair=? AND author=? AND request=?",
                params![id, c.actor, key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(storage)?;
        if prior != body || q["task_id"] != task {
            return Err(conflict("request ID already used for another source reply"));
        }
        ticket
    } else {
        org_admin(c)?;
        let pending=c.db.query_row("SELECT id FROM forge_outbox WHERE pair=? AND kind IN('backlink','review_link') AND (phase='pending' OR phase IN('uncertain','sending') AND expires<=?) ORDER BY rowid LIMIT 1",params![id,crate::now()],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
        let Some(ticket) = pending else {
            return Ok(Value::Null);
        };
        ticket
    };
    let mut row=c.db.query_row("SELECT phase,expires,number,task,author,body,kind,remote FROM forge_outbox WHERE pair=? AND id=?",params![id,delivery],|r|Ok(json!({"state":r.get::<_,String>(0)?,"expires":r.get::<_,i64>(1)?,"number":r.get::<_,i64>(2)?,"task":r.get::<_,String>(3)?,"author":r.get::<_,String>(4)?,"body":r.get::<_,String>(5)?,"kind":r.get::<_,String>(6)?,"remote_id":r.get::<_,Option<String>>(7)?,"delivery":delivery}))).map_err(storage)?;
    let task = c.readable("task", required(&row, "task")?)?;
    if task.fields["visibility"] == "private" {
        return Err(forbidden(
            "private task cannot be published to a repository",
        ));
    }
    if row["state"] == "sent" {
        let who: Option<(String, bool)> =
            c.db.query_row(
                "SELECT identity,fallback FROM forge_sent WHERE delivery=?",
                [&delivery],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        let mut receipt = json!({"state":"sent","remote_id":row["remote_id"],"delivery":delivery});
        if let Some((id, fallback)) = who {
            receipt["identity"] = json!(id);
            receipt["fallback"] = json!(fallback);
        }
        return Ok(receipt);
    }
    if row["state"] == "cancelled" {
        return Ok(json!({"state":"cancelled","delivery":delivery}));
    }
    if row["state"] == "sending" && row["expires"].as_i64().unwrap_or(0) > crate::now() {
        return Err(conflict("repository delivery in progress"));
    }
    let lease = Uuid::new_v4().to_string();
    c.db.execute(
        "UPDATE forge_outbox SET phase='sending',lease=?,expires=? WHERE id=?",
        params![lease, crate::now() + 120000, delivery],
    )
    .map_err(storage)?;
    row["lease"] = json!(lease);
    row["recover"] = json!(matches!(
        row["state"].as_str(),
        Some("sending" | "uncertain")
    ));
    row["pair"] = b;
    Ok(row)
}
