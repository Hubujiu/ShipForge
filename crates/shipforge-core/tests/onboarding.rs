use rusqlite::Connection;
use serde_json::{Value, json};
use shipforge_core::Store;
use std::collections::BTreeSet;
use tempfile::tempdir;

fn run(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|error| panic!("{q}: {error}"))
}
fn progress(db: &mut Store) -> Value {
    run(db, "owner", json!({"op":"onboarding.get"}))
}
fn create(db: &mut Store, kind: &str, id: &str, fields: Value) -> Value {
    run(
        db,
        "owner",
        json!({"op":"create","kind":kind,"title":id,"id":id,"fields":fields}),
    )
}

#[test]
fn the_six_groups_have_unique_steps_and_only_successful_work_earns_points() {
    let mut db = Store::memory().unwrap();
    let catalog = run(&mut db, "owner", json!({"op":"onboarding.catalog"}));
    let groups = catalog.as_array().unwrap();
    assert_eq!(
        groups
            .iter()
            .map(|g| g["steps"].as_array().unwrap().len())
            .collect::<Vec<_>>(),
        [5, 5, 6, 8, 6, 6]
    );
    let steps = groups
        .iter()
        .flat_map(|g| g["steps"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        steps
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        36
    );
    assert_eq!(
        steps
            .iter()
            .map(|s| s["points"].as_u64().unwrap())
            .sum::<u64>(),
        560
    );
    let initial = progress(&mut db);
    assert_eq!(initial["completed"], json!([]));
    assert_eq!(initial["points"], 0);
    assert_eq!(initial["rank"], 1);
    assert_eq!(initial["sidebar_visible"], true);
    assert_eq!(initial["sidebar_expanded"], false);
    assert!(db.execute("owner",json!({"op":"create","kind":"task","id":"invalid","title":"invalid","fields":{"assignee":"missing","assignee_kind":"agent"}})).is_err());
    assert_eq!(progress(&mut db), initial);
    let machine = json!({"op":"machine.register","machine_id":"desk","machine_key":"onboarding-machine-key-for-local-tests-00001","title":"desk","runtimes":["local-tool"]});
    run(&mut db, "owner", machine.clone());
    run(&mut db, "owner", machine);
    create(
        &mut db,
        "agent",
        "helper",
        json!({"visibility":"private","machine_id":"desk"}),
    );
    create(&mut db, "task", "work", json!({}));
    assert_eq!(progress(&mut db)["points"], 85);
    run(
        &mut db,
        "owner",
        json!({"op":"follow","id":"work","enabled":false}),
    );
    assert_eq!(progress(&mut db)["points"], 85);
    let assign = json!({"op":"update","kind":"task","id":"work","fields":{"assignee":"helper","assignee_kind":"agent"},"request_id":"assign-once"});
    let saved = run(&mut db, "owner", assign.clone());
    assert_eq!(run(&mut db, "owner", assign), saved);
    let state = progress(&mut db);
    assert_eq!(state["points"], 115);
    assert_eq!(
        state["completed"],
        json!([
            "assign_agent",
            "connect_machine",
            "create_agent",
            "create_task"
        ])
    );
    assert_ne!(saved["fields"]["status"], "done");
}

#[test]
fn member_display_and_progress_survive_reopen_with_organization_isolation() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("team.sqlite");
    let mut db = Store::open(&path).unwrap();
    create(&mut db, "member", "teammate", json!({"role":"member"}));
    create(&mut db, "task", "first", json!({}));
    let query = json!({"op":"onboarding.get"});
    let other = run(&mut db, "teammate", query.clone());
    assert_eq!(other["points"], 0);
    assert_eq!(other["rank"], 2);
    run(
        &mut db,
        "teammate",
        json!({"op":"onboarding.display","sidebar_visible":false,"sidebar_expanded":true}),
    );
    assert_eq!(progress(&mut db)["sidebar_visible"], true);
    assert!(
        db.execute(
            "teammate",
            json!({"op":"onboarding.display","sidebar_visible":"false"})
        )
        .is_err()
    );
    create(&mut db, "space", "second", json!({"key":"SEC"}));
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"onboarding.get","space":"second"})
        )["points"],
        25
    );
    create(&mut db, "organization", "different", json!({}));
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"onboarding.get","space":"different"})
        )["points"],
        0
    );
    drop(db);
    let mut db = Store::open(&path).unwrap();
    let restored = run(&mut db, "teammate", query.clone());
    assert_eq!(restored["sidebar_visible"], false);
    assert_eq!(restored["sidebar_expanded"], true);
    assert_eq!(progress(&mut db)["points"], 25);
    run(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"member","id":"teammate"}),
    );
    assert!(db.execute("teammate", query).is_err());
}

