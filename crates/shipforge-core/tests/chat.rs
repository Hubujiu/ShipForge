use serde_json::{Value, json};
use shipforge_core::Store;

fn run(db: &mut Store, actor: &str, request: Value) -> Value {
    db.execute(actor, request.clone())
        .unwrap_or_else(|e| panic!("{request} as {actor}: {e}"))
}
fn workspace() -> Store {
    let mut db = Store::memory().unwrap();
    for name in ["a", "b", "c", "d"] {
        run(
            &mut db,
            "owner",
            json!({"op":"create","kind":"member","id":name,"title":name,"fields":{"actor":name,"role":"member"}}),
        );
    }
    db
}
fn room(db: &mut Store, group: bool) {
    run(
        db,
        "a",
        json!({"op":"create","kind":"chat","id":"room","title":"Project","fields":{"chat_type":if group {"group"}else{"conversation"}}}),
    );
}
fn manage(db: &mut Store, actor: &str, action: &str, target: &str) -> Value {
    run(
        db,
        actor,
        json!({"op":"chat.manage","id":"room","action":action,"target":target}),
    )
}
fn message(db: &mut Store, id: &str, mentions: Value, all: bool) -> Value {
    run(
        db,
        "a",
        json!({"op":"create","kind":"message","id":id,"title":id,"fields":{"chat":"room","body":id,"mentions":mentions,"mention_all":all}}),
    )
}
fn inbox(db: &mut Store, actor: &str) -> Vec<String> {
    run(db, actor, json!({"op":"list","kind":"notification"}))
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["fields"]["message"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn group_membership_ownership_mutes_and_history_are_scoped_to_the_caller() {
    let mut db = workspace();
    room(&mut db, true);
    for q in [
        json!({"op":"get","kind":"chat","id":"room"}),
        json!({"op":"chat.manage","id":"room","action":"add","target":"b"}),
    ] {
        assert!(db.execute("b", q).is_err());
    }
    for who in ["b", "c"] {
        manage(&mut db, "a", "add", who);
    }
    run(
        &mut db,
        "a",
        json!({"op":"create","kind":"agent","id":"assistant","title":"Assistant","fields":{"visibility":"private"}}),
    );
    run(
        &mut db,
        "a",
        json!({"op":"chat.manage","id":"room","target_kind":"agent","target":"assistant","action":"add"}),
    );
    assert_eq!(
        run(&mut db, "a", json!({"op":"chat.info","id":"room"}))["agents"],
        json!(["assistant"])
    );
    assert_eq!(
        run(&mut db, "b", json!({"op":"chat.info","id":"room"}))["agents"],
        json!([])
    );
    run(
        &mut db,
        "a",
        json!({"op":"chat.manage","id":"room","target_kind":"agent","target":"assistant","action":"remove"}),
    );
    for q in [
        json!({"op":"chat.manage","id":"room","action":"dissolve"}),
        json!({"op":"chat.manage","id":"room","action":"transfer","target":"c"}),
        json!({"op":"update","kind":"chat","id":"room","title":"changed"}),
        json!({"op":"delete","kind":"chat","id":"room"}),
    ] {
        assert!(db.execute("b", q).is_err());
    }
    run(
        &mut db,
        "b",
        json!({"op":"chat.preferences","id":"room","muted":true,"ignore_all":true}),
    );
    assert_eq!(
        run(&mut db, "a", json!({"op":"chat.info","id":"room"}))["muted"],
        false
    );
    message(&mut db, "plain", json!([]), false);
    message(&mut db, "everyone", json!([]), true);
    message(&mut db, "direct", json!(["b"]), true);
    assert_eq!(inbox(&mut db, "b"), vec!["direct"]);
    assert_eq!(inbox(&mut db, "c").len(), 3);
    assert!(db.execute("a",json!({"op":"create","kind":"message","title":"no","fields":{"chat":"room","mentions":["d"]}})).is_err());
    let clear = json!({"op":"chat.manage","id":"room","action":"clear","request_id":"clear-once"});
    run(&mut db, "b", clear.clone());
    assert!(
        db.execute("b", json!({"op":"get","kind":"message","id":"direct"}))
            .is_err()
    );
    assert!(inbox(&mut db, "b").is_empty());
    for actor in ["a", "c"] {
        assert_eq!(
            run(
                &mut db,
                actor,
                json!({"op":"get","kind":"message","id":"direct"})
            )["id"],
            "direct"
        );
    }
    message(&mut db, "later", json!(["b"]), false);
    run(&mut db, "b", clear);
    assert_eq!(inbox(&mut db, "b"), vec!["later"]);
    manage(&mut db, "a", "transfer", "b");
    assert_eq!(
        run(&mut db, "c", json!({"op":"chat.info","id":"room"}))["owner"],
        "b"
    );
    manage(&mut db, "b", "remove", "a");
    assert!(
        db.execute("a", json!({"op":"get","kind":"message","id":"later"}))
            .is_err()
    );
    manage(&mut db, "c", "leave", "");
    assert!(
        db.execute("c", json!({"op":"chat.info","id":"room"}))
            .is_err()
    );
    manage(&mut db, "b", "dissolve", "");
    assert!(
        db.execute("b", json!({"op":"chat.info","id":"room"}))
            .is_err()
    );
}

#[test]
fn authorizations_gate_invitations_history_search_events_and_attachment_downloads() {
    let mut db = workspace();
    room(&mut db, false);
    message(&mut db, "classified-evidence", json!([]), false);
    let upload = run(
        &mut db,
        "a",
        json!({"op":"attachment.upload","kind":"message","id":"classified-evidence","filename":"notes.txt","content_base64":"aGk="}),
    );
    let download = json!({"op":"attachment.download","kind":"message","id":"classified-evidence","attachment_id":upload["fields"]["attachments"][0]["id"]});
    manage(&mut db, "a", "invite", "b");
    assert_eq!(
        run(&mut db, "b", json!({"op":"chat.invitations"}))[0]["id"],
        "room"
    );
    assert_eq!(
        run(&mut db, "c", json!({"op":"chat.invitations"})),
        json!([])
    );
    assert!(db.execute("b", download.clone()).is_err());
    assert!(
        db.execute(
            "c",
            json!({"op":"chat.manage","id":"room","action":"accept"})
        )
        .is_err()
    );
    manage(&mut db, "b", "accept", "");
    assert_eq!(
        run(&mut db, "b", download.clone())["content_base64"],
        "aGk="
    );
    assert_eq!(
        run(
            &mut db,
            "b",
            json!({"op":"search","query":"classified-evidence"})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert!(
        run(&mut db, "b", json!({"op":"chat.info","id":"room"}))
            .get("members")
            .is_none()
    );
    manage(&mut db, "b", "invite", "c");
    manage(&mut db, "c", "decline", "");
    assert!(
        run(&mut db, "c", json!({"op":"chat.invitations"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    run(
        &mut db,
        "a",
        json!({"op":"create","kind":"artifact","id":"draft","title":"Draft","fields":{"message":"classified-evidence"}}),
    );
    assert_eq!(
        run(
            &mut db,
            "b",
            json!({"op":"get","kind":"artifact","id":"draft"})
        )["id"],
        "draft"
    );
    manage(&mut db, "a", "revoke", "b");
    for q in [
        download,
        json!({"op":"get","kind":"artifact","id":"draft"}),
        json!({"op":"chat.info","id":"room"}),
        json!({"op":"get","kind":"message","id":"classified-evidence"}),
    ] {
        assert!(db.execute("b", q).is_err());
    }
    assert_eq!(
        run(
            &mut db,
            "b",
            json!({"op":"search","query":"classified-evidence"})
        ),
        json!([])
    );
    assert!(
        !run(&mut db, "b", json!({"op":"events"}))
            .to_string()
            .contains("classified-evidence")
    );
    manage(&mut db, "a", "invite", "b");
    run(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"member","id":"b"}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","id":"b","title":"B","fields":{"actor":"b","role":"member"}}),
    );
    assert_eq!(
        run(&mut db, "b", json!({"op":"chat.invitations"})),
        json!([])
    );
}

#[test]
fn frozen_shares_enforce_scope_only_contain_selected_content_and_revoke_all_same_selection() {
    let mut db = workspace();
    room(&mut db, false);
    message(&mut db, "first", json!([]), false);
    message(&mut db, "second", json!([]), false);
    let upload = run(
        &mut db,
        "a",
        json!({"op":"attachment.upload","kind":"message","id":"first","filename":"note.txt","content_base64":"aGk="}),
    );
    let create = json!({"op":"chat.share.create","id":"room","message_ids":["second","first"],"request_id":"share-once"});
    let org = run(&mut db, "a", create.clone());
    assert_eq!(run(&mut db, "a", create.clone()), org);
    let public = run(
        &mut db,
        "a",
        json!({"op":"chat.share.create","id":"room","scope":"link","message_ids":["first","second"]}),
    );
    let single = run(
        &mut db,
        "a",
        json!({"op":"chat.share.create","id":"room","scope":"link","message_ids":["first"]}),
    );
    let org_key = org["share"].as_str().unwrap();
    let key = public["share"].as_str().unwrap();
    assert!(db.shared_conversation(org_key, None).is_err());
    assert!(db.shared_conversation(org_key, Some("absent")).is_err());
    let expected = run(
        &mut db,
        "b",
        json!({"op":"chat.share.read","share":org_key}),
    );
    assert_eq!(
        expected["messages"][0],
        json!({"author":"a","body":"first","attachments":["note.txt"]})
    );
    assert!(
        !expected
            .to_string()
            .contains(upload["fields"]["attachments"][0]["id"].as_str().unwrap())
    );
    run(
        &mut db,
        "a",
        json!({"op":"update","kind":"message","id":"first","fields":{"body":"edited"}}),
    );
    message(&mut db, "later", json!([]), false);
    let snapshot = db.shared_conversation(key, None).unwrap();
    assert_eq!(snapshot["messages"], expected["messages"]);
    assert!(
        db.execute(
            "b",
            json!({"op":"chat.share.revoke","id":"room","share":key})
        )
        .is_err()
    );
    assert_eq!(
        run(
            &mut db,
            "a",
            json!({"op":"chat.share.revoke","id":"room","share":key})
        )["revoked"],
        2
    );
    assert!(db.shared_conversation(key, None).is_err());
    assert!(db.shared_conversation(org_key, Some("b")).is_err());
    assert!(db.execute("a", create).is_err());
    assert!(
        db.shared_conversation(single["share"].as_str().unwrap(), None)
            .is_ok()
    );
}

#[test]
fn task_private_conversations_check_task_access_before_grants_and_after_task_visibility_changes() {
    let mut db = workspace();
    run(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"task","title":"Task","fields":{"visibility":"private"}}),
    );
    run(
        &mut db,
        "a",
        json!({"op":"create","kind":"chat","id":"room","title":"Task room","fields":{"task":"task"}}),
    );
    assert!(
        db.execute(
            "a",
            json!({"op":"chat.manage","id":"room","action":"invite","target":"b"})
        )
        .is_err()
    );
    run(
        &mut db,
        "a",
        json!({"op":"update","kind":"task","id":"task","fields":{"visibility":"space"}}),
    );
    manage(&mut db, "a", "invite", "b");
    manage(&mut db, "b", "accept", "");
    assert_eq!(
        run(&mut db, "b", json!({"op":"chat.info","id":"room"}))["owner"],
        "a"
    );
    run(
        &mut db,
        "a",
        json!({"op":"update","kind":"task","id":"task","fields":{"visibility":"private"}}),
    );
    assert!(
        db.execute("b", json!({"op":"chat.info","id":"room"}))
            .is_err()
    );
}
