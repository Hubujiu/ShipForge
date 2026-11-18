use crate::config::Result;
use serde_json::{Value, json};
use shipforge_client::Client;
use std::time::Duration;
use ureq::http::Response;

fn repository(data: &Value) -> Result<&str> {
    if data["fields"]["provider"] != "github" || data["fields"]["enabled"] == false {
        return Err("an enabled github connector is required".into());
    }
    let name = data["fields"]["repository"]
        .as_str()
        .ok_or("github connector needs repository")?;
    let parts: Vec<_> = name.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
        })
    {
        return Err("invalid GitHub repository".into());
    }
    Ok(name)
}
struct Api {
    agent: ureq::Agent,
    token: Option<String>,
}
impl Api {
    fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .max_redirects(0)
                .timeout_global(Some(Duration::from_secs(30)))
                .build()
                .into(),
            token: std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty()),
        }
    }
    fn get(&self, url: &str) -> Result<Response<ureq::Body>> {
        let mut req = self
            .agent
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "ShipForge-Worker");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req.call().map_err(|e| format!("GitHub GET: {e}"))
    }
    fn patch(&self, url: &str, etag: Option<&str>, data: Value) -> Result<Value> {
        let token = self
            .token
            .as_deref()
            .ok_or("GITHUB_TOKEN is required to update an issue")?;
        let mut req = self
            .agent
            .patch(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "ShipForge-Worker")
            .header("Authorization", format!("Bearer {token}"));
        if let Some(etag) = etag {
            req = req.header("If-Match", etag);
        }
        req.send_json(data)
            .map_err(|e| format!("GitHub PATCH: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| e.to_string())
    }
}
fn request(issue: &Value, space: &str, connector: &str) -> Result<Value> {
    let number = issue["number"]
        .as_u64()
        .ok_or("GitHub issue number missing")?;
    let revision = issue["updated_at"]
        .as_str()
        .ok_or("GitHub updated_at missing")?;
    Ok(
        json!({"op":"connector.ingest","space":space,"id":connector,"external_id":number.to_string(),"event_id":format!("{number}:{revision}:{}",super::config::hex_digest(issue["title"].to_string()+&issue["body"].to_string()+&issue["state"].to_string())),"source_version":revision,"title":issue["title"],"description":issue["body"].as_str().unwrap_or_default(),"closed":issue["state"]=="closed"}),
    )
}
pub fn sync(client: &mut Client, space: &str, id: &str, number: Option<u64>) -> Result<Value> {
    let connector = client.execute(json!({"op":"get","space":space,"kind":"connector","id":id}))?;
    let repo = repository(&connector)?;
    let api = Api::new();
    if let Some(number) = number {
        let url = format!("https://api.github.com/repos/{repo}/issues/{number}");
        let issue: Value = api
            .get(&url)?
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_json()
            .map_err(|e| e.to_string())?;
        if issue.get("pull_request").is_some() {
            return Err("the requested number is a pull request".into());
        }
        return client.execute(request(&issue, space, id)?);
    }
    let mut page = 1u64;
    let mut total = 0;
    let mut created = 0;
    let mut updated = 0;
    let mut conflicts = Vec::new();
    loop {
        let url = format!(
            "https://api.github.com/repos/{repo}/issues?state=all&sort=updated&direction=asc&per_page=100&page={page}"
        );
        let mut resp = api.get(&url)?;
        let issues: Vec<Value> = resp
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_json()
            .map_err(|e| e.to_string())?;
        for issue in issues
            .iter()
            .filter(|value| value.get("pull_request").is_none())
        {
            let result = client.execute(request(issue, space, id)?)?;
            total += 1;
            match result["action"].as_str() {
                Some("created") => created += 1,
                Some("updated") => updated += 1,
                Some("conflict") => conflicts.push(result),
                _ => {}
            }
        }
        if issues.len() < 100 {
            break;
        }
        page += 1;
    }
    Ok(json!({"issues":total,"created":created,"updated":updated,"conflicts":conflicts}))
}
pub fn push(client: &mut Client, space: &str, id: &str, task: &str) -> Result<Value> {
    push_on(
        client,
        space,
        id,
        task,
        &Api::new(),
        "https://api.github.com",
    )
}
fn push_on(
    client: &mut Client,
    space: &str,
    id: &str,
    task: &str,
    api: &Api,
    origin: &str,
) -> Result<Value> {
    let connector = client.execute(json!({"op":"get","space":space,"kind":"connector","id":id}))?;
    let repo = repository(&connector)?;
    let mapping =
        client.execute(json!({"op":"connector.export","space":space,"id":id,"task_id":task}))?;
    let number = mapping["external_id"]
        .as_str()
        .ok_or("missing issue mapping")?
        .parse::<u64>()
        .map_err(|_| "invalid GitHub issue number")?;
    let url = format!("{origin}/repos/{repo}/issues/{number}");
    let mut response = api.get(&url)?;
    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let remote: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    if remote.get("pull_request").is_some() {
        return Err("GitHub issue mapping points to a pull request".into());
    }
    if remote["updated_at"] != mapping["source_version"] {
        return Err("upstream issue changed; synchronize and resolve it before pushing".into());
    }
    let value = &mapping["value"];
    let state = if value["closed"] == true {
        "closed"
    } else {
        "open"
    };
    if remote["title"] == value["title"]
        && remote["body"].as_str().unwrap_or_default()
            == value["description"].as_str().unwrap_or_default()
        && remote["state"] == state
    {
        return Ok(json!({"action":"unchanged","external_id":number,"task":task}));
    }
    let changed = api.patch(
        &url,
        etag.as_deref(),
        json!({"title":value["title"],"body":value["description"],"state":state}),
    )?;
    client.execute(request(&changed, space, id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn bytes<R: BufRead>(reader: &mut R, length: usize, chunked: bool) -> Vec<u8> {
        let mut data = Vec::new();
        if chunked {
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let size = usize::from_str_radix(line.trim(), 16).unwrap();
                if size == 0 {
                    break;
                }
                let pos = data.len();
                data.resize(pos + size, 0);
                reader.read_exact(&mut data[pos..]).unwrap();
                let mut end = String::new();
                reader.read_line(&mut end).unwrap();
            }
        } else {
            reader.take(length as u64).read_to_end(&mut data).unwrap();
        }
        data
    }
    #[test]
    fn github_updates_only_mapped_issues_and_refuses_stale_changes() {
        let mut client = Client::memory().unwrap();
        client.execute(json!({"op":"create","kind":"connector","id":"gh","title":"Source","fields":{"provider":"github","repository":"sample/repo"}})).unwrap();
        let first = json!({"number":12,"updated_at":"2026-01-01T00:00:00Z","title":"Before","body":"base","state":"open"});
        let applied = client
            .execute(request(&first, "default", "gh").unwrap())
            .unwrap();
        let id = applied["task"].as_str().unwrap();
        client
            .execute(json!({"op":"update","kind":"task","id":id,"title":"After"}))
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            for n in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut start = String::new();
                reader.read_line(&mut start).unwrap();
                let method = if n == 1 { "PATCH" } else { "GET" };
                assert!(start.starts_with(&format!("{method} /repos/sample/repo/issues/12 ")));
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim_end().is_empty() {
                        break;
                    }
                    headers.push_str(&line.to_ascii_lowercase());
                }
                assert!(headers.contains("authorization: bearer synthetic-token"));
                let mut issue = first.clone();
                if n == 1 {
                    assert!(headers.contains("if-match: \"first\""));
                    let size = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|l| l.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let body = bytes(
                        &mut reader,
                        size,
                        headers.contains("transfer-encoding: chunked"),
                    );
                    let value: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(value, json!({"title":"After","body":"base","state":"open"}));
                    issue["title"] = json!("After");
                    issue["updated_at"] = json!("2026-01-02T00:00:00Z");
                }
                if n == 2 {
                    issue["updated_at"] = json!("2026-01-03T00:00:00Z");
                }
                let body = issue.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"first\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        let api = Api {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(6)))
                .proxy(None)
                .build()
                .into(),
            token: Some("synthetic-token".into()),
        };
        let receipt = push_on(&mut client, "default", "gh", id, &api, &origin).unwrap();
        assert_eq!(receipt["task"], id);
        assert_eq!(
            client
                .execute(json!({"op":"connector.export","id":"gh","task_id":id}))
                .unwrap()["source_version"],
            "2026-01-02T00:00:00Z"
        );
        assert!(
            push_on(&mut client, "default", "gh", id, &api, &origin)
                .unwrap_err()
                .contains("upstream issue changed")
        );
        worker.join().unwrap();
    }
}