#[test]
fn first_run_configures_a_new_workspace_once_and_recovers_history_when_upgrading() {
    let folder = tempdir().unwrap();
    let path = folder.path().join("upgraded.sqlite");
    let mut db = Store::open(&path).unwrap();
    let mut setup = json!({"op":"onboarding.setup","organization_name":"维护组","organization_slug":"maintainers","work_type":"other","task_types":["task","story"]});
    assert_eq!(progress(&mut db)["setup_required"], true);
    let mut bad = setup.clone();
    bad["task_types"] = json!(["task", "task"]);
    assert!(db.execute("owner", bad).is_err());
    assert_eq!(progress(&mut db)["setup_required"], true);
    setup["request_id"] = json!("initial-setup");
    let response = run(&mut db, "owner", setup.clone());
    assert_eq!(response["setup_required"], false);
    assert_eq!(run(&mut db, "owner", setup.clone()), response);
    setup.as_object_mut().unwrap().remove("request_id");
    assert!(db.execute("owner", setup).is_err());
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"get","kind":"organization","id":"default"})
        )["title"],
        "维护组"
    );
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"get","kind":"space","id":"default"})
        )["fields"]["agent_type_default"],
        "office"
    );
    let ty = run(
        &mut db,
        "owner",
        json!({"op":"get","kind":"task_type","id":"task"}),
    );
    assert_eq!(ty["fields"]["enabled"], true);
    assert_eq!(ty["fields"]["default"], true);
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"get","kind":"task_type","id":"bug"})
        )["fields"]["enabled"],
        false
    );
    assert_eq!(progress(&mut db)["points"], 0);
    create(&mut db, "task", "removed", json!({}));
    run(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"task","id":"removed"}),
    );
    drop(db);
    // Model the preceding release: keep its real event history, but remove guide-only tables.
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("DROP TABLE guide_points;DROP TABLE guide_credits;DROP TABLE guide_setup;DROP TABLE guide_display;DROP TABLE guide_migrations;").unwrap();
    drop(connection);
    let mut db = Store::open(&path).unwrap();
    let upgraded = progress(&mut db);
    assert_eq!(upgraded["setup_required"], false);
    assert_eq!(upgraded["completed"], json!(["create_task"]));
    assert_eq!(upgraded["points"], 25);
    drop(db);
    assert_eq!(progress(&mut Store::open(&path).unwrap()), upgraded);
}

#[test]
fn fractional_estimates_roll_up_visible_work_without_destroying_disabled_values() {
    let mut db = Store::memory().unwrap();
    create(&mut db, "iteration", "today", json!({}));
    let task = json!({"op":"create","kind":"task","id":"first","title":"first","fields":{"iteration":"today","estimate":0.75,"status":"done"}});
    assert!(db.execute("owner", task.clone()).is_err());
    let enable =
        json!({"op":"update","kind":"space","id":"default","fields":{"estimates_enabled":true}});
    run(&mut db, "owner", enable.clone());
    run(&mut db, "owner", task);
    create(
        &mut db,
        "task",
        "second",
        json!({"iteration":"today","estimate":1.5,"visibility":"private"}),
    );
    create(&mut db, "member", "reader", json!({"role":"viewer"}));
    let timeline = json!({"op":"iteration.timeline"});
    let rows = run(&mut db, "owner", timeline.clone());
    assert_eq!(rows[0]["fields"]["planning"]["estimate"], 2.25);
    assert_eq!(rows[0]["fields"]["planning"]["completed_estimate"], 0.75);
    assert_eq!(
        run(&mut db, "reader", timeline.clone())[0]["fields"]["planning"]["estimate"],
        0.75
    );
    run(
        &mut db,
        "owner",
        json!({"op":"update","kind":"space","id":"default","fields":{"estimates_enabled":false}}),
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"update","kind":"task","id":"first","fields":{"estimate":4}})
        )
        .is_err()
    );
    assert!(
        run(&mut db, "owner", timeline.clone())[0]["fields"]["planning"]
            .get("estimate")
            .is_none()
    );
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"op":"update","kind":"task","id":"first","title":"New title"})
        )["fields"]["estimate"],
        0.75
    );
    run(&mut db, "owner", enable);
    assert_eq!(
        run(&mut db, "owner", timeline)[0]["fields"]["planning"]["estimate"],
        2.25
    );
}

#[test]
fn new_spaces_get_independent_work_type_defaults() {
    let mut db = Store::memory().unwrap();
    for (id, work, task_type, agent_type) in [
        ("dev", "software", "requirement", "code"),
        ("plan", "project", "requirement", "office"),
        ("ops", "other", "task", "office"),
    ] {
        create(&mut db, "space", id, json!({"work_type":work}));
        let task = run(
            &mut db,
            "owner",
            json!({"op":"create","kind":"task","space":id,"title":"first task"}),
        );
        let agent = run(
            &mut db,
            "owner",
            json!({"op":"create","kind":"agent","space":id,"title":"first agent"}),
        );
        assert_eq!(task["fields"]["type"], task_type);
        assert_eq!(agent["fields"]["agent_type"], agent_type);
        assert_eq!(
            run(
                &mut db,
                "owner",
                json!({"op":"get","kind":"task_type","space":id,"id":"bug"})
            )["fields"]["enabled"],
            work != "other"
        );
    }
    assert!(db.execute("owner",json!({"op":"create","kind":"space","id":"invalid","title":"Invalid","fields":{"work_type":"software","task_types":[]}})).is_err());
    assert!(
        db.execute(
            "owner",
            json!({"op":"get","space":"invalid","kind":"space","id":"invalid"})
        )
        .is_err()
    );
}

