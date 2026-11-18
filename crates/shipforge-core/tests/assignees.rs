use serde_json::{Value, json};
use shipforge_core::Store;

fn run(db: &mut Store, q: Value) -> Value {
    db.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn team() -> Store {
    let mut db = Store::memory().unwrap();
    for id in ["alice", "bob", "chen"] {
        run(
            &mut db,
            json!({"op":"create","kind":"member","id":id,"title":id,"fields":{"role":"member"}}),
        );
    }
    run(
        &mut db,
        json!({"op":"create","kind":"agent","id":"robot","title":"Robot","fields":{"visibility":"space"}}),
    );
    db
}
fn multiple(db: &mut Store, on: bool) {
    run(
        db,
        json!({"op":"update","kind":"space","id":"default","fields":{"allow_multiple_assignees":on}}),
    );
}
fn create(db: &mut Store) {
    run(
        db,
        json!({"op":"create","kind":"task","id":"t","title":"Shared design","fields":{"status":"todo","assignees":["alice","bob"],"executor":{"kind":"agent","id":"robot"}}}),
    );
}
#[test]
fn equal_assignees_keep_one_execution_one_status_and_independent_notifications() {
    let mut db = team();
    let q = json!({"op":"create","kind":"task","title":"Invalid","fields":{"assignees":["alice","bob"]}});
    assert!(
        db.execute("owner", q)
            .unwrap_err()
            .contains("enable multiple")
    );
    multiple(&mut db, true);
    create(&mut db);
    for who in ["alice", "bob"] {
        let my_tasks = db
            .execute(
                who,
                json!({"op":"list","kind":"task","filter":{"assignee":"@me"}}),
            )
            .unwrap();
        assert_eq!(my_tasks.as_array().unwrap().len(), 1);
        assert_eq!(my_tasks[0]["fields"]["executor"]["id"], "robot");
        let inbox = db
            .execute(who, json!({"op":"list","kind":"notification"}))
            .unwrap();
        assert!(
            inbox
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["fields"]["task"] == "t" && r["fields"]["importance"] == "important")
        );
    }
    let append = json!({"op":"task.assign","id":"t","mode":"append","assignees":["bob","chen"],"request_id":"extra"});
    let expanded = run(&mut db, append.clone());
    assert_eq!(
        expanded["fields"]["assignees"],
        json!(["alice", "bob", "chen"])
    );
    assert_eq!(expanded, run(&mut db, append));
    assert_eq!(
        run(&mut db, json!({"op":"list","kind":"run"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    multiple(&mut db, false);
    let changed = run(
        &mut db,
        json!({"op":"task.assign","id":"t","assignees":["chen","bob"]}),
    );
    assert_eq!(changed["fields"]["assignees"], json!(["chen", "bob"]));
    assert_eq!(changed["fields"]["assignee"], "robot");
    assert!(
        db.execute(
            "owner",
            json!({"op":"task.assign","id":"t","assignees":["stranger"]})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"task.assign","id":"t","assignees":["bob","bob"]})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"task.assign","id":"t","executor":{"kind":"member","id":"alice"}})
        )
        .is_err()
    );
    let cleared = run(
        &mut db,
        json!({"op":"task.assign","id":"t","executor":null}),
    );
    assert_eq!(cleared["fields"]["assignees"], json!(["chen", "bob"]));
    db.execute(
        "bob",
        json!({"op":"update","kind":"task","id":"t","fields":{"status":"done"}}),
    )
    .unwrap();
    assert_eq!(
        db.execute("chen", json!({"op":"get","kind":"task","id":"t"}))
            .unwrap()["fields"]["status"],
        "done"
    );
    run(&mut db, json!({"op":"delete","kind":"member","id":"bob"}));
    assert_eq!(
        run(&mut db, json!({"op":"get","kind":"task","id":"t"}))["fields"]["assignees"],
        json!(["chen"])
    );
}

