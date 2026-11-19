use crate::appearance::{paint as rgb, surface as white};
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
            .when(self.mode != "demo" && !self.smoke, |bar| {
                bar.on_close_window(cx.listener(|s, _, w, cx| {
                    if s.machine_window_close(w, cx) {
                        w.remove_window();
                    }
                }))
            })
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
                                    .child(text(
                                        self.personal.shortcut_label("command_menu"),
                                        10.,
                                        DIM,
                                    )),
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
            .w(px(54.))
            .h_full()
            .flex_shrink_0()
            .items_center()
            .py(px(13.))
            .gap(px(10.))
            .bg(white())
            .border_1()
            .border_color(rgb(LINE))
            .rounded(px(8.))
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
                UiAction::People,
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
            .when(self.mode != "demo", |r| {
                r.child(item(
                    "rail-accounts",
                    "账号列表",
                    "users",
                    UiAction::Accounts,
                    self.loc.section == "accounts",
                ))
            })
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
        .pl(px(
            if matches!(
                page,
                Page::Iterations | Page::Projects | Page::Triage | Page::Views
            ) {
                31.
            } else {
                10.
            },
        ))
        .pr(px(9.))
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
                    .filter(|r| r.members().contains(&self.actor.as_str()))
                    .count()
                    .to_string(),
                10.,
                DIM,
            ))
        })
    }
    pub(crate) fn nav_group(&self, label: &str, pages: &[Page], cx: &Context<Self>) -> Div {
        let mut group = col()
            .gap(px(2.))
            .mb(px(14.))
            .child(text(label, 11., DIM).px(px(9.)).pb(px(4.)));
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
        let setting = setting
            || matches!(self.loc.page, Tags | TagGroups)
            || (self.loc.page == GettingStarted && self.loc.section == "settings");
        let mut list = col().w_full().px(px(8.)).pt(px(6.));
        if setting {
            list = list
                .child(self.personal_navigation(cx))
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
                        Tags,
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
                .child(self.chat_sidebar_group(cx))
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
                        .when(self.writable(), |d| {
                            d.child(self.tool_button(
                                "add-space-item",
                                "在此新建任务",
                                "plus",
                                UiAction::Create("task".into()),
                                cx,
                            ))
                        }),
                )
                .child(self.space_sidebar(cx))
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
            .bg(white())
            .border_1()
            .rounded(px(8.))
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
                    .when(self.writable(), |d| {
                        d.child(
                            primary(self.button(
                                "quick-add",
                                "快捷新建",
                                UiAction::Form(Mode::QuickCreate),
                                cx,
                            ))
                            .size(px(24.))
                            .px_0()
                            .child(icon("plus", 14.)),
                        )
                    }),
            )
            .child(
                div()
                    .id("space-navigation")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .when(!setting && self.guide.visible(), |panel| {
                panel.child(self.guide_sidebar(cx))
            })
            .when(setting, |panel| {
                panel.child(self.nav_item(GettingStarted, cx))
            })
            .child(
                row()
                    .h(px(49.))
                    .px(px(15.))
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(mark(if self.offline {
                        DIM
                    } else if self.error.is_some() {
                        ERROR
                    } else {
                        GREEN
                    }))
                    .child(text(
                        if self.offline {
                            "团队服务离线"
                        } else if self.mode == "team" {
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
    fn editor_row(
        &mut self,
        i: usize,
        e: crate::forms::Editor,
        open_pick: Option<usize>,
        mode: &Mode,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let raw = e.raw(cx);
        let id = e
            .bound
            .as_ref()
            .map(|b| format!("custom-field-{}", b.id))
            .unwrap_or_else(|| {
                e.trigger.map_or_else(
                    || format!("modal-{}", e.spec.key),
                    |t| format!("trigger-{t}-{}", e.spec.key),
                )
            });
        let label = e
            .bound
            .as_ref()
            .map(|b| b.label.as_str())
            .unwrap_or_else(|| crate::locale::label(e.spec.label));
        let mut field = col().w_full().gap(px(4.)).child(text(label, 12., MUTED));
        match e.control {
            Control::Text(ref input) => {
                field = field.child(self.field(&id, label, input, w, cx));
            }
            Control::Pick { value, focus } => {
                let (mut options, allow_empty, multi) = match e.spec.format {
                    Format::Choice(_) if e.bound.is_some() => (
                        e.bound
                            .as_ref()
                            .unwrap()
                            .options
                            .iter()
                            .map(|s| (s.clone(), s.clone()))
                            .collect(),
                        true,
                        false,
                    ),
                    Format::Ref(k) => (self.options(k), true, false),
                    Format::Many(k) => (self.options(k), true, true),
                    Format::Bool => (
                        vec![
                            ("true".into(), crate::locale::label("启用").into()),
                            ("false".into(), crate::locale::label("关闭").into()),
                        ],
                        e.bound.is_some(),
                        false,
                    ),
                    Format::Choice(v) => (
                        v.iter()
                            .map(|(k, l)| {
                                (
                                    k.to_string(),
                                    crate::locale::choice(e.spec.key, k, l).to_string(),
                                )
                            })
                            .collect(),
                        false,
                        false,
                    ),
                    _ => unreachable!(),
                };
                if allow_empty {
                    options.insert(0, (String::new(), crate::locale::label("未设置").into()));
                }
                if let crate::forms::Selection::Many(values) = &value {
                    for id in values {
                        if !options.iter().any(|(v, _)| v == id) {
                            options.push((id.clone(), id.clone()));
                        }
                    }
                }
                let display = if e.spec.format == Format::Many("$workflow_statuses")
                    && let crate::forms::Selection::Many(selected) = &value
                {
                    selected
                        .iter()
                        .filter_map(|id| options.iter().find(|(key, _)| key == id))
                        .map(|(_, label)| label.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ")
                } else if multi {
                    options
                        .iter()
                        .filter(|(v, _)| !v.is_empty() && value.contains(v))
                        .map(|(_, l)| l.as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                } else {
                    options
                        .iter()
                        .find(|(v, _)| value.contains(v))
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
                    let search = self
                        .form
                        .as_ref()
                        .and_then(|form| form.picker_search.clone());
                    let query = search
                        .as_ref()
                        .map(|field| field.value(cx).trim().to_owned())
                        .unwrap_or_default();
                    let folded = query.to_lowercase();
                    if let Some(search) = search {
                        field = field.child(self.field(
                            &format!("choice-search-{i}"),
                            crate::locale::label("搜索标签"),
                            &search,
                            w,
                            cx,
                        ));
                    }
                    for (index, (v, l)) in options.iter().enumerate() {
                        if !folded.is_empty()
                            && (v.is_empty() || !l.to_lowercase().contains(&folded))
                        {
                            continue;
                        }
                        let active = value.contains(v);
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
                    if matches!(e.spec.format, Format::Many("$tag_choices"))
                        && !query.is_empty()
                        && !options
                            .iter()
                            .any(|(id, title)| id == &query || title.to_lowercase() == folded)
                        && !self
                            .list("tag")
                            .iter()
                            .any(|tag| tag.title.to_lowercase() == folded)
                    {
                        let label =
                            format!("{}「{}」", crate::locale::label("创建并选中标签"), query);
                        menu = menu.child(
                            self.button(
                                format!("field-{i}-create"),
                                label.clone(),
                                UiAction::Choose(i, query),
                                cx,
                            )
                            .w_full()
                            .justify_start()
                            .child(icon("plus", 13.))
                            .child(label),
                        );
                    }
                    field = field.child(menu);
                }
            }
        }
        if e.spec.key == "path"
            || e.spec.key == "workspace"
                && matches!(
                    mode,
                    Mode::Machine { stop: false } | Mode::LocalDirectory { diagnose: false }
                )
        {
            let save = match mode {
                Mode::Download(_, file) => {
                    Some(file["filename"].as_str().unwrap_or("attachment").to_owned())
                }
                Mode::Conversation(crate::app::conversation_ui::CommandForm::Image(_)) => {
                    Some("ShipForge-conversation.png".into())
                }
                Mode::Export => Some("shipforge-workspace.json".to_owned()),
                _ => None,
            };
            field = field.child(
                outlined(self.button(
                    format!("browse-{i}"),
                    "浏览文件",
                    UiAction::Browse { index: i, save },
                    cx,
                ))
                .child(crate::locale::label("浏览…")),
            );
        }
        field
    }
    fn form_layer(&mut self, w: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.form.as_ref()?;
        let mode = f.mode.clone();
        let entries = f.entries.clone();
        let trigger_ids = f.triggers.iter().map(|t| t.id).collect::<Vec<_>>();
        let hook_ids = f
            .triggers
            .iter()
            .filter_map(|t| {
                t.original
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|_| t.original.get("type").and_then(Value::as_str) == Some("webhook"))
                    .map(|id| (t.id, id.to_owned()))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let open_pick = f.open_pick;
        let error = f.error.clone();
        let search_term = f.entries.first().map(|e| e.raw(cx)).unwrap_or_default();
        let results = f.search_results.clone();
        let preview = f.preview.clone();
        let viewport = w.viewport_size();
        let max_w = f32::from(viewport.width);
        let max_h = f32::from(viewport.height);
        let width = if mode.automation() { 860. } else { 620. };
        let width = f32::min(width, (max_w - 40.).max(340.));
        let mut body = col().w_full().gap(px(12.)).px(px(21.)).py(px(14.));
        if let Mode::OrgDepart(_, action) = &mode {
            body = body.child(text(
                if *action == "dissolve" {
                    "删除后将清除组织、空间和其中的数据，此操作无法撤销。"
                } else {
                    "离开后，你将失去此组织和所属空间的访问权限。"
                },
                13.,
                ERROR,
            ));
        }
        if let Mode::Identity(f) = &mode
            && let Some(instructions) = f.instructions()
        {
            body = body.child(text(instructions, 14., INK));
        }
        if let Mode::Forge(f) = &mode {
            body = body.child(self.repository_content(f, cx));
        }
        if let Mode::Bot(f) = &mode {
            body = body.child(self.bot_content(f, cx));
        }
        if let Mode::Webhook(f) = &mode {
            body = body.child(self.webhook_content(f, cx));
        }
        if let Mode::MachineManagement(f) = &mode {
            body = body.child(self.machine_management_content(f, cx));
        }
        if let Mode::Device(dialog) = &mode {
            body = body.child(self.device_content(dialog, cx));
        }
        if let Mode::GitWorkspace(dialog) = &mode {
            body = body.child(self.git_content(dialog, cx));
        }
        if let Mode::Output { content, .. } = &mode {
            body = body.child(text(content.clone(), 13., INK)).child(
                outlined(self.button(
                    "copy-output",
                    "复制结果",
                    UiAction::Copy(content.clone()),
                    cx,
                ))
                .child(icon("copy", 13.))
                .child(crate::locale::label("复制")),
            );
        }
        if let Mode::Machine { stop: false } = &mode {
            body=body.child(text("已有配置可直接启动。新配置会启用只读示例 Runtime「inspect-context」，可另选外部程序。",13.,MUTED));
        }
        if matches!(mode, Mode::MissingMachine) {
            body = body.child(text(
                "先连接一台装有运行工具的机器，再为 Agent 选择执行环境。",
                14.,
                INK,
            ));
        }
        if matches!(mode, Mode::SetupOrganization) {
            body = body.child(text(
                "组织用于归集团队的空间与成员。地址标识可用小写字母、数字和短横线。",
                13.,
                MUTED,
            ));
        }
        if matches!(mode, Mode::SetupSpace { .. }) {
            body = body.child(text(
                "选择主要工作类型和起始任务类型。之后可以在空间设置中调整。",
                13.,
                MUTED,
            ));
        }
        if let Mode::Confirm { .. } = &mode {
            body = body.child(text("请确认本次操作。", 14., MUTED));
        }
        if let Mode::Work(crate::app::workspace_ui::WorkForm::Confirm { names, .. }) = &mode {
            body = body.child(text(
                format!("确认后将清除这些字段的旧值：{names}"),
                14.,
                INK,
            ));
        }
        if let Mode::Work(crate::app::workspace_ui::WorkForm::Views) = &mode {
            body = body.child(self.all_saved_views(cx));
        }
        if let Mode::DeactivateSpace {
            space, unfinished, ..
        } = &mode
        {
            body = body
                .child(headline(space.title.clone(), 14.))
                .child(text(
                    "停用后，空间中的历史记录仍可查看。恢复后可继续编辑。",
                    13.,
                    MUTED,
                ))
                .child(
                    row()
                        .gap(px(6.))
                        .child(text("未完成任务", 13., MUTED))
                        .child(text(unfinished.to_string(), 13., INK)),
                );
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
        if mode.automation() {
            let mut left = col().flex_1().min_w_0().gap(px(13.));
            let mut right = col().flex_1().min_w_0().gap(px(11.)).child(
                row()
                    .justify_between()
                    .child(headline("触发器", 14.))
                    .child(
                        outlined(self.button(
                            "add-trigger",
                            "添加触发器",
                            UiAction::AddTrigger,
                            cx,
                        ))
                        .child(icon("plus", 13.))
                        .child(crate::locale::label("添加触发器")),
                    ),
            );
            for (i, e) in entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.trigger.is_none() && e.spec.key != "notify")
            {
                left = left.child(self.editor_row(i, e.clone(), open_pick, &mode, w, cx));
            }
            for (index, trigger) in trigger_ids.into_iter().enumerate() {
                let raw = |key: &str| {
                    entries
                        .iter()
                        .find(|e| e.trigger == Some(trigger) && e.spec.key == key)
                        .map(|e| e.raw(cx))
                        .unwrap_or_default()
                };
                let kind = raw("type");
                let freq = raw("frequency");
                let mut card = col()
                    .w_full()
                    .min_w_0()
                    .gap(px(7.))
                    .p(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(0xfafafd))
                    .child(
                        row()
                            .justify_between()
                            .child(headline(format!("触发器 {}", index + 1), 13.))
                            .child(self.tool_button(
                                &format!("remove-trigger-{trigger}"),
                                "移除触发器",
                                "x",
                                UiAction::RemoveTrigger(trigger),
                                cx,
                            )),
                    );
                for (i, e) in entries.iter().enumerate().filter(|(_, e)| {
                    e.trigger == Some(trigger)
                        && crate::forms::trigger_field_visible(e.spec.key, &kind, &freq)
                }) {
                    card = card.child(self.editor_row(i, e.clone(), open_pick, &mode, w, cx));
                }
                if kind == "webhook" {
                    if let Mode::Edit(rule) = &mode
                        && let Some(id) = hook_ids.get(&trigger)
                    {
                        card = card.child(
                            outlined(self.button(
                                format!("edit-hook-{id}"),
                                "安全设置",
                                UiAction::Webhook(crate::app::webhook_ui::Action::Settings(
                                    rule.id.clone(),
                                    id.clone(),
                                )),
                                cx,
                            ))
                            .child(crate::locale::label("安全设置")),
                        );
                    } else {
                        card = card.child(text("保存后可复制私密 URL。", 12., MUTED));
                    }
                }
                right = right.child(card);
            }
            if let Some((i, e)) = entries
                .iter()
                .enumerate()
                .find(|(_, e)| e.trigger.is_none() && e.spec.key == "notify")
            {
                right = right.child(self.editor_row(i, e.clone(), open_pick, &mode, w, cx));
            }
            body = if width >= 720. {
                body.child(
                    row()
                        .w_full()
                        .items_start()
                        .gap(px(22.))
                        .child(left)
                        .child(right),
                )
            } else {
                body.child(left).child(right)
            };
        } else {
            let connector_provider = if matches!(&mode,Mode::Create(k) if k=="connector")
                || matches!(&mode,Mode::Edit(r) if r.kind=="connector")
            {
                entries
                    .iter()
                    .find(|e| e.spec.key == "provider")
                    .map(|e| e.raw(cx))
            } else {
                None
            };
            let move_tasks = entries
                .iter()
                .find(|e| e.spec.key == "unfinished")
                .is_some_and(|e| e.raw(cx) == "move");
            let proxy_mode = entries
                .iter()
                .find(|e| e.spec.key == "mode")
                .map(|e| e.raw(cx))
                .unwrap_or_default();
            let hidden_git_fields = if matches!(mode, Mode::GitWorkspace(_)) {
                crate::app::git_workspace_ui::hidden_fields(&entries, cx)
            } else {
                Vec::new()
            };
            let hide_fork_branch = matches!(
                mode,
                Mode::Workflow {
                    op: "chat.fork.workspace",
                    ..
                }
            ) && entries
                .iter()
                .find(|entry| entry.spec.key == "workspace_mode")
                .is_none_or(|entry| entry.raw(cx) != "branch");
            for (i, e) in entries.into_iter().enumerate().filter(|(_, e)| {
                if hide_fork_branch && matches!(e.spec.key, "checkout_branch" | "target_machine_id")
                {
                    return false;
                }
                if let Mode::Device(dialog) = &mode
                    && !dialog.field_visible(e.spec.key, &proxy_mode)
                {
                    return false;
                }
                if hidden_git_fields.contains(&e.spec.key) {
                    return false;
                }
                if matches!(mode, Mode::DeactivateSpace { .. })
                    && e.spec.key == "target"
                    && !move_tasks
                {
                    return false;
                }
                connector_provider
                    .as_deref()
                    .is_none_or(|p| crate::forms::connector_field_visible(e.spec.key, p))
            }) {
                body = body.child(self.editor_row(i, e, open_pick, &mode, w, cx));
            }
        }
        if matches!(&mode,Mode::Create(k) if k=="view")
            || matches!(&mode,Mode::Edit(r) if r.kind=="view")
        {
            let preview = &self.work.preview;
            let mut panel = col()
                .p(px(10.))
                .rounded(px(6.))
                .bg(rgb(PALE))
                .gap(px(3.))
                .child(headline(
                    format!("预览 · {} 项任务", self.work.preview_total),
                    13.,
                ));
            for t in preview.iter().take(5) {
                panel = panel.child(text(format!("{}   {}", t.code(), t.title), 12., MUTED));
            }
            body = body.child(panel);
        }
        if matches!(mode, Mode::Message(_)) {
            body = body.child(self.slash_panel(cx));
        }
        if let Mode::Conversation(crate::app::conversation_ui::CommandForm::Shortcuts(task)) = &mode
        {
            body = body.child(self.shortcut_panel(task, cx));
        }
        if let Mode::ChatRow(room) = &mode {
            body = body.child(self.chat_row_actions(room, cx));
        }
        if matches!(mode, Mode::Search | Mode::QuickCreate) {
            body = body.child(self.navigation_commands(
                &search_term,
                matches!(mode, Mode::QuickCreate),
                cx,
            ));
        }
        if matches!(mode, Mode::Search | Mode::BindChat(_)) {
            let mut ordered: Vec<_> = results
                .iter()
                .filter(|r| !matches!(mode, Mode::BindChat(_)) || r.kind == "task")
                .take(150)
                .collect();
            ordered.sort_by(|a, b| a.kind.cmp(&b.kind));
            let mut group = "";
            for (i, r) in ordered.into_iter().enumerate() {
                if group != r.kind && matches!(mode, Mode::Search) {
                    group = &r.kind;
                    body = body.child(headline(crate::forms::noun(group), 12.));
                }
                let action = if let Mode::BindChat(room) = &mode {
                    UiAction::Chat(crate::app::chat_actions::Action::Bind {
                        room: room.clone(),
                        task: r.id.clone(),
                    })
                } else {
                    UiAction::OpenSearch(r.clone())
                };
                body = body.child(
                    self.button(format!("search-{i}"), r.title.clone(), action, cx)
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
                        .child(text(
                            format!("{} · {}", self.name("space", &r.space), r.code()),
                            11.,
                            DIM,
                        )),
                );
            }
            if results.is_empty() {
                body = body.child(text(
                    if search_term.trim().is_empty() {
                        if matches!(mode, Mode::BindChat(_)) {
                            "输入任务标题或编号"
                        } else {
                            "输入关键词，在已加入的空间中查找"
                        }
                    } else {
                        "没有匹配的对象"
                    },
                    12.,
                    MUTED,
                ));
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
            body = body.child(text(crate::locale::label(&error).to_owned(), 13., ERROR));
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
                    .child(crate::locale::label("取消")),
            );
        }
        footer = footer.child(
            primary(self.button("modal-submit", confirm, UiAction::Submit, cx))
                .disabled(self.busy)
                .child(crate::locale::label(confirm)),
        );
        let popup = col()
            .id("form-dialog")
            .test_support()
            .role(Role::Dialog)
            .aria_label(crate::locale::label(&mode.heading()).to_owned())
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
                    .child(headline(
                        crate::locale::label(&mode.heading()).to_owned(),
                        16.,
                    ))
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
            .border_1()
            .border_color(rgb(LINE))
            .rounded(px(8.))
            .bg(white())
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
            .capture_action(cx.listener(
                |this, _: &gpui_kit::component::input::IndentInline, w, cx| {
                    if !this.slash_key("tab", w, cx) {
                        cx.propagate();
                    }
                },
            ))
            .capture_action(
                cx.listener(|this, _: &gpui_kit::component::input::Escape, w, cx| {
                    if !this.slash_key("escape", w, cx) {
                        cx.propagate();
                    }
                }),
            )
            .capture_action(cx.listener(Self::personal_enter))
            .child(
                col().size_full().child(chrome).child(
                    row()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .items_start()
                        .justify_start()
                        .pt(px(2.))
                        .pb(px(8.))
                        .pl(px(8.))
                        .pr(px(8.))
                        .gap(px(6.))
                        .bg(rgb(0xf0f0f6))
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
                    .child(text(message.clone(), 13., 0xffffff))
                    .when_some(self.inbox.banner.clone(), |b, link| {
                        b.child(
                            self.button(
                                "open-inbox-banner",
                                "打开通知",
                                UiAction::Inbox(crate::app::inbox_ui::Action::Link(link)),
                                cx,
                            )
                            .bg(white())
                            .child(crate::locale::label("打开通知")),
                        )
                    })
                    .when(
                        self.chat_undo
                            .as_ref()
                            .is_some_and(|(space, _, _)| space == &self.loc.space),
                        |b| {
                            b.child(
                                self.button(
                                    "undo-hide-chat",
                                    crate::locale::undo_chat_hide().to_owned(),
                                    UiAction::Chat(crate::app::chat_actions::Action::Undo),
                                    cx,
                                )
                                .bg(white())
                                .child(crate::locale::undo_chat_hide()),
                            )
                        },
                    ),
            );
        }
        root
    }
}
