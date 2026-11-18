use super::{Auth, Store, auth, split_target};
use ring::hmac;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::Request;

pub struct Routes(HashMap<String, Binding>);
struct Binding {
    workspace: String,
    channel: String,
    app: String,
    key: hmac::Key,
    token: String,
}

impl Routes {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self(HashMap::new()));
        };
        let bytes = std::fs::read(path).map_err(|e| format!("Slack route configuration: {e}"))?;
        let spec: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Slack route configuration: {e}"))?;
        Self::parse(&spec, |s| std::env::var(s).ok().filter(|s| !s.is_empty()))
    }
    fn parse(spec: &Value, secret: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut routes = HashMap::new();
        let mut origins = HashSet::new();
        let mut app_keys = HashMap::new();
        for row in spec
            .as_array()
            .ok_or("invalid: Slack routes must be a JSON array")?
        {
            let field = |name: &str| {
                row[name]
                    .as_str()
                    .filter(|v| {
                        !v.is_empty()
                            && v.len() <= 160
                            && !v
                                .chars()
                                .any(|c| c.is_control() || matches!(c, '/' | '\\' | '?' | '#'))
                    })
                    .ok_or_else(|| format!("invalid: missing or invalid Slack route field {name}"))
            };
            let credential = |name: &str| -> Result<String, String> {
                let variable = field(name)?;
                if !variable
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err("invalid: invalid environment variable name".into());
                }
                secret(variable)
                    .ok_or_else(|| format!("invalid: missing environment variable {variable}"))
            };
            let path = format!("{}/{}", field("space")?, field("connector")?);
            let workspace = field("workspace_id")?.to_string();
            let channel = field("channel_id")?.to_string();
            let app = field("app_id")?.to_string();
            let signing_variable = field("signing_secret_env")?;
            if app_keys
                .insert(app.clone(), signing_variable.to_owned())
                .is_some_and(|old| old != signing_variable)
            {
                return Err(
                    "invalid: routes for one Slack app must share its signing secret".into(),
                );
            }
            if !origins.insert((workspace.clone(), channel.clone(), app.clone()))
                || routes.contains_key(&path)
            {
                return Err("invalid: duplicate Slack source or callback route".into());
            }
            let key = hmac::Key::new(
                hmac::HMAC_SHA256,
                credential("signing_secret_env")?.as_bytes(),
            );
            routes.insert(
                path,
                Binding {
                    workspace,
                    channel,
                    app,
                    key,
                    token: credential("hook_token_env")?,
                },
            );
        }
        Ok(Self(routes))
    }

    pub fn handle(
        &self,
        path: &str,
        request: &mut Request,
        auth: &Auth,
        store: &mut Store,
    ) -> Result<Value, String> {
        let app = path
            .strip_prefix("apps/")
            .and_then(|s| s.strip_suffix("/events"));
        let binding = if let Some(app) = app {
            self.0.values().find(|b| b.app == app)
        } else {
            self.0.get(path)
        }
        .ok_or("not_found: Slack route")?;
        let header = |name: &str| {
            request
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
                .map(|h| h.value.as_str().to_owned())
        };
        let ts =
            header("X-Slack-Request-Timestamp").ok_or("unauthorized: missing Slack timestamp")?;
        let signature =
            header("X-Slack-Signature").ok_or("unauthorized: missing Slack signature")?;
        let form = header("Content-Type").is_some_and(|s| {
            s.split(';').next().is_some_and(|v| {
                v.trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            })
        });
        const LIMIT: usize = 1024 * 1024;
        if request.body_length().is_some_and(|v| v > LIMIT) {
            return Err("too_large: Slack event exceeds 1 MiB".into());
        }
        let mut raw = Vec::new();
        request
            .as_reader()
            .take(LIMIT as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "invalid: cannot read Slack event")?;
        if raw.len() > LIMIT {
            return Err("too_large: Slack event exceeds 1 MiB".into());
        }
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "server: invalid local clock")?
            .as_secs();
        verify(&binding.key, &ts, &signature, &raw, at)?;
        let p: Value = if form {
            let mut items = url::form_urlencoded::parse(&raw).filter(|(k, _)| k == "payload");
            let (_, body) = items
                .next()
                .ok_or("invalid: Slack interactive payload is missing")?;
            if items.next().is_some() {
                return Err("invalid: duplicate Slack payload".into());
            }
            let value: Value = serde_json::from_str(&body)
                .map_err(|_| "invalid: malformed Slack interactive payload")?;
            if value["type"] != "block_actions" {
                return Err("invalid: unsupported Slack interactive action".into());
            }
            value
        } else {
            let value: Value =
                serde_json::from_slice(&raw).map_err(|_| "invalid: malformed Slack JSON")?;
            if value["type"] == "block_actions" {
                return Err("invalid: Slack interactive payload requires form encoding".into());
            }
            value
        };
        if let Some(app) = app {
            let candidates = self.0.iter().filter(|(_, b)| b.app == app);
            if p["type"] == "url_verification" {
                for (path, route) in candidates {
                    match route.receive(path, &p, auth, store) {
                        Ok(challenge) => return Ok(challenge),
                        Err(e)
                            if e.starts_with("forbidden:")
                                || e.starts_with("unauthorized:")
                                || e.starts_with("not_found:") =>
                        {
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                return Err("forbidden: app has no active authorized Slack route".into());
            }
            if p["api_app_id"] != app {
                return Err("forbidden: unexpected Slack app".into());
            }
            let (workspace, channel) = if form {
                (&p["team"]["id"], &p["channel"]["id"])
            } else {
                (&p["team_id"], event_channel(&p["event"]))
            };
            if let Some((path, route)) = candidates.into_iter().find(|(_, b)| {
                workspace.as_str() == Some(&b.workspace) && channel.as_str() == Some(&b.channel)
            }) {
                return route.receive(path, &p, auth, store);
            }
            return if form {
                Err("forbidden: Slack card has no matching channel binding".into())
            } else {
                Ok(json!({"ok":true}))
            };
        }
        binding.receive(path, &p, auth, store)
    }
}

fn event_channel(e: &Value) -> &Value {
    if e["type"] == "message_metadata_posted" {
        &e["channel_id"]
    } else {
        &e["channel"]
    }
}

fn verify(key: &hmac::Key, ts: &str, signature: &str, raw: &[u8], at: u64) -> Result<(), String> {
    let denied = || "unauthorized: invalid or expired Slack signature".to_string();
    if ts.len() > 15
        || !ts.bytes().all(|b| b.is_ascii_digit())
        || ts.parse::<u64>().map_err(|_| denied())?.abs_diff(at) > 300
    {
        return Err(denied());
    }
    let hex = signature
        .strip_prefix("v0=")
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(denied)?;
    let mut tag = [0; 32];
    for (idx, byte) in tag.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[idx * 2..idx * 2 + 2], 16).map_err(|_| denied())?;
    }
    let mut bytes = format!("v0:{ts}:").into_bytes();
    bytes.extend_from_slice(raw);
    hmac::verify(key, &bytes, &tag).map_err(|_| denied())
}

