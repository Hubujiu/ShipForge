use serde_json::{Value, json};
use shipforge_core::Store;
fn op(s: &mut Store, actor: &str, q: Value) -> Value {
    s.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn create(s: &mut Store, actor: &str, space: &str, kind: &str, id: &str, f: Value) -> Value {
    op(
        s,
        actor,
        json!({"op":"create","space":space,"kind":kind,"id":id,"title":id,"fields":f}),
    )
}
fn get(s: &mut Store, actor: &str, space: &str, kind: &str, id: &str) -> Value {
    op(
        s,
        actor,
        json!({"op":"get","space":space,"kind":kind,"id":id}),
    )
}
fn dir(s: &mut Store, actor: &str, view: &str) -> Vec<String> {
    op(s, actor, json!({"op":"agent.directory","view":view}))
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}
fn setup() -> Store {
    let mut s = Store::memory().unwrap();
    for a in ["alice", "bob"] {
        create(
            &mut s,
            "owner",
            "default",
            "member",
            a,
            json!({"role":"member"}),
        );
    }
    s
}
const KEY: &str = "synthetic-test-token-for-directory-work";
fn machine(s: &mut Store, actor: &str, space: &str) {
    op(
        s,
        actor,
        json!({"op":"machine.register","space":space,"machine_id":"box","machine_key":KEY,"title":"Test","runtimes":["local-tool"],"capacity":4}),
    );
}
fn claim(s: &mut Store, actor: &str, space: &str) -> Value {
    op(
        s,
        actor,
        json!({"op":"run.claim","space":space,"machine_id":"box","machine_key":KEY}),
    )
}
fn run(s: &mut Store, actor: &str, space: &str, kind: &str, id: &str) -> Value {
    op(
        s,
        actor,
        json!({"op":"run.enqueue","space":space,"handler_kind":kind,"handler_id":id,"instruction":"Synthetic work"}),
    )
}

#[test]
fn personal_directory_grants_archive_cancel_and_recovery() {
    let mut s = setup();
    create(
        &mut s,
        "alice",
        "default",
        "agent",
        "one",
        json!({"visibility":"private","instructions":"Test"}),
    );
    assert!(dir(&mut s, "bob", "all").is_empty());
    op(
        &mut s,
        "alice",
        json!({"op":"agent.access","id":"one","actor":"bob","permission":"use"}),
    );
    assert_eq!(dir(&mut s, "bob", "private"), ["one"]);
    assert!(
        s.execute(
            "bob",
            json!({"op":"update","kind":"agent","id":"one","title":"Unpermitted edit"})
        )
        .is_err()
    );
    op(
        &mut s,
        "bob",
        json!({"op":"agent.organize","id":"one","action":"favorite"}),
    );
    assert_eq!(dir(&mut s, "bob", "favorite"), ["one"]);
    assert!(dir(&mut s, "alice", "favorite").is_empty());
    op(
        &mut s,
        "bob",
        json!({"op":"agent.organize","id":"one","action":"unfavorite"}),
    );
    assert_eq!(
        op(&mut s, "bob", json!({"op":"agent.directory"}))[0]["fields"]["personal"]["pinned"],
        true
    );
    machine(&mut s, "alice", "default");
    create(
        &mut s,
        "alice",
        "default",
        "chat",
        "history",
        json!({"agent":"one"}),
    );
    let sent = op(
        &mut s,
        "alice",
        json!({"op":"chat.send","id":"history","body":"Test question"}),
    );
    let active = claim(&mut s, "alice", "default");
    assert_eq!(active["run"]["id"], sent["run"]);
    let pending = run(&mut s, "alice", "default", "agent", "one");
    assert!(
        s.execute("bob", json!({"op":"agent.history","id":"one"}))
            .is_err()
    );
    assert!(
        !op(&mut s, "alice", json!({"op":"agent.history","id":"one"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    op(&mut s, "alice", json!({"op":"agent.archive","id":"one"}));
    assert_eq!(
        get(
            &mut s,
            "alice",
            "default",
            "run",
            pending["id"].as_str().unwrap()
        )["fields"]["status"],
        "cancelled"
    );
    assert_eq!(
        get(
            &mut s,
            "alice",
            "default",
            "run",
            sent["run"].as_str().unwrap()
        )["fields"]["status"],
        "cancelling"
    );
    assert_eq!(dir(&mut s, "alice", "archived"), ["one"]);
    assert!(dir(&mut s, "alice", "all").is_empty());
    assert!(
        s.execute("alice", json!({"op":"run.retry","id":pending["id"]}))
            .is_err()
    );
    assert!(
        s.execute(
            "alice",
            json!({"op":"chat.send","id":"history","body":"new"})
        )
        .is_err()
    );
    assert_ne!(
        op(&mut s, "alice", json!({"op":"chat.history","id":"history"}))[0]["fields"]["body"],
        Value::Null
    );
    op(&mut s, "alice", json!({"op":"agent.restore","id":"one"}));
    assert_eq!(dir(&mut s, "alice", "all"), ["one"]);
    create(&mut s, "alice", "default", "task", "task", json!({}));
    create(
        &mut s,
        "alice",
        "default",
        "comment",
        "mentioned",
        json!({"task":"task","body":"Review this","agent_mentions":["one"]}),
    );
    assert!(
        op(&mut s, "alice", json!({"op":"list","kind":"run"}))
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["fields"]["instruction"] == "Review this")
    );
    op(&mut s, "alice", json!({"op":"agent.archive","id":"one"}));
    assert!(s.execute("alice",json!({"op":"create","kind":"comment","title":"new","fields":{"task":"task","agent_mentions":["one"]}})).is_err());
    assert!(s.execute("alice",json!({"op":"update","kind":"task","id":"task","fields":{"assignee_kind":"agent","assignee":"one"}})).is_err());
    assert_eq!(
        get(&mut s, "alice", "default", "comment", "mentioned")["fields"]["body"],
        "Review this"
    );
}
#[test]
fn cross_space_live_configuration_policies_resources_and_immutable_run_snapshots() {
    let mut s = setup();
    create(
        &mut s,
        "owner",
        "default",
        "space",
        "target",
        json!({"visibility":"public"}),
    );
    for a in ["alice", "bob"] {
        op(&mut s, a, json!({"op":"space.join","space":"target"}));
    }
    create(
        &mut s,
        "alice",
        "default",
        "skill",
        "review",
        json!({"instructions":"original skill"}),
    );
    create(
        &mut s,
        "alice",
        "target",
        "skill",
        "review",
        json!({"instructions":"unrelated same ID"}),
    );
    create(
        &mut s,
        "alice",
        "default",
        "agent",
        "base",
        json!({"instructions":"parent prompt","runtime":"local-tool","model":"small","environment":{"COLOR":"a"},"arguments":["--one"],"skills":["review"],"visibility":"private","variant_permission":"manage","allow_variant_environment":false}),
    );
    op(
        &mut s,
        "alice",
        json!({"op":"agent.access","id":"base","actor":"bob","permission":"use"}),
    );
    let make = json!({"op":"agent.variant","id":"base","target_space":"target","variant_id":"branch","visibility":"space","request_id":"make-branch"});
    assert!(s.execute("bob", make.clone()).is_err());
    op(
        &mut s,
        "alice",
        json!({"op":"agent.access","id":"base","actor":"bob","permission":"manage"}),
    );
    let b = op(&mut s, "bob", make.clone());
    assert_eq!(b, op(&mut s, "bob", make));
    assert_eq!(b["space"], "target");
    assert!(
        s.execute(
            "bob",
            json!({"op":"agent.variant","id":"branch","space":"target","target_space":"target"})
        )
        .is_err()
    );
    let resolve = json!({"op":"agent.resolve","space":"target","id":"branch"});
    assert!(op(&mut s, "bob", resolve.clone())["environment"].is_null());
    op(
        &mut s,
        "bob",
        json!({"op":"agent.override","space":"target","id":"branch","field":"model","value":"large"}),
    );
    assert!(s.execute("bob",json!({"op":"agent.override","space":"target","id":"branch","field":"skills","value":[]})).is_err());
    let old = run(&mut s, "bob", "target", "agent", "branch");
    assert_eq!(
        old["fields"]["config"]["skill_versions"][0]["instructions"],
        "original skill"
    );
    op(
        &mut s,
        "alice",
        json!({"op":"update","kind":"agent","id":"base","fields":{"instructions":"new prompt","model":"medium","arguments":["--two"]}}),
    );
    let current = op(&mut s, "bob", resolve.clone());
    assert_eq!(current["instructions"], "new prompt");
    assert_eq!(current["model"], "large");
    assert_eq!(current["arguments"][0], "--two");
    assert_eq!(
        get(&mut s, "bob", "target", "run", old["id"].as_str().unwrap())["fields"]["config"]["instructions"],
        "parent prompt"
    );
    op(
        &mut s,
        "bob",
        json!({"op":"agent.override","space":"target","id":"branch","field":"model","value":null}),
    );
    assert_eq!(op(&mut s, "bob", resolve.clone())["model"], "medium");
    assert!(s.execute("bob",json!({"op":"update","kind":"agent","space":"target","id":"branch","fields":{"parent_space":"default"}})).is_err());
    op(
        &mut s,
        "alice",
        json!({"op":"agent.access","id":"base","actor":"bob","permission":"remove"}),
    );
    assert!(s.execute("bob", resolve).is_err());
    assert!(
        s.execute(
            "bob",
            json!({"op":"get","space":"target","kind":"run","id":old["id"]})
        )
        .is_err()
    );
    assert!(
        op(
            &mut s,
            "bob",
            json!({"op":"agent.directory","space":"target"})
        )
        .as_array()
        .unwrap()
        .is_empty()
    );
}
#[test]
fn exclusive_members_can_delegate_only_from_their_own_crew_and_archive_retains_history() {
    let mut s = setup();
    for id in ["lead", "member"] {
        create(
            &mut s,
            "alice",
            "default",
            "agent",
            id,
            json!({"visibility":"space"}),
        );
    }
    create(
        &mut s,
        "alice",
        "default",
        "crew",
        "crew",
        json!({"leader_id":"lead","members":["lead","member"],"visibility":"space"}),
    );
    create(
        &mut s,
        "alice",
        "default",
        "chat",
        "old",
        json!({"agent":"member"}),
    );
    create(
        &mut s,
        "alice",
        "default",
        "message",
        "old-msg",
        json!({"chat":"old","body":"existing"}),
    );
    op(
        &mut s,
        "alice",
        json!({"op":"crew.exclusive","id":"crew","agent":"member","exclusive":true}),
    );
    assert_eq!(dir(&mut s, "alice", "all"), ["lead"]);
    assert!(
        s.execute(
            "alice",
            json!({"op":"create","kind":"chat","title":"new","fields":{"agent":"member"}})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "alice",
            json!({"op":"create","kind":"message","title":"new","fields":{"chat":"old"}})
        )
        .is_err()
    );
    assert!(s.execute("alice",json!({"op":"create","kind":"crew","title":"other","fields":{"leader_id":"lead","members":["member"]}})).is_err());
    assert!(
        s.execute(
            "alice",
            json!({"op":"run.enqueue","handler_id":"member","instruction":"Direct"})
        )
        .is_err()
    );
    assert_eq!(
        op(&mut s, "alice", json!({"op":"chat.history","id":"old"}))[0]["fields"]["body"],
        "existing"
    );
    machine(&mut s, "alice", "default");
    let parent = run(&mut s, "alice", "default", "crew", "crew");
    let lease = claim(&mut s, "alice", "default");
    assert_eq!(parent["id"], lease["run"]["id"]);
    let report = json!({"op":"run.report","machine_id":"box","machine_key":KEY,"lease":lease["lease"],"id":parent["id"],"status":"done","result":"Delegate","delegates":[{"handler_id":"member","instruction":"Review"}]});
    op(&mut s, "alice", report);
    let child = claim(&mut s, "alice", "default");
    assert_eq!(child["run"]["fields"]["handler_id"], "member");
    op(
        &mut s,
        "alice",
        json!({"op":"crew.exclusive","id":"crew","agent":"member","exclusive":false}),
    );
    assert_eq!(
        run(&mut s, "alice", "default", "agent", "member")["fields"]["status"],
        "queued"
    );
    op(
        &mut s,
        "alice",
        json!({"op":"agent.archive","kind":"crew","id":"crew"}),
    );
    assert!(s.execute("alice",json!({"op":"run.enqueue","handler_id":"crew","handler_kind":"crew","instruction":"After archive"})).is_err());
    assert_eq!(
        get(&mut s, "alice", "default", "crew", "crew")["fields"]["archived"],
        true
    );
}
#[test]
fn public_agent_history_masks_private_chats_and_grants_manager_a_scoped_transcript() {
    let mut s = setup();
    create(
        &mut s,
        "alice",
        "default",
        "agent",
        "helper",
        json!({"visibility":"space"}),
    );
    create(
        &mut s,
        "bob",
        "default",
        "chat",
        "room",
        json!({"agent":"helper","visibility":"private"}),
    );
    machine(&mut s, "bob", "default");
    op(
        &mut s,
        "bob",
        json!({"op":"chat.send","id":"room","body":"Historical discussion"}),
    );
    let job = claim(&mut s, "bob", "default");
    op(
        &mut s,
        "bob",
        json!({"op":"run.report","id":job["run"]["id"],"machine_id":"box","machine_key":KEY,"lease":job["lease"],"status":"done","result":"Example answer","usage":{"input_tokens":40,"output_tokens":3}}),
    );
    assert_eq!(
        op(
            &mut s,
            "alice",
            json!({"op":"agent.settings","id":"helper"})
        )["total_tokens"],
        43
    );
    let masked = op(&mut s, "owner", json!({"op":"agent.history","id":"helper"}));
    assert_eq!(masked[0]["masked"], true);
    assert!(masked[0]["id"].is_null());
    assert!(
        s.execute(
            "owner",
            json!({"op":"agent.history","id":"helper","chat":"room"})
        )
        .is_err()
    );
    let admin = op(&mut s, "alice", json!({"op":"agent.history","id":"helper"}));
    assert_eq!(admin[0]["kind"], "manager_transcript");
    assert!(
        s.execute("alice", json!({"op":"get","kind":"chat","id":"room"}))
            .is_err()
    );
    let view = op(
        &mut s,
        "alice",
        json!({"op":"agent.history","id":"helper","chat":"room"}),
    );
    assert_eq!(view["messages"][0]["body"], "Historical discussion");
    assert_eq!(view["messages"][1]["body"], "Example answer");
    create(
        &mut s,
        "bob",
        "default",
        "chat",
        "other",
        json!({"visibility":"private"}),
    );
    assert!(
        s.execute(
            "alice",
            json!({"op":"agent.history","id":"helper","chat":"other"})
        )
        .is_err()
    );
}
