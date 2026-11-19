use serde_json::{Value, json};
use shipforge_core::Store;
fn call(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{actor} {q}: {e}"))
}
fn workspace() -> Store {
    let mut db = Store::memory().unwrap();
    for actor in ["a", "b", "w"] {
        call(
            &mut db,
            "owner",
            json!({"op":"create","kind":"member","id":actor,"title":actor,"fields":{"role":"member"}}),
        );
    }
    db
}
fn comment(db: &mut Store, id: &str, task: &str, fields: Value) {
    let mut data = fields;
    data["task"] = json!(task);
    call(
        db,
        "a",
        json!({"op":"create","kind":"comment","id":id,"title":id,"fields":data}),
    );
}
#[test]
fn group_filters_priority_and_current_section_bulk_actions_do_not_change_the_task() {
    let mut db = workspace();
    let _issue = call(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"x","title":"Grouped task","fields":{"assignee":"b"}}),
    );
    call(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"second","title":"Second"}),
    );
    call(&mut db, "b", json!({"op":"follow","id":"second"}));
    comment(&mut db, "first", "x", json!({"body":"Noisy kettle"}));
    comment(
        &mut db,
        "second-reply",
        "second",
        json!({"body":"A bright window"}),
    );
    let q = json!({"op":"inbox.query","section":"all"});
    let result = call(&mut db, "b", q.clone());
    assert_eq!(result["unread"], json!({"all":2,"important":1,"other":1}));
    let single = call(
        &mut db,
        "b",
        json!({"op":"inbox.query","section":"important","query":"kettle"}),
    );
    let notif = single["rows"][0]["id"].clone();
    assert_eq!(single["rows"][0]["fields"]["count"], 2);
    let other = call(
        &mut db,
        "b",
        json!({"op":"inbox.bulk","action":"read","section":"other","query":"bright"}),
    );
    assert_eq!(other["changed"], 1);
    let opened = call(&mut db, "b", json!({"op":"inbox.open","id":notif}));
    assert_eq!(opened["target"]["id"], "x");
    assert_eq!(opened["comment"], "first");
    let row = &call(
        &mut db,
        "b",
        json!({"op":"inbox.query","section":"important"}),
    )["rows"][0];
    assert_eq!(row["id"], notif);
    assert_eq!(row["fields"]["read"], true);
    let until = (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp_millis();
    call(
        &mut db,
        "b",
        json!({"op":"notification.act","id":notif,"action":"snooze","until":until}),
    );
    comment(&mut db, "ordinary", "x", json!({}));
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"inbox.query","section":"snoozed"})
        )["rows"][0]["id"],
        notif
    );
    comment(&mut db, "mention", "x", json!({"mentions":["b"]}));
    let awake = call(
        &mut db,
        "b",
        json!({"op":"inbox.query","section":"important"}),
    );
    assert_eq!(awake["rows"][0]["id"], notif);
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"inbox.bulk","section":"important","action":"clear","statuses":["done"]})
        )["changed"],
        0
    );
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"inbox.bulk","section":"important","action":"clear"})
        )["changed"],
        1
    );
    assert_eq!(call(&mut db, "b", q)["rows"].as_array().unwrap().len(), 1);
    assert!(
        db.execute("w", json!({"op":"inbox.open","id":notif}))
            .is_err()
    );
    assert_eq!(
        call(&mut db, "b", json!({"op":"get","kind":"task","id":"x"}))["rev"],
        1
    );
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"get","kind":"comment","id":"mention"})
        )["rev"],
        1
    );
}
#[test]
fn muting_one_field_preserves_other_events_in_the_same_task_update() {
    let mut db = workspace();
    call(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"t","title":"Plan","fields":{"assignee":"b"}}),
    );
    call(
        &mut db,
        "b",
        json!({"op":"inbox.configure","channel":"inbox","key":"assigned_priority","enabled":false}),
    );
    let initial = call(&mut db, "b", json!({"op":"inbox.feed"}));
    call(
        &mut db,
        "a",
        json!({"op":"update","kind":"task","id":"t","title":"Revised plan","fields":{"priority":"high"}}),
    );
    let delivered = call(
        &mut db,
        "b",
        json!({"op":"inbox.feed","after":initial["cursor"]}),
    );
    assert_eq!(delivered["items"].as_array().unwrap().len(), 1);
    assert_eq!(delivered["items"][0]["event"], "task_updated");
}
#[test]
fn notification_routes_honor_per_person_overrides_and_recheck_source_permissions() {
    let mut db = workspace();
    let initial = call(&mut db, "b", json!({"op":"inbox.feed"}));
    let mut q = json!({"op":"inbox.configure","channel":"desktop","key":"assignment","enabled":false,"expected_rev":0});
    call(&mut db, "b", q.clone());
    assert!(db.execute("b", q.clone()).is_err());
    call(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"t","title":"Shared","fields":{"assignee":"b"}}),
    );
    let feed = json!({"op":"inbox.feed","after":initial["cursor"]});
    assert_eq!(call(&mut db, "b", feed.clone())["items"], json!([]));
    let old = call(&mut db, "b", json!({"op":"inbox.query"}));
    assert_eq!(old["total"], 1);
    q = json!({"op":"inbox.configure","channel":"desktop","reset":true});
    call(&mut db, "b", q);
    assert_eq!(call(&mut db, "b", feed.clone())["items"], json!([]));
    comment(
        &mut db,
        "ping",
        "t",
        json!({"mentions":["b"],"body":"Salt lamp"}),
    );
    let delivered = call(&mut db, "b", feed.clone());
    assert_eq!(delivered["items"].as_array().unwrap().len(), 1);
    let item = &delivered["items"][0];
    let (origin, scope, id) =
        shipforge_core::inbox_link_parts(item["link"].as_str().unwrap()).unwrap();
    assert!(
        db.execute(
            "b",
            json!({"op":"inbox.open","id":id,"origin":"different database"})
        )
        .is_err()
    );
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"inbox.locate","space":scope,"id":id,"origin":origin})
        )["comment"],
        "ping"
    );
    call(
        &mut db,
        "a",
        json!({"op":"update","kind":"comment","id":"ping","fields":{"visibility":"private"}}),
    );
    assert_eq!(call(&mut db, "b", feed)["items"], json!([]));
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"inbox.query","query":"Salt lamp"})
        )["total"],
        0
    );
    assert!(call(&mut db, "b", json!({"op":"inbox.open","id":id}))["comment"].is_null());
    call(
        &mut db,
        "b",
        json!({"op":"inbox.configure","channel":"inbox","key":"mentions","enabled":false}),
    );
    comment(&mut db, "mute", "t", json!({"mentions":["b","w"]}));
    assert_eq!(
        call(
            &mut db,
            "b",
            json!({"op":"inbox.feed","after":delivered["cursor"]})
        )["items"],
        json!([])
    );
    assert_eq!(
        call(
            &mut db,
            "w",
            json!({"op":"inbox.query","section":"important"})
        )["total"],
        1
    );
    call(
        &mut db,
        "b",
        json!({"op":"inbox.configure","landing":"important"}),
    );
    assert_eq!(
        call(&mut db, "b", json!({"op":"inbox.query"}))["section"],
        "important"
    );
    call(
        &mut db,
        "b",
        json!({"op":"inbox.configure","landing":"other"}),
    );
    assert_eq!(
        call(&mut db, "b", json!({"op":"inbox.query"}))["section"],
        "all"
    );
}
#[test]
fn automatic_member_fields_and_manual_following_are_independent_and_idempotent() {
    let mut db = workspace();
    call(
        &mut db,
        "owner",
        json!({"op":"create","kind":"field","id":"reviewers","title":"Reviewers","fields":{"value_type":"members"}}),
    );
    call(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"t","title":"Issue","fields":{"custom":{"reviewers":["b","w"]}}}),
    );
    let list = json!({"op":"followers.list","id":"t"});
    let people = call(&mut db, "a", list.clone());
    assert!(
        people["followers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "b")
    );
    assert_eq!(call(&mut db, "b", json!({"op":"inbox.query"}))["total"], 0);
    call(
        &mut db,
        "w",
        json!({"op":"follow","id":"t","enabled":false}),
    );
    call(
        &mut db,
        "a",
        json!({"op":"update","kind":"task","id":"t","title":"Updated"}),
    );
    assert_eq!(call(&mut db, "w", json!({"op":"inbox.query"}))["total"], 0);
    call(
        &mut db,
        "a",
        json!({"op":"update","kind":"task","id":"t","fields":{"custom":{"reviewers":[]}}}),
    );
    assert!(
        call(&mut db, "a", list)["followers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "b")
    );
    let add = json!({"op":"followers.change","id":"t","target":"w","enabled":true});
    call(&mut db, "a", add.clone());
    call(&mut db, "a", add);
    let w = &call(&mut db, "w", json!({"op":"inbox.query"}))["rows"][0];
    assert_eq!(w["fields"]["event"], "follower_added");
    assert_eq!(w["fields"]["count"], 1);
}
#[test]
fn accepted_addressed_invitation_converts_to_a_follower_without_publishing_private_work() {
    let mut db = workspace();
    call(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","id":"lab","title":"Lab","fields":{"key":"LAB","visibility":"private"}}),
    );
    for (id, vis) in [("public-task", "space"), ("hidden-task", "private")] {
        call(
            &mut db,
            "owner",
            json!({"op":"create","kind":"task","space":"lab","id":id,"title":id,"fields":{"visibility":vis}}),
        );
    }
    let invite = call(
        &mut db,
        "owner",
        json!({"op":"invitation.issue","channel":"email","email":"new@example.test","role":"member","spaces":["lab"]}),
    );
    for id in ["public-task", "hidden-task"] {
        call(
            &mut db,
            "owner",
            json!({"op":"followers.change","space":"lab","id":id,"target_kind":"invitation","target":invite["invitation"]["id"],"enabled":true}),
        );
    }
    call(
        &mut db,
        "new@example.test",
        json!({"op":"invitation.accept","link":invite["link"]}),
    );
    let followers = call(
        &mut db,
        "new@example.test",
        json!({"op":"followers.list","space":"lab","id":"public-task"}),
    );
    assert!(
        followers["followers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "member" && f["id"] == "new@example.test")
    );
    assert!(
        db.execute(
            "new@example.test",
            json!({"op":"followers.list","space":"lab","id":"hidden-task"})
        )
        .is_err()
    );
    assert_eq!(
        call(
            &mut db,
            "new@example.test",
            json!({"op":"inbox.query","space":"lab"})
        )["total"],
        0
    );
}
#[test]
fn reactions_are_scoped_and_repeated_set_does_not_duplicate_the_authors_notice() {
    let mut db = workspace();
    call(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"t","title":"Review"}),
    );
    comment(&mut db, "review", "t", json!({}));
    let reaction = json!({"op":"comment.react","id":"review","emoji":"👍","enabled":true});
    call(&mut db, "b", reaction.clone());
    let repeated = call(&mut db, "b", reaction);
    assert_eq!(
        repeated["reactions"][0],
        json!({"emoji":"👍","count":1,"mine":true})
    );
    let inbox = call(&mut db, "a", json!({"op":"inbox.query"}));
    assert_eq!(inbox["rows"][0]["fields"]["event"], "reaction");
    assert_eq!(inbox["rows"][0]["fields"]["count"], 1);
    call(
        &mut db,
        "b",
        json!({"op":"comment.react","id":"review","emoji":"👍","enabled":false}),
    );
    assert_eq!(
        call(
            &mut db,
            "a",
            json!({"op":"comment.reactions","id":"review"})
        )["reactions"],
        json!([])
    );
}
