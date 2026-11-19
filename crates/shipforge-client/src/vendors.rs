use super::Client;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use url::{Host, Url};
mod tapd;
mod trackers;
mod vault;
pub(crate) const CLOUD_OPERATIONS: &[&str] = &[
    "linear.credential",
    "linear.accounts",
    "linear.projects",
    "linear.preview",
    "linear.authorize",
    "clickup.credential",
    "clickup.accounts",
    "clickup.projects",
    "clickup.preview",
    "clickup.authorize",
    "ones.credential",
    "ones.accounts",
    "ones.projects",
    "ones.preview",
];

type Result<T> = std::result::Result<T, String>;

fn origin(official: &str) -> Result<String> {
    let Ok(test) = std::env::var("SHIPFORGE_VENDOR_LOOPBACK") else {
        return Ok(official.into());
    };
    let parsed = Url::parse(&test).map_err(|_| "invalid: invalid vendor loopback fixture")?;
    let local = matches!(parsed.host(),Some(Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(parsed.host(),Some(Host::Ipv6(ip)) if ip.is_loopback());
    if !local
        || parsed.scheme() != "http"
        || parsed.path() != "/"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("invalid: vendor fixture must be an HTTP loopback IP origin".into());
    }
    Ok(parsed.as_str().trim_end_matches('/').into())
}

fn valid_secret(v: &str) -> bool {
    !v.is_empty() && v.len() <= 2048 && v.bytes().all(|b| b.is_ascii_graphic())
}

fn secret(saved: &Value, key: &str, name: &str) -> Result<String> {
    saved[key]
        .as_str()
        .map(str::to_owned)
        .or_else(|| std::env::var(name).ok())
        .filter(|v| valid_secret(v))
        .ok_or_else(|| format!("invalid: save the credential or configure {name} on this machine"))
}

fn input(q: &Value, key: &str) -> Result<String> {
    q[key]
        .as_str()
        .filter(|v| valid_secret(v))
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid: a valid {key} is required"))
}

fn identity(client: &mut Client, write: bool) -> Result<Value> {
    let who = client.execute(json!({"op":"access.whoami"}))?;
    if write && who["scope"] != "write" {
        return Err("forbidden: a writable user session is required".into());
    }
    Ok(who)
}

fn scoped(q: &Value, op: &str) -> Value {
    json!({"op":op,"space":q["space"].as_str().unwrap_or("default"),"id":q["id"],"allow_cached":false})
}

fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .new_agent()
}

fn response(mut res: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("vendor: upstream returned HTTP {status}"));
    }
    if status == 204 {
        return Ok(Value::Null);
    }
    res.body_mut()
        .with_config()
        .limit(8_388_608)
        .read_json()
        .map_err(|_| "vendor: upstream returned invalid JSON".into())
}

struct Zoho {
    http: ureq::Agent,
    root: String,
    token: String,
    size: usize,
}

fn id(row: &Value) -> Result<String> {
    let value = row
        .get("id_string")
        .filter(|v| v.is_string())
        .unwrap_or(&row["id"]);
    let id = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))
        .ok_or("vendor: Zoho returned an invalid resource ID")?;
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("vendor: invalid Zoho resource ID".into());
    }
    Ok(id)
}

impl Zoho {
    fn get(&self, path: &str) -> Result<Value> {
        let res = self
            .http
            .get(format!("{}{path}", self.root))
            .header("Authorization", format!("Zoho-oauthtoken {}", self.token))
            .header("Accept", "application/json")
            .call()
            .map_err(|_| "vendor: Zoho API connection failed")?;
        let data = response(res)?;
        if data.get("error").is_some() {
            return Err("vendor: Zoho rejected the request".into());
        }
        Ok(data)
    }

