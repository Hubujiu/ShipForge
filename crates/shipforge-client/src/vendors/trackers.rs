use super::{Client, Result, http, identity, origin, scoped, vault};
use serde_json::{Value, json};
use shipforge_core::zentao_installation as site;
use std::collections::BTreeMap;
use url::Url;

mod cloud;
mod pingcode;
mod publish;
mod zentao;

struct Api {
    http: ureq::Agent,
    root: String,
    token: String,
    zen: bool,
}

fn id(v: &Value) -> Result<String> {
    let value = v
        .as_str()
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|v| v.to_string()))
        .ok_or("vendor: invalid resource identifier")?;
    if value.is_empty()
        || value.len() > 160
        || matches!(value.as_str(), "." | "..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("vendor: invalid resource identifier".into());
    }
    Ok(value)
}

fn cred(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid: configure {key} on this device"))
}

fn endpoint(root: &str, path: &str, params: &[(&str, String)]) -> Result<Url> {
    let mut url =
        Url::parse(&format!("{root}{path}")).map_err(|_| "invalid: invalid vendor address")?;
    if !params.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
    }
    Ok(url)
}

fn unpack(mut r: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let code = r.status().as_u16();
    if !(200..300).contains(&code) {
        return Err(format!("vendor: upstream returned HTTP {code}"));
    }
    if code == 204 {
        return Ok(Value::Null);
    }
    let data: Value = r
        .body_mut()
        .with_config()
        .limit(8_388_608)
        .read_json()
        .map_err(|_| "vendor: upstream response is not supported API JSON")?;
    if data["status"] == "fail"
        || data["error"].is_object()
        || data["error_code"].as_i64().is_some_and(|v| v != 0)
    {
        return Err("vendor: upstream rejected the request".into());
    }
    Ok(data)
}

impl Api {
    fn auth(&self) -> (&str, String) {
        if self.zen {
            ("Token", self.token.clone())
        } else {
            ("Authorization", format!("Bearer {}", self.token))
        }
    }
    fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value> {
        let url = endpoint(&self.root, path, params)?;
        let (header, value) = self.auth();
        let response = self
            .http
            .get(url.as_str())
            .header(header, value)
            .header("Accept", "application/json")
            .call()
            .map_err(|_| "vendor: unable to reach the configured API")?;
        unpack(response)
    }
    fn update(&self, method: &str, path: &str, body: &Value) -> Result<Value> {
        let url = endpoint(&self.root, path, &[])?;
        let (header, value) = self.auth();
        let res = if method == "PATCH" {
            self.http.patch(url.as_str())
        } else {
            self.http.post(url.as_str())
        };
        unpack(
            res.header(header, value)
                .send_json(body)
                .map_err(|_| "uncertain: network interrupted the publication")?,
        )
    }
    fn pages(
        &self,
        path: &str,
        key: &str,
        query: &[(&str, String)],
        size: usize,
    ) -> Result<Vec<Value>> {
        let mut items = BTreeMap::new();
        let mut bytes = 0;
        let mut offset = 0usize;
        let mut page = usize::from(self.zen);
        loop {
            let mut args = query.to_vec();
            args.push((
                if self.zen { "limit" } else { "page_size" },
                size.to_string(),
            ));
            args.push((
                if self.zen { "page" } else { "page_index" },
                page.to_string(),
            ));
            let data = self.get(path, &args)?;
            let reported = data[if self.zen { "page" } else { "page_index" }]
                .as_u64()
                .or_else(|| {
                    data[if self.zen { "page" } else { "page_index" }]
                        .as_str()?
                        .parse()
                        .ok()
                });
            if reported.is_some_and(|n| n != page as u64) {
                return Err("vendor: incorrect response page".into());
            }
            let rows = data[key]
                .as_array()
                .ok_or("vendor: missing paginated records")?;
            let total = data["total"]
                .as_u64()
                .or_else(|| data["total"].as_str()?.parse().ok());
            let previous = items.len();
            for row in rows {
                let key = id(&row["id"])?;
                if let Some(old) = items.get(&key) {
                    if old != row {
                        return Err(
                            "conflict: source changed a record during pagination; preview again"
                                .into(),
                        );
                    }
                } else {
                    bytes += row.to_string().len();
                    if bytes > 7_000_000 || items.len() >= 5000 {
                        return Err("vendor: batch exceeds 5000 entries or 7 MB".into());
                    }
                    items.insert(key, row.clone());
                }
            }
            offset += rows.len();
            if total.is_some_and(|n| offset as u64 >= n) {
                if total != Some(items.len() as u64) {
                    return Err("conflict: source changed during pagination; preview again".into());
                }
                break;
            }
            if total.is_none() && rows.len() < size {
                break;
            }
            if previous == items.len() {
                return Err("vendor: incomplete or repeated source page".into());
            }
            page += 1;
        }
        Ok(items.into_values().collect())
    }
}

