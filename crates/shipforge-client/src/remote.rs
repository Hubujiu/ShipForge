use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};
use url::{Host, Url};
use uuid::Uuid;

fn storage(error: impl std::fmt::Display) -> String {
    format!("storage: {error}")
}

pub(super) fn endpoint(address: &str, token: &str) -> Result<String, String> {
    let url = Url::parse(address.trim()).map_err(|_| "invalid: invalid team URL".to_string())?;
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err("invalid: a team connection requires HTTPS or loopback HTTP".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid: team URL may contain only scheme, host, port and path".into());
    }
    if token.is_empty() || token.len() > 512 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("invalid: invalid team access token".into());
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

pub(super) struct Remote {
    http: ureq::Agent,
    command_url: String,
    credential: String,
    cache: Connection,
    online: Option<bool>,
    last_network_failure: Option<Instant>,
}

impl Remote {
    pub(super) fn credential_site(&self) -> &str {
        &self.command_url
    }

    pub fn new(directory: &Path, address: &str, token: &str) -> Result<Self, String> {
        let address = endpoint(address, token)?;
        let mut sha = Sha256::new();
        sha.update(address.as_bytes());
        sha.update([0]);
        sha.update(token.as_bytes());
        let cache_dir = directory.join("team-cache");
        std::fs::create_dir_all(&cache_dir).map_err(storage)?;
        let digest = sha.finalize();
        let name = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let cache = Connection::open(cache_dir.join(format!("{name}.sqlite3"))).map_err(storage)?;
        cache
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage)?;
        cache.execute_batch("PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS reads(query TEXT PRIMARY KEY, answer TEXT NOT NULL, at INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS drafts(seq INTEGER PRIMARY KEY, id TEXT UNIQUE NOT NULL, body TEXT NOT NULL, problem TEXT);").map_err(storage)?;
        let http = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(12)))
            .build()
            .new_agent();
        Ok(Self {
            http,
            command_url: format!("{address}/v1/command"),
            credential: token.to_owned(),
            cache,
            online: None,
            last_network_failure: None,
        })
    }

    pub(super) fn send(&mut self, body: &Value) -> Result<Value, String> {
        let call = self
            .http
            .post(&self.command_url)
            .header("Authorization", format!("Bearer {}", self.credential))
            .send_json(body);
        let mut response = match call {
            Ok(r) => {
                self.online = Some(true);
                self.last_network_failure = None;
                r
            }
            Err(e) => {
                self.online = Some(false);
                self.last_network_failure = Some(Instant::now());
                return Err(format!("offline: {e}"));
            }
        };
        let status = response.status().as_u16();
        if matches!(status, 502..=504) {
            self.online = Some(false);
            self.last_network_failure = Some(Instant::now());
            return Err(format!("offline: team gateway returned HTTP {status}"));
        }
        if matches!(status, 401 | 403 | 404) {
            self.cache
                .execute("DELETE FROM reads", [])
                .map_err(storage)?;
        }
        let answer: Value = response
            .body_mut()
            .with_config()
            .limit(32 * 1024 * 1024)
            .read_json()
            .map_err(|_| format!("server: invalid JSON in HTTP {status} reply"))?;
        if status >= 300 || answer["ok"] != true {
            let code = answer["error"]["code"].as_str().unwrap_or("server");
            let message = answer["error"]["message"]
                .as_str()
                .unwrap_or("request rejected");
            return Err(format!("{code}: {message}"));
        }
        let mut data = answer["data"].clone();
        if body["op"] == "bugly.rotate"
            && let Some(path) = data["path"].as_str()
        {
            data["url"] = json!(format!(
                "{}{path}",
                self.command_url.trim_end_matches("/v1/command")
            ));
        }
        if body["op"] == "chat.share.create"
            && let Some(key) = data["share"].as_str()
        {
            let base = self.command_url.trim_end_matches("/v1/command");
            data["link"] = json!(format!("{base}/share/{key}"));
        }
        if body["op"] == "document.share"
            && let Some(path) = data["path"].as_str()
        {
            data["http_url"] = json!(format!(
                "{}{}",
                self.command_url.trim_end_matches("/v1/command"),
                path
            ));
        }
        Ok(data)
    }

    pub fn execute(&mut self, mut request: Value) -> Result<Value, String> {
        let op = request["op"]
            .as_str()
            .ok_or_else(|| "invalid: op is required".to_owned())?
            .to_owned();
        match op.as_str() {
            "sync.status" => return self.status(),
            "sync.pending" => return self.pending(),
            "sync.enqueue" => return self.enqueue(request["request"].clone()),
            "sync.flush" => return self.flush(),
            "sync.resolve" => return self.resolve(&request),
            _ => {}
        }
        let allow_cache = request
            .as_object_mut()
            .and_then(|o| o.remove("allow_cached"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let queue = request
            .as_object_mut()
            .and_then(|o| o.remove("queue_if_offline"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let needs_live_access = matches!(op.as_str(), "get" | "list" | "search")
            && (matches!(
                request["kind"].as_str(),
                Some(
                    "chat"
                        | "document"
                        | "message"
                        | "notification"
                        | "artifact"
                        | "run"
                        | "view"
                        | "comment"
                        | "agent"
                        | "crew"
                        | "skill"
                )
            ) || (op == "search"
                && (request["kind"].as_str().is_none()
                    || request["search_scope"] == "organization")));
        let cacheable = !needs_live_access
            && matches!(
                op.as_str(),
                "list"
                    | "preferences.get"
                    | "get"
                    | "dashboard"
                    | "iteration.timeline"
                    | "search"
                    | "space.list"
                    | "space.navigation"
            );
        if queue && request.get("request_id").is_none() {
            request["request_id"] = json!(Uuid::new_v4().to_string());
        }
        if queue && op == "create" && request.get("id").is_none() {
            let mut seed = Sha256::new();
            seed.update(self.credential.as_bytes());
            seed.update(request["request_id"].to_string());
            let key: String = seed
                .finalize()
                .iter()
                .take(16)
                .map(|b| format!("{b:02x}"))
                .collect();
            request["id"] = json!(format!("draft-{key}"));
        }
        let key = request.to_string();
        // Reopening a board may request several lists. Back off briefly after a failed connection.
        let answer = if cacheable
            && allow_cache
            && self
                .last_network_failure
                .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            Err("offline: team server unavailable".into())
        } else {
            self.send(&request)
        };
        match answer {
            Ok(answer) => {
                if cacheable {
                    let encoded = answer.to_string();
                    let tx = self.cache.transaction().map_err(storage)?;
                    // A changed result may reflect an ACL or membership change affecting other cached queries.
                    tx.execute("DELETE FROM reads WHERE EXISTS (SELECT 1 FROM reads AS previous WHERE previous.query=?1 AND previous.answer<>?2)", params![key, encoded]).map_err(storage)?;
                    tx.execute(
                        "INSERT OR REPLACE INTO reads(query,answer,at) VALUES(?1,?2,unixepoch())",
                        params![key, encoded],
                    )
                    .map_err(storage)?;
                    tx.commit().map_err(storage)?;
                } else if op == "preferences.update" {
                    let query = json!({"op":"preferences.get"}).to_string();
                    let tx = self.cache.transaction().map_err(storage)?;
                    tx.execute(
                        "DELETE FROM reads WHERE json_extract(query,'$.op')='preferences.get'",
                        [],
                    )
                    .map_err(storage)?;
                    tx.execute(
                        "INSERT INTO reads(query,answer,at) VALUES(?1,?2,unixepoch())",
                        params![query, answer.to_string()],
                    )
                    .map_err(storage)?;
                    tx.commit().map_err(storage)?;
                } else if !shipforge_core::is_read_only(&op) && !op.starts_with("access.") {
                    self.cache
                        .execute("DELETE FROM reads", [])
                        .map_err(storage)?;
                }
                Ok(answer)
            }
            Err(error) if error.starts_with("offline:") && cacheable && allow_cache => {
                let saved: Option<String> = self
                    .cache
                    .query_row(
                        "SELECT answer FROM reads WHERE query=?1 AND at>=unixepoch()-86400",
                        [key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(storage)?;
                match saved {
                    Some(text) => serde_json::from_str(&text).map_err(storage),
                    None => Err(error),
                }
            }
            Err(error) if error.starts_with("offline:") && queue => self.enqueue(request),
            Err(error) => Err(error),
        }
    }

    fn status(&self) -> Result<Value, String> {
        let n: i64 = self
            .cache
            .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
            .map_err(storage)?;
        let conflicts: i64 = self
            .cache
            .query_row(
                "SELECT count(*) FROM drafts WHERE problem IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        Ok(json!({"mode":"team","online":self.online,"pending":n,"conflicts":conflicts}))
    }

    fn enqueue(&self, mut body: Value) -> Result<Value, String> {
        let op = body["op"].as_str().unwrap_or("");
        if !matches!(op, "create" | "update" | "delete")
            || !matches!(
                body["kind"].as_str(),
                Some(
                    "task" | "document" | "comment" | "view" | "project" | "iteration" | "message"
                )
            )
        {
            return Err("invalid: offline drafts support editable workspace objects".into());
        }
        if op != "create" && body["expected_rev"].as_i64().is_none() {
            return Err("invalid: expected_rev is required for offline updates and deletes".into());
        }
        if body.get("request_id").is_none() {
            body["request_id"] = json!(Uuid::new_v4().to_string());
        }
        let id = body["request_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 160)
            .ok_or_else(|| "invalid: invalid request_id".to_owned())?;
        let data = body.to_string();
        let changed = self
            .cache
            .execute(
                "INSERT INTO drafts(id,body) VALUES(?1,?2) ON CONFLICT(id) DO NOTHING",
                params![id, data],
            )
            .map_err(storage)?;
        if changed == 0 {
            let existing: String = self
                .cache
                .query_row("SELECT body FROM drafts WHERE id=?1", [id], |r| r.get(0))
                .map_err(storage)?;
            if existing != data {
                return Err("conflict: request ID already belongs to a different draft".into());
            }
        }
        Ok(json!({"queued":true,"request_id":id}))
    }

    fn pending(&self) -> Result<Value, String> {
        let mut statement = self
            .cache
            .prepare("SELECT id,body,problem FROM drafts ORDER BY seq")
            .map_err(storage)?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(storage)?;
        let mut result = Vec::new();
        for row in rows {
            let (id, body, problem) = row.map_err(storage)?;
            result.push(json!({"request_id":id,"request":serde_json::from_str::<Value>(&body).map_err(storage)?,"error":problem}));
        }
        Ok(Value::Array(result))
    }

    fn flush(&mut self) -> Result<Value, String> {
        let mut completed = Vec::new();
        while completed.len() < 100 {
            let next: Option<(String, String, Option<String>)> = self
                .cache
                .query_row(
                    "SELECT id,body,problem FROM drafts ORDER BY seq LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(storage)?;
            let Some((id, body, None)) = next else {
                break;
            };
            match self.send(&serde_json::from_str(&body).map_err(storage)?) {
                Ok(_) => {
                    let tx = self.cache.transaction().map_err(storage)?;
                    tx.execute(
                        "DELETE FROM drafts WHERE id=?1 AND body=?2",
                        params![id, body],
                    )
                    .map_err(storage)?;
                    tx.execute("DELETE FROM reads", []).map_err(storage)?;
                    tx.commit().map_err(storage)?;
                    completed.push(id);
                }
                Err(error) if error.starts_with("offline:") => return Err(error),
                Err(error) => {
                    self.cache
                        .execute(
                            "UPDATE drafts SET problem=?3 WHERE id=?1 AND body=?2",
                            params![id, body, error],
                        )
                        .map_err(storage)?;
                    break;
                }
            }
        }
        let mut status = self.status()?;
        status["changes"] = json!(completed);
        Ok(status)
    }

    fn resolve(&self, request: &Value) -> Result<Value, String> {
        let id = request["id"]
            .as_str()
            .ok_or_else(|| "invalid: id required".to_owned())?;
        let body: String = self
            .cache
            .query_row("SELECT body FROM drafts WHERE id=?1", [id], |r| r.get(0))
            .optional()
            .map_err(storage)?
            .ok_or_else(|| "not_found: draft".to_owned())?;
        if request["discard"] == true {
            let changed = self
                .cache
                .execute(
                    "DELETE FROM drafts WHERE id=?1 AND body=?2",
                    params![id, body],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err("conflict: draft concurrently changed".into());
            }
            return Ok(json!({"discarded":id}));
        }
        let rev = request["expected_rev"]
            .as_i64()
            .ok_or_else(|| "invalid: expected_rev or discard required".to_owned())?;
        let mut draft: Value = serde_json::from_str(&body).map_err(storage)?;
        let new_id = Uuid::new_v4().to_string();
        draft["request_id"] = json!(new_id);
        draft["expected_rev"] = json!(rev);
        let changed = self
            .cache
            .execute(
                "UPDATE drafts SET id=?3,body=?4,problem=NULL WHERE id=?1 AND body=?2",
                params![id, body, new_id, draft.to_string()],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err("conflict: draft concurrently changed".into());
        }
        Ok(json!({"queued":true,"request_id":new_id}))
    }
}
