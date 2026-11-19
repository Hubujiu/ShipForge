use super::*;
use crate::{domain, now};
use rusqlite::OptionalExtension;
use uuid::Uuid;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("
      CREATE TABLE IF NOT EXISTS sf_import_binding(org TEXT NOT NULL,portal TEXT NOT NULL,project TEXT NOT NULL,space TEXT NOT NULL,connector TEXT NOT NULL,PRIMARY KEY(org,portal,project));
      CREATE TABLE IF NOT EXISTS sf_import_generation(space TEXT NOT NULL,connector TEXT NOT NULL,generation INTEGER NOT NULL,PRIMARY KEY(space,connector));
      CREATE TABLE IF NOT EXISTS sf_import_batch(id TEXT PRIMARY KEY,space TEXT NOT NULL,connector TEXT NOT NULL,portal TEXT NOT NULL,project TEXT NOT NULL,config_rev INTEGER NOT NULL,checkpoint TEXT NOT NULL,state TEXT NOT NULL,rows TEXT NOT NULL,summary TEXT NOT NULL,actor TEXT NOT NULL,at INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS sf_import_history ON sf_import_batch(space,connector,at);
      CREATE TABLE IF NOT EXISTS sf_import_item(space TEXT NOT NULL,connector TEXT NOT NULL,external TEXT NOT NULL,task TEXT NOT NULL,birth INTEGER NOT NULL,owner_batch TEXT NOT NULL,base TEXT NOT NULL,modified INTEGER,PRIMARY KEY(space,connector,external));
    ").map_err(storage)
}

pub(super) fn has_ownership(c: &Context<'_>, id: &str) -> Result<bool> {
    c.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sf_import_item WHERE space=? AND connector=?)",
        params![c.space, id],
        |r| r.get(0),
    )
    .map_err(storage)
}

fn project_field(r: &Record) -> &'static str {
    match r.fields["provider"].as_str() {
        Some("tapd") => "tapd_workspace",
        Some("zentao" | "pingcode" | "linear" | "clickup" | "ones" | "teambition" | "asana") => {
            "remote_scope"
        }
        _ => "zoho_project",
    }
}

