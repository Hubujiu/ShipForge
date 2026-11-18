use serde_json::{Value, json};
use shipforge_client::Client;
use std::path::Path;
use std::process::Command;

fn worker(dir: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_shipforge-worker"))
        .args(args)
        .env_remove("SHIPFORGE_SERVER")
        .env_remove("SHIPFORGE_TOKEN")
        .env("SHIPFORGE_DATA_DIR", dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}
#[test]
fn parallel_jobs_use_distinct_real_git_worktrees_while_the_bound_repository_stays_clean() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let cfg = temp.path().join("machine.json");
    let repo = temp.path().join("source");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    std::fs::write(repo.join("baseline.txt"), "retained content").unwrap();
    git(&repo, &["add", "baseline.txt"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=ShipForge Test",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let file = cfg.to_str().unwrap();
    worker(
        &data,
        &[
            "init",
            "--config",
            file,
            "--workspace",
            repo.to_str().unwrap(),
            "--example",
        ],
    );
    let mut settings: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
    settings["capacity"] = json!(2);
    std::fs::write(&cfg, settings.to_string()).unwrap();
    let mut client = Client::local(&data).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"a","title":"A","fields":{"runtime":"inspect-context","max_parallel":2}})).unwrap();
    for id in ["t1", "t2"] {
        client.execute(json!({"op":"create","kind":"task","id":id,"title":id,"fields":{"assignee":"a","assignee_kind":"agent","status":"todo"}})).unwrap();
    }
    let result = worker(&data, &["run", "--config", file, "--once"]);
    let jobs = result["runs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2, "{result}");
    let mut locations = std::collections::BTreeSet::new();
    for job in jobs {
        let r = client
            .execute(json!({"op":"get","kind":"run","id":job["id"]}))
            .unwrap();
        assert_eq!(r["fields"]["status"], "done", "{r}");
        let path = Path::new(r["fields"]["artifacts"][0]["location"].as_str().unwrap());
        assert!(path.join(".git").is_file());
        assert_eq!(
            std::fs::read_to_string(path.join("baseline.txt")).unwrap(),
            "retained content"
        );
        locations.insert(path.to_path_buf());
    }
    assert_eq!(locations.len(), 2);
    assert_eq!(
        git(&repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|s| s.starts_with("worktree "))
            .count(),
        3
    );
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());
}
#[test]
fn packaged_machine_runs_an_external_process_and_writes_a_durable_result() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let path = temp.path().join("machine.json");
    let config = path.to_str().unwrap();
    worker(&data, &["init", "--config", config, "--example"]);
    let mut cli = Client::local(&data).unwrap();
    cli.execute(json!({"op":"create","kind":"agent","id":"test","title":"Agent","fields":{"runtime":"inspect-context","instructions":"Summarize"}})).unwrap();
    cli.execute(json!({"op":"create","kind":"task","id":"t1","title":"First task","fields":{"assignee":"test","assignee_kind":"agent","status":"todo"}})).unwrap();
    let result = worker(&data, &["run", "--config", config, "--once"]);
    assert_eq!(result["runs"][0]["status"], "done");
    let run = cli
        .execute(json!({"op":"get","kind":"run","id":result["runs"][0]["id"]}))
        .unwrap();
    assert_eq!(run["fields"]["result"], "Context received for First task");
    assert!(Path::new(run["fields"]["artifacts"][0]["location"].as_str().unwrap()).is_dir());
    assert!(
        worker(&data, &["run", "--config", config, "--once"])["runs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn agent_executes_an_approved_mcp_stdio_call_and_reads_its_result_in_the_same_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = temp.path().join("machine.json");
    let config = cfg.to_str().unwrap();
    let data = temp.path().join("data");
    let runtime = temp.path().join("runtime.py");
    let mcp = temp.path().join("tool.py");
    std::fs::write(
        &runtime,
        r#"import json, pathlib, sys
r = json.load(sys.stdin)
pathlib.Path(r["fields"]["progress_file"]).write_text(json.dumps({"message":"tool step","percent":75}))
marker = pathlib.Path("same-workdir.txt")
if r["fields"].get("tool_results"):
    value = r["fields"]["tool_results"][0]["result"]["content"][0]["text"]
    assert marker.read_text() == "opened"
    print(json.dumps({"status":"done","result":value,"artifacts":["same-workdir.txt"]}))
else:
    marker.write_text("opened")
    print(json.dumps({"tool_calls":[{"tool_id":"lookup","arguments":{"phrase":"verified"}}]}))
"#,
    )
    .unwrap();
    std::fs::write(&mcp,r#"import json, sys
def read(): return json.loads(sys.stdin.readline())
def reply(msg, result): print(json.dumps({"jsonrpc":"2.0","id":msg["id"],"result":result}),flush=True)
i = read()
assert i["method"] == "initialize"
reply(i, {"protocolVersion":i["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
assert read()["method"] == "notifications/initialized"
call = read()
assert call["method"] == "tools/call" and call["params"]["name"] == "read_note"
reply(call,{"content":[{"type":"text","text":call["params"]["arguments"]["phrase"]}]})
"#).unwrap();
    let python = if cfg!(windows) { "python" } else { "python3" };
    worker(
        &data,
        &[
            "init",
            "--config",
            config,
            "--program",
            python,
            "--runtime",
            "fixture",
            "--arg",
            runtime.to_str().unwrap(),
        ],
    );
    let mut settings: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
    settings["approved_tools"] = json!({"lookup":{"tool_name":"read_note","program":python,"args":[mcp],"timeout_seconds":10}});
    std::fs::write(&cfg, settings.to_string()).unwrap();
    let mut client = Client::local(&data).unwrap();
    client.execute(json!({"op":"create","kind":"tool","id":"lookup","title":"Lookup","fields":{"type":"mcp"}})).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"a","title":"Agent","fields":{"runtime":"fixture","tools":["lookup"]}})).unwrap();
    client.execute(json!({"op":"create","kind":"task","id":"t","title":"T","fields":{"assignee":"a","assignee_kind":"agent","status":"todo"}})).unwrap();
    let output = worker(&data, &["run", "--config", config, "--once"]);
    let run = client
        .execute(json!({"op":"get","kind":"run","id":output["runs"][0]["id"]}))
        .unwrap();
    assert_eq!(run["fields"]["status"], "done", "{run}");
    assert_eq!(run["fields"]["result"], "verified");
    assert_eq!(run["fields"]["progress"]["percent"], 75);
    assert_eq!(run["fields"]["tool_calls"][0]["tool"], "lookup");
    assert_eq!(run["fields"]["artifacts"][0]["path"], "same-workdir.txt");
    let files = client
        .execute(json!({"op":"list","kind":"artifact"}))
        .unwrap();
    assert_eq!(files.as_array().unwrap().len(), 2);
    let artifact = files
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["title"] == "same-workdir.txt")
        .unwrap();
    let downloaded=client.execute(json!({"op":"attachment.download","kind":"artifact","id":artifact["id"],"attachment_id":artifact["fields"]["attachments"][0]["id"]})).unwrap();
    assert_eq!(downloaded["content_base64"], "b3BlbmVk");
}

#[test]
fn direct_directory_lock_serializes_independent_jobs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("direct");
    std::fs::create_dir(&repo).unwrap();
    let file = temp.path().join("machine.json");
    let cfg = file.to_str().unwrap();
    let data = temp.path().join("data");
    let script = temp.path().join("serial.py");
    std::fs::write(
        &script,
        r#"import json, os, pathlib, sys, time
data=json.load(sys.stdin)
marker=pathlib.Path("in-use.txt")
with marker.open("x") as hold:
    hold.write(data["title"])
    time.sleep(0.35)
marker.unlink()
with open("finished.txt","a") as out:
    out.write(data["title"]+"\n")
print(json.dumps({"status":"done","result":"completed"}))
"#,
    )
    .unwrap();
    let python = if cfg!(windows) { "python" } else { "python3" };
    worker(
        &data,
        &[
            "init",
            "--config",
            cfg,
            "--workspace",
            repo.to_str().unwrap(),
            "--runtime",
            "serial",
            "--program",
            python,
            "--arg",
            script.to_str().unwrap(),
        ],
    );
    let mut settings: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    settings["capacity"] = json!(2);
    settings["allow_direct"] = json!(true);
    std::fs::write(&file, settings.to_string()).unwrap();
    let mut cli = Client::local(&data).unwrap();
    cli.execute(json!({"op":"create","kind":"agent","id":"a","title":"A","fields":{"runtime":"serial","max_parallel":2,"directory_mode":"direct"}})).unwrap();
    for id in ["first", "second"] {
        cli.execute(json!({"op":"create","kind":"task","id":id,"title":id,"fields":{"status":"todo","assignee":"a","assignee_kind":"agent"}})).unwrap();
    }
    let jobs = worker(&data, &["run", "--config", cfg, "--once"])["runs"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|r| r["status"] == "done"), "{jobs:?}");
    let lines = std::fs::read_to_string(repo.join("finished.txt")).unwrap();
    assert_eq!(lines.lines().count(), 2);
    assert!(!repo.join("in-use.txt").exists());
}

