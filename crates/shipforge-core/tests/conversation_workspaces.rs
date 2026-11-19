use serde_json::{Value, json};
use shipforge_core::Store;

fn call(db: &mut Store, q: Value) -> Value {
    db.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}

#[test]
fn workspace_access_is_scoped_and_older_workers_cannot_consume_a_continuation() {
    let mut db = Store::memory().unwrap();
    let machine = "fixture";
    let secret = "machine-fixture-key-for-workspace-forks";
    for request in [
        json!({"op":"create","kind":"member","id":"peer","title":"Peer","fields":{"role":"member"}}),
        json!({"op":"create","kind":"agent","id":"builder","title":"Build"}),
        json!({"op":"create","kind":"chat","id":"source","title":"Review sources","fields":{"agent":"builder","visibility":"space"}}),
        json!({"op":"machine.register","machine_id":machine,"machine_key":secret,"title":"Machine","shared":true,"runtimes":["local-tool"]}),
    ] {
        call(&mut db, request);
    }
    let initial = call(
        &mut db,
        json!({"op":"chat.send","id":"source","body":"Begin"}),
    );
    let claim = call(
        &mut db,
        json!({"op":"run.claim","machine_id":machine,"machine_key":secret}),
    );
    call(
        &mut db,
        json!({"op":"run.report","machine_id":machine,"machine_key":secret,"id":claim["run"]["id"],"lease":claim["lease"],
        "status":"done","result":"Ready","workspace_checkpoint":{"git":true,"repository":"https://git.example.test/private/code.git"}}),
    );
    let owner_ctx = call(&mut db, json!({"op":"chat.workspace","id":"source"}));
    assert_eq!(owner_ctx["machine"]["id"], machine);
    let denied = db.execute("peer", json!({"op":"chat.workspace","id":"source"}));
    match denied {
        Ok(view) => assert_eq!(view, json!({"ready":false,"can_reuse":false})),
        Err(error) => assert!(error.starts_with("forbidden:") || error.starts_with("not_found:")),
    }
    let request = json!({"op":"chat.fork","id":"source","message":initial["message"],"workspace_mode":"reuse"});
    assert!(db.execute("peer", request.clone()).is_err());
    let fork = call(&mut db, request);
    let new = call(
        &mut db,
        json!({"op":"chat.send","id":fork["id"],"body":"Continue"}),
    );
    let old_worker = json!({"op":"run.claim","machine_id":machine,"machine_key":secret});
    assert!(call(&mut db, old_worker.clone()).is_null());
    let run = call(&mut db, json!({"op":"get","kind":"run","id":new["run"]}));
    assert_eq!(run["fields"]["waiting_reason"], "workspace_worker_upgrade");
    let mut upgraded = old_worker;
    upgraded["workspace_continuation"] = json!(true);
    let claimed = call(&mut db, upgraded);
    let plan = &claimed["run"]["fields"]["directory_fork"];
    assert_eq!(plan["mode"], "reuse");
    assert_eq!(plan["source_attempt"], 1);
    assert_eq!(plan["source_run"], claim["run"]["id"]);
    assert_eq!(plan["source_machine"], machine);
}