impl Binding {
    fn receive(
        &self,
        path: &str,
        p: &Value,
        auth: &Auth,
        store: &mut Store,
    ) -> Result<Value, String> {
        let (space, id) = split_target(path)?;
        let principal = auth.authenticate(&self.token)?;
        if principal.scope != "hook" {
            return Err("forbidden: Slack route requires a bound hook credential".into());
        }
        let q =
            json!({"op":"provider.ingest","id":id,"space":space,"provider":"slack","payload":p});
        auth::authorize(&principal, &q)?;
        let connector = store.execute(
            &principal.actor,
            json!({"op":"get","kind":"connector","id":id,"space":space}),
        )?;
        let role = store.execute(&principal.actor, json!({"op":"whoami","space":space}))?;
        let f = &connector["fields"];
        if !matches!(role["role"].as_str(), Some("owner" | "admin"))
            || f["provider"] != "slack"
            || f["enabled"] != true
            || f["workspace_id"] != self.workspace
            || f["channel_id"] != self.channel
            || f["app_id"] != self.app
        {
            return Err(
                "forbidden: Slack route does not match an active authorized connector".into(),
            );
        }
        if p["type"] == "block_actions" {
            store.slack_card_action(&principal.actor, space, id, p)?;
            return Ok(Value::Null);
        }
        if p["type"] == "url_verification" {
            let challenge = p["challenge"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 1024)
                .ok_or("invalid: missing Slack challenge")?;
            return Ok(json!({"challenge":challenge}));
        }
        if p["type"] != "event_callback"
            || p["team_id"] != self.workspace
            || p["api_app_id"] != self.app
        {
            return Err("forbidden: unexpected Slack workspace or app".into());
        }
        let event = &p["event"];
        if !matches!(
            event["type"].as_str(),
            Some("message" | "message_metadata_posted")
        ) || event_channel(event).as_str() != Some(&self.channel)
        {
            // Slack sends other subscribed workspace channels to the same URL.
            return Ok(json!({"ok":true}));
        }
        if event["type"] == "message_metadata_posted"
            || !event["bot_id"].is_null()
            || !event["bot_profile"].is_null()
        {
            store.confirm_slack_delivery(&principal.actor, space, id, p)?;
            return Ok(json!({"ok":true}));
        } else if !event["subtype"].is_null() {
            return Ok(json!({"ok":true}));
        }
        store.execute(&principal.actor, q)?;
        Ok(json!({"ok":true}))
    }
}
