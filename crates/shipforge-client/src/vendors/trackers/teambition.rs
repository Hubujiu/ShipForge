use super::*;
use crate::vendors::{administrator, org_location, suffix};
use std::collections::HashSet;
use uuid::Uuid;

struct Teambition {
    root: String,
    app: String,
    tenant: String,
    operator: Option<String>,
}

fn result(response: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let value = unpack(response)?;
    if value.get("result").is_none()
        || value
            .get("errorMessage")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        || value["code"]
            .as_i64()
            .is_some_and(|code| !matches!(code, 0 | 200))
    {
        return Err("vendor: Teambition rejected the request".into());
    }
    Ok(value)
}

impl Teambition {
    fn connect(root: &str, tenant: &str, config: &Value) -> Result<Self> {
        let body = json!({"appId":cred(config,"app_id")?,"appSecret":cred(config,"app_secret")?});
        let resp = http()
            .post(format!("{root}/appToken"))
            .send_json(body)
            .map_err(|_| "vendor: Teambition token service unavailable")?;
        let app = cred(&result(resp)?["result"], "appToken")?;
        let api = Self {
            root: root.into(),
            app,
            tenant: tenant.into(),
            operator: None,
        };
        if api.request("GET", "/org/info", &[], &Value::Null)?["result"]["orgId"] != tenant {
            return Err(
                "forbidden: application is not installed in the bound Teambition organization"
                    .into(),
            );
        }
        Ok(api)
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
    ) -> Result<Value> {
        let agent = http();
        let url = endpoint(&self.root, path, query)?;
        let auth = format!("Bearer {}", self.app);
        let response = if method == "GET" {
            let mut r = agent
                .get(url.as_str())
                .header("Authorization", &auth)
                .header("X-Tenant-Id", &self.tenant)
                .header("X-Tenant-Type", "organization");
            if let Some(operator) = &self.operator {
                r = r.header("X-Operator-Id", operator);
            }
            r.call()
        } else {
            let mut r = if method == "PUT" {
                agent.put(url.as_str())
            } else {
                agent.post(url.as_str())
            };
            r = r
                .header("Authorization", &auth)
                .header("X-Tenant-Id", &self.tenant)
                .header("X-Tenant-Type", "organization");
            if let Some(operator) = &self.operator {
                r = r.header("X-Operator-Id", operator);
            }
            r.send_json(body)
        }
        .map_err(|_| "vendor: Teambition request interrupted")?;
        result(response)
    }
    fn authorize_person(&mut self, values: &Value) -> Result<()> {
        let token = cred(values, "access_token")?;
        let result = self.request(
            "POST",
            "/oauth/userInfo",
            &[],
            &json!({"userAccessToken":token}),
        )?;
        self.operator = Some(id(&result["result"]["userId"])?);
        if let Some(expected) = values["user_id"].as_str()
            && self.operator.as_deref() != Some(expected)
        {
            return Err("forbidden: Teambition account changed".into());
        }
        Ok(())
    }
    fn pages(&self, path: &str, query: &[(&str, String)], size: usize) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        let mut bytes = 0;
        let mut seen = HashSet::new();
        let mut cursor = String::new();
        loop {
            let mut args = query.to_vec();
            args.push(("pageSize", size.to_string()));
            if !cursor.is_empty() {
                args.push(("pageToken", cursor));
            }
            let page = self.request("GET", path, &args, &Value::Null)?;
            let list = page["result"]
                .as_array()
                .ok_or("vendor: invalid Teambition list")?;
            bytes += page["result"].to_string().len();
            if bytes > 7_000_000 || rows.len() + list.len() > 5000 {
                return Err("vendor: Teambition list exceeds the batch limit".into());
            }
            rows.extend(list.iter().cloned());
            cursor = page["nextPageToken"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if cursor.is_empty() {
                break;
            }
            if !seen.insert(cursor.clone()) || seen.len() > 5000 {
                return Err("vendor: repeated Teambition page token".into());
            }
        }
        Ok(rows)
    }
    fn projects(&self, size: usize) -> Result<Vec<Value>> {
        let joined = self.pages("/v3/project/user-joined", &[], size)?;
        let ids = joined
            .iter()
            .map(id)
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let mut found = Vec::new();
        for chunk in ids.iter().collect::<Vec<_>>().chunks(50) {
            let requested = chunk.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            for p in self.pages(
                "/v3/project/query",
                &[("projectIds", requested.join(","))],
                size,
            )? {
                let key = id(&p["id"])?;
                if p["organizationId"] != self.tenant || !requested.contains(&key.as_str()) {
                    return Err(
                        "forbidden: Teambition project is outside the verified membership".into(),
                    );
                }
                if p["isArchived"] != true && p["isSuspended"] != true {
                    found.push(json!({"id":key,"name":p["name"]}));
                }
            }
        }
        Ok(found)
    }
    fn project(&self, scope: &str) -> Result<()> {
        let p = self.pages("/v3/project/query", &[("projectIds", scope.into())], 50)?;
        if p.len() != 1
            || p[0]["id"] != scope
            || p[0]["organizationId"] != self.tenant
            || p[0]["isArchived"] == true
            || p[0]["isSuspended"] == true
        {
            return Err("forbidden: inactive or foreign Teambition project".into());
        }
        Ok(())
    }
    fn detail(&self, key: &str, scope: &str) -> Result<Option<Value>> {
        let value = self.request(
            "GET",
            "/v3/task/query",
            &[("taskId", key.into())],
            &Value::Null,
        )?;
        let rows = value["result"]
            .as_array()
            .ok_or("vendor: invalid Teambition task lookup")?;
        if rows.is_empty() {
            return Ok(None);
        }
        if rows.len() != 1 || rows[0]["id"] != key || rows[0]["projectId"] != scope {
            return Err("forbidden: Teambition item belongs to another project".into());
        }
        if rows[0]["visible"] != "members" || rows[0]["isArchived"] == true {
            return Err("forbidden: Teambition task is restricted or archived".into());
        }
        Ok(Some(rows[0].clone()))
    }
    fn push(
        &self,
        plan: &Value,
        scope: &str,
    ) -> std::result::Result<String, publish::PublicationError> {
        let key = id(&plan["key"])?;
        let before = self.detail(&key, scope)?;
        let Some(before) = before else {
            if plan["intent"] == "delete" && plan["previous_attempt"] == true {
                return Ok(key);
            }
            return Err("not_found: mapped Teambition task is unavailable"
                .to_string()
                .into());
        };
        if plan["intent"] == "delete" {
            if before["content"] != plan["source_title"]
                || before["note"].as_str().unwrap_or("")
                    != plan["source_description"].as_str().unwrap_or("")
                || before["tfsId"] != plan["source_state"]
            {
                return Err(
                    "conflict: Teambition task changed; preview the current record before deletion"
                        .to_owned()
                        .into(),
                );
            }
            self.request("POST", &format!("/v3/task/{key}/delete"), &[], &json!({}))?;
            if self.detail(&key, scope)?.is_some() {
                return Err("vendor: deletion was not confirmed".to_string().into());
            }
        } else if plan["intent"] == "fields" {
            let fields = [
                ("content", "title", "source_title"),
                ("note", "description", "source_description"),
            ];
            for (native, local, base) in fields {
                let current = before[native].as_str().unwrap_or("");
                if current != plan[base].as_str().unwrap_or("")
                    && current != plan[local].as_str().unwrap_or("")
                {
                    return Err(
                        "conflict: Teambition field changed; preview current changes"
                            .to_string()
                            .into(),
                    );
                }
            }
            for (native, local, _) in fields {
                if before[native].as_str().unwrap_or("") != plan[local].as_str().unwrap_or("") {
                    self.request(
                        "PUT",
                        &format!("/v3/task/{key}/{native}"),
                        &[],
                        &json!({(native):plan[local]}),
                    )?;
                }
            }
            let after = self
                .detail(&key, scope)?
                .ok_or("not_found: task disappeared".to_owned())?;
            if fields.iter().any(|(native, local, _)| {
                after[*native].as_str().unwrap_or("") != plan[*local].as_str().unwrap_or("")
            }) {
                return Err("vendor: Teambition did not confirm the field update"
                    .to_string()
                    .into());
            }
        } else {
            let target = plan["target"]
                .as_str()
                .ok_or("invalid: status mapping required".to_owned())?;
            let states = self.pages(
                &format!("/v3/project/{scope}/taskflowstatus/search"),
                &[],
                50,
            )?;
            let valid: Vec<_> = states.iter().filter(|s| s["isDeleted"] != true).collect();
            let mut matches = valid
                .iter()
                .filter(|s| s["id"] == target)
                .copied()
                .collect::<Vec<_>>();
            if matches.is_empty() {
                matches = valid
                    .iter()
                    .filter(|s| s["name"] == target)
                    .copied()
                    .collect();
            }
            if matches.len() != 1 {
                return Err(
                    "invalid: mapped Teambition state must identify one available state"
                        .to_owned()
                        .into(),
                );
            }
            let target = id(&matches[0]["id"])?;
            if before["tfsId"] == target {
                return Ok(key);
            }
            let mut previous = valid.iter().filter(|s| s["name"] == plan["source_state"]);
            let known_name = previous
                .next()
                .is_some_and(|state| state["id"] == before["tfsId"])
                && previous.next().is_none();
            if before["tfsId"] != plan["source_state"] && !known_name {
                return Err("conflict: Teambition status changed; preview first"
                    .to_owned()
                    .into());
            }
            self.request(
                "PUT",
                &format!("/v3/task/{key}/taskflowstatus"),
                &[],
                &json!({"taskflowstatusId":target}),
            )?;
            if self
                .detail(&key, scope)?
                .is_none_or(|v| v["tfsId"] != target)
            {
                return Err("vendor: Teambition did not confirm the status"
                    .to_owned()
                    .into());
            }
        }
        Ok(key)
    }
}

