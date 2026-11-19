use serde_json::{Value, json};
use shipforge_core::Store;

fn call(s: &mut Store, actor: &str, q: Value) -> Value {
    s.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{actor} {q}: {e}"))
}
fn command(s: &mut Store, q: Value) -> Value {
    call(s, "owner", q)
}
fn event(id: &str, ts: &str, thread: Option<&str>, sender: &str, text: &str) -> Value {
    let mut data = json!({"type":"event_callback","team_id":"T1","api_app_id":"A1","event_id":id,"event":{"type":if thread.is_none(){"app_mention"}else{"message"},"channel":"C1","user":sender,"ts":ts,"text":text}});
    if let Some(thread) = thread {
        data["event"]["thread_ts"] = json!(thread);
    }
    json!({"op":"provider.ingest","provider":"slack","id":"channel","payload":data})
}
fn setup(s: &mut Store) -> Value {
    command(
        s,
        json!({"op":"create","kind":"member","id":"alice","title":"Alice","fields":{"role":"member"}}),
    );
    command(
        s,
        json!({"op":"create","kind":"connector","id":"channel","title":"Slack","fields":{"provider":"slack","enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1"}}),
    );
    command(
        s,
        json!({"op":"task_assistant.bind","provider":"slack","connector":"channel","runtime":"local-tool","sender_map":{"U1":"alice"},"expected_rev":0}),
    )
}
#[test]
fn dingtalk_stream_requires_bound_company_group_mention_and_member() {
    let mut s = Store::memory().unwrap();
    setup(&mut s);
    command(
        &mut s,
        json!({"op":"create","kind":"connector","id":"ding","title":"DingTalk","fields":{"provider":"dingtalk","enabled":true,"corp_id":"corp","chat_id":"group"}}),
    );
    let binding = command(
        &mut s,
        json!({"op":"task_assistant.bind","provider":"dingtalk","connector":"ding","runtime":"local-tool","sender_map":{"external-alice":"alice"},"expected_rev":0}),
    );
    let callback = |corp, kind, mention, sender| {
        let d = json!({"msgtype":"text","conversationType":kind,"senderId":sender,"chatbotUserId":"bot","chatbotCorpId":corp,"conversationId":"group","isInAtList":mention,"msgId":"event-1","createAt":1750720000000_i64,"text":{"content":"create a task for the report"}});
        json!({"op":"provider.ingest","id":"ding","provider":"dingtalk","payload":{"specVersion":"1.0","type":"CALLBACK","headers":{"topic":"/v1.0/im/bot/messages/get"},"data":d.to_string()}})
    };
    for q in [
        callback("corp", "1", true, "external-alice"),
        callback("corp", "2", false, "external-alice"),
        callback("corp", "2", true, "unknown"),
    ] {
        assert_eq!(command(&mut s, q)["action"], "ignored");
    }
    assert!(
        s.execute("owner", callback("other-corp", "2", true, "external-alice"))
            .is_err()
    );
    let q = callback("corp", "2", true, "external-alice");
    let accepted = command(&mut s, q.clone());
    assert_eq!(accepted["action"], "queued");
    assert_eq!(accepted["assistant"], binding["id"]);
    assert_eq!(command(&mut s, q)["run"], accepted["run"]);
}

