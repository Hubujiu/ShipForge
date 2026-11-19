use super::{Location, Request, UiAction, Workbench};
use crate::{catalog::Page, controls::*, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};

#[cfg(all(test, target_os = "windows"))]
#[path = "account_ui_test.rs"]
mod native_test;

impl Workbench {
    pub(super) fn restore_session(&mut self, session: &Value) {
        if self.session_started {
            self.pending.clear();
            self.busy = false;
            self.reading = false;
            self.rows.clear();
            self.errors.clear();
            self.form = None;
            self.role.clear();
            self.actor.clear();
            self.account_rows.clear();
            self.drafts.clear();
            self.chat_undo = None;
            self.notice = None;
            self.reset_detail();
            self.metrics = Value::Null;
            self.view_tasks.clear();
            self.work = Default::default();
            self.inbox = Default::default();
        }
        self.session_started = true;
        self.active_profile = session["id"].as_str().map(str::to_owned);
        let route = &session["location"];
        self.loc = Location {
            space: route["space"].as_str().unwrap_or("default").into(),
            page: serde_json::from_value(route["page"].clone()).unwrap_or(Page::Tasks),
            view: serde_json::from_value(route["view"].clone()).ok(),
            selected: serde_json::from_value(route["selected"].clone()).ok(),
            ..Location::default()
        };
        if session["needs_login"] == true {
            self.loc.page = Page::Settings;
            self.loc.section = "accounts".into();
        }
        self.saved_place = Value::Null;
        self.history = vec![self.loc.clone()];
        self.pos = 0;
    }
    pub(super) fn remember_place(&mut self) {
        let Some(id) = &self.active_profile else {
            return;
        };
        if self.loc.section == "accounts" || self.mode != "team" {
            return;
        }
        let place = json!({"space":self.loc.space,"page":self.loc.page,"kind":self.loc.selected.as_ref().map(|r|&r.kind),
            "id":self.loc.selected.as_ref().map(|r|&r.id),"view":self.loc.view.as_ref().map(|r|&r.id)});
        if self.saved_place != place
            && self
                .backend
                .sender
                .send(Request::RememberAccount(
                    json!({"op":"profiles.remember","id":id,"location":place}),
                ))
                .is_ok()
        {
            self.saved_place = place;
        }
    }
    pub(crate) fn account_panel(&self, cx: &Context<Self>) -> Div {
        let mut panel = col().w_full().gap(px(8.)).child(
            row()
                .w_full()
                .child(headline("账号列表", 15.))
                .child(div().flex_1())
                .child(
                    outlined(self.button("add-account", "添加账号", UiAction::Connect, cx))
                        .child(crate::locale::label("添加账号")),
                ),
        );
        panel = panel.child(
            outlined(self.button(
                "mail-sign-in",
                "使用邮箱登录或注册",
                UiAction::Form(Mode::Identity(super::identity_ui::IdentityForm::Login(
                    None,
                ))),
                cx,
            ))
            .child(crate::locale::label("使用邮箱登录或注册")),
        );
        if self.active_profile.is_some()
            && self.access_identity["email_account"] == true
            && self.access_identity["personal"] == false
        {
            panel = panel.child(
                row()
                    .gap(px(8.))
                    .child(
                        outlined(self.button(
                            "identity-profile",
                            "个人资料",
                            UiAction::IdentityProfile,
                            cx,
                        ))
                        .child(crate::locale::label("个人资料")),
                    )
                    .child(
                        outlined(self.button(
                            "identity-logout",
                            "退出当前账号",
                            UiAction::Form(Mode::Confirm {
                                label: "退出当前账号".into(),
                                command: json!({"op":"profiles.logout"}),
                            }),
                            cx,
                        ))
                        .child(crate::locale::label("退出当前账号")),
                    )
                    .child(
                        self.button(
                            "identity-close",
                            "永久注销账号",
                            UiAction::IdentityClose,
                            cx,
                        )
                        .child(crate::locale::label("永久注销账号")),
                    ),
            );
        }
        if self.account_rows.is_empty() {
            return panel.child(text("本机尚未保存团队账号。", 13., MUTED));
        }
        for account in &self.account_rows {
            let Some(id) = account["id"].as_str() else {
                continue;
            };
            let endpoint = account["url"].as_str().unwrap_or_default();
            let actor = if account["active"] == true {
                self.access_identity["display"].as_str()
            } else {
                None
            }
            .or_else(|| account["display"].as_str())
            .or_else(|| account["actor"].as_str())
            .unwrap_or_else(|| crate::locale::label("已保存的账号"));
            let mut line = row().w_full().gap(px(8.)).child(
                col()
                    .flex_1()
                    .child(text(actor.to_owned(), 13., INK))
                    .child(text(endpoint.to_owned(), 12., MUTED)),
            );
            if account["active"] == true {
                line = line.child(text("当前使用", 12., GREEN));
            } else if account["needs_login"] == true {
                let reauth = if account["email_account"] == true {
                    Mode::Identity(super::identity_ui::IdentityForm::Login(Some((
                        id.into(),
                        endpoint.into(),
                    ))))
                } else {
                    Mode::Reauthenticate {
                        account: id.into(),
                        url: endpoint.into(),
                    }
                };
                line = line.child(
                    outlined(self.button(
                        format!("login-{id}"),
                        "重新登录账号",
                        UiAction::Form(reauth),
                        cx,
                    ))
                    .child(crate::locale::label("重新登录账号")),
                );
            } else {
                line = line.child(
                    outlined(self.button(
                        format!("switch-{id}"),
                        "切换账号",
                        UiAction::SwitchAccount(id.into()),
                        cx,
                    ))
                    .child(crate::locale::label("切换账号")),
                );
            }
            panel = panel.child(
                line.child(
                    self.button(
                        format!("forget-{id}"),
                        "从本机移除账号",
                        UiAction::Form(Mode::Confirm {
                            label: format!(
                                "{}「{}」",
                                crate::locale::label("从本机移除账号"),
                                actor
                            ),
                            command: json!({"op":"profiles.forget","id":id}),
                        }),
                        cx,
                    )
                    .child(crate::locale::label("移除")),
                ),
            );
        }
        panel
    }
}
