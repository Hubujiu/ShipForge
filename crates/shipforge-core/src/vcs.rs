//! Associate commits and review activity with exact task numbers in a bound repository.
use crate::{Context, Record, Result, conflict, invalid, required, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
mod cnb;
pub(crate) use cnb::command as cnb_command;

pub(crate) fn init(db: &Connection) -> Result<()> {
    cnb::init(db)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_vcs_events (
        space TEXT NOT NULL,connector TEXT NOT NULL,event TEXT NOT NULL,hash TEXT NOT NULL,response TEXT NOT NULL,
        PRIMARY KEY(space,connector,event));
        CREATE TABLE IF NOT EXISTS sf_vcs_links (
        space TEXT NOT NULL,connector TEXT NOT NULL,object TEXT NOT NULL,activity TEXT NOT NULL,task TEXT NOT NULL,comment TEXT NOT NULL,
        PRIMARY KEY(space,connector,object,activity,task));").map_err(storage)
}

pub(crate) fn validate(fields: &Value) -> Result<()> {
    if let Some(value) = fields.get("link_tasks")
        && !value.is_boolean()
    {
        return Err(invalid("link_tasks must be a boolean"));
    }
    if fields["link_tasks"] != true {
        return Ok(());
    }
    for key in ["review_rule_id", "repository_host"] {
        if !fields[key].is_null() && !fields[key].is_string() {
            return Err(invalid(&format!("{key} must be a string")));
        }
    }
    let repo = required(fields, "repository")?;
    let components: Vec<_> = repo.split('/').collect();
    if !matches!(
        fields["provider"].as_str(),
        Some("github" | "gitlab" | "cnb")
    ) || components.len() < 2
        || (fields["provider"] == "github" && components.len() != 2)
        || repo.len() > 400
        || components.iter().any(|s| {
            s.is_empty()
                || matches!(*s, "." | "..")
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
    {
        return Err(invalid(
            "link_tasks requires a valid repository path for the selected provider",
        ));
    }
    if let Some(host) = fields
        .get("repository_host")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        && (fields["provider"] != "gitlab"
            || host.len() > 253
            || host.split('.').any(|p| {
                p.is_empty()
                    || p.starts_with('-')
                    || p.ends_with('-')
                    || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }))
    {
        return Err(invalid("repository_host must be a GitLab HTTPS hostname"));
    }
    Ok(())
}

pub(crate) fn review_rule(c: &Context<'_>, fields: &Value) -> Result<Option<Record>> {
    let Some(id) = fields["review_rule_id"].as_str().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let Some(rule) = c.find("automation", id)? else {
        return Ok(None);
    };
    if rule.fields["enabled"] != true
        || !c.visible(&rule)?
        || !rule.fields["triggers"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|t| t["type"] == "webhook" && t["enabled"] != false)
    {
        return Ok(None);
    }
    Ok(Some(rule))
}

fn review_run(
    c: &Context<'_>,
    connector: &Record,
    item: &Activity,
    task: Option<&str>,
    rule: &Record,
) -> Result<String> {
    let event =
        crate::worker::digest(&json!([connector.id, item.object, item.activity, task]).to_string());
    let q = json!({"op":"automation.fire","id":rule.id,"source":"webhook","event_id":format!("vcs-review-{event}"),"task_id":task,
        "payload":{"provider":connector.fields["provider"],"repository":connector.fields["repository"],"type":item.kind,
        "action":item.action,"url":item.url,"title":item.title}});
    let run =
        crate::worker::dispatch(c, &q).ok_or_else(|| invalid("automation.fire unavailable"))??;
    Ok(required(&run, "id")?.to_owned())
}

struct Activity {
    object: String,
    activity: String,
    kind: &'static str,
    action: String,
    title: String,
    url: String,
    references: BTreeSet<String>,
}
fn numbers(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut found = BTreeSet::new();
    for i in 0..bytes.len() {
        if !bytes[i].is_ascii_alphanumeric()
            || i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
        {
            continue;
        }
        let prefix = bytes[i..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        if prefix > 12 || bytes.get(i + prefix) != Some(&b'-') {
            continue;
        }
        let digits = i + prefix + 1;
        let length = bytes[digits..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if length == 0
            || length > 19
            || bytes
                .get(digits + length)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            continue;
        }
        found.insert(text[i..digits + length].to_ascii_uppercase());
    }
    found
}

fn parse(connector: &Record, p: &Value) -> Result<Vec<Activity>> {
    let f = &connector.fields;
    if f["enabled"] != true || f["link_tasks"] != true {
        return Err(invalid(
            "enable task linking on the repository connector first",
        ));
    }
    validate(f)?;
    if f["provider"] == "cnb" {
        return cnb::activities(connector, p);
    }
    let github = f["provider"] == "github";
    let repo = required(f, "repository")?;
    let (source, web) = if github {
        (
            required(&p["repository"], "full_name")?,
            required(&p["repository"], "html_url")?,
        )
    } else {
        if let Some(project) = f["project_id"].as_str().filter(|v| !v.is_empty()) {
            let id = &p["project"]["id"];
            if id.as_str() != Some(project)
                && id.as_u64().map(|v| v.to_string()).as_deref() != Some(project)
            {
                return Err(invalid("GitLab project ID does not match the connector"));
            }
        }
        (
            required(&p["project"], "path_with_namespace")?,
            required(&p["project"], "web_url")?,
        )
    };
    let host = if github {
        "github.com"
    } else {
        f["repository_host"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or("gitlab.com")
    };
    let base = format!("https://{host}/{repo}");
    if !source.eq_ignore_ascii_case(repo) || !web.trim_end_matches('/').eq_ignore_ascii_case(&base)
    {
        return Err(invalid("event is outside the configured repository"));
    }
    if (github && p["pull_request"].is_object()) || (!github && p["object_kind"] == "merge_request")
    {
        let change = if github {
            &p["pull_request"]
        } else {
            &p["object_attributes"]
        };
        if github
            && p["repository"]["private"] != true
            && !matches!(
                change["author_association"].as_str(),
                Some("OWNER" | "MEMBER" | "COLLABORATOR")
            )
        {
            return Ok(Vec::new());
        }
        let n = if github {
            p["number"].as_u64().or(change["number"].as_u64())
        } else {
            change["iid"].as_u64()
        }
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("invalid review number"))?;
        if github
            && change["base"]["repo"]["full_name"]
                .as_str()
                .is_some_and(|s| !s.eq_ignore_ascii_case(repo))
            || !github
                && !change["target_project_id"].is_null()
                && change["target_project_id"] != p["project"]["id"]
        {
            return Err(invalid("review target does not match the repository"));
        }
        let action = if github {
            required(p, "action")?
        } else {
            required(change, "action")?
        };
        let action = match (github, action) {
            (
                true,
                "opened" | "reopened" | "edited" | "synchronize" | "ready_for_review"
                | "converted_to_draft",
            )
            | (
                false,
                "open" | "reopen" | "update" | "approved" | "unapproved" | "merge" | "close",
            ) => action,
            (true, "closed") if change["merged"] == true => "merged",
            (true, "closed") => "closed",
            _ => return Ok(Vec::new()),
        };
        let title = required(change, "title")?;
        let description = change[if github { "body" } else { "description" }]
            .as_str()
            .unwrap_or("");
        let branch = if github {
            &change["head"]["ref"]
        } else {
            &change["source_branch"]
        };
        let references = numbers(&format!(
            "{title}\n{description}\n{}",
            branch.as_str().unwrap_or("")
        ));
        let id = format!("{}{n}", if github { "PR #" } else { "MR !" });
        let sha = if github {
            &change["head"]["sha"]
        } else {
            &change["last_commit"]["id"]
        };
        let rev = crate::worker::digest(
            &json!([
                action,
                title,
                description,
                branch,
                sha,
                change["updated_at"]
            ])
            .to_string(),
        );
        let path = if github { "pull" } else { "-/merge_requests" };
        return Ok(vec![Activity {
            object: format!("{base}/{path}/{n}"),
            activity: rev,
            kind: if github {
                "pull_request"
            } else {
                "merge_request"
            },
            action: action.into(),
            title: format!(
                "{id} · {}",
                title
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect::<String>()
            ),
            url: format!("{base}/{path}/{n}"),
            references,
        }]);
    }
    if !github && p["object_kind"] != "push" {
        return Err(invalid("expected a GitLab push or merge request"));
    }
    let commits = p["commits"]
        .as_array()
        .filter(|v| v.len() <= 2048)
        .ok_or_else(|| invalid("expected a push with up to 2048 commits"))?;
    let mut output = Vec::new();
    for commit in commits {
        let sha = required(commit, "id")?;
        if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("invalid Git commit object ID"));
        }
        let msg = required(commit, "message")?;
        let references = numbers(msg);
        let path = if github { "commit" } else { "-/commit" };
        let sha = sha.to_ascii_lowercase();
        let url = format!("{base}/{path}/{sha}");
        output.push(Activity {
            object: url.clone(),
            activity: "commit".into(),
            kind: "commit",
            action: "push".into(),
            title: format!(
                "{} · {}",
                &sha[..8],
                msg.lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect::<String>()
            ),
            url,
            references,
        });
    }
    Ok(output)
}

pub(crate) fn ingest(
    c: &Context<'_>,
    connector: &Record,
    q: &Value,
    preview: bool,
) -> Result<Value> {
    let event = required(q, "event_id")?;
    if event.len() > 256 || event.chars().any(char::is_control) {
        return Err(invalid("invalid VCS delivery ID"));
    }
    let payload = &q["payload"];
    let items = parse(connector, payload)?;
    let fingerprint = if connector.fields["provider"] == "cnb" {
        required(q, "signed_digest")?.to_owned()
    } else {
        crate::worker::digest(&payload.to_string())
    };
    let prior: Option<(String, String)> =
        c.db.query_row(
            "SELECT hash,response FROM sf_vcs_events WHERE space=? AND connector=? AND event=?",
            params![c.space, connector.id, event],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((hash, saved)) = prior {
        if hash != fingerprint {
            return Err(conflict(
                "VCS delivery ID was reused with different content",
            ));
        }
        let mut result: Value = serde_json::from_str(&saved).map_err(storage)?;
        if let Some(rows) = result["links"].as_array_mut() {
            let mut visible = Vec::new();
            for entry in rows.drain(..) {
                if let Some(task) = c.find("task", required(&entry, "task")?)?
                    && task.fields["visibility"] != "private"
                    && c.visible(&task)?
                {
                    visible.push(entry)
                }
            }
            *rows = visible;
        }
        if let Some(runs) = result["review_runs"].as_array_mut() {
            let mut visible = Vec::new();
            for id in runs.drain(..) {
                if let Some(run) = c.find("run", id.as_str().unwrap_or(""))?
                    && c.visible(&run)?
                {
                    visible.push(id)
                }
            }
            *runs = visible;
        }
        return Ok(result);
    }
    let mut index = BTreeMap::new();
    let mut visible = BTreeSet::new();
    for t in c.records("task")? {
        if t.fields["visibility"] != "private"
            && c.visible(&t)?
            && let Some(number) = t.fields["number"].as_str()
        {
            index.insert(number.to_ascii_uppercase(), t.id.clone());
            visible.insert(t.id);
        }
    }
    let mut links = Vec::new();
    let mut review_runs = Vec::new();
    let trusted_review = connector.fields["provider"] == "github"
        || (payload["project"]["id"].is_u64()
            && payload["object_attributes"]["source_project_id"] == payload["project"]["id"]);
    let rule = if preview || !trusted_review {
        None
    } else {
        review_rule(c, &connector.fields)?
    };
    for item in items {
        let mut tasks: BTreeSet<_> = item
            .references
            .iter()
            .filter_map(|n| index.get(n).cloned())
            .collect();
        if item.kind != "commit" {
            let mut st=c.db.prepare("SELECT DISTINCT task FROM sf_vcs_links WHERE space=? AND connector=? AND object=?").map_err(storage)?;
            let rows = st
                .query_map(params![c.space, connector.id, item.object], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(storage)?;
            for row in rows {
                let task = row.map_err(storage)?;
                if visible.contains(&task) {
                    tasks.insert(task);
                }
            }
        }
        if let Some(rule) = &rule
            && matches!(
                item.action.as_str(),
                "open" | "opened" | "reopen" | "reopened" | "ready_for_review"
            )
        {
            if tasks.is_empty() {
                review_runs.push(review_run(c, connector, &item, None, rule)?);
            }
            for task in &tasks {
                review_runs.push(review_run(c, connector, &item, Some(task), rule)?);
            }
        }
        for task in tasks {
            let existing:Option<String>=c.db.query_row("SELECT comment FROM sf_vcs_links WHERE space=? AND connector=? AND object=? AND activity=? AND task=?",
                params![c.space,connector.id,item.object,item.activity,task],|r|r.get(0)).optional().map_err(storage)?;
            let mut link = json!({"task":task,"provider":connector.fields["provider"],"repository":connector.fields["repository"],"type":item.kind,"action":item.action,"title":item.title,"url":item.url});
            if let Some(comment) = existing {
                link["comment"] = json!(comment);
            } else if !preview {
                let id = Uuid::new_v4().to_string();
                c.db.execute("INSERT INTO sf_vcs_links(space,connector,object,activity,task,comment) VALUES(?,?,?,?,?,?)",params![c.space,connector.id,item.object,item.activity,task,id]).map_err(storage)?;
                crate::domain::write_mode(
                    c,
                    &json!({"op":"create","kind":"comment","id":id,"title":item.title,
                    "fields":{"task":task,"body":format!("{} · {}",item.action,item.title),"vcs":link,"source":"vcs"}}),
                    false,
                )?;
                link["comment"] = json!(id);
            }
            links.push(link);
        }
    }
    let result = json!({"action":if preview{"preview"}else{"linked"},"event_id":event,"links":links,"review_runs":review_runs});
    if !preview {
        if connector.fields["provider"] == "cnb" {
            cnb::queue(c, connector, payload, &result)?;
        }
        c.db.execute(
            "INSERT INTO sf_vcs_events(space,connector,event,hash,response) VALUES(?,?,?,?,?)",
            params![
                c.space,
                connector.id,
                event,
                fingerprint,
                result.to_string()
            ],
        )
        .map_err(storage)?;
    }
    Ok(result)
}
