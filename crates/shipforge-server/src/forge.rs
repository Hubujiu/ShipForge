use crate::{Auth, Store};
use ring::hmac;
use serde_json::{Value, json};
use std::io::Read;
use tiny_http::Request;

pub(super) fn oauth_landing(request: Request) {
    let text = "ShipForge 已收到浏览器回调。请复制当前地址栏的完整网址，返回 ShipForge 的“浏览器授权”窗口并完成授权。";
    let res = tiny_http::Response::from_string(text)
        .with_header(
            tiny_http::Header::from_bytes("Content-Type", "text/plain; charset=utf-8").unwrap(),
        )
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(tiny_http::Header::from_bytes("Referrer-Policy", "no-referrer").unwrap());
    let _ = request.respond(res);
}
pub(super) fn handle(
    path: &str,
    request: &mut Request,
    auth: &Auth,
    store: &mut Store,
) -> Result<Value, String> {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_owned())
            .unwrap_or_default()
    };
    let gh = path == "github";
    let pair = if gh {
        ""
    } else {
        path.strip_prefix("gitlab/")
            .filter(|x| uuid::Uuid::parse_str(x).is_ok())
            .ok_or("not_found: project webhook")?
    };
    let signature = header("X-Hub-Signature-256");
    let secret = header("X-Gitlab-Token");
    let mut event = header("X-GitHub-Event");
    let delivery = if gh {
        header("X-GitHub-Delivery")
    } else {
        let id = header("X-Gitlab-Event-UUID");
        if id.is_empty() {
            header("Idempotency-Key")
        } else {
            id
        }
    };
    let mut raw = Vec::new();
    request
        .as_reader()
        .take(8_388_609)
        .read_to_end(&mut raw)
        .map_err(|_| "invalid: unreadable repository event")?;
    if raw.len() > 8_388_608 {
        return Err("too_large: repository event exceeds 8 MiB".into());
    }
    if gh {
        let key = std::env::var("SHIPFORGE_GITHUB_WEBHOOK_SECRET")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or("unauthorized: App webhook unconfigured")?;
        let hex = signature
            .strip_prefix("sha256=")
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("unauthorized: invalid GitHub signature")?;
        let bytes: Vec<u8> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| {
                u8::from_str_radix(std::str::from_utf8(s).unwrap_or(""), 16).unwrap_or_default()
            })
            .collect();
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
            &raw,
            &bytes,
        )
        .map_err(|_| "unauthorized: invalid GitHub signature")?;
    } else if secret.is_empty() {
        return Err("unauthorized: missing GitLab project token".into());
    }
    let body: Value =
        serde_json::from_slice(&raw).map_err(|_| "invalid: malformed repository event")?;
    if gh {
        let shape = match event.as_str() {
            "ping" => return Ok(json!({"ok":true})),
            "installation" => {
                body["installation"].is_object()
                    && body["repository"].is_null()
                    && body["sender"].is_object()
                    && body["repositories_removed"].is_null()
                    && body["repositories_added"].is_null()
            }
            "installation_repositories" => {
                body["installation"].is_object()
                    && (body["repositories_removed"].is_array()
                        || body["repositories_added"].is_array())
            }
            "issues" => body["issue"].is_object() && body["comment"].is_null(),
            "issue_comment" => body["issue"].is_object() && body["comment"].is_object(),
            "pull_request" => body["pull_request"].is_object(),
            "push" => body["commits"].is_array(),
            _ => return Ok(json!({"ignored":true})),
        };
        if !shape {
            return Err("invalid: GitHub event does not match signed payload".into());
        }
        if matches!(event.as_str(), "installation" | "installation_repositories") {
            store.github_installation_notice(&event, &body)?;
            return Ok(json!({"ok":true}));
        }
    } else {
        event = body["object_kind"].as_str().unwrap_or("unknown").to_owned();
    }
    let targets = store.route_forge_hook(
        if gh { "github" } else { "gitlab" },
        pair,
        &secret,
        &event,
        &delivery,
        &body,
    )?;
    for row in targets {
        shipforge_client::verify_forge_hook(auth.data_directory()?, &row)?;
        store.execute(row["actor"].as_str().unwrap_or(""),json!({"op":"forge.internal","id":row["platform"],"space":row["space"],"pair":row["id"],"action":"event","event":event,"event_id":delivery,"payload":body}))?;
        // An upstream outage leaves a durable delivery available for a later retry.
        let _ = shipforge_client::execute_integration(
            auth.data_directory()?,
            row["actor"].as_str().unwrap_or(""),
            json!({"op":"forge.deliver","space":row["space"],"id":row["platform"],"pair":row["id"]}),
        );
    }
    Ok(json!({"ok":true}))
}
