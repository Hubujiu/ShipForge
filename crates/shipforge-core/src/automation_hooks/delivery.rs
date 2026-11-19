use super::*;
use base64::engine::general_purpose::STANDARD;
use ring::hmac;
use rusqlite::TransactionBehavior;
use std::net::IpAddr;

pub struct WebhookReply {
    pub status: u16,
    pub body: Value,
    pub retry_after: Option<i64>,
}
impl WebhookReply {
    fn unknown() -> Self {
        Self {
            status: 404,
            body: json!({"success":false,"error":{"code":"not_found"}}),
            retry_after: None,
        }
    }
}
struct Target {
    space: String,
    rule: String,
    trigger: String,
    signing: bool,
    secret: Option<String>,
    filter: bool,
    ips: Vec<String>,
    start: i64,
    count: i64,
}
fn lookup(db: &Connection, token: &str) -> Result<Option<Target>> {
    if token.len() != 46
        || !token.starts_with("wh_")
        || !token[3..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Ok(None);
    }
    let value=db.query_row("SELECT space,rule,trigger,signing,secret,filtering,allowlist,window_at,attempts FROM automation_hooks WHERE token_hash=?",[hash(token)],|r|Ok((Target{space:r.get(0)?,rule:r.get(1)?,trigger:r.get(2)?,signing:r.get(3)?,secret:r.get(4)?,filter:r.get(5)?,ips:vec![],start:r.get(7)?,count:r.get(8)?},r.get::<_,String>(6)?))).optional().map_err(storage)?;
    value
        .map(|(mut v, ips)| {
            v.ips = serde_json::from_str(&ips).map_err(storage)?;
            Ok(v)
        })
        .transpose()
}
struct Outcome<'a> {
    code: u16,
    sign: &'a str,
    reason: Option<&'a str>,
    run: Option<&'a str>,
}
fn log(
    db: &Connection,
    target: &Target,
    source: IpAddr,
    id: &str,
    outcome: Outcome<'_>,
) -> Result<WebhookReply> {
    let Outcome {
        code,
        sign,
        reason,
        run,
    } = outcome;
    let state = if code >= 400 {
        "rejected"
    } else if run.is_some() {
        "accepted"
    } else {
        "skipped"
    };
    db.execute("INSERT INTO automation_hook_history(id,space,rule,trigger,at,ip,status,signature_state,reason,run) VALUES(?,?,?,?,?,?,?,?,?,?)",params![id,target.space,target.rule,target.trigger,crate::now(),ip::canonical(source).to_string(),state,sign,reason,run]).map_err(storage)?;
    Ok(WebhookReply {
        status: code,
        body: json!({"success":true,"data":{"deliveryId":id,"status":state,"reason":reason}}),
        retry_after: None,
    })
}
fn rejection(
    db: &Connection,
    t: &Target,
    source: IpAddr,
    code: u16,
    sign: &str,
    reason: &str,
) -> Result<WebhookReply> {
    log(
        db,
        t,
        source,
        &Uuid::new_v4().to_string(),
        Outcome {
            code,
            sign,
            reason: Some(reason),
            run: None,
        },
    )
}
fn signature(
    key: &str,
    data: &[u8],
    signed: Option<&str>,
    timestamp: Option<&str>,
    github: Option<&str>,
    now_ms: i64,
) -> std::result::Result<Option<Vec<u8>>, (&'static str, &'static str)> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
    let invalid = ("invalid", "invalid_signature");
    let missing = ("missing", "missing_signature");
    if let Some(signed) = signed {
        let ts = timestamp.filter(|s| !s.is_empty()).ok_or(missing)?;
        if !ts.bytes().all(|b| b.is_ascii_digit()) {
            return Err(("expired", "timestamp_expired"));
        }
        let n = ts
            .parse::<i64>()
            .map_err(|_| ("expired", "timestamp_expired"))?;
        let expired = if ts.len() == 13 {
            now_ms.abs_diff(n) > 300_000
        } else {
            (now_ms / 1000).abs_diff(n) > 300
        };
        if expired {
            return Err(("expired", "timestamp_expired"));
        }
        let sig = STANDARD
            .decode(signed.strip_prefix("v1=").ok_or(invalid)?)
            .map_err(|_| invalid)?;
        let mut message = Vec::with_capacity(ts.len() + 1 + data.len());
        message.extend_from_slice(ts.as_bytes());
        message.push(b'\n');
        message.extend_from_slice(data);
        hmac::verify(&key, &message, &sig).map_err(|_| invalid)?;
        Ok(Some(sig))
    } else if let Some(git) = github {
        let raw = git
            .strip_prefix("sha256=")
            .filter(|s| s.len() == 64 && s.is_ascii())
            .ok_or(invalid)?;
        let sig = (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).map_err(|_| invalid))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        hmac::verify(&key, data, &sig).map_err(|_| invalid)?;
        Ok(None)
    } else {
        Err(missing)
    }
}
impl Store {
    /// Resolve a confidential capability, rate-limit it and check the socket peer before reading any body.
    pub fn automation_hook_preflight(
        &mut self,
        token: &str,
        source: IpAddr,
    ) -> Result<Option<WebhookReply>> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let Some(t) = lookup(&tx, token)? else {
            return Ok(Some(WebhookReply::unknown()));
        };
        let now = chrono::Utc::now().timestamp();
        let elapsed = now.saturating_sub(t.start);
        if elapsed < 60 && t.count >= 60 {
            return Ok(Some(WebhookReply {
                status: 429,
                body: json!({"success":false,"error":{"code":"RATE_LIMITED"}}),
                retry_after: Some((60 - elapsed).clamp(1, 60)),
            }));
        }
        let (start, count) = if elapsed >= 60 {
            (now, 1)
        } else {
            (t.start, t.count + 1)
        };
        tx.execute(
            "UPDATE automation_hooks SET window_at=?,attempts=? WHERE token_hash=?",
            params![start, count, hash(token)],
        )
        .map_err(storage)?;
        let reply = if t.filter && !ip::permits(&t.ips, source) {
            Some(rejection(
                &tx,
                &t,
                source,
                403,
                "ip_not_allowed",
                "ip_not_allowed",
            )?)
        } else {
            None
        };
        tx.commit().map_err(storage)?;
        Ok(reply)
    }
    /// Recheck the capability and its policy under the same transaction as authentication, replay protection and dispatch.
    pub fn automation_hook_receive(
        &mut self,
        token: &str,
        source: IpAddr,
        raw: &[u8],
        signed: Option<&str>,
        timestamp: Option<&str>,
        github: Option<&str>,
    ) -> Result<WebhookReply> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let Some(t) = lookup(&tx, token)? else {
            return Ok(WebhookReply::unknown());
        };
        let result = receive(&tx, &t, source, raw, signed, timestamp, github)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}
