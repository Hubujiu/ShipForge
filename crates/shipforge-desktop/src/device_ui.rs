use super::{After, UiAction, Workbench};
use crate::{
    catalog::Page,
    controls::{INK, MUTED, col, outlined, row, text},
    forms::{Format, Mode, Spec, item},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};

#[derive(Clone)]
pub enum Dialog {
    Proxy(Value),
    About(Value, Option<Value>),
}
#[derive(Clone)]
pub enum Action {
    Settings,
    About,
    Test,
    Update(Value),
    Guide(String),
    GettingStarted,
}
pub(super) enum Reply {
    Settings,
    About,
    Tested(uuid::Uuid),
    Saved,
    Updated(Value),
}
impl Dialog {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Proxy(_) => "网络代理",
            Self::About(..) => "关于 ShipForge",
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::Proxy(_) => "保存并应用",
            Self::About(..) => "关闭",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        if !matches!(self, Self::Proxy(_)) {
            return vec![];
        }
        vec![
            item(
                "mode",
                "连接方式",
                Format::Choice(&[
                    ("system", "使用系统代理"),
                    ("direct", "直接连接"),
                    ("manual", "手动配置代理"),
                ]),
                "system",
            ),
            item("endpoint", "代理服务器地址", Format::Text, ""),
            item("username", "代理用户名", Format::Text, ""),
            item("password", "代理密码", Format::Secret, ""),
            item(
                "clear_password",
                "移除已保存的代理密码",
                Format::Bool,
                "false",
            ),
            item(
                "test_url",
                "测试地址",
                Format::Text,
                "https://api.github.com/",
            ),
        ]
    }
    pub fn prefill(&self) -> Map<String, Value> {
        match self {
            Self::Proxy(v) => v.as_object().cloned().unwrap_or_default(),
            _ => Map::new(),
        }
    }
    pub fn field_visible(&self, key: &str, mode: &str) -> bool {
        if matches!(key, "endpoint" | "username" | "password" | "clear_password") {
            return mode == "manual"
                && (key != "clear_password"
                    || matches!(self,Self::Proxy(cfg) if cfg["has_password"]==true));
        }
        true
    }
}
impl Workbench {
    pub(super) fn device_action(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Settings => self.send(
                json!({"op":"network.settings"}),
                After::Device(Reply::Settings),
                false,
            ),
            Action::About => self.send(
                json!({"op":"device.about"}),
                After::Device(Reply::About),
                false,
            ),
            Action::Update(v) => self.send(
                json!({"op":"device.update"}),
                After::Device(Reply::Updated(v)),
                false,
            ),
            Action::Test => {
                let Some(f) = self.form.as_ref() else {
                    return;
                };
                if !matches!(f.mode, Mode::Device(Dialog::Proxy(_))) {
                    return;
                }
                let id = f.request_id;
                match f.values(cx) {
                    Ok(fields) => {
                        let mut q = Value::Object(fields);
                        q["op"] = json!("network.test");
                        self.send(q, After::Device(Reply::Tested(id)), false);
                    }
                    Err(err) => self.notify_text(err),
                }
            }
            Action::Guide(url) => {
                if url.starts_with("file:")
                    || url.starts_with("https://github.com/Hubujiu/ShipForge/")
                {
                    cx.open_url(&url);
                }
            }
            Action::GettingStarted => {
                self.close_form(w, cx);
                self.act(UiAction::Go(Page::GettingStarted), w, cx);
            }
        }
    }
    pub(super) fn device_submit(
        &mut self,
        dialog: Dialog,
        mut q: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match dialog {
            Dialog::Proxy(cfg) => {
                q["op"] = json!("network.save");
                q["expected_rev"] = cfg["rev"].clone();
                self.send(q, After::Device(Reply::Saved), true);
            }
            Dialog::About(..) => self.close_form(w, cx),
        }
    }
    pub(super) fn device_reply(
        &mut self,
        reply: Reply,
        value: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Reply::Settings => self.open_form(Mode::Device(Dialog::Proxy(value)), w, cx),
            Reply::Saved => {
                self.close_form(w, cx);
                self.notify_text("网络设置已应用");
                self.read(false);
            }
            Reply::Tested(id) => {
                if let Some(f) = &mut self.form
                    && f.request_id == id
                {
                    f.error.clear();
                    f.preview = Some(value);
                }
            }
            Reply::About => self.open_form(Mode::Device(Dialog::About(value, None)), w, cx),
            Reply::Updated(v) => {
                if self
                    .form
                    .as_ref()
                    .is_some_and(|f| matches!(&f.mode, Mode::Device(Dialog::About(..))))
                {
                    self.open_form(Mode::Device(Dialog::About(v, Some(value))), w, cx);
                }
            }
        }
    }
    pub(crate) fn device_content(&self, dialog: &Dialog, cx: &Context<Self>) -> AnyElement {
        let label = crate::locale::label;
        let button = |id: &'static str, title: &'static str, action: UiAction| {
            outlined(self.button(id, title, action, cx)).child(label(title))
        };
        let mut panel = col().gap(px(8.));
        match dialog {
            Dialog::Proxy(cfg) => {
                panel = panel
                    .child(text(label("本机回环服务始终直接连接。"), 13., MUTED))
                    .child(button(
                        "test-device-proxy",
                        "测试连接",
                        UiAction::Device(Action::Test),
                    ));
                if cfg["has_password"] == true {
                    panel = panel.child(text(label("已保存代理密码。"), 13., MUTED));
                }
                if let Some(v) = self.form.as_ref().and_then(|f| f.preview.as_ref()) {
                    panel = panel.child(text(
                        format!(
                            "{} · HTTP {} · {} ms",
                            label("已连接"),
                            v["status"],
                            v["elapsed_ms"]
                        ),
                        14.,
                        INK,
                    ));
                }
            }
            Dialog::About(v, update) => {
                panel = panel
                    .child(text(
                        format!(
                            "ShipForge {} · {} · {} · MIT",
                            v["version"].as_str().unwrap_or(""),
                            v["os"].as_str().unwrap_or(""),
                            v["architecture"].as_str().unwrap_or("")
                        ),
                        16.,
                        INK,
                    ))
                    .child(
                        row()
                            .gap(px(8.))
                            .child(button(
                                "device-guide",
                                "使用手册",
                                UiAction::Device(Action::Guide(
                                    v["guide"].as_str().unwrap_or("").into(),
                                )),
                            ))
                            .child(button(
                                "device-start",
                                "开始使用",
                                UiAction::Device(Action::GettingStarted),
                            ))
                            .child(button(
                                "device-update",
                                "检查更新",
                                UiAction::Device(Action::Update(v.clone())),
                            )),
                    );
                if let Some(result) = update {
                    let message = if result["available"] == true {
                        "有新版本可下载"
                    } else if result["assets_ready"] == false {
                        "发布文件正在准备"
                    } else {
                        "当前已是最新版本"
                    };
                    panel = panel.child(text(
                        format!(
                            "{} · {}",
                            label(message),
                            result["latest"].as_str().unwrap_or("")
                        ),
                        14.,
                        INK,
                    ));
                    panel = panel.child(button(
                        "device-release",
                        "查看发布说明",
                        UiAction::OpenSourceUrl(
                            result["release_url"].as_str().unwrap_or("").into(),
                        ),
                    ));
                }
            }
        }
        panel.into_any_element()
    }
}
