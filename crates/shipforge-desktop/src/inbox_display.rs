use super::*;
fn label(key: &str) -> &str {
    match key {
        "desktop" => "桌面端",
        "mobile" => "移动端",
        "browser" => "浏览器",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        "slack" => "Slack",
        "lark" => "Lark",
        "inbox" => "收件箱",
        "email" => "邮件",
        "assignment" => "任务分配",
        "status" => "任务状态变更",
        "discussion" => "评论与回复",
        "mentions" => "提及",
        "agent" => "Agent 活动",
        "triage" => "分拣责任",
        "task_details" => "任务详情",
        _ => event_label(key),
    }
}
pub(super) fn event_label(key: &str) -> &str {
    match key {
        "assigned" => "任务指派给你",
        "unassigned" => "你不再处理此任务",
        "assignees_changed" => "指派给你的任务更换处理人",
        "followed_assignees" => "关注任务的处理人变更",
        "follower_added" => "你被添加为关注者",
        "assigned_status" => "负责的任务变更状态",
        "followed_status" => "关注的任务变更状态",
        "assigned_priority" => "负责的任务变更优先级",
        "followed_priority" => "关注的任务变更优先级",
        "assigned_start" => "负责的任务变更开始日期",
        "followed_start" => "关注的任务变更开始日期",
        "assigned_due" => "负责的任务变更截止日期",
        "followed_due" => "关注的任务变更截止日期",
        "assigned_done" => "负责的任务已完成",
        "followed_done" => "关注的任务已完成",
        "comment" => "任务有新评论",
        "reply" => "任务有新回复",
        "reaction" => "评论收到表情回应",
        "chat" => "收到对话消息",
        "mention" => "评论提到了你",
        "chat_mention" => "对话提到了你",
        "description_mention" => "任务描述提到了你",
        "agent_comment" => "Agent 在任务中发表评论",
        "run_failed" => "Agent 执行失败",
        "run_blocked" => "Agent 执行受阻",
        "run_done" => "Agent 已完成工作",
        "run_handoff" => "运行需要回复",
        "automation_result" => "自动化产生结果",
        "triage_duty" => "此任务需要你分拣",
        "task_updated" => "任务详情已更新",
        _ => "新通知",
    }
}
#[cfg(windows)]
pub(super) fn system_banner(title: &str, body: &str, link: &str) {
    use windows::{
        Data::Xml::Dom::XmlDocument,
        UI::Notifications::{ToastNotification, ToastNotificationManager},
        core::HSTRING,
    };
    fn xml(text: &str) -> String {
        text.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let show = || -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        let content = format!(
            "<toast activationType=\"protocol\" launch=\"{}\"><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
            xml(link),
            xml(title),
            xml(body)
        );
        doc.LoadXml(&HSTRING::from(content))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("ShipForge.Workspace"))?
            .Show(&toast)?;
        Ok(())
    };
    // A portable launch may have no registered notification identity. Its window banner remains actionable.
    let _ = show();
}
#[cfg(not(windows))]
pub(super) fn system_banner(_: &str, _: &str, _: &str) {}