    fn page(&self, path: &str, group: &str, parameters: &str) -> Result<Vec<Value>> {
        let mut items = BTreeMap::new();
        let mut offset = 1;
        let mut bytes = 0;
        loop {
            let response = self.get(&format!(
                "{path}?index={offset}&range={}{}",
                self.size, parameters
            ))?;
            if response.is_null() {
                break;
            }
            let rows = response[group]
                .as_array()
                .ok_or("vendor: missing Zoho result array")?;
            let previous = items.len();
            if rows.len() > self.size {
                return Err("vendor: Zoho returned more than the requested page size".into());
            }
            for row in rows {
                let key = id(row)?;
                if let Some(old) = items.get(&key) {
                    if old != row {
                        return Err(
                            "vendor: Zoho changed a record during pagination; preview again".into(),
                        );
                    }
                } else {
                    bytes += row.to_string().len();
                    if bytes > 7_000_000 || items.len() >= 5000 {
                        return Err("vendor: Zoho batch exceeds 5000 records or 7 MB".into());
                    }
                    items.insert(key, row.clone());
                }
            }
            if rows.len() < self.size {
                break;
            }
            if items.len() == previous {
                return Err("vendor: Zoho pagination repeated a full page".into());
            }
            offset += rows.len();
        }
        Ok(items.into_values().collect())
    }
}

fn zoho(client: &mut Client, q: &Value) -> Result<Value> {
    let binding = client.execute(scoped(q, "imports.binding"))?;
    let portal = binding["portal_id"]
        .as_str()
        .ok_or("invalid: Zoho portal is required")?;
    let who = identity(client, q["op"] != "zoho.projects")?;
    let location = vault::location(client, &who, &binding, "zoho", portal)?;
    let saving = q["op"] == "zoho.credential";
    if saving && q["clear"] == true {
        return vault::save(&location, None);
    }
    let token = if saving {
        input(q, "access_token")?
    } else {
        secret(
            &vault::read(&location)?,
            "token",
            &format!("SHIPFORGE_ZOHO_TOKEN_{portal}"),
        )?
    };
    let size = q
        .get("page_size")
        .map(|v| {
            v.as_u64()
                .filter(|n| (1..=100).contains(n))
                .ok_or("invalid: page_size must be 1–100")
        })
        .transpose()?
        .unwrap_or(100) as usize;
    let api = Zoho {
        http: http(),
        root: origin("https://bugtracker.zoho.com")?,
        token,
        size,
    };
    let portals = api.get("/restapi/portals/")?;
    let allowed = portals["portals"]
        .as_array()
        .ok_or("vendor: Zoho returned no portals collection")?
        .iter()
        .any(|r| id(r).is_ok_and(|v| v == portal));
    if !allowed {
        return Err("forbidden: Zoho token cannot access the configured portal".into());
    }
    if saving {
        return vault::save(&location, Some(&json!({"token":api.token})));
    }
    let projects = format!("/restapi/portal/{portal}/projects/");
    if q["op"] == "zoho.projects" {
        let mut projects_by_id = BTreeMap::new();
        for status in ["active", "archived"] {
            for p in api.page(&projects, "projects", &format!("&status={status}"))? {
                let id = id(&p)?;
                projects_by_id.insert(
                    id.clone(),
                    json!({"id":id,"name":p["name"],"status":p["status"]}),
                );
            }
        }
        return Ok(
            json!({"portal":portal,"projects":projects_by_id.into_values().collect::<Vec<_>>() }),
        );
    }
    let project = binding["project"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("invalid: select a discovered Zoho project in the connector")?;
    let detail = api.get(&format!("{projects}{project}/"))?;
    if !detail["projects"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|p| id(p).is_ok_and(|v| v == project)))
    {
        return Err("forbidden: Zoho project is unavailable".into());
    }
    let since = if q["full"] == true {
        None
    } else {
        binding["since"].as_i64()
    };
    let mut filter = "&sort_column=last_modified_time&sort_order=ascending".to_owned();
    if let Some(since) = since {
        filter.push_str(&format!("&last_modified_time={since}"));
    }
    let path = format!("{projects}{project}/bugs/");
    let mut issues = api.page(&path, "bugs", &filter)?;
    for issue in &mut issues {
        if issue.get("description").is_none() {
            let key = id(issue)?;
            let detail = api.get(&format!("{path}{key}/"))?;
            let bug = detail["bugs"]
                .as_array()
                .and_then(|rows| rows.iter().find(|v| id(v).is_ok_and(|k| k == key)))
                .ok_or("vendor: Zoho issue detail does not match the requested ID")?;
            let value = issue.as_object_mut().ok_or("vendor: invalid Zoho issue")?;
            for (k, v) in bug.as_object().ok_or("vendor: invalid Zoho issue detail")? {
                value.insert(k.clone(), v.clone());
            }
        }
    }
    let mut stage = scoped(q, "imports.stage");
    stage["issues"] = json!(issues);
    stage["project"] = json!(project);
    stage["binding_rev"] = binding["revision"].clone();
    stage["checkpoint"] = binding["checkpoint"].clone();
    let mut preview = client.execute(stage)?;
    preview["since"] = json!(since);
    Ok(preview)
}

