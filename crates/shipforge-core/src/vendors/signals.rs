use super::*;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_tracker_signal(
        seq INTEGER PRIMARY KEY AUTOINCREMENT, space TEXT NOT NULL, connector TEXT NOT NULL,
        event TEXT NOT NULL, digest BLOB NOT NULL, at INTEGER NOT NULL,
        UNIQUE(space, connector, event));
        CREATE INDEX IF NOT EXISTS sf_tracker_signal_latest ON sf_tracker_signal(space,connector,seq)")
        .map_err(storage)
}

pub(crate) fn uses_project(f: &Value) -> bool {
    cloud::supported(f)
        && ["enterprise_id", "remote_scope"]
            .iter()
            .all(|key| f[key].as_str().is_some_and(|s| !s.is_empty()))
}

pub(super) fn latest(c: &Context<'_>, r: &Record) -> Result<i64> {
    c.db.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM sf_tracker_signal WHERE space=? AND connector=?",
        params![c.space, r.id],
        |row| row.get(0),
    )
    .map_err(storage)
}

pub(crate) fn project_relay(
    c: &Context<'_>,
    r: &Record,
    p: &Value,
    delivery: &str,
    preview: bool,
) -> Result<Value> {
    let project = required(&r.fields, "remote_scope")?;
    let in_scope = if r.fields["provider"] == "asana" {
        let task = if p.get("events").is_some() {
            &p["task"]["data"]
        } else {
            &p["data"]
        };
        task["workspace"]["gid"] == r.fields["enterprise_id"]
            && task["memberships"]
                .as_array()
                .is_some_and(|ms| ms.iter().any(|m| m["project"]["gid"] == project))
    } else {
        p["organizationId"] == r.fields["enterprise_id"]
            && p["data"]["projectId"]
                .as_str()
                .or_else(|| p["data"]["project"]["id"].as_str())
                == Some(project)
    };
    if !in_scope {
        return Err(crate::forbidden(
            "event is outside the bound organization or project",
        ));
    }
    let mut result = json!({"action":"sync_requested","project":project,"event_id":delivery});
    if r.fields["sync_paused"] == true {
        result["action"] = json!("paused");
        return Ok(result);
    }
    if preview {
        return Ok(result);
    }
    let digest = Sha256::digest(p.to_string().as_bytes()).to_vec();
    let existing: Option<(i64, Vec<u8>)> =
        c.db.query_row(
            "SELECT seq,digest FROM sf_tracker_signal WHERE space=? AND connector=? AND event=?",
            params![c.space, r.id, delivery],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let seq = if let Some((seq, old)) = existing {
        if old != digest {
            return Err(conflict(
                "delivery ID was reused for a different tracker event",
            ));
        }
        seq
    } else {
        let at = crate::now();
        c.db.execute(
            "INSERT INTO sf_tracker_signal(space,connector,event,digest,at) VALUES(?,?,?,?,?)",
            params![c.space, r.id, delivery, digest, at],
        )
        .map_err(storage)?;
        let seq = c.db.last_insert_rowid();
        c.db.execute(
            "DELETE FROM sf_tracker_signal WHERE space=? AND connector=? AND at<?",
            params![c.space, r.id, at.saturating_sub(30 * 86400 * 1000)],
        )
        .map_err(storage)?;
        seq
    };
    result["signal_revision"] = json!(seq);
    Ok(result)
}
