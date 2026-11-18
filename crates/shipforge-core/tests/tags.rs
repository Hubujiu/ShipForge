use serde_json::{Value, json};
use shipforge_core::Store;

fn ok(s: &mut Store, actor: &str, q: Value) -> Value {
    s.execute(actor, q.clone())
        .unwrap_or_else(|error| panic!("{q}: {error}"))
}
fn add(s: &mut Store, space: &str, kind: &str, id: &str, name: &str, fields: Value) -> Value {
    ok(
        s,
        "owner",
        json!({"op":"create","space":space,"kind":kind,"id":id,"title":name,"fields":fields}),
    )
}
fn task(s: &mut Store, id: &str) -> Value {
    ok(s, "owner", json!({"op":"get","kind":"task","id":id}))
}

#[test]
fn exclusive_groups_filters_and_bulk_edits_use_the_same_label_rules() {
    let mut s = Store::memory().unwrap();
    add(&mut s, "default", "tag_group", "size", "Size", json!({}));
    add(
        &mut s,
        "default",
        "tag",
        "s",
        "Small",
        json!({"group":"size","color":"#112233"}),
    );
    add(
        &mut s,
        "default",
        "tag",
        "l",
        "Large",
        json!({"group":"size"}),
    );
    add(
        &mut s,
        "default",
        "task",
        "one",
        "First",
        json!({"labels":["s","Feature"]}),
    );
    add(
        &mut s,
        "default",
        "task",
        "two",
        "Second",
        json!({"labels":["s"]}),
    );
    let request = json!({"op":"tag.apply","tasks":["one","two"],"action":"add","labels":["l"],"request_id":"tag-both"});
    let receipt = ok(&mut s, "owner", request.clone());
    assert_eq!(ok(&mut s, "owner", request), receipt);
    assert_eq!(
        task(&mut s, "one")["fields"]["labels"],
        json!(["Feature", "l"])
    );
    assert_eq!(task(&mut s, "two")["fields"]["labels"], json!(["l"]));
    let filtered = ok(
        &mut s,
        "owner",
        json!({"op":"list","kind":"task","filter":{"labels":["l","Feature"]}}),
    );
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["id"], "one");
    assert_eq!(
        ok(
            &mut s,
            "owner",
            json!({"op":"search","kind":"task","query":"large"})
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
    assert!(
        s.execute(
            "owner",
            json!({"op":"tag.apply","tasks":["one","missing"],"action":"replace","labels":["new"]})
        )
        .is_err()
    );
    assert_eq!(
        task(&mut s, "one")["fields"]["labels"],
        json!(["Feature", "l"])
    );
    assert!(
        s.execute("owner", json!({"op":"get","kind":"tag","id":"new"}))
            .is_err()
    );
    ok(
        &mut s,
        "owner",
        json!({"op":"tag.apply","tasks":["one","two"],"action":"clear_group","group":"size"}),
    );
    assert_eq!(task(&mut s, "one")["fields"]["labels"], json!(["Feature"]));
    assert_eq!(task(&mut s, "two")["fields"]["labels"], json!([]));
}

#[test]
fn tag_lifecycle_preserves_archived_history_and_rewrites_saved_views() {
    let mut s = Store::memory().unwrap();
    add(
        &mut s,
        "default",
        "task",
        "old",
        "Old",
        json!({"labels":["frontend"]}),
    );
    add(&mut s, "default", "task", "fresh", "Fresh", json!({}));
    add(
        &mut s,
        "default",
        "view",
        "frontend-only",
        "UI work",
        json!({"filter":{"labels":["frontend"]}}),
    );
    add(&mut s, "default", "tag", "ui", "User interface", json!({}));
    ok(
        &mut s,
        "owner",
        json!({"op":"update","kind":"tag","id":"frontend","fields":{"archived":true}}),
    );
    assert_eq!(task(&mut s, "old")["fields"]["labels"], json!(["frontend"]));
    assert!(
        s.execute(
            "owner",
            json!({"op":"update","kind":"task","id":"fresh","fields":{"labels":["frontend"]}})
        )
        .is_err()
    );
    ok(
        &mut s,
        "owner",
        json!({"op":"tag.merge","id":"frontend","target":"ui"}),
    );
    assert_eq!(task(&mut s, "old")["fields"]["labels"], json!(["ui"]));
    assert_eq!(
        ok(
            &mut s,
            "owner",
            json!({"op":"view.run","id":"frontend-only"})
        )[0]["id"],
        "old"
    );
    assert!(
        s.execute("owner", json!({"op":"get","kind":"tag","id":"frontend"}))
            .is_err()
    );
    ok(
        &mut s,
        "owner",
        json!({"op":"delete","kind":"tag","id":"ui"}),
    );
    assert_eq!(task(&mut s, "old")["fields"]["labels"], json!([]));
}

#[test]
fn organization_tags_cross_spaces_and_space_labels_stay_at_the_source() {
    let mut s = Store::memory().unwrap();
    add(
        &mut s,
        "default",
        "space",
        "next",
        "Second",
        json!({"key":"NX"}),
    );
    add(
        &mut s,
        "default",
        "organization",
        "other",
        "Other org",
        json!({}),
    );
    add(
        &mut s,
        "default",
        "tag",
        "feedback",
        "Customer feedback",
        json!({"scope":"organization"}),
    );
    add(
        &mut s,
        "default",
        "tag",
        "root-only",
        "Root only",
        json!({}),
    );
    assert_eq!(
        ok(
            &mut s,
            "owner",
            json!({"op":"list","kind":"tag","space":"next"})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        ok(
            &mut s,
            "owner",
            json!({"op":"list","kind":"tag","space":"other"})
        ),
        json!([])
    );
    add(
        &mut s,
        "default",
        "member",
        "member",
        "Member",
        json!({"role":"member"}),
    );
    assert!(
        s.execute(
            "member",
            json!({"op":"update","kind":"tag","id":"feedback","fields":{"archived":true}})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "member",
            json!({"op":"delete","kind":"tag","id":"feedback"})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "owner",
            json!({"op":"create","space":"next","kind":"tag","title":"new","id":"feedback"})
        )
        .is_err()
    );
    add(
        &mut s,
        "default",
        "task",
        "customer",
        "Customer",
        json!({"labels":["feedback","root-only"]}),
    );
    ok(
        &mut s,
        "owner",
        json!({"op":"update","kind":"tag","id":"feedback","fields":{"archived":true}}),
    );
    let moved = ok(
        &mut s,
        "owner",
        json!({"op":"task.move","id":"customer","target":"next"}),
    );
    assert_eq!(moved["fields"]["labels"], json!(["feedback"]));
    add(
        &mut s,
        "default",
        "tag",
        "customer-flag",
        "Customer flag",
        json!({"scope":"organization"}),
    );
    ok(
        &mut s,
        "owner",
        json!({"op":"tag.merge","id":"feedback","target":"customer-flag"}),
    );
    assert_eq!(
        ok(
            &mut s,
            "owner",
            json!({"op":"get","kind":"task","id":"customer","space":"next"})
        )["fields"]["labels"],
        json!(["customer-flag"])
    );
}

#[test]
fn legacy_label_text_upgrades_without_changing_user_history_or_awarding_the_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("旧工作区.db");
    let mut store = Store::open(&path).unwrap();
    add(
        &mut store,
        "default",
        "member",
        "illustrator",
        "Illustrator",
        json!({"role":"member"}),
    );
    let actor = "illustrator";
    ok(
        &mut store,
        actor,
        json!({"op":"create","kind":"tag","id":"known","title":"Design"}),
    );
    let before = ok(
        &mut store,
        actor,
        json!({"op":"create","kind":"task","id":"old","title":"Before upgrade"}),
    );
    ok(
        &mut store,
        actor,
        json!({"op":"create","kind":"view","id":"saved","title":"Old filter","fields":{"filter":{"labels":["Artwork/UX"]}}}),
    );
    let credits = ok(&mut store, "owner", json!({"op":"onboarding.get"}))["points"].clone();
    drop(store);
    // A v0.7 task stored displayed labels directly, including titles containing a slash.
    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy.execute("UPDATE records SET fields=json_set(fields,'$.labels',json(?)) WHERE kind='task' AND id='old'",
        [r#"["Design","Artwork/UX","Legacy"]"#]).unwrap();
    legacy.execute("DELETE FROM label_migrations", []).unwrap();
    drop(legacy);
    let mut store = Store::open(&path).unwrap();
    let labels = ok(&mut store, actor, json!({"op":"list","kind":"tag"}));
    let generated = labels
        .as_array()
        .unwrap()
        .iter()
        .find(|label| label["title"] == "Artwork/UX")
        .unwrap()["id"]
        .clone();
    assert_ne!(generated, "Artwork/UX");
    let after = task(&mut store, "old");
    assert_eq!(
        after["fields"]["labels"],
        json!(["known", generated, "Legacy"])
    );
    assert_eq!(after["rev"], before["rev"]);
    assert_eq!(after["updated_at"], before["updated_at"]);
    assert_eq!(
        ok(&mut store, actor, json!({"op":"view.run","id":"saved"}))[0]["id"],
        "old"
    );
    assert_eq!(
        ok(&mut store, "owner", json!({"op":"onboarding.get"}))["points"],
        credits
    );
    drop(store);
    let mut reopened = Store::open(&path).unwrap();
    assert_eq!(task(&mut reopened, "old"), after);
    assert_eq!(
        ok(&mut reopened, actor, json!({"op":"list","kind":"tag"})),
        labels
    );
}

#[test]
fn bulk_change_cannot_reveal_or_partially_modify_another_members_private_task() {
    let mut store = Store::memory().unwrap();
    add(
        &mut store,
        "default",
        "member",
        "colleague",
        "Colleague",
        json!({"role":"member"}),
    );
    add(&mut store, "default", "task", "shared", "Shared", json!({}));
    add(
        &mut store,
        "default",
        "task",
        "private",
        "Private",
        json!({"visibility":"private"}),
    );
    let result = store.execute(
        "colleague",
        json!({"op":"tag.apply","tasks":["shared","private"],"action":"add","labels":["Draft"]}),
    );
    assert!(result.unwrap_err().contains("not_found"));
    assert_eq!(task(&mut store, "shared")["fields"]["labels"], json!([]));
    assert_eq!(
        ok(&mut store, "colleague", json!({"op":"list","kind":"tag"})),
        json!([])
    );
}
