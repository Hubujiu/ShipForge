use crate::auth::{self, Auth};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use shipforge_core::Store;
use std::io::Read;
use tiny_http::{Header, Method, Request, Response, StatusCode};
use url::{Host, Url};

pub fn origin(listen: &str) -> Result<Url, String> {
    let base = std::env::var("SHIPFORGE_PUBLIC_URL").unwrap_or_else(|_| format!("http://{listen}"));
    let mut url = Url::parse(&base).map_err(|_| "invalid: invalid SHIPFORGE_PUBLIC_URL")?;
    let local = matches!(url.host(),Some(Host::Ipv4(ip)) if ip.is_loopback()||ip.is_unspecified())
        || matches!(url.host(),Some(Host::Ipv6(ip)) if ip.is_loopback()||ip.is_unspecified())
        || url.host_str() == Some("localhost");
    if url.host().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "invalid: SHIPFORGE_PUBLIC_URL must be HTTPS or a loopback HTTP address".into(),
        );
    }
    let base_path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&base_path);
    Ok(url)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn answer(
    req: &mut Request,
    auth: &Auth,
    store: &mut Store,
    origin: &Url,
) -> Result<(String, &'static str), String> {
    let parsed = Url::parse(&format!("http://localhost{}", req.url()))
        .map_err(|_| "invalid: malformed callback route")?;
    let parts = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "bugly", "submit", route] if req.method() == &Method::Post => {
            if matches!(origin.host(),Some(Host::Ipv4(ip)) if ip.is_unspecified())
                || matches!(origin.host(),Some(Host::Ipv6(ip)) if ip.is_unspecified())
            {
                return Err(
                    "unavailable: configure SHIPFORGE_PUBLIC_URL for public task links".into(),
                );
            }
            let mut keys = parsed.query_pairs().filter(|(k, _)| k == "key");
            let key = keys
                .next()
                .ok_or("unauthorized: callback credential required")?
                .1;
            if keys.next().is_some() {
                return Err("unauthorized: duplicate credential".into());
            }
            let mut bytes = Vec::new();
            req.as_reader()
                .take(1_100_001)
                .read_to_end(&mut bytes)
                .map_err(|_| "invalid: unreadable body")?;
            if bytes.len() > 1_100_000 {
                return Err("too_large: Bugly payload too large".into());
            }
            let payload: Value =
                serde_json::from_slice(&bytes).map_err(|_| "invalid: UTF-8 Bugly JSON required")?;
            let result = store.bugly_submission(route, &key, &payload)?;
            let mut url = origin.clone();
            url.path_segments_mut()
                .map_err(|_| "invalid: unusable public URL")?
                .pop_if_empty()
                .extend([
                    "v1",
                    "bugly",
                    "task",
                    result["space"].as_str().unwrap(),
                    result["task"].as_str().unwrap(),
                ]);
            Ok((
                json!({"code":0,"message":"success","url":url.as_str()}).to_string(),
                "application/json; charset=utf-8",
            ))
        }
        ["v1", "bugly", "task", _, _] if req.method() == &Method::Get => {
            let authz = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.as_str())
                .ok_or("unauthorized: user token required")?;
            let token = if let Some(v) = authz.strip_prefix("Bearer ") {
                v.to_owned()
            } else {
                let encoded = authz
                    .strip_prefix("Basic ")
                    .ok_or("unauthorized: user token required")?;
                let plain = String::from_utf8(
                    STANDARD
                        .decode(encoded)
                        .map_err(|_| "unauthorized: invalid credential")?,
                )
                .map_err(|_| "unauthorized: invalid credential")?;
                plain
                    .split_once(':')
                    .ok_or("unauthorized: invalid credential")?
                    .1
                    .to_owned()
            };
            let p = auth.authenticate(&token)?;
            let path = parsed.path().trim_start_matches("/v1/bugly/task/");
            let (space, task) = path.split_once('/').ok_or("not_found: task")?;
            let decode = |s: &str| -> Result<String, String> {
                // Query form decoding preserves literal '+' when it is escaped first.
                let encoded = format!("v={}", s.replace('+', "%2B").replace('&', "%26"));
                Ok(url::form_urlencoded::parse(encoded.as_bytes())
                    .next()
                    .ok_or("invalid: bad path")?
                    .1
                    .into_owned())
            };
            let q = json!({"op":"get","kind":"task","id":decode(task)?,"space":decode(space)?});
            auth::authorize(&p, &q)?;
            let t = store.execute(&p.actor, q)?;
            let title = esc(t["title"].as_str().unwrap_or(""));
            let number = esc(t["fields"]["number"].as_str().unwrap_or(""));
            let status = esc(t["fields"]["status"].as_str().unwrap_or(""));
            let description = esc(t["fields"]["description"].as_str().unwrap_or(""));
            Ok((
                format!(
                    "<!doctype html><html><meta charset=utf-8><title>{number} · {title}</title><main><p>{number} · {status}</p><h1>{title}</h1><pre>{description}</pre></main></html>"
                ),
                "text/html; charset=utf-8",
            ))
        }
        ["v1", "bugly", "submit", _] | ["v1", "bugly", "task", _, _] => {
            Err("method: unsupported request method".into())
        }
        _ => Err("not_found: unknown Bugly route".into()),
    }
}

pub fn respond(mut request: Request, auth: &Auth, store: &mut Store, origin: &Url) {
    let task = request.url().starts_with("/v1/bugly/task/");
    let (status, body, mime) = match answer(&mut request, auth, store, origin) {
        Ok((body, mime)) => (200, body, mime),
        Err(error) => {
            let (code, message) = error.split_once(": ").unwrap_or(("invalid", &error));
            let n = match code {
                "unauthorized" => 401,
                "forbidden" => 403,
                "not_found" => 404,
                "method" => 405,
                "conflict" => 409,
                "too_large" => 413,
                "unavailable" => 503,
                "storage" => 500,
                _ => 400,
            };
            let safe = if n == 500 { "request failed" } else { message };
            (
                n,
                json!({"code":n,"message":safe}).to_string(),
                "application/json; charset=utf-8",
            )
        }
    };
    let mut response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", mime).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("Referrer-Policy", "no-referrer").unwrap())
        .with_header(
            Header::from_bytes(
                "Content-Security-Policy",
                "default-src 'none'; frame-ancestors 'none'",
            )
            .unwrap(),
        )
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
    if task && status == 401 {
        response.add_header(
            Header::from_bytes(
                "WWW-Authenticate",
                r#"Basic realm="ShipForge user token", charset="UTF-8""#,
            )
            .unwrap(),
        );
    }
    let _ = request.respond(response);
}
