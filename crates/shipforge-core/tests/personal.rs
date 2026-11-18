use serde_json::{Value, json};
use shipforge_core::{Store, preferences::normalize_shortcut};
use tempfile::tempdir;

fn run(db: &mut Store, op: Value) -> Value {
    db.execute("owner", op.clone())
        .unwrap_or_else(|e| panic!("{op}: {e}"))
}
#[test]
fn preferences_restore_per_actor_and_reject_conflicting_or_reserved_keys() {
    let folder = tempdir().unwrap();
    let path = folder.path().join("个人设置.db");
    let mut db = Store::open(&path).unwrap();
    let initial = run(&mut db, json!({"op":"preferences.get"}));
    assert_eq!(
        initial["preferences"]["shortcuts"]["command_menu"],
        "ctrl-k"
    );
    let change = json!({"op":"preferences.update","expected_rev":1,"request_id":"new-options","preferences":{"theme":"magic-blue","frame_color":"color-6","language":"en","send_with":"shift-enter","open_after_assignment":false,"shortcuts":{"command_menu":"Alt+Shift+J"}}});
    let accepted = run(&mut db, change.clone());
    assert_eq!(accepted["rev"], 2);
    assert_eq!(
        accepted["preferences"]["shortcuts"]["command_menu"],
        "alt-shift-j"
    );
    assert_eq!(run(&mut db, change), accepted);
    assert_eq!(
        db.execute("reader", json!({"op":"preferences.get"}))
            .unwrap(),
        initial
    );
    drop(db);
    let mut restored = Store::open(&path).unwrap();
    assert_eq!(
        run(&mut restored, json!({"op":"preferences.get"})),
        accepted
    );
    for patch in [
        json!({"theme":"unknown"}),
        json!({"shortcuts":{"find_in_view":"alt-shift-j"}}),
        json!({"shortcuts":{"find_in_view":"Ctrl+V"}}),
        json!({"send_with":true}),
        json!({"language":"not-a-locale"}),
        json!({"other":1}),
    ] {
        assert!(
            restored
                .execute(
                    "owner",
                    json!({"op":"preferences.update","preferences":patch})
                )
                .is_err()
        );
    }
    assert!(restored.execute("owner",json!({"op":"preferences.update","expected_rev":1,"preferences":{"frame_color":"none"}})).is_err());
    assert!(
        restored
            .execute("reader", json!({"op":"preferences.get","actor":"owner"}))
            .is_err()
    );
    let reset = run(
        &mut restored,
        json!({"op":"preferences.update","preferences":{"shortcuts":{"command_menu":null}}}),
    );
    assert_eq!(
        reset["preferences"]["shortcuts"],
        initial["preferences"]["shortcuts"]
    );
    assert_eq!(reset["preferences"]["frame_color"], "color-6");
    for v in [
        "a",
        "Ctrl+Ctrl+K",
        "F01",
        "F25",
        "Alt+F4",
        "Ctrl+Z",
        "Win+K",
        "ctrl-shift-",
    ] {
        assert!(normalize_shortcut(v).is_err(), "{v}");
    }
    assert_eq!(normalize_shortcut("F11").unwrap(), "f11");
}
#[test]
fn move_task_preserves_content_comments_attachment_and_permissions() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        json!({"op":"create","kind":"space","id":"next","title":"Other","fields":{"key":"NEXT"}}),
    );
    run(
        &mut db,
        json!({"op":"create","kind":"task","id":"example","title":"First","fields":{"description":"details"}}),
    );
    run(
        &mut db,
        json!({"op":"create","kind":"comment","id":"reply","title":"Reply","fields":{"task":"example","body":"Keep this comment"}}),
    );
    let attached = run(
        &mut db,
        json!({"op":"attachment.upload","kind":"task","id":"example","filename":"sample.txt","content_base64":"YWJj"}),
    );
    let file = &attached["fields"]["attachments"][0]["id"];
    let request = json!({"op":"task.move","id":"example","target":"next","expected_rev":attached["rev"],"request_id":"move-once"});
    assert!(db.execute("stranger", request.clone()).is_err());
    let moved = run(&mut db, request.clone());
    assert_eq!(moved["space"], "next");
    assert_eq!(moved["fields"]["description"], "details");
    assert_eq!(moved["fields"]["number"], "NEXT-1");
    assert_eq!(run(&mut db, request)["id"], moved["id"]);
    assert!(
        db.execute("owner", json!({"op":"get","kind":"task","id":"example"}))
            .is_err()
    );
    let reply = run(
        &mut db,
        json!({"op":"get","space":"next","kind":"comment","id":"reply"}),
    );
    assert_eq!(reply["fields"]["body"], "Keep this comment");
    assert_eq!(
        run(
            &mut db,
            json!({"op":"attachment.download","space":"next","kind":"task","id":"example","attachment_id":file})
        )["content_base64"],
        "YWJj"
    );
    let back = run(
        &mut db,
        json!({"op":"task.move","id":"example","space":"next","target":"default"}),
    );
    assert_eq!(back["fields"]["number"], "SF-2");
}
#[test]
fn task_conversations_reuse_the_assignee_and_quick_commands_bind_the_run() {
    let mut db = Store::memory().unwrap();
    run(
        &mut db,
        json!({"op":"create","kind":"agent","id":"writer","title":"Writer"}),
    );
    run(
        &mut db,
        json!({"op":"create","kind":"task","id":"outline","title":"Outline","fields":{"assignee":"writer","assignee_kind":"agent","status":"in_progress"}}),
    );
    let conversation = json!({"op":"task.conversation","id":"outline"});
    let chat = run(&mut db, conversation.clone());
    assert_eq!(chat["fields"]["task"], "outline");
    assert_eq!(chat["fields"]["visibility"], "private");
    assert_eq!(run(&mut db, conversation)["id"], chat["id"]);
    let command = json!({"op":"task.command","id":"outline","request_id":"first-command"});
    let result = run(&mut db, command.clone());
    assert_eq!(result["run"]["fields"]["chat_id"], chat["id"]);
    assert_eq!(result["run"]["fields"]["task_id"], "outline");
    assert_eq!(run(&mut db, command)["run"]["id"], result["run"]["id"]);
}
