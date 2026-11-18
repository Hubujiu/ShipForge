use crate::auth::Auth;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use shipforge_core::Store;
use std::{
    collections::HashMap,
    io::Read,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Request, Response, StatusCode};
use url::Url;

type Result<T> = std::result::Result<T, String>;
pub(super) struct Site {
    organization: Option<String>,
    base: String,
    rate: Mutex<HashMap<IpAddr, (Instant, u16)>>,
}
pub(super) fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_owned())
}
impl Site {
    pub(super) fn new(
        organization: Option<String>,
        base: Option<String>,
        listen: &str,
    ) -> Result<Self> {
        if organization.as_deref().is_some_and(|s| {
            s.trim().is_empty()
                || s.len() > 160
                || s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
        }) {
            return Err("invalid Jira organization".into());
        }
        let base = base.unwrap_or_else(|| format!("http://{listen}"));
        let url = Url::parse(&base).map_err(|_| "invalid Jira base URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "expected an HTTP(S) Jira base URL without credentials, query or fragment".into(),
            );
        }
        Ok(Self {
            organization,
            base: base.trim_end_matches('/').into(),
            rate: Mutex::new(HashMap::new()),
        })
    }
    fn limit(&self, source: IpAddr) -> Result<()> {
        let mut rates = self
            .rate
            .lock()
            .map_err(|_| "server: rate limiter unavailable")?;
        let now = Instant::now();
        let window = Duration::from_secs(60);
        if rates.len() >= 4096 {
            rates.retain(|_, (at, _)| now.duration_since(*at) < window);
            if rates.len() >= 4096 && !rates.contains_key(&source) {
                return Err("rate: too many Jira requests".into());
            }
        }
        let entry = rates.entry(source).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if entry.1 >= 600 {
            return Err("rate: too many Jira requests".into());
        }
        entry.1 += 1;
        Ok(())
    }
    fn handle(&self, req: &mut Request, auth: &Auth, store: &mut Store) -> Result<(u16, Value)> {
        let org = self
            .organization
            .as_deref()
            .ok_or("not_found: Jira site is unavailable")?;
        if let Some(addr) = req.remote_addr() {
            self.limit(addr.ip())?;
        }
        let header = header(req, "Authorization")
            .ok_or("unauthorized: a personal access token is required")?;
        let (scheme, credential) = header
            .split_once(' ')
            .ok_or("unauthorized: invalid authorization header")?;
        let principal = if scheme.eq_ignore_ascii_case("Bearer") {
            auth.authenticate(credential)?
        } else if scheme.eq_ignore_ascii_case("Basic") && credential.len() <= 1200 {
            let words = String::from_utf8(
                STANDARD
                    .decode(credential)
                    .map_err(|_| "unauthorized: invalid Basic credentials")?,
            )
            .map_err(|_| "unauthorized: invalid Basic credentials")?;
            let (actor, token) = words
                .split_once(':')
                .ok_or("unauthorized: a personal access token is required")?;
            let identity = auth.authenticate(token)?;
            if actor != identity.actor {
                return Err("unauthorized: token does not belong to this user".into());
            }
            identity
        } else {
            return Err("unauthorized: a personal access token is required".into());
        };
        if !matches!(principal.scope.as_str(), "read" | "write") {
            return Err("forbidden: Jira requires a personal token".into());
        }
        let verb = req.method().clone();
        if !matches!(
            verb,
            Method::Get | Method::Post | Method::Put | Method::Delete
        ) {
            return Err("method: unsupported HTTP method".into());
        }
        let target = req.url().strip_prefix("/rest/api/").unwrap();
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if path.len() + query.len() > 16384 {
            return Err("invalid: query is too long".into());
        }
        let (version, route) = path
            .split_once('/')
            .ok_or("not_found: Jira route is unavailable")?;
        let version = match version {
            "2" => 2,
            "3" => 3,
            _ => return Err("not_found: Jira REST version is unavailable".into()),
        };
        let route = route
            .split('/')
            .map(decode_segment)
            .collect::<Result<Vec<_>>>()?
            .join("/");
        let mut params = Map::new();
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if let Some(old) = params.insert(key.to_string(), Value::String(value.to_string())) {
                if key == "fields" || key == "expand" {
                    params.insert(
                        key.into_owned(),
                        json!(format!("{},{}", old.as_str().unwrap(), value)),
                    );
                } else {
                    return Err("invalid: repeated query parameter".into());
                }
            }
        }
        // A modern jira-python client can use POST for a read-only classic search.
        let search = verb == Method::Post && route == "search";
        let read = verb == Method::Get || search;
        if !read && principal.scope != "write" {
            store.execute(
                &principal.actor,
                json!({"op":"jira.read","space":org,"api_version":version,"resource":"myself"}),
            )?;
            return Err("forbidden: this personal token is read-only".into());
        }
        let mut payload = Value::Null;
        if matches!(verb, Method::Post | Method::Put) {
            const MAX: usize = 1024 * 1024;
            if req.body_length().is_some_and(|n| n > MAX) {
                return Err("too_large: JSON body exceeds 1 MiB".into());
            }
            let mut data = Vec::new();
            req.as_reader()
                .take(MAX as u64 + 1)
                .read_to_end(&mut data)
                .map_err(|_| "invalid: could not read request")?;
            if data.len() > MAX {
                return Err("too_large: JSON body exceeds 1 MiB".into());
            }
            payload =
                serde_json::from_slice(&data).map_err(|_| "invalid: expected a JSON object")?;
            let object = payload
                .as_object()
                .ok_or("invalid: expected a JSON object")?;
            if search {
                for (k, v) in object {
                    let value = if k == "fields" && v.is_array() {
                        let items = v
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|f| f.as_str().ok_or("invalid: fields must contain names"))
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        json!(items.join(","))
                    } else {
                        v.clone()
                    };
                    if params.insert(k.clone(), value).is_some() {
                        return Err("invalid: duplicate search parameter".into());
                    }
                }
            }
        }
        let data=store.execute(&principal.actor,json!({"op":if read{"jira.read"}else{"jira.write"},"space":org,"resource":route,
            "method":if search{"GET"}else{verb.as_str()},"api_version":version,"params":params,"payload":payload,"base_url":self.base}))?;
        Ok((
            data["status"].as_u64().unwrap_or(200) as u16,
            data["body"].clone(),
        ))
    }
    pub(super) fn respond(&self, mut req: Request, auth: &Auth, store: &mut Store) {
        let (status, body) = self.handle(&mut req, auth, store).unwrap_or_else(|e| {
            let (code, message) = e.split_once(": ").unwrap_or(("server", "request failed"));
            let status = match code {
                "unauthorized" => 401,
                "forbidden" => 403,
                "not_found" => 404,
                "method" => 405,
                "conflict" => 409,
                "too_large" => 413,
                "rate" => 429,
                "invalid" => 400,
                _ => 500,
            };
            let safe = if status == 500 {
                eprintln!("ShipForge Jira: {e}");
                "workspace operation could not be completed"
            } else {
                message
            };
            (status, json!({"errorMessages":[safe],"errors":{}}))
        });
        let bytes = if status == 204 {
            Vec::new()
        } else {
            body.to_string().into_bytes()
        };
        let mut response = Response::from_data(bytes)
            .with_status_code(StatusCode(status))
            .with_header(
                Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap(),
            )
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
            .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
        if status == 401 {
            response.add_header(
                Header::from_bytes("WWW-Authenticate", "Bearer realm=\"ShipForge Jira\"").unwrap(),
            );
        }
        if status == 429 {
            response.add_header(Header::from_bytes("Retry-After", "60").unwrap());
        }
        let _ = req.respond(response);
    }
}
fn decode_segment(segment: &str) -> Result<String> {
    let mut bytes = segment.bytes();
    let mut decoded = Vec::new();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let mut part = || {
                bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or("invalid: invalid URL encoding")
            };
            decoded.push((16 * part()? + part()?) as u8);
        } else {
            decoded.push(b);
        }
    }
    let value = String::from_utf8(decoded).map_err(|_| "invalid: path must be UTF-8")?;
    if value
        .chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err("invalid: invalid path segment".into());
    }
    Ok(value)
}
