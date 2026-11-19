use super::*;
use publish::PublicationError;
use std::collections::HashSet;

mod clickup;
mod linear;
mod oauth;
mod ones;

struct Cloud {
    http: ureq::Agent,
    provider: String,
    root: String,
    authorization: String,
    account: String,
}

impl Cloud {
    fn new(b: &Value, stored: &Value) -> Result<Self> {
        let provider = b["provider"].as_str().ok_or("invalid: missing provider")?;
        let token = cred(stored, "access_token")?;
        if !token.bytes().all(|c| c.is_ascii_graphic()) {
            return Err("invalid: invalid API credential".into());
        }
        let root = match provider {
            "linear" => origin("https://api.linear.app")?,
            "clickup" => format!("{}/api/v2", origin("https://api.clickup.com")?),
            "ones" => format!(
                "{}/openapi/v2",
                site(
                    b["ones_url"]
                        .as_str()
                        .ok_or("invalid: enter the ONES tenant origin")?
                )?
            ),
            _ => return Err("invalid: unknown tracker provider".into()),
        };
        let bearer = provider == "ones" || provider == "linear" && b["cloud_auth"] == "oauth";
        Ok(Self {
            http: http(),
            root,
            provider: provider.into(),
            authorization: if bearer {
                format!("Bearer {token}")
            } else {
                token
            },
            account: b["enterprise_id"].as_str().unwrap_or_default().to_owned(),
        })
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
    ) -> Result<Value> {
        let url = endpoint(&self.root, path, query)?;
        let header = self.authorization.as_str();
        let reply = match method {
            "GET" => self
                .http
                .get(url.as_str())
                .header("Authorization", header)
                .call(),
            "PUT" => self
                .http
                .put(url.as_str())
                .header("Authorization", header)
                .send_json(body),
            "POST" => self
                .http
                .post(url.as_str())
                .header("Authorization", header)
                .send_json(body),
            _ => return Err("invalid: unsupported vendor method".into()),
        }
        .map_err(|_| "vendor: upstream unavailable or connection interrupted")?;
        let mut value = unpack(reply)?;
        if self.provider == "linear" {
            if value["errors"].as_array().is_some_and(|r| !r.is_empty()) {
                return Err("vendor: Linear returned GraphQL errors".into());
            }
            Ok(value
                .get_mut("data")
                .map(Value::take)
                .ok_or("vendor: invalid GraphQL response")?)
        } else if self.provider == "ones" {
            if let Some(batch) = value.as_array().filter(|batch| batch.len() == 1) {
                value = batch[0].clone();
            }
            if method == "GET"
                && path
                    .strip_prefix("/project/issues/")
                    .is_some_and(|key| !key.is_empty() && !key.contains('/'))
                && !query.iter().any(|(k, _)| *k == "action")
                && value["id"].is_string()
                && value.get("result").is_none()
            {
                return Ok(value);
            }
            if value["result"] != "SUCCESS" {
                return Err("vendor: ONES rejected the request".into());
            }
            Ok(value["data"].take())
        } else {
            Ok(value)
        }
    }
    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.request("GET", path, query, &Value::Null)
    }
    fn accounts(&self) -> Result<Vec<Value>> {
        match self.provider.as_str() {
            "linear" => linear::accounts(self),
            "clickup" => clickup::accounts(self),
            _ => ones::accounts(self),
        }
    }
    fn projects(&self, size: usize) -> Result<Vec<Value>> {
        match self.provider.as_str() {
            "linear" => linear::projects(self, size),
            "clickup" => clickup::projects(self),
            _ => ones::projects(self, size),
        }
    }
    fn issues(&self, scope: &str, size: usize, since: Option<i64>) -> Result<Vec<Value>> {
        match self.provider.as_str() {
            "linear" => linear::issues(self, scope, size, since),
            "clickup" => clickup::issues(self, scope, since),
            _ => ones::issues(self, scope, size),
        }
    }
    fn detail(&self, key: &str) -> Result<Value> {
        match self.provider.as_str() {
            "linear" => linear::detail(self, key),
            "clickup" => self.get(&format!("/task/{key}"), &[]),
            _ => ones::detail(self, key),
        }
    }
    fn verify_item(&self, row: &Value, key: &str, scope: &str) -> Result<String> {
        let (project, state) = match self.provider.as_str() {
            "linear" => (&row["project"]["id"], &row["state"]["id"]),
            "clickup" => (&row["list"]["id"], &row["status"]["id"]),
            _ => (&row["project"]["id"], &row["status"]["id"]),
        };
        if id(&row["id"])? != key || id(project)? != scope {
            return Err("forbidden: work item belongs to a different project/List".into());
        }
        if self.provider == "clickup" && state.is_null() {
            return Ok(cred_text(&row["status"], "status")?.to_owned());
        }
        id(state)
    }
    fn publish(&self, plan: &Value, scope: &str) -> std::result::Result<String, PublicationError> {
        let key = id(&plan["key"])?;
        let current = self.verify_item(&self.detail(&key)?, &key, scope)?;
        if plan["intent"] == "comment" {
            let content = cred_text(plan, "body")?;
            let confirm = plan["confirm_comment"].as_str();
            let result = match self.provider.as_str() {
                "linear" => linear::comment(self, &key, content, confirm),
                "clickup" => clickup::comment(self, &key, content, confirm),
                _ => ones::comment(self, &key, content, confirm),
            };
            return result.map_err(|message| PublicationError {
                state: if confirm.is_none()
                    && (message.starts_with("vendor: upstream returned HTTP 4")
                        || message.starts_with("invalid:"))
                {
                    "failed"
                } else {
                    "uncertain"
                },
                message,
            });
        }
        let states = if self.provider == "clickup" {
            Some(clickup::states(self, scope)?)
        } else {
            None
        };
        let canonical = |state: &str| -> Result<String> {
            if let Some(states) = states.as_ref() {
                Ok(clickup::state_name(states, state)?.to_owned())
            } else {
                id(&json!(state))
            }
        };
        let target = canonical(cred_text(plan, "target")?)?;
        let current = canonical(&current)?;
        if current == target {
            return Ok(key);
        }
        if let Some(source) = plan["source_state"].as_str().filter(|s| !s.is_empty())
            && canonical(source)? != current
        {
            return Err(
                "conflict: upstream state changed independently; preview current changes first"
                    .to_owned()
                    .into(),
            );
        }
        match self.provider.as_str() {
            "linear" => linear::status(self, &key, &target)?,
            "clickup" => clickup::status(self, &key, &target)?,
            _ => ones::status(self, &key, &current, &target, plan["workflow"].as_str())?,
        }
        if canonical(&self.verify_item(&self.detail(&key)?, &key, scope)?)? != target {
            return Err("vendor: source did not confirm the new state"
                .to_owned()
                .into());
        }
        Ok(key)
    }
}

