use serde_json::{Value, json};
use shipforge_core::Store;
use std::sync::{Arc, Barrier};
use tempfile::tempdir;

fn run(db: &mut Store, q: Value) -> Value {
    db.execute("owner", q).unwrap()
}
#[test]
fn editable_builtin_configuration_and_portable_overrides() {
    let mut db = Store::memory().unwrap();
    let statuses = run(&mut db, json!({"op":"list","kind":"status"}));
    assert_eq!(statuses.as_array().unwrap().len(), 6);
    assert_eq!(statuses[0]["id"], "triage");
    assert_eq!(statuses[2]["fields"]["default"], true);
    run(
        &mut db,
        json!({"op":"update","kind":"status","id":"todo","title":"待验证"}),
    );
    run(
        &mut db,
        json!({"op":"create","kind":"status","id":"review","title":"审阅","fields":{"category":"in_progress","default":true,"order":12}}),
    );
    let task = run(
        &mut db,
        json!({"op":"create","kind":"task","id":"test","title":"Review test"}),
    );
    assert_eq!(task["fields"]["status"], "review");
    let usage = run(
        &mut db,
        json!({"op":"list","kind":"status","filter":{"id":"review"}}),
    );
    assert_eq!(usage[0]["fields"]["usage"], 1);
    assert!(
        db.execute(
            "owner",
            json!({"op":"delete","kind":"status","id":"triage"})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"update","kind":"status","id":"done","fields":{"category":"todo"}})
        )
        .is_err()
    );
    let export = run(&mut db, json!({"op":"export"}));
    let mut dest = Store::memory().unwrap();
    let result = run(&mut dest, json!({"op":"import","data":export}));
    assert_eq!(result["updated"], 1);
    assert_eq!(result["created"], 2);
    assert_eq!(
        run(&mut dest, json!({"op":"get","kind":"status","id":"todo"}))["title"],
        "待验证"
    );
    assert_eq!(
        run(&mut dest, json!({"op":"import","data":export}))["created"],
        0
    );
}

#[test]
fn column_mapping_has_a_live_preview_and_stable_identities() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        json!({"op":"create","kind":"field","id":"points","title":"Story points","fields":{"value_type":"number"}}),
    );
    let mut data = json!({"format":"shipforge.rows.v1","kind":"task","connector":"issue-csv","mapping":{"Key":"fields.external_id","Summary":"title","Points":"fields.custom.points","State":"fields.status"},"rows":[{"Key":9234,"Summary":"Print labels","Points":2,"State":"todo","Unmapped":"ignored"}]});
    let preview = run(&mut db, json!({"op":"import.preview","data":data}));
    assert_eq!(preview["sample"][0]["fields"]["custom"]["points"], 2);
    assert_eq!(preview["sample"][0]["fields"]["external_id"], "9234");
    assert_eq!(preview["mapping"], data["mapping"]);
    assert_eq!(run(&mut db, json!({"op":"list","kind":"task"})), json!([]));
    let result = run(&mut db, json!({"op":"import","data":data}));
    assert_eq!(result["created"], 1);
    let task = run(
        &mut db,
        json!({"op":"get","kind":"task","id":preview["sample"][0]["id"]}),
    );
    assert_eq!(task["fields"]["number"], "SF-1");
    data["rows"][0]["Summary"] = json!("changed in CSV");
    assert_eq!(
        run(&mut db, json!({"op":"import","data":data}))["skipped"],
        1
    );
    data["mapping"]["Another"] = json!("title");
    assert!(
        db.execute("owner", json!({"op":"import.preview","data":data}))
            .is_err()
    );
}

#[test]
fn file_conflicts_across_real_connections_and_foreign_file_protection() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("workspace.sqlite3");
    let mut db = Store::open(&file).unwrap();
    run(
        &mut db,
        json!({"op":"create","kind":"task","id":"t","title":"Race"}),
    );
    let barrier = Arc::new(Barrier::new(2));
    let threads:Vec<_>=(0..2).map(|i| {
        let file=file.clone();let gate=barrier.clone();
        std::thread::spawn(move||{
            let mut db=Store::open(&file).unwrap();
            gate.wait();
            db.execute("owner",json!({"op":"update","kind":"task","id":"t","title":format!("Edit {i}"),"expected_rev":1}))
        })
    }).collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|v| v.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|v| v.as_ref().is_err_and(|e| e.starts_with("conflict:")))
            .count(),
        1
    );
    let foreign = dir.path().join("foreign.sqlite3");
    rusqlite::Connection::open(&foreign).unwrap().execute_batch("CREATE TABLE customer_reference(n INTEGER PRIMARY KEY);INSERT INTO customer_reference VALUES(47)").unwrap();
    assert!(Store::open(&foreign).is_err());
    let other = rusqlite::Connection::open(foreign).unwrap();
    let count: i64 = other
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let v: i64 = other
        .query_row("SELECT n FROM customer_reference", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 47);
}

#[test]
fn closed_external_issues_stay_closed_when_triage_is_enabled() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        json!({"op":"update","kind":"space","id":"default","fields":{"triage_enabled":true}}),
    );
    let closed = run(
        &mut db,
        json!({"op":"create","kind":"task","title":"Closed upstream","fields":{"status":"done","source":"integration"}}),
    );
    assert_eq!(closed["fields"]["status"], "done");
    let incoming = run(
        &mut db,
        json!({"op":"create","kind":"task","title":"Needs triage","fields":{"source":"integration"}}),
    );
    assert_eq!(incoming["fields"]["status"], "triage");
}
