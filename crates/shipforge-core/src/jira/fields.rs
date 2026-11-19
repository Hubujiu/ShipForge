use super::{Site, visible};
use crate::{Context, Record, Result, invalid, required};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

pub(super) const PRIORITIES: [(&str, &str, &str); 5] = [
    ("1", "Highest", "urgent"),
    ("2", "High", "high"),
    ("3", "Medium", "medium"),
    ("4", "Low", "low"),
    ("5", "Lowest", "lowest"),
];
pub(super) fn priorities() -> Vec<Value> {
    PRIORITIES
        .iter()
        .map(|(id, name, _)| json!({"id":id,"name":name}))
        .collect()
}
pub(super) fn reference<'a>(object: &'a Value, names: &[&str]) -> Result<&'a str> {
    names
        .iter()
        .find_map(|k| object.get(k)?.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| invalid(&format!("expected an object with {}", names.join(" or "))))
}
pub(super) fn project_key(space: &Record) -> &str {
    space.fields["key"].as_str().unwrap_or("SF")
}
pub(super) fn project(space: &Record) -> Value {
    json!({"id":space.id,"key":project_key(space),"name":space.title,"projectTypeKey":"software","simplified":false})
}
pub(super) fn issue_type(r: &Record) -> Value {
    json!({"id":r.id,"name":r.title,"subtask":false})
}
pub(super) fn status(r: &Record) -> Value {
    let (id, key, name, color) = match r.fields["category"].as_str() {
        Some("in_progress") => (4, "indeterminate", "In Progress", "yellow"),
        Some("done" | "cancelled" | "duplicate") => (3, "done", "Done", "green"),
        _ => (2, "new", "To Do", "blue-gray"),
    };
    json!({"id":r.id,"name":r.title,"statusCategory":{"id":id,"key":key,"name":name,"colorName":color}})
}
pub(super) fn account(id: &str, name: &str) -> Value {
    json!({"accountId":id,"key":id,"name":id,"displayName":name,"active":true,"accountType":"atlassian"})
}
pub(super) fn user(c: &Context<'_>, id: &str) -> Result<Value> {
    if let Some(user) = c.find("member", id)?
        && c.visible(&user)?
    {
        return Ok(account(id, &user.title));
    }
    let org_id = c.org_id()?;
    if org_id != c.space {
        let org = Context {
            space: &org_id,
            ..*c
        };
        if let Some(user) = org.find("member", id)?
            && org.visible(&user)?
        {
            return Ok(account(id, &user.title));
        }
    }
    Ok(account(id, id))
}
pub(super) fn timestamp(millis: i64) -> String {
    Utc.timestamp_millis_opt(millis)
        .single()
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%S%.3f%z")
        .to_string()
}
fn rich(plain: &str) -> Value {
    json!({"type":"doc","version":1,"content":plain.split('\n').map(|line|if line.is_empty(){json!({"type":"paragraph","content":[]})}else{json!({"type":"paragraph","content":[{"type":"text","text":line}]})}).collect::<Vec<_>>()})
}
pub(super) fn plain(value: &Value) -> Result<String> {
    if let Some(s) = value.as_str() {
        return Ok(s.into());
    }
    if value["type"] != "doc" || value["version"] != 1 {
        return Err(invalid("expected plain text or an ADF v1 document"));
    }
    fn text(node: &Value, depth: u8) -> Result<String> {
        if depth > 40 {
            return Err(invalid("ADF document is too deeply nested"));
        }
        let kind = required(node, "type")?;
        match kind {
            "text" => node["text"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("ADF text node requires text")),
            "hardBreak" => Ok("\n".into()),
            "mention" => reference(&node["attrs"], &["text", "id"]).map(str::to_owned),
            "emoji" => reference(&node["attrs"], &["text", "shortName"]).map(str::to_owned),
            "inlineCard" | "blockCard" => reference(&node["attrs"], &["url"]).map(str::to_owned),
            "rule" => Ok("---".into()),
            _ => {
                let children = node["content"]
                    .as_array()
                    .ok_or_else(|| invalid("unsupported ADF node without textual content"))?;
                let separator = if matches!(kind, "paragraph" | "heading" | "codeBlock") {
                    ""
                } else {
                    "\n"
                };
                let items = children
                    .iter()
                    .map(|n| text(n, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                Ok(items.join(separator))
            }
        }
    }
    text(value, 0)
}
fn content(row: &Record, key: &str, version: u64) -> Value {
    let Some(value) = row.fields[key].as_str() else {
        return Value::Null;
    };
    if version == 2 {
        return json!(value);
    }
    let adf = &row.fields[format!("jira_{key}_adf")];
    if adf.is_object() && plain(adf).as_deref() == Ok(value) {
        return adf.clone();
    }
    rich(value)
}
impl Site<'_, '_> {
    pub(super) fn render(&self, space: &Record, t: &Record) -> Result<Value> {
        let c = self.scope(space);
        let f = &t.fields;
        let metadata = |kind: &str, id: &str, format: fn(&Record) -> Value| -> Result<Value> {
            if let Some(record) = c.find(kind, id)?
                && c.visible(&record)?
            {
                Ok(format(&record))
            } else {
                Ok(Value::Null)
            }
        };
        let state = metadata("status", required(f, "status")?, status)?;
        let ty = metadata("task_type", required(f, "type")?, issue_type)?;
        let priority = PRIORITIES
            .iter()
            .find(|(_, _, p)| f["priority"] == *p)
            .map(|(id, name, _)| json!({"id":id,"name":name}));
        let people = crate::task_commands::assignees::people(f)
            .map(|id| user(&c, id))
            .collect::<Result<Vec<_>>>()?;
        let reporter = user(&c, required(f, "creator")?)?;
        let mut labels = Vec::new();
        for value in f["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            match crate::tags::accessible(&c, "tag", value) {
                Ok(tag) => labels.push(tag.title),
                Err(e) if e.starts_with("not_found:") || e.starts_with("forbidden:") => {}
                Err(e) => return Err(e),
            }
        }
        let mut issue = self.identity(t);
        issue["fields"] = json!({"summary":t.title,"description":content(t,"description",self.version),"issuetype":ty,"project":project(space),
        "priority":priority,"resolution":if state["statusCategory"]["key"]=="done"{json!({"id":"1","name":"Done"})}else{Value::Null},
        "status":state,"assignee":people.first(),"reporter":reporter,"creator":reporter,"labels":labels,"created":timestamp(t.created_at),"updated":timestamp(t.updated_at),"duedate":f["due_date"]});
        if let Some(id) = f["parent"].as_str()
            && let Some(parent) = c.find("task", id)?
            && c.visible(&parent)?
        {
            let mut data = self.identity(&parent);
            data["fields"] = json!({"summary":parent.title});
            issue["fields"]["parent"] = data;
        }
        Ok(issue)
    }
    pub(super) fn render_comment(
        &self,
        c: &Context<'_>,
        task: &Record,
        comment: &Record,
    ) -> Result<Value> {
        let author = user(c, required(&comment.fields, "creator")?)?;
        Ok(
            json!({"id":comment.id,"self":format!("{}/comment/{}",self.identity(task)["self"].as_str().unwrap(),comment.id),
            "author":author,"updateAuthor":author,"body":content(comment,"body",self.version),
            "created":timestamp(comment.created_at),"updated":timestamp(comment.updated_at)}),
        )
    }
}
pub(super) fn resolve(c: &Context<'_>, kind: &str, object: &Value) -> Result<String> {
    let key = reference(object, &["id", "name"])?;
    let mut rows = visible(c, kind)?
        .into_iter()
        .filter(|r| r.id.eq_ignore_ascii_case(key) || r.title.eq_ignore_ascii_case(key));
    let first = rows
        .next()
        .ok_or_else(|| invalid(&format!("unknown {kind}")))?;
    if rows.next().is_some() {
        return Err(invalid(&format!("ambiguous {kind} name")));
    }
    Ok(first.id)
}
const CATALOG: &[(&str, &str, &str)] = &[
    ("project", "Project", "project"),
    ("summary", "Summary", "string"),
    ("description", "Description", "string"),
    ("issuetype", "Issue Type", "issuetype"),
    ("status", "Status", "status"),
    ("resolution", "Resolution", "resolution"),
    ("priority", "Priority", "priority"),
    ("assignee", "Assignee", "user"),
    ("reporter", "Reporter", "user"),
    ("creator", "Creator", "user"),
    ("labels", "Labels", "array"),
    ("duedate", "Due date", "date"),
    ("parent", "Parent", "issuelink"),
    ("comment", "Comment", "comments-page"),
    ("created", "Created", "datetime"),
    ("updated", "Updated", "datetime"),
];
pub(super) fn field_catalog() -> Value {
    json!(CATALOG.iter().map(|(id,name,ty)|json!({"id":id,"key":id,"name":name,"custom":false,"clauseNames":[id],"navigable":true,"searchable":true,"schema":{"type":ty,"system":id}})).collect::<Vec<_>>())
}
pub(super) fn create_fields() -> Value {
    let mut rows = json!({});
    for (id, name, ty) in CATALOG.iter().filter(|(id, _, _)| {
        matches!(
            *id,
            "project"
                | "summary"
                | "issuetype"
                | "description"
                | "priority"
                | "assignee"
                | "labels"
                | "duedate"
                | "parent"
        )
    }) {
        rows[*id] = json!({"name":name,"required":matches!(*id,"project"|"summary"|"issuetype"),"schema":{"type":ty,"system":id},"operations":["set"]});
    }
    rows
}
