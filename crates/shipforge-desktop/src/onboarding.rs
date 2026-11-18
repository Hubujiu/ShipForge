use super::{After, UiAction, Workbench};
use crate::{
    appearance::{paint as rgb, surface as white},
    catalog::Page,
    controls::{BG, DIM, GREEN, INK, LINE, MUTED, PALE, col, headline, icon, outlined, row, text},
    forms::Mode,
};
use gpui_kit::{
    component::{Sizable, Size, progress::ProgressCircle},
    prelude::*,
    *,
};
use serde::Deserialize;
use serde_json::{Value, json};
use shipforge_core::onboarding::{Group, Step, groups};
use std::collections::BTreeSet;

#[derive(Clone)]
pub enum Action {
    Setup,
    Full,
    Sidebar,
    Menu(Menu),
    Display(bool),
    Group(&'static str),
    Step(&'static str),
    Start(&'static str),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Sidebar,
    Page,
}
#[derive(Deserialize)]
struct Progress {
    points: u32,
    max_points: u32,
    rank: u32,
    completed: BTreeSet<String>,
    sidebar_visible: bool,
    sidebar_expanded: bool,
    setup_required: bool,
}
pub struct Guide {
    state: Option<Progress>,
    error: Option<String>,
    sections: BTreeSet<&'static str>,
    details: BTreeSet<&'static str>,
    menu: Option<Menu>,
    prompted: bool,
}
impl Default for Guide {
    fn default() -> Self {
        Self {
            state: None,
            error: None,
            sections: BTreeSet::from(["start"]),
            details: BTreeSet::new(),
            menu: None,
            prompted: false,
        }
    }
}
impl Guide {
    pub fn change_space(&mut self) {
        self.state = None;
        self.error = None;
        self.menu = None;
    }
    pub fn close_menu(&mut self) {
        self.menu = None;
    }
    pub fn visible(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.sidebar_visible)
    }
    pub fn offer_setup(&mut self) -> bool {
        if self.prompted || !self.state.as_ref().is_some_and(|s| s.setup_required) {
            return false;
        }
        self.prompted = true;
        true
    }
    pub fn accept(&mut self, value: Value) {
        match serde_json::from_value(value) {
            Ok(progress) => {
                self.state = Some(progress);
                self.error = None;
            }
            Err(_) => self.failed(crate::locale::label("无法读取新手指引进度").into()),
        }
    }
    pub fn failed(&mut self, message: String) {
        self.state = None;
        self.error = Some(message);
    }
}

fn ring(id: impl Into<ElementId>, done: u32, total: u32, diameter: f32) -> ProgressCircle {
    ProgressCircle::new(id)
        .with_size(Size::Size(px(diameter / 0.75)))
        .value(done as f32 / total.max(1) as f32 * 100.)
        .color(rgb(GREEN))
        .accessibility_label(format!(
            "{} {done}/{total}",
            crate::locale::label("完成进度")
        ))
}
fn group_icon(group: &Group) -> &'static str {
    match group.id.as_str() {
        "start" => "compass",
        "collaborate" => "users",
        "agents" => "bot",
        "tasks" => "list-checks",
        "manage" => "boxes",
        _ => "zap",
    }
}
impl Workbench {
    pub fn guide_sidebar(&self, cx: &Context<Self>) -> Div {
        let Some(state) = &self.guide.state else {
            return col();
        };
        let steps = &groups()[0].steps[..4];
        let done = steps
            .iter()
            .filter(|s| state.completed.contains(&s.id))
            .count();
        let mut base = col()
            .w_full()
            .flex_shrink_0()
            .p(px(7.))
            .border_t_1()
            .border_color(rgb(LINE))
            .child(
                self.button(
                    "guide-sidebar-toggle",
                    "新手指引",
                    UiAction::Guide(Action::Sidebar),
                    cx,
                )
                .w_full()
                .h(px(32.))
                .justify_start()
                .child(icon("compass", 15.))
                .child(headline("新手指引", 13.))
                .child(ring("guide-quick-progress", done as u32, 4, 14.))
                .child(div().flex_1())
                .child(icon(
                    if state.sidebar_expanded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    13.,
                )),
            );
        if state.sidebar_expanded {
            for item in steps {
                let complete = state.completed.contains(&item.id);
                base = base.child(
                    self.button(
                        format!("guide-quick-{}", item.id),
                        item.title.as_str(),
                        UiAction::Guide(Action::Start(&item.id)),
                        cx,
                    )
                    .h(px(27.))
                    .justify_start()
                    .disabled(complete)
                    .child(
                        icon(if complete { "check" } else { "circle-dot" }, 14.)
                            .text_color(rgb(if complete { GREEN } else { DIM })),
                    )
                    .child(
                        text(item.title.as_str(), 12., if complete { MUTED } else { INK })
                            .when(complete, |d| d.line_through()),
                    ),
                );
            }
            base = base.child(
                row()
                    .justify_between()
                    .child(
                        self.button("guide-all", "查看全部", UiAction::Guide(Action::Full), cx)
                            .child(crate::locale::label("查看全部")),
                    )
                    .child(
                        self.button(
                            "guide-sidebar-menu",
                            "更多操作",
                            UiAction::Guide(Action::Menu(Menu::Sidebar)),
                            cx,
                        )
                        .child("···"),
                    ),
            );
            if self.guide.menu == Some(Menu::Sidebar) {
                base = base.child(
                    outlined(self.button(
                        "guide-hide-sidebar",
                        "不再显示",
                        UiAction::Guide(Action::Display(false)),
                        cx,
                    ))
                    .child(crate::locale::label("不再显示")),
                );
            }
        }
        base
    }
    pub fn guide_page(&self, width: f32, cx: &Context<Self>) -> AnyElement {
        let Some(state) = &self.guide.state else {
            return col()
                .p(px(28.))
                .gap(px(12.))
                .child(headline("开始使用 ShipForge", 22.))
                .child(text(
                    self.guide
                        .error
                        .clone()
                        .unwrap_or_else(|| crate::locale::label("正在读取设置向导…").into()),
                    13.,
                    MUTED,
                ))
                .when(self.guide.error.is_some(), |d| {
                    d.child(
                        self.button("guide-retry", "重试", UiAction::Refresh, cx)
                            .child(crate::locale::label("重试")),
                    )
                })
                .into_any_element();
        };
        let summary = col()
            .items_center()
            .justify_start()
            .gap(px(12.))
            .py(px(15.))
            .child(
                div()
                    .relative()
                    .size(px(166.))
                    .child(ring("guide-points", state.points, state.max_points, 166.))
                    .child(
                        col()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .child(
                                row()
                                    .size(px(44.))
                                    .rounded(px(10.))
                                    .bg(rgb(GREEN))
                                    .justify_center()
                                    .child(icon("layers", 24.).text_color(white())),
                            )
                            .child(headline(
                                format!("{}/{}", state.points, state.max_points),
                                17.,
                            )),
                    ),
            )
            .child(headline("开始使用 ShipForge", 22.))
            .child(text("完成步骤，熟悉团队的工作方式。", 13., MUTED))
            .child(
                text(crate::locale::rank(state.rank), 12., INK)
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(LINE))
                    .px(px(11.))
                    .py(px(3.)),
            );
        let mut guide = col()
            .min_w_0()
            .flex_1()
            .gap(px(8.))
            .child(
                row()
                    .gap(px(9.))
                    .child(headline("设置向导", 18.))
                    .child(div().flex_1())
                    .child(
                        self.button(
                            "guide-page-menu",
                            "向导选项",
                            UiAction::Guide(Action::Menu(Menu::Page)),
                            cx,
                        )
                        .child("···"),
                    ),
            )
            .child(text("按照以下步骤配置你的团队工作区。", 13., MUTED).pb(px(8.)));
        if state.setup_required {
            guide = guide.child(
                outlined(self.button(
                    "guide-initialize",
                    "初始化默认空间",
                    UiAction::Guide(Action::Setup),
                    cx,
                ))
                .child(crate::locale::label("初始化默认空间")),
            );
        }
        if self.guide.menu == Some(Menu::Page) {
            let label = if state.sidebar_visible {
                "隐藏侧边栏向导"
            } else {
                "显示侧边栏向导"
            };
            guide = guide.child(
                outlined(self.button(
                    "guide-sidebar-visibility",
                    label,
                    UiAction::Guide(Action::Display(!state.sidebar_visible)),
                    cx,
                ))
                .child(crate::locale::label(label)),
            );
        }
        if let Some(err) = &self.guide.error {
            guide = guide.child(text(err.clone(), 12., crate::controls::ERROR));
        }
        for group in groups() {
            guide = guide.child(self.guide_group(group, state, cx));
        }
        let wide = width >= 1050.;
        let list = if wide {
            div()
                .id("guide-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(guide.py(px(42.)).pr(px(26.)))
                .into_any_element()
        } else {
            guide.into_any_element()
        };
        if wide {
            row()
                .items_start()
                .size_full()
                .min_w_0()
                .min_h_0()
                .gap(px(14.))
                .bg(rgb(BG))
                .child(div().w(relative(0.40)).pt(px(40.)).child(summary))
                .child(list)
                .into_any_element()
        } else {
            col()
                .id("guide-narrow")
                .size_full()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(16.))
                .bg(rgb(BG))
                .gap(px(16.))
                .child(summary)
                .child(list)
                .into_any_element()
        }
    }
    fn guide_group(&self, group: &'static Group, state: &Progress, cx: &Context<Self>) -> Div {
        let expanded = self.guide.sections.contains(group.id.as_str());
        let done = group
            .steps
            .iter()
            .filter(|s| state.completed.contains(&s.id))
            .count();
        let mut card = col()
            .w_full()
            .flex_shrink_0()
            .border_1()
            .border_color(rgb(LINE))
            .rounded(px(8.))
            .bg(white())
            .child(
                self.button(
                    format!("guide-section-{}", group.id),
                    group.title.as_str(),
                    UiAction::Guide(Action::Group(&group.id)),
                    cx,
                )
                .w_full()
                .h(px(48.))
                .px(px(16.))
                .justify_start()
                .child(icon(group_icon(group), 16.))
                .child(headline(group.title.as_str(), 13.))
                .child(div().flex_1())
                .child(text(format!("{done}/{}", group.steps.len()), 12., MUTED))
                .child(ring(
                    SharedString::from(format!("guide-ring-{}", group.id)),
                    done as u32,
                    group.steps.len() as u32,
                    15.,
                ))
                .child(icon(
                    if expanded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.,
                )),
            );
        if expanded {
            for step in &group.steps {
                card = card.child(self.guide_step(step, state, cx));
            }
        }
        card
    }
    fn guide_step(&self, step: &'static Step, state: &Progress, cx: &Context<Self>) -> Div {
        let done = state.completed.contains(&step.id);
        let open = self.guide.details.contains(step.id.as_str());
        let mut panel = col().border_t_1().border_color(rgb(LINE)).child(
            self.button(
                format!("guide-step-{}", step.id),
                step.title.as_str(),
                UiAction::Guide(Action::Step(&step.id)),
                cx,
            )
            .w_full()
            .min_h(px(44.))
            .px(px(14.))
            .justify_start()
            .child(
                icon(if done { "check" } else { "circle-dot" }, 16.).text_color(rgb(if done {
                    GREEN
                } else {
                    DIM
                })),
            )
            .child(
                text(step.title.as_str(), 13., if done { MUTED } else { INK })
                    .flex_1()
                    .min_w_0()
                    .when(done, |d| d.line_through()),
            )
            .child(text(format!("+{}", step.points), 12., MUTED))
            .child(icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.,
            )),
        );
        if open {
            panel = panel.child(
                col()
                    .items_start()
                    .pl(px(39.))
                    .pr(px(14.))
                    .pb(px(12.))
                    .gap(px(8.))
                    .child(text(step.hint.as_str(), 12., MUTED))
                    .when(!done, |d| {
                        d.child(
                            outlined(self.button(
                                format!("guide-launch-{}", step.id),
                                step.button.as_str(),
                                UiAction::Guide(Action::Start(&step.id)),
                                cx,
                            ))
                            .bg(rgb(PALE))
                            .child(crate::locale::label(step.button.as_str())),
                        )
                    }),
            );
        }
        panel
    }
    pub fn guide_action(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Setup => self.open_form(Mode::SetupOrganization, w, cx),
            Action::Group(id) => {
                if !self.guide.sections.remove(id) {
                    self.guide.sections.insert(id);
                }
            }
            Action::Step(id) => {
                if !self.guide.details.remove(id) {
                    self.guide.details.insert(id);
                }
            }
            Action::Full => self.navigate(Page::GettingStarted),
            Action::Menu(anchor) => {
                self.guide.menu = (self.guide.menu != Some(anchor)).then_some(anchor)
            }
            Action::Sidebar | Action::Display(_) => {
                let Some(state) = &self.guide.state else {
                    return;
                };
                let mut cmd = json!({"op":"onboarding.display","space":self.loc.space});
                match action {
                    Action::Display(v) => cmd["sidebar_visible"] = json!(v),
                    _ => cmd["sidebar_expanded"] = json!(!state.sidebar_expanded),
                }
                self.guide.close_menu();
                self.send(cmd, After::Guide, true);
            }
            Action::Start(id) => self.guide_start(id, w, cx),
        }
        cx.notify();
    }
    fn guide_start(&mut self, id: &'static str, w: &mut Window, cx: &mut Context<Self>) {
        use Page::*;
        let mut form = None;
        let page = match id {
            "connect_machine" => {
                if self.mode != "demo" {
                    form = Some(Mode::Machine { stop: false });
                }
                Machines
            }
            "create_agent" => {
                self.navigate(Agents);
                self.act(UiAction::Create("agent".into()), w, cx);
                return;
            }
            "create_task" => {
                form = Some(Mode::Create("task".into()));
                Tasks
            }
            "assign_agent" | "assign_member" | "comment_task" | "follow_task" | "project_task"
            | "subtask" => {
                form = Some(Mode::GuideTarget {
                    step: id,
                    kind: "task",
                });
                Tasks
            }
            "import_tasks" => Transfer,
            "invite_member" => {
                form = Some(Mode::Invite { email: true });
                Members
            }
            "handle_inbox" | "clear_inbox" => Inbox,
            "agent_instructions" | "share_agent" | "equip_skill" | "chat_agent" => {
                form = Some(Mode::GuideTarget {
                    step: id,
                    kind: "agent",
                });
                Agents
            }
            "create_crew" => {
                form = Some(Mode::Create("crew".into()));
                Crews
            }
            "install_skill" => Market,
            "label_task" => Tags,
            "relate_task" => {
                form = Some(Mode::Create("relation".into()));
                Relations
            }
            "custom_view" | "team_view" => {
                form = Some(Mode::Create("view".into()));
                Views
            }
            "create_project" => {
                form = Some(Mode::Create("project".into()));
                Projects
            }
            "type_fields" => Types,
            "manage_status" => Statuses,
            "git_link" | "connect_im" | "connect_slack_lark" => Connectors,
            "estimates" => Spaces,
            "create_rule" | "schedule_rule" | "status_rule" | "comment_rule" | "webhook_rule" => {
                form = Some(Mode::Create("automation".into()));
                Automations
            }
            "run_rule" => Automations,
            _ => return,
        };
        self.guide.close_menu();
        self.navigate(page);
        if id == "estimates"
            && let Some(space) = self.find("space", &self.loc.space).cloned()
        {
            form = Some(Mode::Edit(space));
        }
        if let Some(mode) = form {
            let mut preset = serde_json::Map::new();
            if id == "team_view" {
                preset.insert("visibility".into(), json!("space"));
            }
            if id == "custom_view" {
                preset.insert("visibility".into(), json!("private"));
            }
            self.open_prefilled(mode, preset, w, cx);
            if let Some(trigger) = match id {
                "schedule_rule" => Some("schedule"),
                "status_rule" => Some("status"),
                "comment_rule" => Some("comment"),
                "webhook_rule" => Some("webhook"),
                _ => None,
            } && let Some(f) = &mut self.form
            {
                f.triggers.clear();
                f.add_trigger(&json!({"type":trigger,"enabled":true}), w, cx);
            }
        }
    }
    pub fn guide_target(
        &mut self,
        id: &str,
        r: crate::catalog::Record,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_form(w, cx);
        self.open(r.clone(), true);
        let mut preset = serde_json::Map::new();
        let mode = match id {
            "comment_task" => Mode::Comment(r),
            "subtask" => {
                preset.insert("parent".into(), json!(r.id));
                Mode::Create("task".into())
            }
            "chat_agent" => {
                preset.insert("agent".into(), json!(r.id));
                preset.insert("title".into(), json!(r.title));
                Mode::Create("chat".into())
            }
            "share_agent" => {
                preset.insert("visibility".into(), json!("space"));
                Mode::Edit(r)
            }
            "follow_task" => return,
            _ => Mode::Edit(r),
        };
        self.open_prefilled(mode, preset, w, cx);
    }
}
