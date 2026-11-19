use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

pub(crate) fn outside(c: &Context<'_>) -> Result<bool> {
    if c.role().is_ok() {
        return Ok(false);
    }
    let space = c
        .find("space", c.space)?
        .ok_or_else(|| forbidden("space is unavailable"))?;
    if space.fields["visibility"] != "public" || space.fields["deactivated"] == true {
        return Err(forbidden("space is unavailable"));
    }
    let org = c.org_id()?;
    if org == c.space {
        return Err(forbidden("organization membership required"));
    }
    let role = Context { space: &org, ..*c }.role()?;
    if !matches!(role.as_str(), "owner" | "admin" | "member" | "editor") {
        return Err(forbidden("an explicit space invitation is required"));
    }
    Ok(true)
}
pub(crate) fn submit(c: &Context<'_>, q: &Value) -> Result<Value> {
    let outside = outside(c)?;
    let role = c.role().unwrap_or_else(|_| "outsider".into());
    let space = enabled(c)?;
    if !outside
        && !matches!(
            role.as_str(),
            "owner" | "admin" | "member" | "editor" | "guest"
        )
    {
        return Err(forbidden("space submission is unavailable"));
    }
    if outside
        && space.fields["triage_sources"]
            .as_array()
            .is_some_and(|s| !s.iter().any(|x| x == "external"))
    {
        return Err(forbidden("external submissions are disabled"));
    }
    let ty = q
        .get("type")
        .map(|v| v.as_str().ok_or_else(|| invalid("type must be a string")))
        .transpose()?
        .map(str::to_owned)
        .unwrap_or(crate::defaults::selected(c, "task_type", "task")?);
    if space.fields["triage_types"]
        .as_array()
        .is_some_and(|s| !s.iter().any(|t| t == &ty))
    {
        return Err(invalid("the task type is excluded from this queue"));
    }
    let config = c.readable("task_type", &ty)?;
    let status = if let Some(list) = config.fields["statuses"]
        .as_array()
        .filter(|l| !l.is_empty())
    {
        list.iter()
            .filter_map(Value::as_str)
            .find(|id| c.category_for(id).ok().as_deref() == Some("triage"))
            .ok_or_else(|| invalid("the task type has no triage state"))?
    } else {
        "triage"
    };
    let mut f = json!({"type":ty,"status":status,"source":if outside{"organization"}else if role=="guest"{"guest"}else{"member"}});
    for key in ["description", "custom", "priority"] {
        if let Some(v) = q.get(key) {
            f[key] = v.clone();
        }
    }
    let request = json!({"op":"create","kind":"task","title":required(q,"title")?,"fields":f});
    let r = if outside {
        crate::domain::write_submission(c, &request)?
    } else {
        write_request(c, &request)?
    };
    // A submission receipt never reveals the destination or reviewer chosen by space policy.
    Ok(json!({"submitted":true,"id":r["id"],"number":r["fields"]["number"]}))
}
fn workspace(c: &Context<'_>) -> Result<String> {
    c.db.query_row(
        "SELECT reference FROM view_workspace WHERE singleton=1",
        [],
        |r| r.get(0),
    )
    .map_err(storage)
}
pub fn parts(s: &str) -> Result<(String, String, String)> {
    let values: Vec<_> = s
        .strip_prefix("shipforge://task/")
        .ok_or_else(|| invalid("invalid task link"))?
        .split('/')
        .collect();
    if values.len() != 3 || uuid::Uuid::parse_str(values[0]).is_err() {
        return Err(invalid("invalid task link"));
    }
    let decode = |raw: &str| -> Result<String> {
        if raw.len() > 860 {
            return Err(invalid("invalid task link"));
        }
        let s = String::from_utf8(
            URL_SAFE_NO_PAD
                .decode(raw)
                .map_err(|_| invalid("invalid task link"))?,
        )
        .map_err(|_| invalid("invalid task link"))?;
        identifier(&s)?;
        Ok(s)
    };
    Ok((values[0].into(), decode(values[1])?, decode(values[2])?))
}
pub(crate) fn duplicate_target(
    c: &Context<'_>,
    target: &str,
    space: Option<&str>,
) -> Result<(String, String)> {
    let (scope, id) = if target.starts_with("shipforge://") {
        let (ws, s, id) = parts(target)?;
        if ws != workspace(c)? {
            return Err(forbidden("task link belongs to a different workspace"));
        }
        (s, id)
    } else {
        (space.unwrap_or(c.space).into(), target.to_owned())
    };
    let dest = Context {
        space: &scope,
        ..*c
    };
    dest.role()?;
    if dest.org_id()? != c.org_id()? {
        return Err(forbidden("task belongs to a different organization"));
    }
    let canonical = match dest.readable("task", &id) {
        Ok(row) => row.id,
        Err(e) => crate::query::visible_records(&dest, "task")?
            .into_iter()
            .find(|r| r.fields["number"] == id)
            .map(|r| r.id)
            .ok_or(e)?,
    };
    Ok((scope, canonical))
}
pub(crate) fn link(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op == "task.share" {
        let task = c.readable("task", required(q, "id")?)?;
        return Ok(
            json!({"number":task.fields["number"],"link":format!("shipforge://task/{}/{}/{}",workspace(c)?,URL_SAFE_NO_PAD.encode(c.space),URL_SAFE_NO_PAD.encode(task.id))}),
        );
    }
    let (ws, s, id) = parts(required(q, "link")?)?;
    if ws != workspace(c)? {
        return Err(invalid("connect to the workspace that issued this link"));
    }
    let dest = Context { space: &s, ..*c };
    dest.role()?;
    Ok(as_value(dest.readable("task", &id)?))
}
