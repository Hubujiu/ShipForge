use super::*;
use crate::{forbidden, invalid, storage};

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_execution_workspaces(space TEXT,machine TEXT,agent TEXT,kind TEXT,source TEXT,audience TEXT,identity TEXT,token TEXT,PRIMARY KEY(space,machine,agent,kind,source,audience,identity));
       CREATE TRIGGER IF NOT EXISTS sf_workspace_cleanup AFTER DELETE ON records WHEN OLD.kind IN ('task','chat','agent','machine') BEGIN DELETE FROM sf_execution_workspaces WHERE space=OLD.space AND ((kind=OLD.kind AND source=OLD.id) OR (OLD.kind='agent' AND agent=OLD.id) OR (OLD.kind='machine' AND machine=OLD.id));END;").map_err(storage)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_directory_bindings(space TEXT,machine TEXT,agent TEXT,kind TEXT,source TEXT,profile TEXT,mode TEXT,identity TEXT,PRIMARY KEY(space,machine,agent,kind,source));
       CREATE TRIGGER IF NOT EXISTS sf_directory_cleanup AFTER DELETE ON records WHEN OLD.kind IN ('task','chat','agent','machine') BEGIN
       DELETE FROM sf_directory_bindings WHERE space=OLD.space AND ((kind=OLD.kind AND source=OLD.id) OR (OLD.kind='agent' AND agent=OLD.id) OR (OLD.kind='machine' AND machine=OLD.id));END;").map_err(storage)?;
    scheduling::migrate(db)
}
pub(super) fn advertised(raw: &Value) -> Result<Value> {
    if raw.is_null() {
        return Ok(Value::Null);
    }
    let items = raw
        .as_array()
        .filter(|a| a.len() <= 128)
        .ok_or_else(|| invalid("invalid approved directory list"))?;
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    for item in items {
        let id = req(item, "id")?;
        crate::identifier(id)?;
        let key = req(item, "identity")?;
        if key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()) || !ids.insert(id) {
            return Err(invalid("invalid or duplicate directory identity"));
        }
        out.push(json!({"id":id,"identity":key,"direct":item["direct"].as_bool().unwrap_or(false),"git":item["git"].as_bool().unwrap_or(false),"available":item["available"].as_bool().unwrap_or(false)}));
    }
    Ok(json!(out))
}
pub(super) fn list(c: &Context<'_>, q: &Value) -> Result<Value> {
    let m = c.readable("machine", req(q, "id")?)?;
    if m.fields["disabled"] == true || m.fields["shared"] != true && m.fields["owner"] != c.actor {
        return Err(forbidden("machine is unavailable"));
    }
    let v=m.fields["directories"].as_array().into_iter().flatten().map(|d|json!({"id":d["id"],"git":d["git"],"direct":d["direct"],"available":d["available"]})).collect::<Vec<_>>();
    Ok(json!({"machine_id":m.id,"online":crate::machine_is_online(&m.fields),"directories":v}))
}
pub(super) fn validate_agent(c: &Context<'_>, f: &Value) -> Result<()> {
    if ["profiles", "machine_id", "directory_mode"]
        .iter()
        .all(|k| f[k].is_null())
    {
        return Ok(());
    }
    let inherited = if let Some(id) = f["parent_id"].as_str() {
        let origin = Context {
            space: f["parent_space"].as_str().unwrap_or(c.space),
            ..*c
        };
        crate::agents::effective(&origin, id)?
    } else {
        Value::Null
    };
    let setting = |key: &str| {
        f.get(key)
            .filter(|v| !v.is_null())
            .or_else(|| inherited.get(key))
    };
    let Some(raw) = setting("profiles") else {
        return Ok(());
    };
    let items = array_ids(raw)?;
    if items.is_empty() {
        return Ok(());
    }
    let direct = setting("directory_mode").and_then(Value::as_str) == Some("direct");
    if items.len() > 16
        || items.len() > 1 && !direct
        || items.iter().collect::<HashSet<_>>().len() != items.len()
    {
        return Err(invalid(
            "select one isolated profile or up to 16 different direct profiles",
        ));
    }
    let machine = c.readable(
        "machine",
        setting("machine_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("choose a machine for these directories"))?,
    )?;
    for name in items {
        let found = machine.fields["directories"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["id"] == name)
            .ok_or_else(|| invalid("select a directory approved on this machine"))?;
        if found["available"] != true
            || if direct {
                found["direct"] != true
            } else {
                found["git"] != true
            }
        {
            return Err(invalid("selected directory does not support this mode"));
        }
    }
    Ok(())
}
pub(super) enum Choice {
    Legacy,
    Selected {
        profile: String,
        mode: String,
        identity: String,
    },
    Blocked(&'static str),
}
impl Choice {
    pub(super) fn reason(&self) -> Option<&'static str> {
        if let Self::Blocked(r) = self {
            Some(r)
        } else {
            None
        }
    }
}
fn scope(r: &Record) -> Option<(&str, &str)> {
    r.fields["chat_id"]
        .as_str()
        .map(|id| ("chat", id))
        .or_else(|| r.fields["task_id"].as_str().map(|id| ("task", id)))
}
pub(super) fn select(
    c: &Context<'_>,
    r: &Record,
    m: &Record,
    runs: &[Record],
    busy: &[&str],
) -> Result<Choice> {
    let cfg = &r.fields["config"];
    let regs = &m.fields["directories"];
    if !regs.is_array() {
        return Ok(
            if cfg["profiles"]
                .as_array()
                .is_some_and(|ids| !ids.is_empty())
            {
                Choice::Blocked("directory_unavailable")
            } else {
                Choice::Legacy
            },
        );
    }
    if r.fields["crew_workspace"].is_object() {
        return Ok(Choice::Legacy);
    }
    let agent = cfg["agent_id"].as_str().unwrap_or("");
    let pinned = if let Some((kind, id)) = scope(r) {
        c.db.query_row("SELECT profile,mode,identity FROM sf_directory_bindings WHERE space=? AND machine=? AND agent=? AND kind=? AND source=?",params![c.space,m.id,agent,kind,id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?))).optional().map_err(storage)?
    } else {
        None
    };
    let supported = |profile: &str, mode: &str, d: &Value| {
        d["id"] == profile
            && d["available"] == true
            && if mode == "direct" {
                d["direct"] == true
            } else {
                d["git"] == true
            }
    };
    let registered = regs.as_array().unwrap();
    let mut mode = cfg["directory_mode"].as_str().unwrap_or("isolated");
    let mut names = array_ids(&cfg["profiles"])?;
    if names.is_empty() {
        if let Some(id) = cfg["profile"].as_str().filter(|v| !v.is_empty()) {
            names.push(id);
        } else if mode == "direct" || registered.iter().any(|d| d["id"] == "@workspace") {
            names.push("@workspace");
        }
    }
    if let Some((name, pinned_mode, identity)) = &pinned {
        if registered
            .iter()
            .any(|d| supported(name, pinned_mode, d) && d["identity"] == *identity)
        {
            names = vec![name.as_str()];
            mode = pinned_mode;
        } else if r.fields["chat_id"].is_string() {
            return Ok(Choice::Blocked("bound_directory_missing"));
        }
    }
    if names.is_empty() {
        return Ok(Choice::Legacy);
    }
    let mut available = false;
    for name in names {
        let Some(d) = registered.iter().find(|d| supported(name, mode, d)) else {
            continue;
        };
        available = true;
        let identity = req(d, "identity")?;
        if mode == "direct"
            && (busy.contains(&identity)
                || runs.iter().any(|run| {
                    run.fields["machine_id"] == m.id
                        && matches!(
                            run.fields["status"].as_str(),
                            Some("running" | "cancelling")
                        )
                        && run.fields["assigned_directory"]["mode"] == "direct"
                        && run.fields["assigned_directory"]["identity"] == identity
                }))
        {
            continue;
        }
        return Ok(Choice::Selected {
            profile: name.into(),
            mode: mode.into(),
            identity: identity.into(),
        });
    }
    Ok(Choice::Blocked(if available {
        "directory_busy"
    } else {
        "directory_unavailable"
    }))
}
pub(super) fn bind(
    c: &Context<'_>,
    r: &Record,
    m: &Record,
    choice: &Choice,
    f: &mut Value,
) -> Result<()> {
    if let Choice::Selected {
        profile,
        mode,
        identity,
    } = choice
    {
        f["assigned_directory"] = json!({"profile":profile,"mode":mode,"identity":identity});
        if let Some((kind, id)) = scope(r) {
            c.db.execute("INSERT INTO sf_directory_bindings(space,machine,agent,kind,source,profile,mode,identity) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(space,machine,agent,kind,source) DO UPDATE SET profile=excluded.profile,mode=excluded.mode,identity=excluded.identity",params![c.space,m.id,req(&f["config"],"agent_id")?,kind,id,profile,mode,identity]).map_err(storage)?;
        }
    }
    if m.fields["directories"].is_array()
        && f["crew_workspace"].is_null()
        && (f["assigned_directory"]["mode"] == "isolated"
            || (f["assigned_directory"].is_null() && f["config"]["directory_mode"] != "direct"))
        && let Some((kind, id)) = scope(r)
    {
        let resource = c.get(kind, id)?;
        let audience = if f["visibility"] == "private" || resource.fields["visibility"] == "private"
        {
            req(f, "requested_by")?
        } else {
            ""
        };
        let agent = req(&f["config"], "agent_id")?;
        let identity = f["assigned_directory"]["identity"].as_str().unwrap_or("");
        c.db.execute(
            "INSERT OR IGNORE INTO sf_execution_workspaces VALUES(?,?,?,?,?,?,?,?)",
            params![
                c.space,
                m.id,
                agent,
                kind,
                id,
                audience,
                identity,
                Uuid::new_v4().simple().to_string()
            ],
        )
        .map_err(storage)?;
        let token:String=c.db.query_row("SELECT token FROM sf_execution_workspaces WHERE space=? AND machine=? AND agent=? AND kind=? AND source=? AND audience=? AND identity=?",params![c.space,m.id,agent,kind,id,audience,identity],|row|row.get(0)).map_err(storage)?;
        f["isolated_workspace"] = json!({"kind":kind,"id":id,"token":token});
    }
    Ok(())
}
