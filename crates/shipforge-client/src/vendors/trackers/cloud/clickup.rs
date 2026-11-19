use super::*;

pub(super) fn accounts(api: &Cloud) -> Result<Vec<Value>> {
    array(&api.get("/team", &[])?, "teams")?
        .iter()
        .map(|r| Ok(json!({"id":id(&r["id"])?,"name":r["name"]})))
        .collect()
}
pub(super) fn projects(api: &Cloud) -> Result<Vec<Value>> {
    let spaces = api.get(
        &format!("/team/{}/space", api.account),
        &[("archived", "false".into())],
    )?;
    let mut records = Records::new();
    for space in array(&spaces, "spaces")? {
        let sid = id(&space["id"])?;
        let folders = api.get(
            &format!("/space/{sid}/folder"),
            &[("archived", "false".into())],
        )?;
        let mut locations = vec![(format!("/space/{sid}/list"), Value::Null)];
        for folder in array(&folders, "folders")? {
            let fid = id(&folder["id"])?;
            locations.push((
                format!("/folder/{fid}/list"),
                json!({"id":fid,"name":folder["name"]}),
            ));
        }
        for (path, folder) in locations {
            let lists = api.get(&path, &[("archived", "false".into())])?;
            for r in array(&lists, "lists")? {
                if r["space"].is_object() && id(&r["space"]["id"])? != sid {
                    return Err("forbidden: ClickUp returned a List from another Space".into());
                }
                records.add(json!({"id":id(&r["id"])?,"name":r["name"],"space":{"id":sid,"name":space["name"]},"folder":folder}))?;
            }
        }
    }
    Ok(records.finish())
}
pub(super) fn issues(api: &Cloud, scope: &str, since: Option<i64>) -> Result<Vec<Value>> {
    let mut rows = Records::new();
    for page in 0..=5000 {
        let mut params = vec![
            ("page", page.to_string()),
            ("include_closed", "true".into()),
            ("subtasks", "true".into()),
            ("order_by", "updated".into()),
            ("reverse", "false".into()),
        ];
        if let Some(ms) = since {
            params.push(("date_updated_gt", ms.to_string()));
        }
        let data = api.get(&format!("/list/{scope}/task"), &params)?;
        let native = array(&data, "tasks")?;
        if native.is_empty() {
            return Ok(rows.finish());
        }
        let before = rows.rows.len();
        for r in native {
            if id(&r["list"]["id"])? != scope {
                return Err("forbidden: ClickUp returned a task from a different home List".into());
            }
            rows.add(r.clone())?;
        }
        if data["last_page"] == true {
            return Ok(rows.finish());
        }
        if rows.rows.len() == before {
            return Err("vendor: ClickUp repeated a page".into());
        }
    }
    Err("vendor: ClickUp page limit exceeded".into())
}
pub(super) fn states(api: &Cloud, scope: &str) -> Result<Vec<Value>> {
    let list = api.get(&format!("/list/{scope}"), &[])?;
    if id(&list["id"])? != scope {
        return Err("forbidden: wrong ClickUp List".into());
    }
    Ok(array(&list, "statuses")?.to_vec())
}

pub(super) fn state_name<'a>(statuses: &'a [Value], target: &str) -> Result<&'a str> {
    let mut matched = statuses
        .iter()
        .filter(|r| r["id"] == target || r["status"] == target);
    let name = matched
        .next()
        .and_then(|r| r["status"].as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("invalid: mapped status is not available in this List")?;
    if matched.next().is_some() || statuses.iter().filter(|r| r["status"] == name).count() != 1 {
        return Err("conflict: ClickUp status name is ambiguous".into());
    }
    Ok(name)
}

pub(super) fn status(api: &Cloud, key: &str, target_name: &str) -> Result<()> {
    api.request(
        "PUT",
        &format!("/task/{key}"),
        &[],
        &json!({"status":target_name}),
    )?;
    Ok(())
}
pub(super) fn comment(api: &Cloud, key: &str, body: &str, confirm: Option<&str>) -> Result<String> {
    let path = format!("/task/{key}/comment");
    if let Some(known) = confirm {
        let mut query = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..=5000 {
            let result = api.get(&path, &query)?;
            let rows = array(&result, "comments")?;
            if let Some(comment) = rows
                .iter()
                .find(|v| id(&v["id"]).ok().as_deref() == Some(known))
            {
                if comment["comment_text"] != body {
                    return Err("conflict: ClickUp comment text differs".into());
                }
                return Ok(known.into());
            }
            let Some(last) = rows.last() else { break };
            let cursor = id(&last["id"])?;
            if !seen.insert(cursor.clone()) {
                break;
            }
            query = vec![("start", id(&last["date"])?), ("start_id", cursor)];
        }
        return Err("conflict: requested comment is not on this task".into());
    }
    let value = api.request(
        "POST",
        &path,
        &[],
        &json!({"comment_text":body,"notify_all":false}),
    )?;
    id(&value["id"])
}
