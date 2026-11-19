use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use shipforge_core::Store;
use std::io::Cursor;

fn run(s: &mut Store, who: &str, q: Value) -> Value {
    s.execute(who, q.clone())
        .unwrap_or_else(|error| panic!("{who} {q}: {error}"))
}
fn members(s: &mut Store, space: &str) {
    for who in ["alice", "bob", "carl"] {
        run(
            s,
            "owner",
            json!({"op":"create","space":space,"kind":"member","id":who,"title":who,"fields":{"role":"member"}}),
        );
    }
}
const KEY: &str = "synthetic-key-for-personal-assistant-fixture";
fn register(s: &mut Store, scope: &str, actor: &str) {
    run(
        s,
        actor,
        json!({"op":"machine.register","space":scope,"machine_key":KEY,"machine_id":format!("{actor}-machine"),"runtimes":["local-tool"],"title":"Fixture machine"}),
    );
}
fn image(color: u8) -> String {
    let data = image::RgbImage::from_pixel(41, 19, image::Rgb([color, 46, 78]));
    let mut bytes = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(data)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    STANDARD.encode(bytes.into_inner())
}

#[test]
fn personal_assistant_is_organization_scoped_and_uses_only_its_members_worker() {
    let mut s = Store::memory().unwrap();
    members(&mut s, "default");
    run(
        &mut s,
        "owner",
        json!({"op":"create","kind":"space","id":"project","title":"Project","fields":{"organization":"default","visibility":"private"}}),
    );
    members(&mut s, "project");
    run(
        &mut s,
        "owner",
        json!({"op":"create","kind":"organization","id":"elsewhere","title":"Elsewhere"}),
    );
    members(&mut s, "elsewhere");
    let history = json!({"op":"assistant.history"});
    assert!(run(&mut s, "alice", history.clone())["id"].is_null());
    let p = run(
        &mut s,
        "alice",
        json!({"op":"assistant.open","space":"project"}),
    );
    assert!(p["id"].is_string());
    assert_eq!(p["rooms"], json!([]));
    assert_eq!(
        run(&mut s, "alice", json!({"op":"assistant.open"}))["id"],
        p["id"]
    );
    assert_ne!(
        run(&mut s, "bob", json!({"op":"assistant.open"}))["id"],
        p["id"]
    );
    assert_ne!(
        run(
            &mut s,
            "alice",
            json!({"op":"assistant.open","space":"elsewhere"})
        )["id"],
        p["id"]
    );
    assert!(
        run(&mut s, "alice", json!({"op":"list","kind":"agent"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    register(&mut s, "project", "bob");
    register(&mut s, "elsewhere", "alice");
    assert!(
        run(&mut s, "alice", history.clone())["runtimes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.execute(
            "alice",
            json!({"op":"assistant.send","body":"Not online yet"})
        )
        .is_err()
    );
    register(&mut s, "project", "alice");
    let send = json!({"op":"assistant.send","body":"First real question","model":"local-fixture","request_id":"first"});
    let sent = run(&mut s, "alice", send.clone());
    assert_eq!(sent["space"], "project");
    assert_eq!(run(&mut s, "alice", send), sent);
    assert_eq!(
        run(&mut s, "alice", history.clone())["rooms"][0]["id"],
        sent["id"]
    );
    assert_eq!(run(&mut s, "bob", history.clone())["rooms"], json!([]));
    assert_eq!(
        run(
            &mut s,
            "alice",
            json!({"op":"assistant.history","space":"elsewhere"})
        )["rooms"],
        json!([])
    );
    for q in [
        json!({"op":"get","space":"project","kind":"chat","id":sent["id"]}),
        json!({"op":"assistant.send","id":sent["id"],"body":"Intrusion"}),
        json!({"op":"get","space":"project","kind":"run","id":sent["run"]}),
    ] {
        assert!(s.execute("bob", q).is_err());
    }
    for q in [
        json!({"op":"chat.bind_task","space":"project","id":sent["id"],"task":"example"}),
        json!({"op":"chat.manage","space":"project","id":sent["id"],"action":"add","target":"bob"}),
        json!({"op":"assistant.send","id":sent["id"],"body":"No","instructions":"override"}),
    ] {
        assert!(s.execute("alice", q).is_err());
    }
    assert!(
        run(
            &mut s,
            "bob",
            json!({"op":"run.claim","space":"project","machine_id":"bob-machine","machine_key":KEY})
        )
        .is_null()
    );
    let claim = run(
        &mut s,
        "alice",
        json!({"op":"run.claim","space":"project","machine_id":"alice-machine","machine_key":KEY}),
    );
    assert_eq!(claim["run"]["id"], sent["run"]);
    assert_eq!(claim["run"]["fields"]["config"]["model"], "local-fixture");
    run(
        &mut s,
        "alice",
        json!({"op":"run.report","space":"project","id":sent["run"],"machine_id":"alice-machine","machine_key":KEY,"lease":claim["lease"],"status":"done","result":"Completed fixture"}),
    );
    let next = run(
        &mut s,
        "alice",
        json!({"op":"assistant.send","id":sent["id"],"body":"Continue"}),
    );
    assert_eq!(next["id"], sent["id"]);
    let messages = run(
        &mut s,
        "alice",
        json!({"op":"chat.history","space":"project","id":next["id"]}),
    );
    assert!(
        messages
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["fields"]["body"] == "Completed fixture")
    );
    assert_eq!(
        run(&mut s, "alice", history.clone())["rooms"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let export = run(&mut s, "alice", json!({"op":"export","space":"project"}));
    let mut recovered = Store::memory().unwrap();
    run(
        &mut recovered,
        "owner",
        json!({"op":"import","data":export}),
    );
    let room = run(
        &mut recovered,
        "owner",
        json!({"op":"get","kind":"chat","id":sent["id"]}),
    );
    assert_eq!(room["fields"]["visibility"], "private");
    assert!(room["fields"]["handler_id"].is_null());
    let other = run(
        &mut s,
        "alice",
        json!({"op":"assistant.send","body":"New question"}),
    );
    assert_ne!(other["id"], next["id"]);
    run(
        &mut s,
        "owner",
        json!({"op":"delete","space":"project","kind":"member","id":"alice"}),
    );
    assert_eq!(run(&mut s, "alice", history)["rooms"], json!([]));
    assert!(
        s.execute(
            "alice",
            json!({"op":"assistant.send","id":next["id"],"body":"After revoked"})
        )
        .is_err()
    );
}

#[test]
fn group_avatar_is_normalized_and_ownership_is_rechecked_for_replay() {
    let mut s = Store::memory().unwrap();
    members(&mut s, "default");
    let group = run(
        &mut s,
        "alice",
        json!({"op":"chat.start","mode":"group","title":"Fixture group","members":["bob"]}),
    );
    let id = &group["id"];
    let current = run(&mut s, "alice", json!({"op":"chat.avatar","id":id}));
    let upload = json!({"op":"chat.avatar.set","id":id,"expected_rev":current["rev"],"content_base64":image(26),"request_id":"avatar-first"});
    assert!(s.execute("bob", upload.clone()).is_err());
    assert!(s.execute("alice",json!({"op":"chat.avatar.set","id":id,"expected_rev":current["rev"],"content_base64":STANDARD.encode(b"invalid")})).is_err());
    let put = run(&mut s, "alice", upload.clone());
    let full = run(&mut s, "bob", json!({"op":"chat.avatar","id":id}));
    let data = STANDARD
        .decode(full["avatar"]["content_base64"].as_str().unwrap())
        .unwrap();
    let square = image::load_from_memory(&data).unwrap();
    assert_eq!((square.width(), square.height()), (128, 128));
    let sidebar = run(&mut s, "bob", json!({"op":"chat.sidebar"}));
    let icon = sidebar[0]["fields"]["group_avatar"]["icon"]
        .as_str()
        .unwrap()
        .strip_prefix("data:image/png;base64,")
        .unwrap();
    assert_eq!(
        image::load_from_memory(&STANDARD.decode(icon).unwrap())
            .unwrap()
            .width(),
        32
    );
    assert!(
        s.execute("carl", json!({"op":"chat.avatar","id":id}))
            .is_err()
    );
    let same = run(
        &mut s,
        "alice",
        json!({"op":"chat.avatar.set","id":id,"expected_rev":put["rev"],"content_base64":image(26)}),
    );
    assert_eq!(put["rev"], same["rev"]);
    run(
        &mut s,
        "alice",
        json!({"op":"chat.manage","id":id,"action":"transfer","target":"bob"}),
    );
    assert!(s.execute("alice", upload).is_err());
    assert!(
        s.execute(
            "bob",
            json!({"op":"chat.avatar.set","id":id,"expected_rev":put["rev"],"remove":true})
        )
        .is_err()
    );
    let rev = run(&mut s, "bob", json!({"op":"chat.avatar","id":id}))["rev"].clone();
    run(
        &mut s,
        "bob",
        json!({"op":"chat.avatar.set","id":id,"expected_rev":rev,"remove":true}),
    );
    assert!(run(&mut s, "alice", json!({"op":"chat.avatar","id":id}))["avatar"].is_null());
    run(
        &mut s,
        "bob",
        json!({"op":"chat.manage","id":id,"action":"remove","target":"alice"}),
    );
    assert!(
        s.execute("alice", json!({"op":"chat.avatar","id":id}))
            .is_err()
    );
    run(
        &mut s,
        "bob",
        json!({"op":"chat.manage","id":id,"action":"dissolve"}),
    );
    assert!(
        s.execute("bob", json!({"op":"chat.avatar","id":id}))
            .is_err()
    );
}

#[test]
fn member_task_rooms_snapshot_only_that_members_visible_task_context() {
    let mut s = Store::memory().unwrap();
    members(&mut s, "default");
    run(
        &mut s,
        "alice",
        json!({"op":"create","kind":"agent","id":"shared","title":"Shared agent","fields":{"runtime":"local-tool"}}),
    );
    run(
        &mut s,
        "alice",
        json!({"op":"create","kind":"agent","id":"own","title":"Private specialist","fields":{"visibility":"private","runtime":"local-tool"}}),
    );
    assert!(
        s.execute(
            "bob",
            json!({"op":"chat.start","mode":"agent","target":"own"})
        )
        .is_err()
    );
    run(
        &mut s,
        "alice",
        json!({"op":"create","kind":"task","id":"work","title":"Shared task","fields":{"status":"backlog","description":"Approved summary","assignee_kind":"agent","assignee":"shared"}}),
    );
    for (id, visibility) in [("visible", "space"), ("sensitive", "private")] {
        run(
            &mut s,
            "alice",
            json!({"op":"create","kind":"comment","id":id,"title":id,"fields":{"body":id,"task":"work","visibility":visibility}}),
        );
    }
    let open = json!({"op":"task.conversation","id":"work"});
    let a = run(&mut s, "alice", open.clone());
    let b = run(&mut s, "bob", open.clone());
    assert_ne!(a["id"], b["id"]);
    assert_eq!(run(&mut s, "bob", open)["id"], b["id"]);
    assert!(
        s.execute("bob", json!({"op":"chat.history","id":a["id"]}))
            .is_err()
    );
    let sent = run(
        &mut s,
        "bob",
        json!({"op":"chat.send","id":b["id"],"body":"Use the task"}),
    );
    let context = run(
        &mut s,
        "bob",
        json!({"op":"get","kind":"run","id":sent["run"]}),
    )["fields"]["task_context"]
        .clone();
    assert_eq!(context["description"], "Approved summary");
    assert_eq!(
        context["comments"],
        json!([{"title":"visible","body":"visible"}])
    );
    run(
        &mut s,
        "alice",
        json!({"op":"update","kind":"task","id":"work","fields":{"visibility":"private"}}),
    );
    assert!(
        s.execute("bob", json!({"op":"chat.history","id":b["id"]}))
            .is_err()
    );
    assert!(
        s.execute("bob", json!({"op":"get","kind":"run","id":sent["run"]}))
            .is_err()
    );
}
