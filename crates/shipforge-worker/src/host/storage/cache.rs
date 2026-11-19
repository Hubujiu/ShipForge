use super::*;

pub(crate) fn mark_cache_used(state: &Path, key: &str) -> Result<()> {
    if !hex(key, 64) {
        return Err("invalid cache key".into());
    }
    let stamp = ledger(state)?.join(format!("cache-{key}.json"));
    crate::service::save(&stamp, &json!({"cache":key}))
}
pub(super) fn modified_ms(path: &Path) -> Option<i64> {
    let meta = fs::symlink_metadata(path).ok()?;
    if reparse(&meta) {
        return None;
    }
    i64::try_from(
        meta.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}
#[cfg(windows)]
pub(super) fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::PCWSTR};
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut value = 0;
    // The path is NUL terminated and the output lives until the API returns.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut value), None, None) }.ok()?;
    Some(value)
}
#[cfg(unix)]
pub(super) fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // A successful statvfs initializes every field read below.
    if unsafe { libc::statvfs(name.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    u64::try_from(u128::from(stats.f_bavail) * u128::from(stats.f_frsize)).ok()
}
fn pending(state: &Path) -> Result<bool> {
    let dir = state.join("pending");
    Ok(dir.is_dir()
        && listing(&dir)?
            .iter()
            .any(|p| p.extension().is_some_and(|s| s == "json")))
}
fn protection(host: &Host, state: &Path, path: &Path) -> Option<&'static str> {
    if source(&host.cfg, state, path).is_err() {
        return Some("bound_repository");
    }
    if !managed_cache(path) {
        return Some("unverified");
    }
    if pending(state).unwrap_or(true) {
        return Some("pending");
    }
    let worktrees = path.join("worktrees");
    if worktrees.exists() && !listing(&worktrees).is_ok_and(|items| items.is_empty()) {
        return Some("attached_worktree");
    }
    if path.join("objects/info/alternates").exists() {
        return Some("unverified");
    }
    if !listing(path).is_ok_and(|items| {
        items.iter().all(|item| {
            item.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| {
                    matches!(
                        name,
                        "HEAD"
                            | "config"
                            | "description"
                            | "hooks"
                            | "info"
                            | "objects"
                            | "refs"
                            | "packed-refs"
                            | "FETCH_HEAD"
                            | "worktrees"
                            | "branches"
                            | "logs"
                    )
                })
        })
    }) {
        return Some("local_changes");
    }
    let hooks = path.join("hooks");
    if hooks.is_dir()
        && !listing(&hooks).is_ok_and(|items| {
            items
                .iter()
                .all(|p| p.extension().is_some_and(|x| x == "sample"))
        })
    {
        return Some("local_changes");
    }
    let Ok(refs) = git(path, &["for-each-ref", "--format=%(refname)"], &[]) else {
        return Some("unverified");
    };
    if !refs.lines().any(|r| r == "refs/shipforge/default")
        || !refs.lines().all(|r| {
            r == "refs/shipforge/default"
                || r.starts_with("refs/remotes/origin/")
                || r.strip_prefix("refs/heads/shipforge/")
                    .is_some_and(|id| hex(id, 32))
        })
    {
        return Some("local_changes");
    }
    if !git(
        path,
        &[
            "rev-list",
            "--max-count=1",
            "--branches",
            "--not",
            "--remotes=origin",
            "refs/shipforge/default",
        ],
        &[],
    )
    .is_ok_and(|s| s.is_empty())
        || !git(
            path,
            &["fsck", "--no-progress", "--no-reflogs", "--unreachable"],
            &[],
        )
        .is_ok_and(|s| s.is_empty())
    {
        return Some("local_changes");
    }
    None
}
pub(super) fn inspect(
    host: &Host,
    state: &Path,
    path: &Path,
    key: &str,
    held: bool,
) -> Result<Value> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| hex(n, 64))
        .ok_or("invalid cache")?;
    let cache = verified_dir(&state.join("git-cache"))?;
    if path.parent() != Some(cache.as_path()) || verified_dir(path)? != path {
        return Err("cache location changed".into());
    }
    let lock = file_lock(&cache.join(format!("{name}.lock")))?;
    let busy = !held && lock.try_lock().is_err();
    let stamp = ledger(state)?.join(format!("cache-{name}.json"));
    let last = modified_ms(&stamp);
    let mut hash = Sha256::new();
    hash.update(key.as_bytes());
    hash.update(last.unwrap_or(0).to_le_bytes());
    let mut usage = Usage::default();
    let read = walk(path, &mut usage, (!busy).then_some(&mut hash));
    let guard = if busy {
        Some("cache_in_use")
    } else if read.is_err() {
        Some("unreadable")
    } else if usage.links || usage.nested_repo {
        Some("links")
    } else {
        protection(host, state, path)
    };
    Ok(
        json!({"id":key,"category":"cache","bytes":usage.bytes,"files":usage.files,"snapshot":crate::config::hex_digest(hash.finalize()),"runs":[],"protection":guard,"safe_for_schedule":guard.is_none() && last.is_some(),"last_used_at":last}),
    )
}
pub(super) fn clean(
    host: &Host,
    client: &mut Client,
    job: &Value,
    state: &Path,
    path: &Path,
    key: &str,
) -> Result<&'static str> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("cache key required")?;
    let root = verified_dir(&state.join("git-cache"))?;
    if path.parent() != Some(root.as_path()) || !hex(name, 64) {
        return Err("cache location changed".into());
    }
    let lock = file_lock(&root.join(format!("{name}.lock")))?;
    lock.try_lock().map_err(text_error)?;
    let item = inspect(host, state, path, key, true)?;
    let cmd = &job["command"];
    if !item["protection"].is_null()
        || item["snapshot"] != cmd["expected_snapshot"]
        || !cmd["cycle"].is_null() && item["safe_for_schedule"] != true
    {
        return Err("cache is no longer reclaimable".into());
    }
    let trash = state.join("storage-trash");
    fs::create_dir_all(&trash).map_err(text_error)?;
    if verified_dir(&trash)? != trash {
        return Err("invalid trash directory".into());
    }
    host.call(
        client,
        json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]}),
    )?;
    if verified_dir(path)? != path
        || verified_dir(&root)? != root
        || verified_dir(&trash)? != trash
        || pending(state)?
    {
        return Err("cache changed".into());
    }
    let retired = trash.join(Uuid::new_v4().simple().to_string());
    let receipt = retired.with_extension("confirmed");
    crate::config::write_new(&receipt, key.as_bytes())?;
    if let Err(e) = fs::rename(path, &retired) {
        let _ = fs::remove_file(&receipt);
        return Err(e.to_string());
    }
    let _ = fs::remove_file(ledger(state)?.join(format!("cache-{name}.json")));
    if purge_owned(&host.cfg, state, &trash, &retired).is_err() {
        return Ok("reclaim_pending");
    }
    let _ = fs::remove_file(receipt);
    Ok("success")
}
