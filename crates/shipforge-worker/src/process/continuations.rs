use super::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Checkpoint {
    path: PathBuf,
    isolated: bool,
    local_repository: bool,
}

fn slot(state: &Path, id: &str, attempt: &Value) -> Result<PathBuf> {
    let run = Uuid::parse_str(id).map_err(|_| "invalid conversation run ID")?;
    let attempt = attempt
        .as_u64()
        .filter(|attempt| *attempt > 0)
        .ok_or("invalid conversation run attempt")?;
    let directory = state.join("conversation-checkpoints");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let directory = crew_workspace::checked_dir(&directory)?;
    let state = state.canonicalize().map_err(|e| e.to_string())?;
    if directory.parent() != Some(state.as_path()) {
        return Err("invalid machine conversation checkpoint directory".into());
    }
    Ok(directory.join(format!("{run}-{attempt}.json")))
}

fn git_value(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(program_path(Path::new("git")).ok()?)
        .arg("-C")
        .arg(dunce::simplified(path))
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
}

pub(super) fn record(state: &Path, run: &Value, workspace: &WorkingDir) -> Option<Value> {
    if !run["fields"]["chat_id"].is_string()
        || !matches!(
            run["fields"]["handler_kind"].as_str(),
            Some("agent" | "crew")
        )
    {
        return None;
    }
    let path = workspace.path.canonicalize().ok()?;
    let git = git_value(&path, &["rev-parse", "--is-inside-work-tree"])
        .is_some_and(|value| value == "true");
    let origin = git
        .then(|| git_value(&path, &["config", "--local", "--get", "remote.origin.url"]))
        .flatten();
    let checkpoint = Checkpoint {
        path,
        isolated: workspace.isolated,
        local_repository: workspace.local_repository,
    };
    let file = slot(state, run["id"].as_str()?, &run["fields"]["attempt"]).ok()?;
    crate::service::save(&file, &json!(checkpoint)).ok()?;
    let repository = origin.and_then(|address| shipforge_client::git_remote_url(&address).ok());
    Some(json!({"git":git,"repository":repository}))
}

