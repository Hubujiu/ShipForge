use serde_json::{Value, json};
use shipforge_core::Store;
#[test]
fn existing_unlinked_terminal_run_acquires_one_private_conversation_after_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let mut db = Store::open(&path).unwrap();
    go(
        &mut db,
        "owner",
        json!({"op":"create","kind":"agent","id":"a","title":"Legacy Agent","fields":{}}),
    );
    go(
        &mut db,
        "owner",
        json!({"op":"create","kind":"automation","id":"r","title":"Old rule","fields":{"enabled":true,"handler_id":"a","instruction":"New instruction"}}),
    );
    drop(db);
    let sql = rusqlite::Connection::open(&path).unwrap();
    let f = json!({"origin_kind":"automation","automation_id":"r","source":"manual","instruction":"Original old instruction","handler_kind":"agent","handler_id":"a","requested_by":"owner","status":"done","result":"Old result","runtime":"local-tool","config":{"runtime":"local-tool"}});
    let row = json!({"space":"default","kind":"run","id":"old-run","title":"Historical run","rev":1,"fields":f,"created_at":1234,"updated_at":1234});
    sql.execute("INSERT INTO records(space,kind,id,title,fields,rev,created_at,updated_at) VALUES('default','run','old-run','Historical run',?,1,1234,1234)",[f.to_string()]).unwrap();
    sql.execute("INSERT INTO event_log(space,kind,id,rev,action,actor,at,snapshot) VALUES('default','run','old-run',1,'create','owner',1234,?)",[row.to_string()]).unwrap();
    drop(sql);
    let mut db = Store::open(&path).unwrap();
    let old = hist(&mut db, "owner")["items"][0].clone();
    assert_eq!(old["id"], "old-run");
    assert!(old["trigger_actor"].is_null());
    let room = chat(&mut db, "owner", &old["id"]);
    assert_eq!(chat(&mut db, "owner", &old["id"])["id"], room["id"]);
    let messages = go(
        &mut db,
        "owner",
        json!({"op":"chat.history","id":room["id"]}),
    );
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert_eq!(messages[0]["fields"]["body"], "Original old instruction");
    assert_eq!(messages[1]["fields"]["body"], "Old result");
}
fn go(db: &mut Store, a: &str, q: Value) -> Value {
    db.execute(a, q.clone())
        .unwrap_or_else(|err| panic!("{a} {q}: {err}"))
}
fn setup() -> Store {
    let mut db = Store::memory().unwrap();
    for id in ["alice", "bob", "watch"] {
        go(
            &mut db,
            "owner",
            json!({"op":"create","kind":"member","id":id,"title":id,"fields":{"role":"member"}}),
        );
    }
    go(
        &mut db,
        "owner",
        json!({"op":"create","kind":"agent","id":"a","title":"A","fields":{"runtime":"fixture"}}),
    );
    go(
        &mut db,
        "owner",
        json!({"op":"create","kind":"automation","id":"r","title":"Check","fields":{"handler_id":"a","instruction":"Check this release","enabled":true,"notify":["watch"],"triggers":[{"id":"when-comment","type":"comment","keyword":"go"},{"id":"when-done","type":"status","status":"done","run_owner":"bob"}]}}),
    );
    go(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"task","title":"Release","fields":{}}),
    );
    db
}
fn hist(db: &mut Store, a: &str) -> Value {
    go(db, a, json!({"op":"automation.history","id":"r"}))
}
fn chat(db: &mut Store, actor: &str, run: &Value) -> Value {
    go(
        db,
        actor,
        json!({"op":"automation.conversation","id":"r","run":run}),
    )
}
const KEY: &str = "test-only-machine-key-long-enough-for-this-fixture";
#[test]
fn pausing_skips_pending_schedule_and_unavailable_trigger_owner_does_not_block_task_edits() {
    let mut db = setup();
    let schedule = go(
        &mut db,
        "owner",
        json!({"op":"create","kind":"automation","id":"periodic","title":"Periodic","fields":{"handler_id":"a","instruction":"Check","enabled":true,"triggers":[{"id":"hourly","type":"schedule","interval_minutes":60}]}}),
    );
    let tick = go(&mut db, "owner", json!({"op":"automation.tick"}));
    let scheduled = tick["runs"][0].clone();
    assert!(scheduled.is_string());
    let paused = go(
        &mut db,
        "owner",
        json!({"op":"automation.lifecycle","id":"periodic","action":"pause","expected_rev":schedule["rev"]}),
    );
    let r = go(
        &mut db,
        "owner",
        json!({"op":"get","kind":"run","id":scheduled}),
    );
    assert_eq!(r["fields"]["status"], "skipped");
    assert_eq!(r["fields"]["skip_reason"], "rule_paused");
    go(
        &mut db,
        "owner",
        json!({"op":"automation.lifecycle","id":"periodic","action":"resume","expected_rev":paused["rev"]}),
    );
    assert_eq!(
        go(&mut db, "owner", json!({"op":"automation.tick"}))["runs"],
        json!([])
    );
    go(
        &mut db,
        "owner",
        json!({"op":"update","kind":"member","id":"bob","fields":{"role":"viewer"}}),
    );
    go(
        &mut db,
        "alice",
        json!({"op":"update","kind":"task","id":"task","fields":{"status":"done"}}),
    );
    let blocked = hist(&mut db, "owner")["items"][0].clone();
    assert_eq!(blocked["status"], "skipped");
    assert_eq!(blocked["can_open"], false);
    assert!(
        db.execute(
            "bob",
            json!({"op":"automation.conversation","id":"r","run":blocked["id"]})
        )
        .is_err()
    );
}
fn machine(db: &mut Store) {
    go(
        db,
        "alice",
        json!({"op":"machine.register","machine_id":"m","machine_key":KEY,"runtimes":["fixture"],"title":"Machine"}),
    );
}
fn claim(db: &mut Store) -> Value {
    go(
        db,
        "alice",
        json!({"op":"run.claim","machine_id":"m","machine_key":KEY}),
    )
}
fn finish(db: &mut Store, lease: &Value, outcome: &str) -> Value {
    go(
        db,
        "alice",
        json!({"op":"run.report","id":lease["run"]["id"],"lease":lease["lease"],"machine_id":"m","machine_key":KEY,"status":outcome,"result":"Fixture output"}),
    )
}

