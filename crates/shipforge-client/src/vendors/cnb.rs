use super::*;
use ring::{digest, hmac};
use std::{collections::HashSet, path::Path};
use uuid::Uuid;

fn identifier(v: &Value) -> Result<String> {
    let val = v
        .as_str()
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|u| u.to_string()))
        .ok_or("invalid: expected a CNB identifier")?;
    if val.is_empty()
        || val.len() > 180
        || matches!(val.as_str(), "." | "..")
        || !val
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
    {
        return Err("invalid: invalid CNB resource ID".into());
    }
    Ok(val)
}
fn slug(value: &str) -> Result<&str> {
    if value.len() > 400 || value.split('/').any(|p| identifier(&json!(p)).is_err()) {
        return Err("invalid: expected a CNB group or repository path".into());
    }
    Ok(value)
}
fn under(group: &str, repo: &str) -> bool {
    repo.strip_prefix(group)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
struct Api {
    root: String,
    token: String,
}
impl Api {
    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut u =
            Url::parse(&format!("{}{path}", self.root)).map_err(|_| "invalid: invalid CNB path")?;
        if !query.is_empty() {
            u.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        response(
            http()
                .get(u.as_str())
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Accept", "application/json")
                .call()
                .map_err(|_| "vendor: CNB is unavailable")?,
        )
    }
    fn pages(&self, path: &str, extra: &[(&str, String)], size: usize) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut bytes = 0;
        for page in 1..=5000 {
            let mut q = extra.to_vec();
            q.extend([("page", page.to_string()), ("page_size", size.to_string())]);
            let res = self.get(path, &q)?;
            let rows = res
                .as_array()
                .ok_or("vendor: expected a paginated CNB list")?;
            for row in rows {
                let id = identifier(&row["id"])?;
                if !seen.insert(id) {
                    return Err("vendor: repeated CNB result during pagination".into());
                }
                bytes += row.to_string().len();
                if out.len() >= 5000 || bytes > 7_000_000 {
                    return Err("vendor: CNB pagination limit exceeded".into());
                }
                out.push(row.clone());
            }
            if rows.len() < size {
                return Ok(out);
            }
        }
        Err("vendor: too many CNB pages".into())
    }
    fn check_group(&self, group: &str) -> Result<()> {
        let mut parts = group.split('/');
        let root = parts.next().unwrap_or_default();
        if !self
            .pages("/user/groups", &[], 100)?
            .iter()
            .any(|r| r["path"] == root)
        {
            return Err("forbidden: token is not a member of this CNB organization".into());
        }
        let mut parent = root.to_owned();
        for component in parts {
            let next = format!("{parent}/{component}");
            if !self
                .pages(&format!("/user/groups/{parent}"), &[], 100)?
                .iter()
                .any(|r| r["path"] == next)
            {
                return Err("forbidden: token has no access to this CNB subgroup".into());
            }
            parent = next;
        }
        Ok(())
    }
    fn repo(&self, path: &str, group: &str) -> Result<Value> {
        if !under(group, slug(path)?) {
            return Err("forbidden: CNB repository is outside the bound group".into());
        }
        let r = self.get(&format!("/{path}"), &[])?;
        if r["path"] != path {
            return Err("forbidden: CNB returned another repository".into());
        }
        identifier(&r["id"])?;
        Ok(r)
    }
    fn native(&self, repo: &str, p: &Value) -> Result<Value> {
        let event = p["event"].as_str().ok_or("invalid: missing CI event")?;
        let (kind, key, collection) = match event {
            "push" | "commit.add" => ("commit", identifier(&p["sha"])?, "git/commits"),
            "issue.open" | "issue.close" | "issue.reopen" | "issue.update" | "issue.comment" => {
                ("issue", identifier(&p["issue"])?, "issues")
            }
            "pull_request"
            | "pull_request.target"
            | "pull_request.update"
            | "pull_request.merged"
            | "pull_request.closed"
            | "pull_request.comment" => ("pull_request", identifier(&p["pull"])?, "pulls"),
            _ => return Err("invalid: unsupported CNB pipeline event".into()),
        };
        if kind == "commit" {
            if !matches!(key.len(), 40 | 64) || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("invalid: expected a full commit SHA".into());
            }
        } else if key.parse::<u64>().is_err() || key == "0" {
            return Err("invalid: invalid issue or pull request number".into());
        }
        let item = self.get(&format!("/{repo}/-/{collection}/{key}"), &[])?;
        let mut native = json!({"repository":repo,"event":event,"kind":kind,"key":key});
        if kind == "issue" && item["invisible"] == true {
            native["kind"] = json!("ignored");
            return Ok(native);
        }
        if kind == "commit" {
            if item["sha"] != key {
                return Err("forbidden: commit differs from the notification".into());
            }
            native["title"] = item["commit"]["message"].clone();
        } else {
            if identifier(&item["number"])? != key {
                return Err(
                    "forbidden: issue or pull request does not match the CI notification".into(),
                );
            }
            native["title"] = item["title"].clone();
            native["body"] = item["body"].clone();
            native["branch"] = item["head"]["ref"].clone();
            native["revision"] = item["head"]["sha"].clone();
            if event.ends_with(".comment") {
                let cid = identifier(&p["comment_id"])?;
                let comment =
                    self.get(&format!("/{repo}/-/{collection}/{key}/comments/{cid}"), &[])?;
                if identifier(&comment["id"])? != cid {
                    return Err("forbidden: mismatched CNB comment".into());
                }
                if comment["body"]
                    .as_str()
                    .is_some_and(|b| b.contains("<!-- shipforge-cnb:"))
                {
                    native["kind"] = json!("ignored");
                }
                native["comment_id"] = json!(cid);
                native["comment"] = comment["body"].clone();
            }
        }
        if native["title"].as_str().is_none_or(str::is_empty) {
            return Err("vendor: missing source title or commit message".into());
        }
        Ok(native)
    }
}
fn context(client: &mut Client, q: &Value) -> Result<(Value, Value, String)> {
    let space = q["space"].as_str().unwrap_or("default");
    administrator(client, space)?;
    let mut get = scoped(q, "get");
    get["kind"] = json!("connector");
    let row = client.execute(get)?;
    if row["fields"]["provider"] != "cnb" {
        return Err("invalid: select a CNB connector".into());
    }
    let org_row = client.execute(
        json!({"op":"get","kind":"space","space":space,"id":space,"allow_cached":false}),
    )?;
    let org = org_row["fields"]["organization"]
        .as_str()
        .unwrap_or("default")
        .to_owned();
    let b = json!({"organization":org});
    let group = slug(row["fields"]["cnb_group"].as_str().unwrap_or(""))?.to_owned();
    Ok((row, b, group))
}
fn pair_location(
    client: &Client,
    b: &Value,
    space: &str,
    id: &str,
    nonce: &str,
) -> Result<Option<std::path::PathBuf>> {
    org_location(client, b, "cnb-ci", &json!([space, id, nonce]).to_string())
}
fn require_pair(row: &Value, repo: &Value) -> Result<()> {
    if row["fields"]["enabled"] != true
        || row["fields"]["link_tasks"] != true
        || identifier(&repo["id"])? != row["fields"]["cnb_repo_id"]
        || row["fields"]["cnb_pair"].as_str().is_none_or(str::is_empty)
    {
        return Err("forbidden: missing or stale CNB repository pairing".into());
    }
    Ok(())
}
fn callback(root: &str, space: &str, id: &str) -> Result<String> {
    let root = crate::remote::endpoint(root, "test")?;
    let mut u = Url::parse(&root).map_err(|_| "invalid: callback base URL")?;
    u.path_segments_mut()
        .map_err(|_| "invalid: callback URL")?
        .pop_if_empty()
        .extend(["v1", "cnb", space, id]);
    Ok(u.to_string())
}
fn guide(endpoint: &str) -> Value {
    let yaml = include_str!("cnb-ci.yml").replace(
        "__SHIPFORGE_ENDPOINT__",
        &serde_json::to_string(endpoint).unwrap(),
    );
    json!({"endpoint":endpoint,"secret_env":"SHIPFORGE_CNB_SIGNING_KEY","file":".cnb.yml","yaml":yaml})
}
fn required_callback(row: &Value) -> Result<&str> {
    row["fields"]["cnb_callback"]
        .as_str()
        .ok_or_else(|| "invalid: reconfigure callback".to_owned())
}
fn deliver(client: &mut Client, q: &Value, api: &Api, repo: &str) -> Result<Value> {
    let mut sent = Vec::new();
    for _ in 0..100 {
        let work = client.execute(scoped(q, "cnb.internal.claim"))?;
        if work.is_null() {
            break;
        }
        let path = format!(
            "/{repo}/-/{}/comments",
            work["object"]
                .as_str()
                .ok_or("invalid: missing CNB note target")?
        );
        let marker = format!(
            "<!-- shipforge-cnb:{} -->",
            work["delivery"].as_str().unwrap_or("")
        );
        let text = format!(
            "[{}]({})\n\n{marker}",
            work["number"].as_str().unwrap_or("ShipForge"),
            work["link"].as_str().ok_or("invalid: missing task link")?
        );
        let mut state = "failed";
        let out = (|| -> Result<String> {
            if matches!(work["previous"].as_str(), Some("uncertain" | "inflight")) {
                state = "uncertain";
                let records = api.pages(&path, &[], 100)?;
                let mut matches = records.iter().filter(|r| r["body"] == text);
                let first = matches.next();
                if matches.next().is_some() || first.is_none() {
                    return Err(
                        "conflict: uncertain CNB reply has no unique matching comment yet".into(),
                    );
                }
                return identifier(&first.unwrap()["id"]);
            }
            let res = http()
                .post(format!("{}{path}", api.root))
                .header("Authorization", format!("Bearer {}", api.token))
                .header("Accept", "application/json")
                .send_json(json!({"body":text}));
            state = "uncertain";
            let resp = res.map_err(|_| "vendor: CNB comment response was interrupted")?;
            if resp.status().is_client_error() {
                state = "failed";
            }
            identifier(&response(resp)?["id"])
        })();
        let mut finish = scoped(q, "cnb.internal.finish");
        finish["delivery"] = work["delivery"].clone();
        finish["lease"] = work["lease"].clone();
        finish["state"] = json!(if out.is_ok() { "sent" } else { state });
        if let Ok(id) = &out {
            finish["remote_id"] = json!(id);
        }
        client.execute(finish)?;
        sent.push(out?);
    }
    Ok(json!({"sent":sent}))
}

