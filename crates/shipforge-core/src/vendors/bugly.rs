use super::*;
use crate::{Store, domain, now, spaces};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{OptionalExtension, TransactionBehavior};
use uuid::Uuid;

/// Tencent Bugly OpenAPI 202100 authenticates the exact JSON bytes and then the canonical header.
pub fn bugly_authorization(api_id: &str, api_key: &str, body: &[u8]) -> Result<String> {
    if !compact_id(api_id) || api_key.is_empty() || body.is_empty() {
        return Err(invalid("invalid Bugly signing configuration"));
    }
    let key = hmac::Key::new(hmac::HMAC_SHA256, api_key.as_bytes());
    let sign = |bytes: &[u8]| {
        let hex = hmac::sign(&key, bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        base64::engine::general_purpose::STANDARD
            .encode(hex)
            .replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D")
    };
    let mut random = [0; 8];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| storage("unable to generate Bugly nonce"))?;
    let nonce = (u64::from_be_bytes(random) >> 2) + 100_000;
    let timestamp = now() / 1000;
    let canonical = format!(
        "apiID={api_id}&hashedPayload={}&nonce={nonce}&signMethod=HmacSHA256&timestamp={timestamp}&version=202100",
        sign(body)
    );
    Ok(format!(
        "{canonical}&signature={}",
        sign(canonical.as_bytes())
    ))
}

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("
    CREATE TABLE IF NOT EXISTS sf_bugly_hook(space TEXT NOT NULL,connector TEXT NOT NULL,route TEXT UNIQUE NOT NULL,tag BLOB NOT NULL,actor TEXT NOT NULL,product TEXT NOT NULL,PRIMARY KEY(space,connector));
    CREATE TABLE IF NOT EXISTS sf_bugly_link(space TEXT NOT NULL,connector TEXT NOT NULL,product TEXT NOT NULL,issue TEXT NOT NULL,kind TEXT NOT NULL,task TEXT NOT NULL,birth INTEGER NOT NULL,PRIMARY KEY(space,connector,product,issue), UNIQUE(space,task));
    CREATE TABLE IF NOT EXISTS sf_bugly_delivery(seq INTEGER PRIMARY KEY AUTOINCREMENT, space TEXT NOT NULL,connector TEXT NOT NULL,task TEXT NOT NULL,rev INTEGER NOT NULL,payload TEXT NOT NULL,state TEXT NOT NULL,lease TEXT,expiry INTEGER,attempts INTEGER NOT NULL DEFAULT 0,UNIQUE(space,task,rev));
    ").map_err(storage)
}

pub(super) fn disconnect(c: &Context<'_>, id: &str) -> Result<()> {
    c.db.execute(
        "DELETE FROM sf_bugly_hook WHERE space=? AND connector=?",
        params![c.space, id],
    )
    .map_err(storage)?;
    c.db.execute("UPDATE sf_bugly_delivery SET state='cancelled',lease=NULL WHERE space=? AND connector=? AND state IN('pending','leased')",params![c.space,id]).map_err(storage)?;
    Ok(())
}

pub(super) fn cancel_task(c: &Context<'_>, task: &str) -> Result<()> {
    c.db.execute("UPDATE sf_bugly_delivery SET state='cancelled',lease=NULL WHERE space=? AND task=? AND state IN('pending','leased')",params![c.space,task]).map_err(storage)?;
    Ok(())
}

fn rotate(c: &Context<'_>, r: &Record) -> Result<Value> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| storage("unable to generate callback credential"))?;
    let secret = URL_SAFE_NO_PAD.encode(bytes);
    let route = Uuid::new_v4().to_string();
    let tag = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes()),
        route.as_bytes(),
    );
    c.db.execute("INSERT INTO sf_bugly_hook(space,connector,route,tag,actor,product) VALUES(?,?,?,?,?,?) ON CONFLICT(space,connector) DO UPDATE SET route=excluded.route,tag=excluded.tag,actor=excluded.actor,product=excluded.product",params![c.space,r.id,route,tag.as_ref(),c.actor,required(&r.fields,"product_id")?]).map_err(storage)?;
    Ok(json!({"path":format!("/v1/bugly/submit/{route}?key={secret}")}))
}

fn checked(v: &Value, name: &str, limit: usize) -> Result<String> {
    let s = required(v, name)?.trim();
    if s.len() > limit || s.chars().any(char::is_control) {
        return Err(invalid("invalid Bugly field"));
    }
    Ok(s.to_owned())
}

fn org_mapping(c: &Context<'_>) -> Result<Value> {
    let org = c.org_id()?;
    Ok(c.db
        .query_row(
            "SELECT fields FROM records WHERE space=? AND kind='settings' AND id='bugly-defaults'",
            [org],
            |v| v.get::<_, String>(0),
        )
        .optional()
        .map_err(storage)?
        .map(|v| serde_json::from_str::<Value>(&v).map_err(storage))
        .transpose()?
        .unwrap_or(Value::Null))
}

