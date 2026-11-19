use serde_json::{Value, json};
use shipforge_core::Store;

fn request(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q} {e}"))
}
fn member(db: &mut Store, id: &str, space: &str) {
    request(
        db,
        "owner",
        json!({"op":"create","kind":"member","space":space,"id":id,"title":"Directory peer","fields":{"role":"member"}}),
    );
}

#[test]
fn directory_and_search_recheck_each_space_and_page_after_filtering() {
    let mut db = Store::memory().unwrap();
    member(&mut db, "alice", "default");
    member(&mut db, "bob", "default");
    for space in ["team", "private"] {
        request(
            &mut db,
            "owner",
            json!({"op":"create","kind":"space","id":space,"title":space,"fields":{"visibility":"private"}}),
        );
        request(
            &mut db,
            "owner",
            json!({"op":"create","kind":"task","space":space,"id":"twin","title":"Duplicate title"}),
        );
        member(&mut db, "bob", space);
    }
    member(&mut db, "alice", "team");
    request(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"twin","title":"Duplicate title"}),
    );
    request(
        &mut db,
        "owner",
        json!({"op":"create","kind":"document","space":"team","id":"hidden","title":"Duplicate title","fields":{"visibility":"private"}}),
    );
    let q = json!({"op":"search","search_scope":"organization","query":"Duplicate title"});
    let rows = request(&mut db, "alice", q.clone());
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] == "twin" && r["space"] != "private")
    );
    assert_eq!(
        request(
            &mut db,
            "alice",
            json!({"op":"search","query":"Duplicate title"})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    let list = json!({"op":"directory.people","kind":"member","query":"Directory peer"});
    let people = request(&mut db, "alice", list.clone());
    assert!(
        people
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "bob" && r["space"] == "team")
    );
    assert!(!people.to_string().contains("private"));
    request(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"member","id":"alice","space":"team"}),
    );
    assert_eq!(request(&mut db, "alice", q).as_array().unwrap().len(), 1);
    assert!(
        request(&mut db, "alice", list)
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["space"] == "default")
    );
}

#[test]
fn direct_conversations_are_private_and_group_creation_is_atomic() {
    let mut db = Store::memory().unwrap();
    for id in ["alice", "bob", "outsider"] {
        member(&mut db, id, "default");
    }
    let q = json!({"op":"chat.start","mode":"member","target":"bob","request_id":"direct-repeat"});
    let first = request(&mut db, "alice", q.clone());
    assert_eq!(request(&mut db, "alice", q)["id"], first["id"]);
    assert_eq!(
        request(
            &mut db,
            "bob",
            json!({"op":"chat.start","mode":"member","target":"alice"})
        )["id"],
        first["id"]
    );
    assert!(
        db.execute("outsider", json!({"op":"chat.history","id":first["id"]}))
            .is_err()
    );
    assert!(
        db.execute(
            "alice",
            json!({"op":"chat.manage","id":first["id"],"action":"invite","target":"outsider"})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "alice",
            json!({"op":"update","kind":"chat","id":first["id"],"fields":{"visibility":"space"}})
        )
        .is_err()
    );
    assert!(db.execute("alice",json!({"op":"chat.start","mode":"group","title":"Rolled back","members":["bob","absent"]})).is_err());
    assert!(
        request(
            &mut db,
            "alice",
            json!({"op":"search","query":"Rolled back"})
        )
        .as_array()
        .unwrap()
        .is_empty()
    );
    let group = request(
        &mut db,
        "alice",
        json!({"op":"chat.start","mode":"group","title":"Local group","members":["bob"],"agents":[]}),
    );
    request(
        &mut db,
        "bob",
        json!({"op":"create","kind":"message","title":"Group hello","fields":{"chat":group["id"],"body":"Local group body"}}),
    );
    assert_eq!(
        request(
            &mut db,
            "alice",
            json!({"op":"chat.history","id":group["id"]})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert!(
        db.execute("outsider", json!({"op":"chat.history","id":group["id"]}))
            .is_err()
    );
    for agent in ["lead", "peer"] {
        request(
            &mut db,
            "owner",
            json!({"op":"create","kind":"agent","id":agent,"title":agent,"fields":{"visibility":"space"}}),
        );
    }
    request(
        &mut db,
        "owner",
        json!({"op":"create","kind":"crew","id":"both","title":"Both","fields":{"visibility":"space","leader_id":"lead","members":["lead","peer"]}}),
    );
    for (mode, target) in [("agent", "lead"), ("crew", "both")] {
        let room = request(
            &mut db,
            "alice",
            json!({"op":"chat.start","mode":mode,"target":target}),
        );
        assert_eq!(room["fields"]["handler_kind"], mode);
        assert_eq!(room["fields"]["handler_id"], target);
        assert_eq!(room["fields"]["visibility"], "private");
    }
}
