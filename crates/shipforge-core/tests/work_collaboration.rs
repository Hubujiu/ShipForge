use serde_json::{Value, json};
use shipforge_core::Store;

fn apply(db: &mut Store, actor: &str, request: Value) -> Value {
    db.execute(actor, request.clone())
        .unwrap_or_else(|e| panic!("{request}: {e}"))
}
fn add(db: &mut Store, actor: &str, kind: &str, id: &str, f: Value) -> Value {
    apply(
        db,
        actor,
        json!({"op":"create","id":id,"kind":kind,"title":id,"fields":f}),
    )
}
fn get(db: &mut Store, kind: &str, id: &str) -> Value {
    apply(db, "owner", json!({"op":"get","kind":kind,"id":id}))
}
fn members(db: &mut Store) {
    for id in ["alice", "bob"] {
        add(db, "owner", "member", id, json!({"role":"member"}));
    }
}
#[test]
fn switch_and_unbind_preserve_common_hidden_and_legacy_values_atomically() {
    let mut db = Store::memory().unwrap();
    members(&mut db);
    for id in ["shared", "old", "next", "hidden"] {
        add(&mut db, "owner", "field", id, json!({"value_type":"text"}));
    }
    add(
        &mut db,
        "owner",
        "task_type",
        "from",
        json!({"statuses":["todo","done"],"required_fields":["shared"]}),
    );
    let target = json!([{"field":"shared","required":true},{"field":"hidden","enabled":false},{"field":"next","required":true}]);
    add(
        &mut db,
        "owner",
        "task_type",
        "to",
        json!({"field_bindings":target,"statuses":["done"]}),
    );
    let task = add(
        &mut db,
        "alice",
        "task",
        "one",
        json!({"type":"from","status":"todo","custom":{"old":"discard","shared":"characterization","hidden":"retain"}}),
    );
    let mut update = json!({"op":"update","kind":"task","id":"one","fields":{"type":"to"}});
    assert!(db.execute("alice", update.clone()).is_err());
    let preview = apply(
        &mut db,
        "alice",
        json!({"op":"task.type.preview","id":"one","target":"to"}),
    );
    assert_eq!(preview["remove"], json!(["old"]));
    assert_eq!(preview["missing"], json!(["next"]));
    assert_eq!(preview["status_allowed"], false);
    assert_eq!(get(&mut db, "task", "one")["rev"], task["rev"]);
    update["expected_rev"] = preview["expected_rev"].clone();
    update["expected_type_rev"] = preview["expected_type_rev"].clone();
    update["confirm_remove"] = preview["remove"].clone();
    assert!(db.execute("alice", update.clone()).is_err());
    update["fields"]["status"] = json!("done");
    update["fields"]["custom"] = json!({"next":"ready"});
    apply(
        &mut db,
        "bob",
        json!({"op":"update","kind":"task","id":"one","title":"concurrent edit"}),
    );
    assert!(db.execute("alice", update.clone()).is_err());
    update["expected_rev"] = get(&mut db, "task", "one")["rev"].clone();
    let changed = apply(&mut db, "alice", update);
    assert_eq!(
        changed["fields"]["custom"],
        json!({"next":"ready","shared":"characterization","hidden":"retain"})
    );
    assert!(db.execute("alice",json!({"op":"update","kind":"task","id":"one","fields":{"custom":{"hidden":"overwrite"}}})).is_err());
    let remaining = json!([{"field":"shared","required":true},{"field":"hidden","enabled":true}]);
    let q = json!({"op":"task_type.bindings.preview","id":"to","field_bindings":remaining});
    assert!(db.execute("alice", q.clone()).is_err());
    let p = apply(&mut db, "owner", q.clone());
    assert_eq!(p["counts"], json!({"next":1}));
    let mut change = json!({"op":"update","kind":"task_type","id":"to","expected_rev":p["expected_rev"],"confirm_remove":p["remove"],"impact_token":p["impact_token"],"fields":{"field_bindings":remaining}});
    apply(
        &mut db,
        "alice",
        json!({"op":"update","kind":"task","id":"one","fields":{"custom":{"next":"new value"}}}),
    );
    assert!(db.execute("owner", change.clone()).is_err());
    assert_eq!(
        get(&mut db, "task", "one")["fields"]["custom"]["next"],
        "new value"
    );
    change["impact_token"] = apply(&mut db, "owner", q)["impact_token"].clone();
    apply(&mut db, "owner", change);
    assert_eq!(
        get(&mut db, "task", "one")["fields"]["custom"],
        json!({"hidden":"retain","shared":"characterization"})
    );
    assert!(
        db.execute("owner", json!({"op":"delete","kind":"field","id":"hidden"}))
            .is_err()
    );
}

