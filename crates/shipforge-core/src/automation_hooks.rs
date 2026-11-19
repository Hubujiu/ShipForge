use crate::{Context, Record, Result, Store, conflict, identifier, invalid, required, storage};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;
mod delivery;
mod ip;
pub use delivery::WebhookReply;

pub(crate) const READS: &[&str] = &["automation.hook.settings", "automation.hook.history"];
pub(crate) const WRITES: &[&str] = &[
    "automation.hook.save",
    "automation.hook.issue",
    "automation.hook.rotate",
    "automation.hook.revoke",
    "automation.hook.configure",
    "automation.hook.secret",
    "automation.hook.reset_secret",
];
const BASE: &str = "/v1/automation-webhooks/";
pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS automation_hooks(
        space TEXT NOT NULL, rule TEXT NOT NULL, trigger TEXT NOT NULL, token_hash BLOB UNIQUE,
        secret TEXT, signing INTEGER NOT NULL DEFAULT 0, filtering INTEGER NOT NULL DEFAULT 0,
        allowlist TEXT NOT NULL DEFAULT '[]', rev INTEGER NOT NULL DEFAULT 0,
        window_at INTEGER NOT NULL DEFAULT 0, attempts INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY(space,rule,trigger));
        CREATE TABLE IF NOT EXISTS automation_hook_requests(space TEXT NOT NULL,actor TEXT NOT NULL, request_hash BLOB NOT NULL,
            PRIMARY KEY(space,actor,request_hash));
        CREATE TABLE IF NOT EXISTS automation_hook_replays(space TEXT NOT NULL,rule TEXT NOT NULL,trigger TEXT NOT NULL,
            signature BLOB NOT NULL, expires INTEGER NOT NULL, PRIMARY KEY(space,rule,trigger,signature),
            FOREIGN KEY(space,rule,trigger) REFERENCES automation_hooks(space,rule,trigger) ON DELETE CASCADE);
        CREATE TABLE IF NOT EXISTS automation_hook_history(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
            space TEXT NOT NULL,rule TEXT NOT NULL,trigger TEXT NOT NULL,at INTEGER NOT NULL,ip TEXT NOT NULL,
            status TEXT NOT NULL,signature_state TEXT NOT NULL,reason TEXT,run TEXT,
            FOREIGN KEY(space,rule,trigger) REFERENCES automation_hooks(space,rule,trigger) ON DELETE CASCADE);
        CREATE INDEX IF NOT EXISTS automation_hook_page ON automation_hook_history(space,rule,trigger,seq DESC);
        CREATE TRIGGER IF NOT EXISTS automation_hook_deleted AFTER DELETE ON records WHEN old.kind='automation'
           BEGIN DELETE FROM automation_hooks WHERE space=old.space AND rule=old.id; END;
        CREATE TRIGGER IF NOT EXISTS automation_hook_changed AFTER UPDATE OF fields ON records WHEN new.kind='automation'
           BEGIN DELETE FROM automation_hooks WHERE space=new.space AND rule=new.id AND trigger NOT IN(
             SELECT json_extract(t.value,'$.id') FROM json_each(new.fields,'$.triggers') t WHERE json_extract(t.value,'$.type')='webhook' AND json_type(t.value,'$.id')='text'); END;
        ").map_err(storage)?;
    // Upgrade existing unkeyed webhooks without disturbing schedule keys or named callback credentials.
    let mut st=db.prepare("SELECT space,id,fields FROM records r WHERE kind='automation' AND EXISTS(SELECT 1 FROM json_each(r.fields,'$.triggers') t WHERE json_extract(t.value,'$.type')='webhook' AND json_type(t.value,'$.id') IS NOT 'text')").map_err(storage)?;
    let old = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (space, id, raw) in old {
        let mut fields: Value = serde_json::from_str(&raw).map_err(storage)?;
        for t in fields["triggers"]
            .as_array_mut()
            .into_iter()
            .flatten()
            .filter(|t| t["type"] == "webhook" && !t["id"].is_string())
        {
            t["id"] = json!(Uuid::new_v4().to_string());
        }
        db.execute(
            "UPDATE records SET fields=? WHERE space=? AND kind='automation' AND id=?",
            params![fields.to_string(), space, id],
        )
        .map_err(storage)?;
    }
    Ok(())
}
pub(crate) fn identify(fields: &mut Value) -> Result<()> {
    let Some(triggers) = fields.get_mut("triggers").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut seen = HashSet::new();
    for t in triggers.iter_mut().filter(|t| t["type"] == "webhook") {
        if !t["id"].is_string() {
            t["id"] = json!(Uuid::new_v4().to_string());
        }
        let id = required(t, "id")?;
        identifier(id)?;
        if !seen.insert(id.to_owned()) {
            return Err(invalid(
                "webhook triggers in an automation need distinct IDs",
            ));
        }
        for field in [
            "secret",
            "signing_secret",
            "url",
            "token",
            "signing",
            "ip_filter",
            "allowlist",
        ] {
            if t.get(field).is_some() {
                return Err(invalid("use the dedicated Webhook security settings"));
            }
        }
    }
    Ok(())
}
fn random(prefix: &str) -> Result<String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| storage("random generator unavailable"))?;
    Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes)))
}
fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
fn open_row(c: &Context<'_>, r: &str, t: &str) -> Result<()> {
    c.db.execute(
        "INSERT OR IGNORE INTO automation_hooks(space,rule,trigger) VALUES(?,?,?)",
        params![c.space, r, t],
    )
    .map_err(storage)?;
    Ok(())
}
fn issue(c: &Context<'_>, r: &str, t: &str, rotate: bool) -> Result<Value> {
    open_row(c, r, t)?;
    let token = random("wh_")?;
    let n=c.db.execute("UPDATE automation_hooks SET token_hash=?,rev=rev+1,window_at=0,attempts=0 WHERE space=? AND rule=? AND trigger=? AND (token_hash IS NOT NULL)=?",params![hash(&token),c.space,r,t,rotate]).map_err(storage)?;
    if n != 1 {
        return Err(conflict(if rotate {
            "issue a URL before rotating it"
        } else {
            "a URL is already active; rotate it to obtain a new one"
        }));
    }
    let base =
        std::env::var("SHIPFORGE_PUBLIC_URL").unwrap_or_else(|_| "http://127.0.0.1:47138".into());
    let mut origin = url::Url::parse(&base).map_err(|_| invalid("invalid public service URL"))?;
    let local = matches!(origin.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback()||ip.is_unspecified())
        || matches!(origin.host(),Some(url::Host::Ipv6(ip)) if ip.is_loopback()||ip.is_unspecified())
        || origin.host_str() == Some("localhost");
    if origin.host().is_none()
        || !(origin.scheme() == "https" || origin.scheme() == "http" && local)
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(invalid("public service URL must use HTTPS or local HTTP"));
    }
    origin.set_path(&format!(
        "{}/{}/{}",
        origin.path().trim_end_matches('/'),
        BASE.trim_matches('/'),
        token
    ));
    Ok(
        json!({"id":r,"trigger_id":t,"path":format!("{BASE}{token}"),"url":origin.as_str(),"settings":settings(c,r,t)?}),
    )
}
fn settings(c: &Context<'_>, r: &str, t: &str) -> Result<Value> {
    let v:Option<(bool,bool,bool,bool,String,i64)>=c.db.query_row("SELECT token_hash IS NOT NULL,signing,secret IS NOT NULL,filtering,allowlist,rev FROM automation_hooks WHERE space=? AND rule=? AND trigger=?",params![c.space,r,t],|o|Ok((o.get(0)?,o.get(1)?,o.get(2)?,o.get(3)?,o.get(4)?,o.get(5)?))).optional().map_err(storage)?;
    let (active, signing, has_secret, filtering, raw, rev) =
        v.unwrap_or((false, false, false, false, "[]".into(), 0));
    let ips: Vec<String> = serde_json::from_str(&raw).map_err(storage)?;
    let last:Option<String>=c.db.query_row("SELECT ip FROM automation_hook_history WHERE space=? AND rule=? AND trigger=? ORDER BY seq DESC LIMIT 1",params![c.space,r,t],|r|r.get(0)).optional().map_err(storage)?;
    Ok(
        json!({"id":r,"trigger_id":t,"issued":active,"signing":signing,"has_secret":has_secret,"ip_filter":filtering,"allowlist":ips,"last_ip":last,"revision":rev}),
    )
}
fn trigger_ids(r: &Record) -> Vec<&str> {
    r.fields["triggers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["type"] == "webhook")
        .filter_map(|t| t["id"].as_str())
        .collect()
}
fn save(c: &Context<'_>, q: &Value) -> Result<Value> {
    let action = required(q, "action")?;
    if !matches!(action, "create" | "update") {
        return Err(invalid("choose create or update"));
    }
    let mut record = q.clone();
    record["op"] = json!(action);
    record["kind"] = json!("automation");
    let old = if action == "update" {
        Some(c.readable("automation", required(q, "id")?)?)
    } else {
        None
    };
    let previous = old.as_ref().map(trigger_ids).unwrap_or_default();
    let item = crate::domain::write(c, &record)?;
    let r = c.readable("automation", required(&item, "id")?)?;
    let mut issued = Vec::new();
    for trigger in trigger_ids(&r)
        .into_iter()
        .filter(|id| !previous.contains(id))
    {
        issued.push(issue(c, &r.id, trigger, false)?);
    }
    Ok(json!({"record":item,"issued":issued}))
}
pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    if WRITES.contains(&op)
        && op != "automation.hook.secret"
        && let Some(value) = q.get("request_id")
    {
        let key = value
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 160)
            .ok_or_else(|| invalid("invalid request_id"))?;
        let first =
            c.db.execute(
                "INSERT OR IGNORE INTO automation_hook_requests VALUES(?,?,?)",
                params![c.space, c.actor, hash(key)],
            )
            .map_err(storage)?;
        if first == 0 {
            return Err(conflict(
                "webhook change already applied; previously issued credentials are not replayed",
            ));
        }
    }
    if op == "automation.hook.save" {
        return save(c, q);
    }
    let r = c.readable("automation", required(q, "id")?)?;
    let trigger = required(q, "trigger_id")?;
    if !trigger_ids(&r).contains(&trigger) {
        return Err("not_found: webhook trigger".into());
    }
    if op == "automation.hook.history" {
        return delivery::history(c, &r.id, trigger, q);
    }
    c.authorize_write("automation", Some(&r))?;
    let current = settings(c, &r.id, trigger)?;
    if op == "automation.hook.settings" {
        return Ok(current);
    }
    if let Some(version) = q.get("expected_version")
        && version.as_i64() != current["revision"].as_i64()
    {
        return Err(conflict("Webhook settings changed; refresh before saving"));
    }
    if op == "automation.hook.issue" || op == "automation.hook.rotate" {
        return issue(c, &r.id, trigger, op == "automation.hook.rotate");
    }
    open_row(c, &r.id, trigger)?;
    match op {
        "automation.hook.revoke" => {
            c.db.execute("UPDATE automation_hooks SET token_hash=NULL,rev=rev+1 WHERE space=? AND rule=? AND trigger=? AND token_hash IS NOT NULL",params![c.space,r.id,trigger]).map_err(storage)?;
        }
        "automation.hook.configure" => {
            let flag = |name: &str| {
                q.get(name).map_or(Ok(current[name] == true), |v| {
                    v.as_bool()
                        .ok_or_else(|| invalid(&format!("{name} must be boolean")))
                })
            };
            let signing = flag("signing")?;
            let filtering = flag("ip_filter")?;
            let allow = ip::list(q.get("allowlist").unwrap_or(&current["allowlist"]))?;
            if filtering && allow.is_empty() {
                return Err(invalid("IP allowlist cannot be empty while enabled"));
            }
            let new_secret = if signing && current["has_secret"] != true {
                Some(random("whsec_")?)
            } else {
                None
            };
            c.db.execute("UPDATE automation_hooks SET signing=?,filtering=?,allowlist=?,secret=COALESCE(secret,?),rev=rev+1 WHERE space=? AND rule=? AND trigger=?",params![signing,filtering,json!(allow).to_string(),new_secret,c.space,r.id,trigger]).map_err(storage)?;
        }
        "automation.hook.reset_secret" | "automation.hook.secret" => {
            if current["has_secret"] != true {
                return Err(conflict("enable signature verification first"));
            }
            if op == "automation.hook.reset_secret" {
                c.db.execute("UPDATE automation_hooks SET secret=?,rev=rev+1 WHERE space=? AND rule=? AND trigger=?",params![random("whsec_")?,c.space,r.id,trigger]).map_err(storage)?;
            }
            let key: String =
                c.db.query_row(
                    "SELECT secret FROM automation_hooks WHERE space=? AND rule=? AND trigger=?",
                    params![c.space, r.id, trigger],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            return Ok(
                json!({"id":r.id,"trigger_id":trigger,"secret":key,"settings":settings(c,&r.id,trigger)?}),
            );
        }
        _ => return Err(invalid("unknown webhook management operation")),
    }
    settings(c, &r.id, trigger)
}
pub(crate) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    let ops = READS
        .iter()
        .chain(WRITES)
        .filter(|v| **v != "automation.hook.save")
        .copied()
        .collect::<Vec<_>>();
    rules.push(json!({"if":{"properties":{"op":{"enum":ops}}},"then":{"required":["id","trigger_id"],"properties":{"expected_version":{"type":"integer","minimum":0},"trigger_id":{"type":"string","minLength":1}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"automation.hook.save"}}},"then":{"required":["action"],"properties":{"action":{"enum":["create","update"]}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"automation.hook.configure"}}},"then":{"properties":{"signing":{"type":"boolean"},"ip_filter":{"type":"boolean"},"allowlist":{"type":"array","maxItems":20,"items":{"type":"string","maxLength":124}}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"automation.hook.history"}}},"then":{"properties":{"limit":{"type":"integer","minimum":1,"maximum":100},"before":{"type":"integer","minimum":1}}}}));
}
