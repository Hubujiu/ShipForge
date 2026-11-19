use crate::{Context, Record, Result, conflict, domain, forbidden, invalid, json, required};
use serde_json::Value;

pub(crate) fn migrate_owners(db: &rusqlite::Connection) -> Result<()> {
    // Older verified-account organizations stored their creator as an administrator.
    db.execute("UPDATE records AS m SET fields=json_set(fields,'$.role','owner'),rev=rev+1
        WHERE m.kind='member' AND EXISTS(SELECT 1 FROM records o WHERE o.space=m.space AND o.kind='organization' AND o.id=m.space AND json_extract(o.fields,'$.creator')=m.id)
        AND NOT EXISTS(SELECT 1 FROM records p WHERE p.space=m.space AND p.kind='member' AND json_extract(p.fields,'$.role')='owner')",[]).map_err(crate::storage)?;
    Ok(())
}
fn set_role(c: &Context<'_>, member: &Record, role: &str) -> Result<Record> {
    let mut fields = member.fields.clone();
    fields["role"] = json!(role);
    c.update(member, &member.title, fields)
}

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
    let roster = c.records("member")?;
    let members = roster.len();
    let owners = roster
        .iter()
        .filter(|m| m.fields["role"] == "owner")
        .count();
    let owner = roster
        .iter()
        .any(|m| m.id == c.actor && m.fields["role"] == "owner");
    let mut choices = Vec::new();
    if owner {
        for m in &roster {
            choices.push(json!({"id":m.id,"name":m.title,"role":m.fields["role"]}));
        }
    }
    Ok(
        json!({"id":org.id,"name":org.title,"rev":org.rev,"default_language":language,"priority_levels":levels,"members":members,"owners":owners,"ownership_choices":choices,"can_manage_owners":owner,"can_configure":matches!(c.role()?.as_str(),"owner"|"admin"),"can_leave":(!owner||owners>1) && c.actor!="owner","can_dissolve":owner && org.id!="default"}),
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
    if op == "organization.ownership" {
        let impact = details(&root, &org)?;
        if impact["can_manage_owners"] != true {
            return Err(forbidden("organization owner required"));
        }
        let target = root.get("member", required(q, "target")?)?;
        let action = required(q, "action")?;
        match action {
            "grant" | "transfer" => {
                if target.id == c.actor {
                    return Err(invalid("select another organization member"));
                }
                if action == "transfer" && c.actor == "owner" {
                    return Err(forbidden("the installation owner must remain an owner"));
                }
                if target.fields["role"] != "owner" {
                    set_role(&root, &target, "owner")?;
                }
                if action == "transfer" {
                    set_role(&root, &root.get("member", c.actor)?, "admin")?;
                }
            }
            "revoke" => {
                if target.fields["role"] != "owner" {
                    return Err(invalid("select an organization owner"));
                }
                if target.id == "owner" || impact["owners"].as_u64().unwrap_or(0) <= 1 {
                    return Err(conflict("at least one organization owner must remain"));
                }
                set_role(&root, &target, "admin")?;
            }
            _ => return Err(invalid("choose grant, transfer or revoke")),
        }
        let next = root.update(&org, &org.title, org.fields.clone())?;
        return details(&root, &next);
    }
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
            let mut member = root.get("member", c.actor)?;
            if member.fields["role"] == "owner" {
                member = set_role(&root, &member, "admin")?;
            }
            crate::tasks::cleanup(&root, &member)?;
            crate::worker::allow_delete(&root, &member)?;
            crate::chat::forget_grants(&root, c.actor)?;
            root.remove(&member)?;
            // The organization revision protects simultaneous owner changes and destructive confirmations.
            if impact["can_manage_owners"] == true {
                root.update(&org, &org.title, org.fields.clone())?;
            }
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