#[test]
fn private_view_order_save_as_and_shared_link_use_readers_live_acl_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("work.sqlite");
    let mut db = Store::open(&path).unwrap();
    members(&mut db);
    for id in ["p1", "p2", "p3"] {
        add(
            &mut db,
            "alice",
            "view",
            id,
            json!({"visibility":"private"}),
        );
    }
    add(
        &mut db,
        "bob",
        "view",
        "own",
        json!({"visibility":"private"}),
    );
    let shared = add(
        &mut db,
        "alice",
        "view",
        "团队 1?",
        json!({"visibility":"space","layout":"table","filter":{"priority":"high"},"group":"status","display_fields":["priority"]}),
    );
    add(
        &mut db,
        "alice",
        "task",
        "a",
        json!({"priority":"high","visibility":"private"}),
    );
    add(&mut db, "bob", "task", "b", json!({"priority":"high"}));
    add(&mut db, "bob", "task", "c", json!({"priority":"low"}));
    for id in ["p3", "p1"] {
        apply(
            &mut db,
            "alice",
            json!({"op":"view.organize","id":id,"pinned":true}),
        );
    }
    apply(
        &mut db,
        "alice",
        json!({"op":"view.organize","id":"p3","before":"p1"}),
    );
    assert!(
        db.execute(
            "alice",
            json!({"op":"view.organize","id":"p2","before":"p1"})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "bob",
            json!({"op":"view.organize","id":"团队 1?","pinned":true})
        )
        .is_err()
    );
    let cat = apply(&mut db, "alice", json!({"op":"view.catalog"}));
    assert_eq!(cat[0]["id"], "p3");
    assert_eq!(cat[1]["id"], "p1");
    assert_eq!(cat[2]["id"], "p2");
    assert!(
        apply(&mut db, "bob", json!({"op":"view.catalog"}))
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["fields"]["personally_pinned"] == false)
    );
    let changes = json!({"filter":{"priority":"low"},"layout":"list"});
    assert_eq!(
        apply(
            &mut db,
            "bob",
            json!({"op":"view.run","id":shared["id"],"changes":changes})
        )[0]["id"],
        "c"
    );
    let copy = apply(
        &mut db,
        "bob",
        json!({"op":"view.save_as","id":shared["id"],"title":"My low priority","changes":changes}),
    );
    assert_eq!(copy["fields"]["visibility"], "private");
    assert_eq!(copy["fields"]["group"], "status");
    assert_eq!(copy["fields"]["display_fields"], json!(["priority"]));
    assert!(
        db.execute(
            "bob",
            json!({"op":"update","kind":"view","id":shared["id"],"fields":{"layout":"board"}})
        )
        .is_err()
    );
    let link = apply(
        &mut db,
        "alice",
        json!({"op":"view.share","id":shared["id"]}),
    )["link"]
        .clone();
    let own = apply(&mut db, "alice", json!({"op":"view.share","id":"p3"}))["link"].clone();
    drop(db);
    let mut db = Store::open(&path).unwrap();
    let open = json!({"op":"view.open","link":link});
    assert_eq!(
        apply(&mut db, "alice", open.clone())["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let bob = apply(&mut db, "bob", open.clone());
    assert_eq!(bob["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(bob["tasks"][0]["id"], "b");
    assert_eq!(bob["view"]["rev"], shared["rev"]);
    assert!(
        db.execute("bob", json!({"op":"view.open","link":own}))
            .is_err()
    );
    assert!(
        Store::memory()
            .unwrap()
            .execute("owner", open.clone())
            .is_err()
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"member","id":"bob"}),
    );
    assert!(db.execute("bob", open).is_err());
}

#[test]
fn any_reply_locates_one_root_markers_move_and_private_or_invalid_threads_remain_closed() {
    let mut db = Store::memory().unwrap();
    members(&mut db);
    for id in ["issue", "other"] {
        add(&mut db, "alice", "task", id, json!({}));
    }
    add(
        &mut db,
        "alice",
        "comment",
        "root",
        json!({"task":"issue","body":"discussion"}),
    );
    add(
        &mut db,
        "bob",
        "comment",
        "reply",
        json!({"task":"issue","parent":"root","body":"feedback"}),
    );
    add(
        &mut db,
        "alice",
        "comment",
        "reply2",
        json!({"task":"issue","parent":"reply","body":"fix"}),
    );
    let mut q = json!({"op":"comment.resolve","task_id":"issue","id":"reply2","expected_rev":1});
    let root = apply(&mut db, "bob", q.clone());
    assert_eq!(root["id"], "root");
    assert_eq!(root["fields"]["resolved"], true);
    assert_eq!(root["fields"]["resolution_comment"], "reply2");
    q["id"] = json!("reply");
    assert!(db.execute("bob", q.clone()).is_err());
    q["expected_rev"] = root["rev"].clone();
    let moved = apply(&mut db, "alice", q);
    assert_eq!(moved["fields"]["resolution_comment"], "reply");
    let thread = apply(
        &mut db,
        "bob",
        json!({"op":"comment.thread","task_id":"issue","id":"reply2"}),
    );
    assert_eq!(thread["selected"], "reply2");
    assert_eq!(thread["comments"].as_array().unwrap().len(), 3);
    for wrong in [
        json!({"op":"comment.reopen","task_id":"other","id":"reply"}),
        json!({"op":"update","kind":"comment","id":"reply","fields":{"parent":"reply2"}}),
        json!({"op":"create","kind":"comment","title":"forged","fields":{"task":"issue","authored_by_agent":"fake"}}),
    ] {
        assert!(db.execute("owner", wrong).is_err());
    }
    assert_eq!(get(&mut db, "comment", "root")["rev"], moved["rev"]);
    let reopened = apply(
        &mut db,
        "bob",
        json!({"op":"comment.reopen","task_id":"issue","id":"reply2"}),
    );
    assert_eq!(reopened["fields"]["resolved"], false);
    assert!(reopened["fields"]["resolution_comment"].is_null());
    assert_eq!(
        get(&mut db, "comment", "reply")["fields"]["body"],
        "feedback"
    );
    add(
        &mut db,
        "alice",
        "comment",
        "hidden",
        json!({"task":"issue","parent":"root","visibility":"private"}),
    );
    add(
        &mut db,
        "alice",
        "comment",
        "below-hidden",
        json!({"task":"issue","parent":"hidden"}),
    );
    assert!(
        db.execute(
            "alice",
            json!({"op":"comment.resolve","task_id":"issue","id":"below-hidden"})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "bob",
            json!({"op":"comment.thread","task_id":"issue","id":"below-hidden"})
        )
        .is_err()
    );
    apply(
        &mut db,
        "alice",
        json!({"op":"update","kind":"task","id":"issue","fields":{"visibility":"private"}}),
    );
    assert!(
        db.execute(
            "bob",
            json!({"op":"comment.resolve","task_id":"issue","id":"reply"})
        )
        .is_err()
    );
}

#[test]
fn leased_agent_can_mark_its_own_root_and_cannot_forge_authorship_or_use_stale_lease() {
    let mut db = Store::memory().unwrap();
    let secret = "synthetic-machine-secret-with-at-least-32-characters";
    apply(
        &mut db,
        "owner",
        json!({"op":"machine.register","machine_id":"runner","machine_key":secret,"title":"Runner","runtimes":["local-tool"]}),
    );
    add(
        &mut db,
        "owner",
        "agent",
        "review",
        json!({"runtime":"local-tool"}),
    );
    add(
        &mut db,
        "owner",
        "task",
        "work",
        json!({"assignee":"review","assignee_kind":"agent"}),
    );
    add(
        &mut db,
        "owner",
        "comment",
        "human",
        json!({"task":"work","body":"Human root"}),
    );
    let lease = apply(
        &mut db,
        "owner",
        json!({"op":"run.claim","machine_id":"runner","machine_key":secret}),
    );
    let mut q = json!({"op":"run.discussion","machine_id":"runner","machine_key":secret,"id":lease["run"]["id"],"lease":lease["lease"],"action":"comment","call_id":"draft-1","body":"Agent root"});
    let root = apply(&mut db, "owner", q.clone());
    assert_eq!(root["fields"]["authored_by_agent"], "review");
    assert_eq!(apply(&mut db, "owner", q.clone())["id"], root["id"]);
    q["body"] = json!("Changed duplicate");
    assert!(db.execute("owner", q.clone()).is_err());
    q["action"] = json!("resolve");
    q["comment_id"] = json!("human");
    assert!(db.execute("owner", q.clone()).is_err());
    q["comment_id"] = root["id"].clone();
    assert_eq!(
        apply(&mut db, "owner", q.clone())["fields"]["resolved_by_agent"],
        "review"
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"run.report","id":lease["run"]["id"],"machine_id":"runner","machine_key":secret,"lease":lease["lease"],"status":"done"}),
    );
    q["action"] = json!("reopen");
    assert!(db.execute("owner", q).is_err());
}