fn bound(c: &Context<'_>, r: &Record) -> Result<(String, String)> {
    let portal = match r.fields["provider"].as_str() {
        Some("tapd") => "tapd".into(),
        Some("zentao") => format!(
            "zentao|{}|{}",
            trackers::zentao_installation(required(&r.fields, "instance_url")?)?,
            required(&r.fields, "scope_kind")?
        ),
        Some("pingcode") => format!("pingcode|{}", required(&r.fields, "enterprise_id")?),
        Some("linear" | "clickup" | "ones" | "teambition" | "asana") => {
            cloud::namespace(&r.fields)?
        }
        _ => required(&r.fields, "portal_id")?.to_owned(),
    };
    let project = required(&r.fields, project_field(r))?.to_owned();
    let existing: Option<(String, String)> =
        c.db.query_row(
            "SELECT space,connector FROM sf_import_binding WHERE org=? AND portal=? AND project=?",
            params![c.org_id()?, portal, project],
            |x| Ok((x.get(0)?, x.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if existing.is_some_and(|(space, id)| space != c.space || id != r.id) {
        return Err(conflict(
            "this vendor project is already bound in the organization",
        ));
    }
    Ok((portal, project))
}

fn checkpoint(c: &Context<'_>, r: &Record) -> Result<String> {
    c.db.query_row(
        "SELECT CAST(generation AS TEXT) FROM sf_import_generation WHERE space=? AND connector=?",
        params![c.space, r.id],
        |row| row.get(0),
    )
    .optional()
    .map_err(storage)
    .map(|v| v.unwrap_or_else(|| "0".into()))
}

fn advance(c: &Context<'_>, r: &Record) -> Result<()> {
    c.db.execute("INSERT INTO sf_import_generation VALUES(?,?,1) ON CONFLICT(space,connector) DO UPDATE SET generation=generation+1",params![c.space,r.id]).map_err(storage)?;
    Ok(())
}

pub(super) fn binding(c: &Context<'_>, r: &Record, verify_scope: bool) -> Result<Value> {
    let since:Option<i64>=c.db.query_row("SELECT CASE WHEN COUNT(modified)=COUNT(*) THEN MAX(modified) END FROM sf_import_item WHERE space=? AND connector=?",params![c.space,r.id],|v|v.get(0)).map_err(storage)?;
    if verify_scope
        && (!cloud::supported(&r.fields)
            || r.fields["enterprise_id"]
                .as_str()
                .is_some_and(|v| !v.is_empty()))
        && r.fields[project_field(r)]
            .as_str()
            .is_some_and(|p| !p.is_empty())
    {
        bound(c, r)?;
    }
    let mut binding = json!({"organization":c.org_id()?,"provider":r.fields["provider"],"portal_id":r.fields["portal_id"],"tapd_auth":r.fields["tapd_auth"],"company_id":r.fields["company_id"],"project":r.fields[project_field(r)],"revision":r.rev,"checkpoint":checkpoint(c,r)?,"since":since.map(|ms|ms.saturating_sub(1)),"modified_after":since.and_then(|ms|chrono::DateTime::from_timestamp_millis(ms.saturating_sub(1000))).map(|d|d.format("%Y-%m-%d %H:%M:%S").to_string()),"instance_url":r.fields["instance_url"],"scope_kind":r.fields["scope_kind"],"enterprise_id":r.fields["enterprise_id"],"pingcode_auth":r.fields["pingcode_auth"],"sync_paused":r.fields["sync_paused"]==true,"cloud_auth":r.fields["cloud_auth"],"ones_url":r.fields["ones_url"]});
    if matches!(r.fields["provider"].as_str(), Some("linear" | "asana"))
        && signals::uses_project(&r.fields)
    {
        binding["signal_revision"] = json!(signals::latest(c, r)?);
    }
    if r.fields["provider"] == "teambition" {
        binding["connector_created"] = json!(r.created_at);
    }
    if r.fields["provider"] == "asana" {
        binding["vendor_write"] = json!(r.fields["vendor_write"] == true);
    }
    Ok(binding)
}

struct Mapping {
    task: String,
    birth: i64,
    base: Value,
    modified: Option<i64>,
    adopting: bool,
}

fn mapping(c: &Context<'_>, r: &Record, external: &str) -> Result<Option<Mapping>> {
    let row:Option<(String,i64,String,Option<i64>)>=c.db.query_row("SELECT task,birth,base,modified FROM sf_import_item WHERE space=? AND connector=? AND external=?",params![c.space,r.id,external],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?,v.get(3)?))).optional().map_err(storage)?;
    if let Some((task, birth, base, modified)) = row {
        return Ok(Some(Mapping {
            task,
            birth,
            base: serde_json::from_str(&base).map_err(storage)?,
            modified,
            adopting: false,
        }));
    }
    if !matches!(r.fields["provider"].as_str(), Some("linear" | "asana")) {
        return Ok(None);
    }
    let Some(issue) = external.strip_prefix("workitem:") else {
        return Ok(None);
    };
    let legacy:Option<(String,String,String)>=c.db.query_row(
        "SELECT o.task,o.base,(SELECT snapshot FROM event_log WHERE space=o.space AND kind='task' AND id=o.task AND action='create' ORDER BY seq LIMIT 1) FROM sf_connector_objects o WHERE o.space=? AND o.connector=? AND o.external=?",
        params![c.space,r.id,format!("{}:{issue}",required(&r.fields,"provider")?)],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(storage)?;
    let Some((task, base, initial)) = legacy else {
        return Ok(None);
    };
    let before: Record = serde_json::from_str(&initial).map_err(storage)?;
    let source: Value = serde_json::from_str(&base).map_err(storage)?;
    let mut base = before.fields;
    for key in ["title", "description", "assignees", "source_assignees"] {
        if let Some(value) = source.get(key) {
            base[key] = value.clone();
        }
    }
    if source["closed"] == true {
        base["status"] = json!("done");
    } else if base["status"] == "done" {
        base["status"] = json!("todo");
    }
    Ok(Some(Mapping {
        task,
        birth: before.rev,
        base,
        modified: None,
        adopting: true,
    }))
}

pub(super) fn owned(
    c: &Context<'_>,
    r: &Record,
    id: &str,
    birth: i64,
    external: &str,
) -> Result<Option<Record>> {
    let deleted:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM event_log WHERE space=? AND kind='task' AND id=? AND action='delete' AND rev>=?)",params![c.space,id,birth],|x|x.get(0)).map_err(storage)?;
    if deleted {
        return Ok(None);
    }
    let Some(task) = c
        .find("task", id)?
        .filter(|t| t.fields["connector"] == r.id)
    else {
        return Ok(None);
    };
    if task.fields["external_id"] == external {
        return Ok(Some(task));
    }
    if matches!(r.fields["provider"].as_str(), Some("linear" | "asana"))
        && let Some(issue) = external.strip_prefix("workitem:")
    {
        let legacy = format!("{}:{issue}", required(&r.fields, "provider")?);
        if task.fields["external_id"] == legacy {
            let original:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM sf_connector_objects WHERE space=? AND connector=? AND external=? AND task=?)",params![c.space,r.id,legacy,id],|row|row.get(0)).map_err(storage)?;
            if original {
                return Ok(Some(task));
            }
        }
    }
    Ok(None)
}