const KEY: &str = "fixture-machine-key-for-external-tasks";
fn claim(s: &mut Store) -> Value {
    command(
        s,
        json!({"op":"run.claim","machine_id":"m","machine_key":KEY}),
    )
}
fn report(s: &mut Store, lease: &Value, draft: Value) -> Value {
    command(
        s,
        json!({"op":"run.report","id":lease["run"]["id"],"machine_id":"m","machine_key":KEY,"lease":lease["lease"],"status":"done","result":"done","task_draft":draft}),
    )
}
#[test]
fn localized_templates_overrides_slots_lifecycle_and_directory_permissions() {
    let mut s = Store::memory().unwrap();
    let initial = setup(&mut s);
    assert!(initial["id"].is_string());
    assert_eq!(initial["state"], "connected");
    let cat = call(&mut s, "alice", json!({"op":"task_assistant.catalog"}));
    assert_eq!(cat.as_array().unwrap().len(), 5);
    assert!(
        cat.as_array()
            .unwrap()
            .iter()
            .all(|r| r["can_manage"] == false)
    );
    assert!(
        s.execute("alice", json!({"op":"task_assistant.template"}))
            .is_err()
    );
    for q in [
        json!({"op":"get","kind":"agent","id":initial["id"]}),
        json!({"op":"chat.start","mode":"agent","target":initial["id"]}),
        json!({"op":"run.enqueue","handler_kind":"im_assistant","handler_id":initial["id"],"instruction":"no"}),
    ] {
        assert!(s.execute("owner", q).is_err());
    }
    assert_eq!(
        command(&mut s, json!({"op":"list","kind":"agent"})),
        json!([])
    );
    let en = command(
        &mut s,
        json!({"op":"task_assistant.template.save","expected_rev":0,"language":"en","instructions":"{{productName}} {{imName}} {{cliName}} {{spaceName}}"}),
    );
    assert_eq!(en["rev"], 1);
    let cfg = command(
        &mut s,
        json!({"op":"task_assistant.settings","provider":"slack","language":"en"}),
    );
    let txt = cfg["effective"]["instructions"].as_str().unwrap();
    assert!(txt.contains("ShipForge Slack shipforge"));
    assert!(!txt.contains("{{"));
    let own = command(
        &mut s,
        json!({"op":"task_assistant.configure","expected_rev":initial["rev"],"provider":"slack","language":"en","instructions":"My local prompt"}),
    );
    assert_eq!(own["effective"]["overridden"]["instructions"], true);
    command(
        &mut s,
        json!({"op":"task_assistant.template.save","expected_rev":1,"language":"en","instructions":"updated"}),
    );
    assert_eq!(
        command(
            &mut s,
            json!({"op":"task_assistant.settings","provider":"slack","language":"en"})
        )["effective"]["instructions"],
        "My local prompt"
    );
    let restore = command(
        &mut s,
        json!({"op":"task_assistant.configure","expected_rev":own["rev"],"provider":"slack","language":"en","instructions":null}),
    );
    assert_eq!(restore["effective"]["instructions"], "updated");
    let off = command(
        &mut s,
        json!({"op":"task_assistant.unbind","expected_rev":restore["rev"],"provider":"slack"}),
    );
    assert_eq!(
        command(
            &mut s,
            event("E1", "1.100001", None, "U1", "create an issue")
        )["action"],
        "ignored"
    );
    let resumed = command(
        &mut s,
        json!({"op":"task_assistant.bind","expected_rev":off["rev"],"provider":"slack","connector":"channel","runtime":"local-tool"}),
    );
    assert_eq!(resumed["id"], initial["id"]);
    command(
        &mut s,
        json!({"op":"task_assistant.remove","provider":"slack","expected_rev":resumed["rev"]}),
    );
    let next = command(
        &mut s,
        json!({"op":"task_assistant.bind","expected_rev":0,"provider":"slack","connector":"channel","runtime":"local-tool","sender_map":{"U1":"alice"}}),
    );
    assert_ne!(next["id"], initial["id"]);
}
#[test]
fn explicit_signed_route_runs_with_fresh_rights_and_returns_only_scoped_task_data() {
    let mut s = Store::memory().unwrap();
    let bot = setup(&mut s);
    command(
        &mut s,
        json!({"op":"machine.register","machine_id":"m","machine_key":KEY,"runtimes":["local-tool"],"title":"Fixture"}),
    );
    for e in [
        event("bad", "1.000001", None, "U-unmapped", "create work"),
        event("no", "1.000002", None, "U1", "do not create a task"),
    ] {
        assert_eq!(command(&mut s, e)["action"], "ignored");
    }
    let req = event(
        "create",
        "2.000001",
        None,
        "U1",
        "<@Ubot> create a task for the parser",
    );
    let sent = command(&mut s, req.clone());
    assert_eq!(command(&mut s, req), sent);
    assert_eq!(sent["action"], "queued");
    assert!(
        s.execute(
            "owner",
            event("create", "2.000009", None, "U1", "create another")
        )
        .is_err()
    );
    let lease = claim(&mut s);
    assert_eq!(lease["run"]["fields"]["handler_kind"], "im_assistant");
    let result = report(
        &mut s,
        &lease,
        json!({"action":"create","title":"Parser check","description":"test fixture"}),
    );
    let task = result["fields"]["assistant_action"]["task"].clone();
    assert!(task.is_string(), "{result}");
    let saved = call(&mut s, "alice", json!({"op":"get","kind":"task","id":task}));
    assert_eq!(
        saved["fields"]["created_by_assistant"]["assistant"],
        bot["id"]
    );
    let ack = command(&mut s, json!({"op":"im.claim","id":"channel"}));
    assert_eq!(ack["thread"]["thread"], "2.000001");
    assert!(ack["text"].as_str().unwrap().contains("Parser check"));
    command(
        &mut s,
        event("show", "2.000002", Some("2.000001"), "U1", "show task"),
    );
    let l = claim(&mut s);
    assert_eq!(l["run"]["fields"]["external_task"]["id"], task);
    assert!(l["run"]["fields"]["external_thread"][0]["body"].is_string());
    let done = report(&mut s, &l, json!({"action":"show"}));
    assert_eq!(done["fields"]["assistant_action"]["action"], "show");
    command(
        &mut s,
        event(
            "change",
            "2.000003",
            Some("2.000001"),
            "U1",
            "update this task",
        ),
    );
    let l = claim(&mut s);
    let done = report(
        &mut s,
        &l,
        json!({"action":"update","title":"Updated by Alice"}),
    );
    assert_eq!(done["fields"]["assistant_action"]["action"], "update");
    assert_eq!(
        command(
            &mut s,
            event("wrong", "2.000004", Some("2.000001"), "U1", "update SF-999")
        )["action"],
        "ignored"
    );
    command(
        &mut s,
        event("race", "2.000005", Some("2.000001"), "U1", "update task"),
    );
    let l = claim(&mut s);
    command(
        &mut s,
        json!({"op":"update","kind":"task","id":task,"title":"Human edit"}),
    );
    assert_eq!(
        report(&mut s, &l, json!({"action":"update","title":"stale"}))["fields"]["status"],
        "failed"
    );
    command(
        &mut s,
        event("revoke", "2.000006", Some("2.000001"), "U1", "update task"),
    );
    let l = claim(&mut s);
    command(
        &mut s,
        json!({"op":"task_assistant.unbind","expected_rev":bot["rev"],"provider":"slack"}),
    );
    assert_eq!(
        report(&mut s, &l, json!({"action":"update","title":"Forbidden"}))["fields"]["status"],
        "cancelled"
    );
    assert_eq!(
        command(&mut s, json!({"op":"get","kind":"task","id":task}))["title"],
        "Human edit"
    );
}

