use serde_json::{Value, json};
use shipforge_core::Store;
fn run(s: &mut Store, actor: &str, q: Value) -> Value {
    s.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}

#[test]
fn reported_offline_discovery_queues_and_rechecks_member_access_before_returning_results() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("fixture.db");
    let mut s = Store::open(&path).unwrap();
    put(&mut s, "member", "requester", json!({"role":"member"}));
    let machine =
        json!({"machine_id":"runner","machine_key":"synthetic-skill-job-key-with-32-characters"});
    let mut registration = machine.clone();
    for (k, v) in [
        ("op", json!("machine.register")),
        ("title", json!("Runner")),
        ("runtimes", json!(["fixture"])),
        ("shared", json!(true)),
    ] {
        registration[k] = v;
    }
    run(&mut s, "owner", registration);
    // Age a single synthetic heartbeat beyond the normal 90-second online grace period.
    rusqlite::Connection::open(path).unwrap().execute("UPDATE records SET fields=json_set(fields,'$.last_seen',0) WHERE kind='machine' AND id='runner'",[]).unwrap();
    let queued = run(
        &mut s,
        "requester",
        json!({"op":"skill.discover","machine_id":"runner","runtime":"fixture"}),
    );
    assert_eq!(queued["machine_online"], false);
    let mut jobs = machine.clone();
    jobs["op"] = json!("skill.jobs");
    assert_eq!(run(&mut s, "owner", jobs.clone())[0]["id"], queued["id"]);
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"member","id":"requester","fields":{"role":"viewer"}}),
    );
    let mut finish = machine;
    finish["id"] = queued["id"].clone();
    finish["op"] = json!("skill.finish");
    finish["sources"] = json!([]);
    assert_eq!(run(&mut s, "owner", finish.clone())["state"], "failed");
    assert_eq!(run(&mut s, "owner", finish)["state"], "failed");
    assert_eq!(run(&mut s, "owner", jobs), json!([]));
}
fn put(s: &mut Store, kind: &str, id: &str, f: Value) {
    run(
        s,
        "owner",
        json!({"op":"create","kind":kind,"id":id,"title":id,"fields":f}),
    );
}

#[test]
fn imported_files_are_atomic_and_same_named_existing_skills_bind_without_overwriting() {
    let mut s = Store::memory().unwrap();
    put(&mut s, "agent", "helper", json!({}));
    let package = json!([{"path":"SKILL.md","text":"# Guide\nKeep source stable"},{"path":"samples/case.md","text":"sample"}]);
    let bad = json!([{"path":"SKILL.md","text":"# Other"},{"path":"data.md","text":"one"},{"path":"DATA.MD","text":"two"}]);
    assert!(
        s.execute(
            "owner",
            json!({"op":"skill.import","packages":[{"files":package},{"files":bad}]})
        )
        .is_err()
    );
    assert_eq!(
        run(&mut s, "owner", json!({"op":"list","kind":"skill"})),
        json!([])
    );
    let first = run(
        &mut s,
        "owner",
        json!({"op":"skill.import","files":package}),
    );
    let id = &first["results"][0]["id"];
    let mut changed = package;
    changed[0]["text"] = json!("# Guide\nDifferent source");
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"skill.import","files":changed})
        )["results"][0]["status"],
        "skipped"
    );
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"skill.import","files":changed,"agent":"helper"})
        )["results"][0]["status"],
        "linked"
    );
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"skill.import","files":changed,"agent":"helper"})
        )["results"][0]["status"],
        "exists"
    );
    assert_eq!(
        run(&mut s, "owner", json!({"op":"get","kind":"skill","id":id}))["fields"]["files"][0]["text"],
        "# Guide\nKeep source stable"
    );
}

