use serde_json::{Value, json};
use shipforge_core::Store;
use tempfile::tempdir;

fn execute(store: &mut Store, actor: &str, q: Value) -> Value {
    store
        .execute(actor, q.clone())
        .unwrap_or_else(|error| panic!("{q}: {error}"))
}
fn accept(store: &mut Store, actor: &str, invite: &Value) -> Result<Value, String> {
    store.execute(
        actor,
        json!({"op":"invitation.accept","link":invite["link"]}),
    )
}
#[test]
fn addressed_invitations_require_the_right_identity_and_survive_restart_without_disclosing_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("invite.db");
    let mut s = Store::open(&path).unwrap();
    execute(
        &mut s,
        "owner",
        json!({"op":"create","kind":"space","id":"design","title":"Design","fields":{"key":"DES","visibility":"private"}}),
    );
    let issue = json!({"op":"invitation.issue","channel":"email","email":"Member@example.test","role":"admin","spaces":["design"],"request_id":"test-addressed"});
    let invite = execute(&mut s, "owner", issue.clone());
    assert_eq!(execute(&mut s, "owner", issue), invite);
    assert!(accept(&mut s, "other@example.test", &invite).is_err());
    assert!(
        s.execute("other@example.test", json!({"op":"invitation.list"}))
            .is_err()
    );
    drop(s);
    let mut s = Store::open(&path).unwrap();
    let actor = "member@example.test";
    assert_eq!(accept(&mut s, actor, &invite).unwrap()["joined"], true);
    assert_eq!(accept(&mut s, actor, &invite).unwrap()["joined"], false);
    assert_eq!(
        execute(&mut s, actor, json!({"op":"whoami"}))["role"],
        "admin"
    );
    assert_eq!(
        execute(&mut s, actor, json!({"op":"whoami","space":"design"}))["role"],
        "member"
    );
    assert_eq!(
        execute(&mut s, "owner", json!({"op":"onboarding.get"}))["completed"],
        json!(["invite_member"])
    );
    let listed = execute(&mut s, "owner", json!({"op":"invitation.list"}));
    assert_eq!(listed[0]["fields"]["state"], "accepted");
    assert!(!listed.to_string().contains("shipforge://invite/"));
    assert!(
        !execute(&mut s, "owner", json!({"op":"events"}))
            .to_string()
            .contains("shipforge://invite/")
    );
}
#[test]
fn revocation_invalidates_the_old_link_and_existing_roles_are_preserved() {
    let mut s = Store::memory().unwrap();
    let issue = json!({"op":"invitation.issue","channel":"link","role":"guest"});
    assert!(
        s.execute(
            "owner",
            json!({"op":"invitation.issue","channel":"link","role":"admin"})
        )
        .is_err()
    );
    let issued = execute(&mut s, "owner", issue);
    let id = &issued["invitation"]["id"];
    let copy = json!({"op":"invitation.copy","id":id});
    assert_eq!(
        execute(&mut s, "owner", copy.clone())["link"],
        issued["link"]
    );
    assert_eq!(accept(&mut s, "guest", &issued).unwrap()["joined"], true);
    assert_eq!(
        execute(&mut s, "guest", json!({"op":"whoami"}))["role"],
        "guest"
    );
    assert!(
        execute(&mut s, "guest", json!({"op":"list","kind":"invitation"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(s.execute("guest", copy.clone()).is_err());
    assert!(s.execute("guest", json!({"op":"invitation.list"})).is_err());
    assert_eq!(accept(&mut s, "owner", &issued).unwrap()["joined"], false);
    assert_eq!(
        execute(&mut s, "owner", json!({"op":"whoami"}))["role"],
        "owner"
    );
    execute(&mut s, "owner", json!({"op":"invitation.revoke","id":id}));
    assert!(accept(&mut s, "later", &issued).is_err());
    let reopened = execute(&mut s, "owner", copy);
    assert_ne!(reopened["link"], issued["link"]);
    assert!(accept(&mut s, "later", &issued).is_err());
    assert_eq!(accept(&mut s, "later", &reopened).unwrap()["joined"], true);
    assert!(s.execute("owner",json!({"op":"create","kind":"invitation","title":"fake","fields":{"state":"active","role":"owner"}})).is_err());
    assert!(
        s.execute(
            "owner",
            json!({"op":"invitation.issue","channel":"email","email":"bad","role":"member"})
        )
        .is_err()
    );
}
#[test]
fn revoked_authority_expiry_and_foreign_spaces_cannot_grant_membership() {
    let mut s = Store::memory().unwrap();
    execute(
        &mut s,
        "owner",
        json!({"op":"create","kind":"organization","id":"other-org","title":"Other"}),
    );
    assert!(
        s.execute(
            "owner",
            json!({"op":"invitation.issue","channel":"link","role":"guest","spaces":["other-org"]})
        )
        .is_err()
    );
    assert!(
        s.execute(
            "owner",
            json!({"op":"invitation.issue","channel":"link","role":"guest","expires_at":1})
        )
        .is_err()
    );
    execute(
        &mut s,
        "owner",
        json!({"op":"create","kind":"member","id":"manager","title":"Manager","fields":{"role":"admin"}}),
    );
    let issued = execute(
        &mut s,
        "manager",
        json!({"op":"invitation.issue","channel":"link","role":"member"}),
    );
    execute(
        &mut s,
        "owner",
        json!({"op":"update","kind":"member","id":"manager","fields":{"role":"member"}}),
    );
    assert!(accept(&mut s, "newcomer", &issued).is_err());
    assert!(s.execute("newcomer", json!({"op":"whoami"})).is_err());
}