#[test]
fn portable_transfer_restores_hidden_values_and_discussion_state_without_transferring_execution_credentials()
 {
    let mut source = Store::memory().unwrap();
    add(
        &mut source,
        "owner",
        "field",
        "history",
        json!({"value_type":"text"}),
    );
    let ty = add(
        &mut source,
        "owner",
        "task_type",
        "case",
        json!({"field_bindings":[{"field":"history"}]}),
    );
    add(
        &mut source,
        "owner",
        "task",
        "t",
        json!({"type":"case","custom":{"history":"retained"}}),
    );
    apply(
        &mut source,
        "owner",
        json!({"op":"update","kind":"task_type","id":"case","expected_rev":ty["rev"],"fields":{"field_bindings":[{"field":"history","enabled":false}]}}),
    );
    add(&mut source, "owner", "comment", "root", json!({"task":"t"}));
    add(
        &mut source,
        "owner",
        "comment",
        "answer",
        json!({"task":"t","parent":"root"}),
    );
    apply(
        &mut source,
        "owner",
        json!({"op":"comment.resolve","task_id":"t","id":"answer"}),
    );
    let mut data = apply(&mut source, "owner", json!({"op":"export"}));
    let forged = data["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["id"] == "root")
        .unwrap();
    forged["fields"]["authored_by_agent"] = json!("remote-agent");
    let mut dest = Store::memory().unwrap();
    apply(&mut dest, "owner", json!({"op":"import","data":data}));
    assert_eq!(
        get(&mut dest, "task", "t")["fields"]["custom"]["history"],
        "retained"
    );
    let root = get(&mut dest, "comment", "root");
    assert_eq!(root["fields"]["resolved"], true);
    assert_eq!(root["fields"]["resolution_comment"], "answer");
    assert!(root["fields"]["authored_by_agent"].is_null());
}
