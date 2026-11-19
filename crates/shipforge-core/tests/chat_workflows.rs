use serde_json::{Value, json};
use shipforge_core::Store;

fn run(s: &mut Store, actor: &str, q: Value) -> Value {
    s.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn write(s: &mut Store, actor: &str, kind: &str, id: &str, f: Value) -> Value {
    run(
        s,
        actor,
        json!({"op":"create","kind":kind,"id":id,"title":id,"fields":f}),
    )
}
fn msg(s: &mut Store, actor: &str, room: &str, id: &str) {
    write(s, actor, "message", id, json!({"chat":room,"body":id}));
}
fn setup(s: &mut Store) {
    for id in ["alice", "bob", "carl"] {
        write(s, "owner", "member", id, json!({"role":"member"}));
    }
    write(
        s,
        "owner",
        "agent",
        "ada",
        json!({"runtime":"local-tool","instructions":"Synthetic agent","chat_parallel":4}),
    );
}
fn sidebar(s: &mut Store, actor: &str, id: &str) -> Value {
    run(s, actor, json!({"op":"chat.sidebar"}))
        .as_array()
        .unwrap()
        .iter()
        .find(|room| room["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}
fn organize(s: &mut Store, actor: &str, id: &str, action: &str) -> Value {
    run(
        s,
        actor,
        json!({"op":"chat.organize","id":id,"action":action}),
    )
}
fn messages(s: &mut Store, actor: &str, id: &str) -> Vec<Value> {
    run(
        s,
        actor,
        json!({"op":"list","kind":"message","filter":{"chat":id},"limit":100}),
    )
    .as_array()
    .unwrap()
    .clone()
}
fn invite(s: &mut Store, room: &str) {
    run(
        s,
        "alice",
        json!({"op":"chat.manage","id":room,"action":"invite","target":"bob"}),
    );
    run(
        s,
        "bob",
        json!({"op":"chat.manage","id":room,"action":"accept"}),
    );
}
const KEY: &str = "synthetic-key-for-chat-workflow-worker-test";
fn machine(s: &mut Store) {
    run(
        s,
        "alice",
        json!({"op":"machine.register","machine_id":"box","machine_key":KEY,"title":"Synthetic worker","runtimes":["local-tool"],"capacity":4}),
    );
}
fn claim(s: &mut Store) -> Value {
    run(
        s,
        "alice",
        json!({"op":"run.claim","machine_id":"box","machine_key":KEY}),
    )
}
fn report(lease: &Value, text: &str) -> Value {
    json!({"op":"run.report","id":lease["run"]["id"],"machine_id":"box","machine_key":KEY,"lease":lease["lease"],"status":"done","result":text})
}

#[test]
fn hide_unpins_undo_and_only_a_visible_new_message_reveals_the_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rooms.db");
    let mut s = Store::open(&path).unwrap();
    setup(&mut s);
    for id in ["one", "other"] {
        write(
            &mut s,
            "alice",
            "chat",
            id,
            json!({"visibility":"space","agent":"ada"}),
        );
    }
    msg(&mut s, "alice", "one", "before");
    organize(&mut s, "alice", "one", "pin");
    assert_eq!(
        run(&mut s, "alice", json!({"op":"chat.sidebar"}))[0]["id"],
        "one"
    );
    assert_eq!(
        sidebar(&mut s, "bob", "one")["fields"]["sidebar"]["pinned"],
        false
    );
    let hidden = organize(&mut s, "alice", "one", "hide");
    assert_eq!(hidden["pinned"], false);
    assert!(sidebar(&mut s, "alice", "one").is_null());
    assert!(!sidebar(&mut s, "bob", "one").is_null());
    assert_eq!(messages(&mut s, "alice", "one").len(), 1);
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert!(sidebar(&mut s, "alice", "one").is_null());
    run(
        &mut s,
        "alice",
        json!({"op":"chat.organize","id":"one","action":"undo","undo_token":hidden["undo_token"]}),
    );
    assert_eq!(
        sidebar(&mut s, "alice", "one")["fields"]["sidebar"]["pinned"],
        true
    );
    let hidden = organize(&mut s, "alice", "one", "hide");
    msg(&mut s, "bob", "other", "elsewhere");
    run(
        &mut s,
        "alice",
        json!({"op":"update","kind":"chat","id":"one","title":"Renamed"}),
    );
    run(
        &mut s,
        "alice",
        json!({"op":"update","kind":"message","id":"before","fields":{"body":"edited"}}),
    );
    write(
        &mut s,
        "bob",
        "message",
        "confidential",
        json!({"chat":"one","body":"private to Bob","visibility":"private"}),
    );
    assert!(sidebar(&mut s, "alice", "one").is_null());
    msg(&mut s, "bob", "one", "new");
    assert_eq!(
        sidebar(&mut s, "alice", "one")["fields"]["sidebar"]["pinned"],
        false
    );
    assert!(s.execute("alice",json!({"op":"chat.organize","id":"one","action":"undo","undo_token":hidden["undo_token"]})).is_err());
}
#[test]
fn forks_freeze_visible_prefix_and_respect_source_clear_and_revocation_across_generations() {
    let mut s = Store::memory().unwrap();
    setup(&mut s);
    write(&mut s, "alice", "task", "linked", json!({}));
    write(
        &mut s,
        "alice",
        "chat",
        "room",
        json!({"task":"linked","agent":"ada","visibility":"private"}),
    );
    invite(&mut s, "room");
    msg(&mut s, "alice", "room", "u1");
    msg(&mut s, "bob", "room", "reply");
    msg(&mut s, "alice", "room", "later");
    run(
        &mut s,
        "alice",
        json!({"op":"attachment.upload","kind":"message","id":"u1","filename":"proof.txt","content_base64":"YWJj"}),
    );
    let q = json!({"op":"chat.fork","id":"room","message":"reply","request_id":"branch-one"});
    let fork = run(&mut s, "bob", q.clone());
    assert_eq!(run(&mut s, "bob", q), fork);
    let id = fork["id"].as_str().unwrap();
    assert_eq!(fork["fields"]["task"], "linked");
    assert_eq!(fork["fields"]["visibility"], "private");
    let inherited = messages(&mut s, "bob", id);
    assert_eq!(inherited.len(), 2);
    let first = inherited
        .iter()
        .find(|m| m["fields"]["body"] == "u1")
        .unwrap();
    let first_id = first["id"].as_str().unwrap();
    let download = json!({"op":"attachment.download","kind":"message","id":first_id,"attachment_id":first["fields"]["attachments"][0]["id"]});
    assert_eq!(
        run(&mut s, "bob", download.clone())["content_base64"],
        "YWJj"
    );
    assert!(
        s.execute("carl", json!({"op":"get","kind":"message","id":first_id}))
            .is_err()
    );
    assert!(
        s.execute(
            "bob",
            json!({"op":"chat.share.create","id":id,"scope":"link","message_ids":[first_id]})
        )
        .is_err()
    );
    run(
        &mut s,
        "alice",
        json!({"op":"update","kind":"message","id":"u1","fields":{"body":"edited source"}}),
    );
    assert_eq!(
        run(
            &mut s,
            "bob",
            json!({"op":"get","kind":"message","id":first_id})
        )["fields"]["body"],
        "u1"
    );
    msg(&mut s, "bob", id, "branch-only");
    let nested = run(
        &mut s,
        "bob",
        json!({"op":"chat.fork","id":id,"message":"branch-only"}),
    );
    let nested = nested["id"].as_str().unwrap();
    assert_eq!(messages(&mut s, "bob", nested).len(), 3);
    run(
        &mut s,
        "bob",
        json!({"op":"chat.manage","id":"room","action":"clear"}),
    );
    assert!(s.execute("bob", download).is_err());
    assert_eq!(messages(&mut s, "bob", id).len(), 1);
    assert_eq!(messages(&mut s, "bob", nested).len(), 1);
    assert!(
        s.execute("bob", json!({"op":"chat.fork","id":"room","message":"u1"}))
            .is_err()
    );
    assert_eq!(messages(&mut s, "alice", "room").len(), 3);
    msg(&mut s, "alice", "room", "fresh");
    let fresh = run(
        &mut s,
        "bob",
        json!({"op":"chat.fork","id":"room","message":"fresh"}),
    );
    let fresh = fresh["id"].as_str().unwrap();
    assert_eq!(messages(&mut s, "bob", fresh).len(), 1);
    run(
        &mut s,
        "alice",
        json!({"op":"chat.manage","id":"room","action":"revoke","target":"bob"}),
    );
    assert!(messages(&mut s, "bob", fresh).is_empty());
}
#[test]
fn busy_agent_creates_one_task_from_the_original_conversation_on_accepted_report() {
    let mut s = Store::memory().unwrap();
    setup(&mut s);
    machine(&mut s);
    write(
        &mut s,
        "alice",
        "chat",
        "room",
        json!({"agent":"ada","visibility":"private"}),
    );
    invite(&mut s, "room");
    let sent = run(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":"room","body":"Please research the issue."}),
    );
    let first = claim(&mut s);
    assert_eq!(first["run"]["id"], sent["run"]);
    let request = json!({"op":"chat.convert","id":"room","request_id":"convert-once"});
    let pending = run(&mut s, "alice", request.clone());
    assert_eq!(
        pending["run"],
        run(&mut s, "alice", json!({"op":"chat.convert","id":"room"}))["run"]
    );
    assert!(claim(&mut s).is_null());
    assert!(
        run(&mut s, "alice", json!({"op":"list","kind":"task"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.execute("bob", json!({"op":"chat.convert","id":"room"}))
            .is_err()
    );
    run(
        &mut s,
        "alice",
        report(&first, "Prior research is complete."),
    );
    let active = claim(&mut s);
    assert_eq!(active["run"]["id"], pending["run"]);
    assert_eq!(active["run"]["fields"]["chat_context"]["id"], "room");
    let context = &active["run"]["fields"]["chat_context"]["messages"];
    assert!(
        context
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "Prior research is complete.")
    );
    let mut completed = report(&active, "Report ready");
    completed["task_draft"] =
        json!({"title":"Improve search latency","description":"Measure timings for the chat list"});
    let result = run(&mut s, "alice", completed.clone());
    assert_eq!(run(&mut s, "alice", completed), result);
    assert_eq!(result["fields"]["status"], "done");
    let id = &result["fields"]["created_task"];
    assert_eq!(
        run(
            &mut s,
            "alice",
            json!({"op":"get","kind":"chat","id":"room"})
        )["fields"]["task"],
        *id
    );
    let tasks = run(&mut s, "alice", json!({"op":"list","kind":"task"}));
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    assert_eq!(tasks[0]["title"], "Improve search latency");
    assert_eq!(tasks[0]["fields"]["visibility"], "private");
    assert_eq!(run(&mut s, "alice", request)["run"], pending["run"]);
    assert!(
        s.execute("carl", json!({"op":"get","kind":"task","id":id}))
            .is_err()
    );
}
#[test]
fn bind_existing_checks_ownership_task_acl_and_source_report_after_access_loss() {
    let mut s = Store::memory().unwrap();
    setup(&mut s);
    machine(&mut s);
    write(
        &mut s,
        "alice",
        "chat",
        "room",
        json!({"agent":"ada","visibility":"private"}),
    );
    invite(&mut s, "room");
    write(
        &mut s,
        "carl",
        "task",
        "restricted",
        json!({"visibility":"private"}),
    );
    write(&mut s, "alice", "task", "visible", json!({}));
    assert!(
        s.execute(
            "alice",
            json!({"op":"chat.bind_task","id":"room","task":"restricted"})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "bob",
            json!({"op":"chat.bind_task","id":"room","task":"visible"})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "alice",
            json!({"op":"update","kind":"chat","id":"room","fields":{"task":"visible"}})
        )
        .is_err()
    );
    let bound = run(
        &mut s,
        "alice",
        json!({"op":"chat.bind_task","id":"room","task":"visible"}),
    );
    assert_eq!(bound["fields"]["task"], "visible");
    let fork_source = run(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":"room","body":"Question"}),
    );
    let first = claim(&mut s);
    run(&mut s, "alice", report(&first, "Answer"));
    let fork = run(
        &mut s,
        "alice",
        json!({"op":"chat.fork","id":"room","message":fork_source["message"]}),
    );
    let fork = fork["id"].as_str().unwrap();
    let sent = run(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":fork,"body":"Continue on branch"}),
    );
    let active = claim(&mut s);
    assert_eq!(active["run"]["id"], sent["run"]);
    assert_eq!(active["run"]["fields"]["chat_context"]["id"], fork);
    assert_eq!(
        active["run"]["fields"]["chat_context"]["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    run(
        &mut s,
        "alice",
        json!({"op":"chat.manage","id":"room","action":"clear"}),
    );
    let result = run(&mut s, "alice", report(&active, "old privileged context"));
    assert_eq!(result["fields"]["status"], "cancelled");
    assert!(
        !json!(messages(&mut s, "alice", fork))
            .to_string()
            .contains("old privileged context")
    );
}

#[test]
fn completed_branch_answers_and_artifacts_follow_their_source_permissions() {
    let mut s = Store::memory().unwrap();
    setup(&mut s);
    machine(&mut s);
    write(
        &mut s,
        "alice",
        "chat",
        "source",
        json!({"agent":"ada","visibility":"private"}),
    );
    msg(&mut s, "alice", "source", "original");
    let fork = run(
        &mut s,
        "alice",
        json!({"op":"chat.fork","id":"source","message":"original"}),
    );
    let branch = fork["id"].as_str().unwrap();
    run(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":branch,"body":"Summarize this finding"}),
    );
    let job = claim(&mut s);
    let mut response = report(&job, "Derived finding");
    response["artifacts"] =
        json!([{"name":"derived.txt","filename":"derived.txt","content_base64":"YWJj"}]);
    run(&mut s, "alice", response);
    let items = messages(&mut s, "alice", branch);
    let answer = items
        .iter()
        .find(|r| r["fields"]["body"] == "Derived finding")
        .unwrap();
    let artifact = run(&mut s, "alice", json!({"op":"list","kind":"artifact"}))[0].clone();
    assert_eq!(artifact["fields"]["run"], job["run"]["id"]);
    let nested = run(
        &mut s,
        "alice",
        json!({"op":"chat.fork","id":branch,"message":answer["id"]}),
    );
    invite(&mut s, branch);
    assert!(
        s.execute(
            "bob",
            json!({"op":"get","kind":"message","id":answer["id"]})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "bob",
            json!({"op":"get","kind":"artifact","id":artifact["id"]})
        )
        .is_err()
    );
    invite(&mut s, "source");
    assert_eq!(
        run(
            &mut s,
            "bob",
            json!({"op":"get","kind":"message","id":answer["id"]})
        )["fields"]["body"],
        "Derived finding"
    );
    run(
        &mut s,
        "alice",
        json!({"op":"chat.manage","id":"source","action":"clear"}),
    );
    for (kind, id) in [
        ("message", &answer["id"]),
        ("artifact", &artifact["id"]),
        ("run", &job["run"]["id"]),
    ] {
        assert!(
            s.execute("alice", json!({"op":"get","kind":kind,"id":id}))
                .is_err()
        );
    }
    assert_eq!(
        run(
            &mut s,
            "alice",
            json!({"op":"chat.history","id":nested["id"]})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        run(
            &mut s,
            "bob",
            json!({"op":"get","kind":"message","id":answer["id"]})
        )["fields"]["body"],
        "Derived finding"
    );
}

#[test]
fn automatic_and_other_members_task_chats_enter_the_list_after_the_first_personal_turn() {
    let mut s = Store::memory().unwrap();
    setup(&mut s);
    write(&mut s, "alice", "task", "task", json!({}));
    write(
        &mut s,
        "alice",
        "chat",
        "room",
        json!({"agent":"ada","task":"task","visibility":"space"}),
    );
    write(
        &mut s,
        "alice",
        "chat",
        "automatic",
        json!({"agent":"ada","task":"task","visibility":"space","origin_kind":"automation"}),
    );
    assert!(!sidebar(&mut s, "alice", "room").is_null());
    assert!(sidebar(&mut s, "bob", "room").is_null());
    assert!(sidebar(&mut s, "alice", "automatic").is_null());
    msg(&mut s, "bob", "automatic", "bob-first");
    assert!(!sidebar(&mut s, "bob", "automatic").is_null());
    assert!(sidebar(&mut s, "alice", "automatic").is_null());
    msg(&mut s, "bob", "room", "personal-turn");
    assert!(!sidebar(&mut s, "bob", "room").is_null());
}
