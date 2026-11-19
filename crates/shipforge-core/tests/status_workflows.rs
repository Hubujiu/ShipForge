use serde_json::{Value, json};
use shipforge_core::Store;
use tempfile::tempdir;

fn run(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn space(db: &mut Store, id: &str) {
    run(
        db,
        "owner",
        json!({"op":"create","kind":"space","id":id,"title":id,"fields":{"visibility":"private","work_type":"software","task_types":["bug","task"]}}),
    );
}
fn get(db: &mut Store, space: &str, kind: &str, id: &str) -> Value {
    run(
        db,
        "owner",
        json!({"op":"get","space":space,"kind":kind,"id":id}),
    )
}
fn workflow(db: &mut Store, actor: &str, space: &str, statuses: Value, default: Value) -> Value {
    let rev = get(db, space, "task_type", "bug")["rev"].clone();
    run(
        db,
        actor,
        json!({"op":"task_type.workflow.save","space":space,"id":"bug","expected_rev":rev,"statuses":statuses,"default_status":default}),
    )
}
fn task(db: &mut Store, actor: &str, space: &str, id: &str, fields: Value) -> Value {
    run(
        db,
        actor,
        json!({"op":"create","kind":"task","space":space,"id":id,"title":id,"fields":fields}),
    )
}
fn catalog(db: &mut Store, actor: &str, space: &str, lang: &str, id: &str) -> Value {
    run(
        db,
        actor,
        json!({"op":"status.library","space":space,"language":lang}),
    )["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .unwrap()
}
#[test]
fn scoped_statuses_keep_independent_collisions_and_migrate_references_atomically() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("status.sqlite");
    let mut db = Store::open(&path).unwrap();
    space(&mut db, "first");
    space(&mut db, "second");
    for (scope, actor, role) in [
        ("default", "lead", "member"),
        ("first", "lead", "admin"),
        ("default", "viewer", "viewer"),
        ("first", "viewer", "viewer"),
        ("default", "org-admin", "admin"),
    ] {
        run(
            &mut db,
            "owner",
            json!({"op":"create","space":scope,"kind":"member","id":actor,"title":actor,"fields":{"role":role}}),
        );
    }
    run(
        &mut db,
        "lead",
        json!({"op":"create","kind":"status","space":"first","id":"review","title":"Local legacy","fields":{"category":"backlog"}}),
    );
    task(
        &mut db,
        "lead",
        "first",
        "legacy",
        json!({"type":"task","status":"review"}),
    );
    let cmd = json!({"op":"status.library.save","space":"first","id":"review","title":"Review fallback","fields":{"category":"in_progress","order":18,"collapsed":true,"color":"#339977","names":{"zh-CN":"待复核","en":"Quality review"}}});
    assert!(
        db.execute("lead", cmd.clone())
            .unwrap_err()
            .starts_with("forbidden")
    );
    run(&mut db, "owner", cmd);
    let source = catalog(&mut db, "lead", "first", "en", "review");
    assert_eq!(source["name"], "Quality review");
    assert!(source["local_id"].is_null());
    assert_eq!(
        catalog(&mut db, "lead", "first", "de", "review")["name"],
        "Review fallback"
    );
    let records = json!(["todo",{"library":"review"},"done"]);
    let first = workflow(
        &mut db,
        "lead",
        "first",
        records.clone(),
        json!({"library":"review"}),
    );
    let local = first["default_status"].as_str().unwrap().to_owned();
    assert_ne!(local, "review");
    assert_eq!(first["statuses"][1]["id"], local);
    assert_eq!(first["statuses"][1]["collapsed"], true);
    workflow(
        &mut db,
        "owner",
        "second",
        records,
        json!({"library":"review"}),
    );
    assert_eq!(
        task(&mut db, "lead", "first", "new", json!({"type":"bug"}))["fields"]["status"],
        local
    );
    assert_eq!(
        task(
            &mut db,
            "owner",
            "second",
            "secret",
            json!({"type":"bug","visibility":"private"})
        )["fields"]["status"],
        "review"
    );
    assert_eq!(
        catalog(&mut db, "viewer", "first", "zh-CN", "review")["usage"],
        json!({"tasks":1,"workflows":1,"spaces":1})
    );
    assert_eq!(
        catalog(&mut db, "org-admin", "default", "zh-CN", "review")["usage"],
        json!({"tasks":2,"workflows":2,"spaces":2})
    );
    assert!(
        db.execute(
            "org-admin",
            json!({"op":"get","kind":"task","space":"second","id":"secret"})
        )
        .is_err()
    );
    let revision = get(&mut db, "first", "task_type", "bug")["rev"].clone();
    for (actor, statuses, default) in [
        ("viewer", json!(["todo", local, "done"]), json!("todo")),
        ("lead", json!(["todo", "done"]), json!("todo")),
        ("lead", json!(["todo", local, "done"]), json!("done")),
        ("lead", json!(["todo", local, local, "done"]), json!("todo")),
    ] {
        assert!(db.execute(actor,json!({"op":"task_type.workflow.save","space":"first","id":"bug","expected_rev":revision,"statuses":statuses,"default_status":default})).is_err());
    }
    run(
        &mut db,
        "owner",
        json!({"op":"status.library.save","id":"review","expected_rev":source["rev"],"title":"Reviewed","fields":{"names":{"en":"Peer review"}}}),
    );
    assert_eq!(get(&mut db, "first", "status", &local)["title"], "Reviewed");
    assert_eq!(
        get(&mut db, "first", "status", "review")["title"],
        "Local legacy"
    );
    let preview = json!({"op":"status.library.removal","id":"review","target":"todo"});
    assert_eq!(
        run(
            &mut db,
            "org-admin",
            json!({"op":"status.library.removal","id":"review","target":"done"})
        )["reason"],
        "terminal_default"
    );
    let stale = run(&mut db, "org-admin", preview.clone());
    assert!(stale["can_remove"] == true);
    assert!(!stale.to_string().contains("secret"));
    run(
        &mut db,
        "lead",
        json!({"op":"update","kind":"task","space":"first","id":"new","title":"Changed"}),
    );
    assert!(db.execute("org-admin",json!({"op":"status.library.remove","id":"review","target":"todo","preview":stale["preview"]})).unwrap_err().starts_with("conflict"));
    assert_eq!(
        get(&mut db, "second", "task", "secret")["fields"]["status"],
        "review"
    );
    let current = run(&mut db, "org-admin", preview);
    run(
        &mut db,
        "org-admin",
        json!({"op":"status.library.remove","id":"review","target":"todo","preview":current["preview"]}),
    );
    drop(db);
    let mut db = Store::open(&path).unwrap();
    for (space, id) in [("first", "new"), ("second", "secret")] {
        assert_eq!(get(&mut db, space, "task", id)["fields"]["status"], "todo");
    }
    assert_eq!(
        get(&mut db, "first", "task", "legacy")["fields"]["status"],
        "review"
    );
    assert_eq!(
        get(&mut db, "first", "status", "review")["title"],
        "Local legacy"
    );
    assert_eq!(
        get(&mut db, "first", "task_type", "bug")["fields"]["statuses"],
        json!(["todo", "done"])
    );
    assert_eq!(
        get(&mut db, "first", "task_type", "bug")["fields"]["default_status"],
        "todo"
    );
}
#[test]
fn default_protection_retired_builtins_archived_tasks_and_customized_space_copy() {
    let mut db = Store::memory().unwrap();
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"status.library.removal","id":"todo","target":"backlog"})
        )["reason"],
        "organization_default"
    );
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"status.library.removal","id":"done","target":"cancelled"})
        )["reason"],
        "last_in_category"
    );
    assert!(db.execute("owner",json!({"op":"status.library.save","id":"gone","expected_rev":2,"title":"Gone","fields":{"category":"todo"}})).is_err());
    run(
        &mut db,
        "owner",
        json!({"op":"status.library.save","id":"released","title":"Released","fields":{"category":"done"}}),
    );
    space(&mut db, "history");
    space(&mut db, "detached");
    run(
        &mut db,
        "owner",
        json!({"op":"update","kind":"status","space":"detached","id":"done","title":"Independently renamed"}),
    );
    task(
        &mut db,
        "owner",
        "history",
        "archive",
        json!({"type":"task","status":"done"}),
    );
    let before = run(
        &mut db,
        "owner",
        json!({"op":"space.deactivate.preview","space":"history"}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"space.deactivate","space":"history","expected_rev":before["rev"],"preview_token":before["preview_token"]}),
    );
    let pre = run(
        &mut db,
        "owner",
        json!({"op":"status.library.removal","id":"done","target":"released"}),
    );
    assert!(pre["can_remove"] == true);
    run(
        &mut db,
        "owner",
        json!({"op":"status.library.remove","id":"done","target":"released","preview":pre["preview"]}),
    );
    assert_eq!(
        get(&mut db, "history", "task", "archive")["fields"]["status"],
        "released"
    );
    assert_eq!(
        get(&mut db, "detached", "status", "done")["title"],
        "Independently renamed"
    );
    space(&mut db, "later");
    assert!(
        db.execute(
            "owner",
            json!({"op":"get","space":"later","kind":"status","id":"done"})
        )
        .is_err()
    );
    assert_eq!(
        get(&mut db, "later", "status", "released")["fields"]["category"],
        "done"
    );
    let terminal = get(&mut db, "default", "status", "released");
    assert!(db.execute("owner",json!({"op":"status.library.save","id":"released","expected_rev":terminal["rev"],"fields":{"default":true}})).is_err());
    let old = get(&mut db, "default", "status", "backlog");
    run(
        &mut db,
        "owner",
        json!({"op":"status.library.save","id":"backlog","expected_rev":old["rev"],"fields":{"default":true}}),
    );
    assert_eq!(
        task(
            &mut db,
            "owner",
            "later",
            "implicit",
            json!({"type":"task"})
        )["fields"]["status"],
        "backlog"
    );
}
