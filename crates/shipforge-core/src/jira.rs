//! Jira-compatible views over the same organization permissions and domain writes as the native clients.
mod fields;
mod jql;
mod write;

use crate::{Context, Record, Result, conflict, invalid, required};
use serde_json::{Value, json};
use std::collections::BTreeMap;

struct Site<'a, 'db> {
    org: &'a Context<'db>,
    spaces: Vec<Record>,
    version: u64,
    base: &'a str,
}

fn hidden() -> String {
    "not_found: project or issue is unavailable".into()
}
fn method() -> String {
    "method: HTTP method is not available for this resource".into()
}

pub(crate) fn execute(c: &Context<'_>, q: &Value, read: bool) -> Result<Value> {
    if c.find("organization", c.space)?.is_none() {
        return Err(hidden());
    }
    c.role().map_err(|e| {
        if e.starts_with("forbidden:") {
            hidden()
        } else {
            e
        }
    })?;
    let version = q["api_version"]
        .as_u64()
        .filter(|v| matches!(v, 2 | 3))
        .ok_or_else(|| invalid("api_version must be 2 or 3"))?;
    let verb = q["method"].as_str().unwrap_or("GET");
    if read != (verb == "GET") {
        return Err(method());
    }
    let resource = required(q, "resource")?;
    if resource.len() > 1000 {
        return Err(invalid("resource is too long"));
    }
    let mut spaces = Vec::new();
    for space in c.all_spaces()? {
        if space.fields["organization"].as_str().unwrap_or("default") != c.space {
            continue;
        }
        let scope = Context {
            space: &space.id,
            ..*c
        };
        match scope.role() {
            Ok(_) => spaces.push(space),
            Err(e) if e.starts_with("forbidden:") => {}
            Err(e) => return Err(e),
        }
    }
    let site = Site {
        org: c,
        spaces,
        version,
        base: q["base_url"].as_str().unwrap_or("").trim_end_matches('/'),
    };
    let path: Vec<_> = resource.trim_matches('/').split('/').collect();
    let params = q.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(invalid("params must be an object"));
    }
    let payload = &q["payload"];
    let mut status = 200;
    let body = match (verb, path.as_slice()) {
        ("GET", ["myself"]) => fields::user(c, c.actor)?,
        ("GET", ["serverInfo"]) => {
            json!({"baseUrl":site.base,"serverTitle":"ShipForge","version":"9.0.0","versionNumbers":[9,0,0],"deploymentType":"Server","buildNumber":900000,"serverTime":fields::timestamp(crate::now())})
        }
        ("GET", ["project"]) => Value::Array(site.spaces.iter().map(fields::project).collect()),
        ("GET", ["project", key]) => site.project_detail(site.project(key)?)?,
        ("GET", ["issue", "createmeta"]) => site.createmeta(&params)?,
        ("GET", ["search"]) => site.search(&params)?,
        ("GET", ["user", "search"] | ["user", "assignable", "search"]) => site.users(&params)?,
        ("GET", [kind @ ("priority" | "resolution" | "status" | "issuetype" | "field")]) => {
            site.metadata(kind, &params)?
        }
        ("POST", ["issue"]) => {
            if payload["fields"]["issuetype"].is_null() {
                return Err(invalid("issuetype is required"));
            }
            let project = fields::reference(&payload["fields"]["project"], &["key", "id"])?;
            let space = site.project(project)?;
            let task = write::issue(&site, space, None, payload, None)?;
            status = 201;
            site.identity(&task)
        }
        ("GET", ["issue", key, "comment", id]) => {
            let (space, task) = site.issue(key)?;
            let c = site.scope(space);
            let comment = c.readable("comment", id)?;
            if comment.fields["task"] != task.id {
                return Err(hidden());
            }
            site.render_comment(&c, &task, &comment)?
        }
        (_, ["issue", key] | ["issue", key, "comment" | "transitions"]) => {
            let (space, task) = site.issue(key)?;
            let scope = site.scope(space);
            match (verb, path.get(2).copied()) {
                ("GET", None) => {
                    let mut result = site.render(space, &task)?;
                    if wants_comments(&params, true)? {
                        result["fields"]["comment"] = site.comments(&scope, &task, &json!({}))?;
                    }
                    filter_fields(&mut result, &params)?;
                    result
                }
                ("PUT", None) => {
                    write::issue(&site, space, Some(&task), payload, None)?;
                    status = 204;
                    Value::Null
                }
                ("DELETE", None) => {
                    write::apply(&scope, json!({"op":"delete","kind":"task","id":task.id}))?;
                    status = 204;
                    Value::Null
                }
                ("GET", Some("comment")) => site.comments(&scope, &task, &params)?,
                ("POST", Some("comment")) => {
                    status = 201;
                    write::comment(&site, &scope, &task, payload)?
                }
                ("GET", Some("transitions")) => {
                    json!({"transitions":site.transitions(&scope,&task)?})
                }
                ("POST", Some("transitions")) => {
                    scope.authorize_write("task", Some(&task))?;
                    let value = &payload["transition"]["id"];
                    let id = value
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| value.as_u64().map(|id| id.to_string()))
                        .ok_or_else(|| invalid("transition id is required"))?;
                    if !site
                        .transitions(&scope, &task)?
                        .iter()
                        .any(|s| s["id"] == id)
                    {
                        return Err(invalid("transition is unavailable for this issue"));
                    }
                    write::issue(&site, space, Some(&task), payload, Some(&id))?;
                    status = 204;
                    Value::Null
                }
                _ => return Err(method()),
            }
        }
        (_, ["issue" | "myself" | "serverInfo" | "search" | "project"]) => return Err(method()),
        _ => return Err("not_found: unknown Jira resource".into()),
    };
    Ok(json!({"status":status,"body":body}))
}

