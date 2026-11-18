use serde_json::{Value, json};
use shipforge_core::Store;

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/inbound.json")).unwrap()
}
fn connector(db: &mut Store, provider: &str) {
    db.execute("owner",json!({"op":"create","kind":"connector","id":provider,"title":provider,"fields":{"provider":provider,"enabled":true}})).unwrap();
}
fn envelope(case: &Value, op: &str) -> Value {
    let mut q = json!({"op":op,"id":case["provider"],"provider":case["provider"],"payload":case["payload"]});
    if let Some(event) = case.get("delivery") {
        q["event_id"] = event.clone();
    }
    q
}
#[test]
fn five_provider_fixtures_preview_apply_and_replay() {
    let mut db = Store::memory().unwrap();
    for case in cases() {
        let provider = case["provider"].as_str().unwrap();
        connector(&mut db, provider);
        let p = db
            .execute("owner", envelope(&case, "provider.preview"))
            .unwrap();
        assert_eq!(
            json!([p["external_id"], p["title"], p["description"], p["closed"]]),
            case["expected"]
        );
        assert_eq!(p["source_version"].as_str().unwrap().len(), 20);
        assert!(!p["event_id"].as_str().unwrap().is_empty());
        let task_count = |db: &mut Store| {
            db.execute("owner", json!({"op":"list","kind":"task"}))
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        };
        let before = task_count(&mut db);
        let q = envelope(&case, "provider.ingest");
        let created = db.execute("owner", q.clone()).unwrap();
        assert_eq!(created["action"], "created");
        assert_eq!(db.execute("owner", q).unwrap(), created);
        assert_eq!(task_count(&mut db), before + 1);
        let row = db
            .execute(
                "owner",
                json!({"op":"get","kind":"task","id":created["task"]}),
            )
            .unwrap();
        assert_eq!(row["fields"]["description"], case["expected"][2]);
        assert_eq!(row["fields"]["connector"], provider);
    }
}
#[test]
fn issue_updates_reconcile_source_order_and_manual_conflicts() {
    let fixtures = cases();
    for case in &fixtures[..2] {
        let mut db = Store::memory().unwrap();
        let provider = case["provider"].as_str().unwrap();
        connector(&mut db, provider);
        let mut q = envelope(case, "provider.ingest");
        let initial = db.execute("owner", q.clone()).unwrap();
        let old = q.clone();
        let task = &initial["task"];
        q["event_id"] = json!("second-delivery");
        if provider == "gitlab" {
            let a = &mut q["payload"]["object_attributes"];
            a["updated_at"] = json!("2026-01-10T13:00:00.003Z");
            a["state"] = json!("closed");
            a["action"] = json!("close");
            a["title"] = json!("Revised issue");
        } else {
            q["payload"]["webhookEvent"] = json!("jira:issue_updated");
            let f = &mut q["payload"]["issue"]["fields"];
            f["updated"] = json!("2026-01-10T13:00:00.003+0000");
            f["status"]["statusCategory"]["key"] = json!("done");
            f["summary"] = json!("Revised issue");
        }
        db.execute(
            "owner",
            json!({"op":"update","kind":"task","id":task,"title":"Local edit"}),
        )
        .unwrap();
        let conflict = db.execute("owner", q.clone()).unwrap();
        assert_eq!(conflict["action"], "conflict");
        assert_eq!(conflict["fields"], json!(["title"]));
        q["resolve"] = json!("source");
        let done = db.execute("owner", q).unwrap();
        assert_eq!(done["action"], "updated");
        assert_eq!(&done["task"], task);
        let mut stale = old;
        stale["event_id"] = json!("late-old-version");
        assert_eq!(db.execute("owner", stale).unwrap()["action"], "unchanged");
        let row = db
            .execute("owner", json!({"op":"get","kind":"task","id":task}))
            .unwrap();
        assert_eq!(row["title"], "Revised issue");
        assert_eq!(row["fields"]["status"], "done");
    }
}
#[test]
fn unsupported_and_out_of_scope_deliveries_roll_back() {
    let mut db = Store::memory().unwrap();
    let fixtures = cases();
    for case in &fixtures {
        connector(&mut db, case["provider"].as_str().unwrap());
    }
    let mut mismatch = envelope(&fixtures[0], "provider.ingest");
    mismatch["id"] = json!("jira");
    assert!(db.execute("owner", mismatch).is_err());
    let mut gitlab = envelope(&fixtures[0], "provider.ingest");
    gitlab.as_object_mut().unwrap().remove("event_id");
    assert!(db.execute("owner", gitlab.clone()).is_err());
    gitlab["event_id"] = json!("a");
    gitlab["payload"]["object_attributes"]["confidential"] = json!(true);
    assert!(db.execute("owner", gitlab).is_err());
    let mut slack = envelope(&fixtures[2], "provider.ingest");
    slack["payload"]["event"]["subtype"] = json!("bot_message");
    assert!(db.execute("owner", slack).is_err());
    let mut telegram = envelope(&fixtures[3], "provider.ingest");
    telegram["event_id"] = json!("forged");
    assert!(db.execute("owner", telegram).is_err());
    let mut feishu = envelope(&fixtures[4], "provider.ingest");
    feishu["payload"]["event"]["message"]["content"] = json!("broken");
    assert!(db.execute("owner", feishu).is_err());
    db.execute(
        "owner",
        json!({"op":"update","kind":"connector","id":"slack","fields":{"channel_id":"Cother"}}),
    )
    .unwrap();
    assert!(
        db.execute("owner", envelope(&fixtures[2], "provider.ingest"))
            .is_err()
    );
    db.execute(
        "owner",
        json!({"op":"update","kind":"connector","id":"jira","fields":{"visibility":"private"}}),
    )
    .unwrap();
    db.execute(
        "owner",
        json!({"op":"create","kind":"member","id":"r","title":"Reader","fields":{"role":"viewer"}}),
    )
    .unwrap();
    assert!(
        db.execute("r", envelope(&fixtures[1], "provider.preview"))
            .is_err()
    );
    assert!(
        db.execute("r", envelope(&fixtures[0], "provider.ingest"))
            .is_err()
    );
    assert_eq!(
        db.execute("owner", json!({"op":"list","kind":"task"}))
            .unwrap(),
        json!([])
    );
}
