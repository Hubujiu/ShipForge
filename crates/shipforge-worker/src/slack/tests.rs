use super::*;
use std::thread;
use tiny_http::{Header, Response as Reply, Server};

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