fn apply(c: &Context<'_>, r: &Record, batch: &str, row: &Value, write: bool) -> Result<Value> {
    let external = required(row, "external")?;
    let fields = &row["fields"];
    let modified = row["modified"].as_i64();
    let mut changes = fields.clone();
    let mut conflicts = vec![];
    let (task, action) = if let Some(original) = mapping(c, r, external)? {
        let Some(local) = owned(c, r, &original.task, original.birth, external)? else {
            return Ok(json!({"external":external,"action":"detached"}));
        };
        c.readable("task", &local.id)?;
        if row["keep_existing"] == true {
            return Ok(json!({"external":external,"action":"unchanged","task":local.id}));
        }
        if matches!((modified,original.modified),(Some(new),Some(old)) if new<old) {
            return Ok(json!({"external":external,"action":"unchanged","task":local.id}));
        }
        let patch = changes.as_object_mut().unwrap();
        patch.retain(|key, source| {
            let local_value = if key == "title" {
                json!(local.title)
            } else {
                local.fields[key].clone()
            };
            if original.base[key] == *source || local_value == *source {
                return false;
            }
            if local_value != original.base[key] {
                conflicts.push(key.clone());
                return false;
            }
            true
        });
        if original.adopting {
            patch.insert("external_provider".into(), r.fields["provider"].clone());
        }
        let action = if !conflicts.is_empty() {
            "conflict"
        } else if patch.is_empty() {
            "unchanged"
        } else {
            "updated"
        };
        if write {
            if original.adopting {
                c.db.execute("INSERT INTO sf_import_item(space,connector,external,task,birth,owner_batch,base,modified) VALUES(?,?,?,?,?,?,?,NULL)",
                    params![c.space,r.id,external,local.id,original.birth,format!("adopt:{batch}"),original.base.to_string()]).map_err(storage)?;
            }
            if !patch.is_empty() {
                let title = patch.remove("title").unwrap_or_else(|| json!(local.title));
                domain::write_import(
                    c,
                    &json!({"op":"update","kind":"task","id":local.id,"expected_rev":local.rev,"title":title,"fields":changes}),
                    true,
                )?;
            }
            // Preserve the source base of conflicting fields so the conflict stays visible on reimport.
            let mut next = fields.clone();
            for key in &conflicts {
                next[key] = original.base[key].clone();
            }
            c.db.execute("UPDATE sf_import_item SET base=?,modified=COALESCE(?,modified) WHERE space=? AND connector=? AND external=?",params![next.to_string(),modified,c.space,r.id,external]).map_err(storage)?;
        }
        (Some(local.id), action)
    } else if write {
        let title = changes.as_object_mut().unwrap().remove("title").unwrap();
        changes["type"] = json!(row["task_type"].as_str().unwrap_or("bug"));
        changes["connector"] = json!(r.id);
        changes["source"] = json!("integration");
        changes["external_id"] = json!(external);
        changes["external_provider"] = r.fields["provider"].clone();
        let created = domain::write_import(
            c,
            &json!({"op":"create","kind":"task","title":title,"fields":changes,"preserve_source_status":row["historical"]==true}),
            true,
        )?;
        let task = required(&created, "id")?.to_owned();
        c.db.execute("INSERT INTO sf_import_item(space,connector,external,task,birth,owner_batch,base,modified) VALUES(?,?,?,?,?,?,?,?)",params![c.space,r.id,external,task,created["rev"].as_i64(),batch,fields.to_string(),modified]).map_err(storage)?;
        (Some(task), "created")
    } else {
        (None, "created")
    };
    Ok(json!({"external":external,"task":task,"action":action,"conflicts":conflicts}))
}