pub(super) fn execute(client: &mut Client, op: &str, q: &Value) -> Result<Value> {
    let (row, b, group) = context(client, q)?;
    let space = q["space"].as_str().unwrap_or("default");
    let key = q["id"].as_str().ok_or("invalid: connector ID")?;
    if op == "cnb.activity" {
        return client.execute(scoped(q, "cnb.internal.history"));
    }
    let credentials = org_location(client, &b, "cnb-group", &group)?;
    if op == "cnb.credential" {
        administrator(client, b["organization"].as_str().unwrap())?;
        if q["clear"] == true {
            return vault::save(&credentials, None);
        }
    }
    let saved =
        if op == "cnb.credential" && q["access_token"].as_str().is_some_and(|s| !s.is_empty()) {
            json!({"access_token":input(q,"access_token")?})
        } else {
            vault::read(&credentials)?
        };
    let api = Api {
        root: origin("https://api.cnb.cool")?,
        token: input(&saved, "access_token")?,
    };
    api.check_group(&group)?;
    if op == "cnb.credential" {
        let mut result = vault::save(&credentials, Some(&saved))?;
        result["suffix"] = json!(suffix(&api.token));
        result["group"] = json!(group);
        return Ok(result);
    }
    if op == "cnb.repositories" {
        let size = q
            .get("page_size")
            .map(|v| {
                v.as_u64()
                    .filter(|n| (1..=100).contains(n))
                    .ok_or("invalid: page_size must be 1–100")
            })
            .transpose()?
            .unwrap_or(50);
        let mut list = Vec::new();
        for r in api.pages(
            &format!("/{group}/-/repos"),
            &[("descendant", "all".into())],
            size as usize,
        )? {
            let path = slug(
                r["path"]
                    .as_str()
                    .ok_or("vendor: missing repository path")?,
            )?;
            if !under(&group, path) {
                return Err("forbidden: CNB returned a repository outside the group".into());
            }
            list.push(json!({"path":path,"id":identifier(&r["id"])?,"name":r["name"]}));
        }
        return Ok(json!({"group":group,"repositories":list}));
    }
    let path = if op == "cnb.pair" {
        q["repository"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| row["fields"]["repository"].as_str())
    } else {
        row["fields"]["repository"].as_str()
    }
    .ok_or("invalid: select the repository path")?;
    let repo = api.repo(path, &group)?;
    if op == "cnb.pair" {
        let url = callback(
            q["server_url"]
                .as_str()
                .ok_or("invalid: supply the public ShipForge team address")?,
            space,
            key,
        )?;
        let repo_id = identifier(&repo["id"])?;
        let old = row["fields"]["cnb_pair"].as_str().filter(|s| !s.is_empty());
        if let Some(old) = old {
            if row["fields"]["repository"] != path || row["fields"]["cnb_repo_id"] != repo_id {
                return Err("conflict: create a separate connector for this repository".into());
            }
            if q["rotate"] != true {
                return Ok(guide(required_callback(&row)?));
            }
            identifier(&json!(old))?;
        }
        let nonce = Uuid::new_v4().simple().to_string();
        let signing = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let location = pair_location(client, &b, space, key, &nonce)?;
        let who = identity(client, true)?;
        vault::save(
            &location,
            Some(
                &json!({"secret":signing,"actor":who["actor"],"group":group,"repo":path,"repo_id":repo_id}),
            ),
        )?;
        let update=client.execute(json!({"op":"update","space":space,"kind":"connector","id":key,"expected_rev":row["rev"],"fields":{"repository":path,"link_tasks":true,"cnb_pair":nonce,"cnb_repo_id":repo_id,"cnb_callback":url}}));
        if let Err(err) = update {
            vault::save(&location, None)?;
            return Err(err);
        }
        if let Some(old) = old {
            vault::save(&pair_location(client, &b, space, key, old)?, None)?;
        }
        let mut result = guide(&url);
        result["signing_key"] = json!(signing);
        return Ok(result);
    }
    require_pair(&row, &repo)?;
    if op == "cnb.ci" {
        return Ok(guide(required_callback(&row)?));
    }
    deliver(client, q, &api, path)
}

