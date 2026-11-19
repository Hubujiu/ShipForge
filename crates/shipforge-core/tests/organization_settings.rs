use serde_json::{Value, json};
use shipforge_core::Store;
fn run(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
#[test]
fn organization_priority_import_preview_language_and_legacy_values() {
    let mut db = Store::memory().unwrap();
    let settings = json!({"op":"organization.settings"});
    let default = run(&mut db, "owner", settings.clone());
    assert_eq!(default["priority_levels"], 4);
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","id":"alice","title":"Alice","fields":{"role":"member"}}),
    );
    let config = json!({"op":"organization.configure","expected_rev":default["rev"],"default_language":"en"});
    assert!(db.execute("alice", config.clone()).is_err());
    run(&mut db, "owner", config);
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","id":"child","title":"Child"}),
    );
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"get","kind":"status","space":"child","id":"todo"})
        )["title"],
        "To do"
    );
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"get","kind":"status","id":"todo"})
        )["title"],
        "待处理"
    );
    let source = json!({"format":"shipforge.rows.v1","kind":"task","mapping":{"Title":"title","ID":"id","Level":"fields.priority"},"rows":[{"ID":"from-csv","Title":"Imported","Level":"最低"}]});
    run(
        &mut db,
        "owner",
        json!({"op":"import.preview","space":"child","data":source}),
    );
    assert_eq!(
        run(&mut db, "owner", settings.clone())["priority_levels"],
        4
    );
    run(
        &mut db,
        "owner",
        json!({"op":"import","space":"child","data":source}),
    );
    let fifth = run(&mut db, "owner", settings.clone());
    assert_eq!(fifth["priority_levels"], 5);
    let fourth = run(
        &mut db,
        "owner",
        json!({"op":"organization.configure","priority_levels":4,"expected_rev":fifth["rev"]}),
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"organization.configure","priority_levels":5,"expected_rev":fifth["rev"]})
        )
        .is_err()
    );
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"update","kind":"task","space":"child","id":"from-csv","title":"Still here"})
        )["fields"]["priority"],
        "lowest"
    );
    assert!(db.execute("owner",json!({"op":"create","kind":"task","space":"child","title":"New","fields":{"priority":"lowest"}})).is_err());
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","title":"No priority","fields":{"priority":"none"}}),
    );
    assert_eq!(run(&mut db, "owner", settings)["rev"], fourth["rev"]);
}
#[test]
fn leaving_and_dissolving_are_limited_to_the_selected_organization() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"organization","id":"secondary","title":"Secondary"}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","space":"secondary","id":"peer","title":"Peer","fields":{"role":"member"}}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","space":"secondary","id":"group","title":"Group"}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","space":"group","id":"peer","title":"Peer","fields":{"role":"member"}}),
    );
    let org = run(
        &mut db,
        "owner",
        json!({"op":"organization.settings","space":"group"}),
    );
    let delete = json!({"op":"organization.depart","space":"secondary","action":"dissolve","confirmation":"Secondary","expected_rev":org["rev"],"members":2});
    assert!(db.execute("peer", delete.clone()).is_err());
    let leave = json!({"op":"organization.depart","space":"group","action":"leave","confirmation":"Secondary","expected_rev":org["rev"]});
    assert!(db.execute("owner", leave.clone()).is_err());
    run(&mut db, "peer", leave);
    assert!(
        db.execute("peer", json!({"op":"whoami","space":"group"}))
            .is_err()
    );
    assert!(db.execute("owner", delete).is_err());
    run(
        &mut db,
        "owner",
        json!({"op":"organization.depart","space":"secondary","action":"dissolve","confirmation":"Secondary","expected_rev":org["rev"],"members":1}),
    );
    let spaces = run(&mut db, "owner", json!({"op":"space.list"}));
    assert_eq!(spaces.as_array().unwrap().len(), 1);
    assert_eq!(spaces[0]["id"], "default");
}
