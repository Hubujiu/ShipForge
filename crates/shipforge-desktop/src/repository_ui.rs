use super::{After, UiAction, Workbench};
use crate::{
    controls::{INK, MUTED, col, outlined, row, text},
    forms::{Format, Mode, Spec, item},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};

#[derive(Clone)]
pub enum Form {
    Status(&'static str, Value),
    Consent(&'static str, &'static str, Value),
    Pat(&'static str),
    Repos(&'static str, Value),
    Pair(&'static str, Value),
    Receipt(Value),
    Disconnect(&'static str, &'static str),
    Thread(&'static str, String, Value),
    Reply(&'static str, String, String, String),
    Confirm(&'static str, &'static str, Value),
}
#[derive(Clone)]
pub enum Action {
    Status(&'static str),
    Begin(&'static str, &'static str),
    Repos(&'static str),
    Thread(&'static str, String),
    Run(&'static str, &'static str, Value),
}
pub(super) enum Next {
    Status(&'static str),
    Consent(&'static str, &'static str),
    Repos(&'static str),
    Receipt,
    Thread(&'static str, String),
    Reload(&'static str),
}
const PLATFORMS: [(&str, &str); 3] = [
    ("github", "GitHub"),
    ("gitlab", "GitLab.com"),
    ("gitlab-self", "自建 GitLab"),
];
fn title(p: &str) -> &'static str {
    match p {
        "github" => "GitHub",
        "gitlab" => "GitLab.com",
        _ => "自建 GitLab",
    }
}
fn strv<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
impl Form {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Status(p, _) => title(p),
            Self::Consent(..) => "浏览器授权",
            Self::Pat(_) => "使用自建 GitLab 令牌",
            Self::Repos(..) => "选择代码仓库",
            Self::Pair(..) => "仓库与空间配对",
            Self::Receipt(_) => "仓库操作结果",
            Self::Disconnect(..) => "解除授权",
            Self::Thread(..) => "来源 Issue",
            Self::Reply(..) => "回复到来源 Issue",
            Self::Confirm(_, op, _) => {
                if *op == "forge.unlink" {
                    "解除任务配对"
                } else {
                    "推送任务更改"
                }
            }
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::Consent(..) => "完成授权",
            Self::Pat(_) => "验证并连接",
            Self::Pair(..) => "保存配对",
            Self::Disconnect(..) => "解除授权",
            Self::Reply(..) => "发送到来源",
            Self::Confirm(..) => "确认",
            _ => "关闭",
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        if let Self::Pair(_, r) = self {
            let mut m = r.as_object().cloned().unwrap_or_default();
            m.entry("backlinks").or_insert(json!(r["public"] == false));
            m
        } else {
            Map::new()
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        match self {
            Self::Consent(p, pur, _) => {
                let mut f = vec![item(
                    "callback",
                    "浏览器授权后完整回调网址",
                    Format::Secret,
                    "",
                )];
                if *p == "github" && *pur == "organization" {
                    f.push(item("installation", "GitHub 安装 ID", Format::Text, ""));
                }
                f
            }
            Self::Pat(pur) => {
                let mut f = Vec::new();
                if *pur == "organization" {
                    f.push(item(
                        "instance",
                        "GitLab 实例的 HTTPS 地址",
                        Format::Text,
                        "",
                    ));
                }
                f.push(item(
                    "token",
                    "具有 api scope 的个人访问令牌",
                    Format::Secret,
                    "",
                ));
                f
            }
            Self::Pair(_, r) => {
                let choices = if r["direction"].is_string() {
                    &[
                        ("incoming", "仅从仓库接收"),
                        ("both", "双向"),
                        ("off", "解除配对"),
                    ][..]
                } else {
                    &[("incoming", "仅从仓库接收"), ("both", "双向")][..]
                };
                vec![
                    item("direction", "同步方向", Format::Choice(choices), "incoming"),
                    item("backlinks", "在来源写入任务链接", Format::Bool, "false"),
                    item("pull_requests", "关联拉取或合并请求", Format::Bool, "true"),
                    item("commits", "关联提交", Format::Bool, "true"),
                    item(
                        "confirm_public",
                        "允许向公开仓库发送当前空间的内容",
                        Format::Bool,
                        "false",
                    ),
                ]
            }
            Self::Reply(..) => vec![item("body", "发送到来源的正文", Format::Body, "")],
            _ => vec![],
        }
    }
}
impl Workbench {
    pub(crate) fn repository_toolbar(&self, task: Option<&str>, cx: &Context<Self>) -> AnyElement {
        let mut bar = row().gap(px(6.)).flex_wrap();
        for (p, name) in PLATFORMS {
            let act = task.map_or(Action::Status(p), |t| Action::Thread(p, t.into()));
            let label = if task.is_some() {
                format!("{name} 来源")
            } else {
                name.into()
            };
            bar = bar.child(
                outlined(self.button(
                    format!("repository-{p}"),
                    label.clone(),
                    UiAction::Forge(act),
                    cx,
                ))
                .child(label),
            );
        }
        bar.into_any_element()
    }
    pub(crate) fn repository_content(&self, f: &Form, cx: &Context<Self>) -> AnyElement {
        let mut panel = col().gap(px(8.));
        let button = |id: String, label: &'static str, a: UiAction| {
            outlined(self.button(id, label, a, cx)).child(label)
        };
        let form = |f| UiAction::Form(Mode::Forge(f));
        let act = UiAction::Forge;
        match f {
            Form::Status(p, data) => {
                for (pur, label, key) in [
                    ("organization", "组织授权", "account"),
                    ("personal", "个人授权", "personal"),
                ] {
                    if pur == "organization" && data["can_admin"] != true {
                        continue;
                    }
                    if pur == "personal" && data["account"].is_null() {
                        continue;
                    }
                    let connected = data[key].is_object();
                    let status = if connected {
                        strv(&data[key], "login")
                    } else {
                        "未连接"
                    };
                    panel = panel
                        .child(text(format!("{label} · {status}"), 14., INK))
                        .child(button(
                            format!("forge-{pur}"),
                            if connected {
                                "更新授权"
                            } else {
                                "连接账号"
                            },
                            if *p == "gitlab-self" {
                                form(Form::Pat(pur))
                            } else {
                                act(Action::Begin(p, pur))
                            },
                        ));
                    if let Some(exp) = data[key]["expires"].as_str() {
                        let label = if exp
                            .get(..10)
                            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                            .is_some_and(|d| {
                                d <= chrono::Utc::now().date_naive() + chrono::Duration::days(14)
                            }) {
                            "请更新即将或已经到期的令牌"
                        } else {
                            "凭据有效期"
                        };
                        panel = panel.child(text(format!("{label}　{exp}"), 13., MUTED));
                    }
                    if connected {
                        panel = panel.child(button(
                            format!("unlink-{pur}"),
                            "解除授权",
                            form(Form::Disconnect(p, pur)),
                        ));
                    }
                }
                if data["account"].is_object() && data["can_pair"] == true {
                    panel = panel.child(button(
                        "list-forge-repos".into(),
                        "选择代码仓库",
                        act(Action::Repos(p)),
                    ));
                }
                for b in data["pairs"].as_array().into_iter().flatten() {
                    panel = panel.child(text(strv(b, "repository").to_owned(), 14., INK));
                    if data["can_pair"] == true {
                        panel = panel
                            .child(button(
                                format!("edit-pair-{}", strv(b, "id")),
                                "配对设置",
                                form(Form::Pair(p, b.clone())),
                            ))
                            .child(button(
                                format!("deliver-{}", strv(b, "id")),
                                "重试待发链接",
                                act(Action::Run(p, "forge.deliver", json!({"pair":b["id"]}))),
                            ));
                    }
                }
            }
            Form::Consent(p, pur, v) => {
                if *p == "gitlab" && *pur == "personal" {
                    panel = panel.child(text(
                        "连接此身份会替换本人在另一个 GitLab 实例的绑定。",
                        13.,
                        MUTED,
                    ));
                }
                for (k, label) in [
                    ("installation_url", "安装或选择 GitHub App 仓库"),
                    ("authorize_url", "打开账号授权页"),
                ] {
                    if let Some(u) = v[k].as_str() {
                        panel =
                            panel.child(button(k.into(), label, UiAction::OpenSourceUrl(u.into())));
                    }
                }
                panel = panel.child(text(
                    "浏览器返回回调地址后，复制完整网址并在下方提交。",
                    13.,
                    MUTED,
                ));
            }
            Form::Repos(p, d) => {
                for r in d.as_array().into_iter().flatten() {
                    panel = panel
                        .child(text(strv(r, "repository").to_owned(), 14., INK))
                        .child(button(
                            format!("pick-repo-{}", strv(r, "repository_id")),
                            "配对到此空间",
                            form(Form::Pair(p, r.clone())),
                        ));
                }
                if d.as_array().is_none_or(Vec::is_empty) {
                    panel = panel.child(text("当前授权没有可配对的仓库。", 13., MUTED));
                }
            }
            Form::Pat("personal") => {
                panel = panel.child(text(
                    "连接此身份会替换本人在另一个 GitLab 实例的绑定。",
                    13.,
                    MUTED,
                ));
            }
            Form::Pair(_, b) => {
                panel = panel.child(text(strv(b, "repository").to_owned(), 14., INK));
            }
            Form::Receipt(v) => {
                if let Some(path) = v["webhook_path"].as_str() {
                    panel = panel
                        .child(text(format!("Webhook 路径　{path}"), 13., INK))
                        .child(button(
                            "copy-hook-path".into(),
                            "复制路径",
                            UiAction::Copy(path.into()),
                        ));
                }
                if let Some(secret) = v["webhook_token"].as_str() {
                    panel = panel
                        .child(text(
                            "请在 GitLab 项目的 Webhooks 中启用 Issue、评论、推送与合并事件。",
                            13.,
                            MUTED,
                        ))
                        .child(button(
                            "copy-hook-token".into(),
                            "复制项目 Webhook 密钥",
                            UiAction::Copy(secret.into()),
                        ));
                }
                if v["state"] == "sent" {
                    panel = panel.child(text(
                        if v["fallback"] == true {
                            "已使用组织身份发送"
                        } else {
                            "已发送"
                        },
                        14.,
                        INK,
                    ));
                } else if let Some(s) = v["action"].as_str() {
                    panel = panel.child(text(s.to_owned(), 14., INK));
                } else if v["webhook_path"].is_null() {
                    panel = panel.child(text("操作已完成", 14., INK));
                }
            }
            Form::Thread(p, task, links) => {
                if links.as_array().is_none_or(Vec::is_empty) {
                    panel = panel.child(text("任务没有此平台的来源 Issue。", 13., MUTED));
                }
                for b in links.as_array().into_iter().flatten() {
                    let pair = strv(b, "pair");
                    panel = panel
                        .child(button(
                            format!("source-{pair}"),
                            "打开来源 Issue",
                            UiAction::OpenSourceUrl(strv(b, "url").into()),
                        ))
                        .child(text(
                            format!("{} #{}", strv(b, "repository"), b["number"]),
                            14.,
                            INK,
                        ));
                    if let Some(author) = b["author"]["login"].as_str() {
                        panel = panel.child(text(format!("来源创建者　{author}"), 13., MUTED));
                    }
                    for n in b["comments"].as_array().into_iter().flatten() {
                        panel = panel.child(text(
                            format!("{}　{}", strv(n, "login"), strv(n, "body")),
                            13.,
                            INK,
                        ));
                    }
                    if b["active"] == true {
                        let args = json!({"pair":pair,"task_id":task});
                        panel = panel
                            .child(button(
                                format!("refresh-{pair}"),
                                "刷新来源",
                                act(Action::Run(p, "forge.refresh", args.clone())),
                            ))
                            .child(button(
                                format!("forget-{pair}"),
                                "解除任务配对",
                                form(Form::Confirm(p, "forge.unlink", args.clone())),
                            ));
                        if b["can_reply"] == true {
                            panel = panel
                                .child(button(
                                    format!("send-{pair}"),
                                    "回复到来源",
                                    form(Form::Reply(
                                        p,
                                        task.clone(),
                                        pair.into(),
                                        uuid::Uuid::new_v4().to_string(),
                                    )),
                                ))
                                .child(button(
                                    format!("push-{pair}"),
                                    "推送任务更改",
                                    form(Form::Confirm(p, "forge.push", args)),
                                ));
                        }
                    } else {
                        panel = panel.child(text("已解除配对", 13., MUTED));
                    }
                }
            }
            Form::Disconnect(_, pur) => {
                panel = panel.child(text(
                    if *pur == "organization" {
                        "解除组织授权后，此平台的配对停止同步。"
                    } else {
                        "后续发往来源的回复将改用组织身份。"
                    },
                    13.,
                    MUTED,
                ))
            }
            Form::Confirm(_, op, _) => {
                panel = panel.child(text(
                    if *op == "forge.push" {
                        "将此任务的标题、描述和开闭状态发送到配对的 Issue。"
                    } else {
                        "保留当前任务与来源记录，停止同步此 Issue。"
                    },
                    13.,
                    MUTED,
                ))
            }
            _ => {}
        }
        panel.into_any_element()
    }
    pub(super) fn repository_action(&mut self, a: Action) {
        let (p, op, mut q, next) = match a {
            Action::Status(p) => (p, "forge.status", json!({}), Next::Status(p)),
            Action::Begin(p, pur) => (
                p,
                "forge.authorize",
                json!({"purpose":pur}),
                Next::Consent(p, pur),
            ),
            Action::Repos(p) => (p, "forge.repositories", json!({}), Next::Repos(p)),
            Action::Thread(p, t) => (p, "forge.thread", json!({"task_id":t}), Next::Thread(p, t)),
            Action::Run(p, op, args) => (p, op, args, Next::Receipt),
        };
        q["op"] = json!(op);
        q["space"] = json!(self.loc.space);
        q["id"] = json!(p);
        self.send(q, After::Forge(next), false);
    }
    pub(super) fn repository_submit(
        &mut self,
        f: Form,
        mut q: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (p, op, next) = match f {
            Form::Consent(p, pur, _) => {
                q["purpose"] = json!(pur);
                (p, "forge.authorize", Next::Reload(p))
            }
            Form::Pat(pur) => {
                q["purpose"] = json!(pur);
                (
                    "gitlab-self",
                    "forge.credential",
                    Next::Reload("gitlab-self"),
                )
            }
            Form::Pair(p, b) => {
                q["repository_id"] = b["repository_id"].clone();
                q["pair"] = b["id"].clone();
                (p, "forge.pair", Next::Receipt)
            }
            Form::Disconnect(p, pur) => {
                q["purpose"] = json!(pur);
                (p, "forge.disconnect", Next::Reload(p))
            }
            Form::Reply(p, t, b, key) => {
                q["task_id"] = json!(t);
                q["pair"] = json!(b);
                q["request_id"] = json!(key);
                (p, "forge.reply", Next::Receipt)
            }
            Form::Confirm(p, op, args) => {
                q = args;
                (p, op, Next::Receipt)
            }
            _ => {
                self.close_form(w, cx);
                return;
            }
        };
        q["id"] = json!(p);
        q["op"] = json!(op);
        q["space"] = json!(self.loc.space);
        self.send(q, After::Forge(next), true);
    }
    pub(super) fn repository_result(
        &mut self,
        next: Next,
        data: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let f = match next {
            Next::Status(p) => Form::Status(p, data),
            Next::Consent(p, pur) => Form::Consent(p, pur, data),
            Next::Repos(p) => Form::Repos(p, data),
            Next::Thread(p, t) => Form::Thread(p, t, data),
            Next::Receipt => Form::Receipt(data),
            Next::Reload(p) => {
                self.repository_action(Action::Status(p));
                return;
            }
        };
        self.open_form(Mode::Forge(f), w, cx);
    }
}