/// Validate a repository-scoped pipeline notification before resolving its references with the CNB API.
pub fn receive_cnb(
    dir: &Path,
    space: &str,
    connector: &str,
    timestamp: &str,
    signature: &str,
    raw: &[u8],
) -> Result<Value> {
    let q = json!({"id":connector,"space":space});
    let _profile = crate::network::ProfileScope::enter(Some(dir));
    let mut lookup = Client::integration_actor(dir, "owner")?;
    let (row, b, group) = context(&mut lookup, &q)?;
    let r = &row["fields"];
    let pair = identifier(&r["cnb_pair"]).map_err(|_| "unauthorized: missing CNB pairing")?;
    let stored = vault::read(&pair_location(&lookup, &b, space, connector, &pair)?)?;
    let key = input(&stored, "secret")?;
    let stamp = timestamp
        .parse::<i64>()
        .map_err(|_| "unauthorized: invalid CNB timestamp")?;
    if chrono::Utc::now().timestamp().abs_diff(stamp) > 300
        || signature.len() != 64
        || !signature.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("unauthorized: invalid or expired CI signature".into());
    }
    let mut signed = timestamp.as_bytes().to_vec();
    signed.push(b'.');
    signed.extend(raw);
    let mut tag = [0u8; 32];
    for (i, v) in tag.iter_mut().enumerate() {
        *v = u8::from_str_radix(&signature[i * 2..i * 2 + 2], 16)
            .map_err(|_| "unauthorized: invalid CI signature")?;
    }
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
        &signed,
        &tag,
    )
    .map_err(|_| "unauthorized: invalid CI signature")?;
    let p: Value = serde_json::from_slice(raw).map_err(|_| "invalid: malformed CI notification")?;
    let path = slug(stored["repo"].as_str().ok_or("forbidden: stale pairing")?)?;
    if p["repository"] != path
        || r["repository"] != path
        || stored["repo_id"] != r["cnb_repo_id"]
        || stored["group"] != group
        || r["enabled"] != true
    {
        return Err("forbidden: unexpected CNB pipeline repository".into());
    }
    let mut client = Client::integration_actor(
        dir,
        stored["actor"]
            .as_str()
            .ok_or("unauthorized: missing pairing principal")?,
    )?;
    administrator(&mut client, space)?;
    let saved = vault::read(&org_location(&client, &b, "cnb-group", &group)?)?;
    let api = Api {
        root: origin("https://api.cnb.cool")?,
        token: input(&saved, "access_token")?,
    };
    let event_id = identifier(&p["build"])?;
    let hash = digest::digest(&digest::SHA256, raw)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut cmd = scoped(&q, "cnb.internal.receipt");
    cmd["event_id"] = json!(event_id);
    cmd["signed_digest"] = json!(hash);
    let prior = client.execute(cmd.clone())?;
    if prior.is_null() {
        api.check_group(&group)?;
        require_pair(&row, &api.repo(path, &group)?)?;
        let mut data = api.native(path, &p)?;
        data["repo_id"] = r["cnb_repo_id"].clone();
        cmd["op"] = json!("cnb.internal.record");
        cmd["payload"] = data;
        client.execute(cmd)?;
    }
    if r["cnb_backlinks"] == true {
        api.check_group(&group)?;
        require_pair(&row, &api.repo(path, &group)?)?;
        deliver(&mut client, &q, &api, path)?;
    }
    Ok(json!({"received":true,"event_id":event_id}))
}
