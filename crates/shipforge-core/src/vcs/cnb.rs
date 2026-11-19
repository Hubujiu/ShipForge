use super::*;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_cnb_notes(id TEXT PRIMARY KEY,space TEXT NOT NULL,connector TEXT NOT NULL,repository TEXT NOT NULL,object TEXT NOT NULL,task TEXT NOT NULL,state TEXT NOT NULL,lease TEXT,until INTEGER,at INTEGER NOT NULL,remote TEXT,UNIQUE(space,connector,repository,object,task));").map_err(storage)
}
pub(super) fn activities(r: &Record, p: &Value) -> Result<Vec<Activity>> {
    let repo = required(&r.fields, "repository")?;
    if p["repository"] != repo || p["repo_id"] != r.fields["cnb_repo_id"] {
        return Err(crate::forbidden(
            "CNB event belongs to a different paired repository",
        ));
    }
    if p["kind"] == "ignored" {
        return Ok(vec![]);
    }
    let key = required(p, "key")?;
    let (kind, path) = match p["kind"].as_str() {
        Some("commit")
            if matches!(key.len(), 40 | 64) && key.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            ("commit", "commit")
        }
        Some("issue") if key.parse::<u64>().is_ok_and(|n| n > 0) => ("issue", "issues"),
        Some("pull_request") if key.parse::<u64>().is_ok_and(|n| n > 0) => {
            ("pull_request", "pulls")
        }
        _ => return Err(invalid("unsupported CNB resource")),
    };
    let event = required(p, "event")?;
    let title = required(p, "title")?;
    let object = format!("https://cnb.cool/{repo}/-/{path}/{key}");
    let text = format!(
        "{}\n{}\n{}\n{}",
        title,
        p["body"].as_str().unwrap_or(""),
        p["branch"].as_str().unwrap_or(""),
        p["comment"].as_str().unwrap_or("")
    );
    let activity = if kind == "commit" {
        "commit".into()
    } else {
        crate::worker::digest(
            &json!([
                event,
                p["revision"],
                p["comment_id"],
                title,
                p["body"],
                p["branch"]
            ])
            .to_string(),
        )
    };
    Ok(vec![Activity {
        object: object.clone(),
        activity,
        kind,
        action: event.into(),
        title: title.chars().take(220).collect(),
        url: object,
        references: numbers(&text),
    }])
}

pub(super) fn queue(c: &Context<'_>, r: &Record, p: &Value, out: &Value) -> Result<()> {
    if r.fields["cnb_backlinks"] != true
        || !matches!(p["kind"].as_str(), Some("issue" | "pull_request"))
    {
        return Ok(());
    }
    let object = format!(
        "{}/{}",
        if p["kind"] == "issue" {
            "issues"
        } else {
            "pulls"
        },
        required(p, "key")?
    );
    for link in out["links"].as_array().into_iter().flatten() {
        c.db.execute("INSERT OR IGNORE INTO sf_cnb_notes(id,space,connector,repository,object,task,state,at) VALUES(?,?,?,?,?,?,'pending',?)",params![Uuid::new_v4().to_string(),c.space,r.id,r.fields["repository"].as_str(),object,required(link,"task")?,crate::now()]).map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn command(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    c.require_admin()?;
    let r = c.readable("connector", required(q, "id")?)?;
    if r.fields["provider"] != "cnb" {
        return Err(invalid("select a CNB connector"));
    }
    let repo = r.fields["repository"].as_str().unwrap_or("");
    if op == "cnb.internal.history" {
        let mut s=c.db.prepare("SELECT id,object,state,at,remote FROM sf_cnb_notes WHERE space=? AND connector=? AND repository=? ORDER BY rowid DESC LIMIT 100").map_err(storage)?;
        let rows=s.query_map(params![c.space,r.id,repo],|row|Ok(json!({"delivery":row.get::<_,String>(0)?,"object":row.get::<_,String>(1)?,"state":row.get::<_,String>(2)?,"at":row.get::<_,i64>(3)?,"remote_id":row.get::<_,Option<String>>(4)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
        let latest:Option<String>=c.db.query_row("SELECT event FROM sf_vcs_events WHERE space=? AND connector=? ORDER BY rowid DESC LIMIT 1",params![c.space,r.id],|x|x.get(0)).optional().map_err(storage)?;
        return Ok(json!({"latest_event":latest,"replies":rows}));
    }
    if r.fields["enabled"] != true
        || r.fields["link_tasks"] != true
        || r.fields["cnb_repo_id"].as_str().is_none_or(str::is_empty)
    {
        return Err(crate::forbidden("no active CNB pairing"));
    }
    match op {
        "cnb.internal.receipt" => {
            let seen: Option<String> =
                c.db.query_row(
                    "SELECT hash FROM sf_vcs_events WHERE space=? AND connector=? AND event=?",
                    params![c.space, r.id, required(q, "event_id")?],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?;
            if let Some(digest) = seen {
                if q["signed_digest"] != digest {
                    return Err(conflict("CNB build ID was reused with different input"));
                }
                Ok(json!({"received":true,"event_id":q["event_id"]}))
            } else {
                Ok(Value::Null)
            }
        }
        "cnb.internal.record" => super::ingest(c, &r, q, false),
        "cnb.internal.claim" => {
            if r.fields["cnb_backlinks"] != true {
                return Ok(Value::Null);
            }
            loop {
                let picked:Option<(String,String,String,String)>=c.db.query_row("SELECT id,object,task,state FROM sf_cnb_notes WHERE space=? AND connector=? AND repository=? AND (state IN ('pending','failed','uncertain') OR (state='inflight' AND until<?)) ORDER BY rowid LIMIT 1",params![c.space,r.id,repo,crate::now()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(storage)?;
                let Some((id, object, task, state)) = picked else {
                    return Ok(Value::Null);
                };
                if c.find("task", &task)?
                    .is_none_or(|t| t.fields["visibility"] == "private")
                    || c.readable("task", &task).is_err()
                {
                    c.db.execute(
                        "UPDATE sf_cnb_notes SET state='cancelled',lease=NULL WHERE id=?",
                        [&id],
                    )
                    .map_err(storage)?;
                    continue;
                }
                let link = crate::triage::execute(c, "task.share", &json!({"id":task}))?;
                let lease = Uuid::new_v4().to_string();
                c.db.execute(
                    "UPDATE sf_cnb_notes SET state='inflight',lease=?,until=?,at=? WHERE id=?",
                    params![lease, crate::now() + 120000, crate::now(), id],
                )
                .map_err(storage)?;
                return Ok(
                    json!({"delivery":id,"lease":lease,"object":object,"link":link["link"],"number":link["number"],"previous":state}),
                );
            }
        }
        "cnb.internal.finish" => {
            let state = required(q, "state")?;
            if !matches!(state, "sent" | "failed" | "uncertain") {
                return Err(invalid("unsupported CNB outcome"));
            }
            let n=c.db.execute("UPDATE sf_cnb_notes SET state=?,remote=?,lease=NULL,until=NULL,at=? WHERE space=? AND connector=? AND id=? AND lease=? AND state='inflight'",params![state,q["remote_id"].as_str(),crate::now(),c.space,r.id,required(q,"delivery")?,required(q,"lease")?]).map_err(storage)?;
            if n != 1 {
                return Err(conflict("CNB delivery lease has expired or been replaced"));
            }
            Ok(json!({"state":state,"delivery":q["delivery"]}))
        }
        _ => Err(invalid("invalid CNB operation")),
    }
}