fn summarize(
    c: &Context<'_>,
    r: &Record,
    batch: &str,
    rows: &[Value],
    write: bool,
) -> Result<Value> {
    let mut counts = json!({"created":0,"updated":0,"unchanged":0,"conflict":0,"detached":0});
    let mut details = Vec::new();
    for row in rows {
        let mut outcome = apply(c, r, batch, row, write)?;
        if let Some(warnings) = row["fields"]["source_warnings"]
            .as_array()
            .filter(|w| !w.is_empty())
        {
            outcome["warnings"] = json!(warnings);
        }
        let key = outcome["action"].as_str().unwrap();
        counts[key] = json!(counts[key].as_u64().unwrap() + 1);
        details.push(outcome);
    }
    Ok(json!({"batch":batch,"counts":counts,"items":details}))
}

pub(super) fn stage(c: &Context<'_>, r: &Record, q: &Value) -> Result<Value> {
    if q["poll"] == true && cloud::supported(&r.fields) && r.fields["sync_paused"] == true {
        return Ok(json!({"state":"paused","batch":null}));
    }
    let (portal, project) = bound(c, r)?;
    if q["project"] != project
        || q["binding_rev"].as_i64() != Some(r.rev)
        || q["checkpoint"] != checkpoint(c, r)?
    {
        return Err(conflict("project binding changed; fetch again"));
    }
    if q["issues"].to_string().len() > 7_000_000 {
        return Err(invalid("issue batch exceeds 7 MB"));
    }
    let rows = match r.fields["provider"].as_str() {
        Some("tapd") => tapd::normalize(c, r, q)?,
        Some("linear" | "clickup" | "ones" | "teambition" | "asana") => cloud::normalize(c, r, q)?,
        Some("zentao" | "pingcode") => trackers::normalize(c, r, q)?,
        _ => zoho::normalize(c, r, &q["issues"])?,
    };
    let batch = Uuid::new_v4().to_string();
    let mut summary = summarize(c, r, &batch, &rows, false)?;
    if q["poll"] == true
        && (cloud::supported(&r.fields)
            || matches!(r.fields["provider"].as_str(), Some("zentao" | "pingcode")))
    {
        let blocked = summary["counts"]["conflict"] != 0 || summary["counts"]["detached"] != 0;
        if blocked || (summary["counts"]["created"] == 0 && summary["counts"]["updated"] == 0) {
            summary["batch"] = Value::Null;
            summary["state"] = json!(if blocked { "needs_review" } else { "unchanged" });
            return Ok(summary);
        }
    }
    summary["state"] = json!("preview");
    c.db.execute("INSERT INTO sf_import_batch(id,space,connector,portal,project,config_rev,checkpoint,state,rows,summary,actor,at) VALUES(?,?,?,?,?,?,?,'preview',?,?,?,?)",params![batch,c.space,r.id,portal,project,r.rev,checkpoint(c,r)?,json!(rows).to_string(),summary.to_string(),c.actor,now()]).map_err(storage)?;
    Ok(summary)
}

pub(super) fn commit(c: &Context<'_>, r: &Record, id: &str) -> Result<Value> {
    let (portal, project) = bound(c, r)?;
    let record:Option<(String,String,String,i64,String,String,String)>=c.db.query_row("SELECT state,portal,project,config_rev,checkpoint,rows,summary FROM sf_import_batch WHERE id=? AND space=? AND connector=?",params![id,c.space,r.id],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?,v.get(3)?,v.get(4)?,v.get(5)?,v.get(6)?))).optional().map_err(storage)?;
    let (state, p, j, rev, previous, rows, summary) =
        record.ok_or("not_found: unknown import batch")?;
    if state == "completed" {
        return serde_json::from_str(&summary).map_err(storage);
    }
    if state != "preview"
        || p != portal
        || j != project
        || rev != r.rev
        || previous != checkpoint(c, r)?
    {
        return Err(conflict(
            "this preview is stale or closed; preview the project again",
        ));
    }
    c.db.execute("INSERT OR IGNORE INTO sf_import_binding(org,portal,project,space,connector) VALUES(?,?,?,?,?)",params![c.org_id()?,portal,project,c.space,r.id]).map_err(storage)?;
    let rows: Vec<Value> = serde_json::from_str(&rows).map_err(storage)?;
    let mut result = summarize(c, r, id, &rows, true)?;
    if cloud::supported(&r.fields) {
        result["relations"] = cloud::connect_relations(c, r)?;
    }
    result["state"] = json!("completed");
    finish(c, id, "completed", &result)?;
    advance(c, r)?;
    if !has_ownership(c, &r.id)? {
        release(c, r)?;
    }
    Ok(result)
}

