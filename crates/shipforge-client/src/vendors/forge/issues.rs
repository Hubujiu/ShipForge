use super::*;
fn endpoint(p: &str, b: &Value, number: &Value, review: bool) -> Result<String> {
    let n = id(number)?;
    if p == "github" {
        Ok(format!("/repos/{}/issues/{n}", field(b, "repository")?))
    } else {
        Ok(format!(
            "/projects/{}/{}/{n}",
            id(&b["repository_id"])?,
            if review { "merge_requests" } else { "issues" }
        ))
    }
}
fn sender(
    client: &Client,
    cfg: &Value,
    p: &str,
    org: Api,
    personal: bool,
    b: &Value,
) -> Result<(Api, bool, Option<String>)> {
    let mut fallback = personal;
    if personal && !cfg["personal"].is_null() {
        let found = auth::api(client, cfg, p, true).and_then(|api| {
            let who = api.request("GET", "/user", &Value::Null)?;
            if id(&who["id"])? != cfg["personal"]["account"] {
                return Err("forbidden: personal source identity changed".into());
            }
            let path = if p == "github" {
                format!("/repos/{}", field(b, "repository")?)
            } else {
                format!("/projects/{}", id(&b["repository_id"])?)
            };
            let r = api.request("GET", &path, &Value::Null)?;
            if id(&r["id"])? != b["repository_id"] {
                return Err("forbidden: unexpected personal repository".into());
            }
            Ok((api, id(&who["id"])?))
        });
        match found {
            Ok((api, person)) => return Ok((api, false, Some(person))),
            Err(e) if permission_error(&e) || e.contains("source authorization was rejected") => {
                fallback = true
            }
            Err(e) => return Err(e),
        }
    }
    let who = if p == "github" {
        None
    } else {
        Some(
            cfg["account"]["account"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    };
    Ok((org, fallback, who))
}
pub(super) fn send(client: &mut Client, q: &Value, cfg: &Value, drain: bool) -> Result<Value> {
    let p = field(q, "id")?;
    let b = cfg["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|b| b["id"] == q["pair"])
        .ok_or("not_found: active repository pairing")?;
    let org = managed_api(client, cfg, p, b)?;
    let plan = invoke(client, q, if drain { "drain" } else { "reserve" }, q)?;
    if plan.is_null() || plan["lease"].is_null() {
        return Ok(plan);
    }
    let mut ticket = json!({"credential":cfg["account"]["credential"],"pair":q["pair"],"delivery":plan["delivery"],"lease":plan["lease"]});
    let sending = (|| -> Result<(String, bool)> {
        let root = endpoint(p, b, &plan["number"], plan["kind"] == "review_link")?;
        if p != "github" {
            let target = org.request("GET", &root, &Value::Null)?;
            if target["confidential"] == true
                || id(&target["iid"])? != id(&plan["number"])?
                || id(&target["project_id"])? != b["repository_id"]
            {
                return Err("forbidden: private or mismatched GitLab target".into());
            }
        }
        let (api, fallback, person) = sender(client, cfg, p, org, !drain, b)?;
        let path = format!(
            "{root}/{}",
            if p == "github" { "comments" } else { "notes" }
        );
        let marker = format!("<!-- shipforge:{} -->", field(&plan, "delivery")?);
        if plan["recover"] == true {
            let slug = std::env::var("SHIPFORGE_GITHUB_APP_SLUG").unwrap_or_default();
            let result = api
                .pages(&path, None)?
                .into_iter()
                .find(|n| {
                    let author = &n[if p == "github" { "user" } else { "author" }];
                    let owned = person
                        .as_ref()
                        .is_some_and(|id_| id(&author["id"]).ok().as_ref() == Some(id_))
                        || (p == "github"
                            && author["type"] == "Bot"
                            && author["login"] == format!("{slug}[bot]"));
                    owned && n["body"].as_str().is_some_and(|b| b.ends_with(&marker))
                })
                .ok_or("uncertain: source reply has no matching confirmation yet")?;
            return Ok((id(&result["id"])?, fallback));
        }
        let body = format!("{}\n\n{marker}", field(&plan, "body")?);
        let payload = json!({"body":body});
        invoke(client, q, "can_publish", &ticket)?;
        let sent = api.request("POST", &path, &payload);
        // A definite permission response cannot have committed the original write.
        if !drain && !fallback && sent.as_ref().is_err_and(|e| permission_error(e)) {
            let org = managed_api(client, cfg, p, b)?;
            invoke(client, q, "can_publish", &ticket)?;
            return Ok((id(&org.request("POST", &path, &payload)?["id"])?, true));
        }
        Ok((id(&sent?["id"])?, fallback))
    })();
    match sending {
        Ok((remote, fallback)) => {
            ticket["state"] = json!("sent");
            ticket["remote_id"] = json!(remote);
            ticket["identity"] = json!(if drain || fallback {
                "organization"
            } else {
                "personal"
            });
            ticket["fallback"] = json!(fallback);
            invoke(client, q, "settle", &ticket)
        }
        Err(e) => {
            let definite = e.starts_with("invalid:")
                || e.starts_with("forbidden:")
                || e.starts_with("vendor: upstream returned HTTP 4");
            ticket["state"] = json!(if definite && plan["recover"] != true {
                "failed"
            } else {
                "uncertain"
            });
            invoke(client, q, "settle", &ticket)?;
            Err(e)
        }
    }
}
pub(super) fn sync(client: &mut Client, q: &Value, cfg: &Value, push: bool) -> Result<Value> {
    let p = field(q, "id")?;
    let gh = p == "github";
    let m = invoke(
        client,
        q,
        if push { "push_plan" } else { "refresh_plan" },
        q,
    )?;
    let b = &m["pair"];
    let api = managed_api(client, cfg, p, b)?;
    let path = endpoint(p, b, &m["number"], false)?;
    let mut remote = api.request("GET", &path, &Value::Null)?;
    let n = id(&m["number"])?;
    let valid = |r: &Value| -> Result<()> {
        if id(&r[if gh { "number" } else { "iid" }])? != n
            || r["confidential"] == true
            || r["pull_request"].is_object()
            || !gh && id(&r["project_id"])? != b["repository_id"]
        {
            return Err("forbidden: remote issue does not match the repository mapping".into());
        }
        Ok(())
    };
    valid(&remote)?;
    if push {
        let body_key = if gh { "body" } else { "description" };
        let closed = remote["state"] == "closed";
        let mut changes = json!({});
        if remote["title"] != m["title"] {
            changes["title"] = m["title"].clone();
        }
        if remote[body_key].as_str().unwrap_or("") != m["body"].as_str().unwrap_or("") {
            changes[body_key] = m["body"].clone();
        }
        if closed != m["closed"].as_bool().unwrap_or(false) {
            let closed = m["closed"] == true;
            changes[if gh { "state" } else { "state_event" }] = json!(if gh {
                if closed { "closed" } else { "open" }
            } else if closed {
                "close"
            } else {
                "reopen"
            });
        }
        if let Some(members) = m["outbound_assignees"].as_array() {
            let mut selected = std::collections::BTreeMap::new();
            for member in members {
                selected.insert(id(&member["id"])?, field(member, "login")?.to_owned());
            }
            let mut current = std::collections::BTreeSet::new();
            for person in remote["assignees"].as_array().into_iter().flatten() {
                let key = id(&person["id"])?;
                current.insert(key.clone());
                if m["external_assignees"]
                    .as_array()
                    .is_some_and(|l| l.iter().any(|v| v == &key))
                {
                    selected.insert(
                        key,
                        person["login"]
                            .as_str()
                            .or(person["username"].as_str())
                            .unwrap_or("")
                            .into(),
                    );
                }
            }
            if current != selected.keys().cloned().collect() {
                let baseline = m["baseline_assignee_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                if current != baseline {
                    return Err(
                        "conflict: source assignees changed; refresh before publishing".into(),
                    );
                }
                if gh && selected.values().any(String::is_empty) {
                    return Err("conflict: missing source login for a retained assignee".into());
                }
                if gh {
                    changes["assignees"] = json!(selected.values().collect::<Vec<_>>());
                } else {
                    changes["assignee_ids"] = json!(
                        selected
                            .keys()
                            .map(|id| id.parse::<i64>())
                            .collect::<std::result::Result<Vec<_>, _>>()
                            .map_err(|_| "invalid: invalid GitLab assignee")?
                    );
                }
            }
        }
        if changes.as_object().is_some_and(|o| !o.is_empty()) {
            let baseline = &m["source_baseline"];
            if changes.get("title").is_some() && remote["title"] != baseline["title"]
                || changes.get(body_key).is_some()
                    && remote[body_key].as_str().unwrap_or("")
                        != baseline["description"].as_str().unwrap_or("")
                || closed != m["closed"].as_bool().unwrap_or(false)
                    && closed != baseline["closed"].as_bool().unwrap_or(false)
            {
                return Err("conflict: source issue changed; refresh before publishing".into());
            }
            let stamp = field(&remote, "updated_at")?
                .parse::<chrono::DateTime<chrono::Utc>>()
                .map_err(|_| "vendor: invalid upstream version")?
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
            if m["source_version"] != stamp {
                return Err("conflict: source issue changed; refresh before publishing".into());
            }
            if invoke(client, q, "push_plan", q)? != m
                || invoke(client, q, "config", &Value::Null)?["account"]["credential"]
                    != cfg["account"]["credential"]
            {
                return Err(
                    "conflict: source pairing or task changed; refresh before publishing".into(),
                );
            }
            let method = if gh { "PATCH" } else { "PUT" };
            if changes.get("assignees").is_some() || changes.get("assignee_ids").is_some() {
                let (act, fallback, _) = sender(client, cfg, p, api, true, b)?;
                let result = act.request(method, &path, &changes);
                remote = if !fallback && result.as_ref().is_err_and(|e| permission_error(e)) {
                    managed_api(client, cfg, p, b)?.request(method, &path, &changes)?
                } else {
                    result?
                };
                let actual = remote["assignees"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| {
                        if gh {
                            v["login"].as_str().unwrap_or("").to_owned()
                        } else {
                            id(&v["id"]).unwrap_or_default()
                        }
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                let expected = changes[if gh { "assignees" } else { "assignee_ids" }]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| {
                        if gh {
                            v.as_str().unwrap_or("").to_owned()
                        } else {
                            id(v).unwrap_or_default()
                        }
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                if actual != expected {
                    return Err("conflict: source rejected one or more issue assignees; refresh for the current values".into());
                }
            } else {
                remote = api.request(method, &path, &changes)?;
            }
            valid(&remote)?;
        }
    }
    let body = if gh {
        json!({"action":"edited","installation":{"id":b["installation"]},"repository":{"id":b["repository_id"],"full_name":b["repository"]},"issue":remote})
    } else {
        remote["action"] = json!("update");
        json!({"object_kind":"issue","project":{"id":b["repository_id"],"path_with_namespace":b["repository"]},"object_attributes":remote})
    };
    invoke(
        client,
        q,
        "sync_event",
        &json!({"pair":q["pair"],"task_id":q["task_id"],"event":if gh{"issues"}else{"issue"},"payload":body,"event_id":format!("sync-{}",digest(&body.to_string()))}),
    )
}
