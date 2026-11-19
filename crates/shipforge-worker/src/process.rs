use crate::config::{Config, Result, Runtime, program_path};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
mod crew_workspace;
mod run_skills;

struct WorkingDir {
    path: PathBuf,
    _lock: Option<File>,
    isolated: bool,
}
fn git_at(source: &Path, args: &[&str], destination: &Path) -> Result<()> {
    let out = Command::new(program_path(Path::new("git"))?)
        .arg("-C")
        .arg(dunce::simplified(source))
        .args(args)
        .arg(dunce::simplified(destination))
        .arg("HEAD")
        .stdout(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "git worktree: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
fn directory(
    cfg: &Config,
    run: &Value,
    state: &Path,
    pulse: &mut impl FnMut() -> Result<bool>,
    timeout: Duration,
) -> Result<WorkingDir> {
    let settings = &run["fields"]["config"];
    let assigned = &run["fields"]["assigned_directory"];
    let chosen = assigned["profile"]
        .as_str()
        .or_else(|| settings["profile"].as_str())
        .filter(|s| !s.is_empty() && *s != "@workspace");
    let mode = assigned["mode"]
        .as_str()
        .or(settings["directory_mode"].as_str())
        .unwrap_or("isolated");
    let source = if let Some(profile) = chosen {
        Some(
            cfg.profiles
                .get(profile)
                .ok_or("requested workspace profile is not approved on this machine")?,
        )
    } else {
        cfg.workspace.as_ref()
    };
    let source = source
        .map(|p| {
            p.canonicalize()
                .map_err(|e| format!("approved directory {}: {e}", p.display()))
        })
        .transpose()?;
    if let Some(identity) = assigned.get("identity").and_then(Value::as_str)
        && source
            .as_deref()
            .is_none_or(|path| crate::config::directory_identity(path) != identity)
    {
        return Err("approved directory changed after the claim".into());
    }
    if !run["fields"]["crew_workspace"].is_null() || run["fields"]["isolated_workspace"].is_object()
    {
        return crew_workspace::open(cfg, run, state, source.as_deref(), pulse, timeout);
    }
    if mode == "direct" {
        if !cfg.permits_direct(chosen.unwrap_or("@workspace")) {
            return Err("direct directory execution is disabled by machine configuration".into());
        }
        let path = source.ok_or("direct execution needs an approved local workspace")?;
        if !path.is_dir() {
            return Err("approved source is not a directory".into());
        }
        let lock = directory_lock(&crate::config::directory_identity(&path))?;
        let until = Instant::now() + timeout;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < until => {
                    if pulse()? {
                        return Err("cancelled".into());
                    }
                    thread::sleep(Duration::from_secs(1));
                }
                Err(err) => return Err(format!("directory lock failed: {err}")),
            }
        }
        return Ok(WorkingDir {
            path,
            _lock: Some(lock),
            isolated: false,
        });
    }
    let root = state.join("work");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join(format!("run-{}", Uuid::new_v4().simple()));
    if let Some(source) = source {
        if path.starts_with(&source) {
            return Err(
                "keep machine config/state outside the bound repository for isolated worktrees"
                    .into(),
            );
        }
        git_at(&source, &["worktree", "add", "--detach"], &path)?;
    } else {
        fs::create_dir(&path).map_err(|e| e.to_string())?;
    }
    Ok(WorkingDir {
        path,
        _lock: None,
        isolated: true,
    })
}
const CAPTURE_BYTES: usize = 96 * 1024;
fn directory_lock(identity: &str) -> Result<File> {
    let locks = std::env::temp_dir().join("shipforge-directory-locks");
    fs::create_dir_all(&locks).map_err(|e| e.to_string())?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(locks.join(format!("{identity}.lock")))
        .map_err(|e| e.to_string())
}
pub(crate) fn busy_directories(cfg: &Config) -> Vec<String> {
    cfg.profile_paths()
        .filter(|(id, _)| cfg.permits_direct(id))
        .filter_map(|(_, p)| p.canonicalize().ok())
        .map(|p| crate::config::directory_identity(&p))
        .filter(|id| {
            directory_lock(id)
                .is_ok_and(|file| matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
        })
        .collect()
}
pub(crate) fn approve_directory(
    file: &Path,
    name: &str,
    path: &Path,
    direct: bool,
) -> Result<Value> {
    if !path.is_absolute()
        || !path.is_dir()
        || name == "@workspace"
        || name.trim().is_empty()
        || name.len() > 160
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err("choose an existing absolute directory and a unique logical name".into());
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)))
            || !matches!(canonical.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        {
            return Err("choose a local Windows volume".into());
        }
    }
    for key in [
        "USERPROFILE",
        "HOME",
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ] {
        if let Some(folder) =
            std::env::var_os(key).and_then(|p| PathBuf::from(p).canonicalize().ok())
            && (canonical == folder
                || !matches!(key, "USERPROFILE" | "HOME") && canonical.starts_with(folder))
        {
            return Err(
                "choose a project directory outside system folders and the home root".into(),
            );
        }
    }
    let state = crate::config::state_dir(file)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("settings.lock"))
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let cfg = Config::load(file)?;
    if cfg
        .profiles
        .get(name)
        .is_some_and(|p| p.canonicalize().ok().as_ref() != Some(&canonical))
    {
        return Err("this profile name already points to a different directory".into());
    }
    if !direct && !canonical.join(".git").exists() {
        return Err("a non-Git workspace requires explicit direct execution permission".into());
    }
    let mut value: Value = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    value["profiles"][name] = json!(path);
    let mut direct_profiles = cfg.direct_profiles;
    if direct {
        direct_profiles.insert(name.to_owned());
    } else {
        direct_profiles.remove(name);
    }
    value["direct_profiles"] = json!(direct_profiles);
    let mut temporary =
        tempfile::NamedTempFile::new_in(file.parent().ok_or("invalid configuration path")?)
            .map_err(|e| e.to_string())?;
    temporary
        .write_all(&serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    Config::load(temporary.path())?;
    temporary.persist(file).map_err(|e| e.to_string())?;
    Ok(json!({"machine":cfg.machine_id,"profile":name,"direct":cfg.allow_direct||direct}))
}
fn read_output(stream: impl Read) -> String {
    let mut reader = std::io::BufReader::new(stream);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    while let Ok(size) = reader.read(&mut chunk) {
        if size == 0 {
            break;
        }
        let n = size.min(CAPTURE_BYTES - bytes.len());
        bytes.extend_from_slice(&chunk[..n]);
        truncated |= n < size;
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    text
}
pub(crate) fn stop(child: &mut Child) {
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:/Windows"));
        let _ = Command::new(system.join("System32/taskkill.exe"))
            .args(["/T", "/F", "/PID"])
            .arg(child.id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn bounded_artifacts(list: &Value, workspace: &Path) -> Result<Vec<Value>> {
    use base64::Engine;
    let mut items = Vec::new();
    let mut inline_budget = 1024 * 1024;
    if let Some(entries) = list.as_array() {
        if entries.len() > 96 {
            return Err("too many artifacts".into());
        }
        for entry in entries {
            let raw = entry
                .as_str()
                .or_else(|| entry["path"].as_str())
                .ok_or("artifact needs a relative path")?;
            let rel = Path::new(raw);
            if rel.is_absolute()
                || !rel
                    .components()
                    .all(|p| matches!(p, Component::Normal(_) | Component::CurDir))
            {
                return Err("artifact must be relative to the approved working directory".into());
            }
            let file = workspace
                .join(rel)
                .canonicalize()
                .map_err(|e| format!("artifact {raw}: {e}"))?;
            if !file.starts_with(workspace) || !file.is_file() {
                return Err("artifact escapes workspace or is not a file".into());
            }
            let size = file.metadata().map_err(|e| e.to_string())?.len();
            let mut meta = json!({"name":entry["name"].as_str().unwrap_or(raw),"path":raw,"location":file,"bytes":size});
            let limit = inline_budget.min(256 * 1024);
            if size <= limit {
                let mut content = Vec::new();
                File::open(&file)
                    .and_then(|f| f.take(limit + 1).read_to_end(&mut content))
                    .map_err(|e| e.to_string())?;
                if content.len() as u64 <= limit {
                    inline_budget -= content.len() as u64;
                    meta["filename"] = json!(
                        file.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("artifact")
                    );
                    meta["media_type"] = json!(
                        entry["media_type"]
                            .as_str()
                            .unwrap_or("application/octet-stream")
                    );
                    meta["content_base64"] =
                        json!(base64::engine::general_purpose::STANDARD.encode(content));
                }
            }
            items.push(meta);
        }
    }
    Ok(items)
}
pub(crate) fn spawn_command(spec: &Runtime, dir: &Path) -> Result<Child> {
    let program = program_path(&spec.program)?;
    #[cfg(windows)]
    if program
        .extension()
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("exe") && !ext.eq_ignore_ascii_case("com"))
    {
        return Err("configure an executable and fixed arguments; for scripts explicitly configure the interpreter".into());
    }
    let mut cmd = Command::new(program);
    cmd.args(&spec.args).current_dir(dir).env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "SYSTEMDRIVE",
        "WINDIR",
        "PATHEXT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "HOME",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "LANG",
        "LC_ALL",
    ]
    .into_iter()
    .chain(spec.inherit_env.iter().map(String::as_str))
    {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("launch {}: {e}", spec.program.display()))
}
pub fn execute(
    config: &Config,
    state: &Path,
    run: &Value,
    mut pulse: impl FnMut() -> Result<bool>,
    mut tools: impl FnMut(&Value, &Path) -> Result<Value>,
) -> Result<Value> {
    let runtime = run["fields"]["runtime"].as_str().ok_or("missing runtime")?;
    let spec = config
        .runtimes
        .get(runtime)
        .ok_or("runtime is not enabled in this machine configuration")?;
    let timeout = Duration::from_secs(
        spec.timeout_seconds
            .min(
                run["fields"]["config"]["run_timeout_minutes"]
                    .as_u64()
                    .map(|m| m.clamp(15, 43_200) * 60)
                    .or_else(|| run["fields"]["config"]["timeout_seconds"].as_u64())
                    .unwrap_or(180 * 60),
            )
            .max(1),
    );
    let began = Instant::now();
    let workspace = directory(config, run, state, &mut pulse, timeout)?;
    let canonical = workspace.path.canonicalize().map_err(|e| e.to_string())?;
    let until = began + timeout;
    let mut context = run.clone();
    let _skills = run_skills::stage(state, &mut context)?;
    for round in 0..=4 {
        if pulse()? {
            return Err("cancelled".into());
        }
        let mut result = execute_round(spec, &canonical, &context, &mut pulse, until, state)?;
        let calls = result.as_object_mut().and_then(|r| r.remove("tool_calls"));
        let calls = calls.as_ref().and_then(Value::as_array);
        if result["status"] != "done" || calls.is_none_or(Vec::is_empty) {
            if workspace.isolated {
                result["artifacts"]
                    .as_array_mut()
                    .expect("artifact array")
                    .push(json!({"name":"工作目录","location":canonical,"type":"workspace"}));
            }
            return Ok(result);
        }
        let calls = calls.expect("nonempty array");
        if round == 4 || calls.len() > 8 {
            return Err("external runtime exceeded the tool call limit".into());
        }
        let mut results = Vec::new();
        for call in calls {
            if Instant::now() >= until {
                return Err("runtime timeout".into());
            }
            if pulse()? {
                return Err("cancelled".into());
            }
            results.push(tools(call, &canonical)?);
        }
        context["fields"]["tool_results"] = json!(results);
    }
    Err("external runtime exceeded the tool call limit".into())
}
fn execute_round(
    spec: &Runtime,
    canonical: &Path,
    run: &Value,
    mut pulse: impl FnMut() -> Result<bool>,
    until: Instant,
    state: &Path,
) -> Result<Value> {
    if pulse()? {
        return Err("cancelled".into());
    }
    if Instant::now() >= until {
        return Err("runtime timeout".into());
    }
    let mut child = spawn_command(spec, canonical)?;
    crate::diagnostics::record(state, run, "started", Some(child.id()), None);
    let mut input = child.stdin.take().expect("piped stdin");
    let mut context = serde_json::to_vec(run).map_err(|e| e.to_string())?;
    context.push(b'\n');
    let writer = thread::spawn(move || {
        let _ = input.write_all(&context);
    });
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output = thread::spawn(move || read_output(stdout));
    let errors = thread::spawn(move || read_output(stderr));
    let mut end_reason = None;
    let succeeded = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Err(e) => {
                stop(&mut child);
                end_reason = Some(e.to_string());
                break false;
            }
            Ok(None) => {}
        }
        let reason = if Instant::now() >= until {
            Some("runtime timeout".to_owned())
        } else {
            match pulse() {
                Ok(true) => Some("cancelled".into()),
                Err(e) => Some(e),
                Ok(false) => None,
            }
        };
        if let Some(reason) = reason {
            stop(&mut child);
            end_reason = Some(reason);
            break false;
        }
        thread::sleep(Duration::from_millis(150));
    };
    crate::diagnostics::record(state, run, "process_exited", None, None);
    // Detached output producers must not indefinitely pin the supervisor's completion.
    let drain_until = Instant::now() + Duration::from_secs(2);
    while !(writer.is_finished() && output.is_finished() && errors.is_finished())
        && Instant::now() < drain_until
    {
        thread::sleep(Duration::from_millis(20));
    }
    let out = if output.is_finished() {
        output.join().unwrap_or_default()
    } else {
        String::new()
    };
    let err = if errors.is_finished() {
        errors.join().unwrap_or_default()
    } else {
        String::new()
    };
    let parsed = serde_json::from_str::<Value>(out.trim())
        .ok()
        .filter(Value::is_object)
        .unwrap_or(Value::Null);
    let default = if succeeded && parsed["error"].is_null() {
        "done"
    } else {
        "failed"
    };
    let status = if end_reason.as_deref() == Some("cancelled") {
        "cancelled"
    } else if succeeded {
        parsed["status"]
            .as_str()
            .filter(|s| matches!(*s, "done" | "failed" | "waiting" | "cancelled"))
            .unwrap_or(default)
    } else {
        default
    };
    let mut result = end_reason.unwrap_or_else(|| {
        parsed["result"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| out.trim().to_owned())
    });
    if !succeeded && !err.trim().is_empty() {
        result.push('\n');
        result.push_str(err.trim());
    }
    let artifacts = bounded_artifacts(&parsed["artifacts"], canonical)?;
    let failure = if status == "failed" {
        let stderr_error = serde_json::from_str::<Value>(err.trim()).ok();
        let provider = parsed
            .get("error")
            .or_else(|| {
                stderr_error
                    .as_ref()
                    .and_then(|v| v.get("error").or(Some(v)))
            })
            .unwrap_or(&Value::Null);
        let failure = shipforge_client::runtime_diagnostics::classify(provider, &result);
        result = failure["message"].as_str().unwrap_or_default().to_owned();
        failure
    } else {
        Value::Null
    };
    Ok(
        json!({"status":status,"result":result,"error":failure,"artifacts":artifacts,"delegates":parsed["delegates"],"tool_calls":parsed["tool_calls"],"task_draft":parsed["task_draft"],"usage":parsed["usage"]}),
    )
}
