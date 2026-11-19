use super::{After, UiAction, Workbench};
use crate::controls::{INK, MUTED, col, outlined, text};
use crate::forms::{Format, Mode, Spec, item};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};

#[derive(Clone)]
pub enum HookForm {
    Settings(Value),
    Issued(Vec<Value>),
    Secret(Value),
    Confirm(&'static str, Value),
    History(Value),
}
#[derive(Clone)]
pub enum Action {
    Settings(String, String),
    History(String, String, Option<i64>),
    Perform(&'static str, Value),
}
pub(super) enum Next {
    Settings,
    Issued,
    Secret,
    History,
    Saved,
}
impl HookForm {
    pub fn heading(&self) -> &'static str {
        match self {
            Self::Settings(_) => "Webhook 安全设置",
            Self::Issued(_) => "立即复制此 URL",
            Self::Secret(_) => "签名密钥",
            Self::History(_) => "Webhook 投递记录",
            Self::Confirm(op, _) => match *op {
                "automation.hook.rotate" => "重新生成 URL",
                "automation.hook.revoke" => "吊销 URL",
                _ => "重置签名密钥",
            },
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::Settings(_) => "保存安全设置",
            Self::Confirm(_, _) => "确认",
            _ => "关闭",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        if matches!(self, Self::Settings(_)) {
            vec![
                item("signing", "签名校验", Format::Bool, "false"),
                item("ip_filter", "IP 白名单", Format::Bool, "false"),
                item("allowlist", "每行一个地址或 CIDR", Format::Body, ""),
            ]
        } else {
            vec![]
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        let Self::Settings(v) = self else {
            return Map::new();
        };
        let mut m = v.as_object().cloned().unwrap_or_default();
        m.insert(
            "allowlist".into(),
            json!(
                v["allowlist"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        );
        m
    }
}
impl Workbench {
    pub(crate) fn webhook_content(&self, form: &HookForm, cx: &Context<Self>) -> AnyElement {
        let mut panel = col().gap(px(7.));
        let button = |id: String, name: &'static str, action: UiAction| {
            outlined(self.button(id, name, action, cx)).child(crate::locale::label(name))
        };
        match form {
            HookForm::Settings(s) => {
                let id = s["id"].as_str().unwrap_or("");
                let trigger = s["trigger_id"].as_str().unwrap_or("");
                if s["issued"] == true {
                    for (key, label, op) in [
                        ("replace", "重新生成 URL", "automation.hook.rotate"),
                        ("revoke", "吊销 URL", "automation.hook.revoke"),
                    ] {
                        panel = panel.child(button(
                            format!("hook-{key}"),
                            label,
                            UiAction::Form(Mode::Webhook(HookForm::Confirm(op, s.clone()))),
                        ));
                    }
                } else {
                    panel = panel.child(button(
                        "hook-issue".into(),
                        "生成私密 URL",
                        UiAction::Webhook(Action::Perform("automation.hook.issue", s.clone())),
                    ));
                }
                if s["has_secret"] == true {
                    panel = panel
                        .child(text("签名密钥　••••••••", 13., MUTED))
                        .child(button(
                            "hook-reveal".into(),
                            "查看签名密钥",
                            UiAction::Webhook(Action::Perform("automation.hook.secret", s.clone())),
                        ))
                        .child(button(
                            "hook-reset".into(),
                            "重置签名密钥",
                            UiAction::Form(Mode::Webhook(HookForm::Confirm(
                                "automation.hook.reset_secret",
                                s.clone(),
                            ))),
                        ));
                }
                if let Some(ip) = s["last_ip"].as_str() {
                    panel = panel.child(text(format!("最近来源 IP　{ip}"), 13., MUTED));
                }
                panel = panel.child(button(
                    "hook-history".into(),
                    "查看投递记录",
                    UiAction::Webhook(Action::History(id.into(), trigger.into(), None)),
                ));
            }
            HookForm::Issued(rows) => {
                for (i, v) in rows.iter().enumerate() {
                    if let Some(url) = v["url"].as_str() {
                        panel = panel.child(text(url.to_owned(), 13., INK)).child(button(
                            format!("hook-copy-{i}"),
                            "复制 URL",
                            UiAction::Copy(url.into()),
                        ));
                    }
                }
            }
            HookForm::Secret(v) => {
                if let Some(key) = v["secret"].as_str() {
                    panel = panel.child(text(key.to_owned(), 13., INK)).child(button(
                        "hook-copy-secret".into(),
                        "复制签名密钥",
                        UiAction::Copy(key.into()),
                    ));
                }
            }
            HookForm::Confirm(op, _) => {
                panel = panel.child(text(
                    if *op == "automation.hook.reset_secret" {
                        "确认后，调用方需立即改用新密钥签名。"
                    } else {
                        "确认后，旧 URL 会立即失效。"
                    },
                    14.,
                    INK,
                ));
            }
            HookForm::History(v) => {
                let items = v["items"].as_array().into_iter().flatten();
                if items.clone().next().is_none() {
                    panel = panel.child(text("暂无投递记录", 14., MUTED));
                }
                for entry in items {
                    let status = match entry["status"].as_str() {
                        Some("accepted") => "已接收",
                        Some("rejected") => "已拒绝",
                        _ => "已跳过",
                    };
                    let sign = match entry["signature_state"].as_str() {
                        Some("valid") => "签名有效",
                        Some("invalid") => "签名无效",
                        Some("missing") => "缺少签名",
                        Some("expired") => "时间戳过期",
                        Some("ip_not_allowed") => "IP 不允许",
                        Some("not_required") => "无需签名",
                        _ => "未校验",
                    };
                    let at = entry["at"]
                        .as_i64()
                        .and_then(chrono::DateTime::from_timestamp_millis)
                        .map(|d| {
                            d.with_timezone(&chrono::Local)
                                .format("%m-%d %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_default();
                    panel = panel.child(text(
                        format!(
                            "{at}　{status}　{}　{sign}　{}",
                            entry["source_ip"].as_str().unwrap_or(""),
                            entry["reason"].as_str().unwrap_or("")
                        ),
                        13.,
                        INK,
                    ));
                }
                if let Some(next) = v["next_before"].as_i64() {
                    panel = panel.child(button(
                        "hook-next".into(),
                        "较早的投递",
                        UiAction::Webhook(Action::History(
                            v["id"].as_str().unwrap_or("").into(),
                            v["trigger_id"].as_str().unwrap_or("").into(),
                            Some(next),
                        )),
                    ));
                }
            }
        }
        panel.into_any_element()
    }
    pub(super) fn webhook_action(&mut self, a: Action) {
        let (mut q, next, write) = match a {
            Action::Settings(id, t) => (
                json!({"op":"automation.hook.settings","id":id,"trigger_id":t}),
                Next::Settings,
                false,
            ),
            Action::History(id, t, before) => {
                let mut q =
                    json!({"op":"automation.hook.history","id":id,"trigger_id":t,"limit":20});
                if let Some(n) = before {
                    q["before"] = json!(n);
                }
                (q, Next::History, false)
            }
            Action::Perform(op, s) => (
                json!({"op":op,"id":s["id"],"trigger_id":s["trigger_id"],"expected_version":s["revision"]}),
                match op {
                    "automation.hook.issue" | "automation.hook.rotate" => Next::Issued,
                    "automation.hook.secret" | "automation.hook.reset_secret" => Next::Secret,
                    _ => Next::Settings,
                },
                true,
            ),
        };
        q["space"] = json!(self.loc.space);
        self.send(q, After::Webhook(next), write);
    }
    pub(super) fn webhook_submit(
        &mut self,
        form: HookForm,
        values: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match form {
            HookForm::Settings(s) => {
                let list = values["allowlist"]
                    .as_str()
                    .unwrap_or("")
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                self.send(json!({"op":"automation.hook.configure","space":self.loc.space,"id":s["id"],"trigger_id":s["trigger_id"],"expected_version":s["revision"],"signing":values["signing"],"ip_filter":values["ip_filter"],"allowlist":list}),After::Webhook(Next::Settings),true);
            }
            HookForm::Confirm(op, s) => self.webhook_action(Action::Perform(op, s)),
            _ => self.close_form(w, cx),
        }
    }
    pub(super) fn webhook_reply(
        &mut self,
        next: Next,
        v: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = v.get("settings")
            && let Some(current) = self.form.as_mut()
            && let Mode::Webhook(HookForm::Settings(old)) = &mut current.mode
        {
            *old = settings.clone();
        }
        if let Some(settings) = v.get("settings")
            && let Some(parent) = self.hook_parents.last_mut()
            && let Mode::Webhook(HookForm::Settings(old)) = &mut parent.mode
        {
            *old = settings.clone();
        }
        if matches!(next, Next::Settings)
            && self
                .form
                .as_ref()
                .is_some_and(|f| matches!(f.mode, Mode::Webhook(HookForm::Confirm(..))))
            && self
                .hook_parents
                .last()
                .is_some_and(|f| matches!(f.mode, Mode::Webhook(HookForm::Settings(_))))
        {
            self.close_form(w, cx);
            if let Some(parent) = &mut self.form {
                parent.mode = Mode::Webhook(HookForm::Settings(v));
            }
            return;
        }
        let f = match next {
            Next::Saved => {
                self.reply(
                    After::Saved("automation".into()),
                    v["record"].clone(),
                    w,
                    cx,
                );
                let rows = v["issued"].as_array().cloned().unwrap_or_default();
                if rows.is_empty() {
                    return;
                }
                HookForm::Issued(rows)
            }
            Next::Issued => HookForm::Issued(vec![v]),
            Next::Secret => HookForm::Secret(v),
            Next::History => HookForm::History(v),
            Next::Settings => HookForm::Settings(v),
        };
        self.open_form(Mode::Webhook(f), w, cx);
    }
}
