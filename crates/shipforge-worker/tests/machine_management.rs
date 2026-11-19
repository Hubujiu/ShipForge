use serde_json::{Value, json};
use shipforge_client::Client;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn invoke(data: &Path, cfg: &Path, action: &str, options: &[&str]) -> Value {
    let mut cmd = Command::new(isolated_binary(cfg));
    cmd.args(["service", action, "--directory"])
        .arg(data)
        .arg("--config")
        .arg(cfg)
        .args(options)
        .env_remove("SHIPFORGE_SERVER")
        .env_remove("SHIPFORGE_TOKEN")
        .env("SHIPFORGE_DATA_DIR", data);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    let out = rx
        .recv_timeout(std::time::Duration::from_secs(25))
        .expect("service must close the caller's pipes while its supervisor stays online")
        .unwrap();
    assert!(
        out.status.success(),
        "{action}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn isolated_binary(cfg: &Path) -> PathBuf {
    cfg.with_file_name(if cfg!(windows) {
        "isolated-service.exe"
    } else {
        "isolated-service"
    })
}
struct Cleanup<'a>(&'a Path, &'a Path);
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = Command::new(isolated_binary(self.1))
            .args(["service", "stop", "--directory"])
            .arg(self.0)
            .arg("--config")
            .arg(self.1)
            .env("SHIPFORGE_DATA_DIR", self.0)
            .env_remove("SHIPFORGE_SERVER")
            .env_remove("SHIPFORGE_TOKEN")
            .output();
    }
}
fn config(cfg: &Path) {
    fs::write(cfg,json!({"machine_id":"isolated-box","name":"Test machine","space":"default","capacity":1,"runtimes":{}}).to_string()).unwrap();
}
#[test]
fn real_supervisor_follows_lifecycle_and_quit_policy_without_user_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("service data");
    let cfg = tmp.path().join("machine.json");
    config(&cfg);
    fs::copy(
        env!("CARGO_BIN_EXE_shipforge-worker"),
        isolated_binary(&cfg),
    )
    .unwrap();
    let _cleanup = Cleanup(&data, &cfg);
    assert_eq!(invoke(&data, &cfg, "status", &[])["running"], false);
    assert_eq!(invoke(&data, &cfg, "desktop-open", &[])["running"], false);
    let set = invoke(
        &data,
        &cfg,
        "configure",
        &["--mode", "desktop", "--close-to-tray", "false"],
    );
    assert_eq!(set["policy"]["keep_background"], true);
    let first = invoke(&data, &cfg, "desktop-open", &[]);
    assert_eq!(first["status"], "online");
    let original = first["pid"].as_u64().unwrap();
    assert_eq!(invoke(&data, &cfg, "start", &[])["pid"], original);
    assert_eq!(invoke(&data, &cfg, "desktop-exit", &[])["pid"], original);
    let second = invoke(&data, &cfg, "restart", &[]);
    assert_eq!(second["status"], "online");
    assert_ne!(second["pid"], original);
    invoke(&data, &cfg, "configure", &["--keep-background", "false"]);
    assert_eq!(invoke(&data, &cfg, "desktop-exit", &[])["running"], false);
    assert_eq!(
        invoke(&data, &cfg, "status", &[])["policy"]["mode"],
        "desktop"
    );
    let mut client = Client::local(&data).unwrap();
    assert_eq!(
        client
            .execute(json!({"op":"machine.runtimes","id":"isolated-box"}))
            .unwrap()["online"],
        false
    );
}
fn python() -> PathBuf {
    let exe = if cfg!(windows) { "python" } else { "python3" };
    let out = Command::new(exe)
        .args(["-c", "import sys;print(sys.executable)"])
        .output()
        .unwrap();
    assert!(out.status.success());
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}
#[test]
fn existing_workspace_configurations_share_desktop_policy() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    let first = data.join("machine.json");
    let folder = data.join("machines");
    fs::create_dir(&folder).unwrap();
    let second = folder.join("team.json");
    config(&first);
    fs::write(
        &second,
        json!({"machine_id":"team-box","name":"Older machine","space":"team","runtimes":{}})
            .to_string(),
    )
    .unwrap();
    let aggregate = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_shipforge-worker"))
            .arg("service")
            .args(args)
            .arg("--directory")
            .arg(data)
            .env_remove("SHIPFORGE_SERVER")
            .env_remove("SHIPFORGE_TOKEN")
            .env("SHIPFORGE_DATA_DIR", data)
            .output()
            .unwrap();
        assert!(out.status.success());
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let v = aggregate(&["status"]);
    assert_eq!(v["machines"].as_array().unwrap().len(), 2);
    assert!(
        v["machines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["running"] == false)
    );
    let v = aggregate(&["desktop-exit", "--keep-background", "false"]);
    assert!(
        v["machines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["policy"]["keep_background"] == false)
    );
    assert!(!data.join("machine-services").is_dir());
}
fn run_once(data: &Path, cfg: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_shipforge-worker"))
        .args(["run", "--once", "--config"])
        .arg(cfg)
        .env("SHIPFORGE_DATA_DIR", data)
        .env_remove("SHIPFORGE_SERVER")
        .env_remove("SHIPFORGE_TOKEN")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn restart_interrupts_a_running_runtime_and_releases_its_job() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("machine.json");
    let data = tmp.path().join("active data");
    let marker = tmp.path().join("runtime.pid");
    fs::copy(
        env!("CARGO_BIN_EXE_shipforge-worker"),
        isolated_binary(&cfg),
    )
    .unwrap();
    fs::write(&cfg,json!({"machine_id":"isolated-box","name":"Cancellation QA","runtimes":{"slow-qa":{"program":python(),"args":["-c","import os,pathlib,sys,time;pathlib.Path(sys.argv[1]).write_text(str(os.getpid()));time.sleep(90)",marker],"timeout_seconds":100}}}).to_string()).unwrap();
    let _cleanup = Cleanup(&data, &cfg);
    let first = invoke(&data, &cfg, "start", &[]);
    let mut client = Client::local(&data).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"agent","title":"QA","fields":{"runtime":"slow-qa"}})).unwrap();
    let run=client.execute(json!({"op":"run.enqueue","handler_id":"agent","instruction":"Isolated sleeping fixture"})).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while !marker.is_file() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    assert!(marker.is_file());
    let second = invoke(&data, &cfg, "restart", &[]);
    assert_ne!(first["pid"], second["pid"]);
    let saved = client
        .execute(json!({"op":"get","kind":"run","id":run["id"]}))
        .unwrap();
    assert_eq!(saved["fields"]["status"], "cancelled", "{saved}");
    assert_eq!(invoke(&data, &cfg, "stop", &[])["running"], false);
}
#[test]
fn separate_worker_runs_a_local_install_upgrade_version_and_model_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("runtime data");
    let cfg = tmp.path().join("machine.json");
    let installed = tmp.path().join("installed.marker");
    let script = tmp.path().join("fixture.py");
    let python = python();
    fs::write(&script,r#"import json,os,pathlib,re,sys
mode,path=sys.argv[1],pathlib.Path(sys.argv[2])
if mode in ("install","upgrade"):path.write_text("1.2.3" if mode=="install" else "2.0.0")
elif mode=="probe":print("QA "+path.read_text()+" INTERNAL_LOCAL_ONLY")
elif mode=="test":
 data=json.load(sys.stdin)
 assert not any(k.startswith("SHIPFORGE_") for k in os.environ)
 print(json.dumps({"status":"done","result":re.search(r"SHIPFORGE_[a-f0-9]+",data["fields"]["instruction"]).group(0)}))
elif mode=="wrong":print('{"status":"done","result":"fixed reply"}')
elif mode=="login":sys.stderr.write("authentication required INTERNAL_LOCAL_ONLY");sys.exit(1)
"#).unwrap();
    let cmd =
        |name: &str| json!({"program":python,"args":[script,name,installed],"timeout_seconds":5});
    let mut config = json!({"machine_id":"isolated-box","name":"Runtime QA","runtimes":{"qa-model":cmd("test")},"runtime_management":{"qa-model":{"executable":installed,"probe":cmd("probe"),"install":cmd("install"),"upgrade":cmd("upgrade")}}});
    fs::write(&cfg, config.to_string()).unwrap();
    run_once(&data, &cfg);
    let mut client = Client::local(&data).unwrap();
    let info = |c: &mut Client| {
        c.execute(json!({"op":"machine.runtimes","id":"isolated-box"}))
            .unwrap()
    };
    let cap = |v: &Value| {
        v["runtimes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "qa-model")
            .unwrap()
            .clone()
    };
    assert_eq!(cap(&info(&mut client))["status"], "not_detected");
    let submit = |c: &mut Client, action: &str| {
        let v=c.execute(json!({"op":"machine.runtime.request","id":"isolated-box","runtime":"qa-model","action":action})).unwrap();
        run_once(&data, &cfg);
        let list = info(c);
        let job = list["recent"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == v["id"])
            .unwrap();
        (job["result"]["outcome"].clone(), list)
    };
    let (done, list) = submit(&mut client, "install");
    assert_eq!(done, "success");
    assert_eq!(cap(&list)["version"], "1.2.3");
    assert!(!list.to_string().contains("INTERNAL_LOCAL_ONLY"));
    let (_, list) = submit(&mut client, "upgrade");
    assert_eq!(cap(&list)["version"], "2.0.0");
    assert_eq!(submit(&mut client, "rescan").0, "success");
    assert_eq!(submit(&mut client, "test").0, "success");
    for (mode, expected) in [("wrong", "unexpected_reply"), ("login", "login_required")] {
        config["runtimes"]["qa-model"] = cmd(mode);
        fs::write(&cfg, config.to_string()).unwrap();
        assert_eq!(submit(&mut client, "test").0, expected);
    }
}