fn mapped<'a>(r: &'a Record, defaults: &'a Value, kind: &str, key: &str) -> Option<&'a str> {
    r.fields["mapping"][kind][key]
        .as_str()
        .or_else(|| defaults["mapping"][kind][key].as_str())
}

impl Store {
    /// Receive the native Bugly callback using its current, connector-bound URL credential.
    pub fn bugly_submission(&mut self, route: &str, key: &str, payload: &Value) -> Result<Value> {
        let denied = || "unauthorized: invalid Bugly callback credential".to_owned();
        if Uuid::parse_str(route).is_err() || key.len() != 43 {
            return Err(denied());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let binding: Option<(String, String, String, Vec<u8>, String)> = tx
            .query_row(
                "SELECT space,connector,actor,tag,product FROM sf_bugly_hook WHERE route=?",
                [route],
                |v| Ok((v.get(0)?, v.get(1)?, v.get(2)?, v.get(3)?, v.get(4)?)),
            )
            .optional()
            .map_err(storage)?;
        let (space, id, actor, tag, product) = binding.ok_or_else(denied)?;
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
            route.as_bytes(),
            &tag,
        )
        .map_err(|_| denied())?;
        let c = Context {
            db: &tx,
            actor: &actor,
            space: &space,
        };
        spaces::ensure_active(&c)?;
        let r = connector(&c, &id, "bugly", false)?;
        if product != required(&r.fields, "product_id")? || payload["ProductID"] != product {
            return Err(crate::forbidden(
                "Bugly product does not match this binding",
            ));
        }
        let issue = checked(payload, "IssueKey", 160)?;
        let kind = checked(payload, "IssueType", 64)?;
        let existing:Option<(String,i64)>=c.db.query_row("SELECT task,birth FROM sf_bugly_link WHERE space=? AND connector=? AND product=? AND issue=?",params![space,id,product,issue],|v|Ok((v.get(0)?,v.get(1)?))).optional().map_err(storage)?;
        let task = if let Some((id, birth)) = existing {
            let found = c.readable("task", &id)?;
            let deleted:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM event_log WHERE space=? AND kind='task' AND id=? AND action='delete' AND rev>=?)",params![space,id,birth],|x|x.get(0)).map_err(storage)?;
            if deleted || found.fields["connector"] != r.id || found.fields["external_id"] != issue
            {
                return Err(conflict("the original Bugly task is unavailable"));
            }
            id
        } else {
            let f = &payload["FieldValues"];
            let title = required(f, "title")?;
            let description = f["description"].as_str().unwrap_or("");
            if description.len() > 1_048_576 {
                return Err(invalid("Bugly description exceeds 1 MiB"));
            }
            let raw_priority = f["priority"].as_str().unwrap_or("");
            let defaults = org_mapping(&c)?;
            let priority =
                mapped(&r, &defaults, "priority", raw_priority).unwrap_or(match raw_priority {
                    "urgent" | "high" | "medium" | "low" => raw_priority,
                    "lowest" | "Lowest" | "最低" => "lowest",
                    _ => "none",
                });
            let mut fields = json!({"source":"integration","connector":id,"external_id":issue,"external_provider":"bugly","description":description,"priority":priority,
                "source_priority":raw_priority,"source_reporter":f["reporter"].as_str(),"source_project":payload["ProjectName"].as_str(),"source_issue_type":kind,
                "source_assignees":f["current_owner"].as_str().filter(|s|!s.trim().is_empty()).into_iter().collect::<Vec<_>>()});
            if let Some(t) = r.fields["default_task_type"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                fields["type"] = json!(t);
            }
            if let Some(d) = f["deadline"].as_str() {
                fields["source_deadline"] = json!(d);
            }
            let task = domain::write_import(
                &c,
                &json!({"op":"create","kind":"task","title":title,"fields":fields}),
                true,
            )?;
            let task_id = required(&task, "id")?.to_owned();
            c.db.execute("INSERT INTO sf_bugly_link(space,connector,product,issue,kind,task,birth) VALUES(?,?,?,?,?,?,?)",params![space,id,product,issue,kind,task_id,task["rev"].as_i64()]).map_err(storage)?;
            task_id
        };
        tx.commit().map_err(storage)?;
        Ok(json!({"space":space,"task":task}))
    }
}

