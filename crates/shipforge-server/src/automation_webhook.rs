use serde_json::{Value, json};
use shipforge_core::{Store, WebhookReply};
use std::io::Read;
use tiny_http::{Header, Method, Request, Response, StatusCode};
const PREFIX: &str = "/v1/automation-webhooks/";
pub fn matches(request: &Request) -> bool {
    request.url().starts_with(PREFIX)
}
fn header(request: &Request, name: &'static str) -> Option<String> {
    let mut headers = request.headers().iter().filter(|h| h.field.equiv(name));
    let first = headers.next()?.value.as_str().to_owned();
    Some(if headers.next().is_none() {
        first
    } else {
        String::new()
    })
}
fn handle(request: &mut Request, store: &mut Store) -> Result<WebhookReply, String> {
    if request.method() != &Method::Post {
        return Ok(WebhookReply {
            status: 405,
            body: json!({"success":false,"error":{"code":"method"}}),
            retry_after: None,
        });
    }
    let token = request.url().strip_prefix(PREFIX).unwrap_or("").to_owned();
    let ip = request
        .remote_addr()
        .ok_or("server: connection IP unavailable")?
        .ip();
    if let Some(reply) = store.automation_hook_preflight(&token, ip)? {
        return Ok(reply);
    }
    let sig = header(request, "X-Webhook-Signature");
    let ts = header(request, "X-Webhook-Timestamp");
    let github = header(request, "X-Hub-Signature-256");
    const LIMIT: usize = 131072;
    let mut bytes = Vec::new();
    if request.body_length().is_some_and(|n| n > LIMIT) {
        bytes.resize(LIMIT + 1, 0);
    } else {
        request
            .as_reader()
            .take(LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "invalid: incomplete webhook body")?;
    }
    store.automation_hook_receive(
        &token,
        ip,
        &bytes,
        sig.as_deref(),
        ts.as_deref(),
        github.as_deref(),
    )
}
pub fn serve(mut request: Request, store: &mut Store) {
    let reply = handle(&mut request, store).unwrap_or_else(|_| WebhookReply {
        status: 500,
        body: json!({"success":false,"error":{"code":"server"}}),
        retry_after: None,
    });
    let mut response = Response::from_string(reply.body.to_string())
        .with_status_code(StatusCode(reply.status))
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("Referrer-Policy", "no-referrer").unwrap());
    if let Some(seconds) = reply.retry_after {
        response.add_header(Header::from_bytes("Retry-After", seconds.to_string()).unwrap());
    }
    let _ = request.respond(response);
}
pub(crate) fn absolute(data: &mut Value, origin: &url::Url) {
    fn set(v: &mut Value, origin: &url::Url) {
        if let Some(path) = v["path"].as_str().filter(|p| p.starts_with(PREFIX))
            && let Ok(url) = origin.join(path.trim_start_matches('/'))
        {
            v["url"] = json!(url.as_str());
        }
    }
    set(data, origin);
    if let Some(rows) = data.get_mut("issued").and_then(Value::as_array_mut) {
        for row in rows {
            set(row, origin);
        }
    }
}