impl<'a, 'db> Site<'a, 'db> {
    fn scope<'b>(&'b self, space: &'b Record) -> Context<'b> {
        Context {
            db: self.org.db,
            actor: self.org.actor,
            space: &space.id,
        }
    }
    fn project(&self, key: &str) -> Result<&Record> {
        if let Some(space) = self.spaces.iter().find(|s| s.id == key) {
            return Ok(space);
        }
        let mut candidates = self
            .spaces
            .iter()
            .filter(|s| fields::project_key(s).eq_ignore_ascii_case(key));
        let project = candidates.next().ok_or_else(hidden)?;
        if candidates.next().is_some() {
            return Err(conflict(
                "multiple visible spaces use this project key; configure distinct space keys",
            ));
        }
        Ok(project)
    }
    fn issue(&self, key: &str) -> Result<(&Record, Record)> {
        let Some((prefix, number)) = key.rsplit_once('-') else {
            return Err(invalid("use a full project issue key, for example TEAM-42"));
        };
        if prefix.is_empty() || number.parse::<u64>().ok().filter(|n| *n > 0).is_none() {
            return Err(invalid("use a full project issue key, for example TEAM-42"));
        }
        let mut found = None;
        for space in &self.spaces {
            let scope = self.scope(space);
            let mut query=scope.db.prepare("SELECT id FROM records WHERE space=?1 AND kind='task' AND json_extract(fields,'$.number')=?2 COLLATE NOCASE").map_err(crate::storage)?;
            for id in query
                .query_map(rusqlite::params![scope.space, key], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(crate::storage)?
            {
                let task = scope.get("task", &id.map_err(crate::storage)?)?;
                if scope.visible(&task)? {
                    if found.is_some() {
                        return Err(conflict(
                            "multiple visible issues use this key; configure distinct space keys",
                        ));
                    }
                    found = Some((space, task));
                }
            }
        }
        found.ok_or_else(hidden)
    }
    fn identity(&self, task: &Record) -> Value {
        let key = task.fields["number"].as_str().unwrap_or("");
        json!({"key":key,"id":key.rsplit('-').next().unwrap_or(key),"self":format!("{}/rest/api/{}/issue/{}",self.base,self.version,key)})
    }
    fn transitions(&self, c: &Context<'_>, task: &Record) -> Result<Vec<Value>> {
        if c.authorize_write("task", Some(task)).is_err() {
            return Ok(vec![]);
        }
        let kind = c.readable("task_type", required(&task.fields, "type")?)?;
        let allowed = kind.fields["statuses"].as_array().filter(|v| !v.is_empty());
        let triage = c.get("space", c.space)?.fields["triage_enabled"] == true;
        let mut opts = Vec::new();
        for record in visible(c, "status")? {
            if record.id == task.fields["status"]
                || !triage && record.fields["category"] == "triage"
                || allowed.is_some_and(|v| !v.iter().any(|id| id == &record.id))
            {
                continue;
            }
            opts.push(json!({"id":record.id,"name":record.title,"to":fields::status(&record),"hasScreen":false}));
        }
        Ok(opts)
    }
    fn comments(&self, c: &Context<'_>, task: &Record, params: &Value) -> Result<Value> {
        let mut items = Vec::new();
        for row in c.records("comment")? {
            if row.fields["task"] == task.id && c.visible(&row)? {
                items.push(row);
            }
        }
        items.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        let (start, max) = page(params)?;
        let total = items.len();
        let items = items
            .into_iter()
            .skip(start)
            .take(max)
            .map(|r| self.render_comment(c, task, &r))
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"startAt":start,"maxResults":max,"total":total,"comments":items}))
    }
    fn search(&self, params: &Value) -> Result<Value> {
        let query = jql::parse(param(params, "jql", "")?, self.org.actor)?;
        let (start, max) = page(params)?;
        let mut matches = Vec::new();
        let comments = wants_comments(params, false)?;
        for space in &self.spaces {
            let c = self.scope(space);
            for task in visible(&c, "task")? {
                let issue = self.render(space, &task)?;
                if query.matches(&issue) {
                    matches.push((space, task.id, issue));
                }
            }
        }
        matches.sort_by(|a, b| query.compare(&a.2, &b.2));
        let total = matches.len();
        let mut issues = Vec::new();
        for (space, id, mut issue) in matches.into_iter().skip(start).take(max) {
            if comments {
                let scope = self.scope(space);
                issue["fields"]["comment"] =
                    self.comments(&scope, &scope.get("task", &id)?, &json!({}))?;
            }
            filter_fields(&mut issue, params)?;
            issues.push(issue);
        }
        Ok(json!({"startAt":start,"maxResults":max,"total":total,"issues":issues}))
    }
    fn metadata(&self, kind: &str, params: &Value) -> Result<Value> {
        match kind {
            "field" => return Ok(fields::field_catalog()),
            "priority" => return Ok(json!(fields::priorities())),
            "resolution" => return Ok(json!([{"id":"1","name":"Done"}])),
            _ => {}
        }
        let mut rows = BTreeMap::new();
        let scopes = if let Some(key) = params["project"].as_str() {
            vec![self.project(key)?]
        } else {
            self.spaces.iter().collect()
        };
        for space in scopes {
            for r in visible(
                &self.scope(space),
                if kind == "status" {
                    "status"
                } else {
                    "task_type"
                },
            )? {
                if r.fields["enabled"] != false {
                    rows.entry(r.id.clone()).or_insert_with(|| {
                        if kind == "status" {
                            fields::status(&r)
                        } else {
                            fields::issue_type(&r)
                        }
                    });
                }
            }
        }
        Ok(Value::Array(rows.into_values().collect()))
    }
    fn project_detail(&self, space: &Record) -> Result<Value> {
        let mut result = fields::project(space);
        let types = visible(&self.scope(space), "task_type")?;
        result["issueTypes"] = json!(
            types
                .iter()
                .filter(|r| r.fields["enabled"] != false)
                .map(fields::issue_type)
                .collect::<Vec<_>>()
        );
        Ok(result)
    }
    fn createmeta(&self, params: &Value) -> Result<Value> {
        let keys = param(params, "projectKeys", "")?;
        let ids = param(params, "projectIds", "")?;
        let mut projects = Vec::new();
        for space in &self.spaces {
            if (!keys.is_empty()
                && !keys
                    .split(',')
                    .any(|k| k.eq_ignore_ascii_case(fields::project_key(space))))
                || (!ids.is_empty() && !ids.split(',').any(|k| k == space.id))
            {
                continue;
            }
            let mut data = self.project_detail(space)?;
            let mut types = data.as_object_mut().unwrap().remove("issueTypes").unwrap();
            for ty in types.as_array_mut().unwrap() {
                ty["fields"] = fields::create_fields();
            }
            data["issuetypes"] = types;
            projects.push(data);
        }
        Ok(json!({"projects":projects,"expand":"projects"}))
    }
    fn users(&self, params: &Value) -> Result<Value> {
        let query = param(params, "query", param(params, "username", "")?)?.to_lowercase();
        let (start, max) = page(params)?;
        let scopes = if let Some(key) = params["project"].as_str() {
            vec![self.project(key)?]
        } else {
            self.spaces.iter().collect()
        };
        let mut users = BTreeMap::new();
        for space in scopes {
            let c = self.scope(space);
            for user in visible(&c, "member")? {
                if user.id.to_lowercase().contains(&query)
                    || user.title.to_lowercase().contains(&query)
                {
                    users
                        .entry(user.id.clone())
                        .or_insert_with(|| fields::account(&user.id, &user.title));
                }
            }
        }
        Ok(Value::Array(
            users.into_values().skip(start).take(max).collect(),
        ))
    }
}

