use super::*;

fn checked_dir(dir: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("crew workspace cannot be a reparse point".into());
        }
    }
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("invalid crew workspace directory".into());
    }
    dir.canonicalize().map_err(|e| e.to_string())
}

pub(super) fn open(
    cfg: &Config,
    run: &Value,
    state: &Path,
    source: Option<&Path>,
    pulse: &mut impl FnMut() -> Result<bool>,
    timeout: Duration,
) -> Result<WorkingDir> {
    let scope = &run["fields"]["crew_workspace"];
    let token = scope["token"]
        .as_str()
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("invalid crew workspace namespace")?;
    if run["space"] != cfg.space
        || run["fields"]["machine_id"] != cfg.machine_id
        || !scope["crew"].is_string()
        || !scope["task"].is_string()
        || scope["task"] != run["fields"]["task_id"]
    {
        return Err("crew workspace does not match this machine, space or task".into());
    }
    let state = checked_dir(state)?;
    let root = state.join("crew-workspaces");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let root = checked_dir(&root)?;
    if source.is_some_and(|p| root.starts_with(p)) {
        return Err("keep machine configuration outside the bound repository".into());
    }
    let key = crate::config::hex_digest(
        json!([
            cfg.space,
            cfg.machine_id,
            scope["crew"],
            scope["task"],
            token,
            source
        ])
        .to_string()
        .as_bytes(),
    );
    let bucket = root.join(key);
    fs::create_dir_all(&bucket).map_err(|e| e.to_string())?;
    let bucket = checked_dir(&bucket)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(bucket.join("initialize.lock"))
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + timeout;
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < until => {
                if pulse()? {
                    return Err("cancelled".into());
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("initialize shared workspace: {e}")),
        }
    }
    let record = bucket.join("ready");
    let id = match fs::read_to_string(&record) {
        Ok(id) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => id,
        Ok(_) => return Err("invalid local crew workspace record".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = Uuid::new_v4().simple().to_string();
            let path = bucket.join(&id);
            if let Some(source) = source {
                git_at(source, &["worktree", "add", "--detach"], &path)?;
            } else {
                fs::create_dir(&path).map_err(|e| e.to_string())?;
            }
            let temp = bucket.join(format!("ready-{id}"));
            crate::config::write_new(&temp, id.as_bytes())?;
            fs::rename(temp, record).map_err(|e| e.to_string())?;
            id
        }
        Err(e) => return Err(e.to_string()),
    };
    let path = checked_dir(&bucket.join(id))?;
    if path.parent() != Some(bucket.as_path()) {
        return Err("invalid local crew checkout".into());
    }
    // Only initialization is serialized. Delegated jobs can run concurrently in the published checkout.
    Ok(WorkingDir {
        path,
        _lock: None,
        isolated: true,
    })
}