fn cred_text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("invalid: missing {key}"))
}

fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    v[key]
        .as_array()
        .ok_or_else(|| format!("vendor: missing {key} collection"))
}
struct Records {
    rows: BTreeMap<String, Value>,
    size: usize,
}
impl Records {
    fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            size: 0,
        }
    }
    fn add(&mut self, row: Value) -> Result<()> {
        let key = id(&row["id"])?;
        if let Some(prior) = self.rows.get(&key) {
            if prior != &row {
                return Err("conflict: source changed while paging; try again".into());
            }
            return Ok(());
        }
        self.size += row.to_string().len();
        if self.rows.len() >= 5000 || self.size > 7_000_000 {
            return Err("vendor: collection exceeds 5000 items or 7 MB".into());
        }
        self.rows.insert(key, row);
        Ok(())
    }
    fn finish(self) -> Vec<Value> {
        self.rows.into_values().collect()
    }
}
fn next_page(page: &Value, seen: &mut HashSet<String>) -> Result<Option<String>> {
    match page["hasNextPage"].as_bool() {
        Some(false) => Ok(None),
        Some(true) => {
            let cursor = page["endCursor"]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() < 2048)
                .ok_or("vendor: missing page cursor")?
                .to_owned();
            if seen.len() >= 5000 || !seen.insert(cursor.clone()) {
                return Err("vendor: pagination repeated a cursor or exceeded the limit".into());
            }
            Ok(Some(cursor))
        }
        None => Err("vendor: missing pagination metadata".into()),
    }
}
fn account_location(
    client: &Client,
    who: &Value,
    b: &Value,
    account: &str,
) -> Result<Option<std::path::PathBuf>> {
    let source = if b["provider"] == "ones" {
        site(b["ones_url"].as_str().unwrap_or_default())?
    } else {
        String::new()
    };
    vault::location(
        client,
        who,
        b,
        b["provider"].as_str().unwrap_or_default(),
        &json!([source, account, b["cloud_auth"].as_str().unwrap_or("key")]).to_string(),
    )
}
fn waiting_location(
    client: &Client,
    who: &Value,
    b: &Value,
    q: &Value,
) -> Result<Option<std::path::PathBuf>> {
    vault::location(
        client,
        who,
        b,
        "tracker-account-choice",
        &json!([
            b["provider"],
            b["ones_url"],
            b["cloud_auth"],
            q["space"].as_str().unwrap_or("default"),
            q["id"]
        ])
        .to_string(),
    )
}