#[test]
fn task_triggers_pick_actual_owner_and_the_conversation_rechecks_permissions() {
    let mut db = setup();
    machine(&mut db);
    go(
        &mut db,
        "alice",
        json!({"op":"create","kind":"comment","id":"c1","title":"go","fields":{"task":"task","body":"go verify"}}),
    );
    let rec = hist(&mut db, "alice")["items"][0].clone();
    assert_eq!(rec["source"], "comment");
    assert_eq!(rec["trigger_actor"], "alice");
    assert_eq!(rec["trigger_record"], "c1");
    assert_eq!(rec["owner"], "alice");
    let run = rec["id"].clone();
    let room = chat(&mut db, "alice", &run);
    let lease = claim(&mut db);
    assert_eq!(lease["run"]["fields"]["chat_id"], room["id"]);
    assert_eq!(lease["run"]["fields"]["unattended"], true);
    finish(&mut db, &lease, "done");
    let messages = go(
        &mut db,
        "alice",
        json!({"op":"chat.history","id":room["id"]}),
    );
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert_eq!(messages[0]["fields"]["author_kind"], "automation");
    assert!(
        db.execute("watch", json!({"op":"chat.history","id":room["id"]}))
            .is_err()
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"automation.conversation","id":"r","run":run})
        )
        .is_err()
    );
    let brief = hist(&mut db, "watch")["items"][0].clone();
    assert_eq!(brief["status"], "done");
    assert_eq!(brief["can_open"], false);
    assert!(brief.get("trigger_actor").is_none());
    let notes = go(&mut db, "watch", json!({"op":"list","kind":"notification"}));
    assert!(
        notes
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["fields"]["automation"] == "r" && r["fields"]["run"].is_null())
    );
    let continuation = go(
        &mut db,
        "alice",
        json!({"op":"chat.send","id":room["id"],"body":"Explain that output"}),
    );
    let l = claim(&mut db);
    assert_eq!(l["run"]["id"], continuation["run"]);
    assert_eq!(
        l["run"]["fields"]["automation_context"]["trigger_record"],
        "c1"
    );
    assert_eq!(l["run"]["fields"]["unattended"], true);
    finish(&mut db, &l, "done");
    go(
        &mut db,
        "alice",
        json!({"op":"update","kind":"task","id":"task","fields":{"status":"done"}}),
    );
    let b = hist(&mut db, "bob")["items"][0].clone();
    assert_eq!(b["owner"], "bob");
    assert_eq!(b["trigger_actor"], "alice");
    assert_eq!(b["trigger_record"], "task");
    assert_eq!(b["from_status"], "todo");
    assert_eq!(b["to_status"], "done");
    let b_room = chat(&mut db, "bob", &b["id"]);
    assert_ne!(b_room["id"], room["id"]);
    assert!(
        db.execute("alice", json!({"op":"get","kind":"chat","id":b_room["id"]}))
            .is_err()
    );
    go(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"task","fields":{"visibility":"private"}}),
    );
    assert!(
        db.execute("bob", json!({"op":"chat.history","id":b_room["id"]}))
            .is_err()
    );
    assert!(
        db.execute(
            "alice",
            json!({"op":"automation.conversation","id":"r","run":run})
        )
        .is_err()
    );
}

