use super::*;
use crate::vendors::{administrator, input, org_location, suffix};
use std::collections::HashSet;

const TASK_FIELDS: &str = "gid,resource_type,name,notes,modified_at,completed,workspace.gid,memberships.project.gid,assignee.gid,created_by.gid,permalink_url";

fn gid(v: &Value) -> Result<&str> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| "invalid: expected an Asana numeric GID".to_owned())
}
struct Asana {
    root: String,
    token: String,
    limit: usize,
}
impl Asana {
    fn request(
        &self,
        method: &str,
        path: &str,
        params: &[(&str, String)],
        body: &Value,
    ) -> Result<Value> {
        let url = endpoint(&self.root, path, params)?;
        let agent = http();
        let auth = format!("Bearer {}", self.token);
        let response = match method {
            "GET" => agent
                .get(url.as_str())
                .header("Authorization", &auth)
                .header("Accept", "application/json")
                .call(),
            "PUT" => agent
                .put(url.as_str())
                .header("Authorization", &auth)
                .send_json(body),
            "POST" => agent
                .post(url.as_str())
                .header("Authorization", &auth)
                .send_json(body),
            _ => return Err("invalid: unsupported Asana request".into()),
        }
        .map_err(|_| "vendor: Asana request was interrupted")?;
        let value = unpack(response)?;
        if value.get("data").is_none() {
            return Err("vendor: expected Asana data".into());
        }
        Ok(value)
    }
    fn pages(&self, path: &str, fields: &str, params: &[(&str, String)]) -> Result<Vec<Value>> {
        let mut params = params.to_vec();
        params.extend([
            ("opt_fields", fields.to_owned()),
            ("limit", self.limit.to_string()),
        ]);
        let mut out = Vec::new();
        let mut offsets = HashSet::new();
        let mut ids = HashSet::new();
        let mut bytes = 0usize;
        for _ in 0..=5000 {
            let res = self.request("GET", path, &params, &Value::Null)?;
            let rows = res["data"]
                .as_array()
                .ok_or("vendor: expected an Asana resource list")?;
            for row in rows {
                if !ids.insert(gid(&row["gid"])?.to_owned()) {
                    return Err("vendor: repeated Asana GID during pagination".into());
                }
                bytes += row.to_string().len();
                if out.len() >= 5000 || bytes > 7_000_000 {
                    return Err("vendor: Asana result exceeds import limit".into());
                }
                out.push(row.clone());
            }
            if res["next_page"].is_null() {
                return Ok(out);
            }
            let offset = res["next_page"]["offset"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 2048)
                .ok_or("vendor: invalid Asana next-page offset")?;
            if !offsets.insert(offset.to_owned()) {
                return Err("vendor: repeated Asana pagination offset".into());
            }
            params.retain(|(name, _)| *name != "offset");
            params.push(("offset", offset.to_owned()));
        }
        Err("vendor: too many Asana pages".into())
    }
    fn project(&self, workspace: &str, project: &str) -> Result<()> {
        let res = self.request(
            "GET",
            &format!("/projects/{project}"),
            &[("opt_fields", "name,workspace.gid,archived".into())],
            &Value::Null,
        )?;
        let row = &res["data"];
        if row["gid"] != project || row["workspace"]["gid"] != workspace || row["archived"] == true
        {
            return Err("forbidden: archived or out-of-workspace Asana project".into());
        }
        Ok(())
    }
    fn task(&self, key: &str, workspace: &str, project: &str) -> Result<Value> {
        let res = self.request(
            "GET",
            &format!("/tasks/{key}"),
            &[("opt_fields", TASK_FIELDS.into())],
            &Value::Null,
        )?;
        let row = &res["data"];
        if row["gid"] != key
            || row["resource_type"] != "task"
            || row["workspace"]["gid"] != workspace
            || !row["memberships"]
                .as_array()
                .is_some_and(|ms| ms.iter().any(|m| m["project"]["gid"] == project))
        {
            return Err("forbidden: task belongs to a different Asana workspace or project".into());
        }
        Ok(row.clone())
    }
    fn push(
        &self,
        plan: &Value,
        workspace: &str,
        project: &str,
    ) -> std::result::Result<String, publish::PublicationError> {
        let key = gid(&plan["key"])?;
        let current = self.task(key, workspace, project)?;
        let path = format!("/tasks/{key}");
        if plan["intent"] == "comment" {
            let stories = format!("{path}/stories");
            let uncertain = |message: String| publish::PublicationError {
                state: "uncertain",
                message,
            };
            if let Some(known) = plan["confirm_comment"].as_str() {
                let rows = self
                    .pages(&stories, "text,resource_subtype", &[])
                    .map_err(uncertain)?;
                if rows.iter().any(|r| {
                    r["gid"] == known
                        && r["resource_subtype"] == "comment_added"
                        && r["text"] == plan["body"]
                }) {
                    return Ok(known.to_owned());
                }
                return Err(uncertain(
                    "conflict: Asana story is not the matching comment on this task".into(),
                ));
            }
            let sent = self
                .request(
                    "POST",
                    &stories,
                    &[],
                    &json!({"data":{"text":plan["body"]}}),
                )
                .map_err(|message| publish::PublicationError {
                    state: if message.starts_with("vendor: upstream returned HTTP 4") {
                        "failed"
                    } else {
                        "uncertain"
                    },
                    message,
                })?;
            return Ok(gid(&sent["data"]["gid"]).map_err(uncertain)?.to_owned());
        }
        let mut data = serde_json::Map::new();
        for (remote, local, baseline) in [
            ("name", "title", "source_title"),
            ("notes", "description", "source_description"),
        ] {
            if current[remote] == plan[local] {
                continue;
            }
            if current[remote] != plan[baseline] {
                return Err(
                    "conflict: Asana fields changed since the last import; preview the project"
                        .to_owned()
                        .into(),
                );
            }
            data.insert(remote.to_owned(), plan[local].clone());
        }
        if !data.is_empty() {
            self.request("PUT", &path, &[], &json!({"data":data}))?;
        }
        let after = self.task(key, workspace, project)?;
        if after["name"] != plan["title"] || after["notes"] != plan["description"] {
            return Err("vendor: Asana did not confirm the requested fields"
                .to_owned()
                .into());
        }
        Ok(key.to_owned())
    }
}