fn finish(c: &Context<'_>, id: &str, state: &str, result: &Value) -> Result<()> {
    c.db.execute(
        "UPDATE sf_import_batch SET state=?,summary=?,rows='[]' WHERE id=?",
        params![state, result.to_string(), id],
    )
    .map_err(storage)?;
    Ok(())
}

fn release(c: &Context<'_>, r: &Record) -> Result<()> {
    c.db.execute(
        "DELETE FROM sf_import_binding WHERE space=? AND connector=?",
        params![c.space, r.id],
    )
    .map_err(storage)?;
    Ok(())
}

pub(super) fn rollback(c: &Context<'_>, r: &Record, id: &str) -> Result<Value> {
    let row: Option<(String, String)> =
        c.db.query_row(
            "SELECT state,summary FROM sf_import_batch WHERE space=? AND connector=? AND id=?",
            params![c.space, r.id, id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let (state, summary) = row.ok_or("not_found: unknown import batch")?;
    let mut result: Value = serde_json::from_str(&summary).map_err(storage)?;
    if matches!(state.as_str(), "rolled_back" | "cancelled") {
        return Ok(result);
    }
    let mut deleted = 0;
    let mut released_existing = 0;
    if state == "completed" {
        let mut stmt=c.db.prepare("SELECT task,birth,external FROM sf_import_item WHERE space=? AND connector=? AND owner_batch=?").map_err(storage)?;
        let items = stmt
            .query_map(params![c.space, r.id, id], |x| {
                Ok((
                    x.get::<_, String>(0)?,
                    x.get::<_, i64>(1)?,
                    x.get::<_, String>(2)?,
                ))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        for (task, birth, external) in items {
            if let Some(row) = owned(c, r, &task, birth, &external)? {
                domain::write(
                    c,
                    &json!({"op":"delete","kind":"task","id":task,"expected_rev":row.rev}),
                )?;
                deleted += 1;
            }
        }
        c.db.execute(
            "DELETE FROM sf_import_item WHERE space=? AND connector=? AND owner_batch=?",
            params![c.space, r.id, id],
        )
        .map_err(storage)?;
        if matches!(r.fields["provider"].as_str(), Some("linear" | "asana")) {
            released_existing =
                c.db.execute(
                    "DELETE FROM sf_import_item WHERE space=? AND connector=? AND owner_batch=?",
                    params![c.space, r.id, format!("adopt:{id}")],
                )
                .map_err(storage)?;
        }
        advance(c, r)?;
        if !has_ownership(c, &r.id)? {
            release(c, r)?;
        }
    }
    let state = if state == "preview" {
        "cancelled"
    } else {
        "rolled_back"
    };
    result["state"] = json!(state);
    result["deleted"] = json!(deleted);
    if released_existing > 0 {
        result["released_existing"] = json!(released_existing);
    }
    if (deleted > 0 || released_existing > 0) && cloud::supported(&r.fields) {
        cloud::sync_control(c, r, "pause")?;
        result["sync_paused"] = json!(true);
    }
    finish(c, id, state, &result)?;
    Ok(result)
}

pub(super) fn history(c: &Context<'_>, r: &Record) -> Result<Value> {
    let mut stmt=c.db.prepare("SELECT id,state,summary,actor,at,portal,project FROM sf_import_batch WHERE space=? AND connector=? ORDER BY rowid DESC LIMIT 200").map_err(storage)?;
    let rows = stmt
        .query_map(params![c.space, r.id], |v| {
            Ok((
                v.get::<_, String>(0)?,
                v.get::<_, String>(1)?,
                v.get::<_, String>(2)?,
                v.get::<_, String>(3)?,
                v.get::<_, i64>(4)?,
                v.get::<_, String>(5)?,
                v.get::<_, String>(6)?,
            ))
        })
        .map_err(storage)?;
    let mut history = Vec::new();
    for row in rows {
        let (id, state, raw, actor, at, portal, project) = row.map_err(storage)?;
        let summary: Value = serde_json::from_str(&raw).map_err(storage)?;
        history.push(json!({"batch":id,"state":state,"counts":summary["counts"],"deleted":summary["deleted"],"actor":actor,"at":at,"portal":portal,"project":project}));
    }
    Ok(json!({"items":history}))
}
