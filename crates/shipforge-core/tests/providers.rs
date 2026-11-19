use serde_json::{Value, json};
use shipforge_core::Store;

fn cases() -> Vec<Value> {
    let mut fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/inbound.json")).unwrap();
    fixtures.extend(
        serde_json::from_str::<Vec<Value>>(include_str!("fixtures/inbound-extended.json")).unwrap(),
    );
    fixtures
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
fn jira_priority_handles_lowest_unmapped_missing_and_local_conflict() {
    let mut db = Store::memory().unwrap();
    connector(&mut db, "jira");
    let case = cases()
        .into_iter()
        .find(|v| v["provider"] == "jira")
        .unwrap();
    let mut q = envelope(&case, "provider.ingest");
    q["payload"]["issue"]["fields"]["priority"] = json!({"id":"10017","name":"Lowest"});
    let mut preview = q.clone();
    preview["op"] = json!("provider.preview");
    let settings = json!({"op":"organization.settings"});
    assert_eq!(db.execute("owner", preview).unwrap()["priority"], "lowest");
    assert_eq!(
        db.execute("owner", settings.clone()).unwrap()["priority_levels"],
        4
    );
    let created = db.execute("owner", q.clone()).unwrap();
    let get = json!({"op":"get","kind":"task","id":created["task"]});
    assert_eq!(
        db.execute("owner", get.clone()).unwrap()["fields"]["priority"],
        "lowest"
    );
    assert_eq!(db.execute("owner", settings).unwrap()["priority_levels"], 5);
    for (i, priority) in [None, Some(json!({"id":"5","name":"Custom"}))]
        .into_iter()
        .enumerate()
    {
        q["event_id"] = json!(format!("other-{i}"));
        let fields = &mut q["payload"]["issue"]["fields"];
        fields["updated"] = json!(format!("2026-01-10T14:0{i}:00.000+0000"));
        fields.as_object_mut().unwrap().remove("priority");
        if let Some(p) = priority {
            fields["priority"] = p;
        }
        assert_eq!(
            db.execute("owner", q.clone()).unwrap()["action"],
            "unchanged"
        );
        assert_eq!(
            db.execute("owner", get.clone()).unwrap()["fields"]["priority"],
            "lowest"
        );
    }
    db.execute(
        "owner",
        json!({"op":"update","kind":"task","id":created["task"],"fields":{"priority":"high"}}),
    )
    .unwrap();
    q["event_id"] = json!("clear");
    q["payload"]["issue"]["fields"]["priority"] = Value::Null;
    q["payload"]["issue"]["fields"]["updated"] = json!("2026-01-10T15:00:00.000+0000");
    assert_eq!(
        db.execute("owner", q.clone()).unwrap()["fields"],
        json!(["priority"])
    );
    q["resolve"] = json!("source");
    assert_eq!(db.execute("owner", q).unwrap()["action"], "updated");
    assert_eq!(
        db.execute("owner", get).unwrap()["fields"]["priority"],
        "none"
    );
}
#[test]
fn every_provider_fixture_previews_applies_and_replays() {
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
    for case in fixtures.iter().filter(|c| {
        matches!(
            c["provider"].as_str(),
            Some("gitlab" | "jira" | "linear" | "asana")
        )
    }) {
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
        } else if provider == "jira" {
            q["payload"]["webhookEvent"] = json!("jira:issue_updated");
            let f = &mut q["payload"]["issue"]["fields"];
            f["updated"] = json!("2026-01-10T13:00:00.003+0000");
            f["status"]["statusCategory"]["key"] = json!("done");
            f["summary"] = json!("Revised issue");
        } else if provider == "linear" {
            q["payload"]["action"] = json!("update");
            let d = &mut q["payload"]["data"];
            d["updatedAt"] = json!("2026-01-10T13:00:00.003Z");
            d["state"]["type"] = json!("completed");
            d["title"] = json!("Revised issue");
        } else {
            let d = &mut q["payload"]["task"]["data"];
            d["modified_at"] = json!("2026-01-10T13:00:00.003Z");
            d["completed"] = json!(true);
            d["name"] = json!("Revised issue");
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
fn six_extended_adapters_validate_native_shapes_and_origin_filters() {
    let mut db = Store::memory().unwrap();
    for case in cases().into_iter().skip(5) {
        let name = case["provider"].as_str().unwrap();
        connector(&mut db, name);
        let original = envelope(&case, "provider.ingest");
        let mut q = original.clone();
        let scope = match name {
            "linear" => {
                q["payload"]["action"] = json!("remove");
                json!({"team_id":"foreign-team"})
            }
            "asana" => {
                q["payload"]["events"][0]["resource"]["gid"] = json!("wrong");
                json!({"project_id":"foreign-project"})
            }
            "teams" => {
                q["payload"]["from"]["role"] = json!("bot");
                json!({"tenant_id":"foreign-tenant"})
            }
            "discord" => {
                q["payload"]["d"]["webhook_id"] = json!("11");
                json!({"channel_id":"foreign-channel"})
            }
            "wecom" => {
                q["payload"]["Encrypt"] = json!("cipher-text");
                json!({"corp_id":"another-corp"})
            }
            "dingtalk" => {
                q["payload"]["headers"]["topic"] = json!("/other");
                json!({"chat_id":"foreign-chat"})
            }
            other => panic!("missing test for {other}"),
        };
        assert!(
            db.execute("owner", q).is_err(),
            "{name} accepted unsupported event"
        );
        db.execute(
            "owner",
            json!({"op":"update","kind":"connector","id":name,"fields":scope}),
        )
        .unwrap();
        assert!(
            db.execute("owner", original).is_err(),
            "{name} ignored source scope"
        );
    }
    assert_eq!(
        db.execute("owner", json!({"op":"list","kind":"task"}))
            .unwrap(),
        json!([])
    );
}

#[test]
fn asana_get_snapshot_and_dingtalk_sdk_inner_callback_are_accepted() {
    let mut db = Store::memory().unwrap();
    let fixtures = cases();
    let asana = fixtures.iter().find(|c| c["provider"] == "asana").unwrap();
    connector(&mut db, "asana");
    let mut q = envelope(asana, "provider.ingest");
    q["payload"] = asana["payload"]["task"].clone();
    let a = db.execute("owner", q.clone()).unwrap();
    assert_eq!(a["action"], "created");
    let mut thin = q.clone();
    thin["payload"] = json!({"events":asana["payload"]["events"]});
    assert!(db.execute("owner", thin).is_err());
    q["payload"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("notes");
    assert!(db.execute("owner", q).is_err());
    let ding = fixtures
        .iter()
        .find(|c| c["provider"] == "dingtalk")
        .unwrap();
    connector(&mut db, "dingtalk");
    let stream = envelope(ding, "provider.ingest");
    let mut inner = stream.clone();
    inner["payload"] = serde_json::from_str(stream["payload"]["data"].as_str().unwrap()).unwrap();
    let posted = db.execute("owner", inner).unwrap();
    assert_eq!(db.execute("owner", stream).unwrap(), posted);
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