fn deliver(client: &mut Client, q: &Value) -> Result<Value> {
    let binding = client.execute(scoped(q, "bugly.binding"))?;
    let product = binding["product_id"]
        .as_str()
        .ok_or("invalid: missing Bugly product ID")?;
    let who = identity(client, true)?;
    let location = vault::location(client, &who, &binding, "bugly", product)?;
    if q["op"] == "bugly.credentials" {
        if q["clear"] == true {
            return vault::save(&location, None);
        }
        return vault::save(
            &location,
            Some(
                &json!({"api_id":input(q,"api_id")?,"api_key":input(q,"api_key")?,"product_key":input(q,"product_key")?}),
            ),
        );
    }
    let saved = vault::read(&location)?;
    let base = origin("https://api.bugly.tds.qq.com")?;
    let api_id = secret(&saved, "api_id", "SHIPFORGE_BUGLY_API_ID")?;
    let key = secret(&saved, "api_key", "SHIPFORGE_BUGLY_API_KEY")?;
    let product_key = secret(
        &saved,
        "product_key",
        &format!("SHIPFORGE_BUGLY_PRODUCT_KEY_{product}"),
    )?;
    let limit = q
        .get("limit")
        .map(|v| {
            v.as_u64()
                .filter(|n| (1..=100).contains(n))
                .ok_or("invalid: delivery limit must be 1–100")
        })
        .transpose()?
        .unwrap_or(20);
    let api = http();
    let mut sent = 0;
    for _ in 0..limit {
        let work = client.execute(scoped(q, "bugly.claim"))?;
        if work.is_null() {
            break;
        }
        let payload = &work["payload"];
        let submit = || -> Result<()> {
            if payload["product_id"] != product {
                return Err("invalid: Bugly product binding changed".into());
            }
            let bytes = serde_json::to_vec(payload)
                .map_err(|_| "invalid: Bugly request encoding failed")?;
            let auth = shipforge_core::bugly_authorization(&api_id, &key, &bytes)?;
            let res = api
                .post(format!("{base}/v1/issue/modify_issue_status"))
                .header("Authorization", auth)
                .header("X-ProductId", product)
                .header("X-ProductKey", &product_key)
                .header("Content-Type", "application/json")
                .send(bytes.as_slice())
                .map_err(|_| "vendor: Bugly API connection failed")?;
            let ack = response(res)?;
            if ack["baseRsp"]["code"].as_i64() != Some(0) {
                return Err("vendor: Bugly did not acknowledge the status change".into());
            }
            Ok(())
        }();
        let mut settle = scoped(q, "bugly.settle");
        settle["success"] = json!(submit.is_ok());
        settle["delivery"] = work["delivery"].clone();
        settle["lease"] = work["lease"].clone();
        client.execute(settle)?;
        submit?;
        sent += 1;
    }
    Ok(json!({"sent":sent}))
}

pub(super) fn execute(client: &mut Client, q: &Value) -> Result<Value> {
    if q["op"].as_str().is_some_and(|s| {
        CLOUD_OPERATIONS.contains(&s)
            || s.starts_with("zentao.")
            || s.starts_with("pingcode.")
            || matches!(s, "imports.push" | "imports.synchronize")
    }) {
        return trackers::execute(client, q);
    }
    if q["op"].as_str().is_some_and(|s| s.starts_with("tapd.")) {
        return tapd::execute(client, q);
    }
    if q["id"].as_str().is_none_or(str::is_empty) {
        return Err("invalid: connector id is required".into());
    }
    if q["op"].as_str().is_some_and(|op| op.starts_with("bugly.")) {
        deliver(client, q)
    } else {
        zoho(client, q)
    }
}