pub(super) fn status(c: &Context<'_>, task: &Record) -> Result<()> {
    if task.fields["external_provider"] != "bugly" {
        return Ok(());
    }
    let Some(id) = task.fields["connector"].as_str() else {
        return Ok(());
    };
    let Some(r) = c.find("connector", id)? else {
        return Ok(());
    };
    if r.fields["provider"] != "bugly"
        || r.fields["enabled"] == false
        || r.fields["bugly_outbound"] != true
    {
        return Ok(());
    }
    if task.fields["visibility"] == "private" {
        return Ok(());
    }
    let binding:Option<(String,String,String)>=c.db.query_row("SELECT l.product,l.issue,l.kind FROM sf_bugly_link l JOIN sf_bugly_hook h ON h.space=l.space AND h.connector=l.connector AND h.product=l.product WHERE l.space=? AND l.connector=? AND l.task=? AND NOT EXISTS(SELECT 1 FROM event_log e WHERE e.space=l.space AND e.kind='task' AND e.id=l.task AND e.action='delete' AND e.rev>=l.birth)",params![c.space,id,task.id],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?))).optional().map_err(storage)?;
    let Some((product, issue, kind)) = binding else {
        return Ok(());
    };
    let local = required(&task.fields, "status")?;
    let defaults = org_mapping(c)?;
    let Some(remote) = mapped(&r, &defaults, "status", local).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let payload = json!({"product_id":product,"issue_key":issue,"issue_type":kind,"operator":"ShipForge","issue_status":remote});
    c.db.execute("INSERT OR IGNORE INTO sf_bugly_delivery(space,connector,task,rev,payload,state) VALUES(?,?,?,?,?,'pending')",params![c.space,id,task.id,task.rev,payload.to_string()]).map_err(storage)?;
    Ok(())
}

fn claim(c: &Context<'_>, r: &Record) -> Result<Value> {
    if r.fields["bugly_outbound"] != true {
        return Err(invalid("Bugly status writeback is disabled"));
    }
    // Never let a later transition overtake a leased transition.
    loop {
        let entry:Option<(i64,String,String,String,Option<i64>)>=c.db.query_row("SELECT seq,task,payload,state,expiry FROM sf_bugly_delivery WHERE space=? AND connector=? AND state IN('pending','leased') ORDER BY seq LIMIT 1",params![c.space,r.id],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?,v.get(3)?,v.get(4)?))).optional().map_err(storage)?;
        let Some((seq, task, body, state, expiry)) = entry else {
            return Ok(Value::Null);
        };
        if state == "leased" && expiry.unwrap_or_default() > now() {
            return Ok(Value::Null);
        }
        if c.find("task", &task)?
            .is_none_or(|t| t.fields["visibility"] == "private" || t.fields["connector"] != r.id)
        {
            c.db.execute(
                "UPDATE sf_bugly_delivery SET state='cancelled',lease=NULL WHERE seq=?",
                [seq],
            )
            .map_err(storage)?;
            continue;
        }
        c.readable("task", &task)?;
        let lease = Uuid::new_v4().to_string();
        c.db.execute("UPDATE sf_bugly_delivery SET state='leased',attempts=attempts+1,lease=?,expiry=? WHERE seq=?",params![lease,now()+120000,seq]).map_err(storage)?;
        let payload: Value = serde_json::from_str(&body).map_err(storage)?;
        return Ok(json!({"delivery":seq,"lease":lease,"payload":payload}));
    }
}

pub(super) fn execute(c: &Context<'_>, r: &Record, q: &Value, op: &str) -> Result<Value> {
    match op {
        "bugly.binding" => {
            Ok(json!({"organization":c.org_id()?,"product_id":r.fields["product_id"]}))
        }
        "bugly.rotate" => rotate(c, r),
        "bugly.revoke" => {
            disconnect(c, &r.id)?;
            Ok(json!({"revoked":true}))
        }
        "bugly.claim" => claim(c, r),
        "bugly.settle" => {
            let seq = q["delivery"]
                .as_i64()
                .ok_or_else(|| invalid("delivery number required"))?;
            let state = if q["success"]
                .as_bool()
                .ok_or_else(|| invalid("success must be boolean"))?
            {
                "sent"
            } else {
                "pending"
            };
            if c.db.execute("UPDATE sf_bugly_delivery SET state=?,lease=NULL,expiry=NULL WHERE seq=? AND space=? AND connector=? AND state='leased' AND lease=?",params![state,seq,c.space,r.id,required(q,"lease")?]).map_err(storage)?!=1 {
                return Err(conflict("Bugly delivery lease is unavailable"));
            }
            Ok(json!({"delivery":seq,"state":state}))
        }
        "bugly.outbox" => {
            let mut stmt=c.db.prepare("SELECT seq,task,rev,state,attempts FROM sf_bugly_delivery WHERE space=? AND connector=? ORDER BY seq DESC LIMIT 200").map_err(storage)?;
            let items=stmt.query_map(params![c.space,r.id],|v|Ok(json!({"delivery":v.get::<_,i64>(0)?,"task":v.get::<_,String>(1)?,"rev":v.get::<_,i64>(2)?,"state":v.get::<_,String>(3)?,"attempts":v.get::<_,i64>(4)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
            Ok(json!({"items":items}))
        }
        _ => Err(invalid("unknown Bugly operation")),
    }
}
