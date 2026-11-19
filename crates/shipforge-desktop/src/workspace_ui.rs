use super::*;
use crate::{
    appearance::paint as rgb,
    controls::*,
    forms::{Format, Spec, item},
};
use std::collections::{HashMap, HashSet};
#[derive(Clone)]
pub struct BoundInput {
    pub id: String,
    pub label: String,
    pub options: Vec<String>,
}

#[derive(Default)]
pub struct State {
    pub preview: Vec<Record>,
    pub preview_total: u64,
    preview_key: Option<Value>,
    expanded: HashSet<(String, String)>,
    selected: Option<(String, String)>,
}
#[derive(Clone)]
pub enum Action {
    Share(String),
    SaveChanges(Record),
    Toggle(String),
    Locate { task: String, comment: String },
    ShowViews,
}
#[derive(Clone)]
pub enum WorkForm {
    Reply { task: String, parent: String },
    OpenView,
    SaveAs(Record),
    Confirm { command: Value, names: String },
    Views,
}
impl WorkForm {
    pub fn title(&self) -> String {
        match self {
            Self::Reply { .. } => "回复任务讨论",
            Self::OpenView => "打开视图链接",
            Self::SaveAs(_) => "另存为新视图",
            Self::Confirm { .. } => "确认移除字段值",
            Self::Views => "全部保存视图",
        }
        .into()
    }
    pub fn submit_label(&self) -> &'static str {
        match self {
            Self::Reply { .. } => "发送",
            Self::OpenView => "打开",
            Self::SaveAs(_) => "保存",
            Self::Confirm { .. } => "确认移除",
            Self::Views => "关闭",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        match self {
            Self::Reply { .. } => vec![item("body", "回复内容", Format::Body, "")],
            Self::OpenView => vec![item("link", "视图链接", Format::Text, "")],
            Self::SaveAs(_) => vec![
                item("title", "新视图名称", Format::Text, ""),
                item(
                    "visibility",
                    "可见范围",
                    Format::Choice(&[("private", "仅自己"), ("space", "空间共享")]),
                    "private",
                ),
            ],
            _ => vec![],
        }
    }
}
pub enum Response {
    Impact(Value),
    Preview(Value),
    Shared,
    Opened,
    Located { inbox: bool },
}

pub(crate) fn retain_other_filters(view: &Record, fields: &mut Map<String, Value>) {
    let mut full = view
        .fields
        .get("filter")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for k in [
        "type", "status", "priority", "assignee", "project", "labels",
    ] {
        full.remove(k);
    }
    if let Some(m) = fields.get("filter").and_then(Value::as_object) {
        full.extend(m.clone());
    }
    fields.insert("filter".into(), Value::Object(full));
}

