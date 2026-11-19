use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

struct Tapd {
    http: ureq::Agent,
    base: String,
    header: String,
}

impl Tapd {
    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut url = Url::parse(&format!("{}/{path}", self.base))
            .map_err(|_| "invalid: invalid TAPD address")?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        for attempt in 0..3 {
            let res = self
                .http
                .get(url.as_str())
                .header("Authorization", &self.header)
                .header("Accept", "application/json")
                .call()
                .map_err(|_| "vendor: TAPD API connection failed")?;
            if res.status().as_u16() == 429 {
                let seconds = res
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(60);
                if attempt == 2 || seconds > 60 {
                    return Err(format!(
                        "rate_limited: TAPD quota unavailable; retry after {seconds} seconds"
                    ));
                }
                std::thread::sleep(Duration::from_secs(seconds));
                continue;
            }
            let envelope = response(res)?;
            if envelope["status"].as_i64() != Some(1) {
                return Err("vendor: TAPD rejected the request".into());
            }
            return Ok(envelope["data"].clone());
        }
        unreachable!("rate-limited requests exit on the final attempt")
    }

    fn projects(&self, binding: &Value) -> Result<Vec<Value>> {
        let data = if binding["tapd_auth"] == "company" {
            let company = binding["company_id"]
                .as_str()
                .ok_or("invalid: missing TAPD company ID")?;
            self.get(
                "workspaces/projects",
                &[
                    ("company_id", company.into()),
                    ("category", "project".into()),
                ],
            )?
        } else {
            let user = self.get("users/info", &[])?;
            let nick = user["nick"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("vendor: TAPD token did not identify its user")?;
            self.get(
                "workspaces/user_participant_projects",
                &[("nick", nick.into())],
            )?
        };
        let mut projects = Vec::new();
        for entry in data
            .as_array()
            .ok_or("vendor: TAPD projects collection is missing")?
        {
            let item = &entry["Workspace"];
            if item["category"] == "organization" {
                continue;
            }
            projects.push(json!({"id":id(item)?,"name":item["name"],"status":item["status"],"ended":item["status"]=="closed"}));
        }
        Ok(projects)
    }

    fn page(&self, project: &str, size: usize, since: Option<&str>) -> Result<Vec<Value>> {
        let mut rows = BTreeMap::new();
        let mut bytes = 0;
        for (resource, kind) in [("stories", "Story"), ("bugs", "Bug")] {
            for page in 1.. {
                let mut query = vec![
                    ("workspace_id", project.to_owned()),
                    ("limit", size.to_string()),
                    ("page", page.to_string()),
                    ("order", "modified ASC".to_owned()),
                ];
                if let Some(since) = since {
                    query.push(("modified", format!(">{since}")));
                }
                let data = self.get(resource, &query)?;
                let items = data.as_array().ok_or("vendor: missing TAPD result array")?;
                if items.len() > size {
                    return Err(
                        "vendor: TAPD returned too many records for the requested page".into(),
                    );
                }
                let previous = rows.len();
                for item in items {
                    let key = (kind, id(&item[kind])?);
                    if let Some(old) = rows.get(&key) {
                        if old != item {
                            return Err(
                                "vendor: TAPD changed a record during pagination; preview again"
                                    .into(),
                            );
                        }
                    } else {
                        bytes += item.to_string().len();
                        if rows.len() >= 5000 || bytes > 7_000_000 {
                            return Err("vendor: TAPD batch exceeds 5000 records or 7 MB".into());
                        }
                        rows.insert(key, item.clone());
                    }
                }
                if items.len() < size {
                    break;
                }
                if rows.len() == previous {
                    return Err("vendor: TAPD pagination repeated a full page".into());
                }
            }
        }
        Ok(rows.into_values().collect())
    }
}

pub(super) fn execute(client: &mut Client, q: &Value) -> Result<Value> {
    let binding = client.execute(scoped(q, "imports.binding"))?;
    if binding["provider"] != "tapd" {
        return Err("invalid: select a TAPD connector".into());
    }
    let personal = binding["tapd_auth"] == "personal";
    let who = identity(client, q["op"] != "tapd.projects")?;
    let scope = binding["company_id"]
        .as_str()
        .filter(|_| !personal)
        .unwrap_or("personal");
    let location = vault::location(client, &who, &binding, "tapd", scope)?;
    let saving = q["op"] == "tapd.credential";
    if saving && q["clear"] == true {
        return vault::save(&location, None);
    }
    let credentials = if saving {
        if personal {
            json!({"token":input(q,"access_token")?})
        } else {
            json!({"user":input(q,"api_user")?,"password":input(q,"api_password")?})
        }
    } else {
        vault::read(&location)?
    };
    let header = if personal {
        format!(
            "Bearer {}",
            secret(&credentials, "token", "SHIPFORGE_TAPD_TOKEN")?
        )
    } else {
        let user = secret(
            &credentials,
            "user",
            &format!("SHIPFORGE_TAPD_USER_{scope}"),
        )?;
        if user.contains(':') {
            return Err("invalid: TAPD API account contains a colon".into());
        }
        let password = secret(
            &credentials,
            "password",
            &format!("SHIPFORGE_TAPD_PASSWORD_{scope}"),
        )?;
        format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
    };
    let api = Tapd {
        http: http(),
        base: origin("https://api.tapd.cn")?,
        header,
    };
    let projects = api.projects(&binding)?;
    if saving {
        return vault::save(&location, Some(&credentials));
    }
    if q["op"] == "tapd.projects" {
        return Ok(json!({"projects":projects,"auth":binding["tapd_auth"]}));
    }
    let project = binding["project"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("invalid: select a TAPD workspace")?;
    if !projects.iter().any(|p| p["id"] == project) {
        return Err("forbidden: TAPD project is outside the credential's permissions".into());
    }
    let mode = q["import_mode"].as_str().unwrap_or("history");
    if !matches!(mode, "history" | "refresh" | "changes") {
        return Err("invalid: unknown TAPD import mode".into());
    }
    let since = if mode == "changes" {
        binding["modified_after"].as_str()
    } else {
        None
    };
    let size = q
        .get("page_size")
        .map(|v| {
            v.as_u64()
                .filter(|n| (1..=200).contains(n))
                .ok_or("invalid: TAPD page_size must be 1–200")
        })
        .transpose()?
        .unwrap_or(100) as usize;
    let issues = api.page(project, size, since)?;
    let mut stage = scoped(q, "imports.stage");
    stage["issues"] = json!(issues);
    stage["import_mode"] = json!(mode);
    stage["project"] = json!(project);
    stage["binding_rev"] = binding["revision"].clone();
    stage["checkpoint"] = binding["checkpoint"].clone();
    let mut preview = client.execute(stage)?;
    preview["modified_after"] = json!(since);
    Ok(preview)
}