pub(in crate::vendors) fn execute(client: &mut Client, op: &str, q: &Value) -> Result<Value> {
    let mut get = scoped(q, "imports.binding");
    if q["clear"] == true {
        get["clear_device_credentials"] = json!(true);
    }
    let b = client.execute(get)?;
    if b["provider"] != "asana" {
        return Err("invalid: select an Asana connector".into());
    }
    let space = q["space"].as_str().unwrap_or("default");
    let org = b["organization"]
        .as_str()
        .ok_or("invalid: missing organization")?;
    if matches!(op, "asana.accounts" | "asana.credential") {
        administrator(client, org)?;
    } else if op != "asana.push" {
        administrator(client, space)?;
    }
    if op == "asana.synchronize" && b["sync_paused"] == true {
        return Ok(json!({"state":"paused","batch":null,"signal_revision":b["signal_revision"]}));
    }
    let bound = b["enterprise_id"].as_str().filter(|s| !s.is_empty());
    let chosen = q["workspace_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or(bound);
    if bound.is_some() && chosen != bound {
        return Err("conflict: change the connector workspace before rebinding".into());
    }
    let at = |client: &Client, scope: &str| org_location(client, &b, "asana-workspace", scope);
    if op == "asana.credential" && q["clear"] == true {
        return vault::save(
            &at(client, bound.ok_or("invalid: choose a workspace first")?)?,
            None,
        );
    }
    let token = if matches!(op, "asana.credential" | "asana.accounts")
        && q["access_token"].as_str().is_some_and(|v| !v.is_empty())
    {
        input(q, "access_token")?
    } else {
        input(
            &vault::read(&at(
                client,
                bound.ok_or("invalid: supply a token and discover its workspaces")?,
            )?)?,
            "access_token",
        )?
    };
    let limit = q
        .get("page_size")
        .map(|v| {
            v.as_u64()
                .filter(|n| (1..=100).contains(n))
                .ok_or("invalid: Asana page_size must be 1–100")
        })
        .transpose()?
        .unwrap_or(50) as usize;
    let api = Asana {
        root: format!("{}/api/1.0", origin("https://app.asana.com")?),
        token,
        limit,
    };
    let accounts = api.pages("/workspaces", "name", &[])?;
    if op == "asana.accounts" {
        return Ok(json!({"workspaces":accounts}));
    }
    let workspace = chosen
        .or_else(|| {
            if accounts.len() == 1 {
                accounts[0]["gid"].as_str()
            } else {
                None
            }
        })
        .ok_or("invalid: select a workspace ID when the Asana token sees multiple workspaces")?;
    if !accounts.iter().any(|r| r["gid"] == workspace) {
        return Err("forbidden: Asana token cannot access this workspace".into());
    }
    if op == "asana.credential" {
        if bound.is_none() {
            client.execute(json!({"op":"update","space":space,"kind":"connector","id":q["id"],"expected_rev":b["revision"],"fields":{"enterprise_id":workspace}}))?;
        }
        let mut saved = vault::save(
            &at(client, workspace)?,
            Some(&json!({"access_token":api.token})),
        )?;
        saved["suffix"] = json!(suffix(&api.token));
        saved["workspace"] = json!(workspace);
        return Ok(saved);
    }
    if op == "asana.projects" {
        let rows = api.pages(
            &format!("/workspaces/{workspace}/projects"),
            "name,workspace.gid,archived",
            &[("archived", "false".to_owned())],
        )?;
        if rows
            .iter()
            .any(|p| p["workspace"]["gid"] != workspace || p["archived"] == true)
        {
            return Err("forbidden: Asana returned an out-of-scope project".into());
        }
        return Ok(json!({"workspace":workspace,"projects":rows}));
    }
    let project = gid(&b["project"])?;
    api.project(workspace, project)?;
    if op == "asana.push" {
        if let Some(body) = q.get("body") {
            if q["intent"] != "comment" || q["comment_id"].is_string() || b["vendor_write"] != true
            {
                return Err("invalid: enable writeback and choose a new Asana comment".into());
            }
            let message = body
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 1_048_576)
                .ok_or("invalid: enter a comment")?;
            let request = q["request_id"]
                .as_str()
                .filter(|s| uuid::Uuid::parse_str(s).is_ok())
                .ok_or("invalid: new Asana comments require a stable request UUID")?;
            let task = client
                .execute(json!({"op":"get","space":space,"kind":"task","id":q["task_id"]}))?;
            if task["fields"]["connector"] != q["id"]
                || task["fields"]["external_provider"] != "asana"
                || task["fields"]["visibility"] == "private"
            {
                return Err("forbidden: select a project-linked public Asana task".into());
            }
            let comment=client.execute(json!({"op":"create","kind":"comment","space":space,"title":message.chars().take(60).collect::<String>(),"fields":{"body":message,"task":q["task_id"]},"request_id":format!("asana-{request}")}))?;
            let mut send = q.clone();
            send["comment_id"] = comment["id"].clone();
            let mut receipt =
                publish::with_lease(client, &send, |plan| api.push(plan, workspace, project))
                    .map_err(|e| {
                        format!(
                            "{e}. Local comment {}",
                            comment["id"].as_str().unwrap_or_default()
                        )
                    })?;
            receipt["comment_id"] = comment["id"].clone();
            return Ok(receipt);
        }
        return publish::with_lease(client, q, |plan| api.push(plan, workspace, project));
    }
    let poll = op == "asana.synchronize";
    let mode = if poll {
        "changes"
    } else {
        q["import_mode"].as_str().unwrap_or("history")
    };
    let mut query = vec![
        ("project", project.to_owned()),
        ("completed_since", "1970-01-01T00:00:00Z".to_owned()),
    ];
    if mode == "changes"
        && let Some(time) = b["since"]
            .as_i64()
            .and_then(chrono::DateTime::from_timestamp_millis)
    {
        query.push((
            "modified_since",
            time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ));
    }
    let rows = api.pages("/tasks", TASK_FIELDS, &query)?;
    let mut result = stage_batch(client, q, &b, project, rows, mode, poll)?;
    result["signal_revision"] = b["signal_revision"].clone();
    Ok(result)
}