impl Workbench {
    pub fn inbox_page(&self, cx: &Context<Self>) -> Div {
        let mut panel = col().size_full().min_h_0().gap(px(10.)).p(px(18.));
        let btn = |id: String, title: String, action: Action| {
            outlined(self.button(id, title.clone(), UiAction::Inbox(action), cx)).child(title)
        };
        let mut tabs = row().gap(px(5.)).flex_wrap();
        for (section, title) in [
            ("all", "全部"),
            ("important", "重要"),
            ("other", "其他"),
            ("snoozed", "稍后"),
            ("cleared", "已清理"),
        ] {
            let count = self.inbox.counts[section].as_u64().unwrap_or(0);
            let name = if count > 0 {
                format!("{title} {count}")
            } else {
                crate::locale::label(title).to_owned()
            };
            tabs = tabs.child(
                btn(
                    format!("inbox-tab-{section}"),
                    name,
                    Action::Section(section.into()),
                )
                .when(self.inbox.section == section, |b| b.bg(rgb(SELECTED))),
            );
        }
        let mut bar = row().w_full().flex_wrap().gap(px(6.)).child(btn(
            "inbox-filter".into(),
            crate::locale::label("筛选收件箱").into(),
            Action::Filter,
        ));
        if self.inbox_can_write() {
            bar = bar
                .child(btn(
                    "inbox-mark-all".into(),
                    crate::locale::label("全部标为已读").into(),
                    Action::ReadAll,
                ))
                .child(btn(
                    "inbox-clear-section".into(),
                    crate::locale::label("清理当前分区").into(),
                    Action::Clear,
                ));
        }
        bar = bar.child(btn(
            "inbox-settings".into(),
            crate::locale::label("通知设置").into(),
            Action::Settings(None),
        ));
        panel = panel.child(headline("收件箱", 22.)).child(tabs).child(bar);
        let mut list = col()
            .id("inbox-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(4.));
        for r in &self.inbox.rows {
            let read = r.fields.get("read") == Some(&Value::Bool(true));
            let important = r.field("importance") == "important"
                || r.fields.get("important") == Some(&Value::Bool(true));
            let key = format!("inbox-{}-{}", r.space, r.id);
            let primary = self
                .button(
                    format!("{key}-open"),
                    r.title.clone(),
                    UiAction::Inbox(Action::Open(r.clone())),
                    cx,
                )
                .flex_1()
                .min_w_0()
                .justify_start()
                .child(mark(if read {
                    DIM
                } else if important {
                    ERROR
                } else {
                    0x4278b9
                }))
                .child(text(r.title.clone(), 14., if read { MUTED } else { INK }));
            let mut line = row()
                .w_full()
                .gap(px(7.))
                .flex_wrap()
                .min_h(px(45.))
                .border_b_1()
                .border_color(rgb(LINE))
                .child(primary)
                .child(text(
                    format!(
                        "{} · {}",
                        self.name("space", &r.space),
                        crate::locale::label(event_label(r.field("event")))
                    ),
                    12.,
                    MUTED,
                ));
            if self.inbox_can_write() {
                let active = r.field("state") == "inbox";
                for (action, title) in [
                    (
                        if read { "unread" } else { "read" },
                        if read { "标记未读" } else { "标记已读" },
                    ),
                    (
                        if active { "snooze" } else { "restore" },
                        if active { "稍后" } else { "移回收件箱" },
                    ),
                    ("clear", "清理"),
                ] {
                    if action == "clear" && r.field("state") == "cleared" {
                        continue;
                    }
                    line = line.child(btn(
                        format!("{key}-{action}"),
                        crate::locale::label(title).into(),
                        Action::Change(r.clone(), action.into()),
                    ));
                }
            }
            list = list.child(line);
        }
        if self.inbox.rows.is_empty() {
            list = list.child(text("当前筛选没有通知。", 14., MUTED));
        }
        let offset = self
            .loc
            .filter
            .get("offset")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if offset > 0 {
            list = list.child(btn(
                "inbox-prev".into(),
                crate::locale::label("上一页").into(),
                Action::Page(offset - 500),
            ));
        }
        if offset as usize + self.inbox.rows.len() < self.inbox.total {
            list = list.child(btn(
                "inbox-next".into(),
                crate::locale::label("下一页").into(),
                Action::Page(offset + 500),
            ));
        }
        panel.child(list)
    }
    pub fn notification_settings(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut panel = col()
            .id("notice-settings")
            .size_full()
            .overflow_y_scroll()
            .p(px(23.))
            .gap(px(10.))
            .child(headline("通知设置", 22.));
        let btn = |id: String, title: String, action: Action| {
            outlined(self.button(id, title.clone(), UiAction::Inbox(action), cx)).child(title)
        };
        let cfg = &self.inbox.settings;
        if let Some(ch) = &self.inbox.channel {
            let overrides = &cfg["overrides"][ch];
            panel = panel.child(
                row()
                    .gap(px(8.))
                    .child(btn(
                        "notice-channels".into(),
                        crate::locale::label("返回通知渠道").into(),
                        Action::Settings(None),
                    ))
                    .child(headline(label(ch), 17.)),
            );
            if self.inbox_can_write() {
                panel = panel.child(btn(
                    format!("notice-reset-{ch}"),
                    crate::locale::label("恢复渠道默认设置").into(),
                    Action::Reset(ch.clone()),
                ));
            }
            let toggle = |key: &str, title: &str| {
                let on = overrides[key] != false;
                let text = format!(
                    "{} · {}",
                    crate::locale::label(title),
                    crate::locale::label(if on { "开启" } else { "关闭" })
                );
                btn(
                    format!("notice-{ch}-{key}"),
                    text,
                    Action::Toggle {
                        channel: ch.clone(),
                        key: key.into(),
                        enabled: !on,
                    },
                )
            };
            panel = panel.child(toggle("all", "渠道总开关"));
            let mut current = "";
            for pair in cfg["events"].as_array().into_iter().flatten() {
                let Some(group) = pair[1].as_str() else {
                    continue;
                };
                let Some(event) = pair[0].as_str() else {
                    continue;
                };
                if current != group {
                    panel = panel.child(toggle(group, label(group)));
                    current = group;
                }
                panel = panel.child(row().pl(px(18.)).child(toggle(event, event_label(event))));
            }
        } else {
            for id in cfg["channels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let status = if cfg["overrides"][id]["all"] == false {
                    "已关闭"
                } else {
                    match id {
                        "desktop" | "inbox" => "已启用",
                        "mobile" => "需要移动设备",
                        "browser" => "需要浏览器订阅",
                        "email" => "需要邮件投递服务",
                        _ => "需要外部通知服务",
                    }
                };
                panel = panel.child(
                    row()
                        .gap(px(12.))
                        .child(btn(
                            format!("notice-channel-{id}"),
                            crate::locale::label(label(id)).into(),
                            Action::Settings(Some(id.into())),
                        ))
                        .child(text(status, 12., MUTED)),
                );
            }
        }
        panel
    }
}
