use super::{After, Request, UiAction, Workbench};
use crate::{
    controls::{INK, MUTED, col, outlined, row, text},
    forms::{Format, Mode, Spec, item},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[cfg(windows)]
#[path = "machine_management/tray.rs"]
mod tray;

#[derive(Clone)]
pub enum Dialog {
    SettingsFile(String),
    Policy(String, Value),
    Restart(String),
    Quit(bool),
    Runtimes(Value),
    Command(String, String, &'static str),
}
#[derive(Clone)]
pub enum Action {
    Settings,
    Inventory(String),
    Quit,
}
pub(super) enum Reply {
    Policy(String),
    Saved,
    Quit,
    Inventory,
    Queued(String),
}
const MODES: &[(&str, &str)] = &[
    ("manual", "手动启动"),
    ("desktop", "打开桌面时启动"),
    ("login", "登录 Windows 时启动"),
];
impl Dialog {
    pub fn title(&self) -> &'static str {
        match self {
            Self::SettingsFile(_) | Self::Policy(..) => "本机服务设置",
            Self::Restart(_) => "重新启动工作机",
            Self::Quit(_) => "退出 ShipForge",
            Self::Runtimes(_) => "运行时能力",
            Self::Command(_, _, op) => operation(op),
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::SettingsFile(_) => "读取配置",
            Self::Policy(..) => "保存",
            Self::Restart(_) => "确认重启",
            Self::Quit(_) => "确认退出",
            Self::Runtimes(_) => "关闭",
            Self::Command(..) => "确认执行",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        match self {
            Self::SettingsFile(_) => vec![item("config", "工作机配置文件", Format::Text, "")],
            Self::Policy(..) => vec![
                item("mode", "启动方式", Format::Choice(MODES), "manual"),
                item(
                    "keep_background",
                    "退出桌面后继续运行工作机",
                    Format::Bool,
                    "true",
                ),
                item(
                    "close_to_tray",
                    "关闭窗口时收起到托盘",
                    Format::Bool,
                    "true",
                ),
            ],
            Self::Quit(_) => vec![item(
                "keep_background",
                "退出桌面后继续运行工作机",
                Format::Bool,
                "true",
            )],
            _ => vec![],
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        let data = match self {
            Self::SettingsFile(path) => json!({"config":path}),
            Self::Policy(_, v) => v["policy"].clone(),
            Self::Quit(keep) => json!({"keep_background":keep}),
            _ => Value::Null,
        };
        data.as_object().cloned().unwrap_or_default()
    }
}
fn operation(op: &str) -> &'static str {
    match op {
        "rescan" => "重新检测",
        "install" => "安装运行时",
        "upgrade" => "升级运行时",
        "test" => "测试模型连接",
        _ => "刷新",
    }
}
fn state(s: &Value) -> String {
    crate::locale::label(match s.as_str() {
        Some("available") => "可用",
        Some("not_detected") => "未检测到",
        Some("detection_failed") => "检测失败",
        Some("offline") => "离线",
        Some("queued") => "排队中",
        Some("installing") => "安装中",
        Some("upgrading") => "升级中",
        Some("testing") => "测试中",
        Some("checking") => "检测中",
        Some("success") => "成功",
        Some("login_required") => "需要登录",
        Some("timeout") => "超时",
        Some("unexpected_reply") => "模型返回不符合测试要求",
        Some("manager_unavailable") => "包管理器不可用",
        Some("failed") => "失败",
        _ => s.as_str().unwrap_or(""),
    })
    .to_string()
}
fn guide(name: &str) -> Option<&'static str> {
    match name {
        "claude" => Some("https://code.claude.com/docs/en/setup"),
        "gemini" => Some("https://geminicli.com/docs/get-started/installation/"),
        "codex" => Some("https://developers.openai.com/codex/cli/"),
        "opencode" => Some("https://opencode.ai/docs/"),
        _ => None,
    }
}
pub fn bind_close(view: &Entity<Workbench>, window: &mut Window, cx: &mut App) {
    let view = view.downgrade();
    window.on_window_should_close(cx, move |w, cx| {
        view.update(cx, |s, cx| s.machine_window_close(w, cx))
            .unwrap_or(true)
    });
}
#[cfg(all(windows, test))]
pub(crate) fn test_hidden_icon(window: &Window) {
    tray::test_hidden_icon(window);
}
impl Workbench {
    fn desktop_machine_policy(&self) -> Value {
        self.directory
            .clone()
            .or_else(|| shipforge_client::data_dir().ok())
            .and_then(|d| std::fs::read(d.join("desktop-service-policy.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| json!({"keep_background":true,"close_to_tray":true}))
    }
    pub(crate) fn machine_window_close(&mut self, w: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.smoke || self.mode == "demo" {
            return true;
        }
        #[cfg(windows)]
        if !tray::take_exit()
            && self.desktop_machine_policy()["close_to_tray"] != false
            && tray::hide(w)
        {
            return false;
        }
        self.machine_management_action(Action::Quit, w, cx);
        false
    }
    fn local_service_request(&mut self, fields: Value, reply: Reply) -> Result<(), String> {
        let ticket = self.register(After::MachineManagement(reply), true);
        self.backend
            .sender
            .send(Request::Machine {
                ticket,
                space: self.loc.space.clone(),
                fields,
                stop: false,
            })
            .map_err(|e| e.to_string())
    }
    pub(super) fn machine_management_action(
        &mut self,
        action: Action,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Settings => {
                let path =
                    crate::machine::default_config(self.directory.as_deref(), &self.loc.space)
                        .unwrap_or_default();
                self.open_form(
                    Mode::MachineManagement(Dialog::SettingsFile(path.to_string_lossy().into())),
                    w,
                    cx,
                );
            }
            Action::Quit => self.open_form(
                Mode::MachineManagement(Dialog::Quit(
                    self.desktop_machine_policy()["keep_background"] != false,
                )),
                w,
                cx,
            ),
            Action::Inventory(id) => self.send(
                json!({"op":"machine.runtimes","id":id,"space":self.loc.space}),
                After::MachineManagement(Reply::Inventory),
                false,
            ),
        }
    }
    pub(super) fn machine_management_submit(
        &mut self,
        form: Dialog,
        mut values: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match form{
        Dialog::SettingsFile(_)=>{let path=values["config"].as_str().unwrap_or("").trim().to_owned();if !PathBuf::from(&path).is_absolute(){return Err("需要工作机配置文件的绝对路径。".into());}self.local_service_request(json!({"config":path,"service_operation":"status"}),Reply::Policy(path))?;}
        Dialog::Policy(path,_)=>{values["config"]=json!(path);values["service_operation"]=json!("configure");self.local_service_request(values,Reply::Saved)?;}
        Dialog::Restart(path)=>{self.local_service_request(json!({"config":path,"service_operation":"restart"}),Reply::Policy(path))?;}
        Dialog::Quit(_)=>{values["all"]=json!(true);values["service_operation"]=json!("desktop-exit");self.local_service_request(values,Reply::Quit)?;}
        Dialog::Runtimes(_)=>self.close_form(w,cx),
        Dialog::Command(id,runtime,action)=>self.send(json!({"op":"machine.runtime.request","space":self.loc.space,"id":id,"runtime":runtime,"action":action,"request_id":uuid::Uuid::new_v4().to_string()}),After::MachineManagement(Reply::Queued(id)),true)
    }
        Ok(())
    }
    pub(super) fn machine_management_reply(
        &mut self,
        r: Reply,
        v: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match r {
            Reply::Policy(path) => {
                self.open_form(Mode::MachineManagement(Dialog::Policy(path, v)), w, cx)
            }
            Reply::Saved => {
                self.close_form(w, cx);
                self.notify_text("已保存本机服务设置");
            }
            Reply::Quit => {
                if let Some(issue) = v["machines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find_map(|m| m["error"].as_str())
                {
                    self.error = Some(issue.into());
                    return;
                }
                w.remove_window();
            }
            Reply::Inventory => {
                self.open_form(Mode::MachineManagement(Dialog::Runtimes(v)), w, cx);
                self.machine_poll_at = Instant::now() + Duration::from_secs(2);
            }
            Reply::Queued(id) => self.machine_management_action(Action::Inventory(id), w, cx),
        }
    }
    pub(super) fn machine_management_tick(&mut self) {
        if self.machine_poll_at > Instant::now()
            || self.busy
            || self
                .pending
                .values()
                .any(|p| matches!(p.after, After::MachineManagement(_)))
        {
            return;
        }
        let Some(Mode::MachineManagement(Dialog::Runtimes(data))) =
            self.form.as_ref().map(|f| &f.mode)
        else {
            return;
        };
        if data["recent"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|j| matches!(j["state"].as_str(), Some("queued" | "running")))
        {
            self.machine_poll_at = Instant::now() + Duration::from_secs(2);
            self.send(
                json!({"op":"machine.runtimes","space":self.loc.space,"id":data["machine"]}),
                After::MachineManagement(Reply::Inventory),
                false,
            );
        }
    }
    pub(crate) fn machine_management_content(&self, f: &Dialog, cx: &Context<Self>) -> AnyElement {
        let mut body = col().gap(px(8.));
        let btn = |key: String, label: &'static str, a: UiAction| {
            outlined(self.button(SharedString::from(key), label, a, cx))
                .child(crate::locale::label(label))
        };
        let form = |f| UiAction::Form(Mode::MachineManagement(f));
        match f {
            Dialog::SettingsFile(_) => {
                body = body
                    .child(text("选择这台电脑上已初始化的工作机配置。", 13., MUTED))
                    .child(btn(
                        "machine-quit-menu".into(),
                        "退出 ShipForge",
                        UiAction::MachineManagement(Action::Quit),
                    ))
            }
            Dialog::Policy(path, v) => {
                body = body
                    .child(text(
                        format!(
                            "{} · {}",
                            v["machine"].as_str().unwrap_or(""),
                            crate::locale::label(if v["running"] == true {
                                "运行中"
                            } else {
                                "已停止"
                            })
                        ),
                        14.,
                        INK,
                    ))
                    .child(btn(
                        "machine-restart".into(),
                        "重新启动工作机",
                        form(Dialog::Restart(path.clone())),
                    ))
                    .child(btn(
                        "machine-quit".into(),
                        "退出 ShipForge",
                        UiAction::MachineManagement(Action::Quit),
                    ));
            }
            Dialog::Restart(_) => {
                body = body.child(text(
                    "重启会中断本机正在执行的工作，然后重新连接。",
                    13.,
                    MUTED,
                ))
            }
            Dialog::Quit(_) => {
                body = body.child(text("关闭后台运行会中断本机正在执行的工作。", 13., MUTED))
            }
            Dialog::Command(_, name, act) => {
                body = body.child(text(
                    format!("{} · {name}", crate::locale::label(operation(act))),
                    14.,
                    INK,
                ))
            }
            Dialog::Runtimes(v) => {
                let id = v["machine"].as_str().unwrap_or("");
                body = body.child(btn(
                    "runtime-refresh".into(),
                    "刷新",
                    UiAction::MachineManagement(Action::Inventory(id.into())),
                ));
                for runtime in v["runtimes"].as_array().into_iter().flatten() {
                    let name = runtime["name"].as_str().unwrap_or("");
                    let status = state(&runtime["status"]);
                    let version = runtime["version"].as_str().unwrap_or("");
                    let mut actions = row().gap(px(4.)).flex_wrap();
                    let available = runtime["detected_status"] == "available";
                    if let Some(url) = guide(name) {
                        actions = actions.child(btn(
                            format!("runtime-guide-{name}"),
                            "安装与登录指南",
                            UiAction::OpenSourceUrl(url.into()),
                        ));
                    }
                    if v["can_manage"] == true
                        && v["online"] == true
                        && runtime["operation"].is_null()
                    {
                        for (op, show) in [
                            ("rescan", true),
                            ("install", !available && runtime["installable"] == true),
                            ("upgrade", available && runtime["upgradable"] == true),
                            ("test", available && runtime["testable"] == true),
                        ] {
                            if show {
                                actions = actions.child(btn(
                                    format!("runtime-{op}-{name}"),
                                    operation(op),
                                    form(Dialog::Command(id.into(), name.into(), op)),
                                ));
                            }
                        }
                    }
                    body = body
                        .child(text(format!("{name}   {version}   {status}"), 14., INK))
                        .child(actions);
                }
                for job in v["recent"].as_array().into_iter().flatten().take(8) {
                    let outcome = &job["result"]["outcome"];
                    body = body.child(text(
                        format!(
                            "{} · {} · {}",
                            job["name"].as_str().unwrap_or(""),
                            crate::locale::label(operation(job["action"].as_str().unwrap_or(""))),
                            state(if outcome.is_null() {
                                &job["state"]
                            } else {
                                outcome
                            })
                        ),
                        12.,
                        MUTED,
                    ));
                }
            }
        }
        body.into_any_element()
    }
}