#[test]
fn feishu_and_lark_keep_separate_cloud_scope_and_preserve_code_blocks() {
    let mut s = Store::memory().unwrap();
    command(
        &mut s,
        json!({"op":"create","kind":"member","id":"alice","title":"Alice","fields":{"role":"member"}}),
    );
    let mut ids = vec![];
    for cloud in ["feishu", "lark"] {
        command(
            &mut s,
            json!({"op":"create","kind":"connector","id":cloud,"title":cloud,"fields":{"enabled":true,"provider":cloud,"tenant_key":cloud,"chat_id":"chat","bot_open_id":"bot"}}),
        );
        ids.push(command(&mut s,json!({"op":"task_assistant.bind","provider":cloud,"connector":cloud,"expected_rev":0,"runtime":"local-tool","sender_map":{"ou_alice":"alice"}}))["id"].clone());
        let rich = json!({"en_us":{"content":[[{"tag":"at","user_id":"bot"},{"tag":"text","text":"create a task for this code"}],[{"tag":"code_block","language":"rust","text":"check();"}]]}});
        let mut event = json!({"op":"provider.ingest","id":cloud,"provider":cloud,"payload":{"schema":"2.0","header":{"event_type":"im.message.receive_v1","tenant_key":cloud,"event_id":"evt"},"event":{"sender":{"sender_type":"user","sender_id":{"open_id":"ou_alice"}},"message":{"message_id":"message","chat_type":"group","mentions":[{"id":{"open_id":"bot"}}],"chat_id":"chat","message_type":"post","create_time":"1750720000000","content":rich.to_string()}}}});
        if cloud == "lark" {
            event["payload"]["header"]["tenant_key"] = json!("feishu");
            assert_eq!(command(&mut s, event.clone())["action"], "ignored");
            event["payload"]["header"]["tenant_key"] = json!("lark");
        }
        let accepted = command(&mut s, event);
        let run = command(
            &mut s,
            json!({"op":"get","kind":"run","id":accepted["run"]}),
        );
        assert!(
            run["fields"]["external_body"]
                .as_str()
                .unwrap()
                .contains("~~~rust\ncheck();\n~~~")
        );
    }
    assert_ne!(ids[0], ids[1]);
}
