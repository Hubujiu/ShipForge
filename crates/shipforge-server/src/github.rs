use super::{Auth, Store, auth, split_target};
use ring::hmac;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use tiny_http::Request;

struct Binding {
    repo: String,
    key: hmac::Key,
    token: String,
}
pub struct Routes(HashMap<String, Binding>);
impl Routes {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self(HashMap::new()));
        };
        let spec: Value = serde_json::from_slice(
            &std::fs::read(path).map_err(|e| format!("GitHub route configuration: {e}"))?,
        )
        .map_err(|e| format!("GitHub route configuration: {e}"))?;
        let mut routes = HashMap::new();
        let mut installations = HashSet::new();
        for row in spec
            .as_array()
            .ok_or("invalid: GitHub routes must be a JSON array")?
        {
            let field = |key: &str| {
                row[key]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 400 && !s.chars().any(char::is_control))
                    .ok_or_else(|| format!("invalid: missing GitHub {key}"))
            };
            let secret = |key: &str| -> Result<String, String> {
                let env = field(key)?;
                if !env.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    return Err("invalid: invalid environment variable name".into());
                }
                std::env::var(env)
                    .ok()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("invalid: missing environment variable {env}"))
            };
            let (space, id) = (field("space")?, field("connector")?);
            if [space, id]
                .iter()
                .any(|s| s.contains(['/', '\\', '?', '#']))
            {
                return Err("invalid: invalid GitHub route".into());
            }
            let repo = field("repository")?.to_ascii_lowercase();
            let path = format!("{space}/{id}");
            let signing = secret("signing_secret_env")?;
            // A valid signature is bound to the request body, not its delivery headers or URL.
            // Two independent installations of the same repository must use different secrets.
            if routes.contains_key(&path)
                || !installations.insert((
                    repo.clone(),
                    ring::digest::digest(&ring::digest::SHA256, signing.as_bytes())
                        .as_ref()
                        .to_vec(),
                ))
            {
                return Err("invalid: duplicate GitHub route or repository webhook secret".into());
            }
            routes.insert(
                path,
                Binding {
                    repo,
                    key: hmac::Key::new(hmac::HMAC_SHA256, signing.as_bytes()),
                    token: secret("hook_token_env")?,
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
        let binding = self.0.get(path).ok_or("not_found: GitHub route")?;
        let header = |name: &str| {
            request
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
                .map(|h| h.value.as_str().to_owned())
        };
        let signature =
            header("X-Hub-Signature-256").ok_or("unauthorized: missing GitHub signature")?;
        let hex = signature
            .strip_prefix("sha256=")
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("unauthorized: invalid GitHub signature")?;
        let mut bytes = [0; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| "unauthorized: invalid GitHub signature")?;
        }
        let delivery = header("X-GitHub-Delivery").ok_or("invalid: missing GitHub delivery ID")?;
        let event = header("X-GitHub-Event").ok_or("invalid: missing GitHub event")?;
        const LIMIT: usize = 8 * 1024 * 1024;
        if request.body_length().is_some_and(|n| n > LIMIT) {
            return Err("too_large: GitHub event exceeds 8 MiB".into());
        }
        let mut raw = Vec::new();
        request
            .as_reader()
            .take(LIMIT as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "invalid: cannot read GitHub event")?;
        if raw.len() > LIMIT {
            return Err("too_large: GitHub event exceeds 8 MiB".into());
        }
        hmac::verify(&binding.key, &raw, &bytes)
            .map_err(|_| "unauthorized: invalid GitHub signature")?;
        let payload: Value =
            serde_json::from_slice(&raw).map_err(|_| "invalid: malformed GitHub JSON")?;
        let (space, id) = split_target(path)?;
        let p = auth.authenticate(&binding.token)?;
        if p.scope != "hook" {
            return Err("forbidden: GitHub route requires a bound hook credential".into());
        }
        let q = json!({"op":"provider.ingest","space":space,"id":id,"provider":"github","event_id":delivery,"payload":payload});
        auth::authorize(&p, &q)?;
        let connector = store.execute(
            &p.actor,
            json!({"op":"get","kind":"connector","id":id,"space":space}),
        )?;
        let f = &connector["fields"];
        if f["provider"] != "github"
            || f["enabled"] != true
            || f["link_tasks"] != true
            || !f["repository"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(&binding.repo))
            || !payload["repository"]["full_name"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(&binding.repo))
        {
            return Err("forbidden: unexpected GitHub repository or connector".into());
        }
        let expected = match event.as_str() {
            "ping" => return Ok(json!({"ok":true})),
            "push" => payload["commits"].is_array() && payload["pull_request"].is_null(),
            "pull_request" => payload["pull_request"].is_object(),
            _ => return Ok(json!({"ignored":true})),
        };
        if !expected {
            return Err("invalid: GitHub event type does not match the signed payload".into());
        }
        store.execute(&p.actor, q)?;
        Ok(json!({"ok":true}))
    }
}
