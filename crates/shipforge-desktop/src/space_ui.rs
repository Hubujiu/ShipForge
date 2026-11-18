use super::{After, UiAction, Workbench};
use crate::{
    appearance::{paint as rgb, surface as white},
    catalog::{Page, Record},
    controls::*,
    forms::Mode,
    locale,
};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};

#[derive(Clone)]
pub enum Action {
    Join(String),
    Leave(Record),
    Preview(Record),
    Restore(Record),
    Pin(String, bool),
    Enable(String),
}

const MODULES: &[(&str, Page)] = &[
    ("iterations", Page::Iterations),
    ("triage", Page::Triage),
    ("views", Page::Views),
    ("projects", Page::Projects),
    ("customer_requests", Page::CustomerRequests),
];
fn flag(record: &Record, key: &str) -> bool {
    record
        .fields
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

impl Workbench {
    pub(crate) fn current_archived(&self) -> bool {
        self.find("space", &self.loc.space)
            .is_some_and(|r| flag(r, "deactivated"))
    }
    pub(crate) fn archived_action(&self, action: &UiAction) -> bool {
        if !self.current_archived() {
            return false;
        }
        match action {
            UiAction::Create(kind) => kind != "space",
            UiAction::Edit(record) | UiAction::Delete(record) => record.space == self.loc.space,
            UiAction::Apply(q, _) | UiAction::Form(Mode::Confirm { command: q, .. }) => {
                q["space"]
                    .as_str()
                    .is_none_or(|space| space == self.loc.space)
                    && !matches!(
                        q["op"].as_str(),
                        Some(
                            "space.restore"
                                | "space.pin"
                                | "preferences.update"
                                | "sync.resolve"
                                | "access.revoke"
                        )
                    )
            }
            UiAction::Personal(
                super::personal::Action::Move
                | super::personal::Action::Chat(_)
                | super::personal::Action::Command(_),
            )
            | UiAction::Form(
                Mode::Comment(_)
                | Mode::ThreadReply { .. }
                | Mode::Message(_)
                | Mode::MoveTask
                | Mode::Upload(_)
                | Mode::Workflow { .. }
                | Mode::BulkTags
                | Mode::MergeTags
                | Mode::Invite { .. },
            )
            | UiAction::SaveView
            | UiAction::Import
            | UiAction::RestoreVersion(..)
            | UiAction::RuleFire(_)
            | UiAction::IterationEnd(_) => true,
            _ => false,
        }
    }
    pub(crate) fn can_create_space(&self) -> bool {
        let org = self
            .find("space", &self.loc.space)
            .map(|s| s.field("organization"))
            .unwrap_or("default");
        self.find("space", org)
            .is_some_and(|r| flag(r, "can_manage_content") && !flag(r, "deactivated"))
    }
    fn modules(&self) -> &[Value] {
        self.find("space", &self.loc.space)
            .and_then(|r| r.fields.get("navigation"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
    pub(crate) fn module_disabled(&self, page: Page) -> bool {
        MODULES
            .iter()
            .find(|(_, p)| *p == page)
            .is_some_and(|(id, _)| {
                self.modules()
                    .iter()
                    .any(|m| m["id"] == *id && m["enabled"] == false)
            })
    }
    pub(super) fn navigate_space_page(&mut self, page: Page) {
        if self.module_disabled(page) {
            self.navigate(Page::SpaceModules);
            self.notify_text("可在此启用空间功能。");
        } else {
            self.navigate(page);
        }
    }
    pub(crate) fn space_sidebar(&self, cx: &Context<Self>) -> Div {
        let mut pages = vec![Page::Tasks];
        for (id, page) in MODULES {
            if self.modules().iter().any(|m| {
                m["id"] == *id
                    && m["enabled"] == true
                    && (m["pinned"] == true || self.loc.page == *page)
            }) {
                pages.push(*page);
            }
        }
        pages.push(Page::SpaceModules);
        self.nav_group("空间工作", &pages, cx)
    }
    pub(crate) fn space_module_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut body = col()
            .id("space-modules-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(18.))
            .gap(px(8.))
            .child(headline("空间功能", 18.));
        let can_enable = self
            .find("space", &self.loc.space)
            .is_some_and(|r| flag(r, "can_manage_content") && !flag(r, "deactivated"));
        for (id, page) in MODULES {
            let info = page.info();
            let Some(state) = self.modules().iter().find(|s| s["id"] == *id) else {
                continue;
            };
            let mut line = row()
                .w_full()
                .min_h(px(48.))
                .px(px(11.))
                .gap(px(9.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(LINE))
                .bg(white())
                .child(icon(info.icon, 16.))
                .child(headline(info.label, 14.))
                .child(div().flex_1());
            if state["enabled"] == true {
                let pinned = state["pinned"] == true;
                let label = if pinned {
                    "取消固定"
                } else {
                    "固定到侧栏"
                };
                line = line
                    .child(
                        outlined(self.button(
                            format!("pin-{id}"),
                            label,
                            UiAction::Space(Action::Pin(id.to_string(), !pinned)),
                            cx,
                        ))
                        .child(locale::label(label)),
                    )
                    .child(
                        outlined(self.button(
                            format!("open-module-{id}"),
                            "打开",
                            UiAction::Go(*page),
                            cx,
                        ))
                        .child(locale::label("打开")),
                    );
            } else {
                line = line.child(text("未启用", 12., MUTED));
                if can_enable {
                    line = line.child(
                        outlined(self.button(
                            format!("enable-{id}"),
                            "启用模块",
                            UiAction::Space(Action::Enable(id.to_string())),
                            cx,
                        ))
                        .child(locale::label("启用模块")),
                    );
                }
            }
            body = body.child(line);
        }
        body
    }
    pub(crate) fn space_directory(&self, cx: &Context<Self>) -> impl IntoElement {
        let archived = self.loc.section == "archived";
        let mut tabs = row().gap(px(5.)).px(px(16.)).py(px(8.));
        for (key, label) in [("active", "使用中"), ("archived", "已停用")] {
            tabs = tabs.child(
                self.button(
                    format!("spaces-{key}"),
                    label,
                    UiAction::Section(key.to_string()),
                    cx,
                )
                .when(archived == (key == "archived"), |b| b.bg(rgb(SELECTED)))
                .child(locale::label(label)),
            );
        }
        let mut list = col()
            .id("space-directory-list")
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .px(px(16.))
            .gap(px(6.));
        let mut count = 0;
        for space in self
            .list("space")
            .iter()
            .filter(|s| flag(s, "deactivated") == archived)
        {
            count += 1;
            let id = &space.id;
            let mut line = row()
                .min_h(px(56.))
                .gap(px(7.))
                .px(px(10.))
                .border_1()
                .border_color(rgb(LINE))
                .rounded(px(6.))
                .bg(white())
                .child(icon("boxes", 16.))
                .child(
                    col()
                        .min_w_0()
                        .flex_1()
                        .child(headline(space.title.clone(), 14.))
                        .child(text(
                            if space.field("visibility") == "public" {
                                "公开空间"
                            } else {
                                "私有空间"
                            },
                            11.,
                            MUTED,
                        )),
                );
            let action = |name: &'static str, label: &'static str, do_it: UiAction| {
                outlined(self.button(format!("space-{name}-{id}"), label, do_it, cx))
                    .child(locale::label(label))
            };
            if flag(space, "can_manage") && !archived {
                line = line.child(action(
                    "settings",
                    "空间设置",
                    UiAction::Edit(space.clone()),
                ));
            }
            if flag(space, "can_leave") {
                line = line.child(action(
                    "leave",
                    "退出空间",
                    UiAction::Space(Action::Leave(space.clone())),
                ));
            }
            if flag(space, "can_manage_content") {
                if archived {
                    line = line.child(action(
                        "restore",
                        "恢复空间",
                        UiAction::Space(Action::Restore(space.clone())),
                    ));
                } else {
                    line = line.child(action(
                        "deactivate",
                        "停用空间",
                        UiAction::Space(Action::Preview(space.clone())),
                    ));
                }
            }
            if flag(space, "joined") {
                line = line.child(action(
                    "open",
                    if archived {
                        "查看历史"
                    } else {
                        "进入空间"
                    },
                    UiAction::SelectSpace(space.clone()),
                ));
            } else if flag(space, "can_join") {
                line = line.child(action(
                    "join",
                    "加入空间",
                    UiAction::Space(Action::Join(id.clone())),
                ));
            }
            list = list.child(line);
        }
        if count == 0 {
            list = list.child(
                text(
                    if archived {
                        "没有已停用的空间。"
                    } else {
                        "没有可用空间。"
                    },
                    13.,
                    MUTED,
                )
                .p(px(12.)),
            );
        }
        col().flex_1().min_h_0().child(tabs).child(list)
    }
    pub(super) fn space_move_targets(&self) -> Vec<(String, String)> {
        let Some(Mode::DeactivateSpace { space, .. }) = self.form.as_ref().map(|f| &f.mode) else {
            return vec![];
        };
        self.space_move_options(&space.id)
    }
    pub(super) fn space_move_options(&self, id: &str) -> Vec<(String, String)> {
        let Some(origin) = self.find("space", id) else {
            return vec![];
        };
        self.list("space")
            .iter()
            .filter(|s| {
                s.id != id
                    && s.field("organization") == origin.field("organization")
                    && flag(s, "can_edit_content")
            })
            .map(|s| (s.id.clone(), s.title.clone()))
            .collect()
    }
    pub(super) fn space_action(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        match action{
            Action::Join(id)=>self.send(json!({"op":"space.join","space":id}),After::SpaceMembership,true),
            Action::Leave(space)=>self.open_form(Mode::Confirm{label:format!("{} · {}",locale::label("退出空间"),space.title),command:json!({"op":"space.leave","space":space.id})},w,cx),
            Action::Preview(space)=>self.send(json!({"op":"space.deactivate.preview","space":space.id}),After::SpacePreview(space),false),
            Action::Restore(space)=>self.open_form(Mode::Confirm{label:format!("{} · {}",locale::label("恢复空间"),space.title),command:json!({"op":"space.restore","space":space.id,"expected_rev":space.rev})},w,cx),
            Action::Pin(module,pinned)=>self.send(json!({"op":"space.pin","space":self.loc.space,"module":module,"pinned":pinned}),After::Notice(locale::label("侧栏已更新").into()),true),
            Action::Enable(module)=>{
                let Some(space)=self.find("space",&self.loc.space)else{return};
                let key=match module.as_str(){"iterations"=>"iterations_enabled","triage"=>"triage_enabled","customer_requests"=>"customer_requests_enabled",_=>return};
                self.send(json!({"op":"update","kind":"space","id":space.id,"space":space.id,"expected_rev":space.rev,"fields":{(key):true}}),After::Notice(locale::label("空间功能已启用").into()),true);
            }
        }
    }
    pub(super) fn space_membership_reply(
        &mut self,
        result: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_form(w, cx);
        if result["joined"] == false && result["space"] == self.loc.space {
            let org = self
                .find("space", &self.loc.space)
                .map(|s| s.field("organization"))
                .unwrap_or("default")
                .to_owned();
            if let Some(parent) = self.find("space", &org).cloned() {
                self.switch_space(parent);
            }
        }
        self.navigate(Page::Spaces);
        if result["fields"]["deactivated"] == true {
            self.loc.section = "archived".into();
        }
        self.notify_text("空间已更新");
    }
}