fn owned_checkout(state: &Path, path: &Path) -> Result<Option<PathBuf>> {
    let state = state.canonicalize().map_err(|e| e.to_string())?;
    let relative = path
        .strip_prefix(&state)
        .map_err(|_| "managed conversation workspace moved")?;
    let parts = relative
        .iter()
        .map(|item| item.to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or("invalid managed checkout path")?;
    let hex = |value: &str, size| {
        value.len() == size && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    match parts.as_slice() {
        ["work", run] if run.strip_prefix("run-").is_some_and(|id| hex(id, 32)) => Ok(None),
        ["execution-workspaces" | "crew-workspaces", key, id] if hex(key, 64) && hex(id, 32) => {
            Ok(path.parent().map(|parent| parent.join("initialize.lock")))
        }
        _ => Err("original workspace is no longer a managed checkout".into()),
    }
}

pub(super) fn resume(
    cfg: &Config,
    state: &Path,
    run: &Value,
    pulse: &mut impl FnMut() -> Result<bool>,
    until: Instant,
) -> Result<WorkingDir> {
    let plan = &run["fields"]["directory_fork"];
    if plan["machine"] != cfg.machine_id {
        return Err("the source conversation workspace belongs to a different machine".into());
    }
    if plan["source_machine"] != cfg.machine_id {
        return remote(cfg, state, run, plan, pulse, until);
    }
    let id = plan["source_run"]
        .as_str()
        .ok_or("missing source conversation workspace")?;
    let checkpoint = slot(state, id, &plan["source_attempt"])?;
    let meta = fs::symlink_metadata(&checkpoint)
        .map_err(|_| "original workspace is not available on this machine")?;
    if !meta.is_file() || meta.len() > 65_536 {
        return Err("invalid local conversation checkpoint file".into());
    }
    let saved: Checkpoint = serde_json::from_slice(
        &fs::read(checkpoint).map_err(|_| "original workspace is not available on this machine")?,
    )
    .map_err(|_| "invalid conversation checkpoint")?;
    let source = crew_workspace::checked_dir(&saved.path)?;
    if source != saved.path {
        return Err("original workspace moved after checkpoint".into());
    }
    let scoped_lock = if saved.isolated {
        owned_checkout(state, &source)?
    } else {
        let approved = cfg.profile_paths().any(|(id, dir)| {
            cfg.permits_direct(id) && dir.canonicalize().is_ok_and(|path| path == source)
        });
        if !approved {
            return Err("original directory is no longer approved on this machine".into());
        }
        None
    };
    let lock = if let Some(path) = scoped_lock {
        OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|e| e.to_string())?
    } else {
        directory_lock(&crate::config::directory_identity(&source))?
    };
    repositories::wait(&lock, pulse, until)?;
    if plan["mode"] == "reuse" {
        return Ok(WorkingDir {
            path: source,
            _lock: Some(lock),
            isolated: saved.isolated,
            local_repository: saved.local_repository,
        });
    }
    if plan["mode"] != "branch" {
        return Err("unknown continuation mode".into());
    }
    let revision = if let Some(branch) = plan["checkout_branch"].as_str() {
        [
            format!("refs/heads/{branch}"),
            format!("refs/remotes/origin/{branch}"),
        ]
        .into_iter()
        .find(|r| {
            git_value(
                &source,
                &["rev-parse", "--verify", &format!("{r}^{{commit}}")],
            )
            .is_some()
        })
        .ok_or("the baseline branch is unavailable on this machine")?
    } else {
        "HEAD".into()
    };
    let root = state.join("work");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let local_id = Uuid::new_v4().simple();
    let path = crew_workspace::checked_dir(&root)?.join(format!("run-{local_id}"));
    let branch = format!("shipforge/fork-{local_id}");
    git_at_revision(
        &source,
        &["worktree", "add", "-b", &branch],
        &path,
        &revision,
    )?;
    Ok(WorkingDir {
        path,
        _lock: None,
        isolated: true,
        local_repository: saved.local_repository,
    })
}

fn remote(
    cfg: &Config,
    state: &Path,
    run: &Value,
    plan: &Value,
    pulse: &mut impl FnMut() -> Result<bool>,
    until: Instant,
) -> Result<WorkingDir> {
    let url = plan["repository"]
        .as_str()
        .ok_or("source Git repository unavailable for another machine")?;
    let branch = plan["checkout_branch"]
        .as_str()
        .ok_or("choose a branch available on the target machine")?;
    let declared = run["git"]["repositories"]
        .as_array()
        .is_some_and(|items| items.iter().any(|repo| repo["url"] == url));
    let runtime = &run["fields"]["config"];
    let selected = runtime["profiles"]
        .as_array()
        .and_then(|ids| ids.first())
        .and_then(Value::as_str)
        .or_else(|| runtime["profile"].as_str())
        .filter(|id| !id.is_empty());
    let approved = runtime["directory_mode"] != "temporary"
        && cfg
            .profile_paths()
            .find(|(id, _)| *id == selected.unwrap_or("@workspace"))
            .and_then(|(_, dir)| {
                git_value(dir, &["config", "--local", "--get", "remote.origin.url"])
            })
            .and_then(|remote| shipforge_client::git_remote_url(&remote).ok())
            .is_some_and(|address| address == url);
    if !declared && !approved {
        return Err(
            "the target machine does not have the original Git repository configured".into(),
        );
    }
    let root = state.join("work");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = crew_workspace::checked_dir(&root)?.join(format!("run-{}", Uuid::new_v4().simple()));
    repositories::checkout(
        state,
        &path,
        &json!([{"url":url,"branch":branch}]),
        pulse,
        until,
    )?;
    Ok(WorkingDir {
        path,
        _lock: None,
        isolated: true,
        local_repository: false,
    })
}
