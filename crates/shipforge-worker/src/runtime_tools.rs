//! Only this machine's configuration and built-in package definitions can select an executable.
use crate::config::{Config, Runtime, program_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Management {
    pub executable: Option<PathBuf>,
    pub probe: Option<Runtime>,
    pub install: Option<Runtime>,
    pub upgrade: Option<Runtime>,
}
const RUNTIMES: &[&str] = &["claude", "codex", "gemini", "opencode"];
const ENVIRONMENTS: &[&str] = &["node", "npm", "git", "python", "uv", "bun"];
fn names(cfg: &Config) -> BTreeSet<&str> {
    RUNTIMES
        .iter()
        .chain(ENVIRONMENTS)
        .copied()
        .chain(cfg.runtimes.keys().map(String::as_str))
        .chain(cfg.runtime_management.keys().map(String::as_str))
        .collect()
}
fn js_entry(name: &str) -> Option<(PathBuf, Vec<String>)> {
    let (package, bin) = match name {
        "gemini" => ("@google/gemini-cli", "gemini"),
        "codex" => ("@openai/codex", "codex"),
        "claude" => ("@anthropic-ai/claude-code", "claude"),
        "npm" => ("npm", "npm"),
        _ => return None,
    };
    let node = program_path(Path::new("node")).ok()?;
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
    {
        let root = dir.join("node_modules").join(package);
        let Some((root, path)) = (|| {
            let root = root.canonicalize().ok()?;
            let v: Value =
                serde_json::from_slice(&fs::read(root.join("package.json")).ok()?).ok()?;
            let entry = v["bin"].as_str().or_else(|| v["bin"][bin].as_str())?;
            let p = root.join(entry).canonicalize().ok()?;
            Some((root, p))
        })() else {
            continue;
        };
        if path.starts_with(root) && path.is_file() {
            return Some((node, vec![path.to_string_lossy().into()]));
        }
    }
    None
}
fn executable(cfg: &Config, name: &str) -> Option<(PathBuf, Vec<String>)> {
    let custom = cfg
        .runtime_management
        .get(name)
        .and_then(|m| m.executable.as_deref())
        .or_else(|| cfg.runtimes.get(name).map(|r| r.program.as_path()));
    if let Some(path) = custom {
        if let Ok(program) = program_path(path) {
            return Some((program, vec![]));
        }
        if path != Path::new(name) {
            return None;
        }
    }
    program_path(Path::new(name))
        .ok()
        .or_else(|| user_package(name))
        .map(|p| (p, vec![]))
        .or_else(|| js_entry(name))
}
fn win_package(name: &str) -> Option<&'static str> {
    match name {
        "claude" if cfg!(windows) => Some("Anthropic.ClaudeCode"),
        _ => None,
    }
}
fn user_package(name: &str) -> Option<PathBuf> {
    if !cfg!(windows) || !matches!(name, "claude" | "winget") {
        return None;
    }
    let app = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    if name == "winget" {
        return Some(app.join("Microsoft/WindowsApps/winget.exe")).filter(|p| p.is_file());
    }
    [
        Some(app.join("Microsoft/WinGet/Links/claude.exe")),
        std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".local/bin/claude.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}
fn winget() -> Option<PathBuf> {
    program_path(Path::new("winget"))
        .ok()
        .or_else(|| user_package("winget"))
}
fn snapshot(cfg: &Config, name: &str, cache: &Value) -> Value {
    let has = executable(cfg, name).is_some();
    let setup = cfg.runtime_management.get(name);
    let manual = win_package(name).is_some() && winget().is_some();
    let previous = cache.get(name).filter(|v| v["present"] == has);
    let launch = cfg
        .runtime_launch
        .get(name)
        .zip(cfg.runtimes.get(name))
        .map(|(profile, spec)| profile.public(name, spec));
    json!({"launch":launch,"name":name,"kind":if ENVIRONMENTS.contains(&name){"environment"}else{"runtime"},"status":previous.map(|v|v["status"].clone()).unwrap_or_else(||json!(if has{"available"}else{"not_detected"})),"version":previous.and_then(|v|v["version"].as_str()),"configured":cfg.runtimes.contains_key(name),"installable":setup.is_some_and(|m|m.install.is_some())||manual,"upgradable":setup.is_some_and(|m|m.upgrade.is_some())||manual,"testable":cfg.runtimes.contains_key(name)||name=="claude"})
}
pub fn inventory(cfg: &Config, state: &Path) -> Value {
    let cache = read_cache(state);
    Value::Array(
        names(cfg)
            .into_iter()
            .map(|name| snapshot(cfg, name, &cache))
            .collect(),
    )
}
fn read_cache(state: &Path) -> Value {
    fs::read(state.join("runtime-versions.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}))
}
fn bounded(mut stream: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut scratch = [0u8; 4096];
    while let Ok(count) = stream.read(&mut scratch) {
        if count == 0 {
            break;
        }
        let take = count.min(65536 - bytes.len());
        bytes.extend_from_slice(&scratch[..take]);
    }
    bytes
}
fn invoke(
    runtime: &Runtime,
    input: Option<Vec<u8>>,
    temp: &Path,
    state: &Path,
    seconds: u64,
) -> Result<Vec<u8>, &'static str> {
    let mut cmd = Command::new(program_path(&runtime.program).map_err(|_| "not_detected")?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.env_clear()
        .current_dir(temp)
        .args(&runtime.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "PATHEXT",
        "COMSPEC",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "HOME",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMFILES",
        "LANG",
    ]
    .iter()
    .copied()
    .chain(runtime.inherit_env.iter().map(String::as_str))
    {
        if !key.to_ascii_uppercase().starts_with("SHIPFORGE_")
            && let Some(v) = std::env::var_os(key)
        {
            cmd.env(key, v);
        }
    }
    crate::service::hidden(&mut cmd);
    let mut child = cmd.spawn().map_err(|_| "failed")?;
    let stdout = child.stdout.take().ok_or("failed")?;
    let stderr = child.stderr.take().ok_or("failed")?;
    let a = thread::spawn(move || bounded(stdout));
    let b = thread::spawn(move || bounded(stderr));
    if let Some(mut pipe) = child.stdin.take()
        && let Some(payload) = input
    {
        let _ = pipe.write_all(&payload);
    }
    let limit = Instant::now() + Duration::from_secs(seconds.min(runtime.timeout_seconds));
    let status = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break Ok(exit),
            Ok(None) => {}
            Err(_) => break Err("failed"),
        }
        let stopping =
            fs::read(state.join("stop.request")).is_ok_and(|bytes| bytes == b"immediate");
        if stopping || Instant::now() >= limit {
            crate::process::stop(&mut child);
            let _ = child.wait();
            break Err(if stopping { "failed" } else { "timeout" });
        }
        thread::sleep(Duration::from_millis(75));
    };
    let out = a.join().map_err(|_| "failed")?;
    let err = b.join().map_err(|_| "failed")?;
    if status?.success() {
        return Ok(out);
    }
    let text = String::from_utf8_lossy(&err).to_lowercase();
    Err(
        if [
            "login",
            "log in",
            "unauthorized",
            "authentication",
            "not authenticated",
            "api key",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            "login_required"
        } else {
            "failed"
        },
    )
}
fn command(program: PathBuf, args: Vec<String>) -> Runtime {
    Runtime {
        program,
        args,
        slash_commands: None,
        inherit_env: Vec::new(),
        timeout_seconds: 600,
    }
}
fn version(text: &[u8]) -> Option<String> {
    String::from_utf8_lossy(text)
        .split(|c: char| !(c.is_ascii_alphanumeric() || ".-+".contains(c)))
        .map(|s| s.trim_start_matches('v'))
        .find_map(|s| {
            let numeric = s.split(['-', '+']).next()?;
            let parts: Vec<_> = numeric.split('.').collect();
            (s.len() <= 48
                && (2..=4).contains(&parts.len())
                && parts.iter().all(|p| {
                    !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_digit())
                }))
            .then(|| numeric.to_string())
        })
}
fn probe(cfg: &Config, state: &Path, name: &str, temp: &Path) -> Result<(), &'static str> {
    let executable = executable(cfg, name);
    let present = executable.is_some();
    let test = cfg
        .runtime_management
        .get(name)
        .and_then(|m| m.probe.clone())
        .or_else(|| {
            executable.map(|(program, mut args)| {
                args.push("--version".into());
                command(program, args)
            })
        });
    let result = if present {
        test.ok_or("not_detected")
            .and_then(|spec| invoke(&spec, None, temp, state, 8))
            .and_then(|out| version(&out).ok_or("failed"))
    } else {
        Err("not_detected")
    };
    let mut cache = read_cache(state);
    cache[name] = json!({"present":present,"status":if !present{"not_detected"}else if result.is_ok(){"available"}else{"detection_failed"},"version":result.as_ref().ok()});
    crate::service::save(&state.join("runtime-versions.json"), &cache).map_err(|_| "failed")?;
    result.map(|_| ())
}
fn install(cfg: &Config, name: &str, action: &str) -> Result<Runtime, &'static str> {
    if let Some(spec) = cfg.runtime_management.get(name).and_then(|m| {
        if action == "install" {
            m.install.as_ref()
        } else {
            m.upgrade.as_ref()
        }
    }) {
        return Ok(spec.clone());
    }
    let package = win_package(name).ok_or("unavailable")?;
    Ok(command(
        winget().ok_or("manager_unavailable")?,
        [
            action,
            "--id",
            package,
            "--exact",
            "--source",
            "winget",
            "--scope",
            "user",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]
        .map(str::to_owned)
        .to_vec(),
    ))
}
fn connection(cfg: &Config, state: &Path, name: &str, temp: &Path) -> Result<(), &'static str> {
    let token = format!("SHIPFORGE_{}", Uuid::new_v4().simple());
    let prompt = format!("Reply with exactly {token}. Do not use tools.");
    let output = if let Some(spec) = cfg.runtimes.get(name) {
        let payload = json!({"kind":"run","id":"runtime-connection-test","fields":{"instruction":prompt,"task":{"title":prompt},"connection_test":true}});
        let out = invoke(
            spec,
            Some(payload.to_string().into_bytes()),
            temp,
            state,
            60,
        )?;
        let value: Value = serde_json::from_slice(&out).map_err(|_| "unexpected_reply")?;
        if value["status"] != "done" {
            return Err("unexpected_reply");
        }
        value["result"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string()
    } else if name == "claude" {
        let (program, mut args) = executable(cfg, name).ok_or("not_detected")?;
        args.extend(["-p", "--tools", "", "--output-format", "text", &prompt].map(str::to_owned));
        String::from_utf8_lossy(&invoke(&command(program, args), None, temp, state, 60)?)
            .trim()
            .to_string()
    } else {
        return Err("unavailable");
    };
    if token == output {
        Ok(())
    } else {
        Err("unexpected_reply")
    }
}
pub fn perform(cfg: &Config, state: &Path, name: &str, action: &str) -> &'static str {
    if !names(cfg).contains(name) {
        return "unavailable";
    }
    let result = (|| {
        let dir = tempfile::tempdir_in(state).map_err(|_| "failed")?;
        match action {
            "rescan" => probe(cfg, state, name, dir.path()),
            "install" | "upgrade" => {
                let spec = install(cfg, name, action)?;
                invoke(&spec, None, dir.path(), state, 600)?;
                probe(cfg, state, name, dir.path())
            }
            "test" => connection(cfg, state, name, dir.path()),
            _ => Err("unavailable"),
        }
    })();
    result.err().unwrap_or("success")
}