fn credentials(
    client: &Client,
    who: &Value,
    b: &Value,
    zen: bool,
    q: &Value,
) -> Result<(Api, Option<std::path::PathBuf>, Value)> {
    let provider = if zen { "zentao" } else { "pingcode" };
    let external = if zen {
        site(
            b["instance_url"]
                .as_str()
                .ok_or("invalid: missing ZenTao installation root")?,
        )?
    } else {
        id(&b["enterprise_id"])?
    };
    let resource = credential_resource(b, q, &external);
    let loc = vault::location(client, who, b, provider, &resource)?;
    let saving = q["op"]
        .as_str()
        .is_some_and(|op| op.ends_with(".credential"));
    let saved = if saving {
        q.clone()
    } else {
        vault::read(&loc)?
    };
    if zen {
        let (api, values) = zentao::connect(&external, &saved)?;
        return Ok((api, loc, values));
    }
    let (api, values) = pingcode::connect(b, &saved, saving, false)?;
    match pingcode::check_enterprise(&api, b) {
        Ok(()) => Ok((api, loc, values)),
        Err(err)
            if !saving
                && err == "vendor: upstream returned HTTP 401"
                && matches!(
                    b["pingcode_auth"].as_str(),
                    Some("application" | "authorization")
                ) =>
        {
            let current = vault::read(&loc)?;
            let (api, values) = if current["access_token"] != saved["access_token"] {
                let (api, _) = pingcode::connect(b, &current, false, false)?;
                (api, current)
            } else {
                pingcode::connect(b, &current, true, true)?
            };
            pingcode::check_enterprise(&api, b)?;
            vault::save(&loc, Some(&values))?;
            Ok((api, loc, values))
        }
        Err(err) => Err(err),
    }
}

fn credential_resource(binding: &Value, q: &Value, external: &str) -> String {
    json!([
        external,
        q["space"].as_str().unwrap_or("default"),
        q["id"],
        binding["pingcode_auth"]
    ])
    .to_string()
}

pub(super) fn execute(client: &mut Client, q: &Value) -> Result<Value> {
    let op = q["op"].as_str().ok_or("invalid: no operation")?;
    let mut request = scoped(q, "imports.binding");
    request["clear_device_credentials"] = json!(op.ends_with(".credential") && q["clear"] == true);
    let binding = client.execute(request)?;
    let provider = binding["provider"]
        .as_str()
        .ok_or("invalid: select a supported tracker")?;
    if matches!(provider, "linear" | "clickup" | "ones") {
        return cloud::execute(client, q, &binding);
    }
    let zen = match provider {
        "zentao" => true,
        "pingcode" => false,
        _ => return Err("invalid: unsupported tracker connector".into()),
    };
    if !matches!(op, "imports.push" | "imports.synchronize")
        && !op.starts_with(&format!("{provider}."))
    {
        return Err("invalid: provider differs from the selected connector".into());
    }
    let who = identity(client, !op.ends_with(".projects"))?;
    if op.ends_with(".credential") && q["clear"] == true {
        let external = if zen {
            site(binding["instance_url"].as_str().unwrap_or_default())?
        } else {
            id(&binding["enterprise_id"])?
        };
        let resource = credential_resource(&binding, q, &external);
        let loc = vault::location(client, &who, &binding, provider, &resource)?;
        return vault::save(&loc, None);
    }
    let (api, loc, values) = credentials(client, &who, &binding, zen, q)?;
    let size = q
        .get("page_size")
        .map(|v| {
            v.as_u64()
                .filter(|v| (1..=100).contains(v))
                .ok_or("invalid: page_size must be 1–100")
        })
        .transpose()?
        .unwrap_or(50) as usize;
    let projects = if zen {
        zentao::projects(&api, &binding, size)?
    } else {
        pingcode::projects(&api, size)?
    };
    if op.ends_with(".credential") {
        return vault::save(&loc, Some(&values));
    }
    if op.ends_with(".projects") {
        return Ok(json!({"provider":provider,"projects":projects}));
    }
    let scope = id(&binding["project"])
        .map_err(|_| "invalid: select a discovered project or product first")?;
    if !projects.iter().any(|p| p["id"] == scope) {
        return Err("forbidden: selected project is outside upstream permissions".into());
    }
    if op == "imports.push" {
        return publish::run(client, q, &api, &binding);
    }
    let polling = op == "imports.synchronize";
    let mode = if polling {
        "changes"
    } else {
        q["import_mode"].as_str().unwrap_or("history")
    };
    if !matches!(mode, "history" | "refresh" | "changes") {
        return Err("invalid: unknown import mode".into());
    }
    let since = if mode == "changes" {
        binding["since"].as_i64()
    } else {
        None
    };
    let issues = if zen {
        zentao::issues(&api, &binding, size)?
    } else {
        pingcode::issues(&api, &scope, size, since)?
    };
    let mut result = stage_batch(client, q, &binding, &scope, issues, mode, polling)?;
    result["since"] = if zen { Value::Null } else { json!(since) };
    Ok(result)
}

fn stage_batch(
    client: &mut Client,
    q: &Value,
    binding: &Value,
    scope: &str,
    issues: Vec<Value>,
    mode: &str,
    polling: bool,
) -> Result<Value> {
    let mut stage = scoped(q, "imports.stage");
    stage["project"] = json!(scope);
    stage["issues"] = json!(issues);
    stage["binding_rev"] = binding["revision"].clone();
    stage["checkpoint"] = binding["checkpoint"].clone();
    stage["import_mode"] = json!(mode);
    stage["source_account"] = binding["enterprise_id"].clone();
    stage["poll"] = json!(polling);
    let mut result = client.execute(stage)?;
    if polling && result["state"] == "preview" {
        let mut commit = scoped(q, "imports.commit");
        commit["batch"] = result["batch"].clone();
        result = client.execute(commit)?;
    }
    result["mode"] = json!(mode);
    Ok(result)
}
