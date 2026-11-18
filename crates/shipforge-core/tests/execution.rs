use serde_json::{Value, json};
use shipforge_core::Store;

const SECRET: &str = "test-issued-secret-for-machine-with-enough-entropy";
fn execute(store: &mut Store, request: Value) -> Value {
    store
        .execute("owner", request.clone())
        .unwrap_or_else(|e| panic!("{request} failed: {e}"))
}
fn create(store: &mut Store, kind: &str, id: &str, fields: Value) -> Value {
    execute(
        store,
        json!({"op":"create","kind":kind,"id":id,"title":id,"fields":fields}),
    )
}
fn get(store: &mut Store, kind: &str, id: &str) -> Value {
    execute(store, json!({"op":"get","kind":kind,"id":id}))
}
fn machine(store: &mut Store) {
    execute(
        store,
        json!({"op":"machine.register","machine_id":"test-machine","machine_key":SECRET,"title":"Test machine","runtimes":["local-tool"],"capacity":2}),
    );
}
fn claim(store: &mut Store) -> Value {
    execute(
        store,
        json!({"op":"run.claim","machine_id":"test-machine","machine_key":SECRET}),
    )
}
fn report_request(claim: &Value, status: &str, result: &str) -> Value {
    json!({"op":"run.report","machine_id":"test-machine","machine_key":SECRET,"id":claim["run"]["id"],"lease":claim["lease"],"status":status,"result":result,"artifacts":[]})
}
#[test]
fn task_assignment_is_deferred_in_backlog_and_leases_are_fenced() {
    let mut db = Store::memory().unwrap();
    machine(&mut db);
    create(
        &mut db,
        "skill",
        "review",
        json!({"instructions":"check the change"}),
    );
    create(
        &mut db,
        "agent",
        "parent",
        json!({"runtime":"local-tool","instructions":"Start", "skills":["review"]}),
    );
    create(
        &mut db,
        "agent",
        "builder",
        json!({"parent_id":"parent","instructions":"Design"}),
    );
    let snapshot = execute(&mut db, json!({"op":"agent.resolve","id":"builder"}));
    assert_eq!(snapshot["instructions"], "Design");
    assert_eq!(snapshot["skills"][0], "review");
    create(
        &mut db,
        "task",
        "t0",
        json!({"description":"Write docs","status":"backlog","assignee":"builder","assignee_kind":"agent"}),
    );
    assert!(claim(&mut db).is_null());
    execute(
        &mut db,
        json!({"op":"update","kind":"task","id":"t0","fields":{"status":"todo"}}),
    );
    execute(
        &mut db,
        json!({"op":"update","kind":"task","id":"t0","fields":{"status":"in_progress"}}),
    );
    let run = claim(&mut db);
    assert_eq!(run["run"]["fields"]["task_context"]["id"], "t0");
    assert_eq!(
        run["run"]["fields"]["config"]["skill_versions"][0]["rev"],
        1
    );
    assert!(claim(&mut db).is_null());
    let fake = json!({"op":"run.heartbeat","id":run["run"]["id"],"machine_id":"test-machine","machine_key":SECRET,"lease":"wrong"});
    assert!(db.execute("owner", fake).is_err());
    let report = report_request(&run, "waiting", "Need a decision");
    execute(&mut db, report.clone());
    execute(&mut db, report);
    execute(
        &mut db,
        json!({"op":"run.reply","id":run["run"]["id"],"text":"Proceed"}),
    );
    let next = claim(&mut db);
    assert_ne!(run["lease"], next["lease"]);
    assert_eq!(next["run"]["fields"]["reply"]["text"], "Proceed");
    assert!(
        db.execute("owner", report_request(&run, "done", "late"))
            .is_err()
    );
    execute(&mut db, json!({"op":"run.cancel","id":next["run"]["id"]}));
    let heartbeat = execute(
        &mut db,
        json!({"op":"run.heartbeat","id":next["run"]["id"],"machine_id":"test-machine","machine_key":SECRET,"lease":next["lease"]}),
    );
    assert_eq!(heartbeat["cancel"], true);
    let result = execute(&mut db, report_request(&next, "done", "too late"));
    assert_eq!(result["fields"]["status"], "cancelled");
    assert_eq!(
        execute(&mut db, json!({"op":"list","kind":"run"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let public = get(&mut db, "machine", "test-machine").to_string();
    assert!(!public.contains(SECRET));
    assert!(
        db.execute(
            "owner",
            json!({"op":"create","kind":"run","title":"forged","fields":{}})
        )
        .is_err()
    );
}
#[test]
fn catalog_install_is_idempotent_and_document_tool_access_is_checked_against_live_grants() {
    let mut db = Store::memory().unwrap();
    let entries = execute(&mut db, json!({"op":"catalog.list"}));
    assert!(entries.as_array().unwrap().len() >= 4);
    let template = json!({"op":"catalog.install","id":"acceptance-check"});
    assert_eq!(
        execute(&mut db, template.clone()),
        execute(&mut db, template)
    );
    machine(&mut db);
    create(&mut db, "document", "guide", json!({"body":"Team runbook"}));
    create(
        &mut db,
        "tool",
        "read-guide",
        json!({"type":"document","document":"guide"}),
    );
    create(
        &mut db,
        "agent",
        "doc-agent",
        json!({"tools":["read-guide"]}),
    );
    create(
        &mut db,
        "task",
        "doc-task",
        json!({"status":"todo","assignee_kind":"agent","assignee":"doc-agent"}),
    );
    let run = claim(&mut db);
    let access = json!({"op":"run.tool","id":run["run"]["id"],"machine_id":"test-machine","machine_key":SECRET,"lease":run["lease"],"tool_id":"read-guide","call_id":"primary"});
    assert_eq!(execute(&mut db, access.clone())["body"], "Team runbook");
    assert_eq!(execute(&mut db, access.clone())["document"], "guide");
    assert_eq!(
        get(&mut db, "run", run["run"]["id"].as_str().unwrap())["fields"]["tool_calls"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"delete","kind":"task","id":"doc-task"})
        )
        .is_err()
    );
    execute(
        &mut db,
        json!({"op":"update","kind":"agent","id":"doc-agent","fields":{"tools":[]}}),
    );
    assert!(db.execute("owner", access).is_err());
    execute(&mut db, report_request(&run, "done", "Finished"));
    execute(
        &mut db,
        json!({"op":"delete","kind":"task","id":"doc-task"}),
    );
}
#[test]
fn crew_delegates_to_named_members_and_aggregates_the_child_result() {
    let mut db = Store::memory().unwrap();
    machine(&mut db);
    create(&mut db, "agent", "lead", json!({}));
    create(&mut db, "agent", "reviewer", json!({}));
    create(
        &mut db,
        "crew",
        "group",
        json!({"leader_id":"lead","members":["reviewer"]}),
    );
    execute(
        &mut db,
        json!({"op":"run.enqueue","handler_id":"group","handler_kind":"crew","instruction":"Review"}),
    );
    let original = claim(&mut db);
    let mut report = report_request(&original, "done", "delegating");
    report["delegates"] = json!([{"handler_id":"reviewer","instruction":"Check output"}]);
    let state = execute(&mut db, report);
    assert_eq!(state["fields"]["status"], "waiting_children");
    let child = claim(&mut db);
    assert_eq!(child["run"]["fields"]["parent_id"], original["run"]["id"]);
    execute(&mut db, report_request(&child, "done", "Verified"));
    let summary = claim(&mut db);
    assert_eq!(summary["run"]["id"], original["run"]["id"]);
    assert_eq!(
        summary["run"]["fields"]["child_results"][0]["result"],
        "Verified"
    );
    execute(&mut db, report_request(&summary, "done", "Complete"));
    assert!(claim(&mut db).is_null());
}
#[test]
fn rule_dedup_and_connector_three_way_merge() {
    let mut db = Store::memory().unwrap();
    create(&mut db, "agent", "a", json!({}));
    create(
        &mut db,
        "automation",
        "r",
        json!({"handler_id":"a","instruction":"Summarize", "enabled":true,"triggers":[{"type":"webhook"},{"type":"schedule","interval_minutes":60}]}),
    );
    let request = json!({"op":"automation.fire","id":"r","source":"webhook","event_id":"ev1"});
    let one = execute(&mut db, request.clone());
    assert_eq!(one["id"], execute(&mut db, request)["id"]);
    let first = execute(&mut db, json!({"op":"automation.tick"}));
    let second = execute(&mut db, json!({"op":"automation.tick"}));
    assert_eq!(first, second);
    create(
        &mut db,
        "connector",
        "gh",
        json!({"provider":"github","repository":"example/sample","enabled":true}),
    );
    let mut inbound = json!({"op":"connector.ingest","id":"gh","external_id":"9","event_id":"e1","source_version":"2026-01-01T00:00:00Z","title":"Original","description":"v0","closed":false});
    let created = execute(&mut db, inbound.clone());
    assert_eq!(created, execute(&mut db, inbound.clone()));
    let task = created["task"].as_str().unwrap();
    execute(
        &mut db,
        json!({"op":"update","kind":"task","id":task,"title":"local"}),
    );
    inbound["event_id"] = json!("e2");
    inbound["source_version"] = json!("2026-01-02T00:00:00Z");
    inbound["title"] = json!("upstream");
    assert_eq!(execute(&mut db, inbound.clone())["action"], "conflict");
    inbound["resolve"] = json!("source");
    assert_eq!(execute(&mut db, inbound)["action"], "updated");
    let mapped = execute(
        &mut db,
        json!({"op":"connector.export","id":"gh","task_id":task}),
    );
    assert_eq!(mapped["value"]["title"], "upstream");
    assert_eq!(mapped["external_id"], "9");
}
#[test]
fn viewers_cannot_dispatch_and_private_chat_runs_do_not_leak() {
    let mut db = Store::memory().unwrap();
    create(
        &mut db,
        "member",
        "bob",
        json!({"actor":"bob","role":"viewer"}),
    );
    create(&mut db, "agent", "a", json!({}));
    create(
        &mut db,
        "chat",
        "private-chat",
        json!({"visibility":"private"}),
    );
    let run = execute(
        &mut db,
        json!({"op":"run.enqueue","handler_id":"a","chat_id":"private-chat","instruction":"Personal task"}),
    );
    assert!(
        db.execute("bob", json!({"op":"get","kind":"run","id":run["id"]}))
            .is_err()
    );
    assert!(
        db.execute(
            "bob",
            json!({"op":"run.enqueue","handler_id":"a","instruction":"run this"})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"run.claim","machine_id":"unknown","machine_key":"wrong"})
        )
        .is_err()
    );
}
