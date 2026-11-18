use super::origin::slack_ts;
use crate::{Context, Record, Result, conflict, invalid, now, required, storage};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

fn enabled(connector: &Record) -> bool {
    let f = &connector.fields;
    f["provider"] == "slack"
        && f["enabled"] == true
        && f["outbound"] == true
        && ["workspace_id", "channel_id", "app_id", "bot_id"]
            .iter()
            .all(|key| f[key].as_str().is_some_and(|s| !s.is_empty()))
}
pub(super) fn scoped(connector: &Record, route: &Value) -> bool {
    enabled(connector)
        && route["provider"] == "slack"
        && route["workspace"] == connector.fields["workspace_id"]
        && route["channel"] == connector.fields["channel_id"]
        && route["thread"].as_str().is_some_and(slack_ts)
}

pub(crate) fn changed(c: &Context<'_>, before: Option<&Record>, record: &Record) -> Result<()> {
    if record.kind == "connector" {
        if before.is_some_and(|old| {
            ["workspace_id", "channel_id", "app_id", "bot_id", "provider"]
                .iter()
                .any(|key| old.fields[key] != record.fields[key])
        }) || !enabled(record)
        {
            cancel(c, "connector", &record.id)?;
        }
        return Ok(());
    }
    if !matches!(record.kind.as_str(), "task" | "comment") {
        return Ok(());
    }
    if record.fields["visibility"] == "private" {
        return cancel(c, &record.kind, &record.id);
    }
    let (task, body) = match record.kind.as_str() {
        "task"
            if before.is_some_and(|old| {
                old.fields["status"] != record.fields["status"] || old.title != record.title
            }) =>
        {
            let number = record.fields["number"].as_str().unwrap_or(&record.id);
            let status = record.fields["status"].as_str().unwrap_or_default();
            (
                record.clone(),
                format!("{number} · {} [{}]", record.title, status),
            )
        }
        "comment" if before.is_none() => {
            let task = c.get("task", required(&record.fields, "task")?)?;
            if task.fields["visibility"] == "private" {
                return Ok(());
            }
            let number = task.fields["number"].as_str().unwrap_or(&task.id);
            let actor = record.fields["creator"].as_str().unwrap_or(c.actor);
            let text = required(&record.fields, "body")?;
            let body = format!("{number} · {actor}\n{text}");
            (task, body)
        }
        _ => return Ok(()),
    };
    // Slack recommends 4,000 characters and truncates very long messages. Keep a visible boundary.
    let text = if body.chars().count() > 3999 {
        body.chars().take(3998).chain(['…']).collect()
    } else {
        body
    };
    let mut links =
        c.db.prepare(
            "SELECT connector,route FROM sf_im_threads t WHERE space=?1 AND task=?2 AND active=1
        AND (?3 IS NULL OR EXISTS(SELECT 1 FROM sf_im_replies m WHERE m.space=t.space AND m.comment=?3 AND m.connector=t.connector AND m.route=t.route))",
        )
        .map_err(storage)?;
    let rows = links
        .query_map(
            params![
                c.space,
                task.id,
                if record.kind == "comment" {
                    Some(&record.id)
                } else {
                    None
                }
            ],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(storage)?;
    for row in rows {
        let (id, route) = row.map_err(storage)?;
        let Some(connector) = c.find("connector", &id)? else {
            continue;
        };
        if !scoped(&connector, &serde_json::from_str(&route).map_err(storage)?) {
            continue;
        }
        c.db.execute("INSERT OR IGNORE INTO sf_im_outbox(id,space,connector,route,task,kind,source,rev,body) VALUES(?,?,?,?,?,?,?,?,?)",
            params![Uuid::new_v4().to_string(),c.space,id,route,task.id,record.kind,record.id,record.rev,text]).map_err(storage)?;
    }
    Ok(())
}

fn cancel(c: &Context<'_>, kind: &str, id: &str) -> Result<()> {
    let condition = match kind {
        "task" => "task=?2",
        "comment" => "kind='comment' AND source=?2",
        "connector" => "connector=?2",
        _ => return Ok(()),
    };
    let sql = format!(
        "UPDATE sf_im_outbox SET state='cancelled',body='',lease=NULL,error=NULL WHERE space=?1 AND {condition} AND state!='sent'"
    );
    c.db.execute(&sql, params![c.space, id]).map_err(storage)?;
    Ok(())
}
pub(super) fn cancel_route(c: &Context<'_>, id: &str, route: &str) -> Result<()> {
    c.db.execute("UPDATE sf_im_outbox SET state='cancelled',body='',lease=NULL,error=NULL WHERE space=? AND connector=? AND route=? AND state!='sent'",params![c.space,id,route]).map_err(storage)?;
    Ok(())
}
pub(crate) fn removed(c: &Context<'_>, row: &Record) -> Result<()> {
    cancel(c, &row.kind, &row.id)?;
    if row.kind == "task" {
        c.db.execute(
            "UPDATE sf_im_threads SET active=0 WHERE space=? AND task=?",
            params![c.space, row.id],
        )
        .map_err(storage)?;
    }
    Ok(())
}

fn expiry(c: &Context<'_>, id: &str) -> Result<()> {
    c.db.execute("UPDATE sf_im_outbox SET state='uncertain',error='lease expired before acknowledgement' WHERE space=? AND connector=? AND state='leased' AND expires<=?",
        params![c.space,id,now()]).map_err(storage)?;
    Ok(())
}

fn list(c: &Context<'_>, connector: &Record, q: &Value) -> Result<Value> {
    let limit = q["limit"].as_i64().unwrap_or(100);
    if !(1..=5000).contains(&limit) {
        return Err(invalid("limit must be between 1 and 5000"));
    }
    let wanted = if q.get("state").is_some() {
        Some(required(q, "state")?)
    } else {
        None
    };
    if wanted.is_some_and(|s| {
        !matches!(
            s,
            "pending" | "leased" | "uncertain" | "failed" | "sent" | "cancelled"
        )
    }) {
        return Err(invalid("invalid delivery state"));
    }
    let available = q
        .get("available_only")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| invalid("available_only must be boolean"))
        })
        .transpose()?
        .unwrap_or(false);
    if available && limited(c, connector)? {
        return Ok(json!([]));
    }
    let mut st=c.db.prepare("SELECT id,task,kind,source,rev,CASE WHEN state='leased' AND expires<=?1 THEN 'uncertain' ELSE state END AS effective,attempts,remote,error,available FROM sf_im_outbox
        WHERE space=?2 AND connector=?3 AND (?4 IS NULL OR effective=?4) AND (?5=0 OR available<=?1) ORDER BY seq DESC LIMIT ?6").map_err(storage)?;
    let rows=st.query_map(params![now(),c.space,connector.id,wanted,available,limit],|r| Ok(json!({"delivery_id":r.get::<_,String>(0)?,"task":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"source":r.get::<_,String>(3)?,"rev":r.get::<_,i64>(4)?,"state":r.get::<_,String>(5)?,"attempts":r.get::<_,i64>(6)?,"remote_id":r.get::<_,Option<String>>(7)?,"error":r.get::<_,Option<String>>(8)?,"available_at":r.get::<_,i64>(9)?}))).map_err(storage)?;
    let mut result = Vec::new();
    for row in rows {
        let row = row.map_err(storage)?;
        if let Some(task) = c.find("task", required(&row, "task")?)?
            && task.fields["visibility"] != "private"
            && c.visible(&task)?
        {
            if row["kind"] == "comment"
                && c.find("comment", required(&row, "source")?)?
                    .is_none_or(|r| r.fields["visibility"] == "private")
            {
                continue;
            }
            result.push(row)
        }
    }
    Ok(json!(result))
}

