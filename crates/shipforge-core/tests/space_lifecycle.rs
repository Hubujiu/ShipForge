use serde_json::{Value, json};
use shipforge_core::Store;
use tempfile::tempdir;

fn run(s: &mut Store, actor: &str, q: Value) -> Value {
    s.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn space(s: &mut Store, id: &str, vis: &str, work: &str) {
    run(
        s,
        "owner",
        json!({"op":"create","kind":"space","id":id,"title":id,"fields":{"visibility":vis,"work_type":work}}),
    );
}
fn member(s: &mut Store, actor: &str, role: &str) {
    run(
        s,
        "owner",
        json!({"op":"create","kind":"member","id":actor,"title":actor,"fields":{"role":role}}),
    );
}
fn listed(s: &mut Store, actor: &str, id: &str) -> Value {
    run(s, actor, json!({"op":"space.list"}))
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}
fn nav(s: &mut Store, actor: &str, space: &str, module: &str) -> Value {
    run(s, actor, json!({"op":"space.navigation","space":space}))["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == module)
        .unwrap()
        .clone()
}
fn task(s: &mut Store, space: &str, id: &str, status: &str) {
    run(
        s,
        "owner",
        json!({"op":"create","kind":"task","id":id,"space":space,"title":id,"fields":{"status":status}}),
    );
}

#[test]
fn discovery_join_leave_and_private_metadata_follow_membership() {
    let mut s = Store::memory().unwrap();
    for (id, role) in [
        ("ana", "member"),
        ("sam", "viewer"),
        ("li", "admin"),
        ("guest", "guest"),
    ] {
        member(&mut s, id, role);
    }
    space(&mut s, "roadmap", "public", "project");
    space(&mut s, "private", "private", "software");
    task(&mut s, "private", "secret", "todo");
    let public = listed(&mut s, "ana", "roadmap");
    assert_eq!(public["fields"]["can_join"], true);
    assert_eq!(public["fields"]["joined"], false);
    assert!(public["fields"]["navigation"].is_null());
    assert!(listed(&mut s, "ana", "private").is_null());
    assert!(listed(&mut s, "guest", "roadmap").is_null());
    assert!(
        s.execute("ana", json!({"op":"list","kind":"task","space":"roadmap"}))
            .is_err()
    );
    assert!(
        s.execute("ana", json!({"op":"space.join","space":"private"}))
            .is_err()
    );
    assert!(
        s.execute("guest", json!({"op":"space.join","space":"roadmap"}))
            .is_err()
    );
    assert_eq!(
        listed(&mut s, "li", "private")["fields"]["can_manage"],
        true
    );
    assert!(
        s.execute(
            "li",
            json!({"op":"get","kind":"task","id":"secret","space":"private"})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "li",
            json!({"op":"space.deactivate.preview","space":"private"})
        )
        .is_err()
    );
    assert!(s.execute("li",json!({"op":"update","kind":"space","id":"private","space":"private","fields":{"triage_enabled":true}})).is_err());
    run(
        &mut s,
        "li",
        json!({"op":"update","kind":"space","id":"private","space":"private","title":"Renamed private space"}),
    );
    for actor in ["ana", "sam"] {
        let q = json!({"op":"space.join","space":"roadmap","request_id":"join-roadmap"});
        assert_eq!(run(&mut s, actor, q.clone()), run(&mut s, actor, q));
    }
    assert_eq!(
        run(&mut s, "sam", json!({"op":"whoami","space":"roadmap"}))["role"],
        "viewer"
    );
    assert!(
        s.execute(
            "sam",
            json!({"op":"create","kind":"task","space":"roadmap","title":"write"})
        )
        .is_err()
    );
    let public = listed(&mut s, "ana", "roadmap");
    assert_eq!(public["fields"]["can_leave"], true);
    assert_eq!(public["fields"]["navigation"].as_array().unwrap().len(), 5);
    run(
        &mut s,
        "owner",
        json!({"op":"create","kind":"task","space":"roadmap","id":"assigned","title":"Assigned task","fields":{"assignees":["ana"]}}),
    );
    run(
        &mut s,
        "ana",
        json!({"op":"space.leave","space":"roadmap","actor":"sam"}),
    );
    assert!(
        s.execute("ana", json!({"op":"list","kind":"task","space":"roadmap"}))
            .is_err()
    );
    assert!(
        run(
            &mut s,
            "sam",
            json!({"op":"get","kind":"task","id":"assigned","space":"roadmap"})
        )["fields"]["assignees"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.execute("owner", json!({"op":"space.leave","space":"roadmap"}))
            .is_err()
    );
}

#[test]
fn archived_space_is_read_only_and_restores_history_without_reviving_cancelled_work() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.db");
    let mut s = Store::open(&path).unwrap();
    space(&mut s, "history", "public", "software");
    task(&mut s, "history", "open", "todo");
    task(&mut s, "history", "done", "done");
    run(
        &mut s,
        "owner",
        json!({"op":"create","kind":"comment","space":"history","id":"note","title":"Prior comment","fields":{"task":"open","body":"Retained history"}}),
    );
    let q = json!({"op":"space.deactivate","space":"history"});
    assert!(
        s.execute("owner", q.clone())
            .unwrap_err()
            .contains("unfinished")
    );
    let pre = run(
        &mut s,
        "owner",
        json!({"op":"space.deactivate.preview","space":"history"}),
    );
    assert_eq!(pre["unfinished"], 1);
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"task","space":"history","id":"open","title":"Edited after preview"}),
    );
    assert!(s.execute("owner",json!({"op":"space.deactivate","space":"history","unfinished":"cancel","preview_token":pre["preview_token"]})).unwrap_err().contains("preview"));
    let pre = run(
        &mut s,
        "owner",
        json!({"op":"space.deactivate.preview","space":"history"}),
    );
    assert!(s.execute("owner",json!({"op":"space.deactivate","space":"history","unfinished":"cancel","expected_rev":0})).is_err());
    assert!(s.execute("owner",json!({"op":"update","kind":"space","space":"history","id":"history","fields":{"deactivated":true}})).is_err());
    let archived = run(
        &mut s,
        "owner",
        json!({"op":"space.deactivate","space":"history","unfinished":"cancel","expected_rev":pre["rev"],"preview_token":pre["preview_token"]}),
    );
    assert_eq!(archived["fields"]["deactivated"], true);
    drop(s);
    let mut s = Store::open(&path).unwrap();
    for (id, status) in [("open", "cancelled"), ("done", "done")] {
        assert_eq!(
            run(
                &mut s,
                "owner",
                json!({"op":"get","kind":"task","space":"history","id":id})
            )["fields"]["status"],
            status
        );
    }
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"get","kind":"comment","space":"history","id":"note"})
        )["title"],
        "Prior comment"
    );
    for q in [
        json!({"op":"create","kind":"task","space":"history","title":"Late task"}),
        json!({"op":"update","kind":"comment","id":"note","space":"history","title":"Altered history"}),
        json!({"op":"follow","space":"history","id":"open","following":true}),
        json!({"op":"iteration.configure","space":"history","mode":"automatic"}),
        json!({"op":"machine.heartbeat","space":"history","machine_id":"stale","machine_key":"stale"}),
    ] {
        assert!(s.execute("owner", q).unwrap_err().contains("deactivated"));
    }
    task(&mut s, "default", "elsewhere", "todo");
    assert!(
        s.execute(
            "owner",
            json!({"op":"task.move","space":"default","target":"history","id":"elsewhere"})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "owner",
            json!({"op":"invitation.issue","channel":"link","role":"member","spaces":["history"]})
        )
        .is_err()
    );
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"space.restore","space":"history","expected_rev":archived["rev"]})
        )["fields"]["deactivated"],
        false
    );
    task(&mut s, "history", "later", "todo");
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"get","kind":"task","space":"history","id":"open"})
        )["fields"]["status"],
        "cancelled"
    );
}

