use crate::{Context, Record, Result, conflict, domain, forbidden, invalid, json, required};
use serde_json::Value;

pub(crate) fn defaults(c: &Context<'_>) -> Result<(String, u64)> {
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    let row = root.find("organization", &org)?;
    Ok(row
        .map(|r| {
            (
                r.fields["default_language"]
                    .as_str()
                    .unwrap_or("zh-CN")
                    .to_owned(),
                r.fields["priority_levels"].as_u64().unwrap_or(4),
            )
        })
        .unwrap_or_else(|| ("zh-CN".into(), 4)))
}
pub(crate) fn validate(f: &Value) -> Result<()> {
    if f.get("default_language")
        .is_some_and(|v| !matches!(v.as_str(), Some("zh-CN" | "en")))
    {
        return Err(invalid("choose zh-CN or en for new organization content"));
    }
    if f.get("priority_levels")
        .is_some_and(|v| !matches!(v.as_u64(), Some(4 | 5)))
    {
        return Err(invalid("priority_levels must be four or five"));
    }
    Ok(())
}
pub(crate) fn imported_priority(c: &Context<'_>, value: &str) -> Result<()> {
    if value != "lowest" {
        return Ok(());
    }
    let id = c.org_id()?;
    let root = Context { space: &id, ..*c };
    let org = root.get("organization", &id)?;
    if org.fields["priority_levels"] != 5 {
        let mut fields = org.fields.clone();
        fields["priority_levels"] = json!(5);
        root.update(&org, &org.title, fields)?;
    }
    Ok(())
}
fn details(c: &Context<'_>, org: &Record) -> Result<Value> {
    let (language, levels) = defaults(c)?;
    let members = c.records("member")?.len();
    let owner = org.fields["creator"] == c.actor;
    Ok(
        json!({"id":org.id,"name":org.title,"rev":org.rev,"default_language":language,"priority_levels":levels,"members":members,"can_configure":matches!(c.role()?.as_str(),"owner"|"admin"),"can_leave":!owner && c.actor!="owner","can_dissolve":owner && org.id!="default"}),
    )
}
pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    let id = c.org_id()?;
    let root = Context { space: &id, ..*c };
    root.role()?;
    let org = root.get("organization", &id)?;
    if op == "organization.settings" {
        return details(&root, &org);
    }
    if q.get("expected_rev").is_none() {
        return Err(invalid("expected_rev is required"));
    }
    domain::check_rev(q, &org)?;
    if op == "organization.configure" {
        root.require_admin()?;
        if q.get("default_language").is_none() && q.get("priority_levels").is_none() {
            return Err(invalid("choose an organization setting"));
        }
        let mut fields = org.fields.clone();
        for key in ["default_language", "priority_levels"] {
            if let Some(v) = q.get(key) {
                fields[key] = v.clone();
            }
        }
        validate(&fields)?;
        let next = root.update(&org, &org.title, fields)?;
        return details(&root, &next);
    }
    if q["confirmation"] != org.title {
        return Err(invalid("enter the current organization name to confirm"));
    }
    let impact = details(&root, &org)?;
    match required(q, "action")? {
        "leave" => {
            if impact["can_leave"] != true {
                return Err(conflict(
                    "organization owner must retain ownership or dissolve the organization",
                ));
            }
            let member = root.get("member", c.actor)?;
            crate::tasks::cleanup(&root, &member)?;
            crate::worker::allow_delete(&root, &member)?;
            crate::chat::forget_grants(&root, c.actor)?;
            root.remove(&member)?;
        }
        "dissolve" => {
            if impact["can_dissolve"] != true {
                return Err(forbidden(
                    "only the owner of a created organization can dissolve it",
                ));
            }
            if q["members"].as_u64() != impact["members"].as_u64() {
                return Err(conflict("refresh the member count before dissolving"));
            }
            crate::identity::purge_organization(c.db, &id)?;
        }
        _ => return Err(invalid("choose leave or dissolve")),
    }
    Ok(json!({"organization":id,"action":q["action"]}))
}