#[test]
fn skill_conflicts_preserve_local_edits_and_templates_copy_attached_skill_settings() {
    let mut s = Store::memory().unwrap();
    put(&mut s, "skill", "guide", json!({"instructions":"initial"}));
    put(
        &mut s,
        "agent",
        "template",
        json!({"runtime":"review","instructions":"Review", "skills":["guide"], "environment":{"SECRET":"local-only"}}),
    );
    run(
        &mut s,
        "owner",
        json!({"op":"catalog.publish","id":"custom","kind":"agent","title":"Custom","sources":["template"]}),
    );
    let first = run(
        &mut s,
        "owner",
        json!({"op":"catalog.instantiate","id":"custom"}),
    );
    let second = run(
        &mut s,
        "owner",
        json!({"op":"catalog.instantiate","id":"custom"}),
    );
    assert_ne!(first["fields"]["skills"][0], second["fields"]["skills"][0]);
    assert!(first["fields"]["environment"].is_null());
    assert_eq!(first["fields"]["runtime"], "review");
    let pubq = json!({"op":"catalog.publish","id":"bundle","kind":"skill","title":"Bundle","sources":["guide"]});
    run(&mut s, "owner", pubq.clone());
    let installed = run(
        &mut s,
        "owner",
        json!({"op":"catalog.install","id":"bundle"}),
    );
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"skill","id":installed["id"],"fields":{"instructions":"local changes"}}),
    );
    run(
        &mut s,
        "owner",
        json!({"op":"catalog.update","id":"bundle"}),
    );
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"skill","id":"guide","fields":{"instructions":"upstream changes"}}),
    );
    let mut next = pubq;
    next["expected_version"] = json!(1);
    run(&mut s, "owner", next.clone());
    assert!(s.execute("owner", next).is_err());
    assert!(
        s.execute("owner", json!({"op":"catalog.update","id":"bundle"}))
            .is_err()
    );
    let updated = run(
        &mut s,
        "owner",
        json!({"op":"catalog.update","id":"bundle","force":true}),
    );
    assert_eq!(updated["fields"]["instructions"], "upstream changes");
}

#[test]
fn crew_workspace_snapshot_uses_task_crew_and_private_context_and_remains_after_toggling() {
    let mut s = Store::memory().unwrap();
    put(&mut s, "agent", "lead", json!({}));
    put(&mut s, "task", "work", json!({"status":"todo"}));
    for id in ["c1", "c2"] {
        put(
            &mut s,
            "crew",
            id,
            json!({"leader_id":"lead","share_task_directory":true}),
        );
    }
    let call = |s: &mut Store, id: &str| {
        run(
            s,
            "owner",
            json!({"op":"run.enqueue","handler_kind":"crew","handler_id":id,"task_id":"work","instruction":"Review"}),
        )
    };
    let r = call(&mut s, "c1");
    let token = &r["fields"]["crew_workspace"]["token"];
    assert!(token.is_string());
    assert_eq!(
        call(&mut s, "c1")["fields"]["crew_workspace"]["token"],
        *token
    );
    put(
        &mut s,
        "chat",
        "private",
        json!({"visibility":"private","agent":"lead"}),
    );
    let private = run(
        &mut s,
        "owner",
        json!({"op":"run.enqueue","handler_kind":"crew","handler_id":"c1","task_id":"work","chat_id":"private","instruction":"Private discussion"}),
    );
    assert!(private["fields"]["crew_workspace"]["token"].is_string());
    assert_ne!(private["fields"]["crew_workspace"]["token"], *token);
    assert_ne!(
        call(&mut s, "c2")["fields"]["crew_workspace"]["token"],
        *token
    );
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"crew","id":"c1","fields":{"share_task_directory":false}}),
    );
    assert!(call(&mut s, "c1")["fields"]["crew_workspace"].is_null());
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"get","kind":"run","id":r["id"]})
        )["fields"]["crew_workspace"]["token"],
        *token
    );
    assert!(run(&mut s,"owner",json!({"op":"run.enqueue","handler_kind":"crew","handler_id":"c2","instruction":"No task"}))["fields"]["crew_workspace"].is_null());
}