#[test]
fn bulk_move_is_atomic_and_keeps_comments_and_completed_history() {
    let mut s = Store::memory().unwrap();
    space(&mut s, "source", "private", "other");
    space(&mut s, "dest", "private", "other");
    for (id, status) in [("a", "todo"), ("collision", "todo"), ("done", "done")] {
        task(&mut s, "source", id, status);
    }
    task(&mut s, "dest", "collision", "todo");
    run(
        &mut s,
        "owner",
        json!({"op":"create","kind":"comment","space":"source","id":"comment","title":"Portable","fields":{"task":"a","body":"Portable"}}),
    );
    let q = json!({"op":"space.deactivate","space":"source","unfinished":"move","target":"dest"});
    assert!(s.execute("owner", q.clone()).is_err());
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"space.deactivate.preview","space":"source"})
        )["unfinished"],
        2
    );
    assert!(
        s.execute(
            "owner",
            json!({"op":"get","kind":"task","id":"a","space":"dest"})
        )
        .is_err()
    );
    run(
        &mut s,
        "owner",
        json!({"op":"delete","kind":"task","id":"collision","space":"dest"}),
    );
    run(&mut s, "owner", q);
    for id in ["a", "collision"] {
        assert_eq!(
            run(
                &mut s,
                "owner",
                json!({"op":"get","kind":"task","id":id,"space":"dest"})
            )["id"],
            id
        );
    }
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"get","kind":"comment","id":"comment","space":"dest"})
        )["fields"]["body"],
        "Portable"
    );
    assert_eq!(
        run(
            &mut s,
            "owner",
            json!({"op":"get","kind":"task","id":"done","space":"source"})
        )["fields"]["status"],
        "done"
    );
    let empty = run(
        &mut s,
        "owner",
        json!({"op":"space.restore","space":"source"}),
    );
    run(
        &mut s,
        "owner",
        json!({"op":"space.deactivate","space":"source","expected_rev":empty["rev"]}),
    );
}