impl Workbench {
    pub fn task_field_editors(&self, form: &mut Form, w: &mut Window, cx: &mut Context<Self>) {
        let Some(ty) = form
            .entries
            .iter()
            .find(|e| e.spec.key == "type")
            .map(|e| e.raw(cx))
        else {
            return;
        };
        let bindings = self
            .find("task_type", &ty)
            .and_then(|t| t.fields.get("field_bindings"))
            .and_then(Value::as_array);
        if bindings.is_none() && !form.bound_task {
            return;
        }
        for e in &form.entries {
            if let Some(b) = &e.bound {
                form.task_draft.insert(
                    b.id.clone(),
                    e.parsed(cx).unwrap_or_else(|_| json!(e.raw(cx))),
                );
            } else if e.spec.key == "custom"
                && let Ok(Value::Object(draft)) = e.parsed(cx)
            {
                form.task_draft.extend(draft);
            }
        }
        form.entries
            .retain(|e| e.bound.is_none() && e.spec.key != "custom");
        form.bound_task = bindings.is_some();
        if let Some(bindings) = bindings {
            for binding in bindings.iter().filter(|b| b["enabled"] != false) {
                let Some(field) = binding["field"]
                    .as_str()
                    .and_then(|id| self.find("field", id))
                else {
                    continue;
                };
                let format = match field.field("value_type") {
                    "number" => Format::Decimal,
                    "boolean" => Format::Bool,
                    "member" => Format::Ref("member"),
                    "members" => Format::Many("member"),
                    "select" => Format::Choice(&[]),
                    _ => Format::Text,
                };
                let val = form.task_draft.get(&field.id).cloned().unwrap_or_default();
                let vs = Map::from_iter([("bound_value".into(), val)]);
                let mut e = crate::forms::Editor::new(
                    item("bound_value", "字段值", format, ""),
                    &vs,
                    None,
                    w,
                    cx,
                );
                let label = format!(
                    "{}{}{}",
                    field.title,
                    if binding["required"] == true {
                        " · 必填"
                    } else {
                        ""
                    },
                    if field.field("value_type") == "date" {
                        " (YYYY-MM-DD)"
                    } else {
                        ""
                    }
                );
                let options = field
                    .fields
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                e.bound = Some(BoundInput {
                    id: field.id.clone(),
                    label,
                    options,
                });
                form.entries.push(e);
            }
        } else {
            let values =
                Map::from_iter([("custom".into(), Value::Object(form.task_draft.clone()))]);
            form.entries.push(crate::forms::Editor::new(
                item("custom", "自定义字段值（JSON 对象）", Format::Json, "{}"),
                &values,
                None,
                w,
                cx,
            ));
        }
        form.open_pick = None;
    }
    pub fn open_view_link(&mut self, link: String) {
        self.send(
            json!({"op":"view.open","link":link}),
            After::Work(Response::Opened),
            false,
        );
    }
    pub fn work_action(&mut self, a: Action, w: &mut Window, cx: &mut Context<Self>) {
        match a {
            Action::Share(id) => self.send(
                json!({"op":"view.share","space":self.loc.space,"id":id}),
                After::Work(Response::Shared),
                false,
            ),
            Action::SaveChanges(view) => {
                let changes = self.view_changes();
                let mut filter = view.fields.get("filter").cloned().unwrap_or(json!({}));
                if let Some(m) = changes["filter"].as_object() {
                    for (k, v) in m {
                        filter[k] = v.clone();
                    }
                }
                let mut fields = changes;
                fields["filter"] = filter;
                self.send(json!({"op":"update","space":self.loc.space,"kind":"view","id":view.id,"expected_rev":view.rev,"fields":fields}),After::Saved("view".into()),true);
            }
            Action::Toggle(id) => {
                let key = (self.loc.space.clone(), id);
                if !self.work.expanded.remove(&key) {
                    self.work.expanded.insert(key);
                }
                cx.notify();
            }
            Action::Locate { task, comment } => self.send(
                json!({"op":"comment.thread","space":self.loc.space,"task_id":task,"id":comment}),
                After::Work(Response::Located {
                    inbox: self.loc.page == crate::catalog::Page::Inbox,
                }),
                false,
            ),
            Action::ShowViews => self.open_form(Mode::Work(WorkForm::Views), w, cx),
        }
    }
    pub fn work_submit(
        &mut self,
        f: WorkForm,
        vs: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> std::result::Result<(), String> {
        let s = |k: &str| vs.get(k).and_then(Value::as_str).unwrap_or("").trim();
        match f {
            WorkForm::Views => self.close_form(w, cx),
            WorkForm::Reply { task, parent } => {
                if s("body").is_empty() {
                    return Err("请输入回复。".into());
                }
                self.send(json!({"op":"create","kind":"comment","space":self.loc.space,"title":s("body").chars().take(80).collect::<String>(),"fields":{"task":task,"parent":parent,"body":s("body")}}),After::Saved("comment".into()),true);
            }
            WorkForm::OpenView => self.open_view_link(s("link").into()),
            WorkForm::SaveAs(view) => {
                if s("title").is_empty() {
                    return Err("请输入新视图名称。".into());
                }
                let changes = if self
                    .loc
                    .view
                    .as_ref()
                    .is_some_and(|current| current.id == view.id && current.space == view.space)
                {
                    self.view_changes()
                } else {
                    json!({})
                };
                self.send(json!({"op":"view.save_as","space":view.space,"id":view.id,"title":s("title"),"visibility":s("visibility"),"changes":changes}),After::Saved("view".into()),true);
            }
            WorkForm::Confirm { command, .. } => {
                let kind = command["kind"].as_str().unwrap_or("task").to_owned();
                self.send(command, After::Saved(kind), true);
            }
        }
        Ok(())
    }
    pub fn field_change(&mut self, k: &str, old: Option<&Record>, cmd: &Value) -> bool {
        let Some(old) = old else {
            return false;
        };
        let q = if k == "task" && cmd["fields"]["type"] != old.field("type") {
            json!({"op":"task.type.preview","space":old.space,"id":old.id,"target":cmd["fields"]["type"],"status":cmd["fields"]["status"],"custom":cmd["fields"]["custom"],"expected_rev":old.rev})
        } else if k == "task_type" && cmd["fields"]["field_bindings"].is_array() {
            json!({"op":"task_type.bindings.preview","space":old.space,"id":old.id,"expected_rev":old.rev,"field_bindings":cmd["fields"]["field_bindings"]})
        } else {
            return false;
        };
        self.send(q, After::Work(Response::Impact(cmd.clone())), true);
        true
    }
    pub fn work_reply(&mut self, r: Response, v: Value, w: &mut Window, cx: &mut Context<Self>) {
        match r {
            Response::Impact(mut cmd) => {
                if !matches!(self.form.as_ref().map(|f|&f.mode),Some(Mode::Edit(current)) if current.id==cmd["id"])
                {
                    return;
                }
                if v["status_allowed"] == false
                    || v["missing"].as_array().is_some_and(|m| !m.is_empty())
                {
                    let missing = v["missing"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(|id| self.name("field", id))
                        .collect::<Vec<_>>()
                        .join("、");
                    let mut parts = Vec::new();
                    if v["status_allowed"] == false {
                        parts.push("请改用目标工作流中的状态。".into());
                    }
                    if !missing.is_empty() {
                        parts.push(format!("请填写必填字段：{missing}"));
                    }
                    if let Some(form) = &mut self.form {
                        form.error = parts.join(" ");
                    }
                    return;
                }
                if let Some(rev) = v.get("expected_type_rev") {
                    cmd["expected_type_rev"] = rev.clone();
                }
                if let Some(token) = v.get("impact_token") {
                    cmd["impact_token"] = token.clone();
                }
                let loss = v["remove"].as_array().cloned().unwrap_or_default();
                if loss.is_empty() {
                    let kind = cmd["kind"].as_str().unwrap_or("task").to_owned();
                    self.send(cmd, After::Saved(kind), true);
                    return;
                }
                let names = loss
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|id| {
                        let field = self.name("field", id);
                        if let Some(n) = v["counts"][id].as_u64() {
                            format!("{field}（{n} 项任务）")
                        } else {
                            field
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("、");
                cmd["confirm_remove"] = json!(loss);
                self.open_form(
                    Mode::Work(WorkForm::Confirm {
                        command: cmd,
                        names,
                    }),
                    w,
                    cx,
                );
            }
            Response::Preview(key) => {
                if self.work.preview_key.as_ref() == Some(&key) {
                    self.work.preview_total = v["total"].as_u64().unwrap_or_default();
                    self.work.preview =
                        serde_json::from_value(v["items"].clone()).unwrap_or_default();
                }
            }
            Response::Shared => {
                if let Some(link) = v["link"].as_str() {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.to_owned()));
                    self.notify_text("已复制视图链接");
                }
            }
            Response::Opened => {
                if let Ok(view) = serde_json::from_value::<Record>(v["view"].clone()) {
                    self.close_form(w, cx);
                    self.act(UiAction::OpenView(view), w, cx);
                    self.view_tasks =
                        serde_json::from_value(v["tasks"].clone()).unwrap_or_default();
                }
            }
            Response::Located { inbox } => {
                if let Ok(task) = serde_json::from_value::<Record>(v["task"].clone()) {
                    if inbox
                        && (self.loc.page != crate::catalog::Page::Inbox
                            || self.loc.selected.as_ref().is_none_or(|r| {
                                r.kind != "task" || r.id != task.id || r.space != task.space
                            }))
                    {
                        return;
                    }
                    self.close_form(w, cx);
                    if !inbox {
                        self.open(task, true);
                    }
                    if let (Some(root), Some(id)) =
                        (v["root"]["id"].as_str(), v["selected"].as_str())
                    {
                        self.work
                            .expanded
                            .insert((self.loc.space.clone(), root.into()));
                        self.work.selected = Some((self.loc.space.clone(), id.into()));
                        if let Ok(items) =
                            serde_json::from_value::<Vec<Record>>(v["comments"].clone())
                        {
                            let rows = self.rows.entry("comment".into()).or_default();
                            for row in items {
                                if let Some(prev) = rows.iter_mut().find(|r| r.id == row.id) {
                                    *prev = row;
                                } else {
                                    rows.push(row);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    pub fn view_changes(&self) -> Value {
        let mut fields = json!({"filter":{}});
        for (k, v) in &self.loc.filter {
            if let Some(key) = k.strip_prefix("filter_") {
                fields["filter"][key] = v.clone();
            } else if k == "query" {
                fields["query"] = v.clone();
            }
        }
        if !self.loc.task_type.is_empty() {
            fields["filter"]["type"] = json!(self.loc.task_type);
        }
        if matches!(self.loc.layout.as_str(), "board" | "list" | "table") {
            fields["layout"] = json!(self.loc.layout);
        }
        fields
    }
    pub fn sync_bound_fields(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = &self.form else {
            return;
        };
        if !matches!(&f.mode,Mode::Edit(r) if r.kind=="task")
            && !matches!(&f.mode,Mode::Create(k) if k=="task")
        {
            return;
        }
        let Some(ty) = f
            .entries
            .iter()
            .find(|e| e.spec.key == "type")
            .map(|e| e.raw(cx))
        else {
            return;
        };
        let Some(bound) = self
            .find("task_type", &ty)
            .and_then(|r| r.fields.get("field_bindings"))
            .and_then(Value::as_array)
        else {
            return;
        };
        let expected = bound
            .iter()
            .filter(|b| b["enabled"] != false)
            .filter_map(|b| b["field"].as_str())
            .filter(|id| self.find("field", id).is_some());
        let actual = f
            .entries
            .iter()
            .filter_map(|e| e.bound.as_ref().map(|b| b.id.as_str()));
        if (!f.bound_task || !expected.eq(actual))
            && let Some(mut form) = self.form.take()
        {
            self.task_field_editors(&mut form, w, cx);
            self.form = Some(form);
        }
    }
    pub fn view_preview(&mut self, cx: &App) {
        let Some(form) = self.form.as_ref().filter(|f| {
            matches!(&f.mode,Mode::Create(k) if k=="view")
                || matches!(&f.mode,Mode::Edit(r) if r.kind=="view")
        }) else {
            self.work.preview_key = None;
            self.work.preview.clear();
            return;
        };
        let Ok(mut values) = form.values(cx) else {
            return;
        };
        values.insert("title".into(), json!("preview"));
        let Ok((_, mut fields)) = compile_record("view", values, false) else {
            return;
        };
        if let Mode::Edit(v) = &form.mode {
            retain_other_filters(v, &mut fields);
        }
        let q = json!({"op":"view.preview","space":self.loc.space,"fields":fields,"with_total":true,"limit":5});
        if self.work.preview_key.as_ref() != Some(&q) {
            self.work.preview_key = Some(q.clone());
            self.work.preview.clear();
            self.work.preview_total = 0;
            self.send(q.clone(), After::Work(Response::Preview(q)), false);
        }
    }
    pub fn view_tools(&self, view: &Record, cx: &Context<Self>) -> Div {
        let mut actions = row().gap(px(3.));
        if self.writable() {
            actions = actions.child(self.tool_button(
                &format!("copy-view-{}", view.id),
                "另存为新视图",
                "copy",
                UiAction::Form(Mode::Work(WorkForm::SaveAs(view.clone()))),
                cx,
            ));
        }
        actions = actions.child(self.tool_button(
            &format!("share-view-{}", view.id),
            "复制视图链接",
            "link",
            UiAction::Work(Action::Share(view.id.clone())),
            cx,
        ));
        if view.field("visibility") == "private" && view.field("creator") == self.actor {
            let pinned = view.fields.get("personally_pinned") == Some(&json!(true));
            actions = actions.child(self.tool_button(
                &format!("pin-view-{}", view.id),
                if pinned {
                    "取消置顶"
                } else {
                    "置顶视图"
                },
                "pin",
                UiAction::Apply(
                    json!({"op":"view.organize","id":view.id,"pinned":!pinned}),
                    "视图顺序已保存".into(),
                ),
                cx,
            ));
            let peers = self
                .list("view")
                .iter()
                .filter(|r| {
                    r.field("visibility") == "private"
                        && r.fields.get("personally_pinned") == view.fields.get("personally_pinned")
                })
                .collect::<Vec<_>>();
            if let Some(i) = peers.iter().position(|r| r.id == view.id) {
                for (adjacent, op, caption, glyph) in [
                    (i.checked_sub(1), "before", "前移", "chevron-up"),
                    (Some(i + 1), "after_view", "后移", "chevron-down"),
                ] {
                    if let Some(target) = adjacent.and_then(|n| peers.get(n)) {
                        actions = actions.child(self.tool_button(
                            &format!("{op}-{}", view.id),
                            caption,
                            glyph,
                            UiAction::Apply(
                                json!({"op":"view.organize","id":view.id,(op):target.id}),
                                "视图顺序已保存".into(),
                            ),
                            cx,
                        ));
                    }
                }
            }
        }
        actions
    }
    pub fn discussion_panel(&self, task: &Record, cx: &Context<Self>) -> Div {
        let mut children: HashMap<&str, Vec<&Record>> = HashMap::new();
        for row in self
            .list("comment")
            .iter()
            .filter(|r| r.field("task") == task.id)
        {
            children.entry(row.field("parent")).or_default().push(row);
        }
        let roots = children.remove("").unwrap_or_default();
        let mut out = col().gap(px(8.));
        if roots.is_empty() {
            return out.child(text("暂无评论", 12., MUTED));
        }
        for root in roots {
            let resolved = root.fields.get("resolved") == Some(&json!(true));
            let expanded = self
                .work
                .expanded
                .contains(&(task.space.clone(), root.id.clone()));
            let collapsed = resolved && !expanded;
            let mut discussion = col()
                .gap(px(4.))
                .p(px(8.))
                .border_1()
                .border_color(rgb(LINE))
                .rounded(px(6.));
            if resolved {
                discussion = discussion.child(
                    self.button(
                        format!("expand-{}", root.id),
                        if collapsed {
                            "展开已解决讨论"
                        } else {
                            "折叠已解决讨论"
                        },
                        UiAction::Work(Action::Toggle(root.id.clone())),
                        cx,
                    )
                    .child(text(
                        if collapsed {
                            "已解决 · 展开讨论"
                        } else {
                            "已解决 · 收起讨论"
                        },
                        12.,
                        MUTED,
                    )),
                );
            }
            if !collapsed {
                let mut pending = vec![(root, 0usize)];
                let mut seen = HashSet::new();
                while let Some((r, level)) = pending.pop() {
                    if !seen.insert(&r.id) {
                        continue;
                    }
                    let marked = resolved && root.field("resolution_comment") == r.id;
                    let selected = self
                        .work
                        .selected
                        .as_ref()
                        .is_some_and(|(space, id)| space == &task.space && id == &r.id);
                    let body = if r.field("body").is_empty() {
                        &r.title
                    } else {
                        r.field("body")
                    };
                    let mut line = col()
                        .ml(px(10. * level.min(4) as f32))
                        .p(px(4.))
                        .when(selected, |b| b.bg(rgb(PALE)));
                    let vcs = r.fields.get("vcs").filter(|_| r.field("source") == "vcs");
                    let by = if let Some(v) = vcs {
                        format!(
                            "{} · {}",
                            v["provider"].as_str().unwrap_or("Git"),
                            v["repository"].as_str().unwrap_or("")
                        )
                    } else if !r.field("external_provider").is_empty() {
                        format!(
                            "{} · {}",
                            r.field("external_provider"),
                            r.field("external_author")
                        )
                    } else if let Some(name) = r
                        .fields
                        .get("created_by_assistant")
                        .and_then(|v| v["name"].as_str())
                    {
                        name.to_owned()
                    } else if r.field("authored_by_agent").is_empty() {
                        self.name("member", r.field("creator"))
                    } else {
                        self.name("agent", r.field("authored_by_agent"))
                    };
                    line = line
                        .child(text(
                            if marked {
                                format!("{by} · 解决此讨论")
                            } else {
                                by
                            },
                            11.,
                            MUTED,
                        ))
                        .child(text(body.to_owned(), 13., INK))
                        .child(self.document_embeds_panel(r, cx));
                    if let Some(url) = vcs
                        .and_then(|v| v["url"].as_str())
                        .filter(|url| url.starts_with("https://"))
                    {
                        line = line.child(
                            outlined(self.button(
                                format!("vcs-url-{}", r.id),
                                "查看源记录",
                                UiAction::OpenSourceUrl(url.to_owned()),
                                cx,
                            ))
                            .child("查看源记录"),
                        );
                    }
                    if self.writable() {
                        let btn=row().child(self.button(format!("reply-comment-{}",r.id),"回复",UiAction::Form(Mode::Work(WorkForm::Reply{task:task.id.clone(),parent:r.id.clone()})),cx).child("回复"))
                          .child(self.button(format!("mark-comment-{}",r.id),if marked{"重新打开"}else{"用此评论解决"},UiAction::Apply(json!({"op":if marked{"comment.reopen"}else{"comment.resolve"},"id":r.id,"task_id":task.id,"expected_rev":root.rev}),"讨论已更新".into()),cx).child(if marked{"重新打开"}else{"用此评论解决"}));
                        line = line.child(
                            btn.child(
                                self.button(
                                    format!("react-comment-{}", r.id),
                                    "表情回应",
                                    UiAction::Inbox(crate::app::inbox_ui::Action::Reactions(
                                        r.clone(),
                                    )),
                                    cx,
                                )
                                .child(crate::locale::label("表情回应")),
                            ),
                        );
                    }
                    discussion = discussion.child(line);
                    if let Some(replies) = children.remove(r.id.as_str()) {
                        pending.extend(replies.into_iter().rev().map(|r| (r, level + 1)));
                    }
                }
            }
            out = out.child(discussion);
        }
        out
    }
}
