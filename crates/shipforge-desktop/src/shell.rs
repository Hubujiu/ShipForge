use crate::{
    app::{UiAction, Workbench},
    catalog::Page,
    controls::*,
    forms::{Control, Format, Mode},
    motion,
};
use gpui_kit::{
    base::{FocusTrapElement, TestSupportExt},
    component::TitleBar,
    prelude::*,
    *,
};
use serde_json::Value;

impl Workbench {
    fn topbar(&self, width: f32, cx: &Context<Self>) -> impl IntoElement {
        let brand = icon("layers", 17.).text_color(rgb(GREEN));
        TitleBar::new()
            .h(px(40.))
            .bg(rgb(0xf2f2f8))
            .border_b_1()
            .border_color(rgb(LINE))
            .pl_0()
            .child(
                row()
                    .w(px((width - 142.).max(400.)))
                    .h_full()
                    .px(px(12.))
                    .gap(px(6.))
                    .child(
                        row()
                            .w(px((width * 0.24).min(320.)))
                            .gap(px(8.))
                            .child(brand)
                            .child(headline("ShipForge", 13.))
                            .child(text(
                                if self.mode == "demo" {
                                    "演示空间"
                                } else if self.mode == "team" {
                                    "团队工作区"
                                } else {
                                    "本地工作区"
                                },
                                11.,
                                MUTED,
                            )),
                    )
                    .child(
                        row()
                            .gap(px(2.))
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                self.tool_button(
                                    "history-back",
                                    "后退",
                                    "arrow-left",
                                    UiAction::Back,
                                    cx,
                                )
                                .disabled(!self.can_back()),
                            )
                            .child(
                                self.tool_button(
                                    "history-forward",
                                    "前进",
                                    "arrow-right",
                                    UiAction::Forward,
                                    cx,
                                )
                                .disabled(!self.can_forward()),
                            )
                            .child(self.tool_button(
                                "refresh",
                                "刷新",
                                "rotate-cw",
                                UiAction::Refresh,
                                cx,
                            )),
                    )
                    .child(
                        row()
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                self.button("global-search", "全局搜索", UiAction::Search, cx)
                                    .h(px(25.))
                                    .w(px((width * 0.25).clamp(150., 340.)))
                                    .border_1()
                                    .border_color(rgb(LINE))
                                    .bg(rgb(0xfafafe))
                                    .justify_start()
                                    .child(icon("search", 13.).text_color(rgb(MUTED)))
                                    .child(text("搜索", 12., MUTED))
                                    .child(div().flex_1())
                                    .child(text("Ctrl K", 10., DIM)),
                            ),
                    ),
            )
    }
    fn rail(&self, cx: &Context<Self>) -> Div {
        let current = self.loc.page;
        let item = |id: &str, label: &str, glyph: &str, go: UiAction, active: bool| {
            self.tool_button(id, label, glyph, go, cx)
                .size(px(37.))
                .rounded(px(10.))
                .when(active, |b| b.bg(rgb(0xe8e8f1)))
        };
        col()
            .w(px(60.))
            .h_full()
            .flex_shrink_0()
            .items_center()
            .py(px(13.))
            .gap(px(10.))
            .bg(rgb(0xf8f8fb))
            .child(
                self.button("workspace-home", "空间任务", UiAction::Go(Page::Tasks), cx)
                    .size(px(36.))
                    .child(icon("layers", 24.).text_color(rgb(GREEN))),
            )
            .child(div().h(px(10.)))
            .child(item(
                "rail-home",
                "首页",
                "house",
                UiAction::Go(Page::Tasks),
                !matches!(current, Page::Settings | Page::Chats | Page::Members),
            ))
            .child(item(
                "rail-chat",
                "对话",
                "messages-square",
                UiAction::Go(Page::Chats),
                current == Page::Chats,
            ))
            .child(item(
                "rail-members",
                "成员",
                "users",
                UiAction::Go(Page::Members),
                current == Page::Members,
            ))
            .child(item(
                "rail-settings",
                "全部设置",
                "settings",
                UiAction::Go(Page::Settings),
                current == Page::Settings,
            ))
            .child(div().flex_1())
            .child(item(
                "rail-collapse",
                "切换侧栏",
                "panel-left",
                UiAction::ToggleSidebar,
                false,
            ))
            .child(item(
                "rail-sync",
                "连接设置",
                "database",
                UiAction::Go(Page::Settings),
                false,
            ))
    }
    fn nav_item(&self, page: Page, cx: &Context<Self>) -> impl IntoElement {
        let info = page.info();
        let active = self.loc.page == page;
        self.button(
            format!("nav-{:?}", page),
            info.label,
            UiAction::Go(page),
            cx,
        )
        .w_full()
        .h(px(29.))
        .rounded(px(6.))
        .justify_start()
        .gap(px(9.))
        .px(px(9.))
        .when(active, |b| {
            b.bg(rgb(SELECTED)).font_weight(FontWeight::SEMIBOLD)
        })
        .child(icon(info.icon, 14.).text_color(rgb(if active { INK } else { MUTED })))
        .child(text(info.label, 13., INK))
        .child(div().flex_1())
        .when(page == Page::Mine, |b| {
            b.child(text(
                self.list("task")
                    .iter()
                    .filter(|r| r.field("assignee") == self.actor)
                    .count()
                    .to_string(),
                10.,
                DIM,
            ))
        })
    }
    fn nav_group(&self, label: &str, pages: &[Page], cx: &Context<Self>) -> Div {
        let mut group = col()
            .gap(px(2.))
            .mb(px(14.))
            .child(text(label.to_owned(), 11., DIM).px(px(9.)).pb(px(4.)));
        for &page in pages {
            group = group.child(self.nav_item(page, cx));
        }
        group
    }
    fn side(&self, width: f32, cx: &Context<Self>) -> impl IntoElement {
        use Page::*;
        let setting = matches!(
            self.loc.page,
            Settings
                | Organizations
                | Members
                | Spaces
                | RelationTypes
                | Fields
                | Types
                | Relations
                | Statuses
                | Connectors
                | Transfer
                | Machines
                | Tools
        );
        let mut list = col().w_full().px(px(8.)).pt(px(6.));
        if setting {
            list = list
                .child(self.nav_group(
                    "组织",
                    &[
                        Settings,
                        Organizations,
                        Spaces,
                        Members,
                        Machines,
                        Agents,
                        Crews,
                        Connectors,
                        Transfer,
                    ],
                    cx,
                ))
                .child(self.nav_group(
                    &self.space_name(),
                    &[
                        Tasks,
                        Types,
                        Statuses,
                        Fields,
                        RelationTypes,
                        Relations,
                        Tools,
                    ],
                    cx,
                ));
        } else {
            list = list
                .child(self.nav_group("个人工作", &[Inbox, Mine, Assistant, Market], cx))
                .child(
                    row()
                        .h(px(31.))
                        .px(px(4.))
                        .child(
                            self.button(
                                "choose-space",
                                self.space_name(),
                                UiAction::Go(Spaces),
                                cx,
                            )
                            .justify_start()
                            .gap(px(6.))
                            .child(icon("boxes", 15.))
                            .child(headline(self.space_name(), 13.))
                            .child(icon("chevrons-up-down", 12.)),
                        )
                        .child(div().flex_1())
                        .child(self.tool_button(
                            "add-space-item",
                            "在此新建任务",
                            "plus",
                            UiAction::Create("task".into()),
                            cx,
                        )),
                )
                .child(self.nav_group(
                    "空间工作",
                    &[Tasks, Iterations, Projects, Triage, Views],
                    cx,
                ))
                .child(self.nav_group(
                    "协作",
                    &[Agents, Crews, Automations, Docs, Skills, Runs, Artifacts],
                    cx,
                ))
                .child(self.nav_group("组织", &[Members, Connectors, Settings], cx));
        }
        col()
            .w(px(width))
            .min_w_0()
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(0xfafbfd))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(
                row()
                    .h(px(48.))
                    .px(px(14.))
                    .gap(px(8.))
                    .child(headline(if setting { "设置" } else { "首页" }, 16.))
                    .child(div().flex_1())
                    .child(self.tool_button(
                        "sidebar-fold",
                        "收起侧栏",
                        "panel-left-close",
                        UiAction::ToggleSidebar,
                        cx,
                    ))
                    .child(
                        primary(self.button(
                            "quick-add",
                            "新建任务",
                            UiAction::Create("task".into()),
                            cx,
                        ))
                        .size(px(24.))
                        .px_0()
                        .child(icon("plus", 14.)),
                    ),
            )
            .child(
                div()
                    .id("space-navigation")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(
                row()
                    .h(px(49.))
                    .px(px(15.))
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(mark(if self.error.is_some() { ERROR } else { GREEN }))
                    .child(text(
                        if self.mode == "team" {
                            "团队服务"
                        } else if self.mode == "demo" {
                            "内存演示"
                        } else {
                            "本机工作区"
                        },
                        12.,
                        MUTED,
                    ))
                    .child(div().flex_1())
                    .child(self.tool_button(
                        "connection-settings",
                        "连接设置",
                        "settings-2",
                        UiAction::Go(Settings),
                        cx,
                    )),
            )
    }
    fn form_layer(&mut self, w: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.form.as_ref()?;
        let mode = f.mode.clone();
        let entries = f.entries.clone();
        let open_pick = f.open_pick;
        let error = f.error.clone();
        let results = f.search_results.clone();
        let preview = f.preview.clone();
        let viewport = w.viewport_size();
        let max_w = f32::from(viewport.width);
        let max_h = f32::from(viewport.height);
        let width = if matches!(&mode,Mode::Create(k) if k=="automation") {
            860.
        } else {
            620.
        };
        let width = f32::min(width, (max_w - 40.).max(340.));
        let mut body = col().w_full().gap(px(12.)).px(px(21.)).py(px(14.));
        if let Mode::Output { content, .. } = &mode {
            body = body.child(text(content.clone(), 13., INK)).child(
                outlined(self.button(
                    "copy-output",
                    "复制结果",
                    UiAction::Copy(content.clone()),
                    cx,
                ))
                .child(icon("copy", 13.))
                .child("复制"),
            );
        }
        if let Mode::Machine { stop: false } = &mode {
            body=body.child(text("已有配置可直接启动。新配置会启用只读示例 Runtime「inspect-context」，可另选外部程序。",13.,MUTED));
        }
        if let Mode::Confirm { .. } = &mode {
            body = body.child(text("请确认本次操作。", 14., MUTED));
        }
        if let Mode::IterationEnd(r) = &mode {
            let unfinished = self
                .list("task")
                .iter()
                .filter(|t| {
                    t.field("iteration") == r.id
                        && !matches!(t.field("status"), "done" | "cancelled")
                })
                .count();
            body = body.child(text(
                format!("还有 {unfinished} 项工作尚未完成。请选择它们的去向。"),
                14.,
                MUTED,
            ));
        }
        for (i, e) in entries.into_iter().enumerate() {
            let raw = e.raw(cx);
            let id = format!("modal-{}", e.spec.key);
            let label = e.spec.label;
            let mut field = col().w_full().gap(px(4.)).child(text(label, 12., MUTED));
            match e.control {
                Control::Text(ref input) => {
                    field = field.child(self.field(&id, label, input, w, cx));
                }
                Control::Pick { value, focus } => {
                    let (mut options, allow_empty, multi) = match e.spec.format {
                        Format::Ref(k) => (self.options(k), true, false),
                        Format::Many(k) => (self.options(k), true, true),
                        Format::Bool => (
                            vec![
                                ("true".into(), "启用".into()),
                                ("false".into(), "关闭".into()),
                            ],
                            false,
                            false,
                        ),
                        Format::Choice(v) => (
                            v.iter()
                                .map(|(k, l)| (k.to_string(), l.to_string()))
                                .collect(),
                            false,
                            false,
                        ),
                        _ => unreachable!(),
                    };
                    if allow_empty {
                        options.insert(0, (String::new(), "未设置".into()));
                    }
                    let selected: Vec<&str> = value.split(',').map(str::trim).collect();
                    let display = if multi {
                        options
                            .iter()
                            .filter(|(v, _)| !v.is_empty() && selected.contains(&v.as_str()))
                            .map(|(_, l)| l.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    } else {
                        options
                            .iter()
                            .find(|(v, _)| v == &raw)
                            .map(|(_, l)| l.clone())
                            .unwrap_or(raw)
                    };
                    let frame = self.field_frame(&id, &focus, w, cx);
                    field = field.child(
                        frame.px_0().child(
                            self.button(id.clone(), label, UiAction::Pick(i), cx)
                                .track_focus(&focus)
                                .w_full()
                                .h(px(33.))
                                .justify_start()
                                .child(text(
                                    if display.is_empty() {
                                        "请选择".into()
                                    } else {
                                        display
                                    },
                                    13.,
                                    INK,
                                ))
                                .child(div().flex_1())
                                .child(icon("chevron-down", 13.)),
                        ),
                    );
                    if open_pick == Some(i) {
                        let mut menu = col()
                            .id(format!("choice-menu-{i}"))
                            .max_h(px(174.))
                            .overflow_y_scroll()
                            .border_1()
                            .border_color(rgb(LINE))
                            .rounded(px(6.))
                            .p(px(3.))
                            .bg(rgb(PALE));
                        for (index, (v, l)) in options.iter().enumerate() {
                            let active = if multi {
                                selected.contains(&v.as_str())
                            } else {
                                v == &value
                            };
                            menu = menu.child(
                                self.button(
                                    format!("field-{i}-option-{index}"),
                                    l.clone(),
                                    UiAction::Choose(i, v.clone()),
                                    cx,
                                )
                                .w_full()
                                .justify_start()
                                .when(active, |b| b.bg(rgb(SELECTED)))
                                .child(l.clone())
                                .child(div().flex_1())
                                .when(active, |b| b.child(icon("check", 13.))),
                            );
                        }
                        field = field.child(menu);
                    }
                }
            }
            body = body.child(field);
        }
        if matches!(mode, Mode::Search) {
            for (i, r) in results.iter().take(150).enumerate() {
                body = body.child(
                    self.button(
                        format!("search-{i}"),
                        r.title.clone(),
                        UiAction::OpenSearch(r.clone()),
                        cx,
                    )
                    .w_full()
                    .justify_start()
                    .gap(px(8.))
                    .child(icon(
                        Page::from_kind(&r.kind)
                            .map(|p| p.info().icon)
                            .unwrap_or("file-text"),
                        15.,
                    ))
                    .child(text(r.title.clone(), 13., INK))
                    .child(div().flex_1())
                    .child(text(crate::forms::noun(&r.kind), 11., DIM)),
                );
            }
            if results.is_empty() {
                body = body.child(text("输入关键词，在当前空间中查找对象。", 12., MUTED));
            }
        }
        if let Some(p) = &preview {
            body = body.child(text(
                format!(
                    "导入预检：{}",
                    p.get("summary").map(Value::to_string).unwrap_or_default()
                ),
                13.,
                GREEN,
            ));
        }
        if !error.is_empty() {
            body = body.child(text(error, 13., ERROR));
        }
        let confirm = if self.busy {
            "正在处理…"
        } else if preview.is_some() {
            "确认导入"
        } else {
            mode.submit_label()
        };
        let mut footer = row()
            .w_full()
            .px(px(18.))
            .py(px(11.))
            .gap(px(7.))
            .border_t_1()
            .border_color(rgb(LINE))
            .justify_end();
        if !matches!(mode, Mode::Output { .. }) {
            footer = footer.child(
                self.button("modal-cancel", "取消", UiAction::CloseForm, cx)
                    .disabled(self.busy)
                    .child("取消"),
            );
        }
        footer = footer.child(
            primary(self.button("modal-submit", confirm, UiAction::Submit, cx))
                .disabled(self.busy)
                .child(confirm),
        );
        let popup = col()
            .id("form-dialog")
            .test_support()
            .role(Role::Dialog)
            .aria_label(mode.heading())
            .occlude()
            .w(px(width))
            .max_h(px(max_h - 64.))
            .min_h_0()
            .overflow_hidden()
            .bg(white())
            .border_1()
            .border_color(rgb(LINE))
            .rounded(px(12.))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                row()
                    .w_full()
                    .h(px(49.))
                    .px(px(19.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(headline(mode.heading(), 16.))
                    .child(div().flex_1())
                    .child(
                        self.tool_button("modal-x", "关闭", "x", UiAction::CloseForm, cx)
                            .disabled(self.busy),
                    ),
            )
            .child(
                div()
                    .id("form-body-scroll")
                    .overflow_y_scroll()
                    .min_h_0()
                    .max_h(px((max_h - 184.).max(150.)))
                    .child(body),
            )
            .child(footer);
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x25253650))
                .child(popup)
                .focus_trap("modal-focus", &self.modal_focus)
                .into_any_element(),
        )
    }
}
impl Render for Workbench {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = f32::from(w.viewport_size().width);
        let expanded = if self.sidebar {
            if width >= 1100. { 272. } else { 196. }
        } else {
            0.
        };
        let animated = self
            .motion
            .size("workspace-sidebar", expanded, w, cx)
            .max(0.);
        let chrome = self.topbar(width, cx).into_any_element();
        let rail = self.rail(cx);
        let side = self.side(animated, cx).into_any_element();
        let main = self.page_body(w, cx);
        let modal = self.form.is_some();
        let opacity = self
            .motion
            .presence(
                "modal-surface",
                modal,
                if modal {
                    motion::ENTER_DURATION
                } else {
                    motion::EXIT_DURATION
                },
                w,
                cx,
            )
            .clamp(0., 1.);
        let mut workspace = div()
            .relative()
            .h_full()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(motion::layout(
                format!("page-{}-{:?}", self.loc.space, self.loc.page),
                self.revision,
                main,
            ));
        if self.loc.selected.is_some() {
            workspace = workspace.child(self.detail(w, cx));
        }
        let mut root = div()
            .id("shipforge-workbench")
            .track_focus(&self.root_focus)
            .key_context("ShipForge")
            .relative()
            .size_full()
            .overflow_hidden()
            .text_color(rgb(INK))
            .text_size(px(13.))
            .line_height(relative(1.5))
            .bg(rgb(BG))
            .capture_key_down(cx.listener(Self::key))
            .child(
                col().size_full().child(chrome).child(
                    row()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .child(rail)
                        .child(side)
                        .child(workspace),
                ),
            );
        if let Some(layer) = self.form_layer(w, cx) {
            root = root.child(div().absolute().inset_0().opacity(opacity).child(layer));
        }
        if let Some((message, _)) = &self.notice {
            root = root.child(
                row()
                    .absolute()
                    .bottom(px(16.))
                    .right(px(24.))
                    .max_w(px(500.))
                    .rounded(px(7.))
                    .px(px(13.))
                    .py(px(7.))
                    .gap(px(8.))
                    .bg(rgb(INK))
                    .child(icon("check", 13.).text_color(white()))
                    .child(text(message.clone(), 13., 0xffffff)),
            );
        }
        root
    }
}
