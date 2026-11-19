use super::*;
use std::thread;
use tiny_http::{Header, Response as Reply, Server};

#[test]
fn posts_assignment_card_for_a_new_bound_thread_when_enabled() {
    let mut client = Client::memory().unwrap();
    client.execute(json!({"op":"create","kind":"connector","id":"cards","title":"QA cards",
        "fields":{"provider":"slack","enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1","slack_assignment":true,"assignee_map":{"U1":"owner"}}})).unwrap();
    let event = json!({"op":"provider.ingest","id":"cards","provider":"slack","payload":{"type":"event_callback","team_id":"T1","event_id":"E1","event":{"type":"message","channel":"C1","user":"U1","ts":"1.000001","text":"Review the draft"}}});
    client.execute(event.clone()).unwrap();
    client.execute(event).unwrap();
    let (url, worker) = mock(vec![
        identity(),
        (
            "chat.postMessage",
            200,
            json!({"ok":true,"channel":"C1","ts":"2.000003"}),
        ),
    ]);
    let result = batch(
        &mut client,
        "default",
        "cards",
        &Slack::new(&url, "synthetic-token"),
    )
    .unwrap();
    let requests = worker.join().unwrap();
    let message = &requests[1];
    assert_eq!(message["thread_ts"], "1.000001");
    assert_eq!(message["blocks"][0]["text"]["type"], "plain_text");
    assert!(
        message["blocks"][0]["text"]["text"]
            .as_str()
            .unwrap()
            .contains("Review the draft")
    );
    let block = &message["blocks"][1];
    assert_eq!(
        block["block_id"],
        format!(
            "shipforge-assignee-{}",
            result["deliveries"][0]["delivery_id"].as_str().unwrap()
        )
    );
    assert_eq!(block["elements"][0]["type"], "users_select");
    assert_eq!(block["elements"][0]["action_id"], "shipforge.assign");
}

