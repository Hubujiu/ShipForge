use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Component,
    process::Command,
};

fn text_error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn file_lock(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(text_error)
}
fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}
fn verified_dir(path: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(path).map_err(text_error)?;
    if !meta.is_dir() || reparse(&meta) {
        return Err("unverified directory".into());
    }
    path.canonicalize().map_err(text_error)
}
fn ledger(state: &Path) -> Result<PathBuf> {
    let state = verified_dir(state)?;
    let journal = state.join("storage-index");
    fs::create_dir_all(&journal).map_err(text_error)?;
    let journal = verified_dir(&journal)?;
    if journal.parent() != Some(state.as_path()) {
        return Err("invalid storage journal".into());
    }
    Ok(journal)
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn shape(state: &Path, path: &Path) -> Option<(&'static str, String)> {
    let rel = path.strip_prefix(state).ok()?;
    let parts: Vec<_> = rel
        .components()
        .map(|p| {
            if let Component::Normal(s) = p {
                s.to_str()
            } else {
                None
            }
        })
        .collect::<Option<_>>()?;
    let kind = match parts.as_slice() {
        ["work", run] if run.strip_prefix("run-").is_some_and(|id| hex(id, 32)) => "run",
        ["crew-workspaces", key, id] if hex(key, 64) && hex(id, 32) => "crew",
        ["execution-workspaces", key, id] if hex(key, 64) && hex(id, 32) => "task",
        _ => return None,
    };
    Some((
        kind,
        crate::config::hex_digest(rel.as_os_str().as_encoded_bytes()),
    ))
}
pub(crate) fn protect_run(state: &Path, checkout: &Path, id: &str) -> Result<File> {
    let state = verified_dir(state)?;
    let (_, key) = shape(&state, checkout).ok_or("invalid owned working directory")?;
    let journal = ledger(&state)?;
    let lock = file_lock(&journal.join(format!("{key}.lock")))?;
    lock.lock_shared().map_err(text_error)?;
    let metadata = file_lock(&journal.join(format!("{key}.metadata.lock")))?;
    metadata.lock().map_err(text_error)?;
    let file = journal.join(format!("{key}.json"));
    let mut ids = known(&file)?;
    ids.insert(id.to_owned());
    crate::service::save(&file, &json!(ids))?;
    Ok(lock)
}
fn known(path: &Path) -> Result<BTreeSet<String>> {
    match fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map_err(text_error),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(e.to_string()),
    }
}
pub(crate) fn mark_process(state: &Path, path: &Path, pid: u32) -> Result<Option<PathBuf>> {
    let state = verified_dir(state)?;
    let Some((_, key)) = shape(&state, path) else {
        return Ok(None);
    };
    let journal = ledger(&state)?;
    let active = journal.join(format!("{key}.active"));
    fs::create_dir_all(&active).map_err(text_error)?;
    if verified_dir(&active)? != active {
        return Err("invalid checkout process journal".into());
    }
    let marker = active.join(format!("{pid}-{}.json", Uuid::new_v4().simple()));
    crate::config::write_new(&marker, pid.to_string().as_bytes())?;
    Ok(Some(marker))
}
fn listing(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut children = fs::read_dir(dir)
        .map_err(text_error)?
        .map(|e| e.map(|e| e.path()).map_err(text_error))
        .collect::<Result<Vec<_>>>()?;
    children.sort();
    Ok(children)
}
#[derive(Default)]
struct Usage {
    bytes: u64,
    files: u64,
    links: bool,
    nested_repo: bool,
}
fn walk(root: &Path, usage: &mut Usage, mut digest: Option<&mut Sha256>) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    let mut buffer = [0; 32768];
    while let Some(p) = stack.pop() {
        let m = match fs::symlink_metadata(&p) {
            Ok(v) => v,
            Err(e) if digest.is_none() && e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        if p.file_name().is_some_and(|s| s == ".git") && p.parent() != Some(root) {
            usage.nested_repo = true;
        }
        if let Some(h) = digest.as_deref_mut() {
            let rel = p
                .strip_prefix(root)
                .map_err(text_error)?
                .as_os_str()
                .as_encoded_bytes();
            h.update((rel.len() as u64).to_le_bytes());
            h.update(rel);
            h.update(m.len().to_le_bytes());
        }
        if reparse(&m) {
            usage.links = true;
            continue;
        }
        if m.is_dir() {
            stack.extend(listing(&p)?);
        } else if m.is_file() {
            usage.bytes = usage.bytes.saturating_add(m.len());
            usage.files += 1;
            if let Some(h) = digest.as_deref_mut() {
                let mut f = File::open(p).map_err(text_error)?;
                loop {
                    let n = f.read(&mut buffer).map_err(text_error)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buffer[..n]);
                }
            }
        } else {
            usage.links = true;
        }
    }
    Ok(())
}
fn git(dir: &Path, args: &[&str], paths: &[&Path]) -> Result<String> {
    let mut cmd = Command::new(program_path(Path::new("git"))?);
    crate::service::hidden(&mut cmd);
    let out = cmd
        .arg("-C")
        .arg(dunce::simplified(dir))
        .args(args)
        .args(paths.iter().map(|p| dunce::simplified(p)))
        .output()
        .map_err(text_error)?;
    if !out.status.success() {
        return Err("git did not authorize this exact worktree operation".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().into())
}
fn source(cfg: &Config, path: &Path) -> std::result::Result<Option<PathBuf>, &'static str> {
    let bound: Vec<_> = cfg
        .profile_paths()
        .map(|(_, p)| p.canonicalize().map_err(|_| "unverified"))
        .collect::<std::result::Result<_, _>>()?;
    if bound
        .iter()
        .any(|p| p.starts_with(path) || path.starts_with(p))
    {
        return Err("bound_repository");
    }
    if let Ok(data) = shipforge_client::data_dir() {
        for other in crate::service::known_configs(&data).map_err(|_| "unverified")? {
            let settings = Config::load(&other).map_err(|_| "unverified")?;
            if settings.profile_paths().any(|(_, p)| {
                p.canonicalize()
                    .is_ok_and(|target| target.starts_with(path) || path.starts_with(target))
            }) {
                return Err("bound_repository");
            }
        }
    }
    let gitfile = path.join(".git");
    let meta = match fs::symlink_metadata(&gitfile) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => return Err("unverified"),
    };
    if !meta.is_file() || reparse(&meta) {
        return Err("bound_repository");
    }
    let common = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        &[],
    )
    .ok()
    .and_then(|s| PathBuf::from(s).canonicalize().ok())
    .ok_or("unverified")?;
    let admin = git(path, &["rev-parse", "--absolute-git-dir"], &[])
        .ok()
        .and_then(|s| PathBuf::from(s).canonicalize().ok())
        .ok_or("unverified")?;
    if admin.parent() != Some(common.join("worktrees").as_path()) || admin.join("locked").exists() {
        return Err("git_locked");
    }
    let backlink = fs::read_to_string(admin.join("gitdir"))
        .ok()
        .and_then(|s| Path::new(s.trim()).canonicalize().ok());
    if backlink != gitfile.canonicalize().ok() {
        return Err("unverified");
    }
    bound
        .into_iter()
        .find(|p| {
            git(
                p,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                &[],
            )
            .ok()
            .and_then(|s| PathBuf::from(s).canonicalize().ok())
                == Some(common.clone())
        })
        .map(Some)
        .ok_or("unverified")
}
fn candidates(state: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for area in ["work", "execution-workspaces", "crew-workspaces"] {
        let root = state.join(area);
        if !root.exists() {
            continue;
        }
        if verified_dir(&root).as_deref() != Ok(root.as_path()) {
            continue;
        }
        for bucket in listing(&root)? {
            if verified_dir(&bucket).as_deref() != Ok(bucket.as_path()) {
                continue;
            }
            if area == "work" {
                if shape(state, &bucket).is_some() {
                    out.push(bucket);
                }
                continue;
            }
            for entry in listing(&bucket)? {
                if shape(state, &entry).is_some()
                    && verified_dir(&entry).as_deref() == Ok(entry.as_path())
                {
                    out.push(entry);
                }
            }
        }
    }
    Ok(out)
}
fn legacy(v: &Value) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let mut m = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for a in v.as_array().into_iter().flatten() {
        if let (Some(id), Some(p)) = (a["run"].as_str(), a["location"].as_str())
            && let Ok(p) = Path::new(p).canonicalize()
        {
            m.entry(p).or_default().insert(id.to_owned());
        }
    }
    m
}
fn inspect(
    host: &Host,
    path: &Path,
    history: &BTreeMap<PathBuf, BTreeSet<String>>,
    held: bool,
) -> Result<Value> {
    let state = verified_dir(&host.dir)?;
    let (kind, key) = shape(&state, path).ok_or("invalid storage checkout")?;
    let journal = ledger(&state)?;
    let tracked = known(&journal.join(format!("{key}.json")));
    let mut ids = tracked.clone().unwrap_or_default();
    ids.extend(history.get(path).into_iter().flatten().cloned());
    let lock = file_lock(&journal.join(format!("{key}.lock")))?;
    let busy = !held && lock.try_lock().is_err();
    let processes = journal.join(format!("{key}.active"));
    let interrupted = processes.is_dir()
        && (verified_dir(&processes).as_deref() != Ok(processes.as_path())
            || !listing(&processes)?.is_empty());
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    let mut usage = Usage::default();
    let read = walk(path, &mut usage, (!busy && !interrupted).then_some(&mut h));
    let reason = if busy {
        Some("active")
    } else if interrupted {
        Some("recovery")
    } else if read.is_err() {
        Some("unreadable")
    } else if usage.links {
        Some("links")
    } else if usage.nested_repo {
        Some("bound_repository")
    } else if tracked.is_err() || ids.is_empty() || ids.len() > 128 {
        Some("unverified")
    } else if state.join("pending").is_dir()
        && listing(&state.join("pending"))?
            .iter()
            .any(|p| p.extension().is_some_and(|v| v == "json"))
    {
        Some("pending")
    } else {
        source(&host.cfg, path).err()
    };
    Ok(
        json!({"id":key,"category":kind,"bytes":usage.bytes,"files":usage.files,"snapshot":h.finalize().iter().map(|b|format!("{b:02x}")).collect::<String>(),"runs":ids.into_iter().take(128).collect::<Vec<_>>(),"protection":reason}),
    )
}
fn scan(host: &Host, history: &BTreeMap<PathBuf, BTreeSet<String>>) -> Result<Value> {
    let state = verified_dir(&host.dir)?;
    let mut total = Usage::default();
    walk(&state, &mut total, None)?;
    let paths = candidates(&state)?;
    let count = paths.len();
    let entries = paths
        .iter()
        .take(2048)
        .map(|p| inspect(host, p, history, false))
        .collect::<Result<Vec<_>>>()?;
    let size = entries
        .iter()
        .filter_map(|v| v["bytes"].as_u64())
        .fold(0u64, u64::saturating_add);
    let mut retired = Usage::default();
    let trash = state.join("storage-trash");
    if trash.is_dir() {
        walk(&trash, &mut retired, None)?;
    }
    Ok(
        json!({"entries":entries,"total_bytes":total.bytes,"other_bytes":total.bytes.saturating_sub(size),"pending_reclaim_bytes":retired.bytes,"omitted":count.saturating_sub(2048)}),
    )
}
fn purge_owned(cfg: &Config, trash: &Path, target: &Path) -> Result<()> {
    let root = verified_dir(trash)?;
    let checked = verified_dir(target)?;
    if !target
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| hex(s, 32))
        || checked.parent() != Some(root.as_path())
        || checked != target
    {
        return Err("unexpected quarantine target".into());
    }
    let mut usage = Usage::default();
    walk(&checked, &mut usage, None)?;
    if usage.links || usage.nested_repo {
        return Err("quarantined worktree contains protected links or repositories".into());
    }
    if let Some(repo) = source(cfg, &checked).map_err(str::to_owned)? {
        git(&repo, &["worktree", "remove", "--force"], &[&checked])?;
    } else {
        fs::remove_dir_all(&checked).map_err(text_error)?;
    }
    Ok(())
}
fn resume_purge(host: &Host) -> Result<()> {
    let _bindings = crate::service::bindings_guard(&shipforge_client::data_dir()?)?;
    let state = verified_dir(&host.dir)?;
    let trash = state.join("storage-trash");
    if !trash.is_dir() || verified_dir(&trash)? != trash {
        return Ok(());
    }
    for receipt in listing(&trash)? {
        if receipt.extension().is_none_or(|s| s != "confirmed")
            || fs::symlink_metadata(&receipt)
                .map_err(text_error)
                .is_ok_and(|m| reparse(&m))
        {
            continue;
        }
        let Some(id) = receipt
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| hex(s, 32))
        else {
            continue;
        };
        let target = trash.join(id);
        if !target.exists() || purge_owned(&host.cfg, &trash, &target).is_ok() {
            fs::remove_file(receipt).map_err(text_error)?;
        }
    }
    Ok(())
}
fn clean(
    host: &Host,
    client: &mut Client,
    job: &Value,
    history: &BTreeMap<PathBuf, BTreeSet<String>>,
) -> Result<&'static str> {
    let _bindings = crate::service::bindings_guard(&shipforge_client::data_dir()?)?;
    let state = verified_dir(&host.dir)?;
    let command = &job["command"];
    let key = command["workspace"].as_str().ok_or("missing workspace")?;
    let path = candidates(&state)?
        .into_iter()
        .find(|p| shape(&state, p).is_some_and(|(_, id)| id == key))
        .ok_or("workspace disappeared")?;
    let journal = ledger(&state)?;
    let lock = file_lock(&journal.join(format!("{key}.lock")))?;
    lock.try_lock().map_err(text_error)?;
    let (kind, _) = shape(&state, &path).ok_or("unverified")?;
    let bucket = path.parent().ok_or("unverified")?;
    let mut bucket_lock = None;
    if kind != "run" {
        let f = file_lock(&bucket.join("initialize.lock"))?;
        f.try_lock().map_err(text_error)?;
        bucket_lock = Some(f);
    }
    let checked = inspect(host, &path, history, true)?;
    if checked["snapshot"] != command["expected_snapshot"] || !checked["protection"].is_null() {
        return Err("workspace changed".into());
    }
    let repo = source(&host.cfg, &path).map_err(str::to_owned)?;
    let trash = state.join("storage-trash");
    fs::create_dir_all(&trash).map_err(text_error)?;
    if verified_dir(&trash)? != trash {
        return Err("invalid trash directory".into());
    }
    let retired = trash.join(Uuid::new_v4().simple().to_string());
    let ready = bucket.join("ready");
    let displaced = bucket.join(format!("retired-{}", Uuid::new_v4().simple()));
    let owns_ready = kind != "run"
        && fs::read_to_string(&ready).ok().as_deref() == path.file_name().and_then(|n| n.to_str());
    host.call(
        client,
        json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]}),
    )?;
    if verified_dir(&path)? != path
        || verified_dir(bucket)? != bucket
        || verified_dir(&trash)? != trash
    {
        return Err("workspace changed".into());
    }
    let receipt = retired.with_extension("confirmed");
    crate::config::write_new(&receipt, key.as_bytes())?;
    if owns_ready {
        fs::rename(&ready, &displaced).map_err(text_error)?;
    }
    let moved = if let Some(repo) = &repo {
        git(repo, &["worktree", "move"], &[&path, &retired]).map(|_| ())
    } else {
        fs::rename(&path, &retired).map_err(text_error)
    };
    if let Err(e) = moved {
        if owns_ready {
            fs::rename(&displaced, &ready).map_err(text_error)?;
        }
        let _ = fs::remove_file(receipt);
        return Err(e);
    }
    if owns_ready {
        let _ = fs::remove_file(displaced);
    }
    let index = journal.join(format!("{key}.json"));
    if index.is_file() {
        let _ = fs::remove_file(index);
    }
    drop(bucket_lock);
    if purge_owned(&host.cfg, &trash, &retired).is_err() {
        return Ok("reclaim_pending");
    }
    let _ = fs::remove_file(receipt);
    Ok("success")
}
pub(super) fn perform(host: Host, job: Value) -> Result<Value> {
    let mut client = Client::from_env()?;
    let history = legacy(&job["historical"]);
    let mut outcome = "success";
    if job["command"]["action"] == "clean" {
        outcome = clean(&host, &mut client, &job, &history).unwrap_or("conflict");
    } else {
        let _ = resume_purge(&host);
    }
    let result = scan(&host, &history);
    if result.is_err() {
        outcome = "rescan_required";
    }
    host.call(&mut client,json!({"op":"machine.storage.finish","id":job["id"],"lease":job["lease"],"outcome":outcome,"inventory":result.ok()}))
}
