use serde_json::{Value, json};
use shipforge_core::Store;
const KEY: &str = "synthetic-local-conversation-machine-key";
fn go(s: &mut Store, a: &str, q: Value) -> Value {
    s.execute(a, q.clone())
        .unwrap_or_else(|e| panic!("{a} {q}: {e}"))
}
fn new(s: &mut Store, a: &str, k: &str, id: &str, f: Value) {
    go(
        s,
        a,
        json!({"op":"create","kind":k,"id":id,"title":id,"fields":f}),
    );
}
fn fixture() -> Store {
    let mut s = Store::memory().unwrap();
    for actor in ["alice", "bob", "charlie"] {
        new(&mut s, "owner", "member", actor, json!({"role":"member"}));
    }
    new(
        &mut s,
        "alice",
        "skill",
        "guide",
        json!({"instructions":"read our guide"}),
    );
    new(
        &mut s,
        "alice",
        "agent",
        "ada",
        json!({"runtime":"test","skills":["guide"],"visibility":"space"}),
    );
    new(
        &mut s,
        "alice",
        "task",
        "ticket",
        json!({"description":"original snapshot","status":"todo"}),
    );
    s
}
fn machine(s: &mut Store, commands: Value) {
    go(
        s,
        "alice",
        json!({"op":"machine.register","machine_id":"box","title":"Test box","machine_key":KEY,"runtimes":["test"],"shared":false,"slash_commands":commands}),
    );
}
fn claim(s: &mut Store) -> Value {
    go(
        s,
        "alice",
        json!({"op":"run.claim","machine_id":"box","machine_key":KEY}),
    )
}
fn finish(s: &mut Store, lease: &Value) {
    go(
        s,
        "alice",
        json!({"op":"run.report","id":lease["run"]["id"],"machine_id":"box","machine_key":KEY,"lease":lease["lease"],"status":"done","result":"ready to review"}),
    );
}
#[test]
fn shortcuts_queue_with_visible_frozen_context_and_keep_personal_history() {
    let mut s = fixture();
    let q = json!({"op":"shortcut.save","title":"Check ticket","prompt":"Inspect","handler_id":"ada","visibility":"space","request_id":"make-shared"});
    let shared = go(&mut s, "alice", q.clone());
    assert_eq!(go(&mut s, "alice", q)["id"], shared["id"]);
    let private = go(
        &mut s,
        "alice",
        json!({"op":"shortcut.save","title":"Private","handler_id":"ada"}),
    );
    let bob = go(&mut s, "bob", json!({"op":"shortcut.catalog"}));
    assert_eq!(bob.as_array().unwrap().len(), 1);
    assert_eq!(bob[0]["pinned"], false);
    assert!(
        s.execute(
            "bob",
            json!({"op":"shortcut.save","id":shared["id"],"title":"take over"})
        )
        .is_err()
    );
    assert!(
        s.execute("bob", json!({"op":"shortcut.delete","id":private["id"]}))
            .is_err()
    );
    go(
        &mut s,
        "alice",
        json!({"op":"shortcut.organize","id":shared["id"],"order":[shared["id"],private["id"]]}),
    );
    assert_eq!(
        go(&mut s, "alice", json!({"op":"shortcut.catalog"}))[0]["id"],
        shared["id"]
    );
    let cmd = json!({"op":"shortcut.run","id":shared["id"],"task":"ticket","request_id":"once"});
    assert!(s.execute("alice", cmd.clone()).is_err());
    machine(&mut s, json!({"test":["/plan","review"]}));
    let first = go(&mut s, "alice", cmd.clone());
    assert_eq!(go(&mut s, "alice", cmd), first);
    let menu = go(
        &mut s,
        "alice",
        json!({"op":"chat.slash","id":first["chat"]}),
    );
    assert_eq!(menu["items"][0]["token"], "/guide");
    assert_eq!(menu["items"][1]["token"], "/plan");
    go(
        &mut s,
        "alice",
        json!({"op":"update","kind":"task","id":"ticket","fields":{"description":"changed later"}}),
    );
    let leased = claim(&mut s);
    assert_eq!(
        leased["run"]["fields"]["task_context"]["description"],
        "original snapshot"
    );
    let busy = go(
        &mut s,
        "alice",
        json!({"op":"chat.slash","id":first["chat"],"query":"rev"}),
    );
    assert_eq!(busy["busy"], true);
    assert_eq!(busy["items"][0]["token"], "/review");
    let second = go(
        &mut s,
        "alice",
        json!({"op":"shortcut.run","id":shared["id"],"task":"ticket","request_id":"another"}),
    );
    assert_ne!(first["chat"], second["chat"]);
    assert!(
        s.execute("bob", json!({"op":"chat.slash","id":first["chat"]}))
            .is_err()
    );
    assert!(
        s.execute(
            "alice",
            json!({"op":"chat.slash.send","id":first["chat"],"body":"/missing"})
        )
        .is_err()
    );
    go(
        &mut s,
        "alice",
        json!({"op":"chat.slash.send","id":first["chat"],"body":"/review now"}),
    );
    go(
        &mut s,
        "alice",
        json!({"op":"shortcut.delete","id":shared["id"]}),
    );
    let history = go(
        &mut s,
        "alice",
        json!({"op":"shortcut.history","task":"ticket"}),
    );
    assert_eq!(history["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(history["selected"]["chat"], second["chat"]);
    assert_eq!(
        go(
            &mut s,
            "bob",
            json!({"op":"shortcut.history","task":"ticket"})
        )["sessions"],
        json!([])
    );
    machine(&mut s, Value::Null);
    let menu = go(
        &mut s,
        "alice",
        json!({"op":"chat.slash","id":first["chat"]}),
    );
    assert_eq!(menu["commands_reported"], false);
    assert_eq!(menu["items"].as_array().unwrap().len(), 1);
}
#[test]
fn posted_review_clears_visible_pending_marker_without_closing_task() {
    let mut s = fixture();
    machine(&mut s, json!({"test":[]}));
    new(
        &mut s,
        "alice",
        "chat",
        "room",
        json!({"agent":"ada","task":"ticket","visibility":"private"}),
    );
    let turn = go(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":"room","body":"please check"}),
    );
    let lease = claim(&mut s);
    finish(&mut s, &lease);
    let state = json!({"op":"task.review.state","id":"ticket"});
    assert_eq!(
        go(&mut s, "alice", state.clone())["runs"],
        json!([turn["run"]])
    );
    assert_eq!(go(&mut s, "bob", state.clone())["awaiting_review"], false);
    assert!(
        s.execute(
            "bob",
            json!({"op":"task.review","id":"ticket","run":turn["run"]})
        )
        .is_err()
    );
    let unchanged = go(
        &mut s,
        "alice",
        json!({"op":"task.review","id":"ticket","body":"need another change"}),
    );
    assert_eq!(unchanged["cleared"], false);
    assert!(
        s.execute(
            "alice",
            json!({"op":"task.review","id":"ticket","expected_rev":99})
        )
        .is_err()
    );
    assert_eq!(go(&mut s, "alice", state.clone())["awaiting_review"], true);
    let command =
        json!({"op":"task.review","id":"ticket","run":turn["run"],"request_id":"review-once"});
    let reviewed = go(&mut s, "alice", command.clone());
    assert_eq!(reviewed, go(&mut s, "alice", command));
    assert_eq!(reviewed["status"], "todo");
    assert_eq!(go(&mut s, "alice", state)["awaiting_review"], false);
    let history = json!({"op":"chat.history","id":"room"});
    let before = go(&mut s, "alice", history.clone());
    go(&mut s, "alice", json!({"op":"chat.complete","id":"room"}));
    assert_eq!(go(&mut s, "alice", history), before);
    assert_eq!(
        go(
            &mut s,
            "alice",
            json!({"op":"get","kind":"task","id":"ticket"})
        )["fields"]["status"],
        "done"
    );
    assert_eq!(
        go(&mut s, "alice", json!({"op":"chat.info","id":"room"}))["completed"],
        true
    );
    go(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":"room","body":"more work"}),
    );
    assert_eq!(
        go(&mut s, "alice", json!({"op":"chat.info","id":"room"}))["completed"],
        false
    );
}
#[test]
fn embedded_documents_check_current_visibility_and_markdown_outline() {
    let mut s = fixture();
    new(
        &mut s,
        "alice",
        "document",
        "notes",
        json!({"body":"# Notes\n```text\n# not a heading\n```\n## Details","visibility":"space"}),
    );
    let link = go(&mut s, "alice", json!({"op":"document.share","id":"notes"}))["link"].clone();
    go(
        &mut s,
        "alice",
        json!({"op":"update","kind":"task","id":"ticket","fields":{"description":format!("[design]({})",link.as_str().unwrap())}}),
    );
    let embeds = json!({"op":"document.embeds","kind":"task","id":"ticket"});
    assert_eq!(
        go(&mut s, "bob", embeds.clone())["items"][0]["preview"]["outline"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    go(
        &mut s,
        "alice",
        json!({"op":"update","kind":"document","id":"notes","fields":{"body":"updated","visibility":"private"}}),
    );
    let denied = go(&mut s, "bob", embeds.clone());
    assert_eq!(denied["items"][0], json!({"link":link,"available":false}));
    assert!(
        s.execute("charlie", json!({"op":"document.open","link":link}))
            .is_err()
    );
    assert_eq!(
        go(&mut s, "alice", embeds)["items"][0]["preview"]["summary"],
        "updated"
    );
}