// The adapter's production URL is fixed. Only these tests inject a loopback URL.
fn mock(replies: Vec<(&'static str, u16, Value)>) -> (String, thread::JoinHandle<Vec<Value>>) {
    let http = Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", http.server_addr());
    let worker =
        thread::spawn(move || {
            let mut observed = Vec::new();
            for (method, status, body) in replies {
                let mut req = http
                    .recv_timeout(Duration::from_secs(8))
                    .unwrap()
                    .expect("missing request to local Slack fixture");
                assert_eq!(req.method().as_str(), "POST");
                assert_eq!(req.url(), format!("/{method}"));
                assert!(req.headers().iter().any(|h| h.field.equiv("Authorization")
                    && h.value.as_str() == "Bearer synthetic-token"));
                let data: Value = serde_json::from_reader(req.as_reader()).unwrap();
                observed.push(data);
                let mut reply = Reply::from_string(body.to_string())
                    .with_status_code(status)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                if status == 429 {
                    reply = reply.with_header(Header::from_bytes("Retry-After", "23").unwrap());
                }
                req.respond(reply).unwrap();
            }
            observed
        });
    (url, worker)
}
fn workspace() -> Client {
    let mut client = Client::memory().unwrap();
    client.execute(json!({"op":"create","kind":"connector","id":"support","title":"QA","fields":{"provider":"slack","enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1"}})).unwrap();
    let root=client.execute(json!({"op":"provider.ingest","id":"support","provider":"slack","payload":{"type":"event_callback","team_id":"T1","event_id":"E1","event":{"type":"message","channel":"C1","user":"U2","ts":"1.000001","text":"Root"}}})).unwrap();
    let threads = client
        .execute(json!({"op":"im.threads","task_id":root["task"]}))
        .unwrap();
    client.execute(json!({"op":"im.reply","id":"support","task_id":root["task"],"thread":threads[0]["thread"],"body":"A local reply","request_id":"r1"})).unwrap();
    client
}
fn identity() -> (&'static str, u16, Value) {
    (
        "auth.test",
        200,
        json!({"ok":true,"team_id":"T1","bot_id":"B1"}),
    )
}
#[test]
fn authorized_worker_sends_one_plain_thread_reply_and_saves_the_receipt() {
    let mut client = workspace();
    let (url, worker) = mock(vec![
        identity(),
        (
            "chat.postMessage",
            200,
            json!({"ok":true,"channel":"C1","ts":"2.000002"}),
        ),
    ]);
    let result = batch(
        &mut client,
        "default",
        "support",
        &Slack::new(&url, "synthetic-token"),
    )
    .unwrap();
    let observed = worker.join().unwrap();
    let data = &observed[1];
    assert_eq!(data["channel"], "C1");
    assert_eq!(data["thread_ts"], "1.000001");
    assert!(data["text"].as_str().unwrap().contains("A local reply"));
    assert_eq!(data["mrkdwn"], false);
    assert_eq!(data["unfurl_media"], false);
    assert_eq!(
        data["metadata"]["event_payload"]["delivery_id"],
        result["deliveries"][0]["delivery_id"]
    );
    assert_eq!(
        data["metadata"]["event_payload"]["thread_ts"],
        data["thread_ts"]
    );
    let queue = client
        .execute(json!({"op":"im.outbox","id":"support"}))
        .unwrap();
    assert_eq!(queue[0]["state"], "sent");
    assert_eq!(queue[0]["remote_id"], "2.000002");
    let (url, second) = mock(vec![]);
    assert_eq!(
        batch(
            &mut client,
            "default",
            "support",
            &Slack::new(&url, "synthetic-token")
        )
        .unwrap()["deliveries"],
        json!([])
    );
    second.join().unwrap();
}

#[test]
fn external_space_assistant_posts_its_own_scoped_task_receipt_to_slack() {
    let mut client = Client::memory().unwrap();
    let mut command = |q: Value| client.execute(q).unwrap();
    command(
        json!({"op":"create","kind":"member","id":"alice","title":"Alice","fields":{"role":"member"}}),
    );
    command(
        json!({"op":"create","kind":"connector","id":"assistant","title":"External task intake","fields":{"provider":"slack","enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1"}}),
    );
    command(
        json!({"op":"task_assistant.bind","provider":"slack","connector":"assistant","runtime":"local-tool","sender_map":{"U1":"alice"},"expected_rev":0}),
    );
    let work = command(
        json!({"op":"provider.ingest","id":"assistant","provider":"slack","payload":{"type":"event_callback","team_id":"T1","api_app_id":"A1","event_id":"E1","event":{"type":"app_mention","user":"U1","channel":"C1","ts":"7.000001","text":"create a task for the meeting"}}}),
    );
    let key = "synthetic-key-for-space-assistant-worker";
    command(
        json!({"op":"machine.register","machine_id":"fixture","machine_key":key,"title":"Fixture","runtimes":["local-tool"]}),
    );
    let run = command(json!({"op":"run.claim","machine_id":"fixture","machine_key":key}));
    assert_eq!(work["run"], run["run"]["id"]);
    command(
        json!({"op":"run.report","id":work["run"],"lease":run["lease"],"machine_id":"fixture","machine_key":key,"status":"done","result":"Created","task_draft":{"action":"create","title":"Meeting follow-up","description":"Fixture"}}),
    );
    let (url, server) = mock(vec![
        identity(),
        (
            "chat.postMessage",
            200,
            json!({"ok":true,"channel":"C1","ts":"7.000002"}),
        ),
    ]);
    batch(
        &mut client,
        "default",
        "assistant",
        &Slack::new(&url, "synthetic-token"),
    )
    .unwrap();
    let posts = server.join().unwrap();
    assert_eq!(posts[1]["thread_ts"], "7.000001");
    assert_eq!(posts[1]["channel"], "C1");
    assert!(
        posts[1]["text"]
            .as_str()
            .unwrap()
            .contains("Meeting follow-up")
    );
    let sent = client
        .execute(json!({"op":"im.outbox","id":"assistant"}))
        .unwrap();
    assert_eq!(sent[0]["state"], "sent");
    assert_eq!(sent[0]["remote_id"], "7.000002");
}
#[test]
fn wrong_bot_does_not_claim_and_ambiguous_posts_are_not_repeated() {
    let mut client = workspace();
    let (url, wrong) = mock(vec![(
        "auth.test",
        200,
        json!({"ok":true,"team_id":"T2","bot_id":"B1"}),
    )]);
    assert!(
        batch(
            &mut client,
            "default",
            "support",
            &Slack::new(&url, "synthetic-token")
        )
        .is_err()
    );
    wrong.join().unwrap();
    let pending = client
        .execute(json!({"op":"im.outbox","id":"support"}))
        .unwrap();
    assert_eq!(pending[0]["state"], "pending");
    assert_eq!(pending[0]["attempts"], 0);
    let (url, external) = mock(vec![identity(), ("chat.postMessage", 503, json!({}))]);
    assert_eq!(
        batch(
            &mut client,
            "default",
            "support",
            &Slack::new(&url, "synthetic-token")
        )
        .unwrap()["deliveries"][0]["outcome"],
        "uncertain"
    );
    external.join().unwrap();
    assert_eq!(
        client
            .execute(json!({"op":"im.claim","id":"support"}))
            .unwrap(),
        Value::Null
    );
}
#[test]
fn rate_limit_keeps_remote_deadline_and_permanent_rejection_is_visible() {
    for (status, response, state) in [
        (429, json!({}), "pending"),
        (200, json!({"ok":false,"error":"missing_scope"}), "failed"),
    ] {
        let mut client = workspace();
        let (url, external) = mock(vec![identity(), ("chat.postMessage", status, response)]);
        batch(
            &mut client,
            "default",
            "support",
            &Slack::new(&url, "synthetic-token"),
        )
        .unwrap();
        external.join().unwrap();
        let queue = client
            .execute(json!({"op":"im.outbox","id":"support"}))
            .unwrap();
        assert_eq!(queue[0]["state"], state);
        assert_eq!(queue[0]["attempts"], 1);
        if status == 429 {
            let at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            assert!(queue[0]["available_at"].as_i64().unwrap() - at > 20_000);
        } else {
            assert_eq!(queue[0]["error"], "missing_scope");
        }
        assert!(
            client
                .execute(json!({"op":"im.claim","id":"support"}))
                .unwrap()
                .is_null()
        );
    }
}
