use serde_json::{Value, json};
use shipforge_core::Store;
use tempfile::tempdir;

fn apply(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn add(db: &mut Store, k: &str, id: &str, fields: Value) -> Value {
    apply(
        db,
        "owner",
        json!({"op":"create","kind":k,"id":id,"title":id,"fields":fields}),
    )
}
fn add_member(db: &mut Store, actor: &str, role: &str) {
    add(db, "member", actor, json!({"actor":actor,"role":role}));
}

#[test]
fn task_versions_idempotency_and_persistence() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("nested/workspace.sqlite3");
    let mut db = Store::open(&file).unwrap();
    let new = json!({"op":"create","kind":"task","id":"t","title":"Compile","request_id":"one"});
    let first = apply(&mut db, "owner", new.clone());
    assert_eq!(first["fields"]["number"], "SF-1");
    assert_eq!(apply(&mut db, "owner", new.clone()), first);
    let mut different = new;
    different["title"] = json!("other");
    assert!(
        db.execute("owner", different)
            .unwrap_err()
            .starts_with("conflict:")
    );
    let updated = apply(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"t","expected_rev":1,"fields":{"status":"done"}}),
    );
    assert_eq!(updated["rev"], 2);
    assert!(
        db.execute(
            "owner",
            json!({"op":"update","kind":"task","id":"t","expected_rev":1,"title":"stale"})
        )
        .unwrap_err()
        .starts_with("conflict:")
    );
    assert_eq!(
        apply(
            &mut db,
            "owner",
            json!({"op":"get","kind":"task","id":"SF-1"})
        ),
        updated
    );
    drop(db);
    let mut db = Store::open(&file).unwrap();
    assert_eq!(
        apply(&mut db, "owner", json!({"op":"get","kind":"task","id":"t"})),
        updated
    );
}
#[test]
fn memberships_private_versions_and_deep_search_are_enforced() {
    let mut db = Store::memory().unwrap();
    add_member(&mut db, "alice", "member");
    add_member(&mut db, "visitor", "viewer");
    apply(
        &mut db,
        "alice",
        json!({"op":"create","kind":"document","id":"d","title":"Secret note","fields":{"visibility":"private","body":"restricted"}}),
    );
    assert_eq!(
        apply(
            &mut db,
            "owner",
            json!({"op":"search","query":"restricted"})
        ),
        json!([])
    );
    assert!(
        db.execute("visitor", json!({"op":"get","kind":"document","id":"d"}))
            .is_err()
    );
    apply(
        &mut db,
        "alice",
        json!({"op":"update","kind":"document","id":"d","fields":{"visibility":"space","body":"published"}}),
    );
    let history = apply(
        &mut db,
        "visitor",
        json!({"op":"document.history","id":"d"}),
    );
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["version"], 2);
    assert!(
        db.execute("visitor", json!({"op":"create","kind":"task","title":"no"}))
            .unwrap_err()
            .starts_with("forbidden:")
    );
    apply(
        &mut db,
        "alice",
        json!({"op":"document.restore","id":"d","version":1}),
    );
    let doc = apply(
        &mut db,
        "alice",
        json!({"op":"get","kind":"document","id":"d"}),
    );
    assert_eq!(doc["fields"]["visibility"], "space");
    assert_eq!(doc["fields"]["body"], "restricted");
    assert_eq!(doc["rev"], 3);
    apply(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"member","id":"visitor"}),
    );
    assert!(
        db.execute("visitor", json!({"op":"search","query":"Secret"}))
            .is_err()
    );
}
#[test]
fn saved_views_query_current_work_and_iteration_transfer_is_atomic() {
    let mut db = Store::memory().unwrap();
    add(
        &mut db,
        "iteration",
        "s1",
        json!({"start_date":"2026-11-01","end_date":"2026-11-07"}),
    );
    add(&mut db, "iteration", "s2", json!({}));
    add(&mut db, "project", "big", json!({}));
    add(
        &mut db,
        "task",
        "a",
        json!({"iteration":"s1","project":"big","priority":"high","due_date":"2026-11-05"}),
    );
    add(
        &mut db,
        "task",
        "b",
        json!({"iteration":"s1","status":"done","project":"big"}),
    );
    add(
        &mut db,
        "view",
        "active",
        json!({"filter":{"status_not":"done","priority":"high"},"layout":"table"}),
    );
    assert_eq!(
        apply(&mut db, "owner", json!({"op":"view.run","id":"active"}))[0]["id"],
        "a"
    );
    let preview = apply(
        &mut db,
        "owner",
        json!({"op":"iteration.preview","id":"s1"}),
    );
    assert_eq!(preview.as_array().unwrap().len(), 1);
    assert!(
        db.execute(
            "owner",
            json!({"op":"iteration.close","id":"s1","target":"s1"})
        )
        .is_err()
    );
    let closed = apply(
        &mut db,
        "owner",
        json!({"op":"iteration.close","id":"s1","target":"s2"}),
    );
    assert_eq!(closed["moved"], json!(["a"]));
    assert_eq!(
        apply(&mut db, "owner", json!({"op":"get","kind":"task","id":"b"}))["fields"]["iteration"],
        "s1"
    );
    assert_eq!(
        apply(
            &mut db,
            "owner",
            json!({"op":"project.progress","id":"big"})
        )["completed"],
        1
    );
    let dash = apply(
        &mut db,
        "owner",
        json!({"op":"dashboard","today":"2026-11-06"}),
    );
    assert_eq!(dash["overdue"], 1);
    apply(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"a","fields":{"status":"done"}}),
    );
    assert_eq!(
        apply(&mut db, "owner", json!({"op":"view.run","id":"active"})),
        json!([])
    );
}
#[test]
fn task_types_required_fields_status_and_relations() {
    let mut db = Store::memory().unwrap();
    add(&mut db, "field", "effort", json!({"value_type":"number"}));
    add(
        &mut db,
        "status",
        "review",
        json!({"category":"in_progress"}),
    );
    add(
        &mut db,
        "task_type",
        "change",
        json!({"required_fields":["effort"],"statuses":["todo","review","done"]}),
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"create","kind":"task","title":"invalid","fields":{"type":"change"}})
        )
        .is_err()
    );
    add(
        &mut db,
        "task",
        "one",
        json!({"type":"change","custom":{"effort":2},"status":"review"}),
    );
    add(&mut db, "task", "two", json!({"parent":"one"}));
    add(
        &mut db,
        "relation",
        "r",
        json!({"type":"blocks","from":"one","to":"two"}),
    );
    assert!(db.execute("owner",json!({"op":"create","kind":"relation","title":"loop","fields":{"type":"blocks","from":"two","to":"one"}})).is_err());
    assert!(
        db.execute(
            "owner",
            json!({"op":"update","kind":"task","id":"one","fields":{"parent":"two"}})
        )
        .is_err()
    );
    assert_eq!(
        apply(&mut db, "owner", json!({"op":"task.relations","id":"one"}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"delete","kind":"status","id":"review"})
        )
        .is_err()
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"update","kind":"status","id":"review","title":"同伴审核"}),
    );
    assert_eq!(
        apply(
            &mut db,
            "owner",
            json!({"op":"list","kind":"task","filter":{"status_category":"in_progress"}})
        )[0]["id"],
        "one"
    );
}
#[test]
fn triage_permissions_notifications_and_follow_are_scoped() {
    let mut db = Store::memory().unwrap();
    add_member(&mut db, "alice", "member");
    add_member(&mut db, "guest", "guest");
    apply(
        &mut db,
        "owner",
        json!({"op":"update","kind":"space","id":"default","fields":{"triage_enabled":true,"triage_priority_required":true}}),
    );
    let task = apply(
        &mut db,
        "guest",
        json!({"op":"create","kind":"task","id":"request","title":"Feedback","fields":{"status":"done"}}),
    );
    assert_eq!(task["fields"]["status"], "triage");
    assert!(
        db.execute(
            "owner",
            json!({"op":"triage","id":"request","decision":"accept"})
        )
        .is_err()
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"triage","id":"request","decision":"accept","priority":"high"}),
    );
    apply(
        &mut db,
        "alice",
        json!({"op":"follow","id":"request","enabled":true}),
    );
    let note = apply(
        &mut db,
        "owner",
        json!({"op":"create","kind":"comment","title":"Please check","fields":{"task":"request","body":"Review this","mentions":["alice"]}}),
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"request","title":"Updated feedback"}),
    );
    let inbox = apply(&mut db, "alice", json!({"op":"list","kind":"notification"}));
    assert_eq!(inbox.as_array().unwrap().len(), 1);
    assert_eq!(inbox[0]["fields"]["count"], 2);
    let notif = inbox[0]["id"].as_str().unwrap();
    apply(
        &mut db,
        "alice",
        json!({"op":"notification.act","id":notif,"action":"clear"}),
    );
    assert!(
        db.execute(
            "guest",
            json!({"op":"notification.act","id":notif,"action":"read"})
        )
        .is_err()
    );
    assert_eq!(
        apply(
            &mut db,
            "alice",
            json!({"op":"get","kind":"comment","id":note["id"]})
        )["fields"]["task"],
        "request"
    );
    assert_eq!(
        apply(
            &mut db,
            "guest",
            json!({"op":"get","kind":"task","id":"request"})
        )["fields"]["status"],
        "todo"
    );
}
#[test]
fn preview_import_rolls_back_and_import_is_idempotent() {
    let mut source = Store::memory().unwrap();
    add(&mut source, "iteration", "s", json!({}));
    add(&mut source, "task", "child", json!({"iteration":"s"}));
    add(&mut source, "document", "doc", json!({"body":"content"}));
    let data = apply(&mut source, "owner", json!({"op":"export"}));
    let mut dest = Store::memory().unwrap();
    let preview = apply(
        &mut dest,
        "owner",
        json!({"op":"import.preview","data":data}),
    );
    assert_eq!(preview["created"], 3);
    assert_eq!(
        apply(&mut dest, "owner", json!({"op":"list","kind":"task"})),
        json!([])
    );
    let first = apply(&mut dest, "owner", json!({"op":"import","data":data}));
    assert_eq!(first["created"], 3);
    let again = apply(&mut dest, "owner", json!({"op":"import","data":data}));
    assert_eq!(again["skipped"], 3);
    let malformed = json!([{"kind":"task","id":"new","title":"new"},{"kind":"task","id":"bad","title":"bad","fields":{"iteration":"none"}}]);
    assert!(
        dest.execute("owner", json!({"op":"import","data":malformed}))
            .is_err()
    );
    assert!(
        dest.execute("owner", json!({"op":"get","kind":"task","id":"new"}))
            .is_err()
    );
}
#[test]
fn workspace_membership_and_aggregate_progress_cross_space() {
    let mut db = Store::memory().unwrap();
    add_member(&mut db, "a", "member");
    apply(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","id":"other","title":"Other"}),
    );
    add(&mut db, "project", "launch", json!({"spaces":["other"]}));
    add(&mut db, "task", "x", json!({"project":"launch"}));
    apply(
        &mut db,
        "owner",
        json!({"op":"create","space":"other","kind":"task","id":"y","title":"second","fields":{"project":"launch","status":"done"}}),
    );
    assert_eq!(
        apply(
            &mut db,
            "owner",
            json!({"op":"project.progress","id":"launch"})
        )["total"],
        2
    );
    assert_eq!(
        apply(&mut db, "a", json!({"op":"project.progress","id":"launch"}))["total"],
        1
    );
    assert_eq!(
        apply(&mut db, "a", json!({"op":"space.list"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"create","space":"other","kind":"member","title":"a","fields":{"actor":"a","role":"viewer"}}),
    );
    assert_eq!(
        apply(&mut db, "a", json!({"op":"space.list"}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        apply(&mut db, "a", json!({"op":"project.progress","id":"launch"}))["completed"],
        1
    );
}

#[test]
fn organization_guests_need_an_explicit_membership_even_in_public_spaces() {
    let mut db = Store::memory().unwrap();
    add_member(&mut db, "visitor", "guest");
    add_member(&mut db, "colleague", "member");
    for (id, visibility) in [("open-area", "public"), ("closed-area", "private")] {
        add(&mut db, "space", id, json!({"visibility":visibility}));
        apply(
            &mut db,
            "owner",
            json!({"op":"create","space":id,"kind":"task","id":"a","title":"Release checklist"}),
        );
    }
    let spaces = |db: &mut Store, actor: &str| {
        apply(db, actor, json!({"op":"space.list"}))
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(spaces(&mut db, "colleague").contains(&"open-area".to_owned()));
    assert!(!spaces(&mut db, "colleague").contains(&"closed-area".to_owned()));
    let verify_hidden = |db: &mut Store, space: &str| {
        assert!(!spaces(db, "visitor").iter().any(|id| id == space));
        for q in [
            json!({"op":"get","kind":"task","id":"a","space":space}),
            json!({"op":"search","query":"checklist","space":space}),
            json!({"op":"events","space":space}),
        ] {
            assert!(
                db.execute("visitor", q)
                    .unwrap_err()
                    .starts_with("forbidden:")
            );
        }
    };
    verify_hidden(&mut db, "open-area");
    verify_hidden(&mut db, "closed-area");
    for space in ["open-area", "closed-area"] {
        apply(
            &mut db,
            "owner",
            json!({"op":"create","space":space,"kind":"member","id":"visitor","title":"Visitor","fields":{"role":"viewer","actor":"visitor"}}),
        );
        assert_eq!(
            apply(
                &mut db,
                "visitor",
                json!({"op":"get","space":space,"kind":"task","id":"a"})
            )["title"],
            "Release checklist"
        );
        assert!(spaces(&mut db, "visitor").iter().any(|id| id == space));
        apply(
            &mut db,
            "owner",
            json!({"op":"delete","space":space,"kind":"member","id":"visitor"}),
        );
        verify_hidden(&mut db, space);
    }
}

#[test]
fn organization_admins_manage_private_space_metadata_without_receiving_its_content() {
    let mut db = Store::memory().unwrap();
    add_member(&mut db, "org-admin", "admin");
    add_member(&mut db, "someone", "member");
    add(
        &mut db,
        "space",
        "research",
        json!({"visibility":"private"}),
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"create","kind":"project","id":"work","title":"Shared target","fields":{"spaces":["default","research"]}}),
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"create","space":"research","kind":"task","id":"plan","title":"Restricted plan","fields":{"project":"work","status":"done"}}),
    );

    assert!(
        apply(&mut db, "org-admin", json!({"op":"space.list"}))
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "research")
    );
    assert!(
        apply(&mut db, "someone", json!({"op":"space.list"}))
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["id"] != "research")
    );
    let renamed = apply(
        &mut db,
        "org-admin",
        json!({"op":"update","space":"research","kind":"space","id":"research","title":"Research team","request_id":"rename-once"}),
    );
    assert_eq!(renamed["title"], "Research team");
    assert_eq!(
        apply(
            &mut db,
            "org-admin",
            json!({"op":"get","space":"research","kind":"space","id":"research"})
        ),
        renamed
    );
    for q in [
        json!({"op":"list","space":"research","kind":"member"}),
        json!({"op":"get","space":"research","kind":"task","id":"plan"}),
        json!({"op":"search","space":"research","query":"Restricted"}),
        json!({"op":"events","space":"research"}),
        json!({"op":"export","space":"research"}),
        json!({"op":"create","space":"research","kind":"member","id":"org-admin","title":"Admin","fields":{"actor":"org-admin","role":"admin"}}),
        json!({"op":"update","space":"research","kind":"space","id":"research","fields":{"visibility":"public"}}),
    ] {
        assert!(
            db.execute("org-admin", q)
                .unwrap_err()
                .starts_with("forbidden:")
        );
    }
    assert_eq!(
        apply(
            &mut db,
            "org-admin",
            json!({"op":"project.progress","id":"work"})
        )["total"],
        0
    );
    assert_eq!(
        apply(
            &mut db,
            "owner",
            json!({"op":"project.progress","id":"work"})
        )["total"],
        1
    );
    apply(
        &mut db,
        "owner",
        json!({"op":"create","space":"research","kind":"member","id":"org-admin","title":"Admin","fields":{"actor":"org-admin","role":"viewer"}}),
    );
    assert_eq!(
        apply(
            &mut db,
            "org-admin",
            json!({"op":"get","space":"research","kind":"task","id":"plan"})
        )["id"],
        "plan"
    );
    assert!(db.execute("org-admin",json!({"op":"update","kind":"task","space":"research","id":"plan","title":"Restricted edit"})).is_err());
    apply(
        &mut db,
        "owner",
        json!({"op":"delete","space":"research","kind":"member","id":"org-admin"}),
    );
    assert!(
        db.execute(
            "org-admin",
            json!({"op":"list","space":"research","kind":"task"})
        )
        .is_err()
    );
}
