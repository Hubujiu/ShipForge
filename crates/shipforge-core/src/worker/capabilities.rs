use super::*;
use crate::{conflict, forbidden, invalid, storage};

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_runtime_caps(space TEXT,machine TEXT,name TEXT,body TEXT,PRIMARY KEY(space,machine,name));
    CREATE TABLE IF NOT EXISTS sf_runtime_jobs(space TEXT,machine TEXT,id TEXT,name TEXT,action TEXT,actor TEXT,state TEXT,lease TEXT,expires INTEGER,result TEXT,created INTEGER,PRIMARY KEY(space,id));CREATE UNIQUE INDEX IF NOT EXISTS sf_one_runtime_action ON sf_runtime_jobs(space,machine,name) WHERE state IN ('queued','running');
    CREATE TRIGGER IF NOT EXISTS sf_runtime_prune AFTER DELETE ON records WHEN OLD.kind='machine' BEGIN DELETE FROM sf_runtime_jobs WHERE space=OLD.space AND machine=OLD.id;DELETE FROM sf_runtime_caps WHERE space=OLD.space AND machine=OLD.id;END;").map_err(storage)
}
const STATES: &[&str] = &["available", "not_detected", "detection_failed"];
fn summary(input: &Value) -> Result<Value> {
    let name = req(input, "name")?;
    crate::identifier(name)?;
    if !input["status"]
        .as_str()
        .is_some_and(|s| STATES.contains(&s))
    {
        return Err(invalid("invalid runtime discovery status"));
    }
    let version = input["version"].as_str().filter(|s| {
        let mut parts = s.split('.');
        let n = parts.clone().count();
        (2..=4).contains(&n)
            && parts.all(|p| !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_digit()))
    });
    Ok(
        json!({"launch":launch_summary(&input["launch"])?,"name":name,"status":input["status"],"version":version,"configured":input["configured"]==true,"installable":input["installable"]==true,"upgradable":input["upgradable"]==true,"testable":input["testable"]==true,"kind":if input["kind"]=="environment"{"environment"}else{"runtime"},"checked_at":now()}),
    )
}
fn capabilities(c: &Context<'_>, machine: &str) -> Result<Vec<Value>> {
    let mut stmt =
        c.db.prepare("SELECT body FROM sf_runtime_caps WHERE space=? AND machine=? ORDER BY name")
            .map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, machine], |r| r.get::<_, String>(0))
        .map_err(storage)?;
    rows.map(|r| serde_json::from_str(&r.map_err(storage)?).map_err(storage))
        .collect()
}
fn launch_summary(v: &Value) -> Result<Value> {
    if v.is_null() {
        return Ok(Value::Null);
    }
    let strings = |list: &Value| -> Result<Vec<String>> {
        array_ids(list)?
            .into_iter()
            .map(|s| {
                crate::identifier(s)?;
                Ok(s.to_owned())
            })
            .collect()
    };
    let mut result = json!({});
    for key in ["models", "argument_choices"] {
        let mut choices = serde_json::Map::new();
        if let Some(values) = v[key].as_object() {
            if values.len() > 128 {
                return Err(invalid("too many launch choices"));
            }
            for (k, items) in values {
                crate::identifier(k)?;
                choices.insert(k.clone(), json!(strings(items)?));
            }
        }
        result[key] = Value::Object(choices);
    }
    for key in ["default_model", "fixed_model"] {
        if let Some(name) = v[key].as_str() {
            crate::identifier(name)?;
            result[key] = json!(name);
        }
    }
    result["environment_names"] = json!(strings(&v["environment_names"])?);
    Ok(result)
}
pub(super) fn agent_options(c: &Context<'_>, q: &Value) -> Result<Value> {
    let a = c.readable("agent", req(q, "id")?)?;
    let config = crate::agents::effective(c, &a.id)?;
    let runtime = config["runtime"].as_str().unwrap_or("local-tool");
    let mut machines = Vec::new();
    for m in c.records("machine")? {
        if !c.visible(&m)?
            || config["machine_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty() && id != m.id)
        {
            continue;
        }
        if let Some(item) = capabilities(c, &m.id)?
            .into_iter()
            .find(|v| v["name"] == runtime && v["launch"].is_object())
        {
            machines.push(json!({"id":m.id,"title":m.title,"launch":item["launch"]}));
        }
    }
    Ok(
        json!({"id":a.id,"rev":a.rev,"runtime":runtime,"model":config["model"],"reasoning_effort":config["reasoning_effort"],"machines":machines,"can_manage":crate::agents::require_manage(c,&a).is_ok()}),
    )
}
fn jobs(c: &Context<'_>, machine: &str) -> Result<Vec<Value>> {
    let mut stmt=c.db.prepare("SELECT id,name,action,state,result,created FROM sf_runtime_jobs WHERE space=? AND machine=? ORDER BY created DESC,id DESC LIMIT 100").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, machine], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .map_err(storage)?;
    rows.map(|r|{let(id,name,action,state,result,created)=r.map_err(storage)?;Ok(json!({"id":id,"name":name,"action":action,"state":state,"result":result.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(storage)?,"at":created}))}).collect()
}
fn visible(c: &Context<'_>, m: &Record) -> Result<Value> {
    let all = jobs(c, &m.id)?;
    let online = machine_is_online_at(&m.fields, now());
    let mut caps = capabilities(c, &m.id)?;
    for cap in &mut caps {
        let base = cap["status"].clone();
        cap["detected_status"] = base.clone();
        if let Some(job) = all.iter().find(|j| {
            j["name"] == cap["name"] && matches!(j["state"].as_str(), Some("queued" | "running"))
        }) {
            cap["operation"] = job.clone();
            cap["status"] = json!(if job["state"] == "queued" {
                "queued"
            } else {
                match job["action"].as_str() {
                    Some("install") => "installing",
                    Some("upgrade") => "upgrading",
                    Some("test") => "testing",
                    _ => "checking",
                }
            });
        } else if !online && base == "available" {
            cap["status"] = json!("offline");
        }
    }
    Ok(
        json!({"machine":m.id,"online":online,"runtimes":caps,"recent":all,"can_manage":m.fields["owner"]==c.actor&&c.require_edit().is_ok()}),
    )
}
pub(super) fn dispatch(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if matches!(op, "machine.runtimes" | "machine.runtime.request") {
        let m = c.readable("machine", req(q, "id")?)?;
        if op == "machine.runtimes" {
            return visible(c, &m);
        }
        if m.fields["owner"] != c.actor {
            return Err(forbidden(
                "only the machine owner manages its runtime tools",
            ));
        }
        let name = req(q, "runtime")?;
        let action = req(q, "action")?;
        let id = q["request_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        crate::identifier(&id)?;
        let prior: Option<(String, String, String)> =
            c.db.query_row(
                "SELECT machine,name,action FROM sf_runtime_jobs WHERE space=? AND id=?",
                params![c.space, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((machine, rt, act)) = prior {
            if machine == m.id && rt == name && act == action {
                return Ok(json!({"id":id,"duplicate":true}));
            }
            return Err(conflict(
                "request ID already refers to a different runtime action",
            ));
        }
        c.require_edit()?;
        if m.fields["disabled"] == true || !machine_is_online_at(&m.fields, now()) {
            return Err(conflict("machine must be online for runtime operations"));
        }
        let cap = capabilities(c, &m.id)?
            .into_iter()
            .find(|v| v["name"] == name)
            .ok_or_else(|| invalid("runtime was not advertised by this machine"))?;
        if !matches!(action, "rescan" | "install" | "upgrade" | "test")
            || action == "install" && cap["installable"] != true
            || action == "upgrade" && cap["upgradable"] != true
            || action == "test" && cap["testable"] != true
        {
            return Err(invalid("action unavailable for this runtime"));
        }
        if action == "install" && cap["status"] == "available"
            || matches!(action, "upgrade" | "test") && cap["status"] != "available"
        {
            return Err(conflict("runtime installation state has changed"));
        }
        let count=c.db.execute("INSERT OR IGNORE INTO sf_runtime_jobs(space,machine,id,name,action,actor,state,created) VALUES(?,?,?,?,?,?,'queued',?)",params![c.space,m.id,id,name,action,c.actor,now()]).map_err(storage)?;
        if count == 0 {
            return Err(conflict(
                "another action for this runtime is already in progress",
            ));
        }
        return Ok(json!({"id":id,"state":"queued","name":name,"action":action}));
    }
    let m = authenticate(c, q)?;
    match op {
        "machine.runtime.publish" => {
            let list = q["capabilities"]
                .as_array()
                .filter(|l| l.len() <= 128)
                .ok_or_else(|| invalid("invalid runtime inventory"))?;
            let mut seen = HashSet::new();
            c.db.execute(
                "DELETE FROM sf_runtime_caps WHERE space=? AND machine=?",
                params![c.space, m.id],
            )
            .map_err(storage)?;
            for source in list {
                let item = summary(source)?;
                let name = req(&item, "name")?;
                if !seen.insert(name.to_owned()) {
                    return Err(invalid("duplicate runtime name"));
                }
                c.db.execute(
                    "INSERT INTO sf_runtime_caps VALUES(?,?,?,?)",
                    params![c.space, m.id, name, item.to_string()],
                )
                .map_err(storage)?;
            }
            Ok(json!({"count":seen.len()}))
        }
        "machine.runtime.jobs" => {
            let at = now();
            let task:Option<(String,String,String)>=c.db.query_row("SELECT id,name,action FROM sf_runtime_jobs WHERE space=? AND machine=? AND (state='queued' OR (state='running' AND expires<=?)) ORDER BY created,id LIMIT 1",params![c.space,m.id,at],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
            let Some((id, name, action)) = task else {
                return Ok(Value::Null);
            };
            let lease = token();
            c.db.execute("UPDATE sf_runtime_jobs SET state='running',lease=?,expires=? WHERE space=? AND machine=? AND id=?",params![digest(&lease),at+1_200_000,c.space,m.id,id]).map_err(storage)?;
            Ok(json!({"id":id,"runtime":name,"action":action,"lease":lease}))
        }
        "machine.runtime.finish" => {
            let id = req(q, "id")?;
            let lease = digest(req(q, "lease")?);
            let code = req(q, "outcome")?;
            if !matches!(
                code,
                "success"
                    | "timeout"
                    | "failed"
                    | "login_required"
                    | "manager_unavailable"
                    | "not_detected"
                    | "unavailable"
                    | "unexpected_reply"
            ) {
                return Err(invalid("unrecognized runtime outcome"));
            }
            let state = if code == "success" { "done" } else { "failed" };
            let result = json!({"outcome":code}).to_string();
            let n=c.db.execute("UPDATE sf_runtime_jobs SET state=?,result=? WHERE space=? AND machine=? AND id=? AND lease=? AND ((state='running' AND expires>?) OR (state=? AND result=?))",params![state,result,c.space,m.id,id,lease,now(),state,result]).map_err(storage)?;
            if n == 0 {
                return Err(conflict("stale runtime lease"));
            }
            Ok(json!({"id":id,"state":state}))
        }
        _ => Err(invalid("unknown runtime operation")),
    }
}
pub(super) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    for (ops, required) in [
        (vec!["machine.runtimes"], vec!["id"]),
        (
            vec!["machine.runtime.request"],
            vec!["id", "runtime", "action"],
        ),
        (
            vec!["machine.runtime.jobs"],
            vec!["machine_id", "machine_key"],
        ),
        (
            vec!["machine.runtime.publish"],
            vec!["machine_id", "machine_key", "capabilities"],
        ),
        (
            vec!["machine.runtime.finish"],
            vec!["machine_id", "machine_key", "id", "lease", "outcome"],
        ),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"enum":ops}}},"then":{"required":required}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"const":"machine.runtime.request"}}},"then":{"properties":{"action":{"enum":["rescan","install","upgrade","test"]},"runtime":{"type":"string","minLength":1,"maxLength":160}}}}));
}
