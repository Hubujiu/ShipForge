use super::*;
use crate::app::chat_actions::{self, Action};

impl Workbench {
    pub(crate) fn chat_row_actions(&self, room: &Record, cx: &Context<Self>) -> Div {
        let pinned = room
            .fields
            .get("sidebar")
            .is_some_and(|v| v["pinned"] == true);
        let label = if pinned { "取消置顶" } else { "置顶" };
        row()
            .gap(px(6.))
            .child(
                outlined(self.button(
                    format!("chat-pin-{}", room.id),
                    label,
                    chat_actions::organize(room, if pinned { "unpin" } else { "pin" }),
                    cx,
                ))
                .child(crate::locale::label(label)),
            )
            .child(
                outlined(self.button(
                    format!("chat-hide-{}", room.id),
                    "不显示",
                    chat_actions::organize(room, "hide"),
                    cx,
                ))
                .child(crate::locale::label("不显示")),
            )
    }
    fn chat_list_row(&self, room: &Record, compact: bool, cx: &Context<Self>) -> Div {
        let target = room.clone();
        let menu = room.clone();
        let prefix = if compact { "side" } else { "page" };
        let hover = SharedString::from(format!("chat-row-{prefix}-{}", room.id));
        let mut line = row()
            .gap(px(3.))
            .min_h(px(if compact { 29. } else { 43. }))
            .w_full()
            .group(hover.clone())
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |v, _, w, cx| {
                    v.act(UiAction::Chat(Action::Row(menu.clone())), w, cx)
                }),
            )
            .child(
                self.button(
                    format!("chat-{prefix}-{}", room.id),
                    room.title.clone(),
                    UiAction::OpenSearch(target),
                    cx,
                )
                .min_w_0()
                .flex_1()
                .justify_start()
                .child(icon(
                    if room
                        .fields
                        .get("sidebar")
                        .is_some_and(|v| v["pinned"] == true)
                    {
                        "pin"
                    } else {
                        "messages-square"
                    },
                    14.,
                ))
                .child(text(room.title.clone(), 13., INK).truncate()),
            );
        if compact {
            line = line
                .child(
                    div()
                        .invisible()
                        .group_hover(hover, |el| el.visible())
                        .child(self.tool_button(
                            &format!("chat-hover-hide-{}", room.id),
                            crate::locale::label("不显示"),
                            "eye-off",
                            chat_actions::organize(room, "hide"),
                            cx,
                        )),
                )
                .child(self.tool_button(
                    &format!("chat-more-{}", room.id),
                    crate::locale::label("对话列表操作"),
                    "ellipsis",
                    UiAction::Chat(Action::Row(room.clone())),
                    cx,
                ));
        } else {
            line = line.child(self.chat_row_actions(room, cx));
        }
        line
    }
    pub(crate) fn chat_sidebar_group(&self, cx: &Context<Self>) -> Div {
        let mut group = col().min_w_0().mb(px(12.)).child(
            self.button("open-chat-sidebar", "对话", UiAction::Go(Page::Chats), cx)
                .justify_start()
                .child(text("对话", 11., DIM)),
        );
        let rooms = self.list("chat_sidebar");
        let fixed = rooms
            .iter()
            .take_while(|r| r.fields.get("sidebar").is_some_and(|s| s["pinned"] == true))
            .count();
        let max = fixed.max(12);
        for room in rooms.iter().take(max) {
            group = group.child(self.chat_list_row(room, true, cx));
        }
        if rooms.len() > max {
            group = group.child(
                self.button(
                    "all-chat-sidebar",
                    "全部对话",
                    UiAction::Go(Page::Chats),
                    cx,
                )
                .child(crate::locale::label("全部对话")),
            );
        }
        group
    }
    pub(super) fn chat_list_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut list = col()
            .id("chat-recent-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(16.))
            .gap(px(4.));
        let mut last = None;
        for room in self.list("chat_sidebar") {
            let pinned = room
                .fields
                .get("sidebar")
                .is_some_and(|state| state["pinned"] == true);
            if last != Some(pinned) {
                list = list
                    .child(headline(if pinned { "已置顶" } else { "最近对话" }, 13.).pt(px(8.)));
                last = Some(pinned);
            }
            list = list.child(self.chat_list_row(room, false, cx));
        }
        if self.list("chat_sidebar").is_empty() {
            list = list.child(text("这里会显示你的对话。", 13., MUTED));
        }
        list
    }
    pub(super) fn chat_invitations(&self, cx: &Context<Self>) -> Div {
        let open = outlined(self.button(
            "open-conversation-share",
            "打开已分享的对话",
            UiAction::Form(Mode::OpenChatShare),
            cx,
        ))
        .child("打开分享链接");
        let mut list = col().child(row().px(px(16.)).py(px(4.)).child(open));
        for invite in self.list("chat_invitation") {
            let label = format!(
                "{} 邀请你加入「{}」",
                self.name("member", invite.field("inviter")),
                invite.title
            );
            let command =
                json!({"op":"chat.manage","space":self.loc.space,"id":invite.id,"action":"accept"});
            let accept = outlined(self.button(
                format!("accept-{}", invite.id),
                "接受对话邀请",
                UiAction::Apply(command, "已加入对话".into()),
                cx,
            ))
            .child("接受");
            let decline=outlined(self.button(format!("decline-{}",invite.id),"拒绝对话邀请",UiAction::Apply(json!({"op":"chat.manage","space":self.loc.space,"id":invite.id,"action":"decline"}),"已拒绝邀请".into()),cx)).child("拒绝");
            list = list.child(
                row()
                    .px(px(16.))
                    .py(px(5.))
                    .gap(px(8.))
                    .child(text(label, 13., INK))
                    .child(accept)
                    .child(decline),
            );
        }
        list
    }

    pub(super) fn chat_controls(&self, room: &Record, info: &Value, cx: &Context<Self>) -> Div {
        let group = info["group"] == true;
        let managed = group || room.field("visibility") == "private";
        let owner = info["is_owner"] == true;
        let mut actions = row().flex_wrap().gap(px(6.));
        let button = |id: &str, label: &'static str, action: UiAction| {
            outlined(self.button(
                format!("chat-{}-{id}", room.id),
                crate::locale::label(label),
                action,
                cx,
            ))
            .child(crate::locale::label(label))
        };
        if self.writable() && managed && (owner || !group) {
            let label = if owner {
                "管理成员"
            } else {
                "邀请成员"
            };
            actions = actions.child(button(
                "members",
                label,
                UiAction::Form(Mode::Workflow {
                    op: "chat.manage",
                    record: room.clone(),
                }),
            ));
        }
        if self.writable() && owner && group {
            actions = actions.child(button(
                "agents",
                "管理 Agent",
                UiAction::Form(Mode::Workflow {
                    op: "chat.agents",
                    record: room.clone(),
                }),
            ));
        }
        if self.writable() && owner {
            if chat_actions::agent_room(room)
                && room.field("task").is_empty()
                && room.field("task_id").is_empty()
            {
                actions = actions
                    .child(button(
                        "convert",
                        "将对话转为任务",
                        UiAction::Chat(Action::Convert(room.clone())),
                    ))
                    .child(button(
                        "bind",
                        "绑定到已有任务",
                        UiAction::Form(Mode::BindChat(room.clone())),
                    ));
            }
            actions = actions.child(button(
                "share",
                "分享已选消息",
                UiAction::Form(Mode::Workflow {
                    op: "chat.share.create",
                    record: room.clone(),
                }),
            ));
        }
        for (key, on, off) in [
            ("muted", "关闭免打扰", "开启免打扰"),
            ("ignore_all", "恢复 @所有人 提醒", "忽略 @所有人 提醒"),
        ] {
            if key == "ignore_all" && !group {
                continue;
            }
            let current = info[key] == true;
            let cmd =
                json!({"op":"chat.preferences","space":self.loc.space,"id":room.id,(key):!current});
            actions = actions.child(button(
                key,
                if current { on } else { off },
                UiAction::Apply(cmd, "对话提醒已更新".into()),
            ));
        }
        let confirm = |action: &str, label: &'static str| {
            button(
                action,
                label,
                UiAction::Form(Mode::Confirm {
                    label: label.into(),
                    command: json!({"op":"chat.manage","id":room.id,"space":self.loc.space,"action":action}),
                }),
            )
        };
        actions = actions.child(confirm("clear", "清空我的聊天记录"));
        if managed && !owner {
            actions = actions.child(confirm("leave", "退出对话"));
        }
        if group && owner {
            actions = actions.child(confirm("dissolve", "解散群聊"));
        }
        let mut section = col().gap(px(5.));
        for share in info["shares"].as_array().into_iter().flatten() {
            if let Some(key) = share["share"].as_str() {
                let count = share["message_ids"].as_array().map_or(0, Vec::len);
                let scope = if share["scope"] == "link" {
                    "持链接者"
                } else {
                    "本组织成员"
                };
                let read = button(
                    &format!("copy-{key}"),
                    "复制分享码",
                    UiAction::Copy(key.to_owned()),
                );
                let revoke = button(
                    &format!("revoke-{key}"),
                    "撤销此选区的分享",
                    UiAction::Form(Mode::Confirm {
                        label: "撤销这些消息的所有分享链接".into(),
                        command: json!({"op":"chat.share.revoke","space":self.loc.space,"id":room.id,"share":key}),
                    }),
                );
                section = section.child(
                    row()
                        .gap(px(6.))
                        .child(text(format!("{scope} · {count} 条消息"), 12., MUTED))
                        .child(read)
                        .child(button(
                            &format!("png-{key}"),
                            "保存分享图片",
                            UiAction::Form(Mode::Conversation(
                                crate::app::conversation_ui::CommandForm::Image(key.into()),
                            )),
                        ))
                        .child(revoke),
                );
            }
        }
        if managed {
            let title = if group { "群主" } else { "对话所有者" };
            section = section.child(text(
                format!(
                    "{title} · {}",
                    self.name("member", info["owner"].as_str().unwrap_or(""))
                ),
                13.,
                INK,
            ));
            for (field, label, kind) in [
                ("members", "成员", "member"),
                ("pending", "等待接受", "member"),
                ("agents", "Agent", "agent"),
            ] {
                if let Some(list) = info[field].as_array().filter(|v| !v.is_empty()) {
                    let names = list
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|id| self.name(kind, id))
                        .collect::<Vec<_>>()
                        .join("、");
                    section = section.child(text(format!("{label} · {names}"), 12., MUTED));
                }
            }
        }
        section.child(actions)
    }
}
