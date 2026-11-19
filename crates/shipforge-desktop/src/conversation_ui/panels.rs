use super::*;
use controls::{INK, MUTED, VIOLET, col, outlined, row, text};
impl Workbench {
    fn comm_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<String>,
        action: UiAction,
        cx: &Context<Self>,
    ) -> gpui_kit::base::Button {
        let label = label.into();
        outlined(self.button(id, label.clone(), action, cx)).child(label)
    }
    pub fn conversation_controls(&self, r: &Record, wide: bool, cx: &Context<Self>) -> Div {
        let mut buttons = row().gap(px(6.)).flex_wrap();
        match r.kind.as_str() {
            "task" => {
                buttons = buttons.child(self.comm_button(
                    "task-shortcuts",
                    "快捷指令",
                    UiAction::Conversation(Action::Shortcuts(r.clone())),
                    cx,
                ));
                if r.fields.get("awaiting_review") == Some(&Value::Bool(true)) {
                    buttons = buttons.child(text("待审阅", 13., VIOLET));
                    if self.writable() {
                        buttons=buttons.child(self.comm_button("send-confirm-review","已审阅，没问题",UiAction::Form(Mode::Confirm{label:"发送「已审阅，没问题」".into(),command:json!({"op":"task.review","space":r.space,"id":r.id,"expected_rev":r.rev,"request_id":uuid::Uuid::new_v4().to_string()})}),cx));
                    }
                }
            }
            "document" => {
                buttons = buttons
                    .child(self.comm_button(
                        "doc-share",
                        "复制文档链接",
                        UiAction::Conversation(Action::ShareDocument(r.id.clone())),
                        cx,
                    ))
                    .child(self.comm_button(
                        "doc-outline",
                        "文档大纲",
                        UiAction::Conversation(Action::Outline(r.clone())),
                        cx,
                    ));
            }
            "chat" if chat_actions::agent_room(r) && self.writable() => {
                let completed = r
                    .fields
                    .get("conversation")
                    .is_some_and(|v| v["completed"] == true);
                if completed {
                    buttons = buttons.child(text("对话已完成", 13., VIOLET));
                } else {
                    let label = if r.field("task").is_empty() && r.field("task_id").is_empty() {
                        "标为已完成"
                    } else {
                        "将对话及关联任务标为已完成"
                    };
                    buttons=buttons.child(self.comm_button("complete-conversation",label,UiAction::Form(Mode::Confirm{label:label.into(),command:json!({"op":"chat.complete","space":r.space,"id":r.id,"expected_rev":r.rev,"request_id":uuid::Uuid::new_v4().to_string()})}),cx));
                }
            }
            _ => {}
        }
        let mut result = col().gap(px(4.)).child(buttons);
        if r.kind == "document"
            && (wide || self.communication.outline_manual)
            && let Some((s, id, outline)) = &self.communication.outline
            && s == &r.space
            && id == &r.id
        {
            if outline.as_array().is_some_and(Vec::is_empty) {
                result = result.child(text("此文档没有 Markdown 标题。", 12., MUTED));
            }
            for item in outline.as_array().into_iter().flatten() {
                result = result.child(text(
                    format!(
                        "{}{} · 第 {} 行",
                        "　".repeat(item["level"].as_u64().unwrap_or(1).saturating_sub(1) as usize),
                        item["text"].as_str().unwrap_or(""),
                        item["line"]
                    ),
                    12.,
                    INK,
                ));
            }
        }
        result
    }
    pub fn document_embeds_panel(&self, r: &Record, cx: &Context<Self>) -> Div {
        let mut panel = col().gap(px(4.));
        if !(r.field("body").contains("shipforge://document/")
            || r.field("description").contains("shipforge://document/"))
        {
            return panel;
        }
        let key = (r.space.clone(), r.kind.clone(), r.id.clone());
        let data = self.communication.embeds.get(&key);
        panel = panel.child(self.comm_button(
            format!("doc-embeds-{}-{}", r.kind, r.id),
            "预览引用文档",
            UiAction::Conversation(Action::Embeds(r.clone())),
            cx,
        ));
        for (i, item) in data
            .into_iter()
            .flat_map(|v| v["items"].as_array())
            .flatten()
            .enumerate()
        {
            if item["available"] != true {
                panel = panel.child(text("无权访问或文档已不存在", 12., MUTED));
                continue;
            }
            let p = &item["preview"];
            let link = item["link"].as_str().unwrap_or("");
            panel = panel
                .child(self.comm_button(
                    format!("doc-embedded-{}-{}-{i}", r.kind, r.id),
                    format!("打开 {}", p["title"].as_str().unwrap_or("文档")),
                    UiAction::Conversation(Action::Open(link.into())),
                    cx,
                ))
                .child(text(
                    p["summary"].as_str().unwrap_or("").to_owned(),
                    13.,
                    INK,
                ));
        }
        panel
    }
    pub fn slash_panel(&self, cx: &Context<Self>) -> Div {
        let mut menu = col().gap(px(4.));
        if self.communication.slash.is_none() {
            return menu;
        }
        let v = &self.communication.options;
        if v.is_null() {
            return menu.child(text("正在读取 Agent 能力…", 12., MUTED));
        }
        let message = if v["online"] != true {
            "Agent 运行环境离线"
        } else if v["commands_reported"] != true {
            "运行环境尚未报告内置命令"
        } else if v["busy"] == true {
            "Agent 正在处理任务，新消息将排队"
        } else {
            "Tab 插入首项，Esc 关闭"
        };
        menu = menu.child(row().gap(px(5.)).child(text(message, 12., MUTED)).child(
            self.comm_button(
                "slash-retry",
                "重新获取",
                UiAction::Conversation(Action::Retry),
                cx,
            ),
        ));
        for (group, title) in [("skill", "Skills"), ("command", "Commands")] {
            let entries: Vec<_> = v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| r["group"] == group)
                .collect();
            if !entries.is_empty() {
                menu = menu.child(text(title, 12., VIOLET));
            }
            for (i, item) in entries.into_iter().enumerate() {
                let token = item["token"].as_str().unwrap_or("");
                let label = if group == "skill" {
                    format!("{token} · {}", item["name"].as_str().unwrap_or(""))
                } else {
                    token.into()
                };
                menu = menu.child(self.comm_button(
                    format!("slash-{group}-{i}"),
                    label,
                    UiAction::Conversation(Action::Insert(token.into())),
                    cx,
                ));
            }
        }
        if v["online"] == true
            && v["commands_reported"] == true
            && v["items"].as_array().is_some_and(Vec::is_empty)
        {
            menu = menu.child(text("没有匹配的指令。", 12., MUTED));
        }
        menu
    }
    pub fn shortcut_panel(&self, task: &Record, cx: &Context<Self>) -> Div {
        let s = &self.communication;
        let manage = s.manage;
        let mut panel = col().gap(px(7.));
        let mut bar = row().gap(px(5.)).flex_wrap().child(self.comm_button(
            "shortcut-more",
            if s.more {
                "收起更多"
            } else {
                "更多快捷指令"
            },
            UiAction::Conversation(Action::More),
            cx,
        ));
        if self.writable() {
            bar = bar.child(self.comm_button(
                "shortcut-manage",
                if manage {
                    "结束管理"
                } else {
                    "管理快捷指令"
                },
                UiAction::Conversation(Action::Manage),
                cx,
            ));
        }
        if manage {
            bar = bar.child(self.comm_button(
                "shortcut-new",
                "新建快捷指令",
                UiAction::Form(Mode::Conversation(CommandForm::Edit(None))),
                cx,
            ));
        }
        panel = panel.child(bar);
        let favorites: Vec<_> = s
            .catalog
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["pinned"] == true)
            .filter_map(|v| v["id"].as_str())
            .collect();
        if favorites.is_empty() && !manage && !s.more {
            panel = panel.child(text("尚未收藏快捷指令。", 12., MUTED));
        }
        for d in s.catalog.as_array().into_iter().flatten() {
            let pinned = d["pinned"] == true;
            if !manage && !s.more && !pinned {
                continue;
            }
            let id = d["id"].as_str().unwrap_or("");
            let mut line = row().gap(px(5.)).flex_wrap();
            if d["can_run"] == true {
                line = line.child(self.comm_button(
                    format!("shortcut-run-{id}"),
                    d["title"].as_str().unwrap_or(""),
                    UiAction::Conversation(Action::Run(id.into(), task.id.clone())),
                    cx,
                ));
            } else {
                line = line.child(text(
                    d["title"].as_str().unwrap_or("").to_owned(),
                    13.,
                    MUTED,
                ));
            }
            if manage {
                line = line.child(self.comm_button(
                    format!("shortcut-star-{id}"),
                    if pinned { "取消收藏" } else { "收藏" },
                    UiAction::Conversation(Action::Organize(id.into(), json!({"pinned":!pinned}))),
                    cx,
                ));
                if let Some(index) = favorites.iter().position(|value| value == &id) {
                    for (step, label) in [(-1_isize, "上移"), (1, "下移")] {
                        let dest = index as isize + step;
                        if (0..favorites.len() as isize).contains(&dest) {
                            let mut ordered = favorites.clone();
                            ordered.swap(index, dest as usize);
                            line = line.child(self.comm_button(
                                format!("shortcut-position-{id}-{step}"),
                                label,
                                UiAction::Conversation(Action::Organize(
                                    id.into(),
                                    json!({"order":ordered}),
                                )),
                                cx,
                            ));
                        }
                    }
                }
                if d["can_edit"] == true {
                    line=line.child(self.comm_button(format!("shortcut-edit-{id}"),"编辑",UiAction::Form(Mode::Conversation(CommandForm::Edit(Some(d.clone())))),cx))
                    .child(self.comm_button(format!("shortcut-delete-{id}"),"删除快捷指令",UiAction::Form(Mode::Confirm{label:"删除快捷指令".into(),command:json!({"op":"shortcut.delete","space":task.space,"id":id,"expected_rev":d["rev"]})}),cx));
                }
            }
            panel = panel.child(line);
        }
        for (i, session) in s.history["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if i == 0 {
                panel = panel.child(text("历史会话", 13., VIOLET));
            }
            panel = panel.child(self.comm_button(
                format!("shortcut-history-{i}"),
                format!(
                    "{}{}",
                    if i == 0 { "上次选择 · " } else { "" },
                    session["title"].as_str().unwrap_or("会话")
                ),
                UiAction::Conversation(Action::Resume(
                    session["chat"].as_str().unwrap_or("").into(),
                )),
                cx,
            ));
        }
        panel
    }
}
