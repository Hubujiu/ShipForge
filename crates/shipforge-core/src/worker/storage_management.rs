use super::*;
use crate::{conflict, forbidden, invalid, storage};
mod policy;
const OPS: &[&str] = &[
    "machine.storage.preview",
    "machine.storage.configure",
    "machine.storage.pin",
    "machine.storage.request",
    "machine.storage.take",
    "machine.storage.reserve",
    "machine.storage.finish",
];
pub(super) fn init(db: &Connection) -> Result<()> {
    policy::init(db)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_storage_inventory(space TEXT,machine TEXT,summary TEXT,PRIMARY KEY(space,machine));
    CREATE TABLE IF NOT EXISTS sf_storage_jobs(space TEXT,machine TEXT,id TEXT,command TEXT,state TEXT,lease TEXT,expires INTEGER,outcome TEXT,at INTEGER,PRIMARY KEY(space,id));CREATE UNIQUE INDEX IF NOT EXISTS sf_storage_single ON sf_storage_jobs(space,machine) WHERE state IN ('queued','running','committing');
    CREATE TRIGGER IF NOT EXISTS sf_storage_machine_removed AFTER DELETE ON records WHEN OLD.kind='machine' BEGIN DELETE FROM sf_storage_inventory WHERE space=OLD.space AND machine=OLD.id;DELETE FROM sf_storage_jobs WHERE space=OLD.space AND machine=OLD.id;END;").map_err(storage)
}
pub(super) fn reserved(c: &Context<'_>, machine: &str) -> Result<bool> {
    c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_storage_jobs WHERE space=? AND machine=? AND state='committing' AND expires>?)",params![c.space,machine,now()],|r|r.get(0)).map_err(storage)
}
fn inventory(c: &Context<'_>, m: &str) -> Result<Value> {
    let data: Option<String> =
        c.db.query_row(
            "SELECT summary FROM sf_storage_inventory WHERE space=? AND machine=?",
            params![c.space, m],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    data.map(|v| serde_json::from_str(&v).map_err(storage))
        .unwrap_or_else(|| Ok(json!({"entries":[],"scanned_at":null})))
}
fn key(s: &Value) -> bool {
    s.as_str()
        .is_some_and(|k| k.len() == 64 && k.bytes().all(|c| c.is_ascii_hexdigit()))
}
fn cleaned(q: &Value) -> Result<Value> {
    let items = q["entries"]
        .as_array()
        .filter(|a| a.len() <= 2048)
        .ok_or_else(|| invalid("invalid storage inventory"))?;
    let mut entries = Vec::new();
    let mut keys = HashSet::new();
    for item in items {
        if !key(&item["id"])
            || !key(&item["snapshot"])
            || !keys.insert(item["id"].as_str().unwrap())
            || !matches!(
                item["category"].as_str(),
                Some("run" | "task" | "crew" | "cache")
            )
        {
            return Err(invalid("invalid storage entry"));
        }
        let runs = array_ids(&item["runs"])?;
        if runs.len() > 256 {
            return Err(invalid("too many workspace runs"));
        }
        let reason = item["protection"].as_str().filter(|s| {
            matches!(
                *s,
                "active"
                    | "pending"
                    | "bound_repository"
                    | "unverified"
                    | "links"
                    | "git_locked"
                    | "unreadable"
                    | "recovery"
                    | "attached_worktree"
                    | "local_changes"
                    | "cache_in_use"
            )
        });
        let protection = if item["protection"].is_null() {
            None
        } else {
            Some(reason.unwrap_or("unverified"))
        };
        entries.push(json!({"id":item["id"],"snapshot":item["snapshot"],"category":item["category"],"bytes":item["bytes"].as_u64().unwrap_or(0),"files":item["files"].as_u64().unwrap_or(0),"runs":runs,"protection":protection,"last_used_at":item["last_used_at"].as_i64(),"safe_for_schedule":item["safe_for_schedule"]==true}));
    }
    Ok(
        json!({"entries":entries,"scanned_at":now(),"total_bytes":q["total_bytes"].as_u64().unwrap_or(0),"other_bytes":q["other_bytes"].as_u64().unwrap_or(0),"pending_reclaim_bytes":q["pending_reclaim_bytes"].as_u64().unwrap_or(0),"omitted":q["omitted"].as_u64().unwrap_or(0),"available_bytes":q["available_bytes"].as_u64(),"local_hour":q["local_hour"].as_u64().filter(|h|*h<24)}),
    )
}
fn condition(
    c: &Context<'_>,
    machine: &str,
    item: &Value,
    runs: &[Record],
) -> Result<(Option<&'static str>, Option<String>)> {
    if !item["protection"].is_null() {
        return Ok((None, None));
    }
    if policy::pinned(c, machine, item)? {
        return Ok((Some("pinned"), None));
    }
    if item["category"] == "cache" {
        for run in runs.iter().filter(|r| {
            r.fields["machine_id"] == machine || r.fields["config"]["machine_id"] == machine
        }) {
            if matches!(
                run.fields["status"].as_str(),
                Some("running" | "cancelling" | "queued")
            ) {
                return Ok((Some("active"), None));
            }
            if policy::outbox(c, run)? {
                return Ok((Some("pending"), None));
            }
        }
        return Ok((None, None));
    }
    let ids = array_ids(&item["runs"])?;
    if ids.is_empty() {
        return Ok((Some("unverified"), None));
    }
    let mut title = None;
    for id in ids {
        let Some(run) = runs.iter().find(|r| r.id == id) else {
            return Ok((Some("unverified"), None));
        };
        let f = &run.fields;
        if f["machine_id"] != machine || c.readable("run", id).is_err() {
            return Ok((Some("private_or_unknown"), None));
        }
        if f["pinned"] == true {
            return Ok((Some("pinned"), None));
        }
        if f["chat_id"].is_string() {
            return Ok((Some("conversation"), None));
        }
        if policy::outbox(c, run)? {
            return Ok((Some("pending"), None));
        }
        if f["status"] != "done" {
            return Ok((
                Some(
                    if matches!(f["status"].as_str(), Some("running" | "cancelling")) {
                        "active"
                    } else {
                        "recovery"
                    },
                ),
                None,
            ));
        }
        let leased:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_run_leases WHERE space=? AND run=? AND response_hash IS NULL AND expires>?)",params![c.space,id,now()],|r|r.get(0)).map_err(storage)?;
        if leased {
            return Ok((Some("active"), None));
        }
        if let Some(task) = f["task_id"].as_str() {
            let Ok(task) = c.readable("task", task) else {
                return Ok((Some("private_or_unknown"), None));
            };
            if !matches!(
                task.fields["status"].as_str(),
                Some("done" | "cancelled" | "duplicate")
            ) {
                return Ok((Some("open_task"), Some(task.title)));
            }
            if runs.iter().any(|r| {
                r.fields["task_id"] == task.id
                    && !matches!(r.fields["status"].as_str(), Some("done" | "cancelled"))
            }) {
                return Ok((Some("recovery"), Some(task.title)));
            }
            title = Some(task.title);
        } else {
            title = Some(run.title.clone());
        }
    }
    Ok((None, title))
}
fn check<'a>(c: &Context<'_>, machine: &str, summary: &'a Value, q: &Value) -> Result<&'a Value> {
    let item = summary["entries"]
        .as_array()
        .and_then(|xs| xs.iter().find(|i| i["id"] == q["workspace"]))
        .ok_or_else(|| conflict("workspace inventory changed; scan again"))?;
    if item["snapshot"] != q["expected_snapshot"]
        || !item["protection"].is_null()
        || condition(c, machine, item, &c.records("run")?)?.0.is_some()
    {
        return Err(conflict(
            "workspace changed, is active, or may be needed for recovery",
        ));
    }
    Ok(item)
}
fn history(c: &Context<'_>, m: &str) -> Result<Vec<Value>> {
    let mut sql=c.db.prepare("SELECT id,command,state,outcome FROM sf_storage_jobs WHERE space=? AND machine=? ORDER BY at DESC,id DESC LIMIT 20").map_err(storage)?;
    let result = sql
        .query_map(params![c.space, m], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(storage)?;
    result.map(|row|{let(id,body,state,outcome)=row.map_err(storage)?;let body:Value=serde_json::from_str(&body).map_err(storage)?;Ok(json!({"id":id,"action":body["action"],"workspace":body["workspace"],"state":state,"outcome":outcome,"scheduled":body["cycle"].is_string()}))}).collect()
}
fn historical(c: &Context<'_>, machine: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for r in c.records("run")? {
        if r.fields["machine_id"] == machine {
            for a in r.fields["artifacts"].as_array().into_iter().flatten() {
                if a["type"] == "workspace"
                    && let Some(path) = a["location"].as_str()
                {
                    out.push(json!({"run":r.id,"location":path}));
                }
            }
        }
    }
    Ok(out)
}
pub(super) fn dispatch(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if matches!(
        op,
        "machine.storage.preview"
            | "machine.storage.request"
            | "machine.storage.configure"
            | "machine.storage.pin"
    ) {
        let m = c.readable("machine", req(q, "id")?)?;
        if m.fields["owner"] != c.actor {
            return Err(forbidden("only the machine owner manages local storage"));
        }
        let mut listing = inventory(c, &m.id)?;
        if op == "machine.storage.configure" {
            return policy::configure(c, &m, q);
        }
        if op == "machine.storage.pin" {
            return policy::pin(c, &m, q, &listing);
        }
        if op == "machine.storage.preview" {
            policy::decorate(c, &m, &mut listing)?;
            let runs = c.records("run")?;
            for v in listing["entries"].as_array_mut().into_iter().flatten() {
                v["pinned"] = json!(policy::pinned(c, &m.id, v)?);
                let (protection, title) = condition(c, &m.id, v, &runs)?;
                if v["protection"].is_null() {
                    v["protection"] = json!(protection);
                }
                v["title"] = json!(title);
                v.as_object_mut().unwrap().remove("runs");
            }
            listing["jobs"] = json!(history(c, &m.id)?);
            listing["machine"] = json!(m.id);
            listing["online"] = json!(machine_is_online_at(&m.fields, now()));
            return Ok(listing);
        }
        let action = req(q, "action")?;
        let id = q["request_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        crate::identifier(&id)?;
        let body = match action {
            "scan" => json!({"action":"scan"}),
            "clean"
                if q["confirm"] == true && key(&q["workspace"]) && key(&q["expected_snapshot"]) =>
            {
                json!({"action":"clean","workspace":q["workspace"],"expected_snapshot":q["expected_snapshot"]})
            }
            _ => {
                return Err(invalid(
                    "select one workspace and explicitly confirm its expected snapshot",
                ));
            }
        };
        let serialized = body.to_string();
        let prior: Option<(String, String)> =
            c.db.query_row(
                "SELECT machine,command FROM sf_storage_jobs WHERE space=? AND id=?",
                params![c.space, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((m_id, prev)) = prior {
            return if m_id == m.id && prev == serialized {
                Ok(json!({"id":id,"duplicate":true}))
            } else {
                Err(conflict("request ID already used"))
            };
        }
        if !machine_is_online_at(&m.fields, now()) {
            return Err(conflict("machine must be online to manage storage"));
        }
        if action == "clean" {
            check(c, &m.id, &listing, &body)?;
        }
        c.db.execute("UPDATE sf_storage_jobs SET state='failed',outcome='rescan_required' WHERE space=? AND machine=? AND state='committing' AND expires<=?",params![c.space,m.id,now()]).map_err(storage)?;
        if c.db.execute("INSERT OR IGNORE INTO sf_storage_jobs(space,machine,id,command,state,at) VALUES(?,?,?,?,'queued',?)",params![c.space,m.id,id,serialized,now()]).map_err(storage)?==0{return Err(conflict("another storage operation is pending"));}
        return Ok(json!({"id":id,"state":"queued"}));
    }
    let m = authenticate(c, q)?;
    if op == "machine.storage.take" {
        policy::start_due(c, &m, q["policy_version"].as_u64() == Some(1))?;
        let job:Option<(String,String)>=c.db.query_row("SELECT id,command FROM sf_storage_jobs WHERE space=? AND machine=? AND (state='queued' OR (state='running' AND expires<=?)) AND (json_extract(command,'$.cycle') IS NULL OR ?) ORDER BY at LIMIT 1",params![c.space,m.id,now(),q["policy_version"].as_u64()==Some(1)],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
        let Some((id, body)) = job else {
            return Ok(Value::Null);
        };
        let lease = token();
        c.db.execute("UPDATE sf_storage_jobs SET state='running',lease=?,expires=? WHERE space=? AND machine=? AND id=?",params![digest(&lease),now()+1_200_000,c.space,m.id,id]).map_err(storage)?;
        return Ok(
            json!({"id":id,"lease":lease,"command":serde_json::from_str::<Value>(&body).map_err(storage)?,"historical":historical(c,&m.id)?}),
        );
    }
    let id = req(q, "id")?;
    let lease = digest(req(q, "lease")?);
    let job:Option<(String,String)>=c.db.query_row("SELECT state,command FROM sf_storage_jobs WHERE space=? AND machine=? AND id=? AND lease=? AND expires>?",params![c.space,m.id,id,lease,now()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
    let (state, body) = job.ok_or_else(|| conflict("storage lease expired"))?;
    let body: Value = serde_json::from_str(&body).map_err(storage)?;
    if op == "machine.storage.reserve" {
        if state != "running" || body["action"] != "clean" {
            return Err(conflict("storage request cannot commit"));
        }
        let owner = Context {
            actor: req(&m.fields, "owner")?,
            ..*c
        };
        let summary = inventory(c, &m.id)?;
        let item = check(&owner, &m.id, &summary, &body)?;
        policy::reserve(&owner, &m, &body, &summary, item)?;
        c.db.execute(
            "UPDATE sf_storage_jobs SET state='committing',expires=? WHERE space=? AND id=?",
            params![now() + 120_000, c.space, id],
        )
        .map_err(storage)?;
        return Ok(json!({"reserved":true}));
    }
    if !matches!(state.as_str(), "running" | "committing") {
        return Err(conflict("storage job already finished"));
    }
    let outcome = req(q, "outcome")?;
    if !matches!(
        outcome,
        "success" | "reclaim_pending" | "conflict" | "rescan_required" | "failed"
    ) {
        return Err(invalid("unexpected storage outcome"));
    }
    if matches!(outcome, "success" | "reclaim_pending")
        && body["action"] == "clean"
        && state != "committing"
    {
        return Err(conflict("storage deletion was not reserved"));
    }
    if q["inventory"].is_object() {
        let v = cleaned(&q["inventory"])?;
        c.db.execute("INSERT INTO sf_storage_inventory VALUES(?,?,?) ON CONFLICT(space,machine) DO UPDATE SET summary=excluded.summary",params![c.space,m.id,v.to_string()]).map_err(storage)?;
    }
    c.db.execute(
        "UPDATE sf_storage_jobs SET state=?,outcome=? WHERE space=? AND id=?",
        params![
            if outcome == "success" {
                "done"
            } else {
                "failed"
            },
            outcome,
            c.space,
            id
        ],
    )
    .map_err(storage)?;
    policy::finished(c, &m, &body, q["inventory"].is_object())?;
    Ok(json!({"id":id,"outcome":outcome}))
}
pub(super) fn schema(s: &mut Value) {
    policy::schema(s);
    for op in OPS {
        let keys = match *op {
            "machine.storage.preview" => vec!["id"],
            "machine.storage.configure" => vec!["id", "expected_rev", "policy"],
            "machine.storage.pin" => vec!["id", "workspace", "pinned"],
            "machine.storage.request" => vec!["id", "action"],
            "machine.storage.take" => vec!["machine_id", "machine_key"],
            "machine.storage.reserve" => vec!["machine_id", "machine_key", "id", "lease"],
            _ => vec!["machine_id", "machine_key", "id", "lease", "outcome"],
        };
        s["allOf"]
            .as_array_mut()
            .unwrap()
            .push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":keys}}));
    }
    let rules = s["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"const":"machine.storage.request"}}},"then":{"properties":{"action":{"enum":["scan","clean"]},"request_id":{"type":"string","minLength":1,"maxLength":160}}}}));
    rules.push(json!({"if":{"required":["action"],"properties":{"op":{"const":"machine.storage.request"},"action":{"const":"clean"}}},"then":{"required":["workspace","expected_snapshot","confirm"],"properties":{"confirm":{"const":true},"workspace":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"},"expected_snapshot":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"machine.storage.finish"}}},"then":{"properties":{"outcome":{"enum":["success","reclaim_pending","conflict","rescan_required","failed"]}}}}));
}
