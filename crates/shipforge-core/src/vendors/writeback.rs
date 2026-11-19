use super::*;
use crate::{forbidden, now};
use rusqlite::OptionalExtension;
use uuid::Uuid;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_vendor_push(id TEXT PRIMARY KEY,space TEXT NOT NULL,connector TEXT NOT NULL,task TEXT NOT NULL,request_key TEXT NOT NULL,plan TEXT NOT NULL,state TEXT NOT NULL,lease TEXT,expires INTEGER,attempts INTEGER NOT NULL,remote TEXT,at INTEGER NOT NULL,UNIQUE(space,connector,task,request_key))").map_err(storage)
}

struct Previous {
    id: String,
    state: String,
    expires: Option<i64>,
    remote: Option<String>,
    plan: String,
}

fn prepare(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    if r.fields["vendor_write"] != true {
        return Err(forbidden(
            "enable writeback on this project connector first",
        ));
    }
    let task = required(q, "task_id")?;
    let link:Option<(String,i64,String)>=c.db.query_row("SELECT external,birth,base FROM sf_import_item WHERE space=? AND connector=? AND task=?",params![c.space,r.id,task],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?))).optional().map_err(storage)?;
    let (external, birth, raw) = link.ok_or("not_found: this connector does not own the task")?;
    let row = batches::owned(c, r, task, birth, &external)?
        .ok_or("not_found: original linked task is unavailable")?;
    c.readable("task", task)?;
    if row.fields["visibility"] == "private" {
        return Err(forbidden(
            "private tasks cannot be published by this connector",
        ));
    }
    let base: Value = serde_json::from_str(&raw).map_err(storage)?;
    let (kind, key) = external
        .split_once(':')
        .ok_or_else(|| invalid("invalid mapped work item"))?;
    let provider = required(&r.fields, "provider")?;
    let mode = q["intent"].as_str().unwrap_or("status");
    let mut plan = json!({"provider":provider,"entity":kind,"key":key,"project":r.fields["remote_scope"],"scope_kind":r.fields["scope_kind"],"intent":mode,"task":task,"source_state":base["source_status_id"]});
    let generation: i64 =
        c.db.query_row(
            "SELECT generation FROM sf_import_generation WHERE space=? AND connector=?",
            params![c.space, r.id],
            |v| v.get(0),
        )
        .map_err(storage)?;
    plan["generation"] = json!(generation);
    let request_key = match mode {
        "status" => {
            let prior:Option<String>=c.db.query_row("SELECT plan FROM sf_vendor_push WHERE space=? AND connector=? AND task=? AND state='sent' AND request_key LIKE 'status:%' ORDER BY rowid DESC LIMIT 1",params![c.space,r.id,task],|v|v.get(0)).optional().map_err(storage)?;
            if let Some(prior) = prior {
                let prior: Value = serde_json::from_str(&prior).map_err(storage)?;
                if prior["generation"] == generation {
                    plan["source_state"] = prior["target"].clone();
                }
            }
            if provider == "zentao" && kind != "bug" {
                return Err(invalid(
                    "ZenTao status writeback supports linked bug records",
                ));
            }
            let target = r.fields["mapping"]["write_status"][required(&row.fields, "status")?]
                .as_str()
                .filter(|s| compact_id(s) && *s != "." && *s != "..")
                .ok_or_else(|| invalid("configure a write_status mapping for this task status"))?;
            if provider == "zentao" && !matches!(target, "active" | "resolved" | "closed") {
                return Err(invalid("ZenTao target must be active, resolved or closed"));
            }
            plan["target"] = json!(target);
            if target == "resolved" && provider == "zentao" {
                plan["resolution"] = json!(
                    r.fields["mapping"]["resolution"]
                        .as_str()
                        .unwrap_or("fixed")
                );
                plan["resolved_build"] = json!(
                    r.fields["mapping"]["resolved_build"]
                        .as_str()
                        .unwrap_or("trunk")
                );
            }
            format!("status:{}:{target}", row.rev)
        }
        "comment" if provider == "pingcode" => {
            let comment = c.readable("comment", required(q, "comment_id")?)?;
            if comment.fields["task"] != task
                || comment.fields["visibility"] == "private"
                || comment.fields["external_provider"].is_string()
            {
                return Err(forbidden("select a local public comment on this task"));
            }
            plan["body"] = json!(required(&comment.fields, "body")?);
            format!("comment:{}", comment.id)
        }
        _ => return Err(invalid("unsupported publication intent")),
    };
    let previous:Option<Previous>=c.db.query_row("SELECT id,state,expires,remote,plan FROM sf_vendor_push WHERE space=? AND connector=? AND task=? AND request_key=?",params![c.space,r.id,task,request_key],|v|Ok(Previous{id:v.get(0)?,state:v.get(1)?,expires:v.get(2)?,remote:v.get(3)?,plan:v.get(4)?})).optional().map_err(storage)?;
    let delivery = if let Some(previous) = previous {
        if previous.state == "sent" {
            return Ok(
                json!({"delivery":previous.id,"state":previous.state,"remote_id":previous.remote}),
            );
        }
        if previous.state == "inflight" && previous.expires.is_some_and(|v| v > now()) {
            return Err(conflict("this change is being delivered"));
        }
        if mode == "comment" && matches!(previous.state.as_str(), "uncertain" | "inflight") {
            let known = q["confirm_comment"]
                .as_str()
                .filter(|s| compact_id(s) && *s != "." && *s != "..")
                .ok_or_else(|| {
                    conflict(
                        "confirm the upstream comment ID before finishing an uncertain publication",
                    )
                })?;
            let original: Value = serde_json::from_str(&previous.plan).map_err(storage)?;
            plan["body"] = original["body"].clone();
            plan["confirm_comment"] = json!(known);
        }
        previous.id
    } else {
        Uuid::new_v4().to_string()
    };
    let lease = Uuid::new_v4().to_string();
    c.db.execute("INSERT INTO sf_vendor_push(id,space,connector,task,request_key,plan,state,lease,expires,attempts,at) VALUES(?,?,?,?,?,?,'inflight',?,?,1,?) ON CONFLICT(id) DO UPDATE SET state='inflight',plan=excluded.plan,lease=excluded.lease,expires=excluded.expires,attempts=attempts+1,at=excluded.at",params![delivery,c.space,r.id,task,request_key,plan.to_string(),lease,now()+120000,now()]).map_err(storage)?;
    Ok(json!({"delivery":delivery,"lease":lease,"state":"inflight","plan":plan}))
}