pub(in crate::vendors) fn execute(client: &mut Client, op: &str, q: &Value) -> Result<Value> {
    let mut ask = scoped(q, "imports.binding");
    ask["clear_device_credentials"] = json!(q["clear"] == true);
    let b = client.execute(ask)?;
    if b["provider"] != "teambition" {
        return Err("invalid: select a Teambition connector".into());
    }
    let space = q["space"].as_str().unwrap_or("default");
    let tenant = id(&b["enterprise_id"])?;
    let who = identity(client, true)?;
    let app_path = org_location(client, &b, "teambition-app", &tenant)?;
    let person_path = vault::location(client, &who, &b, "teambition-personal", &tenant)?;
    let root = format!("{}/api", origin("https://open.teambition.com")?);
    if op == "teambition.application" {
        administrator(
            client,
            b["organization"]
                .as_str()
                .ok_or("invalid: missing organization")?,
        )?;
        if q["clear"] == true {
            return vault::save(&app_path, None);
        }
        let config = json!({"app_id":cred(q,"app_id")?,"app_secret":cred(q,"app_secret")?});
        Teambition::connect(&root, &tenant, &config)?;
        let mut saved = vault::save(&app_path, Some(&config))?;
        saved["suffix"] = json!(suffix(config["app_secret"].as_str().unwrap()));
        return Ok(saved);
    }
    if op == "teambition.personal" && q["clear"] == true {
        return vault::save(&person_path, None);
    }
    if matches!(op, "teambition.synchronize" | "teambition.preview") {
        administrator(client, space)?;
    }
    if op == "teambition.synchronize" && b["sync_paused"] == true {
        return Ok(json!({"state":"paused","batch":null}));
    }
    let config = vault::read(&app_path)?;
    let mut api = Teambition::connect(&root, &tenant, &config)?;
    let pending = vault::location(client, &who, &b, "teambition-oauth-state", &tenant)?;
    if op == "teambition.authorize" {
        let redirect =
            Url::parse(&cred(q, "redirect_uri")?).map_err(|_| "invalid: invalid callback URL")?;
        if !(redirect.scheme() == "https"
            || redirect.scheme() == "http"
                && matches!(
                    redirect.host_str(),
                    Some("127.0.0.1" | "localhost" | "[::1]")
                ))
            || !redirect.username().is_empty()
            || redirect.password().is_some()
            || redirect.fragment().is_some()
        {
            return Err("invalid: use a registered HTTPS or local callback URL".into());
        }
        let state = Uuid::new_v4().simple().to_string();
        let url = endpoint(
            "https://open.teambition.com/api",
            "/oauth/sso",
            &[
                ("app_id", cred(&config, "app_id")?),
                ("redirect_uri", redirect.to_string()),
                ("state", state.clone()),
            ],
        )?;
        vault::save(
            &pending,
            Some(
                &json!({"state":state,"until":chrono::Utc::now().timestamp()+600,"app_id":config["app_id"]}),
            ),
        )?;
        return Ok(json!({"authorize_url":url.as_str()}));
    }
    if op == "teambition.personal" {
        let mut person = if q["code"].as_str().is_some_and(|s| !s.is_empty()) {
            let attempt = vault::read(&pending)?;
            if q["state"].as_str().is_none_or(str::is_empty)
                || q["state"] != attempt["state"]
                || attempt["app_id"] != config["app_id"]
                || attempt["until"].as_i64().unwrap_or_default() < chrono::Utc::now().timestamp()
            {
                return Err("forbidden: expired or mismatched Teambition OAuth state".into());
            }
            vault::save(&pending, None)?;
            let token = api.request(
                "POST",
                "/oauth/userAccessToken",
                &[],
                &json!({"code":cred(q,"code")?,"grantType":"authorization_code"}),
            )?;
            json!({"access_token":cred(&token["result"],"accessToken")?,"user_id":token["result"]["userId"]})
        } else {
            json!({"access_token":cred(q,"access_token")?})
        };
        api.authorize_person(&person)?;
        person["user_id"] = json!(api.operator);
        person["app_id"] = config["app_id"].clone();
        let mut saved = vault::save(&person_path, Some(&person))?;
        saved["user_id"] = person["user_id"].clone();
        return Ok(saved);
    }
    let size = q
        .get("page_size")
        .map(|s| {
            s.as_u64()
                .filter(|v| (1..=100).contains(v))
                .ok_or("invalid: page_size must be 1–100")
        })
        .transpose()?
        .unwrap_or(50) as usize;
    let scope = b["project"].as_str().unwrap_or("");
    let verified_scope = org_location(
        client,
        &b,
        "teambition-verified-project",
        &json!([tenant, space, q["id"], scope, b["connector_created"]]).to_string(),
    )?;
    if op != "teambition.synchronize" {
        let saved = vault::read(&person_path)?;
        if saved["app_id"] != config["app_id"] {
            return Err(
                "forbidden: bind your Teambition account for this application first".into(),
            );
        }
        api.authorize_person(&saved)?;
        let projects = api.projects(size)?;
        if op == "teambition.projects" {
            return Ok(json!({"projects":projects,"user_id":api.operator,"enterprise_id":tenant}));
        }
        if !projects.iter().any(|p| p["id"] == scope) {
            return Err("forbidden: join this Teambition project before continuing".into());
        }
        if op == "teambition.push" {
            return publish::with_lease(client, q, |plan| api.push(plan, scope));
        }
    }
    let polling = op == "teambition.synchronize";
    if polling && vault::read(&verified_scope)?["verified"] != true {
        return Err("forbidden: preview this project with a joined Teambition user before enabling background sync".into());
    }
    api.operator = None;
    let scope = id(&b["project"])?;
    api.project(&scope)?;
    let states = api.pages(
        &format!("/v3/project/{scope}/taskflowstatus/search"),
        &[],
        size,
    )?;
    let mut rows = api.pages(
        &format!("/v3/project/{scope}/task/query"),
        &[("includeArchived", "false".into())],
        size,
    )?;
    for row in &mut rows {
        if row["projectId"] != scope {
            return Err("forbidden: task is outside the Teambition project".into());
        }
        row["source_status_name"] = states
            .iter()
            .find(|s| s["id"] == row["tfsId"])
            .map(|s| s["name"].clone())
            .unwrap_or_default();
    }
    let mode = if polling {
        "changes"
    } else {
        q["import_mode"].as_str().unwrap_or("history")
    };
    let received = rows.len();
    rows.retain(|r| r["visible"] == "members" && r["isArchived"] != true);
    let excluded = received - rows.len();
    let mut result = stage_batch(client, q, &b, &scope, rows, mode, polling)?;
    result["excluded_restricted_or_archived"] = json!(excluded);
    if !polling {
        vault::save(&verified_scope, Some(&json!({"verified":true})))?;
    }
    Ok(result)
}
