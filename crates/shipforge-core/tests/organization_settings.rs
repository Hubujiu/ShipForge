use serde_json::{Value, json};
use shipforge_core::Store;
fn run(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}

#[test]
fn transferred_ownership_survives_old_account_departure_and_rejects_the_last_demotion() {
    let mut db = Store::memory().unwrap();
    let a = db.verified_login("first@example.test").unwrap()["actor"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = db.verified_login("second@example.test").unwrap()["actor"]
        .as_str()
        .unwrap()
        .to_owned();
    let org = run(&mut db, &a, json!({"op":"organization.list"}))[0]["id"].clone();
    let settings = json!({"op":"organization.settings","space":org});
    let start = run(&mut db, &a, settings.clone());
    run(
        &mut db,
        &a,
        json!({"op":"create","space":org,"kind":"member","id":b,"title":"Partner","fields":{"role":"admin"}}),
    );
    let set = json!({"op":"organization.ownership","space":org,"expected_rev":start["rev"],"action":"grant","target":b});
    assert!(db.execute(&b, set.clone()).is_err());
    assert!(
        db.execute(
            &b,
            json!({"op":"update","space":org,"kind":"member","id":a,"fields":{"role":"member"}})
        )
        .is_err()
    );
    let shared = run(&mut db, &a, set);
    assert_eq!(shared["owners"], 2);
    assert!(db.execute(&a,json!({"op":"organization.ownership","space":org,"expected_rev":start["rev"],"action":"revoke","target":a})).is_err());
    assert_eq!(run(&mut db, &b, settings.clone())["can_dissolve"], true);
    let transfer = run(
        &mut db,
        &a,
        json!({"op":"organization.ownership","space":org,"expected_rev":shared["rev"],"action":"transfer","target":b}),
    );
    assert_eq!(transfer["can_dissolve"], false);
    assert!(db.execute(&b,json!({"op":"organization.ownership","space":org,"expected_rev":transfer["rev"],"action":"revoke","target":b})).is_err());
    assert!(
        db.execute(
            &b,
            json!({"op":"delete","space":org,"kind":"member","id":b})
        )
        .is_err()
    );
    let impact = db.account_departure(&a).unwrap();
    assert!(
        impact["organizations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == org && o["action"] == "leave")
    );
    db.retire_verified_account(&a, false).unwrap();
    let retained = run(&mut db, &b, settings);
    assert_eq!(retained["members"], 1);
    assert_eq!(retained["can_dissolve"], true);
}
#[test]
fn child_space_transfer_requires_current_owner_joined_successor_and_fresh_revision() {
    let mut db = Store::memory().unwrap();
    let first = db.verified_login("space-owner@example.test").unwrap()["actor"]
        .as_str()
        .unwrap()
        .to_owned();
    let org = run(&mut db, &first, json!({"op":"organization.list"}))[0]["id"].clone();
    for id in ["successor", "outside", "reader"] {
        run(
            &mut db,
            &first,
            json!({"op":"create","kind":"member","space":org,"id":id,"title":id,"fields":{"role":"member"}}),
        );
    }
    let org_settings = run(
        &mut db,
        &first,
        json!({"op":"organization.settings","space":org}),
    );
    run(
        &mut db,
        &first,
        json!({"op":"organization.ownership","space":org,"target":"successor","action":"grant","expected_rev":org_settings["rev"]}),
    );
    let child = run(
        &mut db,
        &first,
        json!({"op":"create","kind":"space","space":org,"id":"child","title":"Shared work"}),
    );
    for (id, role) in [("successor", "member"), ("reader", "viewer")] {
        run(
            &mut db,
            &first,
            json!({"op":"create","kind":"member","space":"child","id":id,"title":id,"fields":{"role":role}}),
        );
    }
    let task = run(
        &mut db,
        &first,
        json!({"op":"create","kind":"task","space":"child","title":"Retain this task"}),
    );
    let transfer = json!({"op":"space.transfer","space":"child","target":"successor","expected_rev":child["rev"]});
    let mut stale = transfer.clone();
    stale["expected_rev"] = json!(0);
    assert!(db.execute("successor", transfer.clone()).is_err());
    assert!(db.execute(&first, stale).is_err());
    for invalid in ["outside", "reader", &first] {
        let mut q = transfer.clone();
        q["target"] = json!(invalid);
        assert!(db.execute(&first, q).is_err());
    }
    assert!(
        db.execute(&first, json!({"op":"space.leave","space":"child"}))
            .is_err()
    );
    let impact = db.account_departure(&first).unwrap();
    assert_eq!(impact["spaces_to_transfer"][0]["id"], "child");
    assert!(db.retire_verified_account(&first, true).is_err());
    let transferred = run(&mut db, &first, transfer);
    assert_eq!(transferred["fields"]["creator"], "successor");
    assert_eq!(transferred["fields"]["organization"], org);
    assert!(db.execute(&first,json!({"op":"update","space":"child","kind":"member","id":"successor","fields":{"role":"viewer"}})).is_err());
    assert_eq!(
        run(
            &mut db,
            "successor",
            json!({"op":"get","space":"child","kind":"member","id":"successor"})
        )["fields"]["role"],
        "admin"
    );
    run(&mut db, &first, json!({"op":"space.leave","space":"child"}));
    assert_eq!(
        db.account_departure(&first).unwrap()["spaces_to_transfer"],
        json!([])
    );
    db.retire_verified_account(&first, false).unwrap();
    assert_eq!(
        run(
            &mut db,
            "successor",
            json!({"op":"get","space":"child","kind":"task","id":task["id"]})
        )["title"],
        "Retain this task"
    );
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
