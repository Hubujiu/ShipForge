use serde_json::{Value, json};
use shipforge_core::Store;

fn apply(db: &mut Store, q: Value) -> Value {
    db.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn connector(db: &mut Store, space: &str, id: &str, provider: &str) {
    let mut f = json!({"provider":provider,"enabled":true});
    if provider == "slack" {
        f = json!({"provider":provider,"enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1"});
    }
    apply(
        db,
        json!({"op":"create","kind":"connector","space":space,"id":id,"title":provider,"fields":f}),
    );
}
fn slack(event: &str, ts: &str, root: Option<&str>, text: &str) -> Value {
    let mut m = json!({"type":"message","user":"U1","channel":"C1","ts":ts,"text":text});
    if let Some(root) = root {
        m["thread_ts"] = json!(root);
    }
    json!({"op":"provider.ingest","id":"support","provider":"slack","payload":{"type":"event_callback","team_id":"T1","api_app_id":"A1","event_id":event,"event":m}})
}
fn comment(db: &mut Store, task: &Value, body: &str) -> Value {
    let options = apply(db, json!({"op":"im.threads","task_id":task}));
    apply(
        db,
        json!({"op":"im.reply","id":"support","task_id":task,"thread":options[0]["thread"],"body":body,"request_id":body}),
    )
}
fn queue(db: &mut Store, op: &str) -> Value {
    apply(db, json!({"op":op,"id":"support"}))
}
fn ack(item: &Value, outcome: &str) -> Value {
    json!({"op":"im.ack","id":"support","delivery_id":item["delivery_id"],"lease":item["lease"],"outcome":outcome})
}

#[test]
fn external_reply_chooses_exactly_one_thread_and_keeps_internal_comments_local() {
    let mut db = Store::memory().unwrap();
    connector(&mut db, "default", "support", "slack");
    let root = apply(&mut db, slack("first", "1.000001", None, "First"));
    let task = &root["task"];
    let mut second = slack("second", "2.000001", None, "Second");
    second["op"] = json!("im.bind");
    second["task_id"] = task.clone();
    apply(&mut db, second);
    let topics = apply(&mut db, json!({"op":"im.threads","task_id":task}));
    assert_eq!(topics.as_array().unwrap().len(), 2);
    assert_eq!(
        apply(&mut db, json!({"op":"get","kind":"task","id":task}))["fields"]["im_threads"],
        topics
    );
    apply(
        &mut db,
        json!({"op":"create","kind":"comment","title":"Team only","fields":{"task":task,"reply_thread":topics[0]["thread"]}}),
    );
    assert_eq!(queue(&mut db, "im.outbox"), json!([]));
    let q = json!({"op":"im.reply","id":"support","task_id":task,"thread":topics[1]["thread"],"request_id":"one","body":"Only to the second thread"});
    let reply = apply(&mut db, q.clone());
    assert_eq!(apply(&mut db, q.clone()), reply);
    let mut collision = q.clone();
    collision["thread"] = topics[0]["thread"].clone();
    assert!(db.execute("owner", collision).is_err());
    let mut unrelated = q.clone();
    unrelated["thread"]["thread"] = json!("9.000009");
    assert!(db.execute("owner", unrelated).is_err());
    apply(
        &mut db,
        json!({"op":"create","kind":"member","id":"visitor","title":"Viewer","fields":{"role":"viewer"}}),
    );
    assert!(db.execute("visitor", q).is_err());
    let next = queue(&mut db, "im.claim");
    assert_eq!(next["thread"], topics[1]["thread"]);
    assert_eq!(queue(&mut db, "im.outbox").as_array().unwrap().len(), 1);
}

#[test]
fn slack_conversation_is_transactional_scoped_and_replay_safe() {
    let mut db = Store::memory().unwrap();
    connector(&mut db, "default", "support", "slack");
    let first = slack("E2", "2.000002", Some("1.000001"), "Reply arrived first");
    let answer = apply(&mut db, first.clone());
    let task = &answer["task"];
    let parent = slack("E1", "1.000001", None, "Original message");
    let added = apply(&mut db, parent.clone());
    assert_eq!(added["action"], "commented");
    assert_eq!(&added["task"], task);
    let later = slack("E3", "3.000003", Some("1.000001"), "Another reply");
    let reply = apply(&mut db, later.clone());
    assert_eq!(apply(&mut db, later.clone()), reply);
    assert_eq!(apply(&mut db, first.clone()), answer);
    let mut duplicate = later.clone();
    duplicate["payload"]["event_id"] = json!("NewDelivery");
    assert_eq!(apply(&mut db, duplicate.clone()), reply);
    duplicate["payload"]["event"]["text"] = json!("tampered");
    assert!(db.execute("owner", duplicate).is_err());
    let mut renamed = slack(
        "NewDelivery",
        "4.000004",
        Some("1.000001"),
        "reused delivery",
    );
    assert!(db.execute("owner", renamed.clone()).is_err());
    renamed["payload"]["event_id"] = json!("x");
    renamed["payload"]["event"]["channel"] = json!("C2");
    assert!(db.execute("owner", renamed).is_err());
    assert_eq!(
        apply(&mut db, json!({"op":"list","kind":"task"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let c = apply(
        &mut db,
        json!({"op":"get","kind":"comment","id":reply["comment"]}),
    );
    assert_eq!(c["fields"]["external_author"], "U1");
    assert_eq!(c["fields"]["external_thread"]["thread"], "1.000001");
    assert_eq!(queue(&mut db, "im.outbox"), json!([]));

    apply(
        &mut db,
        json!({"op":"create","kind":"space","id":"second","title":"Second"}),
    );
    connector(&mut db, "second", "support", "slack");
    let mut other = first;
    other["space"] = json!("second");
    let other = apply(&mut db, other);
    assert_ne!(other["task"], *task);
    apply(
        &mut db,
        json!({"op":"create","kind":"member","id":"reader","title":"Reader","fields":{"role":"viewer"}}),
    );
    for op in [
        "im.outbox",
        "im.claim",
        "im.bind",
        "im.unbind",
        "im.retry",
        "im.ack",
    ] {
        assert!(
            db.execute("reader", json!({"op":op,"id":"support"}))
                .unwrap_err()
                .starts_with("forbidden:")
        );
    }
    assert!(
        db.execute(
            "reader",
            json!({"op":"im.outbox","id":"support","space":"second"})
        )
        .is_err()
    );
}

#[test]
fn explicit_binding_status_local_comments_recovery_and_private_records() {
    let mut db = Store::memory().unwrap();
    connector(&mut db, "default", "support", "slack");
    let task = apply(
        &mut db,
        json!({"op":"create","kind":"task","id":"local","title":"Existing task"}),
    );
    let mut binding = slack("E1", "1.000001", None, "Root");
    binding["op"] = json!("im.bind");
    binding["task_id"] = task["id"].clone();
    let bound = apply(&mut db, binding.clone());
    assert_eq!(bound["active"], true);
    let first = apply(&mut db, slack("E1", "1.000001", None, "Root"));
    assert_eq!(first["task"], "local");
    assert_eq!(first["action"], "commented");
    assert!(queue(&mut db, "im.claim").is_null());
    comment(&mut db, &task["id"], "Local decision");
    apply(
        &mut db,
        json!({"op":"update","kind":"task","id":"local","fields":{"status":"done"}}),
    );
    let leased = queue(&mut db, "im.claim");
    assert_eq!(leased["thread"]["thread"], "1.000001");
    assert!(leased["text"].as_str().unwrap().contains("Local decision"));
    assert!(queue(&mut db, "im.claim").is_null());
    let mut invalid = ack(&leased, "sent");
    invalid["lease"] = json!("wrong");
    invalid["remote_id"] = json!("32.000001");
    assert!(db.execute("owner", invalid).is_err());
    apply(&mut db, ack(&leased, "uncertain"));
    assert!(queue(&mut db, "im.claim").is_null());
    apply(
        &mut db,
        json!({"op":"im.retry","id":"support","delivery_id":leased["delivery_id"],"decision":"confirm","remote_id":"32.000001"}),
    );
    let status = queue(&mut db, "im.claim");
    assert!(status["text"].as_str().unwrap().contains("[done]"));
    let mut echo = slack("bot", "33.000001", Some("1.000001"), "Generated");
    echo["confirm_slack_echo"] = json!(true);
    let e = &mut echo["payload"]["event"];
    e["bot_id"] = json!("B1");
    e["app_id"] = json!("A1");
    e["metadata"] = json!({"event_type":"shipforge_delivery","event_payload":{"delivery_id":status["delivery_id"]}});
    let mut wrong = echo.clone();
    wrong["payload"]["event"]["thread_ts"] = json!("3.000001");
    assert!(db.execute("owner", echo.clone()).is_err());
    let mut verified = |q: &Value| {
        db.confirm_slack_delivery("owner", "default", "support", &q["payload"])
            .unwrap()
    };
    assert_eq!(verified(&wrong)["action"], "ignored");
    assert_eq!(verified(&echo)["action"], "confirmed");
    assert_eq!(verified(&echo)["action"], "ignored");
    let mut final_ack = ack(&status, "sent");
    final_ack["remote_id"] = json!("33.000001");
    assert_eq!(apply(&mut db, final_ack)["state"], "sent");

    let private = comment(&mut db, &task["id"], "Private later");
    apply(
        &mut db,
        json!({"op":"update","kind":"comment","id":private["id"],"fields":{"visibility":"private"}}),
    );
    assert!(queue(&mut db, "im.claim").is_null());
    comment(&mut db, &task["id"], "Before unbinding");
    binding["op"] = json!("im.unbind");
    apply(&mut db, binding);
    assert!(queue(&mut db, "im.claim").is_null());
    assert_eq!(
        apply(
            &mut db,
            slack("E2", "2.000002", Some("1.000001"), "Unbound")
        )["action"],
        "ignored"
    );
    let mut new = slack("X", "3.000001", None, "Another root");
    new["op"] = json!("im.bind");
    new["task_id"] = task["id"].clone();
    apply(&mut db, new.clone());
    comment(&mut db, &task["id"], "Before task becomes private");
    apply(
        &mut db,
        json!({"op":"update","kind":"task","id":"local","fields":{"visibility":"private"}}),
    );
    assert!(queue(&mut db, "im.claim").is_null());
    assert!(db.execute("owner", new).is_err());
    assert!(
        db.execute("owner", slack("x", "3.000002", Some("3.000001"), "Hidden"))
            .is_err()
    );
}

#[test]
fn other_messengers_resolve_the_native_thread_and_keep_channels_apart() {
    let base: Vec<Value> = serde_json::from_str(include_str!("fixtures/inbound.json")).unwrap();
    let extended: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/inbound-extended.json")).unwrap();
    for f in base.into_iter().chain(extended).filter(|v| {
        matches!(
            v["provider"].as_str(),
            Some("telegram" | "feishu" | "teams" | "discord")
        )
    }) {
        let name = f["provider"].as_str().unwrap();
        let mut db = Store::memory().unwrap();
        connector(&mut db, "default", "c", name);
        let mut q = json!({"op":"provider.ingest","id":"c","provider":name,"payload":f["payload"]});
        let root = apply(&mut db, q.clone());
        for sequence in 1..=2 {
            let p = &mut q["payload"];
            match name {
                "telegram" => {
                    p["update_id"] = json!(904 + sequence);
                    p["message"]["message_id"] = json!(17 + sequence);
                    p["message"]["reply_to_message"] =
                        json!({"message_id":16+sequence,"chat":{"id":-123}});
                }
                "feishu" => {
                    p["header"]["event_id"] = json!(format!("ev{sequence}"));
                    p["event"]["message"]["message_id"] = json!(format!("f{sequence}"));
                    p["event"]["message"]["root_id"] = json!("om_example");
                }
                "teams" => {
                    p["id"] = json!(format!("m{sequence}"));
                    p["replyToId"] = if sequence == 1 {
                        json!("1767963600000")
                    } else {
                        json!("m1")
                    };
                }
                "discord" => {
                    p["d"]["type"] = json!(19);
                    p["d"]["id"] = json!(format!("91{sequence}"));
                    p["d"]["message_reference"] = json!({"message_id":if sequence==1{"900222"}else{"911"},"channel_id":"900111"});
                }
                _ => unreachable!(),
            }
            let reply = apply(&mut db, q.clone());
            assert_eq!(reply["action"], "commented", "{name}");
            assert_eq!(reply["task"], root["task"]);
            assert_eq!(apply(&mut db, q.clone()), reply);
        }
    }
}

#[test]
fn queued_delivery_survives_reopen_and_expired_lease_requires_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("workspace.sqlite3");
    let mut db = Store::open(&file).unwrap();
    connector(&mut db, "default", "support", "slack");
    let root = apply(&mut db, slack("E1", "1.000001", None, "Root"));
    comment(&mut db, &root["task"], "Survives restart");
    drop(db);
    let mut db = Store::open(&file).unwrap();
    let first = queue(&mut db, "im.claim");
    drop(db);
    // Simulate a terminated worker without sleeping for the 60-second lease.
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute(
            "UPDATE sf_im_outbox SET expires=0 WHERE id=?",
            [first["delivery_id"].as_str().unwrap()],
        )
        .unwrap();
    let mut db = Store::open(&file).unwrap();
    assert_eq!(queue(&mut db, "im.outbox")[0]["state"], "uncertain");
    assert!(queue(&mut db, "im.claim").is_null());
    apply(
        &mut db,
        json!({"op":"im.retry","id":"support","delivery_id":first["delivery_id"],"decision":"resend"}),
    );
    let second = queue(&mut db, "im.claim");
    assert_eq!(first["delivery_id"], second["delivery_id"]);
    assert_ne!(first["lease"], second["lease"]);
    let mut stale = ack(&first, "sent");
    stale["remote_id"] = json!("2.000001");
    assert!(db.execute("owner", stale).is_err());
    let mut limited = ack(&second, "retry");
    limited["retry_after"] = json!(60);
    assert_eq!(apply(&mut db, limited.clone())["state"], "pending");
    assert_eq!(apply(&mut db, limited)["state"], "pending");
    assert!(queue(&mut db, "im.claim").is_null());
}
