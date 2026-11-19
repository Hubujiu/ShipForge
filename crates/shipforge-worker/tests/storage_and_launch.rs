use serde_json::{Value, json};
use shipforge_client::Client;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};
const BIN: &str = env!("CARGO_BIN_EXE_shipforge-worker");
fn cmd(exe: &Path, data: &Path) -> Command {
    let mut c = Command::new(exe);
    c.env("SHIPFORGE_DATA_DIR", data)
        .env_remove("SHIPFORGE_SERVER")
        .env_remove("SHIPFORGE_TOKEN");
    c
}
fn output(mut cmd: Command) -> String {
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
fn git(dir: &Path, args: &[&str]) -> String {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    output(c)
}
struct Fixture {
    _temp: tempfile::TempDir,
    data: PathBuf,
    file: PathBuf,
    repo: PathBuf,
    exe: PathBuf,
    marker: PathBuf,
    release: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let data = root.join("data");
        let file = root.join("owner.json");
        let repo = root.join("bound repo");
        let script = root.join("adapter.py");
        let marker = root.join("active");
        let release = root.join("continue");
        let exe = root.join(if cfg!(windows) {
            "isolated-machine.exe"
        } else {
            "isolated-machine"
        });
        fs::copy(BIN, &exe).unwrap();
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "--quiet"]);
        fs::write(repo.join("baseline.txt"), "keep this repository").unwrap();
        git(&repo, &["add", "baseline.txt"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        let mut p = Command::new(if cfg!(windows) { "python" } else { "python3" });
        p.args(["-c", "import sys;print(sys.executable)"]);
        let python = output(p).trim().to_owned();
        fs::write(&script,r#"import json,os,pathlib,sys,time
q=json.load(sys.stdin); marker,release=map(pathlib.Path,sys.argv[1:3])
p=pathlib.Path.cwd();p.joinpath('captured.json').write_text(json.dumps({'argv':sys.argv[3:],'theme':os.getenv('QA_THEME'),'local':os.getenv('QA_MACHINE_ONLY'),'service_secret':os.getenv('SHIPFORGE_TOKEN')}))
instruction=q['fields'].get('instruction')
if instruction=='hold':
 marker.write_text(str(os.getpid()))
 for i in range(300):
  if release.exists():break
  time.sleep(.1)
print(json.dumps({'status':'failed' if instruction=='fail' else 'done','result':'fixture finished','artifacts':[]}))
"#).unwrap();
        fs::write(&file,json!({"machine_id":"qa-box","name":"Isolated QA","workspace":repo,"runtimes":{"fixture":{"program":python,"args":[script,marker,release],"timeout_seconds":40}},"runtime_launch":{"fixture":{"models":{"tiny":["low","high"],"large":["medium","high"]},"default_model":"tiny","model_flag":"--model","effort_flag":"--effort","default_arguments":{"--style":"short"},"allowed_arguments":{"--style":["short","two words"]},"environment":{"QA_THEME":"machine-default","QA_MACHINE_ONLY":"local-private-fixture"},"allowed_environment":["QA_THEME"]}}}).to_string()).unwrap();
        Self {
            _temp: t,
            data,
            file,
            repo,
            exe,
            marker,
            release,
        }
    }
    fn once(&self) {
        let mut c = cmd(&self.exe, &self.data);
        c.args(["run", "--once", "--config"]).arg(&self.file);
        output(c);
    }
    fn service(&self, op: &str) {
        let mut c = cmd(&self.exe, &self.data);
        c.args(["service", op, "--config"])
            .arg(&self.file)
            .arg("--directory")
            .arg(&self.data);
        output(c);
    }
    fn client(&self) -> Client {
        Client::local(&self.data).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, "end");
        let _ = cmd(&self.exe, &self.data)
            .args(["service", "stop", "--config"])
            .arg(&self.file)
            .arg("--directory")
            .arg(&self.data)
            .output();
    }
}
fn call(c: &mut Client, q: Value) -> Value {
    c.execute(q.clone()).unwrap_or_else(|e| panic!("{q} · {e}"))
}
fn create_agent(c: &mut Client) {
    call(
        c,
        json!({"op":"create","kind":"agent","id":"one","title":"QA agent","fields":{"runtime":"fixture","model":"large","reasoning_effort":"medium","arguments":["--style","two words"],"environment":{"QA_THEME":"agent-override"}}}),
    );
}
fn enqueue(c: &mut Client, instruction: &str) -> Value {
    call(
        c,
        json!({"op":"run.enqueue","handler_id":"one","instruction":instruction}),
    )
}
fn item(c: &mut Client, id: &Value) -> Value {
    call(c, json!({"op":"get","kind":"run","id":id}))
}
fn dir(r: &Value) -> PathBuf {
    PathBuf::from(
        r["fields"]["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["type"] == "workspace")
            .unwrap()["location"]
            .as_str()
            .unwrap(),
    )
}
fn preview(c: &mut Client) -> Value {
    call(c, json!({"op":"machine.storage.preview","id":"qa-box"}))
}
fn scan(c: &mut Client, f: &Fixture) -> Value {
    call(
        c,
        json!({"op":"machine.storage.request","id":"qa-box","action":"scan"}),
    );
    f.once();
    preview(c)
}
fn clean(c: &mut Client, v: &Value) -> Value {
    call(
        c,
        json!({"op":"machine.storage.request","id":"qa-box","action":"clean","confirm":true,"workspace":v["id"],"expected_snapshot":v["snapshot"]}),
    )
}
#[test]
fn approved_model_effort_arguments_and_environment_reach_only_the_requested_process() {
    let f = Fixture::new();
    f.once();
    let mut c = f.client();
    create_agent(&mut c);
    let opt = call(&mut c, json!({"op":"agent.runtime.options","id":"one"}));
    assert_eq!(
        opt["machines"][0]["launch"]["models"]["large"],
        json!(["medium", "high"])
    );
    assert!(!opt.to_string().contains("local-private-fixture"));
    let a = enqueue(&mut c, "one");
    let b = call(
        &mut c,
        json!({"op":"run.enqueue","handler_id":"one","instruction":"override","model":"tiny","reasoning_effort":"high"}),
    );
    f.once();
    f.once();
    for (r, model, effort) in [(&a, "large", "medium"), (&b, "tiny", "high")] {
        let record = item(&mut c, &r["id"]);
        let v: Value =
            serde_json::from_slice(&fs::read(dir(&record).join("captured.json")).unwrap()).unwrap();
        assert_eq!(
            v["argv"],
            json!(["--model", model, "--effort", effort, "--style", "two words"])
        );
        assert_eq!(v["theme"], "agent-override");
        assert_eq!(v["local"], "local-private-fixture");
        assert!(v["service_secret"].is_null());
        assert!(!record.to_string().contains("local-private-fixture"));
    }
    let saved = call(&mut c, json!({"op":"get","kind":"agent","id":"one"}));
    assert_eq!(saved["fields"]["model"], "large");
    let only_model = call(
        &mut c,
        json!({"op":"run.enqueue","handler_id":"one","instruction":"Model default depth","model":"tiny"}),
    );
    f.once();
    let v: Value = serde_json::from_slice(
        &fs::read(dir(&item(&mut c, &only_model["id"])).join("captured.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        v["argv"],
        json!(["--model", "tiny", "--style", "two words"])
    );
    let mut cfg: Value = serde_json::from_slice(&fs::read(&f.file).unwrap()).unwrap();
    cfg["runtimes"]["fixture"]["args"]
        .as_array_mut()
        .unwrap()
        .extend([json!("--model"), json!("large")]);
    fs::write(&f.file, cfg.to_string()).unwrap();
    f.once();
    let published = call(&mut c, json!({"op":"agent.runtime.options","id":"one"}));
    assert_eq!(published["machines"][0]["launch"]["fixed_model"], "large");
    let locked = call(
        &mut c,
        json!({"op":"run.enqueue","handler_id":"one","instruction":"Fixed model","model":"tiny","reasoning_effort":"high"}),
    );
    f.once();
    let v: Value = serde_json::from_slice(
        &fs::read(dir(&item(&mut c, &locked["id"])).join("captured.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        v["argv"],
        json!([
            "--model",
            "large",
            "--effort",
            "high",
            "--style",
            "two words"
        ])
    );
    call(
        &mut c,
        json!({"op":"update","kind":"agent","id":"one","fields":{"arguments":["--not-approved","x"]}}),
    );
    let denied = enqueue(&mut c, "denied");
    f.once();
    assert_eq!(item(&mut c, &denied["id"])["fields"]["status"], "failed");
}
#[test]
fn verified_storage_cleans_one_ended_checkout_and_protects_recovery_bindings_and_stale_snapshots() {
    let f = Fixture::new();
    f.once();
    let mut c = f.client();
    create_agent(&mut c);
    let first = enqueue(&mut c, "first");
    f.once();
    let run = item(&mut c, &first["id"]);
    let checkout = dir(&run);
    assert!(checkout.join(".git").is_file());
    let failed = enqueue(&mut c, "fail");
    f.once();
    assert_eq!(item(&mut c, &failed["id"])["fields"]["status"], "failed");
    let v = scan(&mut c, &f);
    let entries = v["entries"].as_array().unwrap();
    let entry = entries
        .iter()
        .find(|e| e["protection"].is_null())
        .unwrap()
        .clone();
    assert!(entry["bytes"].as_u64().unwrap() > 0);
    assert!(entries.iter().any(|e| e["protection"] == "recovery"));
    assert!(!v.to_string().contains(&checkout.display().to_string()));
    let interrupted = f
        .file
        .with_file_name("owner.json.state")
        .join("storage-index")
        .join(format!("{}.active", entry["id"].as_str().unwrap()))
        .join("test-interrupted.json");
    fs::write(&interrupted, "123").unwrap();
    let v = scan(&mut c, &f);
    assert!(
        v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == entry["id"] && e["protection"] == "recovery")
    );
    fs::remove_file(&interrupted).unwrap();
    scan(&mut c, &f);
    fs::write(checkout.join("changed.txt"), "new work").unwrap();
    clean(&mut c, &entry);
    f.once();
    let v = preview(&mut c);
    assert_eq!(v["jobs"][0]["outcome"], "conflict");
    assert!(checkout.is_dir());
    let mut cfg: Value = serde_json::from_slice(&fs::read(&f.file).unwrap()).unwrap();
    cfg["profiles"] = json!({"protected":checkout});
    fs::write(&f.file, cfg.to_string()).unwrap();
    let v = scan(&mut c, &f);
    assert!(
        v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["protection"] == "bound_repository")
    );
    cfg["profiles"] = json!({});
    fs::write(&f.file, cfg.to_string()).unwrap();
    let v = scan(&mut c, &f);
    let entry = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["protection"].is_null())
        .unwrap();
    assert!(c.execute(json!({"op":"machine.storage.request","id":"qa-box","action":"clean","workspace":entry["id"],"expected_snapshot":entry["snapshot"]})).is_err());
    clean(&mut c, entry);
    f.once();
    let v = preview(&mut c);
    assert_eq!(v["jobs"][0]["outcome"], "success", "{v}");
    assert!(!checkout.exists());
    assert_eq!(
        fs::read_to_string(f.repo.join("baseline.txt")).unwrap(),
        "keep this repository"
    );
    assert_eq!(
        git(&f.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count(),
        2
    );
    assert_eq!(v["entries"].as_array().unwrap().len(), 1);
}
#[test]
fn running_process_keeps_its_checkout_while_storage_scans_in_parallel() {
    let f = Fixture::new();
    f.service("start");
    let mut c = f.client();
    create_agent(&mut c);
    let r = enqueue(&mut c, "hold");
    let until = Instant::now() + Duration::from_secs(20);
    while !f.marker.exists() {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(60));
    }
    let q = call(
        &mut c,
        json!({"op":"machine.storage.request","id":"qa-box","action":"scan"}),
    );
    let listing = loop {
        let v = preview(&mut c);
        if v["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["id"] == q["id"] && j["state"] == "done")
        {
            break v;
        }
        assert!(Instant::now() < until, "{v}");
        thread::sleep(Duration::from_millis(80));
    };
    assert_eq!(listing["entries"][0]["protection"], "active");
    let entry = &listing["entries"][0];
    assert!(c.execute(json!({"op":"machine.storage.request","id":"qa-box","action":"clean","confirm":true,"workspace":entry["id"],"expected_snapshot":entry["snapshot"]})).is_err());
    fs::write(&f.release, "end").unwrap();
    while item(&mut c, &r["id"])["fields"]["status"] != "done" {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(75));
    }
    f.service("stop");
}
