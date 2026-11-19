use serde_json::{Value, json};
use shipforge_core::Store;

fn call(db: &mut Store, actor: &str, space: &str, mut q: Value) -> Value {
    q["space"] = json!(space);
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{actor}, {q}: {e}"))
}
fn owner(db: &mut Store, space: &str, q: Value) -> Value {
    call(db, "owner", space, q)
}
fn add(db: &mut Store, space: &str, kind: &str, id: &str, fields: Value) -> Value {
    owner(
        db,
        space,
        json!({"op":"create","kind":kind,"id":id,"title":id,"fields":fields}),
    )
}
fn get(db: &mut Store, space: &str, id: &str) -> Value {
    owner(db, space, json!({"op":"get","kind":"task","id":id}))
}
fn deny(db: &mut Store, actor: &str, space: &str, mut q: Value) {
    q["space"] = json!(space);
    assert!(
        db.execute(actor, q.clone()).is_err(),
        "{actor} unexpectedly authorized for {q}"
    );
}

#[test]
fn private_guest_and_public_outsider_intake_recheck_each_actor_and_type() {
    let mut db = Store::memory().unwrap();
    for (id, role) in [
        ("visitor", "guest"),
        ("alice", "member"),
        ("stranger", "guest"),
    ] {
        add(&mut db, "default", "member", id, json!({"role":role}));
    }
    for (space, vis) in [("secret", "private"), ("commons", "public")] {
        add(
            &mut db,
            "default",
            "space",
            space,
            json!({"visibility":vis}),
        );
        owner(
            &mut db,
            space,
            json!({"op":"triage.configure","enabled":true,"types":["bug"]}),
        );
    }
    add(
        &mut db,
        "secret",
        "member",
        "visitor",
        json!({"role":"guest"}),
    );
    let submit = json!({"op":"triage.submit","title":"A submission","type":"bug","status":"done","assignee":"owner","request_id":"submit"});
    let private = call(&mut db, "visitor", "secret", submit.clone());
    assert_eq!(private, call(&mut db, "visitor", "secret", submit.clone()));
    let own = call(
        &mut db,
        "visitor",
        "secret",
        json!({"op":"get","kind":"task","id":private["id"]}),
    );
    assert_eq!(own["fields"]["status"], "triage");
    assert!(own["fields"]["assignee"].is_null());
    deny(
        &mut db,
        "visitor",
        "secret",
        json!({"op":"triage","id":own["id"],"decision":"accept"}),
    );
    deny(
        &mut db,
        "visitor",
        "secret",
        json!({"op":"triage.submit","title":"Another type","type":"requirement"}),
    );
    deny(&mut db, "alice", "secret", submit.clone());
    deny(&mut db, "stranger", "commons", submit.clone());
    let receipt = call(&mut db, "alice", "commons", submit.clone());
    assert!(receipt.get("fields").is_none());
    deny(
        &mut db,
        "alice",
        "commons",
        json!({"op":"get","kind":"task","id":receipt["id"]}),
    );
    let outside = get(&mut db, "commons", receipt["id"].as_str().unwrap());
    assert_eq!(outside["fields"]["creator"], "alice");
    owner(
        &mut db,
        "commons",
        json!({"op":"triage","id":outside["id"],"decision":"accept"}),
    );
    owner(
        &mut db,
        "secret",
        json!({"op":"triage.configure","enabled":false,"confirm":true}),
    );
    deny(
        &mut db,
        "visitor",
        "secret",
        json!({"op":"triage.submit","title":"After close","type":"bug"}),
    );
    assert_eq!(
        get(&mut db, "secret", own["id"].as_str().unwrap())["fields"]["status"],
        "backlog"
    );
}
#[test]
fn triage_rules_duty_decisions_batch_and_reopen_are_transactional() {
    let mut db = Store::memory().unwrap();
    let s = "default";
    add(&mut db, s, "member", "alice", json!({"role":"member"}));
    let rules = json!([{"id":"first","when":{"contains":"hot"},"set":{"priority":"high"}},{"id":"last","when":{"priority":"high"},"set":{"priority":"urgent"}},{"id":"disabled","enabled":false,"when":{},"set":{"priority":"low"}}]);
    let config = owner(
        &mut db,
        s,
        json!({"op":"triage.configure","enabled":true,"priority_required":true,"duty":{"mode":"assign","member":"alice"},"rules":rules}),
    );
    assert_eq!(config["conflicts"][0]["field"], "priority");
    deny(
        &mut db,
        "alice",
        s,
        json!({"op":"triage.configure","duty":{"mode":"none"}}),
    );
    let first = add(&mut db, s, "task", "hot-item", json!({"status":"triage"}));
    assert_eq!(first["fields"]["priority"], "urgent");
    assert_eq!(first["fields"]["assignee"], "alice");
    add(&mut db, s, "task", "later", json!({"status":"triage"}));
    let until = chrono::Utc::now().timestamp_millis() + 60_000;
    owner(
        &mut db,
        s,
        json!({"op":"triage","id":"later","decision":"defer","until":until}),
    );
    let queue = owner(&mut db, s, json!({"op":"triage.queue"}));
    assert_eq!(queue["deferred"], 1);
    assert_eq!(queue["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        owner(
            &mut db,
            s,
            json!({"op":"triage.queue","include_deferred":true})
        )["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    owner(
        &mut db,
        s,
        json!({"op":"triage","id":"later","decision":"restore"}),
    );
    deny(
        &mut db,
        "alice",
        s,
        json!({"op":"triage.batch","decision":"accept","tasks":["hot-item","later"]}),
    );
    assert_eq!(get(&mut db, s, "hot-item")["fields"]["status"], "triage");
    call(
        &mut db,
        "alice",
        s,
        json!({"op":"triage","id":"hot-item","decision":"accept","comment":"Verified","subscribe":true}),
    );
    assert_eq!(get(&mut db, s, "hot-item")["fields"]["status"], "todo");
    let link = owner(&mut db, s, json!({"op":"task.share","id":"hot-item"}));
    let dup = owner(
        &mut db,
        s,
        json!({"op":"triage","id":"later","decision":"duplicate","target":link["link"]}),
    );
    assert_eq!(dup["fields"]["status"], "duplicate");
    let opened = call(
        &mut db,
        "alice",
        s,
        json!({"op":"task.open","link":link["link"]}),
    );
    assert_eq!(opened["id"], "hot-item");
    add(
        &mut db,
        s,
        "task_type",
        "nobacklog",
        json!({"statuses":["triage","in_progress","cancelled"]}),
    );
    add(
        &mut db,
        s,
        "task",
        "must-stay",
        json!({"status":"triage","type":"nobacklog"}),
    );
    deny(
        &mut db,
        "owner",
        s,
        json!({"op":"triage.configure","enabled":false,"confirm":true}),
    );
    assert_eq!(
        owner(&mut db, s, json!({"op":"triage.settings"}))["enabled"],
        true
    );
    owner(
        &mut db,
        s,
        json!({"op":"triage","id":"must-stay","decision":"reject","reason":"Wrong team"}),
    );
    assert_eq!(
        get(&mut db, s, "must-stay")["fields"]["status"],
        "cancelled"
    );
    let comments = owner(
        &mut db,
        s,
        json!({"op":"list","kind":"comment","filter":{"task":"must-stay"}}),
    );
    assert_eq!(comments[0]["fields"]["body"], "Wrong team");
}
#[test]
fn project_timeline_shows_overlap_undated_dependencies_and_visibility() {
    let mut db = Store::memory().unwrap();
    let s = "default";
    add(&mut db, s, "member", "alice", json!({"role":"member"}));
    add(
        &mut db,
        s,
        "space",
        "hidden",
        json!({"visibility":"private"}),
    );
    add(
        &mut db,
        s,
        "project",
        "launch",
        json!({"spaces":["hidden"],"owner":"alice","primary_space":"default","priority":"high","status":"active"}),
    );
    for (id, fields) in [
        (
            "main",
            json!({"start_date":"2026-02-01","due_date":"2026-02-08"}),
        ),
        (
            "parallel",
            json!({"planned_start":"2026-02-04","planned_end":"2026-02-11"}),
        ),
        ("undated", json!({})),
    ] {
        let mut f = fields;
        f["project"] = json!("launch");
        add(&mut db, s, "task", id, f);
    }
    add(
        &mut db,
        s,
        "relation",
        "dep",
        json!({"from":"parallel","to":"main","type":"blocks"}),
    );
    add(
        &mut db,
        "hidden",
        "task",
        "secret",
        json!({"project":"launch","due_date":"2026-02-18"}),
    );
    let q = json!({"op":"project.timeline","id":"launch","as_of":"2026-02-09"});
    let own = owner(&mut db, s, q.clone());
    assert_eq!(own["total"], 4);
    let observed = call(
        &mut db,
        "alice",
        s,
        json!({"op":"get","kind":"project","id":"launch"}),
    );
    assert!(observed["fields"]["spaces"].as_array().unwrap().is_empty());
    let plan = call(&mut db, "alice", s, q.clone());
    assert_eq!(plan["total"], 3);
    assert_eq!(plan["start"], "2026-02-01");
    assert_eq!(plan["end"], "2026-02-11");
    assert_eq!(plan["scheduled"][0]["overdue"], true);
    assert_eq!(plan["scheduled"][0]["blocked_by"][0]["id"], "parallel");
    assert_eq!(plan["undated"][0]["task"]["id"], "undated");
    assert!(
        plan["project"]["fields"]["spaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let plan = call(
        &mut db,
        "alice",
        s,
        json!({"op":"project.timeline","id":"launch","basis":"plan"}),
    );
    assert_eq!(plan["scheduled"].as_array().unwrap().len(), 1);
    deny(
        &mut db,
        "alice",
        s,
        json!({"op":"update","kind":"task","id":"parallel","fields":{"planned_end":"2025-01-01"}}),
    );
}
#[test]
fn rule_routing_notifies_members_and_preserves_review_when_target_is_unavailable() {
    let mut db = Store::memory().unwrap();
    let s = "default";
    add(&mut db, s, "member", "reviewer", json!({"role":"member"}));
    add(
        &mut db,
        s,
        "space",
        "product",
        json!({"visibility":"private","key":"DEV"}),
    );
    owner(
        &mut db,
        s,
        json!({"op":"triage.configure","enabled":true,"duty":{"mode":"notify","member":"reviewer"}}),
    );
    add(
        &mut db,
        s,
        "task",
        "incoming",
        json!({"source":"integration"}),
    );
    let notice = call(
        &mut db,
        "reviewer",
        s,
        json!({"op":"list","kind":"notification"}),
    );
    assert!(
        notice
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["fields"]["event"] == "triage_duty" && n["fields"]["task"] == "incoming")
    );
    let route = json!({"id":"move","when":{"contains":"route"},"set":{"space":"product","status":"todo","priority":"high"}});
    owner(&mut db, s, json!({"op":"triage.configure","rules":[route]}));
    let routed = add(&mut db, s, "task", "route-here", json!({"status":"triage"}));
    assert_eq!(routed["space"], "product");
    assert_eq!(routed["fields"]["priority"], "high");
    deny(
        &mut db,
        "owner",
        s,
        json!({"op":"get","kind":"task","id":"route-here"}),
    );
    add(
        &mut db,
        "product",
        "task",
        "route-conflict",
        json!({"priority":"low"}),
    );
    let retained = add(
        &mut db,
        s,
        "task",
        "route-conflict",
        json!({"status":"triage"}),
    );
    assert_eq!(retained["space"], s);
    assert_eq!(retained["fields"]["status"], "triage");
    assert_eq!(retained["fields"]["triage_rule_failed"], true);
    assert_eq!(
        get(&mut db, "product", "route-conflict")["fields"]["priority"],
        "low"
    );
    let private = owner(
        &mut db,
        "product",
        json!({"op":"task.share","id":"route-here"}),
    );
    deny(
        &mut db,
        "reviewer",
        s,
        json!({"op":"task.open","link":private["link"]}),
    );
    let duplicate = owner(
        &mut db,
        s,
        json!({"op":"triage","decision":"duplicate","id":"incoming","target":private["link"]}),
    );
    assert_eq!(
        duplicate["fields"]["triage_duplicate_of"]["task"],
        "route-here"
    );
    let scoped = call(
        &mut db,
        "reviewer",
        s,
        json!({"op":"get","kind":"task","id":"incoming"}),
    );
    assert!(scoped["fields"].get("triage_duplicate_of").is_none());
    add(&mut db, s, "organization", "other", json!({}));
    add(&mut db, "other", "task", "different", json!({}));
    let foreign = owner(
        &mut db,
        "other",
        json!({"op":"task.share","id":"different"}),
    );
    add(&mut db, s, "task", "unresolved", json!({"status":"triage"}));
    deny(
        &mut db,
        "owner",
        s,
        json!({"op":"triage","decision":"duplicate","id":"unresolved","target":foreign["link"]}),
    );
    let config = owner(&mut db, s, json!({"op":"triage.settings"}));
    owner(
        &mut db,
        s,
        json!({"op":"triage.configure","expected_rev":config["rev"],"rules":[],"duty":{"mode":"none"}}),
    );
    let regular = add(
        &mut db,
        s,
        "task",
        "route-no-longer",
        json!({"status":"triage"}),
    );
    assert_eq!(regular["space"], s);
    assert_eq!(regular["fields"]["priority"], "none");
    deny(
        &mut db,
        "owner",
        s,
        json!({"op":"update","kind":"task","id":"route-no-longer","fields":{"triage_rule_failed":true}}),
    );
}
#[test]
fn customer_request_conversion_and_snooze_expiration_follow_the_live_queue() {
    let mut db = Store::memory().unwrap();
    let s = "default";
    owner(
        &mut db,
        s,
        json!({"op":"update","kind":"space","id":s,"fields":{"customer_requests_enabled":true}}),
    );
    owner(&mut db, s, json!({"op":"triage.configure","enabled":true}));
    add(
        &mut db,
        s,
        "customer_request",
        "need",
        json!({"description":"Please improve imports","contact":"private contact"}),
    );
    let q = json!({"op":"customer_request.convert","id":"need","type":"bug"});
    let task = owner(&mut db, s, q.clone());
    assert_eq!(task["fields"]["status"], "triage");
    assert!(task["fields"].get("contact").is_none());
    assert_eq!(owner(&mut db, s, q)["id"], task["id"]);
    let until = chrono::Utc::now().timestamp_millis() + 200;
    owner(
        &mut db,
        s,
        json!({"op":"triage","id":task["id"],"decision":"defer","until":until}),
    );
    std::thread::sleep(std::time::Duration::from_millis(230));
    let visible = owner(&mut db, s, json!({"op":"triage.queue"}));
    assert_eq!(visible["deferred"], 0);
    assert_eq!(visible["items"][0]["id"], task["id"]);
    let plain = add(
        &mut db,
        s,
        "task",
        "explicit",
        json!({"status":"in_progress","source":"integration"}),
    );
    assert_eq!(plain["fields"]["status"], "in_progress");
}
