use crate::appearance::{paint as rgb, surface as white};
use crate::{
    app::{UiAction, Workbench},
    catalog::{Page, Record, label_value, summary, value_text},
    controls::*,
    forms::Mode,
    motion,
};
use gpui_kit::{
    base::{Button, TestSupportExt},
    prelude::*,
    *,
};
use serde_json::{Value, json};
#[path = "chat_panel.rs"]
mod chat_panel;
#[path = "planning.rs"]
mod planning;
#[path = "tokens_ui.rs"]
mod tokens_ui;

#[derive(Clone)]
struct DraggedTask(Record, String);
#[derive(Clone)]
struct DraggedView(Record);
impl Render for DraggedView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        text(self.0.title.clone(), 13., INK)
    }
}
impl Render for DraggedTask {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        row()
            .max_w(px(260.))
            .p(px(8.))
            .gap(px(6.))
            .rounded(px(7.))
            .bg(white())
            .border_1()
            .border_color(rgb(LINE))
            .shadow_md()
            .child(icon("list-checks", 13.))
            .child(text(self.0.title.clone(), 13., INK).truncate())
    }
}

impl Workbench {
    pub fn page_body(&mut self, w: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.loc.page;
        if p == Page::Members && self.loc.section == "people" {
            return self.people_panel(cx).into_any_element();
        }
        if p == Page::Inbox {
            return self.inbox_page(cx).into_any_element();
        }
        if p == Page::Settings && self.loc.section == "notifications" {
            return self.notification_settings(cx).into_any_element();
        }
        if matches!(p, Page::Agents | Page::Crews) {
            return self.agent_directory_page(cx).into_any_element();
        }
        if p == Page::GettingStarted {
            return self.guide_page(f32::from(w.viewport_size().width), cx);
        }
        if p == Page::Settings && matches!(self.loc.section.as_str(), "preferences" | "shortcuts") {
            return self.personal_page(cx);
        }
        let info = p.info();
        let page_title = self
            .loc
            .view
            .as_ref()
            .map(|r| r.title.as_str())
            .unwrap_or(info.label);
        let mut header = row()
            .h(px(44.))
            .w_full()
            .border_b_1()
            .border_color(rgb(LINE))
            .px(px(16.))
            .gap(px(8.))
            .flex_shrink_0()
            .child(
                icon(
                    if matches!(
                        p,
                        Page::Settings
                            | Page::Fields
                            | Page::Types
                            | Page::Relations
                            | Page::Statuses
                            | Page::Transfer
                    ) {
                        "settings"
                    } else {
                        "boxes"
                    },
                    15.,
                )
                .text_color(rgb(MUTED)),
            )
            .child(text(self.space_name(), 12., MUTED))
            .child(text("/", 12., DIM))
            .child(headline(page_title.to_owned(), 14.))
            .child(div().flex_1());
        if p == Page::Organizations {
            header = header.child(self.org_controls(cx));
        }
        if p == Page::Docs {
            header = header.child(
                outlined(self.button(
                    "open-document-link",
                    "打开文档链接",
                    UiAction::Form(Mode::Conversation(
                        crate::app::conversation_ui::CommandForm::OpenDocument,
                    )),
                    cx,
                ))
                .child("打开文档链接"),
            );
        }
        if p == Page::Docs && self.writable() {
            header = header.child(
                outlined(self.button(
                    "upload-document",
                    "从文件新建文档",
                    UiAction::Form(Mode::Upload(None)),
                    cx,
                ))
                .child(icon("upload", 14.))
                .child(crate::locale::label("上传文档")),
            );
        }
        if p == Page::Machines && self.mode != "demo" {
            header = header.child(
                outlined(self.button(
                    "machine-service-policy",
                    "本机服务设置",
                    UiAction::MachineManagement(crate::app::machine_management::Action::Settings),
                    cx,
                ))
                .child(crate::locale::label("本机服务设置")),
            );
            for (diagnose, label) in [(false, "授权本机工作目录"), (true, "本机进程与日志")]
            {
                header = header.child(
                    outlined(self.button(
                        format!("machine-local-{diagnose}"),
                        label,
                        UiAction::Form(Mode::LocalDirectory { diagnose }),
                        cx,
                    ))
                    .child(crate::locale::label(label)),
                );
            }
            if self.access_identity["scope"] == "write" && self.access_identity["personal"] == false
            {
                header = header.child(
                    outlined(self.button(
                        "machine-personal-token",
                        "创建机器连接令牌",
                        UiAction::NewPersonalToken(true),
                        cx,
                    ))
                    .child(crate::locale::label("创建机器连接令牌")),
                );
            }
            header = header.child(
                outlined(self.button("machine-probe", "扫描本机 Runtime", UiAction::Probe, cx))
                    .child(crate::locale::label("探测 Runtime")),
            );
            header = header
                .child(
                    outlined(self.button(
                        "stop-machine",
                        "停止本机工作机",
                        UiAction::Form(Mode::Machine { stop: true }),
                        cx,
                    ))
                    .child(crate::locale::label("停止本机")),
                )
                .child(
                    primary(self.button(
                        "start-machine",
                        "启动本机工作机",
                        UiAction::Form(Mode::Machine { stop: false }),
                        cx,
                    ))
                    .child(icon("play", 13.))
                    .child(crate::locale::label("启动本机工作机")),
                );
        }
        if self.loading {
            header = header.child(text("读取中…", 11., DIM));
        }
        if matches!(p, Page::Tags | Page::TagGroups) {
            header = header.child(
                outlined(self.button(
                    "find-tags",
                    "查找标签",
                    UiAction::Form(Mode::FindLabels),
                    cx,
                ))
                .child(crate::locale::label("查找标签")),
            );
            let (label, page) = if p == Page::Tags {
                ("标签分组", Page::TagGroups)
            } else {
                ("任务标签", Page::Tags)
            };
            header = header.child(
                outlined(self.button("tag-groups-switch", label, UiAction::Go(page), cx))
                    .child(crate::locale::label(label)),
            );
            if p == Page::Tags && self.writable() {
                header = header.child(
                    outlined(self.button(
                        "merge-tags",
                        "合并标签",
                        UiAction::Form(Mode::MergeTags),
                        cx,
                    ))
                    .child(crate::locale::label("合并标签")),
                );
            }
        }
        if matches!(p, Page::Tasks | Page::Mine) && self.writable() {
            header = header.child(
                outlined(self.button(
                    "bulk-task-tags",
                    "批量处理标签",
                    UiAction::Form(Mode::BulkTags),
                    cx,
                ))
                .child(crate::locale::label("批量处理标签")),
            );
        }
        if matches!(p, Page::Members | Page::Settings) {
            header = header.child(
                outlined(self.button("join-org", "接受邀请", UiAction::Form(Mode::JoinInvite), cx))
                    .child(crate::locale::label("接受邀请")),
            );
        }
        if p == Page::Members && self.writable() && matches!(self.role.as_str(), "admin" | "owner")
        {
            let viewing = self.loc.section == "invitations";
            let label = if viewing {
                "成员列表"
            } else {
                "邀请记录"
            };
            header = header
                .child(
                    outlined(self.button(
                        "invitation-list",
                        label,
                        UiAction::Section(if viewing {
                            "".into()
                        } else {
                            "invitations".into()
                        }),
                        cx,
                    ))
                    .child(crate::locale::label(label)),
                )
                .child(
                    outlined(self.button(
                        "invite-team-link",
                        "邀请链接",
                        UiAction::Form(Mode::Invite { email: false }),
                        cx,
                    ))
                    .child(crate::locale::label("邀请链接")),
                )
                .child(
                    primary(self.button(
                        "invite-team-email",
                        "邮箱邀请",
                        UiAction::Form(Mode::Invite { email: true }),
                        cx,
                    ))
                    .child(crate::locale::label("邮箱邀请")),
                );
        }
        if p.creatable()
            && p != Page::Statuses
            && if p == Page::Spaces {
                self.can_create_space()
            } else {
                self.writable()
            }
        {
            let k = info.kind;
            header = header.child(
                primary(self.button(
                    "new-record",
                    crate::locale::new_title(info.noun),
                    UiAction::Create(k.into()),
                    cx,
                ))
                .h(px(26.))
                .child(icon("plus", 13.))
                .child(crate::locale::new_title(info.noun)),
            );
        }
        let mut root = col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(rgb(BG))
            .child(header);
        if self.current_archived() {
            root = root.child(
                text("该空间已停用，历史记录为只读。", 13., MUTED)
                    .px(px(16.))
                    .py(px(6.))
                    .bg(rgb(0xf5efd9)),
            );
        }
        if let Some(error) = self.error.as_ref().or_else(|| {
            self.errors.get(if p == Page::Statuses {
                "status_library"
            } else {
                info.kind
            })
        }) {
            root = root.child(
                row()
                    .w_full()
                    .px(px(16.))
                    .py(px(7.))
                    .gap(px(7.))
                    .bg(rgb(0xffeeef))
                    .child(icon("triangle-alert", 14.).text_color(rgb(ERROR)))
                    .child(text(error.clone(), 12., ERROR).flex_1())
                    .child(
                        self.button("retry-load", "重试", UiAction::Refresh, cx)
                            .child(crate::locale::label("重试")),
                    ),
            );
        }
        match p {
            Page::Statuses => root.child(self.status_library_panel(cx)).into_any_element(),
            Page::Spaces => root.child(self.space_directory(cx)).into_any_element(),
            Page::SpaceModules => root.child(self.space_module_page(cx)).into_any_element(),
            page if self.module_disabled(page) => {
                root.child(self.space_module_page(cx)).into_any_element()
            }
            Page::Triage => root.child(self.review_page(cx)).into_any_element(),
            Page::Tasks | Page::Mine | Page::Dashboard => root
                .child(self.task_page(f32::from(w.viewport_size().width), cx))
                .into_any_element(),
            Page::Settings => root.child(self.settings_page(cx)).into_any_element(),
            Page::Iterations => root.child(self.iteration_page(cx)).into_any_element(),
            Page::Transfer => root.child(self.transfer_page(cx)).into_any_element(),
            Page::Members if self.loc.section == "invitations" => {
                root.child(self.member_invitations(cx)).into_any_element()
            }
            Page::Market => root.child(self.market_page(cx)).into_any_element(),
            Page::Skills => root
                .child(self.skill_toolbar(cx))
                .child(self.record_page(cx))
                .into_any_element(),
            Page::Connectors => root
                .child(self.bot_toolbar(None, cx))
                .child(
                    row().px(px(14.)).py(px(8.)).child(
                        outlined(self.button(
                            "connector-skills",
                            "空间技能",
                            UiAction::Go(Page::Skills),
                            cx,
                        ))
                        .child(crate::locale::label("空间技能")),
                    ),
                )
                .child(self.record_page(cx))
                .into_any_element(),
            Page::Chats => root
                .child(self.chat_invitations(cx))
                .child(self.chat_list_page(cx))
                .into_any_element(),
            _ => root.child(self.record_page(cx)).into_any_element(),
        }
    }
    fn member_invitations(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut list = col()
            .id("organization-invitations")
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .p(px(16.))
            .gap(px(8.));
        let invites = self.list("member_invitation");
        if invites.is_empty() {
            list = list.child(text("暂无邀请记录", 13., MUTED));
        }
        for invite in invites {
            let state = invite.field("state");
            let status = match state {
                "active" => "可加入",
                "revoked" => "已停用",
                "accepted" => "已接受",
                "expired" => "已过期",
                _ => "历史记录",
            };
            let mut item = row()
                .w_full()
                .gap(px(12.))
                .child(
                    headline(
                        if invite.field("channel") == "link" {
                            crate::locale::label(&invite.title).to_owned()
                        } else {
                            invite.title.clone()
                        },
                        13.,
                    )
                    .flex_1()
                    .truncate(),
                )
                .child(text(label_value("role", invite.field("role")), 12., MUTED))
                .child(chip(status, PALE, INK));
            if let Some(mail_status) = invite.fields.get("mail")
                && let Some(state) = mail_status["state"].as_str()
            {
                let mail = match state {
                    "queued" => "邮件待发送",
                    "sending" => "正在提交邮件",
                    "retry" => "邮件稍后重试",
                    "accepted" => "邮件服务已接收",
                    "failed" => "邮件服务拒收",
                    "unconfirmed" => "投递结果待确认",
                    _ => "邮件已取消",
                };
                item = item.child(text(crate::locale::label(mail), 12., MUTED));
                if state == "failed" && invite.field("state") == "active" {
                    item = item.child(
                        outlined(self.button(
                            format!("retry-mail-{}", invite.id),
                            "重试邮件",
                            UiAction::Apply(
                                json!({"op":"delivery.retry","id":mail_status["id"]}),
                                "邮件已排入重试".into(),
                            ),
                            cx,
                        ))
                        .child(crate::locale::label("重试邮件")),
                    );
                }
            }
            if state == "active" || (state == "revoked" && invite.field("channel") == "link") {
                let label = if state == "revoked" {
                    "启用并复制"
                } else {
                    "复制链接"
                };
                item = item.child(
                    outlined(self.button(
                        format!("copy-invitation-{}", invite.id),
                        label,
                        UiAction::CopyInvitation(invite.id.clone()),
                        cx,
                    ))
                    .child(crate::locale::label(label)),
                );
            }
            if state == "active" {
                item=item.child(outlined(self.button(format!("revoke-invitation-{}",invite.id),"停用",UiAction::Form(Mode::Confirm{
                    label:crate::locale::disable_invitation(&invite.title),command:json!({"op":"invitation.revoke","space":self.loc.space,"id":invite.id})}),cx)).child(crate::locale::label("停用")));
            }
            list = list.child(item);
        }
        list
    }
    pub(crate) fn pill(
        &self,
        id: String,
        label: &str,
        action: UiAction,
        active: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        self.button(id, label.to_owned(), action, cx)
            .h(px(26.))
            .px(px(8.))
            .when(active, |b| {
                b.bg(rgb(SELECTED)).font_weight(FontWeight::SEMIBOLD)
            })
            .child(crate::locale::label(label).to_owned())
    }
    fn empty_page(
        &self,
        glyph: &str,
        title: &str,
        copy: &str,
        action: Option<(String, UiAction)>,
        cx: &Context<Self>,
    ) -> Div {
        let mut empty = col()
            .w_full()
            .max_w(px(450.))
            .gap(px(8.))
            .items_start()
            .child(
                row()
                    .size(px(44.))
                    .justify_center()
                    .rounded(px(12.))
                    .bg(rgb(0xf0f1f5))
                    .child(icon(glyph, 23.).text_color(rgb(MUTED))),
            )
            .child(headline(title.to_owned(), 19.))
            .child(text(copy.to_owned(), 13., MUTED));
        if let Some((label, action)) = action {
            empty = empty.child(
                primary(self.button("empty-action", label.clone(), action, cx))
                    .mt(px(5.))
                    .h(px(30.))
                    .child(icon("plus", 13.))
                    .child(crate::locale::label(&label).to_owned()),
            );
        }
        row()
            .flex_1()
            .min_h(px(175.))
            .justify_center()
            .p(px(24.))
            .child(empty)
    }
    fn task_page(&self, viewport: f32, cx: &Context<Self>) -> AnyElement {
        let page = self.loc.page;
        let tasks = self.task_rows();
        let dashboard = page == Page::Dashboard || self.loc.layout == "dashboard";
        if page == Page::Triage
            && self.find("space", &self.loc.space).is_some_and(|r| {
                r.fields.get("triage_enabled") == Some(&Value::Bool(false))
                    || r.fields.get("triage_enabled").is_none()
            })
        {
            let action = self
                .find("space", &self.loc.space)
                .filter(|_| self.writable())
                .map(|r| ("设置分拣台".into(), UiAction::Edit(r.clone())));
            return self
                .empty_page(
                    "inbox",
                    "空间分拣台",
                    "启用后，外部来源的工作会先在此等待审核。",
                    action,
                    cx,
                )
                .into_any_element();
        }
        let mut tools = row()
            .id("table-toolbar")
            .w_full()
            .min_h(px(46.))
            .flex_shrink_0()
            .overflow_x_scroll()
            .px(px(12.))
            .gap(px(3.));
        if page == Page::Mine {
            tools = tools
                .child(self.pill(
                    "mine-assigned".into(),
                    "指派给我的",
                    UiAction::Mine(false),
                    !self.loc.mine_created,
                    cx,
                ))
                .child(self.pill(
                    "mine-created".into(),
                    "我创建的",
                    UiAction::Mine(true),
                    self.loc.mine_created,
                    cx,
                ));
        } else {
            tools = tools.child(self.pill(
                "all-types".into(),
                "全部",
                UiAction::TaskType(String::new()),
                self.loc.task_type.is_empty(),
                cx,
            ));
            let types = self.options("$types");
            for (k, l) in types.iter().filter(|_| types.len() > 1) {
                tools = tools.child(self.pill(
                    format!("type-{k}"),
                    l,
                    UiAction::TaskType(k.clone()),
                    self.loc.task_type == *k,
                    cx,
                ));
            }
        }
        if page == Page::Tasks {
            let max_tabs = ((viewport - 470.) / 100.).clamp(0., 5.) as usize;
            for view in self.list("view").iter().take(max_tabs) {
                tools = tools.child(self.saved_view_tab(view, cx));
            }
            tools = tools.child(self.tool_button(
                "more-saved-views",
                "全部保存视图",
                "ellipsis",
                UiAction::Work(crate::app::workspace_ui::Action::ShowViews),
                cx,
            ));
            if let Some(view) = &self.loc.view
                && view.fields.get("can_edit_view") == Some(&json!(true))
            {
                tools = tools.child(self.tool_button(
                    "save-current-view",
                    "保存到当前视图",
                    "save",
                    UiAction::Work(crate::app::workspace_ui::Action::SaveChanges(view.clone())),
                    cx,
                ));
            }
        }
        tools = tools
            .child(div().flex_1().min_w(px(9.)))
            .child(
                self.tool_button(
                    "task-filter",
                    "筛选任务",
                    "list-filter",
                    UiAction::Filter,
                    cx,
                )
                .when(!self.loc.filter.is_empty(), |b| b.bg(rgb(SELECTED))),
            )
            .child(
                self.tool_button(
                    "layout-board",
                    "看板",
                    "columns-3",
                    UiAction::Layout("board".into()),
                    cx,
                )
                .when(self.loc.layout == "board", |b| b.bg(rgb(SELECTED))),
            )
            .child(
                self.tool_button(
                    "layout-list",
                    "列表",
                    "list",
                    UiAction::Layout("list".into()),
                    cx,
                )
                .when(self.loc.layout == "list", |b| b.bg(rgb(SELECTED))),
            )
            .child(
                self.tool_button(
                    "layout-table",
                    "表格",
                    "panels-top-left",
                    UiAction::Layout("table".into()),
                    cx,
                )
                .when(self.loc.layout == "table", |b| b.bg(rgb(SELECTED))),
            )
            .child(
                self.tool_button(
                    "layout-dash",
                    "仪表盘",
                    "chart-no-axes-column-increasing",
                    UiAction::Layout("dashboard".into()),
                    cx,
                )
                .when(dashboard, |b| b.bg(rgb(SELECTED))),
            )
            .child(self.tool_button(
                "save-view",
                "保存为视图",
                "square-pen",
                UiAction::SaveView,
                cx,
            ));
        let mut out = col().flex_1().min_h_0().w_full().child(tools);
        if !self.loc.filter.is_empty() {
            out = out.child(
                row()
                    .px(px(16.))
                    .pb(px(5.))
                    .gap(px(8.))
                    .child(text(
                        format!("{} 项筛选 · {} 条任务", self.loc.filter.len(), tasks.len()),
                        12.,
                        MUTED,
                    ))
                    .child(
                        self.button("remove-filters", "清除筛选", UiAction::ResetFilter, cx)
                            .child(crate::locale::label("清除")),
                    ),
            );
        }
        if dashboard {
            out = out.child(self.dashboard(&tasks, cx));
        } else if tasks.is_empty() {
            let filtered = !self.loc.filter.is_empty()
                || !self.loc.task_type.is_empty()
                || self.loc.view.is_some();
            let cmd = if filtered {
                Some((
                    "清除筛选".into(),
                    if self.loc.view.is_some() {
                        UiAction::Go(Page::Tasks)
                    } else {
                        UiAction::ResetFilter
                    },
                ))
            } else if self.writable() {
                Some(("新建任务".into(), UiAction::Create("task".into())))
            } else {
                None
            };
            out = out.child(self.empty_page(
                page.info().icon,
                if filtered {
                    "没有匹配的任务"
                } else {
                    "还没有任务"
                },
                page.info().empty,
                cmd,
                cx,
            ));
        } else if self.loc.layout == "board" {
            out = out.child(self.board(&tasks, viewport, cx));
        } else {
            out = out.child(self.task_list(&tasks, self.loc.layout == "table", cx));
        }
        out.into_any_element()
    }
    fn drop_target(&self, element: Div, group: &str, key: &str, cx: &Context<Self>) -> Div {
        if !self.writable() {
            return element;
        }
        let group = group.to_owned();
        let key = key.to_owned();
        element.on_drop::<DraggedTask>(cx.listener(move|this,drag:&DraggedTask,window,cx|{
            let task=&drag.0;
            if task.space!=this.loc.space||drag.1==key{return;}
            let mut patch=json!({(&group):key});
            if group=="assignee"{
                let mut members=task.members();
                members.retain(|id|*id!=drag.1);
                if !key.is_empty() && !members.contains(&key.as_str()){members.push(&key);}
                patch=json!({"assignees":members});
            }
            this.act(UiAction::Apply(json!({"op":"update","kind":"task","id":task.id,"expected_rev":task.rev,"fields":patch}),"任务已移动".into()),window,cx);
        }))
    }
    fn saved_view_tab(&self, v: &Record, cx: &Context<Self>) -> Button {
        let label = if v.fields.get("personally_pinned") == Some(&json!(true)) {
            format!("↑ {}", v.title)
        } else {
            v.title.clone()
        };
        let mut button = self
            .button(
                format!("saved-view-tab-{}", v.id),
                label.clone(),
                UiAction::OpenView(v.clone()),
                cx,
            )
            .child(text(label, 12., INK));
        if v.field("visibility") == "private" && v.field("creator") == self.actor {
            let target = v.clone();
            button = button
                .on_drag(DraggedView(v.clone()), |drag, _, _, cx| {
                    cx.new(|_| drag.clone())
                })
                .on_drop::<DraggedView>(cx.listener(
                    move |this, drag: &DraggedView, window, cx| {
                        if drag.0.id != target.id
                            && drag.0.space == target.space
                            && drag.0.fields.get("personally_pinned")
                                == target.fields.get("personally_pinned")
                        {
                            this.act(
                                UiAction::Apply(
                                    json!({"op":"view.organize","id":drag.0.id,"before":target.id}),
                                    "视图顺序已保存".into(),
                                ),
                                window,
                                cx,
                            );
                        }
                    },
                ));
        }
        button
    }
    pub fn all_saved_views(&self, cx: &Context<Self>) -> Div {
        let mut list = col().gap(px(4.)).child(
            self.button(
                "open-saved-view-link",
                "打开视图链接",
                UiAction::Form(Mode::Work(crate::app::workspace_ui::WorkForm::OpenView)),
                cx,
            )
            .child("打开视图链接"),
        );
        for v in self.list("view") {
            list = list.child(
                row()
                    .child(self.saved_view_tab(v, cx))
                    .child(self.view_tools(v, cx)),
            );
        }
        list
    }
    fn board(&self, tasks: &[Record], viewport: f32, cx: &Context<Self>) -> AnyElement {
        let group = self
            .loc
            .view
            .as_ref()
            .map(|r| r.field("group"))
            .filter(|g| matches!(*g, "priority" | "type" | "assignee"))
            .unwrap_or("status");
        let mut groups: Vec<(String, String, u32)> = match group {
            "priority" => self
                .priority_options()
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string(), VIOLET))
                .collect(),
            "type" => self
                .options("$types")
                .into_iter()
                .map(|(a, b)| (a, b, GREEN))
                .collect(),
            "assignee" => {
                std::iter::once((String::new(), crate::locale::label("未指派").into(), VIOLET))
                    .chain(
                        self.list("member")
                            .iter()
                            .map(|r| (r.id.clone(), r.title.clone(), VIOLET)),
                    )
                    .collect()
            }
            _ => self
                .status_rows(&self.loc.task_type)
                .into_iter()
                .filter(|s| s.field("category") != "triage" || self.loc.page == Page::Triage)
                .map(|s| {
                    (
                        s.id.clone(),
                        s.localized_title().to_owned(),
                        self.status_color(s),
                    )
                })
                .collect(),
        };
        for key in tasks.iter().flat_map(|r| r.grouped_values(group)) {
            if !groups.iter().any(|(v, _, _)| v == key) {
                let label = if group == "assignee" {
                    self.handler_name(key)
                } else {
                    self.name(
                        if group == "type" {
                            "task_type"
                        } else {
                            "status"
                        },
                        key,
                    )
                };
                groups.push((key.into(), label, VIOLET));
            }
        }
        let active = groups
            .iter()
            .filter(|(key, _, _)| {
                tasks
                    .iter()
                    .any(|task| task.grouped_values(group).contains(&key.as_str()))
            })
            .count();
        let collapsed = groups.len() - active;
        let side = if self.sidebar {
            if viewport >= 1100. { 272. } else { 196. }
        } else {
            0.
        };
        let usable = (viewport - 60. - side - 32. - 26.).max(220.);
        let fold_width = if collapsed == 0 {
            0.
        } else {
            (usable * 0.18).clamp(122., 250.)
        };
        let column_width =
            ((usable - fold_width - 9. * active as f32) / active.max(1) as f32).clamp(222., 338.);
        let stack_width = (column_width * active as f32
            + fold_width
            + 9. * active.saturating_sub(usize::from(collapsed == 0)) as f32)
            .max(usable);
        let mut board = row()
            .w(px(stack_width))
            .h_full()
            .items_start()
            .justify_start()
            .gap(px(9.));
        let mut empty = col()
            .w(px(fold_width))
            .flex_shrink_0()
            .gap(px(5.))
            .child(text("折叠的列", 12., MUTED).pb(px(3.)));
        for (key, label, color) in groups {
            let items: Vec<_> = tasks
                .iter()
                .filter(|r| r.grouped_values(group).contains(&key.as_str()))
                .collect();
            if items.is_empty() {
                let target = row()
                    .h(px(31.))
                    .w_full()
                    .px(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(white())
                    .gap(px(7.))
                    .child(mark(color))
                    .child(
                        text(label.clone(), 12., MUTED)
                            .id(format!("status-empty-{group}-{key}"))
                            .test_support()
                            .role(Role::Heading)
                            .aria_label(label),
                    )
                    .child(div().flex_1())
                    .child(text("0", 11., DIM));
                empty = empty.child(self.drop_target(target, group, &key, cx));
                continue;
            }
            let mut cards = col()
                .id(format!("group-scroll-{key}"))
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap(px(7.))
                .p(px(5.));
            let n = items.len();
            for r in items {
                cards = cards.child(motion::layout(
                    format!("{}-task-{group}-{key}-{}", self.loc.space, r.id),
                    self.revision,
                    self.task_card(r, &key, cx),
                ));
            }
            let col = col()
                .w(px(column_width))
                .h_full()
                .min_h_0()
                .flex_shrink_0()
                .rounded(px(8.))
                .bg(rgb(0xf8f8fb))
                .child(
                    row()
                        .h(px(35.))
                        .px(px(10.))
                        .gap(px(7.))
                        .child(mark(color))
                        .child(
                            headline(label.clone(), 13.)
                                .id(format!("board-heading-{group}-{key}"))
                                .test_support()
                                .role(Role::Heading)
                                .aria_label(label),
                        )
                        .child(text(n.to_string(), 11., DIM)),
                )
                .child(cards);
            board = board.child(self.drop_target(col, group, &key, cx));
        }
        if collapsed > 0 {
            board = board.child(empty.ml_auto());
        }
        div()
            .id("task-board-scroll")
            .overflow_x_scroll()
            .w_full()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .px(px(13.))
            .pb(px(13.))
            .child(board)
            .into_any_element()
    }
    fn task_labels(&self, r: &Record) -> Div {
        let mut labels = row().flex_wrap().gap(px(5.));
        for id in r
            .fields
            .get("labels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let color = self
                .find("tag", id)
                .and_then(|r| r.field("color").strip_prefix('#'))
                .and_then(|c| u32::from_str_radix(c, 16).ok())
                .unwrap_or(MUTED);
            labels = labels.child(
                row()
                    .items_center()
                    .gap(px(3.))
                    .child(mark(color))
                    .child(text(self.name("tag", id), 11., MUTED)),
            );
        }
        labels
    }
    fn task_card(&self, r: &Record, column: &str, cx: &Context<Self>) -> impl IntoElement {
        let mut bottom = row()
            .w_full()
            .gap(px(5.))
            .child(icon("circle-dot", 12.).text_color(rgb(GREEN)))
            .child(text(label_value("type", r.field("type")), 11., MUTED))
            .when(
                r.fields.get("awaiting_review") == Some(&Value::Bool(true)),
                |b| b.child(text("待审阅", 11., VIOLET)),
            );
        if !r.field("due_date").is_empty() {
            bottom = bottom
                .child(icon("calendar-days", 12.).text_color(rgb(MUTED)))
                .child(text(r.field("due_date").to_owned(), 11., MUTED));
        }
        bottom = bottom
            .child(div().flex_1())
            .child(text(self.handlers_summary(r), 11., DIM));
        self.button(
            format!("task-card-{}", r.id),
            r.title.clone(),
            UiAction::Open(r.clone()),
            cx,
        )
        .when(self.writable(), |btn| {
            btn.on_drag(DraggedTask(r.clone(), column.into()), |drag, _, _, cx| {
                cx.new(|_| drag.clone())
            })
        })
        .w_full()
        .h_auto()
        .min_h(px(78.))
        .p(px(11.))
        .bg(white())
        .border_1()
        .border_color(rgb(LINE))
        .rounded(px(7.))
        .items_start()
        .justify_start()
        .child(
            col()
                .w_full()
                .min_w_0()
                .gap(px(4.))
                .child(text(r.code(), 11., DIM).truncate())
                .child(headline(r.title.clone(), 13.))
                .when(
                    r.fields
                        .get("labels")
                        .and_then(Value::as_array)
                        .is_some_and(|labels| !labels.is_empty()),
                    |area| area.child(self.task_labels(r)),
                )
                .child(bottom),
        )
    }
    fn task_list(&self, tasks: &[Record], table: bool, cx: &Context<Self>) -> AnyElement {
        let mut list = col()
            .id("task-list")
            .flex_1()
            .w_full()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(16.));
        if table {
            list = list.child(
                row()
                    .h(px(32.))
                    .gap(px(12.))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(text("编号", 11., MUTED).w(px(85.)))
                    .child(text("标题", 11., MUTED).flex_1())
                    .child(text("状态", 11., MUTED).w(px(78.)))
                    .child(text("处理人", 11., MUTED).w(px(95.)))
                    .child(text("优先级", 11., MUTED).w(px(56.))),
            );
        }
        let grouped = !table
            && self
                .loc
                .view
                .as_ref()
                .is_none_or(|v| v.field("group") == "status");
        let groups = if grouped {
            self.status_task_groups(tasks)
        } else {
            vec![(None, tasks.iter().collect())]
        };
        for (group, items) in groups {
            if let Some(status) = group {
                let key = format!(
                    "{}-{}-{}-{}",
                    self.loc.space,
                    self.loc.view.as_ref().map_or("", |r| r.id.as_str()),
                    self.loc.task_type,
                    status
                );
                let folded = self.status_folds.get(&key).copied().unwrap_or_else(|| {
                    self.find("status", &status)
                        .is_some_and(|r| r.fields.get("collapsed") == Some(&Value::Bool(true)))
                });
                let name = self.name("status", &status);
                list = list.child(
                    self.button(
                        format!("fold-status-{status}"),
                        name.clone(),
                        UiAction::FoldStatuses(key, !folded),
                        cx,
                    )
                    .w_full()
                    .justify_start()
                    .child(icon(
                        if folded {
                            "chevron-right"
                        } else {
                            "chevron-down"
                        },
                        13.,
                    ))
                    .child(name)
                    .child(text(items.len().to_string(), 11., MUTED)),
                );
                if folded {
                    continue;
                }
            }
            for r in items {
                let mut b = self
                    .button(
                        format!("task-row-{}", r.id),
                        r.title.clone(),
                        UiAction::Open(r.clone()),
                        cx,
                    )
                    .w_full()
                    .h(px(42.))
                    .px(px(4.))
                    .gap(px(12.))
                    .justify_start()
                    .rounded_none()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(text(r.code(), 11., DIM).w(px(85.)).truncate())
                    .child(text(r.title.clone(), 13., INK).flex_1().truncate())
                    .when(
                        r.fields.get("awaiting_review") == Some(&Value::Bool(true)),
                        |b| b.child(text("待审阅", 11., VIOLET)),
                    )
                    .child(text(self.name("status", r.field("status")), 12., MUTED).w(px(78.)));
                if table {
                    b = b.child(
                        text(self.handlers_summary(r), 12., MUTED)
                            .w(px(95.))
                            .truncate(),
                    );
                }
                b = b
                    .child(text(label_value("priority", r.field("priority")), 12., DIM).w(px(56.)));
                list = list.child(motion::layout(
                    format!("{}-task-row-{}", self.loc.space, r.id),
                    self.revision,
                    b,
                ));
            }
        }
        list.into_any_element()
    }
    fn dashboard(&self, tasks: &[Record], cx: &Context<Self>) -> AnyElement {
        let n = tasks.len();
        let done = self.metrics["completed"]
            .as_u64()
            .map(|v| v as usize)
            .unwrap_or_else(|| tasks.iter().filter(|r| r.field("status") == "done").count());
        let unassigned = tasks
            .iter()
            .filter(|r| r.field("assignee").is_empty())
            .count();
        let info = |label: &str, num: usize| {
            col()
                .flex_1()
                .p(px(15.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(LINE))
                .bg(white())
                .child(text(label.to_owned(), 12., MUTED))
                .child(headline(num.to_string(), 25.))
        };
        let mut chart = col()
            .p(px(16.))
            .gap(px(9.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(LINE))
            .bg(white())
            .child(headline("状态分布", 14.));
        for state in self.status_rows("") {
            let count = tasks
                .iter()
                .filter(|r| r.field("status") == state.id)
                .count();
            chart = chart.child(
                row()
                    .gap(px(8.))
                    .child(text(state.localized_title().to_owned(), 12., MUTED).w(px(70.)))
                    .child(
                        div()
                            .w(px((count as f32 / n.max(1) as f32) * 230.))
                            .h(px(8.))
                            .rounded(px(3.))
                            .bg(rgb(self.status_color(state))),
                    )
                    .child(text(count.to_string(), 12., INK)),
            );
        }
        let mut due = col().gap(px(4.)).child(headline("截止日期", 14.));
        let mut dated: Vec<_> = tasks
            .iter()
            .filter(|r| {
                !r.field("due_date").is_empty()
                    && !matches!(r.field("status"), "done" | "cancelled")
            })
            .collect();
        dated.sort_by_key(|r| r.field("due_date"));
        if dated.is_empty() {
            due = due.child(text("没有设置截止日期的未完成任务。", 13., MUTED));
        }
        let mut priority = col()
            .gap(px(8.))
            .p(px(13.))
            .border_1()
            .border_color(rgb(LINE))
            .rounded(px(8.))
            .child(headline("优先级分布", 14.));
        for (key, label) in self.priority_options() {
            let count = tasks.iter().filter(|r| r.field("priority") == *key).count();
            priority = priority.child(
                row()
                    .gap(px(8.))
                    .child(text((*label).to_owned(), 12., MUTED).w(px(70.)))
                    .child(text(count.to_string(), 12., INK)),
            );
        }
        for r in dated.iter().take(12) {
            due = due.child(
                row()
                    .gap(px(6.))
                    .child(text(r.field("due_date").to_owned(), 12., MUTED))
                    .child(div().flex_1().child(self.related_row(r, cx))),
            );
        }
        col()
            .id("dashboard-scroll")
            .overflow_y_scroll()
            .w_full()
            .flex_1()
            .min_h_0()
            .p(px(16.))
            .gap(px(12.))
            .child(
                row()
                    .w_full()
                    .gap(px(8.))
                    .child(info("全部任务", n))
                    .child(info("已完成", done))
                    .child(info("未指派", unassigned))
                    .child(info(
                        "已逾期",
                        self.metrics["overdue"].as_u64().unwrap_or_default() as usize,
                    ))
                    .child(info(
                        "7 天内到期",
                        self.metrics["due_soon"].as_u64().unwrap_or_default() as usize,
                    )),
            )
            .child(
                row()
                    .items_start()
                    .gap(px(12.))
                    .child(chart.flex_1())
                    .child(priority.flex_1()),
            )
            .child(due)
            .into_any_element()
    }
    fn list_tabs(&self, cx: &Context<Self>) -> Div {
        let mut tabs = row().min_h(px(41.)).px(px(13.)).gap(px(4.));
        let pairs: &[(&str, &str)] = match self.loc.page {
            Page::Tags | Page::TagGroups => &[
                ("", "全部"),
                ("space", "空间标签"),
                ("organization", "组织标签"),
                ("archived", "已归档"),
            ],
            Page::Inbox => &[
                ("", "收件箱"),
                ("unread", "未读"),
                ("snoozed", "稍后"),
                ("cleared", "已清理"),
            ],
            Page::Docs | Page::Agents | Page::Crews | Page::Skills => {
                &[("", "全部"), ("space", "共享"), ("private", "私有")]
            }
            Page::Runs => &[
                ("", "全部"),
                ("queued", "排队中"),
                ("running", "运行中"),
                ("failed", "失败"),
            ],
            Page::Machines => &[("", "全部机器")],
            Page::Iterations => &[
                ("", "全部迭代"),
                ("upcoming", "即将开始"),
                ("planned", "已计划"),
                ("active", "进行中"),
                ("closed", "已结束"),
            ],
            _ => &[],
        };
        for (v, l) in pairs {
            tabs = tabs.child(self.pill(
                format!("list-tab-{v}"),
                l,
                UiAction::Section(v.to_string()),
                self.loc.section == *v,
                cx,
            ));
        }
        tabs
    }
    fn visible_record(&self, r: &Record) -> bool {
        let tab = self.loc.section.as_str();
        match self.loc.page {
            Page::Tags | Page::TagGroups => {
                let archived = r.fields.get("archived") == Some(&Value::Bool(true));
                let query = self
                    .loc
                    .filter
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let scope = if r.field("scope") == "organization" {
                    "organization"
                } else {
                    "space"
                };
                r.title.to_lowercase().contains(&query.to_lowercase())
                    && match tab {
                        "archived" => archived,
                        "space" | "organization" => !archived && scope == tab,
                        _ => !archived,
                    }
            }
            Page::Inbox => match tab {
                "snoozed" => r.field("state") == "snoozed",
                "cleared" => r.field("state") == "cleared",
                "unread" => {
                    r.fields.get("read") != Some(&Value::Bool(true)) && r.field("state") == "inbox"
                }
                _ => !matches!(r.field("state"), "snoozed" | "cleared"),
            },
            Page::Docs | Page::Agents | Page::Crews | Page::Skills => {
                tab.is_empty() || r.field("visibility") == tab
            }
            Page::Runs | Page::Iterations => tab.is_empty() || r.field("status") == tab,
            _ => true,
        }
    }
    fn record_page(&self, cx: &Context<Self>) -> AnyElement {
        let p = self.loc.page;
        let info = p.info();
        let rows = self
            .list(info.kind)
            .iter()
            .filter(|r| self.visible_record(r))
            .collect::<Vec<_>>();
        let mut base = col().flex_1().min_h_0().w_full();
        if matches!(
            p,
            Page::Inbox
                | Page::Docs
                | Page::Agents
                | Page::Crews
                | Page::Skills
                | Page::Machines
                | Page::Runs
                | Page::Iterations
        ) {
            base = base.child(self.list_tabs(cx));
        }
        if rows.is_empty() {
            let act = (p.creatable() && self.writable()).then(|| {
                (
                    crate::locale::new_title(info.noun),
                    UiAction::Create(info.kind.into()),
                )
            });
            return base
                .child(self.empty_page(
                    info.icon,
                    if p == Page::Inbox {
                        "收件箱已清空"
                    } else {
                        info.label
                    },
                    info.empty,
                    act,
                    cx,
                ))
                .into_any_element();
        }
        let mut records = col()
            .id("records-scroll")
            .w_full()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .px(px(17.))
            .py(px(9.))
            .gap(px(4.));
        for r in rows {
            let action = match p {
                Page::Spaces => UiAction::SelectSpace(r.clone()),
                Page::Views => UiAction::OpenView(r.clone()),
                _ => UiAction::Open(r.clone()),
            };
            let selected = self.loc.selected.as_ref().is_some_and(|v| v.id == r.id);
            let secondary = summary(r);
            let mut b = self
                .button(format!("record-{}", r.id), r.title.clone(), action, cx)
                .w_full()
                .h_auto()
                .min_h(px(53.))
                .justify_start()
                .gap(px(12.))
                .px(px(10.))
                .py(px(6.))
                .border_b_1()
                .border_color(rgb(LINE))
                .when(selected, |b| b.bg(rgb(SELECTED)))
                .child(
                    row()
                        .size(px(31.))
                        .justify_center()
                        .rounded(px(8.))
                        .bg(rgb(PALE))
                        .child(icon(info.icon, 16.).text_color(rgb(
                            if matches!(p, Page::Agents | Page::Crews) {
                                VIOLET
                            } else {
                                MUTED
                            },
                        ))),
                )
                .child(
                    col()
                        .flex_1()
                        .min_w_0()
                        .items_start()
                        .child(headline(r.title.clone(), 13.).truncate())
                        .when(!secondary.is_empty(), |s| {
                            s.child(text(secondary, 12., MUTED).truncate())
                        }),
                );
            if !r.field("status").is_empty() {
                b = b.child(chip(label_value("status", r.field("status")), PALE, MUTED));
            }
            if p == Page::Views {
                b = b.child(self.view_tools(r, cx));
            }
            if p == Page::Views && r.fields.get("can_edit_view") == Some(&json!(true)) {
                b = b.child(self.tool_button(
                    &format!("edit-view-{}", r.id),
                    "编辑视图",
                    "square-pen",
                    UiAction::Edit(r.clone()),
                    cx,
                ));
            } else if p == Page::Spaces && self.writable() {
                b = b.child(self.tool_button(
                    &format!("edit-space-{}", r.id),
                    "空间设置",
                    "settings",
                    UiAction::Edit(r.clone()),
                    cx,
                ));
            }
            records = records.child(motion::layout(
                format!("{}-{}-{}", self.loc.space, info.kind, r.id),
                self.revision,
                b,
            ));
        }
        base.child(records).into_any_element()
    }
    fn settings_page(&self, cx: &Context<Self>) -> AnyElement {
        let mut page = col()
            .id("settings-scroll")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(23.))
            .gap(px(15.))
            .child(headline("工作区设置", 22.))
            .child(row().gap(px(8.)).child(mark(GREEN)).child(text(
                format!(
                    "{} · {} · {}",
                    self.space_name(),
                    self.actor,
                    if self.mode == "team" {
                        "团队连接"
                    } else if self.mode == "demo" {
                        "内存演示"
                    } else {
                        "本地数据库"
                    }
                ),
                13.,
                MUTED,
            )));
        page = page.child(
            outlined(self.button(
                "notification-settings",
                "通知设置",
                UiAction::Inbox(crate::app::inbox_ui::Action::Settings(None)),
                cx,
            ))
            .child(crate::locale::label("通知设置")),
        );
        if self.mode != "demo" {
            page = page.child(
                row()
                    .gap(px(8.))
                    .child(
                        primary(self.button("team-connect", "连接团队服务", UiAction::Connect, cx))
                            .child(icon("cloud", 14.))
                            .child(crate::locale::label("连接团队服务")),
                    )
                    .child(
                        outlined(self.button(
                            "settings-accounts",
                            "账号列表",
                            UiAction::Accounts,
                            cx,
                        ))
                        .child(crate::locale::label("账号列表")),
                    )
                    .when(self.mode == "team", |row| {
                        row.child(
                            outlined(self.button(
                                "team-disconnect",
                                "切换到本机",
                                UiAction::Disconnect,
                                cx,
                            ))
                            .child(crate::locale::label("切换到本机")),
                        )
                    })
                    .when(self.mode == "team", |row| {
                        row.child(
                            outlined(self.button(
                                "personal-tokens",
                                "个人访问令牌",
                                UiAction::PersonalTokens,
                                cx,
                            ))
                            .child(crate::locale::label("个人访问令牌")),
                        )
                    })
                    .when(
                        self.mode == "team"
                            && self.access_identity["actor"] == "owner"
                            && self.access_identity["scope"] == "write"
                            && self.access_identity["personal"] == false,
                        |row| {
                            row.child(
                                outlined(self.button(
                                    "issue-token",
                                    "签发令牌",
                                    UiAction::Grant,
                                    cx,
                                ))
                                .child(crate::locale::label("签发令牌")),
                            )
                            .child(
                                outlined(self.button(
                                    "list-tokens",
                                    "管理令牌",
                                    UiAction::Tokens,
                                    cx,
                                ))
                                .child(crate::locale::label("管理令牌")),
                            )
                        },
                    ),
            );
        }
        if self.mode == "team" {
            let mut toolbar = row().gap(px(8.));
            for (op, l) in [("sync.status", "同步状态"), ("sync.flush", "立即同步")] {
                toolbar = toolbar.child(
                    outlined(self.button(op, l, UiAction::Inspect(json!({"op":op}), l.into()), cx))
                        .child(l),
                );
            }
            toolbar = toolbar.child(
                outlined(self.button("pending-drafts", "待同步更改", UiAction::PendingDrafts, cx))
                    .child(crate::locale::label("待同步更改")),
            );
            page = page.child(toolbar);
        }
        if self.loc.section == "drafts" {
            if self.drafts.is_empty() {
                page = page.child(text("当前没有待同步的更改。", 13., MUTED));
            }
            for (index, draft) in self.drafts.iter().enumerate() {
                let req = &draft["request"];
                let title = req["title"]
                    .as_str()
                    .or_else(|| req["id"].as_str())
                    .unwrap_or("更改");
                let mut item = col()
                    .gap(px(4.))
                    .p(px(9.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .rounded(px(6.))
                    .child(headline(title.to_string(), 13.))
                    .child(text(format!("{} · {}", req["op"], req["kind"]), 12., MUTED));
                if let Some(error) = draft["error"].as_str() {
                    item = item.child(text(error.to_string(), 12., ERROR));
                }
                let mut buttons = row().gap(px(7.));
                if !draft["error"].is_null() {
                    buttons = buttons.child(
                        outlined(self.button(
                            format!("rebase-draft-{index}"),
                            "处理冲突",
                            UiAction::Form(Mode::ResolveDraft(draft.clone())),
                            cx,
                        ))
                        .child(crate::locale::label("处理冲突")),
                    );
                }
                buttons=buttons.child(self.button(format!("remove-draft-{index}"),"舍弃草稿",UiAction::Form(Mode::Confirm{label:format!("舍弃草稿「{title}」"),command:json!({"op":"sync.resolve","id":draft["request_id"],"discard":true})}),cx).child(crate::locale::label("舍弃")));
                page = page.child(item.child(buttons));
            }
        }
        if self.loc.section == "accounts" && self.mode != "demo" {
            page = page.child(self.account_panel(cx));
        }
        if self.loc.section == "personal_tokens" && self.mode == "team" {
            page = page.child(self.personal_tokens_panel(cx));
        }
        if self.loc.section == "tokens" {
            for t in &self.tokens {
                let id = t.get("id").and_then(Value::as_str).unwrap_or("");
                page=page.child(row().min_h(px(33.)).gap(px(10.)).child(text(format!("{} · {}",t.get("actor").map(value_text).unwrap_or_default(),t.get("scope").map(value_text).unwrap_or_default()),12.,INK)).child(text(id.to_owned(),11.,DIM)).child(self.button(format!("revoke-{id}"),"撤销访问",UiAction::Form(Mode::Confirm{label:"撤销该访问令牌".into(),command:json!({"op":"access.revoke","id":id,"space":self.loc.space})}),cx).child(crate::locale::label("撤销"))));
            }
        }
        for (title, pages) in [
            (
                "组织",
                &[
                    Page::Organizations,
                    Page::Spaces,
                    Page::Members,
                    Page::Machines,
                    Page::Connectors,
                    Page::Transfer,
                ][..],
            ),
            (
                "空间设置",
                &[
                    Page::Types,
                    Page::Statuses,
                    Page::Fields,
                    Page::RelationTypes,
                    Page::Relations,
                    Page::Tools,
                ][..],
            ),
        ] {
            page = page.child(headline(title, 15.));
            for &p in pages {
                let i = p.info();
                page = page.child(
                    self.button(format!("setting-{:?}", p), i.label, UiAction::Go(p), cx)
                        .w_full()
                        .h(px(39.))
                        .justify_start()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(icon(i.icon, 15.))
                        .child(text(i.label, 13., INK))
                        .child(div().flex_1())
                        .child(icon("chevron-right", 13.)),
                );
            }
        }
        page.into_any_element()
    }
    fn transfer_page(&self, cx: &Context<Self>) -> Div {
        col()
            .w_full()
            .p(px(24.))
            .gap(px(12.))
            .items_start()
            .child(headline("空间数据", 20.))
            .child(text(
                "导出当前空间为 JSON；导入前可预检创建和跳过的记录。",
                13.,
                MUTED,
            ))
            .child(
                row()
                    .gap(px(9.))
                    .child(
                        outlined(self.button("export-file", "导出 JSON", UiAction::Export, cx))
                            .child(icon("download", 14.))
                            .child(crate::locale::label("导出 JSON")),
                    )
                    .when(self.writable(), |r| {
                        r.child(
                            primary(self.button("import-file", "导入 JSON", UiAction::Import, cx))
                                .child(icon("upload", 14.))
                                .child(crate::locale::label("导入 JSON")),
                        )
                    }),
            )
    }
    pub fn detail(&self, w: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(r) = self.loc.selected.as_ref() else {
            return div().into_any_element();
        };
        let available = f32::from(w.viewport_size().width)
            - 60.
            - if self.sidebar {
                if f32::from(w.viewport_size().width) >= 1100. {
                    272.
                } else {
                    196.
                }
            } else {
                0.
            };
        let width = (if r.kind == "task" { 920_f32 } else { 550_f32 }).min(available.max(250.));
        let side_properties = r.kind == "task" && width >= 680.;
        let info = Page::from_kind(&r.kind).map(|p| p.info());
        let mut body = col()
            .w_full()
            .gap(px(14.))
            .p(px(19.))
            .child(text(
                if r.kind == "task" {
                    r.code()
                } else {
                    info.as_ref().map(|i| i.noun).unwrap_or("详情").into()
                },
                12.,
                MUTED,
            ))
            .child(headline(r.title.clone(), 22.));
        if matches!(r.kind.as_str(), "task" | "chat") {
            let key = if r.kind == "task" {
                "task_id"
            } else {
                "chat_id"
            };
            for run in self.list("run").iter().filter(|v| {
                v.field(key) == r.id
                    && v.field("status") == "queued"
                    && matches!(
                        v.field("waiting_reason"),
                        "outside_working_hours" | "scheduled_start"
                    )
            }) {
                body = body.child(self.scheduled_banner(run, cx));
            }
        }
        if r.kind == "member" && self.writable() && self.role != "guest" && self.actor != r.id {
            body = body.child(
                outlined(self.button(
                    "direct-member",
                    "与真人对话",
                    UiAction::Direct(r.id.clone()),
                    cx,
                ))
                .child(crate::locale::label("与真人对话")),
            );
        }
        let long = match r.kind.as_str() {
            "document" | "message" | "comment" => r.field("body"),
            "skill" | "agent" => r.field("instructions"),
            "automation" => r.field("instruction"),
            _ => r.field("description"),
        };
        if matches!(r.kind.as_str(), "document" | "task" | "chat") {
            body = body.child(self.conversation_controls(
                r,
                f32::from(w.viewport_size().width) >= 900.,
                cx,
            ));
        }
        if matches!(r.kind.as_str(), "document" | "task" | "comment") {
            body = body.child(self.document_embeds_panel(r, cx));
        }
        if !long.is_empty() {
            body = body.child(text(long.to_owned(), 14., INK));
        }
        let can_edit = self.writable()
            && (!matches!(r.kind.as_str(), "agent" | "crew")
                || self.list("agent_settings").iter().any(|v| {
                    v.id == r.id
                        && v.fields["record"]["fields"]["can_manage"] == true
                        && !v.fields["record"]["fields"]["parent_id"].is_string()
                }))
            && !matches!(
                r.kind.as_str(),
                "run" | "notification" | "machine" | "artifact"
            );
        let mut actions = row().flex_wrap().gap(px(5.));
        if r.kind == "space" && r.id == self.loc.space {
            actions = actions.child(
                outlined(self.button(
                    "space-iteration-history",
                    "迭代结项历史",
                    UiAction::Inspect(json!({"op":"iteration.history"}), "迭代结项历史".into()),
                    cx,
                ))
                .child("迭代历史"),
            );
        }
        if r.kind == "task_type"
            && self.writable()
            && matches!(self.role.as_str(), "admin" | "owner")
        {
            actions = actions.child(
                outlined(self.button(
                    "configure-type-workflow",
                    "配置类型工作流",
                    UiAction::Form(Mode::Workflow {
                        op: "task_type.workflow.save",
                        record: r.clone(),
                    }),
                    cx,
                ))
                .child(crate::locale::label("配置类型工作流")),
            );
        }
        if can_edit {
            actions = actions.child(
                outlined(self.button("edit-object", "编辑", UiAction::Edit(r.clone()), cx))
                    .child(icon("square-pen", 13.))
                    .child(crate::locale::label("编辑")),
            );
        }
        match r.kind.as_str() {
            "task" => {
                actions = actions
                    .child(
                        outlined(self.button(
                            "share-task-link",
                            "复制任务链接",
                            UiAction::Review(crate::app::review_ui::Action::Share(r.id.clone())),
                            cx,
                        ))
                        .child("复制任务链接"),
                    )
                    .child(
                        outlined(self.button(
                            "open-task-link",
                            "打开任务链接",
                            UiAction::Form(Mode::Review(
                                crate::app::review_ui::ReviewForm::OpenTask,
                            )),
                            cx,
                        ))
                        .child("打开任务链接"),
                    );
                let following = r.fields.get("following") == Some(&Value::Bool(true));
                let (label, glyph) = if following {
                    ("取消关注", "bell-off")
                } else {
                    ("关注", "bell")
                };
                actions = actions.child(
                    outlined(self.button(
                        "task-followers",
                        "更改关注者",
                        UiAction::Inbox(crate::app::inbox_ui::Action::Followers(r.clone())),
                        cx,
                    ))
                    .child(crate::locale::label("更改关注者")),
                );
                actions = actions.when(!self.current_archived(), |a| {
                    a.child(
                        outlined(self.button(
                            "task-follow",
                            label,
                            UiAction::Apply(
                                json!({"op":"follow","id":r.id,"enabled":!following}),
                                "关注设置已更新".into(),
                            ),
                            cx,
                        ))
                        .child(icon(glyph, 13.))
                        .child(crate::locale::label(label).to_owned()),
                    )
                });
                if self.writable() {
                    actions = actions.child(
                        outlined(self.button(
                            "add-comment",
                            "添加评论",
                            UiAction::Form(Mode::Comment(r.clone())),
                            cx,
                        ))
                        .child(icon("messages-square", 13.))
                        .child(crate::locale::label("评论")),
                    );
                }
                if self.writable() {
                    actions = actions.child(
                        outlined(self.button(
                            "task-mention-agent",
                            "在评论中提及 Agent",
                            UiAction::Form(Mode::Directory(
                                crate::app::agent_directory::DirectoryForm::Mention(r.clone()),
                            )),
                            cx,
                        ))
                        .child(crate::locale::label("在评论中提及 Agent")),
                    );
                }
                body = body.child(actions);
                if self.writable()
                    && let Some(threads) = r.fields.get("im_threads").and_then(Value::as_array)
                {
                    let mut replies = row().flex_wrap().gap(px(6.));
                    for (index, target) in threads.iter().enumerate() {
                        let label = format!(
                            "{} · {} / {}",
                            crate::locale::label("回复到 Slack 话题"),
                            target["thread"]["channel"].as_str().unwrap_or(""),
                            target["thread"]["thread"].as_str().unwrap_or("")
                        );
                        let request_id = format!(
                            "thread-reply-{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos()
                        );
                        replies = replies.child(
                            outlined(self.button(
                                format!("thread-reply-{}-{index}", r.id),
                                label.clone(),
                                UiAction::Form(Mode::ThreadReply {
                                    task: r.id.clone(),
                                    target: target.clone(),
                                    request_id,
                                }),
                                cx,
                            ))
                            .child(icon("messages-square", 13.))
                            .child(label),
                        );
                    }
                    body = body.child(replies);
                }
                if self.writable()
                    && matches!(r.field("assignee_kind"), "agent" | "crew")
                    && !r.field("assignee").is_empty()
                {
                    body = body.child(
                        row()
                            .flex_wrap()
                            .gap(px(5.))
                            .child(
                                outlined(self.button(
                                    "task-conversation",
                                    "打开任务会话",
                                    UiAction::Personal(crate::personal::Action::Chat(r.id.clone())),
                                    cx,
                                ))
                                .child(crate::locale::label("任务会话")),
                            )
                            .child(
                                outlined(self.button(
                                    "task-quick-command",
                                    "立即执行任务指令",
                                    UiAction::Personal(crate::personal::Action::Command(
                                        r.id.clone(),
                                    )),
                                    cx,
                                ))
                                .child(crate::locale::label("立即执行")),
                            ),
                    );
                }
                if !side_properties {
                    body = body.child(self.task_properties(r, cx));
                }
                body = body
                    .child(headline("评论与活动", 14.))
                    .child(self.discussion_panel(r, cx));
                let children: Vec<_> = self
                    .list("task")
                    .iter()
                    .filter(|t| t.field("parent") == r.id)
                    .collect();
                if !children.is_empty() {
                    body = body.child(headline("子任务", 14.));
                    for t in children {
                        body = body.child(self.related_row(t, cx));
                    }
                }
                let relations: Vec<_> = self
                    .list("relation")
                    .iter()
                    .filter(|rel| rel.field("from") == r.id || rel.field("to") == r.id)
                    .collect();
                if !relations.is_empty() {
                    body = body.child(headline("任务关联", 14.));
                    for rel in relations {
                        let other = if rel.field("from") == r.id {
                            rel.field("to")
                        } else {
                            rel.field("from")
                        };
                        if let Some(t) = self.find("task", other) {
                            body = body.child(self.related_row(t, cx));
                        }
                    }
                }
                for room in self
                    .list("chat")
                    .iter()
                    .filter(|room| room.field("task") == r.id || room.field("task_id") == r.id)
                {
                    body = body.child(self.related_row(room, cx));
                }
                body = self.related_runs(body, r, cx);
            }
            "chat" => {
                if let Some(id) = r
                    .fields
                    .get("task")
                    .or(r.fields.get("task_id"))
                    .and_then(Value::as_str)
                    && let Some(task) = self.find("task", id)
                {
                    body = body.child(self.related_row(task, cx));
                }
                if let Some(info) = r.fields.get("conversation") {
                    body = body.child(self.chat_controls(r, info, cx));
                }
                if self.writable() {
                    actions = actions.child(
                        primary(self.button(
                            "send-message",
                            "发送消息",
                            UiAction::Form(Mode::Message(r.clone())),
                            cx,
                        ))
                        .child(icon("send", 14.))
                        .child(crate::locale::label("发送消息")),
                    );
                }
                body = body
                    .child(actions)
                    .child(headline("消息", 14.))
                    .child(self.thread_items("message", "chat", &r.id, cx));
                if crate::app::chat_actions::agent_room(r)
                    || r.fields.get("task").is_some_and(Value::is_string)
                {
                    body = self.related_runs(body, r, cx);
                }
            }
            "document" => {
                actions = actions.child(
                    outlined(self.button(
                        "doc-history",
                        "历史版本",
                        UiAction::Versions(r.clone()),
                        cx,
                    ))
                    .child(icon("clock-arrow-left", 13.))
                    .child(crate::locale::label("历史版本")),
                );
                body = body.child(actions);
                if self.drawer_tab == "versions" {
                    body = body.child(headline("历史版本", 15.));
                    for (i, v) in self.versions.iter().enumerate() {
                        let ver = v
                            .get("version")
                            .or_else(|| v.get("rev"))
                            .and_then(Value::as_i64)
                            .unwrap_or_default();
                        body = body.child(
                            row()
                                .gap(px(8.))
                                .child(text(format!("版本 {ver}"), 12., INK))
                                .child(text(
                                    v.get("title").map(value_text).unwrap_or_default(),
                                    12.,
                                    MUTED,
                                ))
                                .child(div().flex_1())
                                .when(self.writable(), |b| {
                                    b.child(
                                        self.button(
                                            format!("restore-{i}"),
                                            "恢复此版本",
                                            UiAction::RestoreVersion(r.clone(), ver),
                                            cx,
                                        )
                                        .child(crate::locale::label("恢复")),
                                    )
                                }),
                        );
                    }
                }
            }
            "customer_request" => {
                if matches!(self.role.as_str(), "owner" | "admin" | "member" | "editor")
                    && !self.current_archived()
                {
                    actions=actions.child(outlined(self.button("convert-request","转为任务",UiAction::Form(Mode::Confirm{label:"将此客户请求转换为任务".into(),command:json!({"op":"customer_request.convert","space":r.space,"id":r.id,"expected_rev":r.rev})}),cx)).child("转为任务"));
                }
                body = body.child(actions).child(self.attributes(r));
            }
            "iteration" | "project" => {
                if r.kind == "iteration"
                    && !matches!(r.field("status"), "closed" | "cancelled" | "active")
                    && self.writable()
                    && (r.fields.get("cadence") != Some(&Value::Bool(true))
                        || r.field("status") == "upcoming"
                            && matches!(self.role.as_str(), "owner" | "admin"))
                {
                    actions=actions.child(outlined(self.button("start-iteration","今天开始",UiAction::Form(Mode::Confirm{label:"今天开始此迭代".into(),command:json!({"op":"iteration.start","space":self.loc.space,"id":r.id,"expected_rev":r.rev})}),cx)).child("今天开始"));
                }
                if r.kind == "iteration" && r.field("status") == "closed" {
                    actions = actions.child(
                        outlined(self.button(
                            "iteration-scope-history",
                            "结项时的范围",
                            UiAction::Inspect(
                                json!({"op":"iteration.history","id":r.id}),
                                "结项时的范围".into(),
                            ),
                            cx,
                        ))
                        .child("查看结项快照"),
                    );
                }
                if r.kind == "iteration"
                    && !matches!(r.field("status"), "closed" | "cancelled")
                    && r.fields.get("cadence") != Some(&Value::Bool(true))
                    && self.writable()
                {
                    actions = actions.child(
                        outlined(self.button(
                            "finish-iteration",
                            "结束迭代",
                            UiAction::IterationEnd(r.clone()),
                            cx,
                        ))
                        .child(crate::locale::label("结束迭代")),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
                if r.kind == "project" {
                    body = body.child(self.project_chart(r, cx));
                }
                body = body.child(headline("关联任务", 14.));
                for t in self
                    .list("task")
                    .iter()
                    .filter(|t| t.field(&r.kind) == r.id)
                {
                    body = body.child(self.related_row(t, cx));
                }
            }
            "agent" | "crew" => {
                actions = actions.child(
                    outlined(self.button(
                        "agent-runs",
                        "运行历史",
                        UiAction::ViewAgentHistory(r.clone()),
                        cx,
                    ))
                    .child(icon("clock-arrow-left", 13.))
                    .child(crate::locale::label("运行历史")),
                );
                body = body.child(actions);
                body = body.child(self.handler_panel(r, cx));
            }
            "automation" => {
                if self.writable() {
                    actions = actions.child(
                        outlined(self.button(
                            "run-rule",
                            "运行一次",
                            UiAction::RuleFire(r.clone()),
                            cx,
                        ))
                        .child(icon("play", 13.))
                        .child(crate::locale::label("运行一次")),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
                body = self.related_runs(body, r, cx);
            }
            "machine" => {
                actions = actions.child(
                    outlined(self.button(
                        "machine-runtime-management",
                        "运行时能力",
                        UiAction::MachineManagement(
                            crate::app::machine_management::Action::Inventory(r.id.clone()),
                        ),
                        cx,
                    ))
                    .child(crate::locale::label("运行时能力")),
                );
                for (op, label) in [
                    ("machine.directories", "获准工作目录"),
                    ("machine.diagnostics", "机器诊断"),
                ] {
                    actions = actions.child(
                        outlined(self.button(
                            op,
                            label,
                            UiAction::Inspect(json!({"op":op,"id":r.id}), label.into()),
                            cx,
                        ))
                        .child(crate::locale::label(label)),
                    );
                }
                if self.writable() {
                    let disabled = r.fields.get("disabled") == Some(&Value::Bool(true));
                    let (op, l) = if disabled {
                        ("machine.enable", "启用接单")
                    } else {
                        ("machine.disable", "暂停接单")
                    };
                    actions = actions.child(
                        outlined(self.button(
                            "machine-enabled",
                            l,
                            UiAction::Form(Mode::Confirm {
                                label: l.into(),
                                command: json!({"op":op,"space":self.loc.space,"id":r.id}),
                            }),
                            cx,
                        ))
                        .child(l),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
            }
            "connector" | "tool" => {
                let ops: &[&str] = if r.kind == "tool" {
                    &["tool.authorize"]
                } else if r.field("provider") == "zoho" {
                    &[
                        "zoho.credential",
                        "zoho.projects",
                        "zoho.preview",
                        "imports.commit",
                        "imports.history",
                        "imports.rollback",
                    ]
                } else if r.field("provider") == "asana" {
                    &[
                        "asana.accounts",
                        "asana.credential",
                        "asana.projects",
                        "asana.preview",
                        "imports.commit",
                        "asana.synchronize",
                        "imports.sync_control",
                        "imports.history",
                        "imports.rollback",
                        "asana.push",
                        "imports.push_history",
                        "provider.preview",
                        "provider.ingest",
                    ]
                } else if r.field("provider") == "teambition" {
                    &[
                        "teambition.application",
                        "teambition.authorize",
                        "teambition.personal",
                        "teambition.projects",
                        "teambition.preview",
                        "imports.commit",
                        "teambition.synchronize",
                        "imports.sync_control",
                        "imports.history",
                        "imports.rollback",
                        "teambition.push",
                        "imports.push_history",
                    ]
                } else if r.field("provider") == "cnb" {
                    &[
                        "cnb.credential",
                        "cnb.repositories",
                        "cnb.pair",
                        "cnb.ci",
                        "cnb.activity",
                        "cnb.deliver",
                    ]
                } else if matches!(r.field("provider"), "linear" | "clickup" | "ones") {
                    match (r.field("provider"), r.field("cloud_auth")) {
                        ("linear", "oauth") => &[
                            "linear.authorize",
                            "linear.accounts",
                            "linear.credential",
                            "linear.projects",
                            "linear.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.sync_control",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                            "provider.preview",
                            "provider.ingest",
                            "connector.ingest",
                            "connector.export",
                        ],
                        ("linear", _) => &[
                            "linear.accounts",
                            "linear.credential",
                            "linear.projects",
                            "linear.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.sync_control",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                            "provider.preview",
                            "provider.ingest",
                            "connector.ingest",
                            "connector.export",
                        ],
                        ("clickup", "oauth") => &[
                            "clickup.authorize",
                            "clickup.accounts",
                            "clickup.credential",
                            "clickup.projects",
                            "clickup.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.sync_control",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                        ],
                        ("clickup", _) => &[
                            "clickup.accounts",
                            "clickup.credential",
                            "clickup.projects",
                            "clickup.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.sync_control",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                        ],
                        _ => &[
                            "ones.accounts",
                            "ones.credential",
                            "ones.projects",
                            "ones.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.sync_control",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                        ],
                    }
                } else if matches!(r.field("provider"), "zentao" | "pingcode") {
                    if r.field("provider") == "zentao" {
                        &[
                            "zentao.credential",
                            "zentao.projects",
                            "zentao.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                        ]
                    } else {
                        &[
                            "pingcode.credential",
                            "pingcode.projects",
                            "pingcode.preview",
                            "imports.commit",
                            "imports.synchronize",
                            "imports.history",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                        ]
                    }
                } else if r.field("provider") == "tapd" {
                    &[
                        "tapd.credential",
                        "tapd.projects",
                        "tapd.preview",
                        "imports.commit",
                        "imports.history",
                        "imports.rollback",
                    ]
                } else if r.field("provider") == "bugly" {
                    &[
                        "bugly.rotate",
                        "bugly.revoke",
                        "bugly.credentials",
                        "bugly.outbox",
                        "bugly.deliver",
                    ]
                } else {
                    &[
                        "provider.preview",
                        "provider.ingest",
                        "connector.ingest",
                        "connector.export",
                    ]
                };
                for op in ops {
                    let l = crate::forms::workflow_name(op);
                    actions = actions.child(
                        outlined(self.button(
                            format!("workflow-{op}"),
                            l,
                            UiAction::Form(Mode::Workflow {
                                op,
                                record: r.clone(),
                            }),
                            cx,
                        ))
                        .child(l),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
            }
            "artifact" => {
                if self.writable() {
                    actions = actions.child(
                        outlined(self.button(
                            "save-artifact",
                            "存为个人文档",
                            UiAction::Apply(
                                json!({"op":"artifact.save","id":r.id,"visibility":"private"}),
                                "产物已保存到文档库".into(),
                            ),
                            cx,
                        ))
                        .child(crate::locale::label("存为个人文档")),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
            }
            "run" => {
                actions = actions.child(
                    outlined(self.button(
                        "run-diagnostics",
                        "运行诊断",
                        UiAction::Inspect(
                            json!({"op":"run.diagnostics","id":r.id}),
                            "运行诊断".into(),
                        ),
                        cx,
                    ))
                    .child(crate::locale::label("运行诊断")),
                );
                if r.field("status") == "queued"
                    && matches!(
                        r.field("waiting_reason"),
                        "outside_working_hours" | "scheduled_start"
                    )
                {
                    body = body.child(self.scheduled_banner(r, cx));
                }
                if self.writable() {
                    let status = r.field("status");
                    if status == "waiting" {
                        actions = actions.child(
                            outlined(self.button(
                                "reply-to-run",
                                "回复",
                                UiAction::Form(Mode::Workflow {
                                    op: "run.reply",
                                    record: r.clone(),
                                }),
                                cx,
                            ))
                            .child(crate::locale::label("回复")),
                        );
                    }
                    let (op, label) = if matches!(
                        status,
                        "queued" | "running" | "waiting" | "waiting_children" | "cancelling"
                    ) {
                        ("run.cancel", "取消运行")
                    } else {
                        ("run.retry", "重新尝试")
                    };
                    actions = actions.child(
                        outlined(self.button(
                            "change-run",
                            label,
                            UiAction::Form(Mode::Confirm {
                                label: label.into(),
                                command: json!({"op":op,"space":self.loc.space,"id":r.id}),
                            }),
                            cx,
                        ))
                        .child(crate::locale::label(label).to_owned()),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
            }
            "notification" => {
                for (op, l) in [
                    ("read", "标记已读"),
                    ("unread", "标记未读"),
                    ("snooze", "一小时后提醒"),
                    ("clear", "清理"),
                    ("restore", "移回收件箱"),
                ] {
                    actions=actions.child(outlined(self.button(format!("notification-{op}"),l,UiAction::Apply(json!({"op":"notification.act","id":r.id,"action":op,"expected_rev":r.rev,"until":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64+3_600_000}),"提醒已更新".into()),cx)).child(l));
                }
                body = body.child(actions).child(self.attributes(r));
                let id = r.field("task");
                if let Some(t) = self.find("task", id) {
                    body = body.child(self.related_row(t, cx));
                }
            }
            _ => body = body.child(actions).child(self.attributes(r)),
        }
        if matches!(
            r.kind.as_str(),
            "document" | "task" | "artifact" | "message"
        ) {
            let mut files = col().gap(px(6.)).child(headline("附件", 14.));
            if self.writable() {
                files = files.child(
                    outlined(self.button(
                        "attach-file",
                        "添加附件",
                        UiAction::Form(Mode::Upload(Some(r.clone()))),
                        cx,
                    ))
                    .child(icon("upload", 13.))
                    .child(crate::locale::label("添加附件")),
                );
            }
            for (i, f) in r
                .fields
                .get("attachments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let name = f["filename"].as_str().unwrap_or("附件").to_string();
                files = files.child(
                    self.button(
                        format!("download-file-{i}"),
                        name.clone(),
                        UiAction::Form(Mode::Download(r.clone(), f.clone())),
                        cx,
                    )
                    .justify_start()
                    .child(icon("download", 13.))
                    .child(text(name, 12., INK))
                    .child(text(format!("{} B", f["bytes"]), 11., MUTED)),
                );
            }
            body = body.child(files);
        }
        let mut toolbar = row()
            .w_full()
            .h(px(43.))
            .px(px(10.))
            .gap(px(7.))
            .border_b_1()
            .border_color(rgb(LINE))
            .child(self.tool_button("close-detail", "关闭详情", "x", UiAction::CloseDetail, cx))
            .child(text(
                info.as_ref().map(|i| i.label).unwrap_or("详情"),
                12.,
                MUTED,
            ))
            .child(div().flex_1())
            .child(self.tool_button(
                "copy-object-id",
                "复制对象标识",
                "copy",
                UiAction::Copy(r.id.clone()),
                cx,
            ));
        if can_edit {
            toolbar = toolbar.child(
                self.button("delete-object", "删除记录", UiAction::Delete(r.clone()), cx)
                    .child(text("删除", 12., MUTED)),
            );
        }
        let detail_content = if side_properties {
            row()
                .w_full()
                .items_start()
                .child(body.flex_1().min_w_0())
                .child(
                    col()
                        .w(px((width * 0.32).clamp(230., 300.)))
                        .flex_shrink_0()
                        .pr(px(16.))
                        .pt(px(18.))
                        .child(self.task_properties(r, cx)),
                )
                .into_any_element()
        } else {
            body.into_any_element()
        };
        col()
            .id("object-detail")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(width))
            .min_w_0()
            .occlude()
            .bg(white())
            .border_l_1()
            .border_color(rgb(LINE))
            .shadow_md()
            .child(toolbar)
            .child(
                div()
                    .id(format!("detail-scroll-{}", r.id))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(detail_content),
            )
            .into_any_element()
    }
    fn task_properties(&self, r: &Record, cx: &Context<Self>) -> Div {
        let mut props = col()
            .w_full()
            .p(px(12.))
            .rounded(px(8.))
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(LINE))
            .gap(px(8.))
            .child(headline("属性", 13.));
        let entries = [
            ("当前状态", self.name("status", r.field("status"))),
            ("类型", label_value("type", r.field("type"))),
            ("人员处理人", self.member_names(r)),
            ("执行者", self.executor_name(r)),
            ("负责人", self.name("member", r.field("owner"))),
            ("优先级", label_value("priority", r.field("priority"))),
            ("迭代", self.name("iteration", r.field("iteration"))),
            ("专项", self.name("project", r.field("project"))),
            ("截止日期", r.field("due_date").into()),
            (
                "标签",
                r.fields
                    .get("labels")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|id| self.name("tag", id))
                    .collect::<Vec<_>>()
                    .join("、"),
            ),
        ];
        if self
            .find("space", &self.loc.space)
            .is_some_and(|space| space.fields.get("estimates_enabled") == Some(&Value::Bool(true)))
        {
            props = props.child(
                row()
                    .gap(px(6.))
                    .child(text("预估工作量", 12., MUTED).w(px(82.)))
                    .child(text(
                        r.fields
                            .get("estimate")
                            .and_then(Value::as_f64)
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "—".into()),
                        12.,
                        INK,
                    )),
            );
        }
        for (k, v) in entries {
            props = props.child(
                row()
                    .gap(px(6.))
                    .child(text(k, 12., MUTED).w(px(82.)))
                    .child(text(if v.is_empty() { "—".into() } else { v }, 12., INK)),
            );
        }
        if let Some(bindings) = self
            .find("task_type", r.field("type"))
            .and_then(|ty| ty.fields.get("field_bindings"))
            .and_then(Value::as_array)
        {
            for b in bindings.iter().filter(|b| b["enabled"] != false) {
                if let Some(id) = b["field"].as_str() {
                    let value = r
                        .fields
                        .get("custom")
                        .and_then(|v| v.get(id))
                        .map(value_text)
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| "—".into());
                    props = props.child(
                        row()
                            .gap(px(6.))
                            .child(text(self.name("field", id), 12., MUTED).w(px(82.)))
                            .child(text(value, 12., INK)),
                    );
                }
            }
        }
        if self.writable() {
            let mut actions = row().flex_wrap().pt(px(8.)).gap(px(4.));
            if r.field("status") == "triage"
                || self
                    .find("status", r.field("status"))
                    .is_some_and(|s| s.field("category") == "triage")
            {
                actions = actions.child(self.review_actions(r, cx));
            } else {
                for state in self.status_rows(r.field("type")) {
                    let status = &state.id;
                    if r.field("status") != status && state.field("category") != "triage" {
                        let label = state.localized_title().to_owned();
                        actions=actions.child(outlined(self.button(format!("set-{status}"),label.clone(),UiAction::Apply(json!({"op":"update","kind":"task","id":r.id,"expected_rev":r.rev,"fields":{"status":status}}),"状态已更新".into()),cx)).child(label));
                    }
                }
            }
            props = props.child(actions);
        }
        props
    }
    fn related_row(&self, r: &Record, cx: &Context<Self>) -> impl IntoElement {
        self.button(
            format!("related-{}-{}", r.kind, r.id),
            r.title.clone(),
            UiAction::OpenSearch(r.clone()),
            cx,
        )
        .justify_start()
        .w_full()
        .child(icon("arrow-right", 13.))
        .child(text(r.title.clone(), 13., INK))
        .child(div().flex_1())
        .child(text(
            if r.kind == "task" {
                self.name("status", r.field("status"))
            } else {
                label_value("status", r.field("status"))
            },
            11.,
            MUTED,
        ))
    }
    fn thread_items(&self, kind: &str, key: &str, id: &str, cx: &Context<Self>) -> Div {
        let mut col = col().gap(px(9.));
        let mut n = 0;
        for r in self.list(kind).iter().filter(|r| r.field(key) == id) {
            let origin = r.fields.get("vcs").filter(|_| r.field("source") == "vcs");
            let mut item = crate::controls::col()
                .p(px(10.))
                .border_1()
                .border_color(rgb(LINE))
                .rounded(px(7.))
                .child(text(
                    if let Some(vcs) = origin {
                        format!(
                            "{} · {}",
                            vcs["provider"].as_str().unwrap_or("Git"),
                            vcs["repository"].as_str().unwrap_or("")
                        )
                    } else if !r.field("external_provider").is_empty() {
                        format!(
                            "{} · {}",
                            r.field("external_provider"),
                            r.field("external_author")
                        )
                    } else if kind == "message" && !r.field("author").is_empty() {
                        self.name(
                            if r.field("author_kind") == "agent" {
                                "agent"
                            } else {
                                "member"
                            },
                            r.field("author"),
                        )
                    } else {
                        self.name("member", r.field("creator"))
                    },
                    11.,
                    MUTED,
                ))
                .child(text(
                    if r.field("body").is_empty() {
                        r.title.clone()
                    } else {
                        r.field("body").into()
                    },
                    13.,
                    INK,
                ));
            if let Some(url) = origin
                .and_then(|v| v["url"].as_str())
                .filter(|u| u.starts_with("https://"))
            {
                item = item.child(
                    outlined(self.button(
                        format!("vcs-url-{}", r.id),
                        "查看源记录",
                        UiAction::OpenSourceUrl(url.to_owned()),
                        cx,
                    ))
                    .child(icon("arrow-right", 13.))
                    .child(crate::locale::label("查看源记录")),
                );
            }
            if kind == "message"
                && self.writable()
                && self
                    .loc
                    .selected
                    .as_ref()
                    .is_some_and(crate::app::chat_actions::agent_room)
            {
                item = item.child(
                    outlined(self.button(
                        format!("fork-message-{}", r.id),
                        "从此消息分叉",
                        UiAction::Chat(crate::app::chat_actions::Action::Fork {
                            room: id.to_owned(),
                            message: r.id.clone(),
                        }),
                        cx,
                    ))
                    .child(crate::locale::label("从此消息分叉")),
                );
            }
            col = col.child(item);
            n += 1;
        }
        if n == 0 {
            col = col.child(text("暂无消息", 12., DIM));
        }
        col
    }
    fn related_runs(&self, mut body: Div, r: &Record, cx: &Context<Self>) -> Div {
        body = body.child(headline("运行记录", 14.));
        let key = match r.kind.as_str() {
            "task" => "task_id",
            "chat" => "chat_id",
            "automation" => "automation_id",
            _ => "handler_id",
        };
        let runs: Vec<_> = self
            .list("run")
            .iter()
            .filter(|a| a.field(key) == r.id)
            .collect();
        if runs.is_empty() {
            body = body.child(text("暂无运行记录", 12., MUTED));
        }
        for run in runs {
            body = body.child(self.related_row(run, cx));
        }
        body
    }
    fn attributes(&self, r: &Record) -> Div {
        let mut table = col()
            .w_full()
            .gap(px(6.))
            .p(px(12.))
            .rounded(px(8.))
            .bg(rgb(PALE));
        for (k, v) in &r.fields {
            if matches!(
                k.as_str(),
                "body" | "description" | "instructions" | "instruction" | "im_threads"
            ) || v.is_null()
                || v == &Value::String(String::new())
            {
                continue;
            }
            let label = field_label(k);
            let value = if v.is_object() || v.is_array() {
                value_text(v)
            } else {
                label_value(k, &value_text(v))
            };
            table = table.child(
                row()
                    .w_full()
                    .items_start()
                    .gap(px(7.))
                    .child(text(label, 12., MUTED).w(px(95.)).flex_shrink_0())
                    .child(text(value, 12., INK).flex_1().min_w_0()),
            );
        }
        table
    }
}
fn field_label(k: &str) -> String {
    match k {
        "status" => "状态",
        "creator" => "创建人",
        "owner" => "负责人",
        "priority" => "优先级",
        "visibility" => "可见范围",
        "version" => "版本",
        "role" => "角色",
        "actor" => "账号",
        "assignee" => "处理人",
        "start_date" => "开始日期",
        "end_date" => "结束日期",
        "runtime" => "Runtime",
        "runtimes" => "已探测 Runtime",
        "machine_id" => "执行机器",
        "skills" => "技能",
        "tools" => "工具",
        "leader_id" => "队长",
        "members" => "成员",
        "parent_id" => "继承自",
        "working_hours" => "工作时段",
        "waiting_reason" => "等待原因",
        "queue_deadline" => "排队时限",
        "run_deadline" => "执行时限",
        "next_start_at" => "预计开始",
        "profiles" => "获准工作目录",
        "directory_mode" => "工作目录策略",
        "capacity" => "容量",
        "last_seen" => "最近在线",
        "handler_id" => "执行者",
        "result" => "运行结果",
        "artifacts" => "产物",
        "triggers" => "触发器",
        "enabled" => "启用",
        "provider" => "供应商",
        "mapping" => "字段映射",
        "reason" => "原因",
        "phase" => "阶段",
        "task_id" => "关联任务",
        "task" => "任务",
        "space" => "空间",
        "updated_at" => "更新时间",
        "number" => "编号",
        "scope" => "权限范围",
        _ => k,
    }
    .to_owned()
}
