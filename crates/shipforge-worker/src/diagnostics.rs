use crate::config::{Config, Result, state_dir};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn lock(state: &Path) -> Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state.join("diagnostic.lock"))
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    Ok(lock)
}
pub(crate) fn record(
    state: &Path,
    run: &Value,
    event: &str,
    pid: Option<u32>,
    category: Option<&str>,
) {
    if let Err(e) = write(state, run, event, pid, category) {
        eprintln!("diagnostic log: {e}");
    }
}
fn write(
    state: &Path,
    run: &Value,
    event: &str,
    pid: Option<u32>,
    category: Option<&str>,
) -> Result<()> {
    let _guard = lock(state)?;
    let path = state.join("diagnostics.jsonl");
    if path.metadata().is_ok_and(|m| m.len() > 512 * 1024) {
        let old = state.join("diagnostics.previous.jsonl");
        if old.exists() {
            fs::remove_file(&old).map_err(|e| e.to_string())?;
        }
        fs::rename(&path, old).map_err(|e| e.to_string())?;
    }
    let entry = json!({"at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,"run":run["id"],"attempt":run["fields"]["attempt"],"event":event,"pid":pid,"category":category});
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    writeln!(file, "{entry}").map_err(|e| e.to_string())
}
pub(crate) fn list(config: &Path) -> Result<Value> {
    let cfg = Config::load(config)?;
    let state = state_dir(config)?;
    let _guard = lock(&state)?;
    let running = OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join("supervisor.lock"))
        .is_ok_and(|f| matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
    let mut recent = std::collections::VecDeque::new();
    let mut active = std::collections::BTreeMap::new();
    for name in ["diagnostics.previous.jsonl", "diagnostics.jsonl"] {
        let path = state.join(name);
        if !path.is_file() {
            continue;
        }
        for line in BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines() {
            let line = line.map_err(|e| e.to_string())?;
            let row: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if row["event"] == "supervisor_started" {
                active.clear();
            }
            if let Some(id) = row["run"].as_str() {
                if row["event"] == "started" {
                    active.insert(id.to_owned(), row["pid"].clone());
                } else {
                    active.remove(id);
                }
            }
            if recent.len() == 100 {
                recent.pop_front();
            }
            recent.push_back(row);
        }
    }
    if !running {
        active.clear();
    }
    Ok(
        json!({"machine":cfg.machine_id,"supervisor_running":running,"capacity":cfg.capacity,"processes":active,"events":recent}),
    )
}
