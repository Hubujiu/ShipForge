use super::{After, Workbench};
use crate::forms::{Format, Mode, Spec, item};
use gpui_kit::*;
use serde_json::{Map, Value, json};

#[cfg(all(test, target_os = "windows"))]
#[path = "identity_ui_test.rs"]
mod native_test;

#[derive(Clone)]
pub enum IdentityForm {
    Login(Option<(String, String)>),
    Verify {
        ticket: String,
        pair: String,
        closing: bool,
        dissolve: bool,
    },
    Profile(Value),
    Impact(Value),
}
pub(super) enum Reply {
    LoginSent,
    CloseSent(bool),
    Resume,
    Profile,
    Impact,
}
impl IdentityForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Login(_) => "使用邮箱登录或注册",
            Self::Verify { .. } => "验证邮箱",
            Self::Profile(_) => "个人资料",
            Self::Impact(_) => "永久注销账号",
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::Login(_) | Self::Impact(_) => "获取验证邮件",
            Self::Verify { closing: true, .. } => "永久注销账号",
            Self::Verify { .. } => "完成登录",
            Self::Profile(_) => "保存",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        match self {
            Self::Login(_) => vec![
                item("url", "团队服务地址", Format::Text, ""),
                item("email", "邮箱", Format::Text, ""),
            ],
            Self::Profile(_) => vec![
                item("name", "姓名", Format::Text, ""),
                item("avatar", "头像地址", Format::Text, ""),
                item("bio", "个人简介", Format::Body, ""),
            ],
            Self::Impact(v) if requires_dissolution(v) => vec![item(
                "dissolve",
                "一并解散仅由我拥有的多人组织",
                Format::Bool,
                "false",
            )],
            Self::Verify { closing: true, .. } => {
                vec![item("confirmation", "输入「注销」确认", Format::Text, "")]
            }
            _ => vec![],
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        match self {
            Self::Profile(v) => v.as_object().cloned().unwrap_or_default(),
            Self::Login(Some((_, url))) => Map::from_iter([("url".into(), json!(url))]),
            _ => Map::new(),
        }
    }
    pub fn instructions(&self) -> Option<String> {
        match self {
            Self::Verify { pair, .. } => Some(format!(
                "{}\n{} · {pair}",
                crate::locale::label(
                    "打开验证邮件，在浏览器输入设备配对码，然后返回此处完成操作。"
                ),
                crate::locale::label("设备配对码")
            )),
            Self::Profile(v) => Some(format!(
                "{} · {}",
                crate::locale::label("已验证邮箱"),
                v["email"].as_str()?
            )),
            Self::Impact(v) => {
                let mut lines = vec![
                    crate::locale::label("注销将清除个人资料并退出所有组织，无法恢复。").to_owned(),
                ];
                for org in v["organizations"].as_array()? {
                    lines.push(format!(
                        "{} · {}",
                        org["name"].as_str()?,
                        crate::locale::label(match org["action"].as_str() {
                            Some("dissolve_solo") => "解散单人组织",
                            Some("dissolve_with_members") => "解散多人组织",
                            _ => "退出组织",
                        })
                    ));
                }
                for m in v["shared_machines"].as_array()? {
                    lines.push(format!(
                        "{} · {}",
                        crate::locale::label("请先处理共享机器权限"),
                        m["name"].as_str()?
                    ));
                }
                for space in v["spaces_to_transfer"].as_array().into_iter().flatten() {
                    lines.push(format!(
                        "{} · {}",
                        crate::locale::label("请先转让空间管理权"),
                        space["name"].as_str()?
                    ));
                }
                Some(lines.join("\n"))
            }
            _ => None,
        }
    }
}
fn requires_dissolution(v: &Value) -> bool {
    v["organizations"]
        .as_array()
        .is_some_and(|orgs| orgs.iter().any(|o| o["action"] == "dissolve_with_members"))
}
impl Workbench {
    pub(crate) fn open_identity_link(&mut self, id: uuid::Uuid) {
        if self.session_started && !self.actor.is_empty() {
            self.send(
                json!({"op":"profiles.verification","id":id.to_string()}),
                After::Identity(Reply::Resume),
                false,
            );
        } else {
            self.identity_link = Some(id.to_string());
        }
    }
    pub(super) fn identity_reply(
        &mut self,
        reply: Reply,
        value: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let form = match reply {
            Reply::Profile => IdentityForm::Profile(value),
            Reply::Impact => IdentityForm::Impact(value),
            Reply::Resume => IdentityForm::Verify {
                ticket: value["id"].as_str().unwrap_or("").into(),
                pair: value["pair"].as_str().unwrap_or("").into(),
                closing: value["closing"] == true,
                dissolve: value["dissolve"] == true,
            },
            Reply::LoginSent | Reply::CloseSent(_) => IdentityForm::Verify {
                ticket: value["id"].as_str().unwrap_or("").into(),
                pair: value["pair"].as_str().unwrap_or("").into(),
                closing: matches!(reply, Reply::CloseSent(_)),
                dissolve: matches!(reply, Reply::CloseSent(true)),
            },
        };
        self.open_form(Mode::Identity(form), w, cx);
    }
    pub(super) fn identity_submit(&mut self, form: IdentityForm, v: Value) -> Result<(), String> {
        let (q, after) = match form {
            IdentityForm::Login(expected) => (
                json!({"op":"profiles.login.start","url":v["url"],"email":v["email"],"account":expected.map(|(id,_)|id)}),
                After::Identity(Reply::LoginSent),
            ),
            IdentityForm::Profile(profile) => (
                json!({"op":"access.account.update","name":v["name"],"avatar":v["avatar"].as_str().unwrap_or(""),"bio":v["bio"].as_str().unwrap_or(""),"expected_rev":profile["rev"]}),
                After::Notice("个人资料已保存".into()),
            ),
            IdentityForm::Impact(impact) => {
                if impact["spaces_to_transfer"]
                    .as_array()
                    .is_some_and(|spaces| !spaces.is_empty())
                {
                    return Err("请先转让空间管理权".into());
                }
                if impact["shared_machines"]
                    .as_array()
                    .is_some_and(|m| !m.is_empty())
                {
                    return Err("请先处理共享机器权限".into());
                }
                if requires_dissolution(&impact) && v["dissolve"] != true {
                    return Err("请确认解散仅由你拥有的多人组织".into());
                }
                (
                    json!({"op":"profiles.close.start","dissolve":v["dissolve"]}),
                    After::Identity(Reply::CloseSent(v["dissolve"] == true)),
                )
            }
            IdentityForm::Verify {
                ticket,
                closing: true,
                dissolve,
                ..
            } => (
                json!({"op":"profiles.close.finish","id":ticket,"dissolve":dissolve,"confirmation":v["confirmation"]}),
                After::Notice("账号已注销".into()),
            ),
            IdentityForm::Verify { ticket, .. } => (
                json!({"op":"profiles.login.finish","id":ticket}),
                After::Notice("已登录".into()),
            ),
        };
        self.send(q, after, true);
        Ok(())
    }
}
