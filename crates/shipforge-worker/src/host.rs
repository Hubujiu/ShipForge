use crate::config::{Config, Result, credential, program_path, state_dir};
use crate::process;
use serde_json::{Value, json};
use shipforge_client::Client;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone)]
struct Host {
    cfg: Config,
    dir: PathBuf,
    key: String,
}
impl Host {
    fn call(&self, client: &mut Client, mut request: Value) -> Result<Value> {
        request["space"] = json!(self.cfg.space);
        request["machine_id"] = json!(self.cfg.machine_id);
        request["machine_key"] = json!(self.key);
        client.execute(request)
    }
    fn register(&self, client: &mut Client) -> Result<()> {
        let enabled = self
            .cfg
            .runtimes
            .iter()
            .filter(|(_, spec)| program_path(&spec.program).is_ok())
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        self.call(client,json!({"op":"machine.register","title":self.cfg.name,"shared":self.cfg.shared,"capacity":self.cfg.capacity,"runtimes":enabled}))?;
        Ok(())
    }
    fn spool(&self, request: &Value) -> Result<PathBuf> {
        let dir = self.dir.join("pending");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.json", Uuid::new_v4().simple()));
        let temp = path.with_extension("tmp");
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&temp).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(request).map_err(|e| e.to_string())?)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(temp, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }
    fn flush(&self, client: &mut Client) -> Result<()> {
        let pending = self.dir.join("pending");
        if !pending.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(pending).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().is_none_or(|v| v != "json") {
                continue;
            }
            let request: Value =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            match self.call(client, request) {
                Ok(_) => fs::remove_file(path).map_err(|e| e.to_string())?,
                Err(e) if e.contains("stale lease") || e.contains("already reported with") => {
                    fs::rename(&path, path.with_extension("rejected"))
                        .map_err(|e| e.to_string())?;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn perform(&self, claim: Value) -> Result<Value> {
        let mut client = Client::from_env()?;
        let id = claim["run"]["id"].as_str().ok_or("claim lacks a run ID")?;
        let lease = claim["lease"]
            .as_str()
            .ok_or("claim lacks lease credential")?;
        let mut previous = Instant::now();
        let mut confirmed = Instant::now();
        let mut authorization = Client::from_env()?;
        let heartbeat = || {
            if previous.elapsed() < Duration::from_secs(3) {
                return Ok(false);
            }
            previous = Instant::now();
            let request = json!({"op":"run.heartbeat","id":id,"lease":lease});
            match self.call(&mut client, request) {
                Ok(response) => {
                    confirmed = Instant::now();
                    Ok(response["cancel"] == true)
                }
                Err(e)
                    if confirmed.elapsed() > Duration::from_secs(85)
                        || e.contains("stale lease")
                        || e.contains("disabled")
                        || e.contains("authentication") =>
                {
                    Err(format!("renew lease: {e}"))
                }
                Err(_) => Ok(false),
            }
        };
        let call_tool = |invocation: &Value, dir: &Path| {
            let tool = invocation["tool_id"]
                .as_str()
                .ok_or("tool call requires tool_id")?;
            let request = json!({"op":"run.tool","id":id,"lease":lease,"tool_id":tool,"call_id":Uuid::new_v4().to_string()});
            let descriptor = self.call(&mut authorization, request)?;
            let args = invocation
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let value = crate::mcp::call(&self.cfg, dir, tool, &descriptor, args)?;
            Ok(json!({"tool_id":tool,"result":value}))
        };
        let payload = process::execute(&self.cfg, &self.dir, &claim["run"], heartbeat, call_tool)
            .unwrap_or_else(|error| {
                let status = if error == "cancelled" {
                    "cancelled"
                } else {
                    "failed"
                };
                json!({"status":status,"result":error,"artifacts":[]})
            });
        let mut request = payload.clone();
        request["op"] = json!("run.report");
        request["id"] = json!(id);
        request["lease"] = json!(lease);
        let saved = self.spool(&request)?;
        self.call(&mut client, request)?;
        fs::remove_file(saved).map_err(|e| e.to_string())?;
        Ok(json!({"id":id,"status":payload["status"]}))
    }
}
pub fn run(file: &Path, once: bool) -> Result<Value> {
    let cfg = Config::load(file)?;
    let dir = state_dir(file)?;
    let key = credential(&dir)?;
    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("supervisor.lock"))
        .map_err(|e| e.to_string())?;
    guard
        .try_lock()
        .map_err(|e| format!("machine supervisor is already running: {e}"))?;
    let stop_marker = dir.join("stop.request");
    if stop_marker.exists() {
        fs::remove_file(&stop_marker).map_err(|e| e.to_string())?;
    }
    let host = Host { cfg, dir, key };
    let mut client = Client::from_env()?;
    host.register(&mut client)?;
    let mut jobs: Vec<thread::JoinHandle<Result<Value>>> = Vec::new();
    let mut results = Vec::new();
    let mut last_ping = Instant::now()
        .checked_sub(Duration::from_secs(65))
        .unwrap_or_else(Instant::now);
    loop {
        if stop_marker.exists() {
            for job in jobs.drain(..) {
                results.push(job.join().map_err(|_| "runtime supervisor stopped")??);
            }
            host.flush(&mut client)?;
            return Ok(json!({"machine":host.cfg.machine_id,"stopped":true,"runs":results}));
        }
        // A finished thread has removed its spool entry, so flush only outside active dispatch.
        if jobs.is_empty() {
            match host.flush(&mut client) {
                Ok(()) => {}
                Err(e) if once => return Err(e),
                Err(e) => {
                    eprintln!("pending report: {e}");
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
            }
        }
        if last_ping.elapsed() >= Duration::from_secs(20) {
            if let Err(e) = host.call(&mut client, json!({"op":"machine.heartbeat"})) {
                if once || e.contains("disabled") || e.contains("authentication") {
                    for job in jobs.drain(..) {
                        let _ = job.join();
                    }
                    return Err(e);
                }
                eprintln!("machine heartbeat: {e}");
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            if let Err(error) = host.call(&mut client, json!({"op":"automation.tick"})) {
                if once {
                    return Err(error);
                }
                eprintln!("automation tick: {error}");
            }
            last_ping = Instant::now();
        }
        while jobs.len() < host.cfg.capacity as usize {
            let claim = match host.call(&mut client, json!({"op":"run.claim"})) {
                Ok(v) => v,
                Err(e) if once => return Err(e),
                Err(e) => {
                    eprintln!("claim: {e}");
                    break;
                }
            };
            if claim.is_null() {
                break;
            }
            let worker = host.clone();
            jobs.push(thread::spawn(move || worker.perform(claim)));
        }
        if once {
            for job in jobs.drain(..) {
                results.push(job.join().map_err(|_| "runtime supervisor stopped")??);
            }
            return Ok(json!({"machine":host.cfg.machine_id,"runs":results}));
        }
        let mut i = 0;
        while i < jobs.len() {
            if jobs[i].is_finished() {
                let res = jobs
                    .swap_remove(i)
                    .join()
                    .map_err(|_| "runtime supervisor stopped".to_string())
                    .and_then(|r| r);
                if let Err(e) = res {
                    eprintln!("run result retained locally: {e}");
                }
            } else {
                i += 1;
            }
        }
        thread::sleep(Duration::from_millis(850));
    }
}
pub fn request_stop(file: &Path) -> Result<Value> {
    Config::load(file)?;
    let dir = state_dir(file)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("supervisor.lock"))
        .map_err(|e| e.to_string())?;
    match lock.try_lock() {
        Ok(()) => Ok(json!({"running":false})),
        Err(std::fs::TryLockError::WouldBlock) => {
            fs::write(dir.join("stop.request"), b"stop").map_err(|e| e.to_string())?;
            Ok(json!({"running":true,"stopping":true}))
        }
        Err(e) => Err(e.to_string()),
    }
}
