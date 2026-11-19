use super::{After, UiAction, Workbench};
use crate::{
    appearance::paint as rgb,
    catalog::{Page, Record, STATUSES},
    controls::*,
    forms::{Control, Form, Mode, Selection},
    locale,
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};

pub(super) fn handles(op: &str) -> bool {
    matches!(
        op,
        "status.library.save" | "status.library.removal" | "task_type.workflow.save"
    )
}
fn reference(key: &str) -> Value {
    if let Some(id) = key.strip_prefix("org/") {
        json!({"library":id})
    } else if let Some(id) = key.strip_prefix("space/") {
        json!({"local":id})
    } else {
        Value::Null
    }
}
fn nonterminal(row: &Record) -> bool {
    !matches!(row.field("category"), "done" | "cancelled" | "duplicate")
}

fn chosen<'a>(form: &'a Form, key: &str) -> Option<&'a str> {
    form.entries
        .iter()
        .find(|e| e.spec.key == key)
        .and_then(|e| match &e.control {
            Control::Pick {
                value: Selection::One(v),
                ..
            } => Some(v.as_str()),
            _ => None,
        })
}
impl Workbench {
    pub(crate) fn status_task_groups<'a>(
        &self,
        tasks: &'a [Record],
    ) -> Vec<(Option<String>, Vec<&'a Record>)> {
        let mut by_id = std::collections::BTreeMap::<String, Vec<&Record>>::new();
        for task in tasks {
            by_id
                .entry(task.field("status").to_owned())
                .or_default()
                .push(task);
        }
        let mut groups = Vec::new();
        for status in self.status_rows(&self.loc.task_type) {
            if let Some(rows) = by_id.remove(&status.id) {
                groups.push((Some(status.id.clone()), rows));
            }
        }
        groups.extend(by_id.into_iter().map(|(id, rows)| (Some(id), rows)));
        groups
    }
    pub(crate) fn status_rows(&self, ty: &str) -> Vec<&Record> {
        let mut rows = self.list("status").iter().collect::<Vec<_>>();
        if let Some(ids) = self
            .find("task_type", ty)
            .and_then(|r| r.fields.get("statuses"))
            .and_then(Value::as_array)
            .filter(|ids| !ids.is_empty())
        {
            rows.retain(|r| ids.iter().any(|v| v == &r.id));
            rows.sort_by_key(|r| ids.iter().position(|v| v == &r.id));
        } else {
            rows.sort_by_key(|r| {
                (
                    r.fields.get("order").and_then(Value::as_i64).unwrap_or(0),
                    r.id.as_str(),
                )
            });
        }
        rows
    }
    pub(crate) fn status_category<'a>(&'a self, id: &'a str) -> &'a str {
        self.find("status", id).map_or(id, |r| r.field("category"))
    }
    pub(crate) fn status_choices(&self, ty: &str) -> Vec<(String, String)> {
        self.status_rows(ty)
            .into_iter()
            .map(|r| (r.id.clone(), r.localized_title().to_owned()))
            .collect()
    }
    pub(crate) fn task_form_statuses(&self) -> Vec<(String, String)> {
        let ty = self
            .form
            .as_ref()
            .and_then(|f| chosen(f, "type"))
            .unwrap_or("");
        let triage = self
            .find("space", &self.loc.space)
            .is_some_and(|s| s.fields.get("triage_enabled") == Some(&Value::Bool(true)));
        self.status_rows(ty)
            .into_iter()
            .filter(|s| s.field("category") != "triage" || triage)
            .map(|r| (r.id.clone(), r.localized_title().to_owned()))
            .collect()
    }
    pub(crate) fn pick_task_initial(&self, form: &mut Form, changed: bool) {
        let ty = chosen(form, "type").unwrap_or("");
        let rows = self.status_rows(ty);
        if rows.is_empty() {
            return;
        }
        let current = chosen(form, "status").unwrap_or("");
        let new_task = matches!(&form.mode,Mode::Create(k) if k=="task");
        if (!changed || !new_task) && rows.iter().any(|r| r.id == current) {
            return;
        }
        let ty_default = self
            .find("task_type", ty)
            .map(|r| r.field("default_status"));
        let triage = self
            .find("space", &self.loc.space)
            .is_some_and(|s| s.fields.get("triage_enabled") == Some(&Value::Bool(true)));
        let mut candidates = rows
            .iter()
            .copied()
            .filter(|r| nonterminal(r) && (triage || r.field("category") != "triage"))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|r| {
            if Some(r.id.as_str()) == ty_default {
                0
            } else if r.fields.get("default") == Some(&Value::Bool(true)) {
                1
            } else {
                2
            }
        });
        let id = if self.loc.page == Page::Triage && new_task {
            rows.iter()
                .copied()
                .find(|r| r.field("category") == "triage")
                .or(candidates.first().copied())
        } else {
            candidates.first().copied()
        };
        if let Some(entry) = form.entries.iter_mut().find(|e| e.spec.key == "status")
            && let Control::Pick { value, .. } = &mut entry.control
        {
            *value = Selection::One(id.map_or_else(String::new, |r| r.id.clone()));
        }
    }
    pub(crate) fn status_color(&self, r: &Record) -> u32 {
        r.field("color")
            .strip_prefix('#')
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .or_else(|| {
                STATUSES
                    .iter()
                    .find(|(id, _, _)| *id == r.field("category"))
                    .map(|(_, _, v)| *v)
            })
            .unwrap_or(VIOLET)
    }
    fn workflow_key(&self, local: &str) -> String {
        self.list("status_library")
            .iter()
            .find(|r| r.field("local_id") == local)
            .map_or_else(|| format!("space/{local}"), |r| format!("org/{}", r.id))
    }
    pub(crate) fn workflow_choices(&self, source: &str) -> Vec<(String, String)> {
        let target = source == "$org_status_targets";
        let initial = source == "$workflow_defaults";
        let current = match self.form.as_ref().map(|f| &f.mode) {
            Some(Mode::Workflow { record, .. }) => record.id.as_str(),
            _ => "",
        };
        let mut rows = self
            .list("status_library")
            .iter()
            .filter(|r| (!target || r.id != current) && (!initial || nonterminal(r)))
            .map(|r| {
                (
                    if target {
                        r.id.clone()
                    } else {
                        format!("org/{}", r.id)
                    },
                    r.localized_title().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        if !target {
            rows.extend(
                self.status_rows("")
                    .into_iter()
                    .filter(|r| {
                        (!initial || nonterminal(r))
                            && !self
                                .list("status_library")
                                .iter()
                                .any(|s| s.field("local_id") == r.id)
                    })
                    .map(|r| {
                        (
                            format!("space/{}", r.id),
                            format!("{} · {}", r.localized_title(), locale::label("仅此空间")),
                        )
                    }),
            );
        }
        rows
    }
    pub(super) fn status_prefill(&self, op: &str, r: &Record) -> Map<String, Value> {
        let mut fields = Map::new();
        match op {
            "status.library.save" if !r.id.is_empty() => {
                fields.extend(r.fields.clone());
                fields.insert("title".into(), json!(r.title));
                let names = r.fields.get("names");
                for (k, lang) in [("zh", "zh-CN"), ("en", "en")] {
                    let v = names
                        .and_then(Value::as_object)
                        .and_then(|m| m.iter().find(|(l, _)| l.eq_ignore_ascii_case(lang)))
                        .and_then(|(_, v)| v.as_str());
                    if let Some(v) = v {
                        fields.insert(k.into(), json!(v));
                    }
                }
            }
            "task_type.workflow.save" => {
                let selected = r
                    .fields
                    .get("statuses")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|local| self.workflow_key(local))
                    .collect::<Vec<_>>();
                fields.insert("statuses".into(), json!(selected));
                if !r.field("default_status").is_empty() {
                    fields.insert(
                        "default_status".into(),
                        json!(self.workflow_key(r.field("default_status"))),
                    );
                }
            }
            _ => {}
        }
        fields
    }
    pub(super) fn submit_status(&mut self, op: &str, record: &Record, values: Map<String, Value>) {
        let v = Value::Object(values);
        let mut q = json!({"op":op,"space":self.loc.space});
        if !record.id.is_empty() {
            q["id"] = json!(record.id);
            q["expected_rev"] = json!(record.rev);
        }
        let after = match op {
            "status.library.save" => {
                q["title"] = v["title"].clone();
                let mut fields = json!({});
                for k in ["category", "color", "collapsed", "order", "default"] {
                    fields[k] = v[k].clone();
                }
                let mut names = record
                    .fields
                    .get("names")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for (k, lang) in [("zh", "zh-CN"), ("en", "en")] {
                    names.retain(|l, _| !l.eq_ignore_ascii_case(lang));
                    if let Some(name) = v[k].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                        names.insert(lang.to_owned(), json!(name));
                    }
                }
                if !names.is_empty() || record.fields.contains_key("names") {
                    fields["names"] = Value::Object(names);
                }
                q["fields"] = fields;
                After::Notice("组织状态已保存".into())
            }
            "task_type.workflow.save" => {
                q["statuses"] = json!(
                    v["statuses"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(reference)
                        .collect::<Vec<_>>()
                );
                q["default_status"] = v["default_status"].as_str().map_or(Value::Null, reference);
                After::Notice("类型工作流已保存".into())
            }
            _ => {
                if let Some(target) = v["target"].as_str().filter(|s| !s.is_empty()) {
                    q["target"] = json!(target);
                }
                After::StatusRemoval
            }
        };
        self.send(q, after, op != "status.library.removal");
    }
    pub(super) fn status_removal_reply(
        &mut self,
        value: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let usage = &value["usage"];
        let totals = format!(
            "{} {} · {} {}",
            usage["tasks"],
            locale::label("任务"),
            usage["workflows"],
            locale::label("类型工作流")
        );
        if value["can_remove"] != true {
            let msg = match value["reason"].as_str() {
                Some("organization_default") => "请先选择其他组织默认状态",
                Some("last_in_category") => "每个类别至少保留一个状态",
                Some("terminal_default") => "请选择非终态迁入状态",
                Some("triage_disabled") => "涉及的空间未启用分拣台",
                _ => "请选择迁入状态",
            };
            self.open_form(
                Mode::Output {
                    heading: locale::label("预览状态迁移").into(),
                    content: format!("{}\n{totals}", locale::label(msg)),
                },
                w,
                cx,
            );
        } else {
            self.open_form(Mode::Confirm{label:format!("{} · {totals}",locale::label("删除组织状态")),command:json!({"op":"status.library.remove","space":self.loc.space,"id":value["id"],"target":value["target"],"preview":value["preview"]})},w,cx);
        }
    }
    pub(crate) fn status_library_panel(&self, cx: &Context<Self>) -> AnyElement {
        let rows = self.list("status_library");
        let can_manage = self.writable()
            && rows
                .first()
                .is_some_and(|r| r.fields.get("can_manage") == Some(&Value::Bool(true)));
        let mut header = row()
            .gap(px(8.))
            .child(headline("组织状态库", 15.))
            .child(div().flex_1());
        if can_manage {
            header = header.child(
                outlined(self.button(
                    "create-org-status",
                    "新建组织状态",
                    UiAction::Form(Mode::Workflow {
                        op: "status.library.save",
                        record: Record::default(),
                    }),
                    cx,
                ))
                .child(locale::label("新建组织状态")),
            );
        }
        let mut panel = col()
            .id("organization-status-library")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(16.))
            .gap(px(6.))
            .child(header);
        for r in rows {
            let name = r.localized_title().to_owned();
            let usage = &r.fields["usage"];
            let category = STATUSES
                .iter()
                .find(|(key, _, _)| *key == r.field("category"))
                .map_or("", |(_, name, _)| *name);
            let summary = format!(
                "{} · {} {} · {} {}",
                locale::label(category),
                usage["tasks"],
                locale::label("任务"),
                usage["workflows"],
                locale::label("类型工作流")
            );
            let mut item = row()
                .min_h(px(42.))
                .w_full()
                .gap(px(8.))
                .border_b_1()
                .border_color(rgb(LINE))
                .child(mark(self.status_color(r)))
                .child(text(name, 13., INK))
                .child(text(summary, 12., MUTED))
                .child(div().flex_1());
            if r.fields.get("default") == Some(&Value::Bool(true)) {
                item = item.child(chip(locale::label("组织默认状态"), PALE, MUTED));
            }
            if can_manage {
                for (key, label, op) in [
                    ("edit", "编辑组织状态", "status.library.save"),
                    ("remove", "预览状态迁移", "status.library.removal"),
                ] {
                    item = item.child(
                        outlined(self.button(
                            format!("org-status-{key}-{}", r.id),
                            label,
                            UiAction::Form(Mode::Workflow {
                                op,
                                record: r.clone(),
                            }),
                            cx,
                        ))
                        .child(locale::label(label)),
                    );
                }
            }
            panel = panel.child(item);
        }
        if self.writable()
            && matches!(self.role.as_str(), "owner" | "admin")
            && self
                .find("space", &self.loc.space)
                .is_some_and(|s| s.field("organization") != self.loc.space)
        {
            panel = panel.child(
                outlined(self.button(
                    "create-space-status",
                    "新建空间状态",
                    UiAction::Create("status".into()),
                    cx,
                ))
                .child(locale::label("新建空间状态")),
            );
        }
        let mut local = self.status_rows("");
        local.retain(|r| !rows.iter().any(|s| s.field("local_id") == r.id));
        if !local.is_empty() {
            panel = panel.child(headline("仅此空间", 14.).mt(px(9.)));
            for r in local {
                panel = panel.child(
                    self.button(
                        format!("local-status-{}", r.id),
                        r.localized_title().to_owned(),
                        UiAction::Open(r.clone()),
                        cx,
                    )
                    .justify_start()
                    .child(r.localized_title().to_owned()),
                );
            }
        }
        panel.into_any_element()
    }
}
