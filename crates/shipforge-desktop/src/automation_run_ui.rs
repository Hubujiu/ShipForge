use super::{After, UiAction, Workbench};
use crate::{catalog::Record, controls::*, forms::Mode, locale};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Pages {
    space: String,
    rule: String,
    before: Vec<String>,
}
#[derive(Clone)]
pub enum Action {
    Next(String, String),
    Back(String),
    Open(String, String),
}

fn label(value: &str) -> &str {
    match value {
        "enabled" => "启用",
        "paused" => "已暂停",
        "archived" => "已归档",
        "no_runs" => "尚无运行",
        "healthy" => "健康",
        "attention" => "需要关注",
        "pending" => "待执行",
        "running" => "运行中",
        "done" => "已完成",
        "failed" => "失败",
        "cancelled" => "已取消",
        "skipped" => "已跳过",
        "interrupted" => "已中断",
        "waiting_children" => "等待小队",
        "waiting" => "等待回复",
        "cancelling" => "正在取消",
        "queued" => "队列中",
        "manual" => "手动",
        "schedule" => "时间表",
        "status" => "任务状态",
        "comment" => "评论",
        "webhook" => "Webhook",
        _ => value,
    }
}
impl Workbench {
    pub(super) fn rule_cursor(&self) -> Option<String> {
        let r = self.loc.selected.as_ref()?;
        (r.kind == "automation"
            && self.rule_pages.space == self.loc.space
            && self.rule_pages.rule == r.id)
            .then(|| self.rule_pages.before.last().cloned())
            .flatten()
    }
    pub(super) fn rule_history_action(&mut self, action: Action) {
        let Some(current) = self
            .loc
            .selected
            .as_ref()
            .filter(|r| r.kind == "automation")
        else {
            return;
        };
        match action {
            Action::Next(rule, id) if rule == current.id => {
                if self.rule_pages.rule != rule || self.rule_pages.space != self.loc.space {
                    self.rule_pages = Pages {
                        space: self.loc.space.clone(),
                        rule,
                        before: Vec::new(),
                    };
                }
                self.rule_pages.before.push(id);
                self.read(false);
            }
            Action::Back(rule) if rule == current.id && self.rule_cursor().is_some() => {
                self.rule_pages.before.pop();
                self.read(false);
            }
            Action::Open(rule, run) if rule == current.id => {
                self.send(json!({"op":"automation.conversation","id":rule,"run":run,"space":self.loc.space}),After::OpenChat,true);
            }
            _ => {}
        }
    }
    pub(crate) fn rule_history_panel(&self, rule: &Record, cx: &Context<Self>) -> AnyElement {
        let rule_state = self.find("automation_status", &rule.id);
        let page = self.find("automation_history", &rule.id);
        let mut panel = col().gap(px(8.)).child(headline("运行历史", 15.));
        let btn = |id: String, name: &'static str, a: UiAction| {
            outlined(self.button(id, locale::label(name), a, cx)).child(locale::label(name))
        };
        if let Some(record) = rule_state {
            let state = record.field("state");
            let mut summary = row()
                .gap(px(8.))
                .flex_wrap()
                .child(chip(locale::label(label(state)).to_owned(), PALE, MUTED))
                .child(text(
                    locale::label(label(record.field("health"))).to_owned(),
                    13.,
                    INK,
                ));
            if self.writable() {
                for (action, title) in [
                    ("pause", "暂停规则"),
                    ("resume", "恢复规则"),
                    ("archive", "归档规则"),
                ] {
                    if !matches!(
                        (action, state),
                        ("pause", "enabled")
                            | ("resume", "paused")
                            | ("archive", "enabled" | "paused")
                    ) {
                        continue;
                    }
                    summary=summary.child(btn(format!("rule-{action}"),title,UiAction::Form(Mode::Confirm{label:locale::label(title).into(),command:json!({"op":"automation.lifecycle","id":rule.id,"space":rule.space,"action":action,"expected_rev":rule.rev})})));
                }
            }
            panel = panel.child(summary);
        }
        if let Some(err) = self
            .errors
            .get("automation_history")
            .or_else(|| self.errors.get("automation_status"))
        {
            panel = panel.child(text(err.clone(), 13., ERROR));
        }
        let Some(data) = page else {
            return panel.into_any_element();
        };
        let items = data.fields.get("items").and_then(Value::as_array);
        for item in items.into_iter().flatten() {
            let id = item["id"].as_str().unwrap_or("");
            let who = item["owner"].as_str().unwrap_or("");
            let member = |id: &str| {
                self.find("member", id)
                    .map(|r| r.title.as_str())
                    .unwrap_or(id)
                    .to_owned()
            };
            let mut item_row = row()
                .gap(px(7.))
                .flex_wrap()
                .child(text(
                    locale::label(label(item["status"].as_str().unwrap_or(""))).to_owned(),
                    13.,
                    INK,
                ))
                .child(text(
                    locale::label(label(item["source"].as_str().unwrap_or(""))).to_owned(),
                    12.,
                    MUTED,
                ))
                .child(text(
                    format!("{} {}", locale::label("负责人"), member(who)),
                    12.,
                    MUTED,
                ));
            if let Some(a) = item["trigger_actor"].as_str() {
                item_row = item_row.child(text(
                    format!("{} {}", locale::label("触发者"), member(a)),
                    12.,
                    MUTED,
                ));
            }
            if let Some(t) = item["created_at"].as_i64() {
                let stamp = chrono::DateTime::from_timestamp_millis(t)
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_default();
                item_row = item_row.child(text(stamp, 12., MUTED));
            }
            if self.writable() && item["can_open"] == true {
                item_row = item_row.child(btn(
                    format!("open-run-chat-{id}"),
                    "打开运行对话",
                    UiAction::RuleHistory(Action::Open(rule.id.clone(), id.into())),
                ));
            }
            if self.writable() && item["can_retry"] == true {
                item_row=item_row.child(btn(format!("retry-rule-{id}"),"重试此运行",UiAction::Form(Mode::Confirm{label:locale::label("重试此运行").into(),command:json!({"op":"automation.retry","space":rule.space,"id":rule.id,"run":id,"request_id":uuid::Uuid::new_v4().to_string()})})));
            }
            panel = panel.child(item_row);
        }
        let mut nav = row().gap(px(8.));
        if self.rule_cursor().is_some() {
            nav = nav.child(btn(
                "rule-prev".into(),
                "上一页",
                UiAction::RuleHistory(Action::Back(rule.id.clone())),
            ));
        }
        if let Some(next) = data.fields.get("next_cursor").and_then(Value::as_str) {
            nav = nav.child(btn(
                "rule-next".into(),
                "下一页",
                UiAction::RuleHistory(Action::Next(rule.id.clone(), next.to_owned())),
            ));
        }
        panel.child(nav).into_any_element()
    }
}