fn receive(
    db: &Connection,
    t: &Target,
    source: IpAddr,
    raw: &[u8],
    signed: Option<&str>,
    timestamp: Option<&str>,
    github: Option<&str>,
) -> Result<WebhookReply> {
    if t.filter && !ip::permits(&t.ips, source) {
        return rejection(db, t, source, 403, "ip_not_allowed", "ip_not_allowed");
    }
    if raw.len() > 131072 {
        return rejection(db, t, source, 413, "unchecked", "body_too_large");
    }
    let now_ms = chrono::Utc::now().timestamp_millis();
    let now = now_ms / 1000;
    let sign = if t.signing { "valid" } else { "not_required" };
    let replay = if t.signing {
        match signature(
            t.secret
                .as_deref()
                .ok_or_else(|| storage("webhook signing configuration missing"))?,
            raw,
            signed,
            timestamp,
            github,
            now_ms,
        ) {
            Ok(sig) => sig,
            Err((sign, reason)) => return rejection(db, t, source, 401, sign, reason),
        }
    } else {
        None
    };
    if let Some(ref sig) = replay {
        db.execute("DELETE FROM automation_hook_replays WHERE space=? AND rule=? AND trigger=? AND expires<=?",params![t.space,t.rule,t.trigger,now]).map_err(storage)?;
        let known:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM automation_hook_replays WHERE space=? AND rule=? AND trigger=? AND signature=?)",params![t.space,t.rule,t.trigger,sig],|r|r.get(0)).map_err(storage)?;
        if known {
            return rejection(db, t, source, 409, "invalid", "replayed_signature");
        }
    }
    let payload = if raw.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(raw) {
            Ok(v) if v.is_object() => v,
            _ => return rejection(db, t, source, 400, sign, "invalid_json"),
        }
    };
    let id = Uuid::new_v4().to_string();
    let fields: String = db
        .query_row(
            "SELECT fields FROM records WHERE space=? AND kind='automation' AND id=?",
            params![t.space, t.rule],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let fields: Value = serde_json::from_str(&fields).map_err(storage)?;
    let owner = fields["creator"].as_str().unwrap_or("");
    let c = Context {
        db,
        actor: owner,
        space: &t.space,
    };
    let trigger = fields["triggers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["id"] == t.trigger && v["type"] == "webhook");
    let event = if fields["enabled"] != true
        || fields["archived"] == true
        || fields["state"] == "archived"
    {
        ("automation_paused", None)
    } else if trigger.is_none_or(|t| t["enabled"] == false) {
        ("trigger_disabled", None)
    } else if crate::identity::ensure_active(db, owner).is_err()
        || c.require_edit().is_err()
        || c.readable("automation", &t.rule).is_err()
    {
        ("access_unavailable", None)
    } else {
        db.execute_batch("SAVEPOINT webhook_dispatch")
            .map_err(storage)?;
        let q = json!({"op":"automation.fire","id":t.rule,"trigger_id":t.trigger,"source":"webhook","event_id":id,"payload":payload});
        let outcome = crate::worker::dispatch(&c, &q)
            .ok_or_else(|| invalid("automation dispatcher unavailable"))?;
        match outcome {
            Ok(run) => {
                db.execute_batch("RELEASE webhook_dispatch")
                    .map_err(storage)?;
                ("", Some(required(&run, "id")?.to_owned()))
            }
            Err(e) if e.starts_with("storage:") => return Err(e),
            Err(_) => {
                db.execute_batch("ROLLBACK TO webhook_dispatch; RELEASE webhook_dispatch")
                    .map_err(storage)?;
                ("run_unavailable", None)
            }
        }
    };
    if let Some(sig) = replay {
        db.execute(
            "INSERT INTO automation_hook_replays VALUES(?,?,?,?,?)",
            params![t.space, t.rule, t.trigger, sig, now + 600],
        )
        .map_err(storage)?;
    }
    log(
        db,
        t,
        source,
        &id,
        Outcome {
            code: 202,
            sign,
            reason: (!event.0.is_empty()).then_some(event.0),
            run: event.1.as_deref(),
        },
    )
}
pub(super) fn history(c: &Context<'_>, r: &str, t: &str, q: &Value) -> Result<Value> {
    let limit = q.get("limit").map_or(Ok(25), |v| {
        v.as_i64()
            .filter(|n| (1..=100).contains(n))
            .ok_or_else(|| invalid("limit must be 1 through 100"))
    })?;
    let before = q.get("before").map_or(Ok(i64::MAX), |v| {
        v.as_i64()
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("invalid delivery cursor"))
    })?;
    let mut st=c.db.prepare("SELECT seq,id,at,ip,status,signature_state,reason,run FROM automation_hook_history WHERE space=? AND rule=? AND trigger=? AND seq<? ORDER BY seq DESC LIMIT ?").map_err(storage)?;
    let mut list = Vec::new();
    for row in st.query_map(params![c.space,r,t,before,limit+1],|r|Ok((r.get::<_,i64>(0)?,json!({"id":r.get::<_,String>(1)?,"at":r.get::<_,i64>(2)?,"source_ip":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,"signature_state":r.get::<_,String>(5)?,"reason":r.get::<_,Option<String>>(6)?,"run":r.get::<_,Option<String>>(7)?})))) .map_err(storage)?{
        let(seq,mut data)=row.map_err(storage)?;if let Some(run)=data["run"].as_str()&&c.readable("run",run).is_err(){data["run"]=Value::Null;}list.push((seq,data));
    }
    let more = list.len() > limit as usize;
    list.truncate(limit as usize);
    let next = more.then(|| list.last().map(|v| v.0)).flatten();
    Ok(
        json!({"id":r,"trigger_id":t,"items":list.into_iter().map(|(_,v)|v).collect::<Vec<_>>(),"next_before":next}),
    )
}
