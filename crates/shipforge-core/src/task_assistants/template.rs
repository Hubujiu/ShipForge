use super::*;

fn defaults() -> Value {
    json!({"zh-CN":{"description":"在 {{imName}} 群中协助处理 {{spaceName}} 的任务。","instructions":"根据当前话题和成员明确提出的请求，整理或查询 {{spaceName}} 中的任务。保留代码块、验收条件和原始上下文。仅使用当前空间的授权结果。"},
    "en":{"description":"Help members of {{spaceName}} track work in {{imName}}.","instructions":"Use the current thread and the member's explicit request to draft or look up work in {{spaceName}}. Preserve code blocks, acceptance criteria and relevant context. Use only the authorized space results."}})
}
pub(super) fn content(c: &Context<'_>) -> Result<(i64, Value)> {
    let org = c.org_id()?;
    let row: Option<(i64, String)> =
        c.db.query_row(
            "SELECT rev,templates FROM sf_im_assistant_templates WHERE organization=?",
            [org],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    match row {
        Some((rev, raw)) => Ok((rev, serde_json::from_str(&raw).map_err(storage)?)),
        None => Ok((0, defaults())),
    }
}
pub(super) fn locale(c: &Context<'_>, q: &Value) -> Result<String> {
    if let Some(lang) = q.get("language") {
        return match lang.as_str() {
            Some("en" | "zh-CN") => Ok(lang.as_str().unwrap().into()),
            _ => Err(invalid("language must be en or zh-CN")),
        };
    }
    Ok(crate::organization::defaults(c)?.0)
}
pub(super) fn text(v: &Value, limit: usize) -> Result<&str> {
    v.as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= limit && !s.contains('\0'))
        .ok_or_else(|| invalid("assistant text is empty or too long"))
}
pub(super) fn execute(c: &Context<'_>, q: &Value, save: bool) -> Result<Value> {
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    root.require_admin()?;
    let (mut rev, mut templates) = content(c)?;
    if save {
        if q["expected_rev"].as_i64() != Some(rev) {
            return Err(conflict("refresh the organization task assistant template"));
        }
        let lang = locale(c, q)?;
        let mut changed = false;
        for (key, limit) in [("description", 4096), ("instructions", 32768)] {
            if let Some(v) = q.get(key) {
                templates[&lang][key] = json!(text(v, limit)?);
                changed = true;
            }
        }
        if !changed {
            return Err(invalid("provide a description or instructions"));
        }
        rev += 1;
        c.db.execute("INSERT INTO sf_im_assistant_templates(organization,rev,templates) VALUES(?,?,?) ON CONFLICT(organization) DO UPDATE SET rev=excluded.rev,templates=excluded.templates",params![org,rev,templates.to_string()]).map_err(storage)?;
    }
    Ok(json!({"organization":org,"rev":rev,"templates":templates}))
}
pub(super) fn effective(c: &Context<'_>, r: &Record, lang: &str) -> Result<Value> {
    let (rev, templates) = content(c)?;
    let provider = required(&r.fields, "provider")?;
    let space = c.get("space", c.space)?.title;
    let im = label(provider, lang);
    let filled = |value: &str| {
        value
            .replace("{{productName}}", "ShipForge")
            .replace("{{cliName}}", "shipforge")
            .replace("{{imName}}", im)
            .replace("{{spaceName}}", &space)
    };
    let mut result = json!({"template_rev":rev,"language":lang,"overridden":{}});
    for key in ["instructions", "description"] {
        let own = r.fields["overrides"][lang][key].as_str();
        result[key] = json!(filled(
            own.unwrap_or(templates[lang][key].as_str().unwrap_or(""))
        ));
        result["overridden"][key] = json!(own.is_some());
    }
    Ok(result)
}