#[test]
fn module_states_are_per_space_and_pins_survive_restart_per_member() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("modules.db");
    let mut s = Store::open(&path).unwrap();
    member(&mut s, "mira", "member");
    space(&mut s, "software", "public", "software");
    space(&mut s, "design", "public", "other");
    run(
        &mut s,
        "mira",
        json!({"op":"space.join","space":"software"}),
    );
    assert_eq!(
        nav(&mut s, "owner", "software", "iterations")["pinned"],
        true
    );
    assert_eq!(
        nav(&mut s, "owner", "software", "projects")["pinned"],
        false
    );
    assert_eq!(nav(&mut s, "owner", "design", "projects")["pinned"], true);
    assert_eq!(nav(&mut s, "owner", "design", "views")["pinned"], true);
    assert_eq!(nav(&mut s, "owner", "software", "triage")["enabled"], false);
    assert!(
        s.execute(
            "mira",
            json!({"op":"space.pin","space":"software","module":"triage","pinned":true})
        )
        .is_err()
    );
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"space","space":"software","id":"software","fields":{"customer_requests_enabled":true}}),
    );
    run(
        &mut s,
        "mira",
        json!({"op":"space.pin","space":"software","module":"customer_requests","pinned":true}),
    );
    run(
        &mut s,
        "mira",
        json!({"op":"create","kind":"customer_request","space":"software","id":"r","title":"Feedback","fields":{"contact":"Example customer","status":"in_progress"}}),
    );
    assert_eq!(
        run(
            &mut s,
            "mira",
            json!({"op":"search","space":"software","query":"Feedback"})
        )[0]["kind"],
        "customer_request"
    );
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"space","space":"software","id":"software","fields":{"customer_requests_enabled":false}}),
    );
    assert!(
        s.execute(
            "mira",
            json!({"op":"list","kind":"customer_request","space":"software"})
        )
        .is_err()
    );
    assert!(
        run(
            &mut s,
            "mira",
            json!({"op":"search","space":"software","query":"Feedback"})
        )
        .as_array()
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        nav(&mut s, "mira", "software", "customer_requests")["pinned"],
        false
    );
    drop(s);
    let mut s = Store::open(&path).unwrap();
    run(
        &mut s,
        "owner",
        json!({"op":"update","kind":"space","space":"software","id":"software","fields":{"customer_requests_enabled":true}}),
    );
    assert_eq!(
        nav(&mut s, "mira", "software", "customer_requests")["pinned"],
        true
    );
    assert_eq!(
        nav(&mut s, "owner", "software", "customer_requests")["pinned"],
        false
    );
    assert_eq!(
        run(
            &mut s,
            "mira",
            json!({"op":"get","kind":"customer_request","space":"software","id":"r"})
        )["fields"]["status"],
        "in_progress"
    );
}

#[test]
fn deactivation_fences_claimed_work_and_cancels_queued_work() {
    let mut s = Store::memory().unwrap();
    space(&mut s, "execution", "public", "software");
    let key = "long-synthetic-local-machine-key-for-tests";
    run(
        &mut s,
        "owner",
        json!({"op":"machine.register","space":"execution","machine_id":"test","machine_key":key,"title":"Test machine","runtimes":["local-tool"],"capacity":1}),
    );
    run(
        &mut s,
        "owner",
        json!({"op":"create","kind":"agent","space":"execution","id":"a","title":"Reader","fields":{"runtime":"local-tool","instructions":"Test"}}),
    );
    for id in ["first", "next"] {
        run(
            &mut s,
            "owner",
            json!({"op":"create","kind":"task","space":"execution","id":id,"title":id,"fields":{"status":"todo","assignee_kind":"agent","assignee":"a"}}),
        );
    }
    let lease = run(
        &mut s,
        "owner",
        json!({"op":"run.claim","space":"execution","machine_id":"test","machine_key":key}),
    );
    assert!(lease["lease"].is_string());
    run(
        &mut s,
        "owner",
        json!({"op":"space.deactivate","space":"execution","unfinished":"cancel"}),
    );
    let runs = run(
        &mut s,
        "owner",
        json!({"op":"list","kind":"run","space":"execution"}),
    );
    assert_eq!(runs.as_array().unwrap().len(), 2);
    assert!(
        runs.as_array()
            .unwrap()
            .iter()
            .all(|r| r["fields"]["status"] == "cancelled")
    );
    let late = json!({"op":"run.report","space":"execution","machine_id":"test","machine_key":key,"id":lease["run"]["id"],"lease":lease["lease"],"status":"done","result":"late"});
    assert!(s.execute("owner", late.clone()).is_err());
    run(
        &mut s,
        "owner",
        json!({"op":"space.restore","space":"execution"}),
    );
    assert!(s.execute("owner", late).is_err());
    assert!(
        run(
            &mut s,
            "owner",
            json!({"op":"run.claim","space":"execution","machine_id":"test","machine_key":key})
        )
        .is_null()
    );
}