fn visible(c: &Context<'_>, kind: &str) -> Result<Vec<Record>> {
    let mut result = Vec::new();
    for r in c.records(kind)? {
        if c.visible(&r)? {
            result.push(r);
        }
    }
    Ok(result)
}
fn param<'a>(p: &'a Value, name: &str, default: &'a str) -> Result<&'a str> {
    p.get(name).map_or(Ok(default), |v| {
        v.as_str()
            .ok_or_else(|| invalid(&format!("{name} must be text")))
    })
}
fn page(p: &Value) -> Result<(usize, usize)> {
    let n = |k: &str, d: usize| -> Result<usize> {
        match p.get(k) {
            None => Ok(d),
            Some(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .or_else(|| v.as_str()?.parse::<usize>().ok())
                .ok_or_else(|| invalid(&format!("{k} must be a nonnegative integer"))),
        }
    };
    Ok((n("startAt", 0)?, n("maxResults", 50)?.min(100)))
}
fn wants_comments(params: &Value, detail: bool) -> Result<bool> {
    let raw = param(params, "fields", if detail { "*all" } else { "*navigable" })?;
    let mut names = raw.split(',').map(str::trim);
    Ok(!names.clone().any(|f| f == "-comment")
        && (names.clone().any(|f| matches!(f, "*all" | "comment"))
            || detail && names.all(|f| f.starts_with('-'))))
}
fn filter_fields(issue: &mut Value, params: &Value) -> Result<()> {
    let Some(raw) = params.get("fields") else {
        return Ok(());
    };
    let fields = raw
        .as_str()
        .ok_or_else(|| invalid("fields must be a comma-separated list"))?
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>();
    let all = fields.contains(&"*all")
        || fields.contains(&"*navigable")
        || fields.iter().all(|k| k.starts_with('-'));
    issue["fields"].as_object_mut().unwrap().retain(|key, _| {
        (all || fields.contains(&key.as_str()))
            && !fields
                .iter()
                .any(|k| k.strip_prefix('-') == Some(key.as_str()))
    });
    Ok(())
}
pub(crate) fn schema(schema: &mut Value) {
    schema["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":"jira.read"}}},"then":{"properties":{"method":{"const":"GET"}}}}));
    schema["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"enum":["jira.read","jira.write"]}}},"then":{"required":["space","api_version","resource"],"properties":{"api_version":{"enum":[2,3]},"resource":{"type":"string","minLength":1},"params":{"type":"object"},"base_url":{"type":"string"}}}}));
    schema["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":"jira.write"}}},"then":{"required":["method"],"properties":{"method":{"enum":["POST","PUT","DELETE"]}}}}));
}