fn limited(c: &Context<'_>, connector: &Record) -> Result<bool> {
    c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_im_rate_limits WHERE workspace=? AND app=? AND available>?)",params![connector.fields["workspace_id"].as_str(),connector.fields["app_id"].as_str(),now()],|r|r.get(0)).map_err(storage)
}

fn claim(c: &Context<'_>, connector: &Record) -> Result<Value> {
    let id = &connector.id;
    if !enabled(connector) {
        return Err(invalid(
            "enable scoped Slack outbound on this connector first",
        ));
    }
    if limited(c, connector)? {
        return Ok(Value::Null);
    }
    loop {
        let candidate:Option<(String,String,String,String,String,String)>=c.db.query_row(
            "SELECT id,route,task,kind,source,body FROM sf_im_outbox o WHERE space=? AND connector=? AND state='pending' AND available<=?
             AND NOT EXISTS(SELECT 1 FROM sf_im_outbox p WHERE p.space=o.space AND p.connector=o.connector AND p.route=o.route AND p.seq<o.seq AND p.state NOT IN('sent','cancelled')) ORDER BY seq LIMIT 1",
            params![c.space,id,now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(storage)?;
        let Some((delivery, route, task, kind, source, text)) = candidate else {
            return Ok(Value::Null);
        };
        let address: Value = serde_json::from_str(&route).map_err(storage)?;
        let active = super::thread(c, id, &route)?.is_some_and(|(target, on)| on && target == task);
        let visible = if let Some(record) = c.find(&kind, &source)? {
            record.fields["visibility"] != "private"
                && c.visible(&record)?
                && (kind != "comment" || record.fields["task"] == task)
        } else {
            false
        };
        let shared = match c.find("task", &task)? {
            Some(row) => row.fields["visibility"] != "private" && c.visible(&row)?,
            None => false,
        };
        if !active || !scoped(connector, &address) || !visible || !shared {
            c.db.execute(
                "UPDATE sf_im_outbox SET state='cancelled',body='',error=NULL WHERE id=?",
                [delivery],
            )
            .map_err(storage)?;
            continue;
        }
        let lease = Uuid::new_v4().to_string();
        c.db.execute("UPDATE sf_im_outbox SET state='leased',lease=?,expires=?,attempts=attempts+1,error=NULL WHERE id=?",
            params![lease,now()+60_000,delivery]).map_err(storage)?;
        return Ok(
            json!({"delivery_id":delivery,"lease":lease,"task":task,"thread":address,"text":text}),
        );
    }
}

pub(super) fn execute(c: &Context<'_>, connector: &Record, q: &Value) -> Result<Value> {
    let op = required(q, "op")?;
    if op == "im.outbox" {
        return list(c, connector, q);
    }
    expiry(c, &connector.id)?;
    if op == "im.claim" {
        return claim(c, connector);
    }
    let delivery = required(q, "delivery_id")?;
    let (state, lease, remote): (String, Option<String>, Option<String>) =
        c.db.query_row(
            "SELECT state,lease,remote FROM sf_im_outbox WHERE space=? AND connector=? AND id=?",
            params![c.space, connector.id, delivery],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| invalid("unknown connector delivery"))?;
    let outcome = if op == "im.ack" {
        let credential = required(q, "lease")?;
        if lease.as_deref() != Some(credential) {
            return Err(conflict("lease does not match this delivery"));
        }
        let outcome = required(q, "outcome")?;
        if state == "sent" && matches!(outcome, "failed" | "uncertain" | "retry") {
            return Ok(json!({"state":"sent"}));
        }
        if (state == "sent" && outcome == "sent" && remote.as_deref() == q["remote_id"].as_str())
            || (state == outcome && matches!(outcome, "failed" | "uncertain"))
            || (state == "pending" && outcome == "retry")
        {
            return Ok(json!({"state":state}));
        }
        if state != "leased" && !(state == "uncertain" && outcome == "sent") {
            return Err(conflict("delivery lease is no longer active"));
        }
        outcome
    } else if op == "im.retry" {
        if !matches!(state.as_str(), "failed" | "uncertain") {
            return Err(conflict(
                "only failed or uncertain deliveries can be reconciled",
            ));
        }
        match required(q, "decision")? {
            "confirm" => "sent",
            "resend" => "retry",
            _ => return Err(invalid("decision must be confirm or resend")),
        }
    } else {
        return Err(invalid("unknown IM operation"));
    };
    let (state, remote_id, delay) = match outcome {
        "sent" => {
            let ts = required(q, "remote_id")?;
            if !slack_ts(ts) {
                return Err(invalid("invalid Slack remote message timestamp"));
            }
            ("sent", Some(ts), 0)
        }
        "retry" => {
            let delay = if op == "im.retry" {
                0
            } else {
                q["retry_after"]
                    .as_i64()
                    .filter(|d| (1..=86400).contains(d))
                    .ok_or_else(|| invalid("retry_after must be 1–86400 seconds"))?
            };
            ("pending", None, delay)
        }
        "failed" | "uncertain" => (outcome, None, 0),
        _ => return Err(invalid("unknown delivery outcome")),
    };
    let error = if state == "sent" {
        None
    } else {
        q["error"]
            .as_str()
            .map(|s| s.chars().take(240).collect::<String>())
    };
    if delay > 0 {
        c.db.execute("INSERT INTO sf_im_rate_limits(workspace,app,available) VALUES(?,?,?) ON CONFLICT(workspace,app) DO UPDATE SET available=max(available,excluded.available)",
        params![required(&connector.fields,"workspace_id")?,required(&connector.fields,"app_id")?,now()+delay*1000]).map_err(storage)?;
    }
    c.db.execute("UPDATE sf_im_outbox SET state=?,remote=?,available=?,error=?,lease=CASE WHEN ? THEN NULL ELSE lease END WHERE id=?",
        params![state,remote_id,now()+delay*1000,error,op=="im.retry",delivery]).map_err(storage)?;
    Ok(json!({"state":state}))
}

// Executed only for a verified bot message of the locally configured Slack app.
pub(crate) fn echo(c: &Context<'_>, connector: &Record, p: &Value) -> Result<Value> {
    let e = &p["event"];
    let meta = &e["metadata"];
    let (channel, ts, thread) = match e["type"].as_str() {
        Some("message_metadata_posted") => (
            &e["channel_id"],
            &e["message_ts"],
            &meta["event_payload"]["thread_ts"],
        ),
        Some("message") => (&e["channel"], &e["ts"], &e["thread_ts"]),
        _ => return Ok(json!({"action":"ignored"})),
    };
    if !enabled(connector)
        || p["team_id"] != connector.fields["workspace_id"]
        || channel != &connector.fields["channel_id"]
        || e["bot_id"] != connector.fields["bot_id"]
        || p["api_app_id"] != connector.fields["app_id"]
        || e.get("app_id").or(e["bot_profile"].get("app_id")) != connector.fields.get("app_id")
        || meta["event_type"] != "shipforge_delivery"
    {
        return Ok(json!({"action":"ignored"}));
    }
    let Some(delivery) = meta["event_payload"]["delivery_id"].as_str() else {
        return Ok(json!({"action":"ignored"}));
    };
    let ts = ts
        .as_str()
        .filter(|ts| slack_ts(ts))
        .ok_or_else(|| invalid("invalid Slack message timestamp"))?;
    let route =
        json!({"provider":"slack","workspace":p["team_id"],"channel":channel,"thread":thread})
            .to_string();
    let n=c.db.execute("UPDATE sf_im_outbox SET state='sent',remote=?,error=NULL WHERE space=? AND connector=? AND id=? AND route=? AND state IN ('leased','uncertain')",
        params![ts,c.space,connector.id,delivery,route]).map_err(storage)?;
    Ok(json!({"action":if n==1{"confirmed"}else{"ignored"}}))
}