pub(super) fn execute(c: &Context<'_>, r: &Record, op: &str, q: &Value) -> Result<Value> {
    if !matches!(r.fields["provider"].as_str(), Some("zentao" | "pingcode")) {
        return Err(invalid("writeback is unavailable for this connector"));
    }
    if op == "imports.prepare_push" {
        return prepare(c, r, q);
    }
    if op == "imports.finish_push" {
        let state = required(q, "result")?;
        if !matches!(state, "sent" | "failed" | "uncertain") {
            return Err(invalid("invalid publication result"));
        }
        let delivery = required(q, "delivery")?;
        let remote = q["remote_id"].as_str().filter(|v| compact_id(v));
        if c.db.execute("UPDATE sf_vendor_push SET state=?,lease=NULL,expires=NULL,remote=?,at=? WHERE space=? AND connector=? AND id=? AND state='inflight' AND lease=?",params![state,remote,now(),c.space,r.id,delivery,required(q,"lease")?]).map_err(storage)?!=1 {
            return Err(conflict("publication lease is unavailable"));
        }
        return Ok(json!({"delivery":delivery,"state":state,"remote_id":remote}));
    }
    let mut stmt=c.db.prepare("SELECT id,task,state,attempts,remote,at FROM sf_vendor_push WHERE space=? AND connector=? ORDER BY rowid DESC LIMIT 200").map_err(storage)?;
    let items=stmt.query_map(params![c.space,r.id],|v|Ok(json!({"delivery":v.get::<_,String>(0)?,"task":v.get::<_,String>(1)?,"state":v.get::<_,String>(2)?,"attempts":v.get::<_,i64>(3)?,"remote_id":v.get::<_,Option<String>>(4)?,"at":v.get::<_,i64>(5)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
    Ok(json!({"items":items}))
}