#[test]
fn old_single_assignment_and_private_executor_are_independent_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.sqlite3");
    {
        let mut db = Store::open(&path).unwrap();
        run(
            &mut db,
            json!({"op":"create","kind":"member","id":"alice","title":"Alice","fields":{"role":"member"}}),
        );
        run(
            &mut db,
            json!({"op":"create","kind":"agent","id":"private","title":"Personal","fields":{"visibility":"private"}}),
        );
        run(
            &mut db,
            json!({"op":"create","kind":"task","id":"t","title":"Legacy","fields":{"status":"backlog","assignee_kind":"member","assignee":"alice"}}),
        );
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute("UPDATE records SET fields=json_remove(fields,'$.executor','$.assignees') WHERE kind='task'",[]).unwrap();
    drop(raw);
    let mut db = Store::open(&path).unwrap();
    assert_eq!(
        run(&mut db, json!({"op":"get","kind":"task","id":"t"}))["fields"]["assignees"],
        json!(["alice"])
    );
    assert!(
        db.execute(
            "alice",
            json!({"op":"task.assign","id":"t","executor":{"kind":"agent","id":"private"}})
        )
        .is_err()
    );
    let worker = run(
        &mut db,
        json!({"op":"update","kind":"task","id":"t","fields":{"assignee_kind":"agent","assignee":"private"}}),
    );
    assert_eq!(worker["fields"]["assignees"], json!(["alice"]));
    let human = run(
        &mut db,
        json!({"op":"update","kind":"task","id":"t","fields":{"assignee_kind":"member","assignee":"owner"}}),
    );
    assert_eq!(human["fields"]["assignees"], json!(["owner"]));
    assert_eq!(human["fields"]["executor"]["id"], "private");
}

#[test]
fn imported_and_synchronized_people_survive_single_person_spaces() {
    let mut db = team();
    let data = json!([{"kind":"task","id":"imported","title":"Import","fields":{"assignees":["alice","bob","external-id"]}}]);
    run(&mut db, json!({"op":"import.preview","data":data}));
    run(&mut db, json!({"op":"import","data":data}));
    let imported = run(&mut db, json!({"op":"get","kind":"task","id":"imported"}));
    assert_eq!(imported["fields"]["assignees"], json!(["alice", "bob"]));
    assert_eq!(
        imported["fields"]["source_assignees"],
        json!(["external-id"])
    );
    run(
        &mut db,
        json!({"op":"create","kind":"connector","id":"git","title":"GitLab","fields":{"provider":"gitlab","enabled":true,"assignee_map":{"10":"alice","20":"bob"}}}),
    );
    let mut fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/inbound.json")).unwrap();
    let mut p = fixtures.remove(0)["payload"].clone();
    p["assignees"] = json!([{"id":10},{"id":20},{"id":30}]);
    let event = json!({"op":"provider.ingest","id":"git","provider":"gitlab","event_id":"source-event","payload":p});
    let first = run(&mut db, event.clone());
    assert_eq!(first, run(&mut db, event));
    let saved = run(
        &mut db,
        json!({"op":"get","kind":"task","id":first["task"]}),
    );
    assert_eq!(saved["fields"]["assignees"], json!(["alice", "bob"]));
    assert_eq!(saved["fields"]["source_assignees"], json!(["30"]));
    run(
        &mut db,
        json!({"op":"task.assign","id":first["task"],"assignees":["chen"]}),
    );
    p["assignees"] = json!([{"id":10}]);
    p["object_attributes"]["updated_at"] = json!("2026-01-10 13:00:00 UTC");
    let mut update = json!({"op":"provider.ingest","id":"git","provider":"gitlab","event_id":"source-update","payload":p});
    let conflict = run(&mut db, update.clone());
    assert_eq!(conflict["fields"], json!(["assignees"]));
    update["resolve"] = json!("source");
    run(&mut db, update);
    let resolved = run(
        &mut db,
        json!({"op":"get","kind":"task","id":first["task"]}),
    );
    assert_eq!(resolved["fields"]["assignees"], json!(["alice"]));
    assert_eq!(resolved["fields"]["source_assignees"], json!([]));
}
