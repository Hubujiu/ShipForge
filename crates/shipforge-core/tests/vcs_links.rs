use serde_json::{Value, json};
use shipforge_core::Store;
fn execute(db: &mut Store, q: Value) -> Value {
    db.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn setup(provider: &str) -> Store {
    let mut db = Store::memory().unwrap();
    execute(
        &mut db,
        json!({"op":"create","kind":"connector","id":"origin","title":"Origin","fields":{"provider":provider,"enabled":true,"link_tasks":true,"repository":"group/demo"}}),
    );
    for i in 1..=12 {
        execute(
            &mut db,
            json!({"op":"create","kind":"task","id":format!("t{i}"),"title":format!("Task {i}"),"fields":if i==2{json!({"visibility":"private"})}else{json!({})}}),
        );
    }
    db
}
fn push(provider: &str, event: &str) -> Value {
    let mut payload = if provider == "github" {
        json!({"repository":{"full_name":"group/demo","html_url":"https://github.com/group/demo"}})
    } else {
        json!({"object_kind":"push","project":{"id":7,"path_with_namespace":"group/demo","web_url":"https://gitlab.com/group/demo"}})
    };
    payload["commits"] = json!([{"id":"abcd".repeat(10),"message":"fix SF-12 and SF-1; XSf-3X, SF-2 is private, SF-999 absent"}]);
    json!({"op":"provider.ingest","id":"origin","provider":provider,"event_id":event,"payload":payload})
}
fn review(provider: &str) -> Value {
    let mut q = push(provider, "review");
    let p = &mut q["payload"];
    p.as_object_mut().unwrap().remove("commits");
    if provider == "github" {
        p["pull_request"] = json!({"number":17,"title":"Review","body":"","author_association":"MEMBER","head":{"ref":"feat/SF-3-notes"},"base":{"repo":{"full_name":"group/demo"}}});
        p["number"] = json!(17);
        p["action"] = json!("opened");
    } else {
        p["object_kind"] = json!("merge_request");
        p["object_attributes"] = json!({"iid":17,"title":"Review","source_branch":"feat/SF-3-notes","action":"open","target_project_id":7});
    }
    q
}
#[test]
fn commits_match_whole_task_numbers_and_replays_do_not_duplicate() {
    for provider in ["github", "gitlab"] {
        let mut db = setup(provider);
        let mut q = push(provider, "delivery1");
        q["op"] = json!("provider.preview");
        let preview = execute(&mut db, q.clone());
        assert_eq!(preview["links"].as_array().unwrap().len(), 2);
        assert_eq!(
            execute(&mut db, json!({"op":"list","kind":"comment"})),
            json!([])
        );
        q["op"] = json!("provider.ingest");
        let result = execute(&mut db, q.clone());
        let tasks: Vec<_> = result["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["task"].as_str().unwrap())
            .collect();
        assert_eq!(tasks, vec!["t1", "t12"]);
        assert_eq!(execute(&mut db, q.clone()), result);
        let link = &result["links"][0];
        assert!(
            link["url"]
                .as_str()
                .unwrap()
                .ends_with(&format!("/{}", "abcd".repeat(10)))
        );
        let row = execute(
            &mut db,
            json!({"op":"get","kind":"comment","id":link["comment"]}),
        );
        assert_eq!(row["fields"]["vcs"]["url"], link["url"]);
        q["event_id"] = json!("another-delivery");
        execute(&mut db, q.clone());
        assert_eq!(
            execute(&mut db, json!({"op":"list","kind":"comment"}))
                .as_array()
                .unwrap()
                .len(),
            2
        );
        q["payload"]["commits"][0]["message"] = json!("tamper SF-11");
        assert!(db.execute("owner", q).is_err());
    }
}
#[test]
fn pull_requests_and_merge_requests_retain_their_task_on_later_transitions() {
    for provider in ["github", "gitlab"] {
        let mut db = setup(provider);
        let mut q = review(provider);
        let out = execute(&mut db, q.clone());
        let link = &out["links"][0];
        assert_eq!(link["task"], "t3");
        let url = link["url"].as_str().unwrap();
        assert!(url.ends_with(if provider == "github" {
            "/pull/17"
        } else {
            "/-/merge_requests/17"
        }));
        q["event_id"] = json!("merged");
        if provider == "github" {
            q["payload"]["action"] = json!("closed");
            q["payload"]["pull_request"]["merged"] = json!(true);
            q["payload"]["pull_request"]["head"]["ref"] = json!("main");
        } else {
            q["payload"]["object_attributes"]["action"] = json!("merge");
            q["payload"]["object_attributes"]["source_branch"] = json!("main");
        }
        let after = execute(&mut db, q.clone());
        assert_eq!(after["links"][0]["task"], "t3");
        assert_ne!(link["comment"], after["links"][0]["comment"]);
        if provider == "github" {
            q["payload"]["repository"]["html_url"] = json!("https://unrelated.example/group/demo");
        } else {
            q["payload"]["project"]["web_url"] = json!("https://unrelated.example/group/demo");
        }
        q["event_id"] = json!("bad");
        assert!(db.execute("owner", q).is_err());
    }
}
#[test]
fn disabled_unbound_cross_space_and_invalid_credentials_cannot_create_activity() {
    let mut db = setup("gitlab");
    let q = push("gitlab", "d");
    execute(
        &mut db,
        json!({"op":"create","kind":"member","id":"reader","title":"Reader","fields":{"role":"viewer"}}),
    );
    assert!(
        db.execute("reader", q.clone())
            .unwrap_err()
            .starts_with("forbidden:")
    );
    execute(
        &mut db,
        json!({"op":"create","kind":"space","id":"other","title":"Other space"}),
    );
    let mut other = q.clone();
    other["space"] = json!("other");
    assert!(db.execute("owner", other).is_err());
    for patch in [
        json!({"enabled":false}),
        json!({"enabled":true,"link_tasks":false}),
        json!({"link_tasks":true,"repository":"another/demo"}),
    ] {
        execute(
            &mut db,
            json!({"op":"update","kind":"connector","id":"origin","fields":patch}),
        );
        assert!(db.execute("owner", q.clone()).is_err());
    }
    for bad in ["bad", "../malicious", "suite/demo?key=bad", "suite//demo"] {
        assert!(
            db.execute(
                "owner",
                json!({"op":"update","kind":"connector","id":"origin","fields":{"repository":bad}})
            )
            .is_err()
        );
    }
    assert!(db.execute("owner",json!({"op":"create","kind":"connector","title":"Unsupported","fields":{"provider":"slack","link_tasks":true,"repository":"a/b"}})).is_err());
    assert_eq!(
        execute(&mut db, json!({"op":"list","kind":"comment"})),
        json!([])
    );
}

#[test]
fn only_repository_collaborators_can_link_public_github_pull_requests() {
    let mut db = setup("github");
    let mut q = review("github");
    q["payload"]["repository"]["private"] = json!(false);
    q["payload"]["pull_request"]["author_association"] = json!("CONTRIBUTOR");
    assert_eq!(execute(&mut db, q.clone())["links"], json!([]));
    q["event_id"] = json!("fork");
    q["payload"]["pull_request"]
        .as_object_mut()
        .unwrap()
        .remove("author_association");
    assert_eq!(execute(&mut db, q.clone())["links"], json!([]));
    q["event_id"] = json!("member");
    q["payload"]["pull_request"]["author_association"] = json!("COLLABORATOR");
    assert_eq!(execute(&mut db, q)["links"][0]["task"], "t3");
}
