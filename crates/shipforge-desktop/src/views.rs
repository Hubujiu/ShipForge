use crate::{
    app::{UiAction, Workbench},
    catalog::{PRIORITIES, Page, Record, STATUSES, TYPES, label_value, summary, value_text},
    controls::*,
    forms::Mode,
    motion,
};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};
#[path = "planning.rs"]
mod planning;

#[derive(Clone)]
struct DraggedTask(Record);
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
        if p == Page::Docs && self.writable() {
            header = header.child(
                outlined(self.button(
                    "upload-document",
                    "从文件新建文档",
                    UiAction::Form(Mode::Upload(None)),
                    cx,
                ))
                .child(icon("upload", 14.))
                .child("上传文档"),
            );
        }
        if p == Page::Machines && self.mode != "demo" {
            header = header.child(
                outlined(self.button("machine-probe", "扫描本机 Runtime", UiAction::Probe, cx))
                    .child("探测 Runtime"),
            );
            header = header
                .child(
                    outlined(self.button(
                        "stop-machine",
                        "停止本机工作机",
                        UiAction::Form(Mode::Machine { stop: true }),
                        cx,
                    ))
                    .child("停止本机"),
                )
                .child(
                    primary(self.button(
                        "start-machine",
                        "启动本机工作机",
                        UiAction::Form(Mode::Machine { stop: false }),
                        cx,
                    ))
                    .child(icon("play", 13.))
                    .child("启动本机工作机"),
                );
        }
        if self.loading {
            header = header.child(text("读取中…", 11., DIM));
        }
        if p.creatable() && self.writable() {
            let k = info.kind;
            header = header.child(
                primary(self.button(
                    "new-record",
                    format!("新建{}", info.noun),
                    UiAction::Create(k.into()),
                    cx,
                ))
                .h(px(26.))
                .child(icon("plus", 13.))
                .child(format!("新建{}", info.noun)),
            );
        }
        let mut root = col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(rgb(BG))
            .child(header);
        if let Some(error) = self.error.as_ref().or_else(|| self.errors.get(info.kind)) {
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
                            .child("重试"),
                    ),
            );
        }
        match p {
            Page::Tasks | Page::Mine | Page::Triage | Page::Dashboard => root
                .child(self.task_page(f32::from(w.viewport_size().width), cx))
                .into_any_element(),
            Page::Settings => root.child(self.settings_page(cx)).into_any_element(),
            Page::Iterations => root.child(self.iteration_page(cx)).into_any_element(),
            Page::Transfer => root.child(self.transfer_page(cx)).into_any_element(),
            Page::Market => root.child(self.market_page(cx)).into_any_element(),
            _ => root.child(self.record_page(cx)).into_any_element(),
        }
    }
    fn pill(
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
            .child(label.to_owned())
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
                    .child(label),
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
            for (k, l) in TYPES.iter().copied() {
                tools = tools.child(self.pill(
                    format!("type-{k}"),
                    l,
                    UiAction::TaskType(k.into()),
                    self.loc.task_type == k,
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
                            .child("清除"),
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
            if task.space!=this.loc.space||task.field(&group)==key{return;}
            let mut patch=json!({(&group):key});
            if group=="assignee"{
                if key.is_empty(){patch["assignee"]=Value::Null;}
                else{patch["assignee_kind"]=json!(["agent","crew","member"].into_iter().find(|kind|this.find(kind,&key).is_some()).unwrap_or("member"));}
            }
            this.act(UiAction::Apply(json!({"op":"update","kind":"task","id":task.id,"expected_rev":task.rev,"fields":patch}),"任务已移动".into()),window,cx);
        }))
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
            "priority" => PRIORITIES
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string(), VIOLET))
                .collect(),
            "type" => self
                .options("$types")
                .into_iter()
                .map(|(a, b)| (a, b, GREEN))
                .collect(),
            "assignee" => Vec::new(),
            _ => STATUSES
                .iter()
                .filter(|s| s.0 != "triage" || self.loc.page == Page::Triage)
                .map(|s| (s.0.into(), s.1.into(), s.2))
                .collect(),
        };
        for r in tasks {
            let key = r.field(group);
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
            .filter(|(key, _, _)| tasks.iter().any(|task| task.field(group) == key))
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
            let items: Vec<_> = tasks.iter().filter(|r| r.field(group) == key).collect();
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
                    .child(text(label, 12., MUTED))
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
                    format!("{}-task-{}", self.loc.space, r.id),
                    self.revision,
                    self.task_card(r, cx),
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
                        .child(headline(label, 13.))
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
    fn task_card(&self, r: &Record, cx: &Context<Self>) -> impl IntoElement {
        let mut bottom = row()
            .w_full()
            .gap(px(5.))
            .child(icon("circle-dot", 12.).text_color(rgb(GREEN)))
            .child(text(label_value("type", r.field("type")), 11., MUTED));
        if !r.field("due_date").is_empty() {
            bottom = bottom
                .child(icon("calendar-days", 12.).text_color(rgb(MUTED)))
                .child(text(r.field("due_date").to_owned(), 11., MUTED));
        }
        bottom = bottom.child(div().flex_1()).child(text(
            self.handler_name(r.field("assignee")),
            11.,
            DIM,
        ));
        self.button(
            format!("task-card-{}", r.id),
            r.title.clone(),
            UiAction::Open(r.clone()),
            cx,
        )
        .when(self.writable(), |btn| {
            btn.on_drag(DraggedTask(r.clone()), |drag, _, _, cx| {
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
        for r in tasks {
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
                .child(text(label_value("status", r.field("status")), 12., MUTED).w(px(78.)));
            if table {
                b = b.child(
                    text(self.handler_name(r.field("assignee")), 12., MUTED)
                        .w(px(95.))
                        .truncate(),
                );
            }
            b = b.child(text(label_value("priority", r.field("priority")), 12., DIM).w(px(56.)));
            list = list.child(motion::layout(
                format!("{}-task-row-{}", self.loc.space, r.id),
                self.revision,
                b,
            ));
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
        for (k, l, color) in STATUSES {
            let count = tasks.iter().filter(|r| r.field("status") == *k).count();
            chart = chart.child(
                row()
                    .gap(px(8.))
                    .child(text((*l).to_owned(), 12., MUTED).w(px(50.)))
                    .child(
                        div()
                            .w(px((count as f32 / n.max(1) as f32) * 230.))
                            .h(px(8.))
                            .rounded(px(3.))
                            .bg(rgb(*color)),
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
        for (key, label) in PRIORITIES {
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
                    format!("新建{}", info.noun),
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
        if self.mode != "demo" {
            page = page.child(
                row()
                    .gap(px(8.))
                    .child(
                        primary(self.button("team-connect", "连接团队服务", UiAction::Connect, cx))
                            .child(icon("cloud", 14.))
                            .child("连接团队服务"),
                    )
                    .when(self.mode == "team", |row| {
                        row.child(
                            outlined(self.button(
                                "team-disconnect",
                                "切换到本机",
                                UiAction::Disconnect,
                                cx,
                            ))
                            .child("切换到本机"),
                        )
                    })
                    .when(self.mode == "team" && self.writable(), |row| {
                        row.child(
                            outlined(self.button("issue-token", "签发令牌", UiAction::Grant, cx))
                                .child("签发令牌"),
                        )
                        .child(
                            outlined(self.button("list-tokens", "管理令牌", UiAction::Tokens, cx))
                                .child("管理令牌"),
                        )
                    }),
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
                    .child("待同步更改"),
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
                        .child("处理冲突"),
                    );
                }
                buttons=buttons.child(self.button(format!("remove-draft-{index}"),"舍弃草稿",UiAction::Form(Mode::Confirm{label:format!("舍弃草稿「{title}」"),command:json!({"op":"sync.resolve","id":draft["request_id"],"discard":true})}),cx).child("舍弃"));
                page = page.child(item.child(buttons));
            }
        }
        if self.loc.section == "tokens" {
            for t in &self.tokens {
                let id = t.get("id").and_then(Value::as_str).unwrap_or("");
                page=page.child(row().min_h(px(33.)).gap(px(10.)).child(text(format!("{} · {}",t.get("actor").map(value_text).unwrap_or_default(),t.get("scope").map(value_text).unwrap_or_default()),12.,INK)).child(text(id.to_owned(),11.,DIM)).child(self.button(format!("revoke-{id}"),"撤销访问",UiAction::Form(Mode::Confirm{label:"撤销该访问令牌".into(),command:json!({"op":"access.revoke","id":id,"space":self.loc.space})}),cx).child("撤销")));
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
                            .child("导出 JSON"),
                    )
                    .when(self.writable(), |r| {
                        r.child(
                            primary(self.button("import-file", "导入 JSON", UiAction::Import, cx))
                                .child(icon("upload", 14.))
                                .child("导入 JSON"),
                        )
                    }),
            )
    }
    fn market_page(&self, cx: &Context<Self>) -> AnyElement {
        let agents = self.loc.section == "agents";
        let kind = if agents { "agent" } else { "skill" };
        let mut page = col()
            .id("market-list")
            .w_full()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .p(px(18.))
            .gap(px(12.))
            .child(
                row()
                    .gap(px(5.))
                    .child(self.pill(
                        "market-skills".into(),
                        "技能",
                        UiAction::Section(String::new()),
                        !agents,
                        cx,
                    ))
                    .child(self.pill(
                        "market-agents".into(),
                        "Agent 模板",
                        UiAction::Section("agents".into()),
                        agents,
                        cx,
                    )),
            );
        for t in self.list("catalog").iter().filter(|t| t.kind == kind) {
            let installed = self
                .list(kind)
                .iter()
                .any(|r| r.field("template_id") == t.id);
            let cmd = json!({"op":"catalog.install","id":t.id});
            page = page.child(
                row()
                    .w_full()
                    .p(px(13.))
                    .gap(px(12.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(white())
                    .rounded(px(9.))
                    .child(icon(if agents { "bot" } else { "blocks" }, 22.).text_color(rgb(VIOLET)))
                    .child(
                        col()
                            .flex_1()
                            .min_w_0()
                            .child(headline(t.title.clone(), 14.))
                            .child(text(t.description.clone(), 12., MUTED)),
                    )
                    .child(
                        outlined(self.button(
                            format!("install-{}", t.id),
                            "添加到空间",
                            UiAction::Apply(cmd, "已添加到空间".into()),
                            cx,
                        ))
                        .disabled(installed || !self.writable())
                        .child(if installed {
                            "已添加"
                        } else {
                            "添加到空间"
                        }),
                    ),
            );
        }
        page.into_any_element()
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
        let long = match r.kind.as_str() {
            "document" | "message" | "comment" => r.field("body"),
            "skill" | "agent" => r.field("instructions"),
            "automation" => r.field("instruction"),
            _ => r.field("description"),
        };
        if !long.is_empty() {
            body = body.child(text(long.to_owned(), 14., INK));
        }
        let can_edit = self.writable()
            && !matches!(
                r.kind.as_str(),
                "run" | "notification" | "machine" | "artifact"
            );
        let mut actions = row().flex_wrap().gap(px(5.));
        if can_edit {
            actions = actions.child(
                outlined(self.button("edit-object", "编辑", UiAction::Edit(r.clone()), cx))
                    .child(icon("square-pen", 13.))
                    .child("编辑"),
            );
        }
        match r.kind.as_str() {
            "task" => {
                let following = r.fields.get("following") == Some(&Value::Bool(true));
                let (label, glyph) = if following {
                    ("取消关注", "bell-off")
                } else {
                    ("关注", "bell")
                };
                actions = actions.child(
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
                    .child(label),
                );
                if self.writable() {
                    actions = actions.child(
                        outlined(self.button(
                            "add-comment",
                            "添加评论",
                            UiAction::Form(Mode::Comment(r.clone())),
                            cx,
                        ))
                        .child(icon("messages-square", 13.))
                        .child("评论"),
                    );
                }
                body = body.child(actions);
                if !side_properties {
                    body = body.child(self.task_properties(r, cx));
                }
                body = body
                    .child(headline("评论与活动", 14.))
                    .child(self.thread_items("comment", "task", &r.id));
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
                body = self.related_runs(body, r, cx);
            }
            "chat" => {
                if self.writable() {
                    actions = actions.child(
                        primary(self.button(
                            "send-message",
                            "发送消息",
                            UiAction::Form(Mode::Message(r.clone())),
                            cx,
                        ))
                        .child(icon("send", 14.))
                        .child("发送消息"),
                    );
                }
                body = body
                    .child(actions)
                    .child(headline("消息", 14.))
                    .child(self.thread_items("message", "chat", &r.id));
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
                    .child("历史版本"),
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
                                        .child("恢复"),
                                    )
                                }),
                        );
                    }
                }
            }
            "iteration" | "project" => {
                if r.kind == "iteration" && r.field("status") != "closed" && self.writable() {
                    actions = actions.child(
                        outlined(self.button(
                            "finish-iteration",
                            "结束迭代",
                            UiAction::IterationEnd(r.clone()),
                            cx,
                        ))
                        .child("结束迭代"),
                    );
                }
                body = body
                    .child(actions)
                    .child(self.attributes(r))
                    .child(headline("关联任务", 14.));
                for t in self
                    .list("task")
                    .iter()
                    .filter(|t| t.field(&r.kind) == r.id)
                {
                    body = body.child(self.related_row(t, cx));
                }
            }
            "agent" => {
                actions = actions.child(
                    outlined(self.button(
                        "agent-runs",
                        "运行历史",
                        UiAction::ViewAgentHistory(r.clone()),
                        cx,
                    ))
                    .child(icon("clock-arrow-left", 13.))
                    .child("运行历史"),
                );
                body = body.child(actions);
                if self.drawer_tab == "runs" {
                    body = self.related_runs(body, r, cx);
                } else {
                    body = body.child(self.attributes(r));
                }
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
                        .child("运行一次"),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
                body = self.related_runs(body, r, cx);
            }
            "machine" => {
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
                        .child("存为个人文档"),
                    );
                }
                body = body.child(actions).child(self.attributes(r));
            }
            "run" => {
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
                            .child("回复"),
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
                        .child(label),
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
                    .child("添加附件"),
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
            ("当前状态", label_value("status", r.field("status"))),
            ("类型", label_value("type", r.field("type"))),
            ("处理人", self.handler_name(r.field("assignee"))),
            ("负责人", self.name("member", r.field("owner"))),
            ("优先级", label_value("priority", r.field("priority"))),
            ("迭代", self.name("iteration", r.field("iteration"))),
            ("专项", self.name("project", r.field("project"))),
            ("截止日期", r.field("due_date").into()),
            ("标签", r.display("labels")),
        ];
        for (k, v) in entries {
            props = props.child(
                row()
                    .gap(px(6.))
                    .child(text(k, 12., MUTED).w(px(82.)))
                    .child(text(if v.is_empty() { "—".into() } else { v }, 12., INK)),
            );
        }
        if self.writable() {
            let mut actions = row().flex_wrap().pt(px(8.)).gap(px(4.));
            if r.field("status") == "triage" {
                actions = actions.child(
                    outlined(self.button(
                        "triage-duplicate",
                        "标记重复",
                        UiAction::Form(Mode::Workflow {
                            op: "triage",
                            record: r.clone(),
                        }),
                        cx,
                    ))
                    .child("标记重复"),
                );
                for (d, l) in [("accept", "接收"), ("defer", "延后"), ("reject", "拒绝")] {
                    actions = actions.child(
                        outlined(self.button(
                            format!("triage-{d}"),
                            l,
                            UiAction::Apply(
                                json!({"op":"triage","id":r.id,"decision":d,"expected_rev":r.rev}),
                                "已分拣".into(),
                            ),
                            cx,
                        ))
                        .child(l),
                    );
                }
            } else {
                for (status, label) in [
                    ("todo", "待开始"),
                    ("in_progress", "开始处理"),
                    ("done", "标为完成"),
                ] {
                    if r.field("status") != status {
                        actions=actions.child(outlined(self.button(format!("set-{status}"),label,UiAction::Apply(json!({"op":"update","kind":"task","id":r.id,"expected_rev":r.rev,"fields":{"status":status}}),"状态已更新".into()),cx)).child(label));
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
        .child(text(label_value("status", r.field("status")), 11., MUTED))
    }
    fn thread_items(&self, kind: &str, key: &str, id: &str) -> Div {
        let mut col = col().gap(px(9.));
        let mut n = 0;
        for r in self.list(kind).iter().filter(|r| r.field(key) == id) {
            col = col.child(
                crate::controls::col()
                    .p(px(10.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .rounded(px(7.))
                    .child(text(self.name("member", r.field("creator")), 11., MUTED))
                    .child(text(
                        if r.field("body").is_empty() {
                            r.title.clone()
                        } else {
                            r.field("body").into()
                        },
                        13.,
                        INK,
                    )),
            );
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
                "body" | "description" | "instructions" | "instruction"
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
