use super::*;
use chrono::{DateTime, SecondsFormat, Utc};

fn issue(c: &Context<'_>, b: &Value, number: i64) -> Result<Option<(String, bool)>> {
    c.db.query_row(
        "SELECT task,active FROM forge_issues WHERE pair=? AND number=?",
        params![b["id"].as_str(), number],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(storage)
}
fn record(c: &Context<'_>, b: &Value, event: &str, hash: &str, data: Value) -> Result<Value> {
    if data["action"] != "conflict" {
        c.db.execute(
            "INSERT INTO forge_receipts(pair,event,fingerprint,result) VALUES(?,?,?,?)",
            params![b["id"].as_str(), event, hash, data.to_string()],
        )
        .map_err(storage)?;
    }
    Ok(data)
}
fn stamp(v: &Value) -> Result<DateTime<Utc>> {
    let s = v
        .as_str()
        .ok_or_else(|| invalid("source timestamp missing"))?;
    s.parse::<DateTime<Utc>>()
        .or_else(|_| format!("{}Z", s.trim_end_matches(" UTC").replace(' ', "T")).parse())
        .map_err(|_| invalid("invalid source timestamp"))
}
fn before_pairing(time: DateTime<Utc>, b: &Value) -> bool {
    let start = b["started"].as_i64().unwrap_or(0);
    if time.timestamp_subsec_nanos() == 0 {
        time.timestamp() < start.div_euclid(1000)
    } else {
        time.timestamp_millis() < start
    }
}
fn external(v: &Value) -> String {
    v.as_i64()
        .map(|n| n.to_string())
        .or_else(|| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
fn checked_repo(p: &str, b: &Value, body: &Value) -> Result<()> {
    let r = if p == "github" {
        &body["repository"]
    } else {
        &body["project"]
    };
    let path = &r[if p == "github" {
        "full_name"
    } else {
        "path_with_namespace"
    }];
    if b["repository_id"] != external(&r["id"])
        || !path.as_str().is_some_and(|v| {
            b["repository"]
                .as_str()
                .is_some_and(|repo| v.eq_ignore_ascii_case(repo))
        })
    {
        return Err(forbidden("event belongs to another repository"));
    }
    if p == "github" && b["installation"] != external(&body["installation"]["id"]) {
        return Err(forbidden("unexpected GitHub installation"));
    }
    Ok(())
}
fn task(c: &Context<'_>, id: &str) -> Result<Record> {
    let t = c.readable("task", id)?;
    if t.fields["visibility"] == "private" {
        return Err(forbidden(
            "private task cannot be published to a repository",
        ));
    }
    Ok(t)
}
pub(super) fn mapping(c: &Context<'_>, p: &str, q: &Value) -> Result<Value> {
    let b = pair(c, p, required(q, "pair")?)?;
    let t = task(c, required(q, "task_id")?)?;
    let number: Option<i64> =
        c.db.query_row(
            "SELECT number FROM forge_issues WHERE pair=? AND task=? AND active=1",
            params![b["id"].as_str(), t.id],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let number = number.ok_or_else(|| "not_found: paired issue".to_owned())?;
    let (version,base):(String,String)=c.db.query_row("SELECT source_version,base FROM sf_connector_objects WHERE space=? AND connector=? AND task=?",params![c.space,b["id"].as_str(),t.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
    let base: Value = serde_json::from_str(&base).map_err(storage)?;
    let empty = json!([]);
    let local = t.fields.get("assignees").unwrap_or(&empty);
    let prior = base.get("assignees").unwrap_or(&empty);
    let mut outbound = Value::Null;
    let mut baseline_ids = Vec::new();
    if local != prior && q["action"] == "push_plan" {
        let mut list = Vec::new();
        for owner in local
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let a = account(c, p, owner)?
                .ok_or_else(|| conflict("a selected assignee has no linked source identity"))?;
            list.push(json!({"id":a["account"],"login":a["login"]}));
        }
        outbound = json!(list);
        for person in prior
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let a=account(c,p,person)?.ok_or_else(||conflict("a former assignee no longer has a linked source identity; refresh before publishing"))?;
            baseline_ids.push(a["account"].clone());
        }
        baseline_ids.extend(
            base["source_assignees"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
    }
    Ok(
        json!({"pair":b,"number":number,"task":t.id,"title":t.title,"body":t.fields["description"].as_str().unwrap_or(""),"closed":matches!(c.status_category(&t)?.as_str(),"done"|"cancelled"|"duplicate"|"completed"|"canceled"),"source_version":version,"source_baseline":base,"baseline_assignee_ids":baseline_ids,"outbound_assignees":outbound,"external_assignees":t.fields.get("source_assignees").unwrap_or(&empty)}),
    )
}
pub(super) fn thread(c: &Context<'_>, p: &str, q: &Value) -> Result<Value> {
    let t = c.readable("task", required(q, "task_id")?)?;
    let mut st=c.db.prepare("SELECT i.pair,i.number,i.url,i.active,p.path,p.active,p.direction,p.instance FROM forge_issues i JOIN forge_pairs p ON p.id=i.pair WHERE p.space=? AND p.org=? AND p.platform=? AND i.task=? ORDER BY p.started DESC").map_err(storage)?;
    let mut links = Vec::new();
    for r in st
        .query_map(params![c.space, c.org_id()?, p, t.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(storage)?
    {
        let (id, num, url, active, path, paired, direction, instance) = r.map_err(storage)?;
        let mut notes=c.db.prepare("SELECT n.note,n.login,n.body,n.updated,a.owner FROM forge_notes n LEFT JOIN forge_accounts a ON a.org=? AND a.platform=? AND a.instance=? AND a.account=n.author AND a.owner!='' WHERE n.pair=? AND n.number=? ORDER BY n.updated,n.note LIMIT 500").map_err(storage)?;
        let notes=notes.query_map(params![c.org_id()?,p,instance,id,num],|r|Ok(json!({"id":r.get::<_,String>(0)?,"login":r.get::<_,String>(1)?,"body":r.get::<_,String>(2)?,"at":r.get::<_,i64>(3)?,"member":r.get::<_,Option<String>>(4)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
        let author=c.db.query_row("SELECT u.account,u.login,a.owner FROM forge_issue_authors u LEFT JOIN forge_accounts a ON a.org=? AND a.platform=? AND a.instance=? AND a.account=u.account AND a.owner!='' WHERE u.pair=? AND u.number=?",params![c.org_id()?,p,instance,id,num],|r|Ok(json!({"id":r.get::<_,String>(0)?,"login":r.get::<_,String>(1)?,"member":r.get::<_,Option<String>>(2)?}))).optional().map_err(storage)?;
        links.push(json!({"author":author,"pair":id,"number":num,"url":url,"repository":path,"active":active&&paired,"can_reply":active&&paired&&direction=="both"&&c.require_edit().is_ok()&&t.fields["visibility"]!="private","comments":notes}));
    }
    Ok(json!(links))
}
pub(super) fn unlink(c: &Context<'_>, p: &str, q: &Value) -> Result<Value> {
    let map = mapping(c, p, q)?;
    c.authorize_write("task", Some(&task(c, required(q, "task_id")?)?))?;
    c.db.execute(
        "UPDATE forge_issues SET active=0 WHERE pair=? AND number=?",
        params![q["pair"].as_str(), map["number"].as_i64()],
    )
    .map_err(storage)?;
    c.db.execute("UPDATE forge_outbox SET phase='cancelled',lease=NULL WHERE pair=? AND task=? AND phase NOT IN('sent','cancelled')",params![q["pair"].as_str(),q["task_id"].as_str()]).map_err(storage)?;
    Ok(json!({"active":false,"task":q["task_id"]}))
}
fn note(c: &Context<'_>, p: &str, b: &Value, body: &Value) -> Result<Value> {
    let gh = p == "github";
    let n = if gh {
        &body["comment"]
    } else {
        &body["object_attributes"]
    };
    let issue_node = &body["issue"];
    if issue_node["pull_request"].is_object()
        || !gh
            && (n["noteable_type"] != "Issue"
                || n["system"] == true
                || n["internal"] == true
                || issue_node["confidential"] == true)
    {
        return Ok(json!({"ignored":"not_issue_comment"}));
    }
    let number = issue_node[if gh { "number" } else { "iid" }]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("issue number missing"))?;
    let Some((task_id, true)) = issue(c, b, number)? else {
        return Ok(json!({"ignored":"issue_not_paired"}));
    };
    task(c, &task_id)?;
    let id = external(&n["id"]);
    let author = if gh { &n["user"] } else { &body["user"] };
    let login = author[if gh { "login" } else { "username" }]
        .as_str()
        .unwrap_or("external");
    let mut body_text = n[if gh { "body" } else { "note" }].as_str().unwrap_or("");
    if let Some((visible,marker))=body_text.rsplit_once("\n\n<!-- shipforge:")
        && let Some(ticket)=marker.strip_suffix(" -->")
        && c.db.query_row("SELECT EXISTS(SELECT 1 FROM forge_outbox WHERE id=? AND pair=? AND number=? AND body=?)",params![ticket,b["id"].as_str(),number,visible],|r|r.get::<_,bool>(0)).map_err(storage)?{body_text=visible;}
    if id.is_empty() || body_text.len() > 262144 {
        return Err(invalid("invalid source comment"));
    }
    let timestamp = stamp(
        n.get("updated_at")
            .filter(|v| v.is_string())
            .unwrap_or(&n["created_at"]),
    )?;
    let time = timestamp.timestamp_millis();
    if before_pairing(timestamp, b) {
        return Ok(json!({"ignored":"before_pairing"}));
    }
    if body["action"]=="deleted"||n["action"]=="delete"{c.db.execute("INSERT OR IGNORE INTO forge_note_deletions VALUES(?,?,?)",params![b["id"].as_str(),number,id]).map_err(storage)?;c.db.execute("DELETE FROM forge_notes WHERE pair=? AND number=? AND note=? AND updated<=?",params![b["id"].as_str(),number,id,time]).map_err(storage)?;}
    else if !c.db.query_row("SELECT EXISTS(SELECT 1 FROM forge_note_deletions WHERE pair=? AND number=? AND note=?)",params![b["id"].as_str(),number,id],|r|r.get::<_,bool>(0)).map_err(storage)?{c.db.execute("INSERT INTO forge_notes(pair,number,note,author,login,body,updated) VALUES(?,?,?,?,?,?,?) ON CONFLICT(pair,number,note) DO UPDATE SET body=excluded.body,updated=excluded.updated WHERE excluded.updated>=forge_notes.updated",params![b["id"].as_str(),number,id,external(&author["id"]),login,body_text,time]).map_err(storage)?;}
    Ok(json!({"action":"comment","task":task_id,"comment":id}))
}
fn receive_issue(c: &Context<'_>, p: &str, b: &Value, body: &Value, event: &str) -> Result<Value> {
    let gh = p == "github";
    let data = &body[if gh { "issue" } else { "object_attributes" }];
    if data["confidential"] == true || data["pull_request"].is_object() {
        return Ok(json!({"ignored":"private_or_pull_request"}));
    }
    let action = if gh {
        body["action"].as_str()
    } else {
        data["action"].as_str()
    }
    .unwrap_or("");
    if !matches!(
        action,
        "open"
            | "opened"
            | "edit"
            | "edited"
            | "update"
            | "close"
            | "closed"
            | "reopen"
            | "reopened"
            | "assigned"
            | "unassigned"
    ) {
        return Ok(json!({"ignored":"unsupported_issue_action"}));
    }
    let number = data[if gh { "number" } else { "iid" }]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("invalid issue number"))?;
    let previous = issue(c, b, number)?;
    if previous.as_ref().is_some_and(|(_, active)| !*active) {
        return Ok(json!({"ignored":"single_task_unlinked"}));
    }
    if previous.is_none()
        && (!matches!(action, "opened" | "open") || before_pairing(stamp(&data["created_at"])?, b))
    {
        return Ok(json!({"ignored":"historical_or_unpaired"}));
    }
    let updated = stamp(&data["updated_at"])?.to_rfc3339_opts(SecondsFormat::Micros, true);
    let mut members = Vec::new();
    let mut externals = Vec::new();
    for person in data["assignees"]
        .as_array()
        .or(body["assignees"].as_array())
        .into_iter()
        .flatten()
    {
        let remote = external(&person["id"]);
        if remote.is_empty() {
            continue;
        }
        let id:Option<String>=c.db.query_row("SELECT owner FROM forge_accounts WHERE org=? AND platform=? AND instance=? AND account=? AND owner!=''",params![c.org_id()?,p,b["instance"].as_str(),remote],|r|r.get(0)).optional().map_err(storage)?;
        if let Some(id) = id.filter(|id| c.readable("member", id).is_ok()) {
            members.push(id);
        } else {
            externals.push(remote);
        }
    }
    let mut q = json!({"op":"connector.ingest","id":b["id"],"external_id":number.to_string(),"event_id":event,"source_version":updated,"title":data["title"],"description":data[if gh{"body"}else{"description"}].as_str().unwrap_or(""),"closed":data["state"]=="closed"});
    if data["assignees"].is_array() || body["assignees"].is_array() {
        q["assignees"] = json!(members);
        q["source_assignees"] = json!(externals);
    }
    let out = crate::worker::forge_ingest(c, &q)?;
    if out["action"] != "conflict" && previous.is_none() {
        let path = b["repository"].as_str().unwrap_or("");
        let site = b["instance"].as_str().unwrap_or("");
        let url = format!(
            "{site}/{path}/{}/{number}",
            if gh { "issues" } else { "-/issues" }
        );
        let tid = required(&out, "task")?;
        c.db.execute(
            "INSERT INTO forge_issues(pair,number,task,url,active) VALUES(?,?,?,?,1)",
            params![b["id"].as_str(), number, tid, url],
        )
        .map_err(storage)?;
        let author = if gh {
            &data["user"]
        } else {
            data.get("author").unwrap_or(&body["user"])
        };
        let account = external(if !gh && !data["author_id"].is_null() {
            &data["author_id"]
        } else {
            &author["id"]
        });
        if !account.is_empty() {
            let login = if external(&author["id"]) == account {
                author[if gh { "login" } else { "username" }]
                    .as_str()
                    .unwrap_or(&account)
            } else {
                &account
            };
            c.db.execute(
                "INSERT INTO forge_issue_authors VALUES(?,?,?,?)",
                params![b["id"].as_str(), number, account, login],
            )
            .map_err(storage)?;
        }
        if b["backlinks"] == true {
            publish::backlink(c, b, number, tid, "issue")?;
        }
    }
    Ok(out)
}
pub(super) fn ingest(c: &Context<'_>, p: &str, q: &Value) -> Result<Value> {
    let b = pair(c, p, required(q, "pair")?)?;
    org_admin(c)?;
    c.require_admin()?;
    let event = required(q, "event_id")?;
    if event.len() > 256 {
        return Err(invalid("delivery ID too long"));
    }
    let payload = &q["payload"];
    checked_repo(p, &b, payload)?;
    let hash = crate::worker::digest(&json!([q["event"], payload]).to_string());
    if let Some((prior, raw)) =
        c.db.query_row(
            "SELECT fingerprint,result FROM forge_receipts WHERE pair=? AND event=?",
            params![b["id"].as_str(), event],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage)?
    {
        if prior != hash {
            return Err(conflict("delivery ID identifies a different event"));
        }
        return serde_json::from_str(&raw).map_err(storage);
    }
    let t = required(q, "event")?;
    let out = match t {
        "issues" | "issue" => receive_issue(c, p, &b, payload, event)?,
        "issue_comment" | "note" => note(c, p, &b, payload)?,
        "push" if b["commits"] != true => json!({"ignored":"commits_disabled"}),
        "pull_request" | "merge_request" if b["pull_requests"] != true => {
            json!({"ignored":"reviews_disabled"})
        }
        "push" | "pull_request" | "merge_request" => {
            let connector = c.readable("connector", required(&b, "id")?)?;
            let out = super::super::ingest(
                c,
                &connector,
                &json!({"payload":payload,"event_id":event}),
                false,
            )?;
            if b["backlinks"] == true && t != "push" {
                let n = payload["number"]
                    .as_i64()
                    .or(payload["object_attributes"]["iid"].as_i64())
                    .unwrap_or(0);
                for link in out["links"].as_array().into_iter().flatten() {
                    publish::backlink(c, &b, n, required(link, "task")?, "review")?;
                }
            }
            out
        }
        _ => json!({"ignored":"event_type"}),
    };
    record(c, &b, event, &hash, out)
}
