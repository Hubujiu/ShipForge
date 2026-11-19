use serde_json::{Value, json};
use shipforge_core::{Store, runtime_diagnostics};

const KEY: &str = "fixture-machine-key-only-in-isolated-runtime-tests";
fn op(s: &mut Store, q: Value) -> Value {
    s.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q} · {e}"))
}
fn create(s: &mut Store, kind: &str, id: &str, f: Value) -> Value {
    op(
        s,
        json!({"op":"create","kind":kind,"id":id,"title":id,"fields":f}),
    )
}
fn machine(s: &mut Store, dirs: Value) {
    op(
        s,
        json!({"op":"machine.register","machine_id":"box","machine_key":KEY,"title":"Test","shared":true,"runtimes":["local-tool"],"capacity":4,"directories":dirs}),
    );
}
fn msg(s: &mut Store, chat: &str) -> Value {
    op(
        s,
        json!({"op":"chat.send","id":chat,"body":"Synthetic message"}),
    )
}
fn queue(s: &mut Store, task: Option<&str>) -> Value {
    op(
        s,
        json!({"op":"run.enqueue","handler_id":"one","instruction":"Synthetic work","task_id":task}),
    )
}
fn claim(s: &mut Store) -> Value {
    op(
        s,
        json!({"op":"run.claim","machine_id":"box","machine_key":KEY}),
    )
}
fn report(s: &mut Store, claim: &Value) -> Value {
    op(
        s,
        json!({"op":"run.report","id":claim["run"]["id"],"machine_id":"box","machine_key":KEY,"lease":claim["lease"],"status":"done","result":"finished","artifacts":[]}),
    )
}
fn view(s: &mut Store, id: &Value) -> Value {
    op(s, json!({"op":"run.diagnostics","id":id}))
}
fn agent_chat(s: &mut Store, name: &str) {
    create(
        s,
        "chat",
        name,
        json!({"agent":"one","visibility":"private"}),
    );
}
#[test]
fn task_and_chat_quotas_are_independent_unless_configured_shared() {
    for shared in [false, true] {
        let mut s = Store::memory().unwrap();
        machine(&mut s, Value::Null);
        create(
            &mut s,
            "agent",
            "one",
            json!({"max_parallel":1,"chat_parallel":2,"chat_capacity_mode":if shared{"shared"}else{"independent"}}),
        );
        let first = queue(&mut s, None);
        let t = claim(&mut s);
        assert_eq!(t["run"]["id"], first["id"]);
        for room in ["a", "b"] {
            agent_chat(&mut s, room);
            msg(&mut s, room);
        }
        let waiting = queue(&mut s, None);
        if shared {
            assert!(claim(&mut s).is_null());
            report(&mut s, &t);
            assert!(!claim(&mut s).is_null());
            assert!(claim(&mut s).is_null());
        } else {
            assert!(!claim(&mut s).is_null());
            assert!(!claim(&mut s).is_null());
            assert!(claim(&mut s).is_null());
            assert_eq!(
                view(&mut s, &waiting["id"])["waiting_reason"],
                "agent_capacity"
            );
        }
    }
}
#[test]
fn future_work_and_closed_shifts_preserve_queue_budget_until_an_authorized_trigger() {
    let mut s = Store::memory().unwrap();
    machine(&mut s, Value::Null);
    create(
        &mut s,
        "agent",
        "one",
        json!({"working_hours":{"timezone":"UTC","week":{}},"queue_timeout_hours":1}),
    );
    create(&mut s, "member", "viewer", json!({"role":"viewer"}));
    create(&mut s, "task", "issue", json!({"status":"todo"}));
    let job = queue(&mut s, Some("issue"));
    let id = &job["id"];
    assert_eq!(job["fields"]["waiting_reason"], "outside_working_hours");
    assert!(job["fields"]["queue_deadline"].is_null());
    assert!(claim(&mut s).is_null());
    assert!(
        s.execute("viewer", json!({"op":"run.trigger_now","id":id}))
            .is_err()
    );
    agent_chat(&mut s, "personal");
    msg(&mut s, "personal");
    let talk = claim(&mut s);
    assert_eq!(talk["run"]["fields"]["trigger_source"], "chat");
    report(&mut s, &talk);
    create(
        &mut s,
        "comment",
        "ping",
        json!({"task":"issue","body":"Please check","agent_mentions":["one"]}),
    );
    let runs = op(&mut s, json!({"op":"list","kind":"run"}));
    assert!(
        runs.as_array()
            .unwrap()
            .iter()
            .any(|r| r["fields"]["trigger_source"] == "comment"
                && r["fields"]["waiting_reason"] == "outside_working_hours")
    );
    let now = op(&mut s, json!({"op":"run.trigger_now","id":id}));
    assert_eq!(
        now["fields"]["queue_deadline"].as_i64().unwrap()
            - now["fields"]["eligible_at"].as_i64().unwrap(),
        3_600_000
    );
    let ready = claim(&mut s);
    assert_eq!(ready["run"]["id"], *id);
    assert_eq!(report(&mut s, &ready)["fields"]["status"], "done");
    op(
        &mut s,
        json!({"op":"update","kind":"agent","id":"one","fields":{"working_hours":{"preset":"off_peak","timezone":"UTC"}}}),
    );
    let future = op(
        &mut s,
        json!({"op":"run.enqueue","handler_id":"one","instruction":"Scheduled","scheduled_at":"2099-01-05T12:00:00Z"}),
    );
    assert_eq!(future["fields"]["waiting_reason"], "scheduled_start");
    assert!(
        future["fields"]["next_start_at"].as_i64().unwrap()
            >= future["fields"]["scheduled_at"].as_i64().unwrap()
    );
}
#[test]
fn server_timers_expire_queued_and_running_work_and_fence_previous_attempts() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("runtime.sqlite");
    let mut s = Store::open(&path).unwrap();
    machine(&mut s, Value::Null);
    create(
        &mut s,
        "agent",
        "one",
        json!({"run_timeout_minutes":15,"queue_timeout_hours":1}),
    );
    let queued = queue(&mut s, None);
    let id = &queued["id"];
    let db = rusqlite::Connection::open(&path).unwrap();
    let expire = |field: &str| {
        db.execute(
            "UPDATE records SET fields=json_set(fields,?1,1) WHERE kind='run' AND id=?2",
            rusqlite::params![field, id.as_str().unwrap()],
        )
        .unwrap();
    };
    expire("$.queue_deadline");
    let timeout = view(&mut s, id);
    assert_eq!(timeout["failure"]["category"], "queue_timeout");
    op(&mut s, json!({"op":"run.retry","id":id}));
    let first = claim(&mut s);
    let fields = &first["run"]["fields"];
    assert_eq!(
        fields["run_deadline"].as_i64().unwrap() - fields["started_at"].as_i64().unwrap(),
        900_000
    );
    expire("$.run_deadline");
    let pulse = op(
        &mut s,
        json!({"op":"run.heartbeat","machine_id":"box","machine_key":KEY,"lease":first["lease"],"id":id}),
    );
    assert_eq!(pulse["cancel"], true);
    let done = report(&mut s, &first);
    assert_eq!(done["fields"]["failure"]["category"], "run_timeout");
    op(&mut s, json!({"op":"run.retry","id":id}));
    let fresh = claim(&mut s);
    assert_ne!(first["lease"], fresh["lease"]);
    // Move the synthetic running attempt beyond its working window, keeping its lease and runtime deadline valid.
    db.execute("UPDATE records SET fields=json_set(fields,'$.config.working_hours',json(?)) WHERE kind='run' AND id=?",rusqlite::params![json!({"week":{},"timezone":"UTC"}).to_string(),id.as_str().unwrap()]).unwrap();
    assert_eq!(
        op(
            &mut s,
            json!({"op":"run.heartbeat","machine_id":"box","machine_key":KEY,"lease":fresh["lease"],"id":id})
        )["cancel"],
        false
    );
    assert!(s.execute("owner",json!({"op":"run.report","machine_id":"box","machine_key":KEY,"lease":first["lease"],"id":id,"status":"done","result":"late"})).is_err());
    assert_eq!(report(&mut s, &fresh)["fields"]["status"], "done");
}
#[test]
fn direct_profiles_choose_free_aliases_pin_each_task_and_respect_revocation() {
    let mut s = Store::memory().unwrap();
    let dir = |id: &str, c: &str| json!({"id":id,"identity":c.repeat(64),"available":true,"direct":true,"git":false});
    machine(
        &mut s,
        json!([dir("one", "a"), dir("alias", "a"), dir("two", "b")]),
    );
    create(
        &mut s,
        "agent",
        "one",
        json!({"machine_id":"box","directory_mode":"direct","profiles":["one","alias","two"],"max_parallel":4}),
    );
    for task in ["a", "b", "c"] {
        create(&mut s, "task", task, json!({"status":"todo"}));
        queue(&mut s, Some(task));
    }
    let first = claim(&mut s);
    let second = claim(&mut s);
    let first_task = first["run"]["fields"]["task_id"].as_str().unwrap();
    assert_eq!(
        first["run"]["fields"]["assigned_directory"]["profile"],
        "one"
    );
    assert_eq!(
        second["run"]["fields"]["assigned_directory"]["profile"],
        "two"
    );
    assert!(claim(&mut s).is_null());
    let history = op(&mut s, json!({"op":"list","kind":"run"}));
    assert!(
        history
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["fields"]["waiting_reason"] == "directory_busy")
    );
    assert_eq!(
        op(&mut s, json!({"op":"machine.directories","id":"box"}))["directories"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    report(&mut s, &first);
    let third = claim(&mut s);
    report(&mut s, &third);
    report(&mut s, &second);
    agent_chat(&mut s, "bound");
    msg(&mut s, "bound");
    let original = claim(&mut s);
    assert_eq!(
        original["run"]["fields"]["assigned_directory"]["profile"],
        "one"
    );
    report(&mut s, &original);
    op(
        &mut s,
        json!({"op":"update","kind":"agent","id":"one","fields":{"profiles":["two"]}}),
    );
    queue(&mut s, Some(first_task));
    let reused = claim(&mut s);
    assert_eq!(
        reused["run"]["fields"]["assigned_directory"]["profile"],
        "one"
    );
    report(&mut s, &reused);
    machine(&mut s, json!([dir("two", "b")]));
    let missing = msg(&mut s, "bound");
    assert!(claim(&mut s).is_null());
    assert_eq!(
        view(&mut s, &missing["run"])["failure"]["category"],
        "bound_directory_missing"
    );
    queue(&mut s, Some(first_task));
    assert_eq!(
        claim(&mut s)["run"]["fields"]["assigned_directory"]["profile"],
        "two"
    );
}
#[test]
fn diagnostic_codes_are_explainable_and_private_processes_are_hidden() {
    for (status, code, category) in [
        (429, "", "provider_limit"),
        (429, "insufficient_quota", "billing_quota"),
        (429, "rate_limit_exceeded", "rate_limit"),
        (403, "", "permission"),
        (401, "", "authentication"),
        (413, "", "payload_size"),
        (504, "", "request_timeout"),
        (529, "", "overloaded"),
        (400, "context_length_exceeded", "context_length"),
        (400, "image_too_large", "image_size"),
        (404, "model_not_found", "model_unavailable"),
        (400, "vision_not_supported", "vision_unsupported"),
    ] {
        let info = runtime_diagnostics::classify(
            &json!({"status":status,"code":code,"message":"fixture-private-text"}),
            "fixture-private-text",
        );
        assert_eq!(info["category"], category);
        assert!(!info.to_string().contains("fixture-private-text"));
        assert_eq!(info["http_status"], status);
    }
    let mut s = Store::memory().unwrap();
    machine(&mut s, Value::Null);
    create(&mut s, "member", "viewer", json!({"role":"viewer"}));
    create(&mut s, "agent", "one", json!({}));
    agent_chat(&mut s, "private");
    msg(&mut s, "private");
    let first = claim(&mut s);
    let id = &first["run"]["id"];
    assert!(id.is_string());
    assert!(
        s.execute("viewer", json!({"op":"run.diagnostics","id":id}))
            .is_err()
    );
    let m = s
        .execute("viewer", json!({"op":"machine.diagnostics","id":"box"}))
        .unwrap();
    assert_eq!(m["visible_runs"], json!([]));
    assert!(!m.to_string().contains(id.as_str().unwrap()));
    assert_eq!(
        op(&mut s, json!({"op":"machine.diagnostics","id":"box"}))["visible_runs"][0]["id"],
        *id
    );
}