pub(super) fn execute(client: &mut Client, q: &Value, b: &Value) -> Result<Value> {
    let provider = cred_text(b, "provider")?;
    let op = cred_text(q, "op")?;
    let action = op
        .split_once('.')
        .map(|(_, suffix)| suffix)
        .ok_or("invalid: select tracker operation")?;
    if !op.starts_with(&format!("{provider}."))
        && !matches!(op, "imports.synchronize" | "imports.push")
    {
        return Err("invalid: operation does not match this connector".into());
    }
    let supplied = ["access_token", "code"]
        .iter()
        .any(|k| q[*k].as_str().is_some_and(|v| !v.is_empty()));
    let who = identity(
        client,
        !matches!(action, "projects" | "accounts") || (action == "accounts" && supplied),
    )?;
    let bound = b["enterprise_id"].as_str().unwrap_or("");
    if op == "imports.synchronize" && b["sync_paused"] == true {
        return Ok(json!({"state":"paused","batch":null}));
    }
    if action == "authorize" {
        return oauth::start(client, &who, b, q);
    }
    if action == "credential" && q["clear"] == true {
        vault::save(&waiting_location(client, &who, b, q)?, None)?;
        if bound.is_empty() {
            return Ok(json!({"saved":false}));
        }
        return vault::save(&account_location(client, &who, b, bound)?, None);
    }
    let size = q
        .get("page_size")
        .map(|s| {
            s.as_u64()
                .filter(|n| (1..=100).contains(n))
                .ok_or("invalid: page_size must be 1–100")
        })
        .transpose()?
        .unwrap_or(50) as usize;
    let entering = matches!(action, "credential" | "accounts") && supplied;
    let mut values = if entering {
        if q["code"].as_str().is_some_and(|s| !s.is_empty()) {
            oauth::exchange(client, &who, b, q)?
        } else {
            json!({"access_token":cred(q,"access_token")?})
        }
    } else if bound.is_empty() || action == "credential" {
        let mut pending = vault::read(&waiting_location(client, &who, b, q)?)?;
        if pending["expires"]
            .as_i64()
            .is_some_and(|until| until >= chrono::Utc::now().timestamp())
        {
            pending["values"].take()
        } else if bound.is_empty() {
            return Err("invalid: enter a credential to discover available accounts".into());
        } else {
            vault::read(&account_location(client, &who, b, bound)?)?
        }
    } else {
        vault::read(&account_location(client, &who, b, bound)?)?
    };
    let mut api = Cloud::new(b, &values)?;
    let accounts = match api.accounts() {
        Err(err)
            if !entering
                && provider == "linear"
                && err == "vendor: upstream returned HTTP 401"
                && values["refresh_token"].is_string() =>
        {
            values = oauth::refresh(&values)?;
            api = Cloud::new(b, &values)?;
            let result = api.accounts()?;
            if !result.iter().any(|x| x["id"] == bound) {
                return Err("forbidden: renewed token belongs to another organization".into());
            }
            vault::save(&account_location(client, &who, b, bound)?, Some(&values))?;
            result
        }
        out => out?,
    };
    if entering && (action == "accounts" || (bound.is_empty() && action == "credential")) {
        vault::save(
            &waiting_location(client, &who, b, q)?,
            Some(&json!({"values":values,"expires":chrono::Utc::now().timestamp()+600})),
        )?;
    }
    if action == "accounts" {
        return Ok(json!({"provider":provider,"accounts":accounts}));
    }
    let chosen = q["account_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or(Some(bound).filter(|s| !s.is_empty()))
        .or_else(|| {
            if accounts.len() == 1 {
                accounts[0]["id"].as_str()
            } else {
                None
            }
        })
        .ok_or("invalid: select a discovered provider account ID")?
        .to_owned();
    if !bound.is_empty() && chosen != bound || !accounts.iter().any(|a| a["id"] == chosen) {
        return Err("forbidden: credential does not authorize this bound account".into());
    }
    api.account = chosen.clone();
    if action == "credential" {
        if bound.is_empty() {
            let mut command = scoped(q, "update");
            command["kind"] = json!("connector");
            command["expected_rev"] = b["revision"].clone();
            command["fields"] = json!({"enterprise_id":chosen});
            client.execute(command)?;
        }
        let mut receipt = vault::save(&account_location(client, &who, b, &chosen)?, Some(&values))?;
        vault::save(&waiting_location(client, &who, b, q)?, None)?;
        receipt["account_id"] = json!(chosen);
        return Ok(receipt);
    }
    let projects = api.projects(size)?;
    if action == "projects" {
        return Ok(json!({"provider":provider,"account_id":chosen,"projects":projects}));
    }
    let scope = id(&b["project"]).map_err(|_| "invalid: select a source project or List first")?;
    if !projects.iter().any(|p| p["id"] == scope) {
        return Err("forbidden: project or List is outside the authenticated scope".into());
    }
    if op == "imports.push" {
        return publish::with_lease(client, q, |plan| api.publish(plan, &scope));
    }
    let polling = op == "imports.synchronize";
    let mode = if polling {
        "changes"
    } else {
        q["import_mode"].as_str().unwrap_or("history")
    };
    if !matches!(mode, "history" | "changes" | "refresh") {
        return Err("invalid: unknown import mode".into());
    }
    let since = if mode == "changes" && provider != "ones" {
        b["since"].as_i64()
    } else {
        None
    };
    let rows = api.issues(&scope, size, since)?;
    let mut result = stage_batch(client, q, b, &scope, rows, mode, polling)?;
    result["since"] = json!(since);
    if let Some(revision) = b.get("signal_revision") {
        result["signal_revision"] = revision.clone();
    }
    Ok(result)
}
