use super::*;
use chrono::Timelike;
use serde::{Deserialize, Serialize};

const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;
const GIB: u64 = 1 << 30;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Policy {
    enabled: bool,
    interval_minutes: u32,
    retention_days: u32,
    cache_retention_days: u32,
    quota_gib: u32,
    min_free_gib: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 1440,
            retention_days: 7,
            cache_retention_days: 7,
            quota_gib: 0,
            min_free_gib: 0,
        }
    }
}
struct Settings {
    rev: i64,
    policy: Policy,
    next_scan: i64,
    next_cleanup: i64,
    episode: Option<String>,
}
pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_storage_policy(space TEXT,machine TEXT,rev INTEGER NOT NULL,config TEXT NOT NULL,next_scan INTEGER NOT NULL,next_cleanup INTEGER NOT NULL,episode TEXT,PRIMARY KEY(space,machine));
    CREATE TABLE IF NOT EXISTS sf_storage_pins(space TEXT,machine TEXT,workspace TEXT,PRIMARY KEY(space,machine,workspace));
    CREATE TRIGGER IF NOT EXISTS sf_storage_policy_removed AFTER DELETE ON records WHEN OLD.kind='machine' BEGIN DELETE FROM sf_storage_policy WHERE space=OLD.space AND machine=OLD.id;DELETE FROM sf_storage_pins WHERE space=OLD.space AND machine=OLD.id;END;").map_err(storage)
}
fn settings(c: &Context<'_>, m: &str) -> Result<Settings> {
    let raw:Option<(i64,String,i64,i64,Option<String>)>=c.db.query_row("SELECT rev,config,next_scan,next_cleanup,episode FROM sf_storage_policy WHERE space=? AND machine=?",params![c.space,m],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(storage)?;
    if let Some((rev, serialized, next_scan, next_cleanup, episode)) = raw {
        return Ok(Settings {
            rev,
            policy: serde_json::from_str(&serialized).map_err(storage)?,
            next_scan,
            next_cleanup,
            episode,
        });
    }
    Ok(Settings {
        rev: 0,
        policy: Policy::default(),
        next_scan: 0,
        next_cleanup: 0,
        episode: None,
    })
}
pub(super) fn configure(c: &Context<'_>, m: &Record, q: &Value) -> Result<Value> {
    let before = settings(c, &m.id)?;
    if q["expected_rev"].as_i64() != Some(before.rev) {
        return Err(conflict("storage policy changed; read it again"));
    }
    if !q["policy"].is_object() {
        return Err(invalid("a storage policy is required"));
    }
    let mut body = json!(before.policy);
    body.as_object_mut()
        .unwrap()
        .extend(q["policy"].as_object().unwrap().clone());
    let after: Policy =
        serde_json::from_value(body).map_err(|_| invalid("invalid storage policy fields"))?;
    if !(1..=10080).contains(&after.interval_minutes)
        || after.retention_days > 3650
        || after.cache_retention_days > 3650
        || after.quota_gib > 1048576
        || after.min_free_gib > 1048576
    {
        return Err(invalid("storage interval, retention or limit out of range"));
    }
    let rev = before.rev + 1;
    c.db.execute("INSERT INTO sf_storage_policy(space,machine,rev,config,next_scan,next_cleanup) VALUES(?,?,?,?,0,0) ON CONFLICT(space,machine) DO UPDATE SET rev=excluded.rev,config=excluded.config,next_scan=0,next_cleanup=0",params![c.space,m.id,rev,json!(after).to_string()]).map_err(storage)?;
    c.db.execute("UPDATE sf_storage_jobs SET state='failed',outcome='policy_changed' WHERE space=? AND machine=? AND state='queued' AND json_extract(command,'$.cycle') IS NOT NULL",params![c.space,m.id]).map_err(storage)?;
    Ok(json!({"rev":rev,"policy":after}))
}
pub(super) fn pinned(c: &Context<'_>, machine: &str, item: &Value) -> Result<bool> {
    c.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sf_storage_pins WHERE space=? AND machine=? AND workspace=?)",
        params![c.space, machine, item["id"].as_str()],
        |r| r.get(0),
    )
    .map_err(storage)
}
pub(super) fn pin(c: &Context<'_>, m: &Record, q: &Value, listing: &Value) -> Result<Value> {
    let value = q["pinned"]
        .as_bool()
        .ok_or_else(|| invalid("pinned must be boolean"))?;
    if !key(&q["workspace"])
        || !listing["entries"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|e| e["id"] == q["workspace"]))
    {
        return Err(conflict("scan the machine before changing a storage pin"));
    }
    if reserved(c, &m.id)? {
        return Err(conflict("a storage deletion is already committing"));
    }
    let sql = if value {
        "INSERT OR IGNORE INTO sf_storage_pins VALUES(?,?,?)"
    } else {
        "DELETE FROM sf_storage_pins WHERE space=? AND machine=? AND workspace=?"
    };
    c.db.execute(sql, params![c.space, m.id, q["workspace"].as_str()])
        .map_err(storage)?;
    Ok(json!({"workspace":q["workspace"],"pinned":value}))
}
pub(super) fn outbox(c: &Context<'_>, run: &Record) -> Result<bool> {
    c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_im_outbox WHERE space=?1 AND (source=?2 OR task=?3) AND state NOT IN('sent','cancelled')) OR EXISTS(SELECT 1 FROM external_bot_turns t JOIN external_bots b ON b.id=t.bot WHERE b.space=?1 AND t.run=?2 AND (t.prepared=0 OR EXISTS(SELECT 1 FROM external_bot_deliveries d WHERE d.turn=t.id AND d.state NOT IN('sent','discarded','cancelled'))))",params![c.space,run.id,run.fields["task_id"].as_str()],|r|r.get(0)).map_err(storage)
}
fn health(policy: &Policy, listing: &Value) -> Value {
    let total = listing["total_bytes"].as_u64().unwrap_or_default();
    let quota = u64::from(policy.quota_gib) * GIB;
    let minimum = u64::from(policy.min_free_gib) * GIB;
    let quota_pressure = quota > 0 && total >= quota.saturating_mul(9) / 10;
    let free = listing["available_bytes"].as_u64();
    let disk_pressure = minimum > 0 && free.is_some_and(|n| n < minimum);
    let state = if listing["scanned_at"].is_null() {
        "unknown"
    } else {
        match (quota_pressure, disk_pressure) {
            (true, true) => "quota_and_low_disk",
            (true, false) => "quota",
            (false, true) => "low_disk",
            _ if minimum > 0 && free.is_none() => "unknown",
            _ => "ok",
        }
    };
    json!({"state":state,"pressure":quota_pressure||disk_pressure,"quota_bytes":quota,"minimum_free_bytes":minimum,"available_bytes":free,"quota_pressure":quota_pressure,"disk_pressure":disk_pressure})
}
fn eligible(
    c: &Context<'_>,
    m: &Record,
    row: &Value,
    runs: &[Record],
    policy: &Policy,
) -> Result<Option<i64>> {
    if row["safe_for_schedule"] != true || !row["protection"].is_null() {
        return Ok(None);
    }
    let Some(mut last) = row["last_used_at"].as_i64().filter(|t| *t > 0) else {
        return Ok(None);
    };
    let retention = if row["category"] == "cache" {
        policy.cache_retention_days
    } else {
        policy.retention_days
    };
    if row["category"] != "cache" {
        for id in array_ids(&row["runs"])? {
            let Some(run) = runs.iter().find(|r| r.id == id) else {
                return Ok(None);
            };
            if run.fields["config"]["local_directory"].is_string()
                || run.fields["local_directory"].is_string()
                || run.fields["config"]["directory_mode"] == "direct"
            {
                return Ok(None);
            }
            let Some(finished) = run.fields["finished_at"].as_i64().filter(|t| *t > 0) else {
                return Ok(None);
            };
            last = last.max(finished);
        }
    }
    let minimum_age = if row["category"] == "cache" { 0 } else { HOUR };
    let old_enough = now().saturating_sub(last) >= (i64::from(retention) * DAY).max(minimum_age);
    Ok((old_enough && condition(c, &m.id, row, runs)?.0.is_none()).then_some(last))
}
pub(super) fn decorate(c: &Context<'_>, m: &Record, listing: &mut Value) -> Result<()> {
    let s = settings(c, &m.id)?;
    listing["health"] = health(&s.policy, listing);
    let runs = c.records("run")?;
    let mut bytes = 0u64;
    for row in listing["entries"].as_array_mut().into_iter().flatten() {
        let ok = eligible(c, m, row, &runs, &s.policy)?.is_some();
        row["policy_eligible"] = json!(ok);
        if ok {
            bytes = bytes.saturating_add(row["bytes"].as_u64().unwrap_or_default());
        }
    }
    listing["policy_eligible_bytes"] = json!(bytes);
    listing["policy_rev"] = json!(s.rev);
    listing["policy"] = json!(s.policy);
    listing["stale"] = json!(
        listing["scanned_at"]
            .as_i64()
            .is_none_or(|t| now() - t > 600_000)
            || !machine_is_online_at(&m.fields, now())
    );
    Ok(())
}
fn queue(c: &Context<'_>, m: &str, command: &Value) -> Result<bool> {
    c.db.execute("INSERT OR IGNORE INTO sf_storage_jobs(space,machine,id,command,state,at) VALUES(?,?,?,?,'queued',?)",params![c.space,m,Uuid::new_v4().to_string(),command.to_string(),now()]).map(|n|n!=0).map_err(storage)
}
pub(super) fn start_due(c: &Context<'_>, m: &Record, supported: bool) -> Result<()> {
    c.db.execute("UPDATE sf_storage_jobs SET state='failed',outcome='rescan_required' WHERE space=? AND machine=? AND state='committing' AND expires<=?",params![c.space,m.id,now()]).map_err(storage)?;
    if !supported {
        return Ok(());
    }
    let s = settings(c, &m.id)?;
    let monitored = s.policy.quota_gib > 0 || s.policy.min_free_gib > 0;
    if (!s.policy.enabled && !monitored) || now() < s.next_scan {
        return Ok(());
    }
    let regular = s.policy.enabled && now() >= s.next_cleanup;
    let interval = i64::from(s.policy.interval_minutes) * 60_000;
    let cmd = json!({"action":"scan","cycle":Uuid::new_v4().to_string(),"policy_rev":s.rev,"regular":regular});
    if queue(c, &m.id, &cmd)? {
        let scan = now()
            + if monitored {
                interval.min(600_000)
            } else {
                interval
            };
        c.db.execute(
            "UPDATE sf_storage_policy SET next_scan=?,next_cleanup=? WHERE space=? AND machine=?",
            params![
                scan,
                if regular {
                    now() + interval
                } else {
                    s.next_cleanup
                },
                c.space,
                m.id
            ],
        )
        .map_err(storage)?;
    }
    Ok(())
}
pub(super) fn reserve(
    c: &Context<'_>,
    m: &Record,
    body: &Value,
    listing: &Value,
    item: &Value,
) -> Result<()> {
    if body["cycle"].is_null() {
        return Ok(());
    }
    let s = settings(c, &m.id)?;
    if !s.policy.enabled
        || body["policy_rev"] != s.rev
        || listing["scanned_at"]
            .as_i64()
            .is_none_or(|at| now() - at > 600_000)
        || eligible(c, m, item, &c.records("run")?, &s.policy)?.is_none()
    {
        return Err(conflict("scheduled cleanup eligibility changed"));
    }
    Ok(())
}
fn notice(c: &Context<'_>, m: &Record, s: &Settings, listing: &Value) -> Result<()> {
    let status = health(&s.policy, listing);
    if status["pressure"] != true {
        if s.episode.is_some() && status["state"] != "unknown" {
            c.db.execute(
                "UPDATE sf_storage_policy SET episode=NULL WHERE space=? AND machine=?",
                params![c.space, m.id],
            )
            .map_err(storage)?;
        }
        return Ok(());
    }
    if s.episode.is_some()
        || listing["local_hour"]
            .as_u64()
            .unwrap_or_else(|| u64::from(chrono::Local::now().hour()))
            < 7
    {
        return Ok(());
    }
    let owner = req(&m.fields, "owner")?;
    let fields = json!({"recipient":owner,"machine":m.id,"kind":"storage_pressure","event":"storage_pressure","state":"inbox","read":false,"importance":"important","source_kind":"machine","source_id":m.id,"count":1});
    let notice = c.create(
        "notification",
        None,
        &format!("机器存储空间不足 · {}", m.title),
        fields,
    )?;
    c.db.execute(
        "UPDATE sf_storage_policy SET episode=? WHERE space=? AND machine=?",
        params![notice.id, c.space, m.id],
    )
    .map_err(storage)?;
    crate::inbox::notify_record(c, &notice, "storage_pressure", Some(("machine", &m.id)))
}
pub(super) fn finished(c: &Context<'_>, m: &Record, body: &Value, scanned: bool) -> Result<()> {
    if !scanned {
        return Ok(());
    }
    let s = settings(c, &m.id)?;
    if s.rev == 0 {
        return Ok(());
    }
    let owner = Context {
        actor: req(&m.fields, "owner")?,
        ..*c
    };
    let listing = inventory(c, &m.id)?;
    notice(&owner, m, &s, &listing)?;
    let Some(cycle) = body["cycle"].as_str() else {
        return Ok(());
    };
    if !s.policy.enabled
        || body["policy_rev"] != s.rev
        || body["regular"] != true && health(&s.policy, &listing)["pressure"] != true
    {
        return Ok(());
    }
    let mut stmt=c.db.prepare("SELECT json_extract(command,'$.workspace') FROM sf_storage_jobs WHERE space=? AND machine=? AND json_extract(command,'$.cycle')=? AND json_extract(command,'$.action')='clean'").map_err(storage)?;
    let attempted = stmt
        .query_map(params![c.space, m.id, cycle], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<std::result::Result<HashSet<_>, _>>()
        .map_err(storage)?;
    if attempted.len() >= 64 {
        return Ok(());
    }
    let runs = owner.records("run")?;
    let mut candidates = Vec::new();
    for entry in listing["entries"].as_array().into_iter().flatten() {
        let Some(id) = entry["id"].as_str() else {
            continue;
        };
        if !attempted.contains(id)
            && let Some(last) = eligible(&owner, m, entry, &runs, &s.policy)?
        {
            candidates.push((entry["category"] != "cache", last, id, entry));
        }
    }
    candidates.sort_by_key(|(kind, last, id, _)| (*kind, *last, *id));
    if let Some((_, _, _, entry)) = candidates.first() {
        queue(
            c,
            &m.id,
            &json!({"action":"clean","workspace":entry["id"],"expected_snapshot":entry["snapshot"],"cycle":cycle,"regular":body["regular"],"policy_rev":s.rev}),
        )?;
    }
    Ok(())
}
pub(super) fn schema(s: &mut Value) {
    let items = s["allOf"].as_array_mut().unwrap();
    items.push(json!({"if":{"properties":{"op":{"const":"machine.storage.take"}}},"then":{"properties":{"policy_version":{"const":1}}}}));
    items.push(json!({"if":{"properties":{"op":{"const":"machine.storage.configure"}}},"then":{"properties":{"expected_rev":{"type":"integer","minimum":0},"policy":{"type":"object","additionalProperties":false,"properties":{"enabled":{"type":"boolean"},"interval_minutes":{"type":"integer","minimum":1,"maximum":10080},"retention_days":{"type":"integer","minimum":0,"maximum":3650},"cache_retention_days":{"type":"integer","minimum":0,"maximum":3650},"quota_gib":{"type":"integer","minimum":0,"maximum":1048576},"min_free_gib":{"type":"integer","minimum":0,"maximum":1048576}}}}}}));
    items.push(json!({"if":{"properties":{"op":{"const":"machine.storage.pin"}}},"then":{"properties":{"workspace":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"},"pinned":{"type":"boolean"}}}}));
}