#[test]
fn progress_arrives_while_running_then_cancellation_and_supervisor_stop_finish_cleanly() {
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let config = temp.path().join("machine.json");
    let cfg = config.to_str().unwrap();
    let script = temp.path().join("long.py");
    std::fs::write(&script,r#"import json, pathlib, sys, time
r=json.load(sys.stdin)
pathlib.Path(r["fields"]["progress_file"]).write_text(json.dumps({"message":"stage started","percent":20}))
time.sleep(30)
print(json.dumps({"status":"done","result":"completed"}))
"#).unwrap();
    let python = if cfg!(windows) { "python" } else { "python3" };
    worker(
        &data,
        &[
            "init",
            "--config",
            cfg,
            "--runtime",
            "slow",
            "--program",
            python,
            "--arg",
            script.to_str().unwrap(),
        ],
    );
    let mut cli = Client::local(&data).unwrap();
    cli.execute(
        json!({"op":"create","kind":"agent","id":"a","title":"A","fields":{"runtime":"slow"}}),
    )
    .unwrap();
    cli.execute(json!({"op":"create","kind":"task","id":"t","title":"T","fields":{"status":"todo","assignee":"a","assignee_kind":"agent"}})).unwrap();
    let mut service = Command::new(env!("CARGO_BIN_EXE_shipforge-worker"))
        .args(["run", "--config", cfg])
        .env_remove("SHIPFORGE_SERVER")
        .env_remove("SHIPFORGE_TOKEN")
        .env("SHIPFORGE_DATA_DIR", &data)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    let id = loop {
        let runs = cli.execute(json!({"op":"list","kind":"run"})).unwrap();
        let run = &runs[0];
        if run["fields"]["status"] == "running" && run["fields"]["progress"]["percent"] == 20 {
            break run["id"].as_str().unwrap().to_string();
        }
        if Instant::now() > until {
            let _ = service.kill();
            let _ = service.wait();
            panic!("no live progress, last record {run}");
        }
        std::thread::sleep(Duration::from_millis(120));
    };
    cli.execute(json!({"op":"run.cancel","id":id})).unwrap();
    assert_eq!(worker(&data, &["stop", "--config", cfg])["stopping"], true);
    let until = Instant::now() + Duration::from_secs(16);
    loop {
        if let Some(status) = service.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > until {
            let _ = service.kill();
            let _ = service.wait();
            panic!("supervisor did not shut down");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let run = cli
        .execute(json!({"op":"get","kind":"run","id":id}))
        .unwrap();
    assert_eq!(run["fields"]["status"], "cancelled", "{run}");
    assert_eq!(run["fields"]["progress"]["message"], "stage started");
    assert_eq!(worker(&data, &["stop", "--config", cfg])["running"], false);
}
