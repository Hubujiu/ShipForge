use super::*;

pub(super) fn connect(site: &str, credentials: &Value) -> Result<(Api, Value)> {
    let account = cred(credentials, "account")?;
    let password = cred(credentials, "password")?;
    let values = json!({"account":account,"password":password});
    // Self-hosted credentials are scoped to this exact, user-configured installation.
    let http = http();
    let root = format!("{site}/api.php/v1");
    let response = http
        .post(format!("{root}/tokens"))
        .send_json(&values)
        .map_err(|_| "vendor: unable to reach the configured ZenTao REST v1 installation")?;
    let token = unpack(response)
        .map_err(|e| format!("{e}; check the ZenTao installation root, version and account"))?;
    let token = cred(&token, "token")?;
    if !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("vendor: invalid ZenTao token".into());
    }
    Ok((
        Api {
            http,
            root,
            token,
            zen: true,
        },
        values,
    ))
}

pub(super) fn projects(api: &Api, b: &Value, size: usize) -> Result<Vec<Value>> {
    let group = if b["scope_kind"] == "product" {
        "products"
    } else {
        "projects"
    };
    let path = format!("/{group}");
    api.pages(&path,group,&[("status","all".into())],size)?.into_iter().map(|r|Ok(json!({"id":id(&r["id"])?,"name":r["name"],"status":r["status"],"kind":b["scope_kind"]}))).collect()
}

pub(super) fn issues(api: &Api, b: &Value, size: usize) -> Result<Vec<Value>> {
    let scope = id(&b["project"])?;
    let product = b["scope_kind"] == "product";
    let base = format!("/{}/{scope}", if product { "products" } else { "projects" });
    let mut output = Vec::new();
    let mut bytes = 0;
    let check_scope = |record: &Value, field: &str, expected: &str| -> Result<()> {
        let value = &record[field];
        let value = if value.is_object() {
            &value["id"]
        } else {
            value
        };
        if !value.is_null() {
            let actual = id(value)?;
            if actual != "0" && actual != expected {
                return Err("forbidden: ZenTao returned a record from another scope".into());
            }
        }
        Ok(())
    };
    let mut collect = |rows: Vec<Value>, kind: &str, path_scope: Option<&str>| -> Result<()> {
        let mut pending = rows;
        while let Some(mut row) = pending.pop() {
            if let Some(children) = row
                .as_object_mut()
                .and_then(|fields| fields.remove("children"))
                .and_then(|v| v.as_array().cloned())
            {
                pending.extend(
                    children
                        .into_iter()
                        .filter(|child| child["id"].is_number() || child["id"].is_string()),
                );
            }
            let kind = if matches!(kind, "story" | "requirement") {
                row["type"]
                    .as_str()
                    .filter(|s| matches!(*s, "story" | "requirement"))
                    .unwrap_or(kind)
                    .to_owned()
            } else {
                kind.to_owned()
            };
            let kind = kind.as_str();
            let key = id(&row["id"])?;
            if !key.bytes().all(|b| b.is_ascii_digit()) {
                return Err("vendor: invalid ZenTao issue ID".into());
            }
            let source_field = if product { "product" } else { "project" };
            check_scope(&row, source_field, &scope)?;
            if let Some(execution) = path_scope {
                check_scope(&row, "execution", execution)?;
            }
            if row["deleted"] == true || row["deleted"] == "1" {
                continue;
            }
            let field = match kind {
                "story" | "requirement" => "spec",
                "bug" => "steps",
                _ => "desc",
            };
            if row.get(field).is_none() {
                let group = if matches!(kind, "story" | "requirement") {
                    "stories"
                } else if kind == "bug" {
                    "bugs"
                } else {
                    "tasks"
                };
                let details = api.get(&format!("/{group}/{key}"), &[])?;
                if id(&details["id"])? != key {
                    return Err("vendor: incompatible ZenTao item detail".into());
                }
                check_scope(&details, source_field, &scope)?;
                if let Some(execution) = path_scope {
                    check_scope(&details, "execution", execution)?;
                }
                row[field] = details[field].clone();
                if !details["lastEditedDate"].is_null() {
                    row["lastEditedDate"] = details["lastEditedDate"].clone();
                }
            }
            bytes += row.to_string().len();
            if bytes > 7_000_000 || output.len() >= 5000 {
                return Err("vendor: ZenTao batch exceeds 5000 entries or 7 MB".into());
            }
            output.push(
                json!({"scope":scope,"scope_kind":b["scope_kind"],"entity":kind,"record":row}),
            );
        }
        Ok(())
    };
    for (group, kind) in [("stories", "story"), ("bugs", "bug")] {
        let rows = api.pages(
            &format!("{base}/{group}"),
            group,
            &[("status", "all".into()), ("order", "id_asc".into())],
            size,
        )?;
        collect(rows, kind, None)?;
    }
    if product {
        let rows = api.pages(
            &format!("{base}/stories"),
            "stories",
            &[
                ("status", "all".into()),
                ("type", "requirement".into()),
                ("order", "id_asc".into()),
            ],
            size,
        )?;
        collect(
            rows.into_iter()
                .filter(|r| r["type"] == "requirement")
                .collect(),
            "requirement",
            None,
        )?;
    }
    if !product {
        let executions = api.pages(&format!("{base}/executions"), "executions", &[], size)?;
        for execution in executions {
            check_scope(&execution, "project", &scope)?;
            let key = id(&execution["id"])?;
            let tasks = api.pages(
                &format!("/executions/{key}/tasks"),
                "tasks",
                &[("status", "all".into()), ("order", "id_asc".into())],
                size,
            )?;
            collect(tasks, "task", Some(&key))?;
        }
    }
    Ok(output)
}
