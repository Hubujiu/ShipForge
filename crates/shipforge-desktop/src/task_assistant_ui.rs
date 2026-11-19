use super::{After, UiAction, Workbench};
use crate::{
    catalog::{Page, Record},
    controls::*,
    forms::{Format, Mode, Spec, item},
    locale,
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};

#[derive(Clone)]
pub enum Action {
    Browse,
    Space(String),
    Settings(Record, &'static str, &'static str),
    Template(&'static str),
    History(Record),
}
pub(super) enum Reply {
    Settings(Box<Record>, &'static str, &'static str),
    Template(&'static str),
}
pub(crate) fn specs(op: &str) -> Vec<Spec> {
    use Format::*;
    if op == "task_assistant.template.save" {
        return vec![
            item("description", "模板描述", Body, ""),
            item("instructions", "模板指令", Body, ""),
        ];
    }
    let mut s = Vec::new();
    if op == "task_assistant.bind" {
        s.push(item(
            "connector",
            "已授权的渠道连接器",
            Ref("$task_assistant_connectors"),
            "",
        ));
    }
    s.extend([
        item("runtime", "Runtime", Text, "local-tool"),
        item("machine_id", "运行机器", Ref("machine"), ""),
        item("model", "默认模型", Text, ""),
        item("skills", "空间技能", Many("skill"), ""),
        item("sender_map", "外部用户 ID 与空间成员 ID 映射", Json, ""),
    ]);
    if op == "task_assistant.configure" {
        s.extend([
            item(
                "custom_description",
                "覆盖当前语言的模板描述",
                Bool,
                "false",
            ),
            item("description", "描述", Body, ""),
            item(
                "custom_instructions",
                "覆盖当前语言的模板指令",
                Bool,
                "false",
            ),
            item("instructions", "指令", Body, ""),
        ]);
    }
    s
}
impl Workbench {
    pub(super) fn task_assistant_connectors(&self) -> Vec<(String, String)> {
        let Some(Mode::Workflow { record, .. }) = self.form.as_ref().map(|f| &f.mode) else {
            return vec![];
        };
        self.list("connector")
            .iter()
            .filter(|c| {
                c.field("provider") == record.field("provider")
                    && c.fields.get("enabled") == Some(&json!(true))
            })
            .map(|r| (r.id.clone(), r.title.clone()))
            .collect()
    }
    pub(super) fn task_assist(&mut self, action: Action) {
        match action{
            Action::Browse=>{self.navigate(Page::Connectors);self.loc.section="task-assistants".into();self.read(true);}
            Action::Space(id)=>{if let Some(r)=self.find("space",&id).cloned(){self.switch_space(r);self.task_assist(Action::Browse);}}
            Action::Template(lang)=>self.send(json!({"op":"task_assistant.template","space":self.loc.space}),After::TaskAssist(Reply::Template(lang)),false),
            Action::History(r)=>self.send(json!({"op":"task_assistant.history","space":r.space,"provider":r.field("provider")}),After::Output(locale::label("任务助手活动").into()),false),
            Action::Settings(r,op,lang)=>self.send(json!({"op":"task_assistant.settings","space":r.space,"provider":r.field("provider"),"language":lang}),After::TaskAssist(Reply::Settings(Box::new(r),op,lang)),false),
        }
    }
    pub(super) fn assistant_reply(
        &mut self,
        reply: Reply,
        data: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (op, r) = match reply {
            Reply::Template(lang) => {
                let mut r = Record {
                    space: self.loc.space.clone(),
                    rev: data["rev"].as_i64().unwrap_or(0),
                    fields: data["templates"][lang]
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                    ..Default::default()
                };
                r.fields.insert("language".into(), json!(lang));
                ("task_assistant.template.save", r)
            }
            Reply::Settings(mut r, op, lang) => {
                r.rev = data["rev"].as_i64().unwrap_or(0);
                let provider = r.field("provider").to_owned();
                r.fields = data["settings"].as_object().cloned().unwrap_or_default();
                r.fields.insert("provider".into(), json!(provider));
                r.fields.insert("language".into(), json!(lang));
                for k in ["instructions", "description"] {
                    if let Some(v) = data["effective"].get(k) {
                        r.fields.insert(k.into(), v.clone());
                    }
                    r.fields.insert(
                        format!("custom_{k}"),
                        data["effective"]["overridden"][k].clone(),
                    );
                }
                (op, *r)
            }
        };
        self.open_form(Mode::Workflow { op, record: r }, w, cx);
    }
    pub(super) fn save_task_assistant(
        &mut self,
        op: &str,
        r: &Record,
        values: Map<String, Value>,
    ) -> Result<(), String> {
        let mut q = Value::Object(values);
        q["op"] = json!(op);
        q["space"] = json!(r.space);
        q["language"] = json!(r.field("language"));
        q["expected_rev"] = json!(r.rev);
        q["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        if op != "task_assistant.template.save" {
            q["provider"] = json!(r.field("provider"));
            if q["runtime"].as_str().is_none_or(|v| v.trim().is_empty()) {
                return Err(locale::label("请选择 Runtime").into());
            }
            if op == "task_assistant.bind" && q["connector"].as_str().is_none_or(|v| v.is_empty()) {
                return Err(locale::label("请选择已授权的渠道连接器").into());
            }
            if q["model"] == "" {
                q["model"] = Value::Null;
            }
            if q["machine_id"] == "" {
                q["machine_id"] = Value::Null;
            }
            if q["sender_map"].is_null() {
                q.as_object_mut().unwrap().remove("sender_map");
            }
            if op == "task_assistant.configure" {
                for k in ["instructions", "description"] {
                    if q[format!("custom_{k}")] != true {
                        q[k] = Value::Null;
                    }
                    q.as_object_mut().unwrap().remove(&format!("custom_{k}"));
                }
            }
        }
        self.send(
            q,
            After::Notice(locale::label("任务助手设置已保存").into()),
            true,
        );
        Ok(())
    }
    pub(crate) fn task_assistant_panel(&self, cx: &Context<Self>) -> AnyElement {
        let btn = |key: String, label: &'static str, action: UiAction| {
            outlined(self.button(key, locale::label(label), action, cx)).child(locale::label(label))
        };
        let mut header = row()
            .gap(px(6.))
            .flex_wrap()
            .child(headline("空间任务助手", 18.))
            .child(btn(
                "exit-assistants".into(),
                "外部应用",
                UiAction::Go(Page::Connectors),
            ));
        if self.writable()
            && self
                .list("assistant_catalog")
                .iter()
                .any(|r| r.fields.get("can_edit_template") == Some(&json!(true)))
        {
            for (lang, title) in [("zh-CN", "中文组织模板"), ("en", "英文组织模板")] {
                header = header.child(btn(
                    format!("assistant-template-{lang}"),
                    title,
                    UiAction::TaskAssist(Action::Template(lang)),
                ));
            }
        }
        let mut panel = col()
            .id("task-assistant-panel")
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(16.))
            .gap(px(8.))
            .child(header);
        if let Some(error) = self.errors.get("assistant_catalog").or(self.error.as_ref()) {
            panel = panel.child(text(error.clone(), 13., ERROR));
        }
        for r in self.list("assistant_catalog") {
            let here = r.space == self.loc.space;
            let connected = r.field("state") == "connected";
            let exists = r.rev > 0;
            let mut row = row()
                .gap(px(6.))
                .flex_wrap()
                .child(text(r.title.clone(), 14., INK))
                .child(text(
                    if connected { "已绑定" } else { "待绑定" },
                    12.,
                    MUTED,
                ));
            if r.field("transport") == "configuration_only" {
                row = row.child(text("仅支持助手设置", 12., MUTED));
            }
            if here && r.fields.get("can_manage") == Some(&json!(true)) && self.writable() {
                if !connected {
                    row = row.child(btn(
                        format!("bind-{}", r.id),
                        "绑定任务助手",
                        UiAction::TaskAssist(Action::Settings(
                            r.clone(),
                            "task_assistant.bind",
                            locale::language(),
                        )),
                    ));
                }
                if exists {
                    for (lang, title) in [("zh-CN", "中文设置"), ("en", "英文设置")] {
                        row = row.child(btn(
                            format!("edit-{}-{lang}", r.id),
                            title,
                            UiAction::TaskAssist(Action::Settings(
                                r.clone(),
                                "task_assistant.configure",
                                lang,
                            )),
                        ));
                    }
                    row = row.child(btn(
                        format!("history-{}", r.id),
                        "任务助手活动",
                        UiAction::TaskAssist(Action::History(r.clone())),
                    ));
                    for (op, label) in [
                        ("task_assistant.unbind", "解绑渠道"),
                        ("task_assistant.remove", "移除任务助手"),
                    ] {
                        if connected || op.ends_with("remove") {
                            row=row.child(btn(format!("{op}-{}",r.id),label,UiAction::Form(Mode::Confirm{label:label.into(),command:json!({"op":op,"space":r.space,"provider":r.field("provider"),"expected_rev":r.rev})})));
                        }
                    }
                }
            } else if !here {
                row = row.child(btn(
                    format!("open-{}", r.id),
                    "进入空间",
                    UiAction::TaskAssist(Action::Space(r.space.clone())),
                ));
            }
            panel = panel.child(row);
        }
        panel.into_any_element()
    }
}
