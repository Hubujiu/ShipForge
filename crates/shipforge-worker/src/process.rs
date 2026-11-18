use crate::config::{Config, Result, Runtime, program_path};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

struct WorkingDir {
    path: PathBuf,
    _lock: Option<File>,
    isolated: bool,
}
fn git_at(source: &Path, args: &[&str], destination: &Path) -> Result<()> {
    let out = Command::new(program_path(Path::new("git"))?)
        .arg("-C")
        .arg(source)
        .args(args)
        .arg(destination)
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
    let source = if let Some(profile) = settings["profile"].as_str().filter(|n| !n.is_empty()) {
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
    if settings["directory_mode"] == "direct" {
        if !cfg.allow_direct {
            return Err("direct directory execution is disabled by machine configuration".into());
        }
        let path = source.ok_or("direct execution needs an approved local workspace")?;
        if !path.is_dir() {
            return Err("approved source is not a directory".into());
        }
        let locks = std::env::temp_dir().join("shipforge-directory-locks");
        fs::create_dir_all(&locks).map_err(|e| e.to_string())?;
        let name = path.to_string_lossy();
        #[cfg(windows)]
        let name = name.to_lowercase();
        let name = format!("{}.lock", crate::config::hex_digest(name.as_bytes()));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(locks.join(name))
            .map_err(|e| e.to_string())?;
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
    let mut items = Vec::new();
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
            items.push(json!({"name":entry["name"].as_str().unwrap_or(raw),"path":raw,"location":file,"bytes":file.metadata().map_err(|e|e.to_string())?.len()}));
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
                run["fields"]["config"]["timeout_seconds"]
                    .as_u64()
                    .unwrap_or(spec.timeout_seconds),
            )
            .max(1),
    );
    let began = Instant::now();
    let workspace = directory(config, run, state, &mut pulse, timeout)?;
    let canonical = workspace.path.canonicalize().map_err(|e| e.to_string())?;
    let until = began + timeout;
    let mut context = run.clone();
    for round in 0..=4 {
        if pulse()? {
            return Err("cancelled".into());
        }
        let mut result = execute_round(spec, &canonical, &context, &mut pulse, until)?;
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
) -> Result<Value> {
    if pulse()? {
        return Err("cancelled".into());
    }
    if Instant::now() >= until {
        return Err("runtime timeout".into());
    }
    let mut child = spawn_command(spec, canonical)?;
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
    let default = if succeeded { "done" } else { "failed" };
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
    Ok(
        json!({"status":status,"result":result,"artifacts":artifacts,"delegates":parsed["delegates"],"tool_calls":parsed["tool_calls"]}),
    )
}
