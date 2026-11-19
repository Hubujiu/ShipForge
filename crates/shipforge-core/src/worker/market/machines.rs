use super::*;

fn allowed(c: &Context<'_>, q: &Value) -> Result<Record> {
    let r = c.readable("machine", req(q, "machine_id")?)?;
    if r.fields["disabled"] == true || (r.fields["owner"] != c.actor && r.fields["shared"] != true)
    {
        return Err(forbidden("machine is unavailable"));
    }
    let runtime = req(q, "runtime")?;
    if !r.fields["runtimes"]
        .as_array()
        .is_some_and(|v| v.contains(&json!(runtime)))
    {
        return Err(invalid("runtime is not registered on this machine"));
    }
    Ok(r)
}
fn candidates(c: &Context<'_>, q: &Value) -> Result<Vec<Value>> {
    let machine = allowed(c, q)?;
    let stored:Option<String>=c.db.query_row("SELECT result FROM skill_jobs WHERE space=? AND machine=? AND actor=? AND state='done' AND json_extract(payload,'$.op')='skill.discover' AND json_extract(payload,'$.runtime')=? ORDER BY created DESC,rowid DESC LIMIT 1",params![c.space,machine.id,c.actor,req(q,"runtime")?],|r|r.get(0)).optional().map_err(storage)?;
    let data: Value = serde_json::from_str(stored.as_deref().unwrap_or("[]")).map_err(storage)?;
    Ok(data.as_array().cloned().unwrap_or_default())
}
fn queue(c: &Context<'_>, q: &Value) -> Result<Value> {
    let machine = allowed(c, q)?;
    let operation = req(q, "op")?;
    if operation == "skill.machine_import" {
        let source = req(q, "source_id")?;
        if !candidates(c, q)?.iter().any(|v| v["id"] == source) {
            return Err(forbidden("select an available discovered skill"));
        }
        if let Some(agent) = q["agent"].as_str().filter(|v| !v.is_empty()) {
            crate::agents::require_manage(c, &c.readable("agent", agent)?)?;
        }
    }
    let payload=json!({"op":operation,"machine_id":machine.id,"runtime":req(q,"runtime")?,"source_id":q["source_id"],"agent":q["agent"]}).to_string();
    let queued:Option<String>=c.db.query_row("SELECT id FROM skill_jobs WHERE space=? AND machine=? AND actor=? AND payload=? AND state='queued'",params![c.space,machine.id,c.actor,payload],|r|r.get(0)).optional().map_err(storage)?;
    let id = if let Some(id) = queued {
        id
    } else {
        let count:i64=c.db.query_row("SELECT COUNT(*) FROM skill_jobs WHERE space=? AND machine=? AND actor=? AND state='queued'",params![c.space,machine.id,c.actor],|r|r.get(0)).map_err(storage)?;
        if count >= 32 {
            return Err(invalid("too many queued skill jobs on this machine"));
        }
        let id = Uuid::new_v4().to_string();
        c.db.execute("INSERT INTO skill_jobs(space,machine,actor,id,payload,state,result,created) VALUES(?,?,?,?,?,'queued','null',?)",params![c.space,machine.id,c.actor,id,payload,now()]).map_err(storage)?;
        id
    };
    Ok(json!({"id":id,"state":"queued","machine_online":crate::machine_is_online(&machine.fields)}))
}
pub(super) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if matches!(op, "skill.discover" | "skill.machine_import") {
        return queue(c, q);
    }
    if op == "skill.sources" {
        let mut sources = candidates(c, q)?;
        let query = q["query"].as_str().unwrap_or("").to_lowercase();
        sources.retain(|s| {
            ["title", "description"].iter().any(|k| {
                s[*k]
                    .as_str()
                    .is_some_and(|v| v.to_lowercase().contains(&query))
            })
        });
        let mut stmt=c.db.prepare("SELECT id,state,result,json_extract(payload,'$.op') FROM skill_jobs WHERE space=? AND machine=? AND actor=? AND json_extract(payload,'$.runtime')=? ORDER BY created DESC,rowid DESC LIMIT 24").map_err(storage)?;
        let jobs = stmt
            .query_map(
                params![c.space, req(q, "machine_id")?, c.actor, req(q, "runtime")?],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(storage)?;
        let mut history = Vec::new();
        for row in jobs {
            let (id, state, result, op) = row.map_err(storage)?;
            let result: Value = serde_json::from_str(&result).map_err(storage)?;
            history.push(json!({"id":id,"state":state,"operation":op,"result":if op=="skill.discover"&&state=="done"{json!({"count":result.as_array().map(Vec::len)})}else{result}}));
        }
        return Ok(json!({"sources":sources,"jobs":history}));
    }
    let machine = super::super::authenticate(c, q)?;
    if machine.fields["disabled"] == true {
        return Err(forbidden("machine is disabled"));
    }
    if op == "skill.jobs" {
        let mut stmt=c.db.prepare("SELECT id,payload FROM skill_jobs WHERE space=? AND machine=? AND state='queued' ORDER BY created,rowid LIMIT 16").map_err(storage)?;
        let jobs = stmt
            .query_map(params![c.space, machine.id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?;
        let mut rows = Vec::new();
        for j in jobs {
            let (id, p) = j.map_err(storage)?;
            let mut p: Value = serde_json::from_str(&p).map_err(storage)?;
            p["id"] = json!(id);
            rows.push(p);
        }
        return Ok(json!(rows));
    }
    if op != "skill.finish" {
        return Err(invalid("unknown skill operation"));
    }
    let id = req(q, "id")?;
    let (actor, state, payload): (String, String, String) =
        c.db.query_row(
            "SELECT actor,state,payload FROM skill_jobs WHERE id=? AND space=? AND machine=?",
            params![id, c.space, machine.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| forbidden("job is unavailable"))?;
    if state != "queued" {
        return Ok(json!({"id":id,"state":state}));
    }
    let args: Value = serde_json::from_str(&payload).map_err(storage)?;
    let requester = Context {
        actor: &actor,
        ..*c
    };
    c.db.execute_batch("SAVEPOINT skill_job_apply")
        .map_err(storage)?;
    let result = (|| {
        requester.require_edit()?;
        allowed(&requester, &args)?;
        if let Some(error) = q["error"].as_str() {
            return Err(error.chars().take(512).collect::<String>());
        }
        if args["op"] == "skill.machine_import" {
            return installation::import(
                &requester,
                &json!({"files":q["files"],"agent":args["agent"]}),
            );
        }
        let sources = q["sources"]
            .as_array()
            .filter(|v| v.len() <= 2048)
            .ok_or_else(|| invalid("invalid skill discovery results"))?;
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for s in sources {
            let id = req(s, "id")?;
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || !seen.insert(id) {
                return Err(invalid("invalid discovered skill id"));
            }
            out.push(json!({"id":id,"title":req(s,"title")?,"description":s["description"].as_str().unwrap_or("").chars().take(400).collect::<String>(),"origin":if s["origin"]=="project"{"project"}else{"runtime"}}));
        }
        Ok(json!(out))
    })();
    let (state, response) = match result {
        Ok(v) => ("done", v),
        Err(e) => {
            c.db.execute_batch("ROLLBACK TO skill_job_apply")
                .map_err(storage)?;
            ("failed", json!({"error":e}))
        }
    };
    c.db.execute_batch("RELEASE skill_job_apply")
        .map_err(storage)?;
    c.db.execute(
        "UPDATE skill_jobs SET state=?,result=? WHERE id=?",
        params![state, response.to_string(), id],
    )
    .map_err(storage)?;
    Ok(json!({"id":id,"state":state}))
}
