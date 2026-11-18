use super::*;

impl Workbench {
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
        let button = |id: &str, label: &str, action: UiAction| {
            outlined(self.button(
                format!("chat-{}-{id}", room.id),
                label.to_owned(),
                action,
                cx,
            ))
            .child(label.to_owned())
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
        let confirm = |action: &str, label: &str| {
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
