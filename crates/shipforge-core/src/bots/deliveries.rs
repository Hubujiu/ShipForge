use super::*;

pub(super) fn cancel(c: &Context<'_>, b: &Binding, scene: Option<&str>) -> Result<()> {
    c.db.execute(
        "UPDATE external_bot_turns SET prepared=1 WHERE bot=?1 AND (?2 IS NULL OR scene=?2)",
        params![b.id, scene],
    )
    .map_err(storage)?;
    c.db.execute("UPDATE external_bot_deliveries SET state='cancelled',lease=NULL WHERE state NOT IN('sent','cancelled') AND turn IN(SELECT id FROM external_bot_turns WHERE bot=?1 AND (?2 IS NULL OR scene=?2))",params![b.id,scene]).map_err(storage)?;
    Ok(())
}
pub(super) fn summary(c: &Context<'_>, b: &Binding) -> Result<Value> {
    let mut st=c.db.prepare("SELECT d.id,state FROM external_bot_deliveries d JOIN external_bot_turns t ON t.id=d.turn WHERE bot=? AND state IN('uncertain','failed') ORDER BY d.rowid DESC LIMIT 30").map_err(storage)?;
    let list = st
        .query_map([&b.id], |r| {
            Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?}))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    Ok(json!(list))
}
pub(super) fn insert(c: &Context<'_>, turn: &str, part: i64, payload: &Value) -> Result<()> {
    c.db.execute("INSERT INTO external_bot_deliveries(id,turn,part,payload) VALUES(?,?,?,?) ON CONFLICT(turn,part) DO NOTHING",params![Uuid::new_v4().to_string(),turn,part,payload.to_string()]).map_err(storage)?;
    Ok(())
}
fn prepare(c: &Context<'_>, b: &Binding) -> Result<()> {
    let mut st =
        c.db.prepare(
            "SELECT id,run,chat FROM external_bot_turns WHERE bot=? AND prepared=0 ORDER BY rowid",
        )
        .map_err(storage)?;
    let turns = st
        .query_map([&b.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (id, run_id, chat) in turns {
        let run = c.find("run", &run_id)?;
        if let Some(run) = run
            && c.visible(&run)?
            && room_visible(c, &chat)?
        {
            let status = run.fields["status"].as_str().unwrap_or("");
            if !matches!(status, "done" | "failed" | "cancelled") {
                continue;
            }
            let result = match status {
                "done" => run.fields["result"].as_str().unwrap_or("").trim(),
                "failed" => "Agent 执行失败",
                "cancelled" => "Agent 执行已取消",
                _ => unreachable!(),
            };
            let chars = result.chars().collect::<Vec<_>>();
            let mut part = 1;
            for chunk in chars.chunks(1200) {
                insert(
                    c,
                    &id,
                    part,
                    &json!({"text":chunk.iter().collect::<String>()}),
                )?;
                part += 1;
            }
            if status == "done" && b.provider == "wechat" {
                for file in c.records("artifact")? {
                    if file.fields["run"] != run_id || !c.visible(&file)? {
                        continue;
                    }
                    for attachment in file.fields["attachments"].as_array().into_iter().flatten() {
                        insert(
                            c,
                            &id,
                            part,
                            &json!({"artifact":file.id,"attachment":attachment["id"],"filename":attachment["filename"]}),
                        )?;
                        part += 1;
                    }
                }
            }
            if part == 1 {
                insert(c, &id, 1, &json!({"text":"Agent 已完成本次请求"}))?;
            }
        }
        c.db.execute("UPDATE external_bot_turns SET prepared=1 WHERE id=?", [id])
            .map_err(storage)?;
    }
    Ok(())
}
pub(super) fn claim(c: &Context<'_>, b: &Binding) -> Result<Value> {
    prepare(c, b)?;
    let clock = chrono::Utc::now().timestamp();
    c.db.execute("UPDATE external_bot_deliveries SET state='uncertain' WHERE state='sending' AND expires<=? AND turn IN (SELECT id FROM external_bot_turns WHERE bot=?)",params![clock,b.id]).map_err(storage)?;
    let item=c.db.query_row("SELECT d.id,d.part,d.payload,t.scene,t.peer,t.remote,t.chat,t.run FROM external_bot_deliveries d JOIN external_bot_turns t ON t.id=d.turn WHERE t.bot=? AND d.state='pending' AND d.available<=? AND NOT EXISTS (SELECT 1 FROM external_bot_deliveries p WHERE p.turn=t.id AND p.part<d.part AND p.state NOT IN ('sent','cancelled')) ORDER BY d.rowid LIMIT 1",params![b.id,clock],|r|Ok(json!({"delivery":r.get::<_,String>(0)?,"part":r.get::<_,i64>(1)?,"payload":r.get::<_,String>(2)?,"scene":r.get::<_,String>(3)?,"peer":r.get::<_,String>(4)?,"remote":r.get::<_,String>(5)?,"chat":r.get::<_,String>(6)?,"run":r.get::<_,Option<String>>(7)?}))).optional().map_err(storage)?;
    let Some(mut out) = item else {
        return Ok(Value::Null);
    };
    c.readable("chat", required(&out, "chat")?)?;
    let lease = Uuid::new_v4().to_string();
    c.db.execute(
        "UPDATE external_bot_deliveries SET state='sending',lease=?,expires=? WHERE id=?",
        params![lease, clock + 180, required(&out, "delivery")?],
    )
    .map_err(storage)?;
    out["payload"] = serde_json::from_str(required(&out, "payload")?).map_err(storage)?;
    out["lease"] = json!(lease);
    Ok(out)
}
pub(super) fn ack(c: &Context<'_>, b: &Binding, q: &Value) -> Result<Value> {
    let id = short(q, "delivery")?;
    let lease = short(q, "lease")?;
    let outcome = required(q, "outcome")?;
    let state = match outcome {
        "sent" | "uncertain" | "failed" => outcome,
        "retry" => "pending",
        _ => return Err(invalid("unknown delivery outcome")),
    };
    let saved:Option<(String,Option<String>)>=c.db.query_row("SELECT state,lease FROM external_bot_deliveries WHERE id=? AND turn IN(SELECT id FROM external_bot_turns WHERE bot=?)",params![id,b.id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
    let Some((previous, token)) = saved else {
        return Err("not_found: bot delivery".into());
    };
    if token.as_deref() != Some(lease) {
        return Err(conflict("stale bot delivery lease"));
    }
    if previous != state {
        if !matches!(previous.as_str(), "sending" | "uncertain") {
            return Err(conflict("bot delivery already settled"));
        }
        let delay = q["retry_after"].as_i64().unwrap_or(5).clamp(1, 3600);
        c.db.execute(
            "UPDATE external_bot_deliveries SET state=?,available=?,receipt=? WHERE id=?",
            params![
                state,
                chrono::Utc::now().timestamp() + delay,
                q["receipt"].as_str().filter(|v| v.len() <= 512),
                id
            ],
        )
        .map_err(storage)?;
    }
    Ok(json!({"id":id,"state":state}))
}
pub(super) fn retry(c: &Context<'_>, b: &Binding, q: &Value) -> Result<Value> {
    let id = short(q, "delivery")?;
    let next = match required(q, "decision")? {
        "resend" => "pending",
        "confirm" => "sent",
        "discard" => "cancelled",
        _ => return Err(invalid("unknown delivery decision")),
    };
    let changed=c.db.execute("UPDATE external_bot_deliveries SET state=?,lease=NULL,available=0 WHERE id=? AND state IN('uncertain','failed') AND turn IN(SELECT id FROM external_bot_turns WHERE bot=?)",params![next,id,b.id]).map_err(storage)?;
    if changed != 1 {
        return Err(conflict("delivery does not require a decision"));
    }
    Ok(json!({"id":id,"state":next}))
}
