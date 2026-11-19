use serde_json::{Value, json};
use shipforge_core::Store;

fn run(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|error| panic!("{q}: {error}"))
}

fn save(db: &mut Store, space: &str, level: &str, id: Option<&str>, settings: Value) -> Value {
    let current = run(
        db,
        "owner",
        json!({"op":"git.settings","space":space,"level":level,"id":id}),
    );
    run(
        db,
        "owner",
        json!({"op":"git.configure","space":space,"level":level,"id":id,"expected_rev":current["rev"],"settings":settings}),
    )
}

#[test]
fn workspaces_resolve_branches_author_and_member_access_at_claim() {
    let mut db = Store::memory().unwrap();
    let actor = db.verified_login("member@example.test").unwrap()["actor"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","id":actor,"title":"Mira","fields":{"role":"member"}}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","id":"code","title":"Dev","fields":{"key":"DEV","visibility":"private"}}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","space":"code","kind":"member","id":actor,"title":"Mira","fields":{"role":"member"}}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","space":"code","kind":"agent","id":"builder","title":"Builder","fields":{}}),
    );
    let org = save(
        &mut db,
        "code",
        "organization",
        None,
        json!({"author":{"name":{"mode":"manual","value":"Org Name"},"email":{"mode":"trigger"},"coauthor":false}}),
    );
    assert_eq!(org["author_sources"]["name"], "organization");
    save(
        &mut db,
        "default",
        "space",
        None,
        json!({"author":{"name":{"mode":"manual","value":"Do Not Inherit from Root Space"}}}),
    );
    let org = run(
        &mut db,
        "owner",
        json!({"op":"git.settings","level":"organization"}),
    );
    assert_eq!(org["author"]["name"]["value"], "Org Name");
    let repo_a = "https://git.example.test/dev/kit.git";
    let repo_b = "https://git.example.test/dev/docs.git";
    let repo_c = "git@git.example.test:dev/devenv.git";
    let space = save(
        &mut db,
        "code",
        "space",
        None,
        json!({
            "author":{"name":{"mode":"trigger_and_agent"}},
            "repositories":[{"url":repo_a,"branch":"space/stable"},{"url":repo_b,"branch":"docs"}]
        }),
    );
    let initial = run(
        &mut db,
        &actor,
        json!({"op":"git.settings","space":"code","level":"agent","id":"builder"}),
    );
    assert_eq!(initial["repositories"].as_array().unwrap().len(), 2);
    assert_eq!(initial["effective_include_space"]["mode"], "all");
    assert!(db.execute(&actor,json!({"op":"git.configure","space":"code","level":"space","expected_rev":space["rev"],"settings":null})).unwrap_err().starts_with("forbidden:"));
    let agent = save(
        &mut db,
        "code",
        "agent",
        Some("builder"),
        json!({
            "author":{"coauthor":true},"repositories":[{"url":repo_a},{"url":"git@git.example.test:dev/devenv.git","branch":"agent/base"}],
            "include_space":{"mode":"selected","urls":[repo_b]}
        }),
    );
    assert_eq!(
        agent["author_sources"],
        json!({"name":"space","email":"organization","coauthor":"agent"})
    );
    assert_eq!(agent["repositories"][0]["branch"], "space/stable");
    assert_eq!(agent["repositories"][1]["url"], repo_c);
    assert_eq!(agent["repositories"][1]["branch"], "agent/base");
    assert_eq!(agent["repositories"][2]["url"], repo_b);
    assert!(db.execute("owner",json!({"op":"git.configure","space":"code","level":"agent","id":"builder","expected_rev":initial["rev"],"settings":null})).unwrap_err().starts_with("conflict:"));
    for settings in [
        json!({"repositories":[{"url":"file:///tmp/remote"}]}),
        json!({"repositories":[{"url":repo_a,"branch":"topic/.hidden"}]}),
        json!({"repositories":[{"url":"https://token@git.example.test/team/source.git"}]}),
    ] {
        assert!(db.execute("owner",json!({"op":"git.configure","space":"code","level":"space","expected_rev":space["rev"],"settings":settings})).unwrap_err().starts_with("invalid:"));
    }
    let secret = "isolated-machine-key-for-git-checkout";
    run(
        &mut db,
        "owner",
        json!({"op":"machine.register","space":"code","machine_id":"host","machine_key":secret,"title":"Fixture","shared":true,"runtimes":["local-tool"]}),
    );
    let queued = run(
        &mut db,
        &actor,
        json!({"op":"run.enqueue","space":"code","handler_id":"builder","instruction":"Review files","checkout_branch":"runs/hotfix"}),
    );
    let claim = run(
        &mut db,
        "owner",
        json!({"op":"run.claim","space":"code","machine_id":"host","machine_key":secret}),
    );
    assert_eq!(claim["run"]["id"], queued["id"]);
    assert_eq!(
        claim["git"]["author"],
        json!({"name":"Mira & Builder","email":"member@example.test","coauthor":true})
    );
    assert!(
        claim["git"]["repositories"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["branch"] == "runs/hotfix")
    );
    let stored = run(
        &mut db,
        "owner",
        json!({"op":"get","space":"code","kind":"run","id":queued["id"]}),
    );
    assert!(!stored.to_string().contains("member@example.test"));
}

#[test]
fn variant_keeps_parent_settings_by_field_and_space_list_follows_joined_roles() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","id":"worker","title":"Worker","fields":{"role":"member"}}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","id":"hidden","title":"Hidden","fields":{"key":"HID","visibility":"private"}}),
    );
    save(
        &mut db,
        "hidden",
        "space",
        None,
        json!({"repositories":[{"url":"https://git.example.test/team/private.git"}]}),
    );
    let repos = run(
        &mut db,
        "worker",
        json!({"op":"git.settings","level":"organization"}),
    );
    assert!(!repos.to_string().contains("private.git"));
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"agent","id":"original","title":"Organizer","fields":{}}),
    );
    run(
        &mut db,
        "owner",
        json!({"op":"create","kind":"agent","id":"variant","title":"Variant","fields":{"parent_id":"original"}}),
    );
    save(
        &mut db,
        "default",
        "agent",
        Some("original"),
        json!({"author":{"name":{"mode":"owner_and_agent"},"coauthor":false},"repositories":[{"url":"https://example.test/team/public.git"}]}),
    );
    let tuned = save(
        &mut db,
        "default",
        "agent",
        Some("variant"),
        json!({"author":{"coauthor":true}}),
    );
    assert_eq!(
        tuned["author_sources"],
        json!({"name":"parent_agent","email":"default","coauthor":"agent"})
    );
    assert_eq!(tuned["repositories_source"], "parent_agent");
    assert_eq!(
        tuned["agent_repositories"][0]["url"],
        "https://example.test/team/public.git"
    );
    let cleared = save(
        &mut db,
        "default",
        "agent",
        Some("variant"),
        json!({"repositories":[]}),
    );
    assert_eq!(cleared["repositories_source"], "agent");
    assert_eq!(cleared["agent_repositories"], json!([]));
}