#[test]
fn each_of_the_36_steps_is_reachable_with_public_business_operations() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        "owner",
        json!({"op":"machine.register","machine_id":"primary","machine_key":"isolated-machine-key-12345678901234567890","title":"Primary","runtimes":["local-tool"]}),
    );
    create(&mut db, "member", "colleague", json!({"role":"member"}));
    run(
        &mut db,
        "owner",
        json!({"op":"invitation.issue","channel":"link","role":"member"}),
    );
    let catalog = run(&mut db, "owner", json!({"op":"catalog.list"}));
    let template = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "skill")
        .unwrap();
    let skill = run(
        &mut db,
        "owner",
        json!({"op":"catalog.install","id":template["id"]}),
    );
    create(
        &mut db,
        "agent",
        "writer",
        json!({"instructions":"Write a summary","visibility":"space","skills":[skill["id"]]}),
    );
    create(
        &mut db,
        "crew",
        "team",
        json!({"leader_id":"writer","members":["writer"]}),
    );
    create(
        &mut db,
        "chat",
        "notes",
        json!({"agent":"writer","visibility":"private"}),
    );
    create(
        &mut db,
        "message",
        "hello",
        json!({"chat":"notes","body":"Hello"}),
    );
    create(&mut db, "project", "launch", json!({}));
    create(
        &mut db,
        "task",
        "main",
        json!({"assignee_kind":"agent","assignee":"writer","project":"launch","labels":["release"]}),
    );
    create(
        &mut db,
        "task",
        "sub",
        json!({"assignee_kind":"member","assignee":"colleague","parent":"main"}),
    );
    create(
        &mut db,
        "relation",
        "related",
        json!({"type":"related","from":"sub","to":"main"}),
    );
    run(&mut db, "owner", json!({"op":"follow","id":"main"}));
    create(
        &mut db,
        "comment",
        "reply",
        json!({"task":"main","body":"Review the draft"}),
    );
    run(
        &mut db,
        "colleague",
        json!({"op":"create","kind":"comment","title":"Reply","fields":{"task":"main","body":"See you","mentions":["owner"]}}),
    );
    let inbox = run(&mut db, "owner", json!({"op":"list","kind":"notification"}));
    let notice = inbox
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["fields"]["task"] == "main")
        .unwrap();
    for action in ["read", "clear"] {
        run(
            &mut db,
            "owner",
            json!({"op":"notification.act","id":notice["id"],"action":action}),
        );
    }
    create(
        &mut db,
        "view",
        "launch-view",
        json!({"visibility":"space","layout":"board"}),
    );
    create(&mut db, "field", "impact", json!({"value_type":"text"}));
    run(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task_type","id":"bug","fields":{"required_fields":["impact"]}}),
    );
    create(
        &mut db,
        "status",
        "review",
        json!({"category":"in_progress"}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"update","kind":"space","id":"default","fields":{"estimates_enabled":true}}),
    );
    create(
        &mut db,
        "connector",
        "repo",
        json!({"provider":"github","repository":"example/project","enabled":true,"link_tasks":true}),
    );
    let mut source = json!({"op":"import.preview","data":[{"kind":"task","id":"imported","title":"Imported","fields":{}}]});
    run(&mut db, "owner", source.clone());
    assert!(
        !progress(&mut db)["completed"]
            .as_array()
            .unwrap()
            .contains(&json!("import_tasks"))
    );
    source["op"] = json!("import");
    run(&mut db, "owner", source);
    let fixtures = [
        include_str!("fixtures/inbound.json"),
        include_str!("fixtures/inbound-extended.json"),
    ];
    for case in fixtures
        .iter()
        .flat_map(|s| serde_json::from_str::<Vec<Value>>(s).unwrap())
        .filter(|c| matches!(c["provider"].as_str(), Some("slack" | "feishu")))
    {
        let provider = case["provider"].as_str().unwrap();
        create(
            &mut db,
            "connector",
            provider,
            json!({"provider":provider,"enabled":true}),
        );
        run(
            &mut db,
            "owner",
            json!({"op":"provider.ingest","id":provider,"provider":provider,"payload":case["payload"]}),
        );
    }
    create(
        &mut db,
        "automation",
        "review-rule",
        json!({"handler_id":"writer","instruction":"Review","enabled":true,"triggers":[{"type":"schedule","interval_minutes":60},{"type":"webhook"},{"type":"comment"},{"type":"status","to":"done"}]}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"automation.fire","id":"review-rule","source":"webhook","event_id":"run-guide","payload":{}}),
    );
    let state = progress(&mut db);
    let expected = shipforge_core::onboarding::groups()
        .iter()
        .flat_map(|g| &g.steps)
        .map(|s| s.id.as_str())
        .collect::<BTreeSet<_>>();
    let actual = state["completed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(state["points"], 560);
}
