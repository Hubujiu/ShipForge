use super::*;

pub(super) fn accounts(api: &Cloud) -> Result<Vec<Value>> {
    array(&api.get("/account/teams", &[])?, "teams")?
        .iter()
        .map(|r| Ok(json!({"id":id(&r["id"])?,"name":r["name"]})))
        .collect()
}
fn paged(api: &Cloud, path: &str, size: usize, extra: &[(&str, String)]) -> Result<Vec<Value>> {
    let mut cursor = None;
    let mut seen = HashSet::new();
    let mut records = Records::new();
    loop {
        let mut query = vec![("teamID", api.account.clone()), ("limit", size.to_string())];
        query.extend_from_slice(extra);
        if let Some(cursor) = cursor.take() {
            query.push(("cursor", cursor));
        }
        let data = api.get(path, &query)?;
        for row in array(&data, "list")? {
            records.add(row.clone())?;
        }
        cursor = next_page(&data["pageInfo"], &mut seen)?;
        if cursor.is_none() {
            break;
        }
    }
    Ok(records.finish())
}
pub(super) fn projects(api: &Cloud, size: usize) -> Result<Vec<Value>> {
    paged(api, "/project/projects", size, &[])?
        .into_iter()
        .map(|p| Ok(json!({"id":id(&p["id"])?,"name":p["name"],"archived":p["isArchive"]})))
        .collect()
}
pub(super) fn detail(api: &Cloud, key: &str) -> Result<Value> {
    api.get(
        &format!("/project/issues/{key}"),
        &[("teamID", api.account.clone())],
    )
}
pub(super) fn issues(api: &Cloud, scope: &str, size: usize) -> Result<Vec<Value>> {
    let types = paged(api, "/project/issueTypes", size, &[])?;
    let mut records = Records::new();
    for kind in types {
        let ty = id(&kind["id"])?;
        for mut row in paged(
            api,
            "/project/issues",
            size,
            &[("projectID", scope.into()), ("issueTypeID", ty.clone())],
        )? {
            let key = id(&row["id"])?;
            if id(&row["project"]["id"])? != scope || id(&row["issueType"]["id"])? != ty {
                return Err(
                    "forbidden: ONES returned an issue from another project or type".into(),
                );
            }
            if !row["descriptionText"].is_string() {
                let full = detail(api, &key)?;
                if id(&full["id"])? != key
                    || id(&full["project"]["id"])? != scope
                    || id(&full["issueType"]["id"])? != ty
                {
                    return Err("forbidden: ONES issue moved while loading its description".into());
                }
                row = full;
            }
            records.add(row)?;
        }
    }
    Ok(records.finish())
}
pub(super) fn status(
    api: &Cloud,
    key: &str,
    current: &str,
    target: &str,
    configured: Option<&str>,
) -> Result<()> {
    let path = format!("/project/issues/{key}");
    let workflows = api.get(
        &path,
        &[
            ("teamID", api.account.clone()),
            ("action", "executableWorkflow".into()),
        ],
    )?;
    let mut matching = workflows
        .as_array()
        .ok_or("vendor: ONES returned no workflow list")?
        .iter()
        .filter(|w| {
            w["start"] == current && w["end"] == target && configured.is_none_or(|id| w["id"] == id)
        });
    let selected = matching
        .next()
        .ok_or("conflict: no executable workflow reaches the mapped status")?;
    if matching.next().is_some() {
        return Err(
            "conflict: map a specific workflow ID when multiple transitions reach the same status"
                .into(),
        );
    }
    api.request(
        "POST",
        &path,
        &[
            ("teamID", api.account.clone()),
            ("action", "executeWorkflow".into()),
        ],
        &json!({"id":id(&selected["id"])?}),
    )?;
    Ok(())
}
pub(super) fn comment(api: &Cloud, key: &str, body: &str, confirm: Option<&str>) -> Result<String> {
    let path = format!("/project/issues/{key}/comments");
    if let Some(known) = confirm {
        let rows = paged(api, &path, 100, &[])?;
        if !rows.iter().any(|c| c["id"] == known && c["text"] == body) {
            return Err("conflict: ONES did not return this comment on this issue".into());
        }
        return Ok(known.to_owned());
    }
    if body.chars().count() > 128 {
        return Err("invalid: ONES documents a 128-character limit for a new comment".into());
    }
    let result = api.request(
        "POST",
        &path,
        &[("teamID", api.account.clone())],
        &json!({"text":body}),
    )?;
    // This endpoint commonly acknowledges success without allocating an ID in its response.
    if result["id"].is_null() {
        Ok(String::new())
    } else {
        id(&result["id"])
    }
}
