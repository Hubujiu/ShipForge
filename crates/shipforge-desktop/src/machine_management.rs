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
#[path = "machine_management/storage_policy.rs"]
mod storage_policy;
#[cfg(windows)]
#[path = "machine_management/tray.rs"]
mod tray;

#[derive(Clone)]
pub enum Dialog {
    StoragePolicy(Value),
    AgentLaunch(Value),
    Storage(Value),
    CleanStorage(String, Value),
    SettingsFile(String),
    Policy(String, Value),
    Restart(String),
    Quit(bool),
    Runtimes(Value),
    Command(String, String, &'static str),
}
#[derive(Clone)]
pub enum Action {
    PinStorage(String, String, bool),
    AgentLaunch(String),
    Storage(String),
    Scan(String),
    Settings,
    Inventory(String),
    Quit,
}
pub(super) enum Reply {
    StorageUpdated(String),
    AgentLaunch,
    Storage,
    StorageQueued(String),
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
            Self::AgentLaunch(_) => "模型与运行参数",
            Self::Storage(_) => "管理存储空间",
            Self::StoragePolicy(_) => "存储配额与清理策略",
            Self::CleanStorage(..) => "确认清理工作目录",
            Self::Command(_, _, op) => operation(op),
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::AgentLaunch(_) => "关闭",
            Self::Storage(_) => "关闭",
            Self::StoragePolicy(_) => "保存存储策略",
            Self::CleanStorage(..) => "确认清理",
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
            Self::StoragePolicy(_) => vec![
                item("enabled", "启用定时安全清理", Format::Bool, "false"),
                item(
                    "interval_minutes",
                    "清理间隔（分钟）",
                    Format::Number,
                    "1440",
                ),
                item("retention_days", "终态目录保留天数", Format::Number, "7"),
                item("cache_retention_days", "缓存保留天数", Format::Number, "7"),
                item(
                    "quota_gib",
                    "托管容量上限（GiB，0 表示不限）",
                    Format::Number,
                    "0",
                ),
                item(
                    "min_free_gib",
                    "最低磁盘可用空间（GiB）",
                    Format::Number,
                    "0",
                ),
            ],
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
            Self::StoragePolicy(v) => v["policy"].clone(),
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
fn format_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", n as f64 / 1_073_741_824.)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / 1_048_576.)
    } else {
        format!("{n} B")
    }
}
fn storage_reason(v: &Value) -> &'static str {
    match v.as_str() {
        None => "可清理",
        Some("active") => "运行中",
        Some("pending" | "recovery") => "等待恢复",
        Some("open_task") => "任务尚未结束",
        Some("conversation") => "对话可继续",
        Some("pinned") => "已固定",
        Some("attached_worktree") => "仍有工作树引用",
        Some("cache_in_use") => "缓存正在使用",
        Some("local_changes") => "含本地 Git 数据",
        Some("bound_repository") => "绑定目录",
        Some("links" | "git_locked") => "目录受保护",
        _ => "归属未确认",
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
            Action::AgentLaunch(id)=>self.send(json!({"op":"agent.runtime.options","space":self.loc.space,"id":id}),After::MachineManagement(Reply::AgentLaunch),false),
            Action::Storage(id)=>self.send(json!({"op":"machine.storage.preview","space":self.loc.space,"id":id}),After::MachineManagement(Reply::Storage),false),
            Action::Scan(id)=>self.send(json!({"op":"machine.storage.request","space":self.loc.space,"id":id,"request_id":uuid::Uuid::new_v4().to_string(),"action":"scan"}),After::MachineManagement(Reply::StorageQueued(id)),true),
            Action::PinStorage(id,key,pinned)=>self.send(json!({"op":"machine.storage.pin","space":self.loc.space,"id":id,"workspace":key,"pinned":pinned}),After::MachineManagement(Reply::StorageUpdated(id)),true),
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
        Dialog::AgentLaunch(_)=>self.close_form(w,cx),
        Dialog::Storage(_)=>self.close_form(w,cx),
        Dialog::StoragePolicy(v)=>{let id=v["machine"].as_str().ok_or("机器不可用")?.to_owned();self.send(json!({"op":"machine.storage.configure","space":self.loc.space,"id":id,"expected_rev":v["policy_rev"],"policy":values}),After::MachineManagement(Reply::StorageUpdated(id)),true);}
        Dialog::CleanStorage(id,entry)=>self.send(json!({"op":"machine.storage.request","action":"clean","confirm":true,"space":self.loc.space,"id":id,"workspace":entry["id"],"expected_snapshot":entry["snapshot"],"request_id":uuid::Uuid::new_v4().to_string()}),After::MachineManagement(Reply::StorageQueued(id)),true),
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
            Reply::AgentLaunch => {
                self.open_form(Mode::MachineManagement(Dialog::AgentLaunch(v)), w, cx)
            }
            Reply::Storage => {
                self.open_form(Mode::MachineManagement(Dialog::Storage(v)), w, cx);
                self.machine_poll_at = Instant::now() + Duration::from_secs(2);
            }
            Reply::StorageQueued(id) => {
                self.notify_text("已加入工作机队列");
                self.machine_management_action(Action::Storage(id), w, cx);
            }
            Reply::StorageUpdated(id) => self.machine_management_action(Action::Storage(id), w, cx),
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
        if let Some(Mode::MachineManagement(Dialog::Storage(data))) =
            self.form.as_ref().map(|f| &f.mode)
        {
            if data["jobs"].as_array().into_iter().flatten().any(|j| {
                matches!(
                    j["state"].as_str(),
                    Some("queued" | "running" | "committing")
                )
            }) {
                self.machine_poll_at = Instant::now() + Duration::from_secs(2);
                self.send(json!({"op":"machine.storage.preview","space":self.loc.space,"id":data["machine"]}),After::MachineManagement(Reply::Storage),false);
            }
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
            Dialog::AgentLaunch(v) => {
                let select = |model: Option<&str>, effort: Option<&str>| {
                    UiAction::Apply(
                        json!({"op":"update","space":self.loc.space,"kind":"agent","id":v["id"],"expected_rev":v["rev"],"fields":{"model":model.unwrap_or(""),"reasoning_effort":effort.unwrap_or("")}}),
                        "已保存模型设置".into(),
                    )
                };
                if v["machines"].as_array().is_none_or(Vec::is_empty) {
                    body = body.child(text("该运行时尚未公布模型与参数。", 13., MUTED));
                }
                for m in v["machines"].as_array().into_iter().flatten() {
                    let launch = &m["launch"];
                    let mid = m["id"].as_str().unwrap_or("");
                    body = body.child(text(m["title"].as_str().unwrap_or("").to_owned(), 14., INK));
                    if let Some(fixed) = launch["fixed_model"].as_str() {
                        body = body.child(text(
                            format!("{} · {fixed}", crate::locale::label("机器固定模型")),
                            13.,
                            MUTED,
                        ));
                    }
                    if v["can_manage"] == true && launch["fixed_model"].is_null() {
                        body = body.child(btn(
                            format!("model-default-{mid}"),
                            "跟随运行时默认模型",
                            select(None, None),
                        ));
                    }
                    for (name, efforts) in launch["models"].as_object().into_iter().flatten() {
                        let current = launch["fixed_model"]
                            .as_str()
                            .or(v["model"].as_str().filter(|s| !s.is_empty()))
                            .or(launch["default_model"].as_str())
                            == Some(name);
                        if v["can_manage"] == true && launch["fixed_model"].is_null() {
                            body = body.child(
                                outlined(self.button(
                                    format!("choose-model-{mid}-{name}"),
                                    name.clone(),
                                    select(Some(name), None),
                                    cx,
                                ))
                                .child(name.clone()),
                            );
                        }
                        if current && v["can_manage"] == true {
                            for effort in efforts
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                            {
                                let label = format!("{name} · {effort}");
                                body = body.child(
                                    outlined(self.button(
                                        format!("choose-effort-{mid}-{effort}"),
                                        label.clone(),
                                        select(
                                            launch["fixed_model"].is_null().then_some(name),
                                            Some(effort),
                                        ),
                                        cx,
                                    ))
                                    .child(label),
                                );
                            }
                        }
                    }
                    for (name, values) in
                        launch["argument_choices"].as_object().into_iter().flatten()
                    {
                        body = body.child(text(
                            format!(
                                "{name} · {}",
                                values
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_str)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            12.,
                            MUTED,
                        ));
                    }
                    for name in launch["environment_names"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        body = body.child(text(format!("ENV · {name}"), 12., MUTED));
                    }
                }
            }
            Dialog::Storage(v) => {
                let id = v["machine"].as_str().unwrap_or("");
                body = body.child(self.storage_health_panel(v, cx));
                if let Some(scanned) = v["scanned_at"].as_i64() {
                    let at = chrono::DateTime::from_timestamp_millis(scanned)
                        .map(|v| {
                            v.with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M")
                                .to_string()
                        })
                        .unwrap_or_default();
                    body = body.child(text(
                        format!(
                            "{} · {} · {at}",
                            crate::locale::label("本机服务占用"),
                            format_bytes(v["total_bytes"].as_u64().unwrap_or(0))
                        ),
                        14.,
                        INK,
                    ));
                }
                if let Some(bytes) = v["pending_reclaim_bytes"].as_u64().filter(|b| *b > 0) {
                    body = body.child(text(
                        format!(
                            "{} · {}",
                            crate::locale::label("待释放空间"),
                            format_bytes(bytes)
                        ),
                        13.,
                        MUTED,
                    ));
                }
                if v["online"] == true {
                    body = body.child(btn(
                        "scan-worker-storage".into(),
                        "重新统计",
                        UiAction::MachineManagement(Action::Scan(id.into())),
                    ));
                }
                if v["entries"].as_array().is_none_or(Vec::is_empty) {
                    body = body.child(text(
                        if v["scanned_at"].is_null() {
                            "尚未统计，请连接工作机并重新统计。"
                        } else {
                            "暂无已登记的工作目录。"
                        },
                        13.,
                        MUTED,
                    ));
                }
                for entry in v["entries"].as_array().into_iter().flatten() {
                    let key = entry["id"].as_str().unwrap_or("");
                    let name = entry["title"].as_str().unwrap_or_else(|| {
                        if entry["category"] == "cache" {
                            crate::locale::label("托管 Git 缓存")
                        } else {
                            &key[..key.len().min(10)]
                        }
                    });
                    body = body.child(text(
                        format!(
                            "{name} · {} · {}",
                            format_bytes(entry["bytes"].as_u64().unwrap_or(0)),
                            crate::locale::label(storage_reason(&entry["protection"]))
                        ),
                        13.,
                        INK,
                    ));
                    if self.access_identity["scope"] != "read" {
                        body = body.child(btn(
                            format!("pin-storage-{key}"),
                            if entry["pinned"] == true {
                                "取消固定工作区"
                            } else {
                                "固定工作区"
                            },
                            UiAction::MachineManagement(Action::PinStorage(
                                id.into(),
                                key.into(),
                                entry["pinned"] != true,
                            )),
                        ));
                    }
                    if v["online"] == true
                        && entry["protection"].is_null()
                        && self.access_identity["scope"] != "read"
                    {
                        body = body.child(btn(
                            format!("clean-storage-{key}"),
                            "清理工作目录",
                            form(Dialog::CleanStorage(id.into(), entry.clone())),
                        ));
                    }
                }
                for j in v["jobs"].as_array().into_iter().flatten().take(3) {
                    body = body.child(text(
                        format!(
                            "{} · {}",
                            crate::locale::label(if j["action"] == "scan" {
                                if j["scheduled"] == true {
                                    "后台统计"
                                } else {
                                    "重新统计"
                                }
                            } else {
                                if j["scheduled"] == true {
                                    "定时安全清理"
                                } else {
                                    "清理工作目录"
                                }
                            }),
                            crate::locale::label(match j["state"].as_str() {
                                Some("queued") => "排队中",
                                Some("done") => "成功",
                                Some("failed") => "目录状态已变化，请重新统计",
                                _ => "处理中",
                            })
                        ),
                        12.,
                        MUTED,
                    ));
                }
            }
            Dialog::StoragePolicy(_) => {
                body = body.child(text(
                    "启用后，工作机会按保留期限重新核验并回收符合条件的数据。",
                    13.,
                    MUTED,
                ));
            }
            Dialog::CleanStorage(_, entry) => {
                body = body
                    .child(text(
                        format!(
                            "{} · {}",
                            entry["title"].as_str().unwrap_or("工作目录"),
                            format_bytes(entry["bytes"].as_u64().unwrap_or(0))
                        ),
                        14.,
                        INK,
                    ))
                    .child(text("确认后删除这份工作副本及其中的本地更改。", 13., MUTED))
            }
        }
        body.into_any_element()
    }
}
