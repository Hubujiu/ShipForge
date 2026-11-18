use crate::{
    app::Workbench,
    catalog::Record,
    forms::{Form, Format},
};
use serde_json::{Map, Value, json};
#[cfg(all(windows, test))]
#[path = "assignees_ui.rs"]
mod ui_test;

impl Record {
    pub fn members(&self) -> Vec<&str> {
        if let Some(ids) = self.fields.get("assignees").and_then(Value::as_array) {
            return ids.iter().filter_map(Value::as_str).collect();
        }
        let id = self.field("assignee");
        if !id.is_empty() && !matches!(self.field("assignee_kind"), "agent" | "crew") {
            vec![id]
        } else {
            vec![]
        }
    }
    pub fn executor(&self) -> Option<(&str, &str)> {
        if let Some(value) = self.fields.get("executor") {
            return Some((value["kind"].as_str()?, value["id"].as_str()?));
        }
        let kind = self.field("assignee_kind");
        (matches!(kind, "agent" | "crew") && !self.field("assignee").is_empty())
            .then_some((kind, self.field("assignee")))
    }
    pub fn grouped_values<'a>(&'a self, group: &str) -> Vec<&'a str> {
        if group != "assignee" {
            return vec![self.field(group)];
        }
        let people = self.members();
        if people.is_empty() { vec![""] } else { people }
    }
    pub fn matches_filter(&self, key: &str, id: &str) -> bool {
        if key == "assignee" {
            self.members().contains(&id) || self.executor().is_some_and(|(_, worker)| worker == id)
        } else {
            self.field(key) == id
        }
    }
}

pub fn initial(record: &Record, fields: &mut Map<String, Value>) {
    fields.insert("assignees".into(), json!(record.members()));
    fields.insert(
        "executor".into(),
        json!(record.executor().map(|(kind, id)| format!("{kind}:{id}"))),
    );
}
pub fn compile(fields: &mut Map<String, Value>) -> Result<(), String> {
    if let Some(value) = fields.get_mut("assignees")
        && !value.is_array()
    {
        *value = match value.as_str() {
            Some(id) if !id.is_empty() => json!([id]),
            _ => json!([]),
        };
    }
    if let Some(value) = fields.get_mut("executor")
        && !value.is_null()
    {
        let (kind, id) = value
            .as_str()
            .and_then(|s| s.split_once(':'))
            .ok_or("请选择 Agent 或小队")?;
        *value = json!({"kind":kind,"id":id});
    }
    Ok(())
}
impl Workbench {
    pub fn configure_assignees(&self, form: &mut Form, current: &Map<String, Value>) {
        let multi = self
            .find("space", &self.loc.space)
            .is_some_and(|s| s.fields.get("allow_multiple_assignees") == Some(&json!(true)))
            || current
                .get("assignees")
                .and_then(Value::as_array)
                .is_some_and(|members| members.len() > 1);
        if !multi && let Some(field) = form.entries.iter_mut().find(|e| e.spec.key == "assignees") {
            field.spec.format = Format::Ref("member");
        }
    }
    pub fn executor_choices(&self) -> Vec<(String, String)> {
        ["agent", "crew"]
            .into_iter()
            .flat_map(|kind| {
                self.list(kind).iter().map(move |r| {
                    (
                        format!("{kind}:{}", r.id),
                        format!(
                            "{} · {}",
                            r.title,
                            crate::locale::label(if kind == "agent" { "Agent" } else { "小队" })
                        ),
                    )
                })
            })
            .collect()
    }
    pub fn member_names(&self, record: &Record) -> String {
        let mut names: Vec<String> = record
            .members()
            .iter()
            .map(|id| self.name("member", id))
            .collect();
        if let Some(external) = record
            .fields
            .get("source_assignees")
            .and_then(Value::as_array)
        {
            names.extend(
                external
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|id| format!("{} · {id}", crate::locale::label("外部成员"))),
            );
        }
        if names.is_empty() {
            return crate::locale::label("未指派").into();
        }
        names.join(", ")
    }
    pub fn executor_name(&self, record: &Record) -> String {
        record
            .executor()
            .map(|(kind, id)| self.name(kind, id))
            .unwrap_or_else(|| "—".into())
    }
    pub fn handlers_summary(&self, record: &Record) -> String {
        let members = self.member_names(record);
        if record.executor().is_none() {
            return members;
        }
        let worker = self.executor_name(record);
        if record.members().is_empty()
            && record
                .fields
                .get("source_assignees")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
        {
            return worker;
        }
        format!("{members} · {worker}")
    }
}