#[test]
fn cursor_ignores_newer_events_and_retry_preserves_original_input_and_conversation() {
    let mut db = setup();
    machine(&mut db);
    let manual =
        |id: &str| json!({"op":"automation.fire","id":"r","event_id":id,"payload":{"sample":id}});
    let first = go(&mut db, "alice", manual("first"));
    let l = claim(&mut db);
    let failed = finish(&mut db, &l, "waiting");
    assert_eq!(failed["fields"]["status"], "failed");
    assert_eq!(failed["fields"]["failure"]["category"], "unattended_input");
    let health = go(&mut db, "alice", json!({"op":"automation.status","id":"r"}));
    assert_eq!(health["health"], "attention");
    let retry = json!({"op":"automation.retry","id":"r","run":first["id"],"request_id":"retry-1"});
    assert!(db.execute("bob", retry.clone()).is_err());
    go(
        &mut db,
        "owner",
        json!({"op":"update","kind":"automation","id":"r","fields":{"instruction":"New rule instructions"}}),
    );
    let r = go(&mut db, "alice", retry.clone());
    assert_eq!(r["id"], first["id"]);
    assert_eq!(r["fields"]["chat_id"], first["fields"]["chat_id"]);
    assert_eq!(r["fields"]["instruction"], "Check this release");
    assert_eq!(go(&mut db, "alice", retry)["id"], first["id"]);
    let l = claim(&mut db);
    assert_eq!(l["run"]["fields"]["webhook_payload"]["sample"], "first");
    finish(&mut db, &l, "done");
    let mut expected: Vec<_> = vec![first["id"].clone()];
    for i in 1..7 {
        let v = go(&mut db, "alice", manual(&format!("event-{i}")));
        expected.push(v["id"].clone());
    }
    let first_page = go(
        &mut db,
        "alice",
        json!({"op":"automation.history","id":"r","limit":2}),
    );
    let mut ids = first_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect::<Vec<_>>();
    let mut cursor = first_page["next_cursor"].clone();
    let newer = go(&mut db, "alice", manual("new"));
    while !cursor.is_null() {
        let page = go(
            &mut db,
            "alice",
            json!({"op":"automation.history","id":"r","limit":2,"cursor":cursor}),
        );
        ids.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].clone()),
        );
        cursor = page["next_cursor"].clone();
    }
    assert_eq!(ids.len(), expected.len());
    assert!(!ids.contains(&newer["id"]));
    assert!(expected.iter().all(|id| ids.contains(id)));
    let r = go(
        &mut db,
        "owner",
        json!({"op":"get","kind":"automation","id":"r"}),
    );
    let change = |act: &str, rev: &Value| json!({"op":"automation.lifecycle","id":"r","action":act,"expected_rev":rev});
    let paused = go(&mut db, "owner", change("pause", &r["rev"]));
    assert!(db.execute("owner", change("resume", &r["rev"])).is_err());
    assert!(db.execute("alice", manual("blocked")).is_err());
    let active = go(&mut db, "owner", change("resume", &paused["rev"]));
    go(&mut db, "owner", change("archive", &active["rev"]));
    assert_eq!(
        go(&mut db, "alice", json!({"op":"automation.status","id":"r"}))["state"],
        "archived"
    );
    assert!(db.execute("alice", manual("blocked-again")).is_err());
    assert!(chat(&mut db, "alice", &first["id"])["id"].is_string());
}
