use crate::config::{Config, Result, hex_digest, state_dir};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod startup;

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub(crate) enum Operation {
    Status,
    Start,
    Stop,
    Restart,
    Configure,
    DesktopOpen,
    DesktopExit,
    Login,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartMode {
    #[default]
    Manual,
    Desktop,
    Login,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Policy {
    pub mode: StartMode,
    pub keep_background: bool,
    pub close_to_tray: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            mode: StartMode::Manual,
            keep_background: true,
            close_to_tray: true,
        }
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) fn save(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().ok_or("invalid service data path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut f = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(f.as_file_mut(), value).map_err(|e| e.to_string())?;
    f.flush()
        .and_then(|()| f.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    f.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn read(path: &Path) -> Result<Value> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(e) => Err(e.to_string()),
    }
}
fn guard(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.lock().map_err(|e| e.to_string())?;
    Ok(f)
}
pub(crate) fn live(state: &Path, machine: &str, phase: &str, began: u64) -> Result<()> {
    save(
        &state.join("service-live.json"),
        &json!({"machine":machine,"status":phase,"pid":std::process::id(),"started_at":began,"at":now()}),
    )
}
pub(crate) fn is_running(state: &Path) -> Result<bool> {
    let f = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join("supervisor.lock"))
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    match f.try_lock() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}
fn policy(state: &Path, data: &Path) -> Result<Policy> {
    let v = read(&state.join("service-policy.json"))?;
    let mut policy: Policy = if v.is_null() {
        Policy::default()
    } else {
        serde_json::from_value(v).map_err(|e| e.to_string())?
    };
    let desktop = read(&data.join("desktop-service-policy.json"))?;
    if let Some(keep) = desktop["keep_background"].as_bool() {
        policy.keep_background = keep;
    }
    if let Some(tray) = desktop["close_to_tray"].as_bool() {
        policy.close_to_tray = tray;
    }
    Ok(policy)
}
fn status(state: &Path, cfg: &Config, data: &Path) -> Result<Value> {
    let running = is_running(state)?;
    let prior = read(&state.join("service-live.json"))?;
    Ok(
        json!({"machine":cfg.machine_id,"status":if running{prior["status"].as_str().unwrap_or("starting")}else{"stopped"},"running":running,"pid":if running{prior["pid"].clone()}else{Value::Null},"started_at":prior["started_at"],"policy":policy(state,data)?}),
    )
}
pub(crate) fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}
#[cfg(windows)]
fn detach_request_streams() {
    // The CLI may itself have inherited the caller's output pipes. CreateProcess inherits every
    // inheritable handle, including those original handles even when child stdio is redirected.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(id: u32) -> *mut std::ffi::c_void;
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    unsafe {
        for id in [-10i32, -11, -12] {
            let h = GetStdHandle(id as u32);
            if !h.is_null() && h as isize != -1 {
                SetHandleInformation(h, 1, 0);
            }
        }
    }
}
fn start(config: &Path, state: &Path, cfg: &Config, data: &Path) -> Result<Value> {
    if is_running(state)? {
        return status(state, cfg, data);
    }
    let at = now();
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(config.with_extension("worker.log"))
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    hidden(&mut command);
    #[cfg(windows)]
    detach_request_streams();
    let mut child = command
        .arg("run")
        .arg("--config")
        .arg(config)
        .env("SHIPFORGE_DATA_DIR", data)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|e| e.to_string())?;
    let limit = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "machine supervisor exited during registration ({exit}); inspect the local worker log"
            ));
        }
        let live = read(&state.join("service-live.json"))?;
        if live["pid"] == child.id()
            && live["status"] == "online"
            && live["at"].as_u64().is_some_and(|t| t >= at)
        {
            return status(state, cfg, data);
        }
        if Instant::now() >= limit {
            return Ok(
                json!({"machine":cfg.machine_id,"status":"starting","running":true,"pid":child.id(),"policy":policy(state,data)?}),
            );
        }
        thread::sleep(Duration::from_millis(60));
    }
}
fn stop(state: &Path, cfg: &Config, data: &Path) -> Result<Value> {
    if !is_running(state)? {
        return status(state, cfg, data);
    }
    fs::write(state.join("stop.request"), b"immediate").map_err(|e| e.to_string())?;
    let limit = Instant::now() + Duration::from_secs(15);
    while is_running(state)? {
        if Instant::now() >= limit {
            return Err("machine is still stopping; wait for this supervisor or retry".into());
        }
        thread::sleep(Duration::from_millis(80));
    }
    status(state, cfg, data)
}
fn identity(config: &Path) -> String {
    hex_digest(config.to_string_lossy().as_bytes())
}
fn track(data: &Path, config: &Path) -> Result<()> {
    save(
        &data
            .join("machine-services")
            .join(format!("{}.json", identity(config))),
        &json!({"config":config}),
    )
}
pub(crate) fn desktop(action: Operation, data: &Path, keep: Option<bool>) -> Result<Value> {
    let index = data.join("machine-services");
    let mut out = Vec::new();
    let mut configs = std::collections::BTreeSet::new();
    if let Ok(path) = data.join("machine.json").canonicalize() {
        configs.insert(path);
    }
    let legacy = data.join("machines");
    if legacy.is_dir() {
        for file in fs::read_dir(legacy).map_err(|e| e.to_string())? {
            let path = file.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(path) = path.canonicalize()
            {
                configs.insert(path);
            }
        }
    }
    if action == Operation::DesktopExit
        && let Some(keep) = keep
    {
        update_desktop(data, Some(keep), None)?;
    }
    if index.is_dir() {
        for e in fs::read_dir(index).map_err(|e| e.to_string())? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let v = read(&p)?;
            if let Some(path) = v["config"]
                .as_str()
                .and_then(|p| Path::new(p).canonicalize().ok())
            {
                configs.insert(path);
            }
        }
    }
    for config in configs {
        out.push(match execute(action, &config, data, None, None, None) {
            Ok(v) => v,
            Err(e) => json!({"error":e}),
        });
    }
    Ok(json!({"machines":out}))
}
fn update_desktop(data: &Path, keep: Option<bool>, tray: Option<bool>) -> Result<()> {
    fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let _guard = guard(&data.join("desktop-service-policy.lock"))?;
    let path = data.join("desktop-service-policy.json");
    let current = read(&path)?;
    save(
        &path,
        &json!({"keep_background":keep.or(current["keep_background"].as_bool()).unwrap_or(true),"close_to_tray":tray.or(current["close_to_tray"].as_bool()).unwrap_or(true)}),
    )
}
pub(crate) fn execute(
    action: Operation,
    config: &Path,
    data: &Path,
    mode: Option<StartMode>,
    keep: Option<bool>,
    tray: Option<bool>,
) -> Result<Value> {
    let config = config.canonicalize().map_err(|e| e.to_string())?;
    let cfg = Config::load(&config)?;
    let state = state_dir(&config)?;
    if action == Operation::Status {
        return status(&state, &cfg, data);
    }
    let _guard = guard(&state.join("service-control.lock"))?;
    let before = policy(&state, data)?;
    match action {
        Operation::Status => status(&state, &cfg, data),
        Operation::Configure => {
            let updated = Policy {
                mode: mode.unwrap_or(before.mode),
                keep_background: keep.unwrap_or(before.keep_background),
                close_to_tray: tray.unwrap_or(before.close_to_tray),
            };
            let changes_login =
                (before.mode == StartMode::Login) != (updated.mode == StartMode::Login);
            if changes_login {
                startup::configure(&config, data, updated.mode == StartMode::Login)?;
            }
            if let Err(e) = track(data, &config)
                .and_then(|()| save(&state.join("service-policy.json"), &json!(updated)))
                .and_then(|()| {
                    if keep.is_some() || tray.is_some() {
                        update_desktop(data, keep, tray)
                    } else {
                        Ok(())
                    }
                })
            {
                let _ = save(&state.join("service-policy.json"), &json!(before));
                if changes_login {
                    let _ = startup::configure(&config, data, before.mode == StartMode::Login);
                }
                return Err(e);
            }
            status(&state, &cfg, data)
        }
        Operation::Start => {
            track(data, &config)?;
            start(&config, &state, &cfg, data)
        }
        Operation::Stop => stop(&state, &cfg, data),
        Operation::Restart => {
            track(data, &config)?;
            stop(&state, &cfg, data)?;
            start(&config, &state, &cfg, data)
        }
        Operation::DesktopOpen if before.mode == StartMode::Desktop => {
            start(&config, &state, &cfg, data)
        }
        Operation::DesktopExit if !before.keep_background => stop(&state, &cfg, data),
        Operation::Login if before.mode == StartMode::Login => start(&config, &state, &cfg, data),
        _ => status(&state, &cfg, data),
    }
}
