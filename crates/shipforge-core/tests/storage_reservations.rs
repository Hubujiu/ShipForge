use serde_json::{Value, json};
use shipforge_core::Store;
const KEY: &str = "isolated-storage-machine-test-secret-at-least-32";
fn human(s: &mut Store, q: Value) -> Value {
    s.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q} · {e}"))
}
fn worker(s: &mut Store, mut q: Value) -> shipforge_core::Result<Value> {
    q["machine_id"] = json!("box");
    q["machine_key"] = json!(KEY);
    s.execute("owner", q)
}
#[test]
fn only_owner_can_clean_and_new_leases_wait_for_the_exact_snapshot_commit() {
    let mut s = Store::memory().unwrap();
    human(
        &mut s,
        json!({"op":"create","kind":"member","id":"peer","title":"Peer","fields":{"role":"member"}}),
    );
    worker(
        &mut s,
        json!({"op":"machine.register","title":"Storage","shared":true,"runtimes":["qa"]}),
    )
    .unwrap();
    human(
        &mut s,
        json!({"op":"create","kind":"agent","id":"a","title":"test","fields":{"runtime":"qa","machine_id":"box"}}),
    );
    let r = human(
        &mut s,
        json!({"op":"run.enqueue","handler_id":"a","instruction":"Finish"}),
    );
    let claim = worker(&mut s, json!({"op":"run.claim"})).unwrap();
    worker(&mut s,json!({"op":"run.report","id":r["id"],"lease":claim["lease"],"status":"done","result":"done","artifacts":[]})).unwrap();
    let id = "1".repeat(64);
    let snapshot = "a".repeat(64);
    human(
        &mut s,
        json!({"op":"machine.storage.request","id":"box","action":"scan"}),
    );
    let scan = worker(&mut s, json!({"op":"machine.storage.take"})).unwrap();
    worker(&mut s,json!({"op":"machine.storage.finish","id":scan["id"],"lease":scan["lease"],"outcome":"success","inventory":{"entries":[{"id":id,"snapshot":snapshot,"category":"run","bytes":42,"runs":[r["id"]]}]}})).unwrap();
    let req = json!({"op":"machine.storage.request","id":"box","action":"clean","confirm":true,"workspace":id,"expected_snapshot":snapshot,"request_id":"cleanup"});
    assert!(
        s.execute("peer", json!({"op":"machine.storage.preview","id":"box"}))
            .is_err()
    );
    assert!(s.execute("peer", req.clone()).is_err());
    let mut wrong = req.clone();
    wrong["expected_snapshot"] = json!("b".repeat(64));
    assert!(s.execute("owner", wrong).is_err());
    human(&mut s, req.clone());
    let job = worker(&mut s, json!({"op":"machine.storage.take"})).unwrap();
    assert!(
        worker(
            &mut s,
            json!({"op":"machine.storage.reserve","id":job["id"],"lease":"wrong"})
        )
        .is_err()
    );
    worker(
        &mut s,
        json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]}),
    )
    .unwrap();
    human(
        &mut s,
        json!({"op":"run.enqueue","handler_id":"a","instruction":"New work"}),
    );
    assert!(worker(&mut s, json!({"op":"run.claim"})).unwrap().is_null());
    worker(&mut s,json!({"op":"machine.storage.finish","id":job["id"],"lease":job["lease"],"outcome":"success","inventory":{"entries":[]}})).unwrap();
    assert!(
        worker(&mut s, json!({"op":"run.claim"}))
            .unwrap()
            .is_object()
    );
    assert_eq!(human(&mut s, req)["duplicate"], true);
    assert!(
        human(&mut s, json!({"op":"machine.storage.preview","id":"box"}))["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
