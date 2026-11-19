//! Apply only status references and definitions to retained data in a deactivated space.
use super::*;

pub(super) fn inactive(c: &Context<'_>) -> Result<bool> {
    Ok(c.get("space", c.space)?.fields["deactivated"] == true)
}
pub(super) fn create(c: &Context<'_>, id: &str, title: &str, fields: Value) -> Result<()> {
    if !inactive(c)? {
        c.create("status", Some(id), title, fields)?;
        return Ok(());
    }
    let now = crate::now();
    let row = Record {
        space: c.space.to_owned(),
        kind: "status".into(),
        id: id.into(),
        title: title.into(),
        fields,
        rev: c.first_revision("status", id)?,
        created_at: now,
        updated_at: now,
    };
    c.db.execute("INSERT INTO records(space,kind,id,title,fields,rev,created_at,updated_at) VALUES(?,'status',?,?,?,?,?,?)",params![c.space,row.id,row.title,row.fields.to_string(),row.rev,now,now]).map_err(storage)?;
    c.event(&row, "create")
}
pub(super) fn revise(c: &Context<'_>, r: &Record, title: &str, fields: Value) -> Result<Record> {
    if !inactive(c)? {
        return c.update_mode(r, title, fields, true);
    }
    if !matches!(r.kind.as_str(), "task" | "task_type" | "status") {
        return Err(invalid("invalid retained status reference"));
    }
    let next = Record {
        title: title.to_owned(),
        fields,
        rev: r.rev + 1,
        updated_at: crate::now(),
        ..r.clone()
    };
    let changed=c.db.execute("UPDATE records SET title=?,fields=?,rev=?,updated_at=? WHERE space=? AND kind=? AND id=? AND rev=?",params![next.title,next.fields.to_string(),next.rev,next.updated_at,c.space,r.kind,r.id,r.rev]).map_err(storage)?;
    if changed != 1 {
        return Err(conflict("retained status reference changed"));
    }
    c.event(&next, "update")?;
    Ok(next)
}
pub(super) fn remove(c: &Context<'_>, status: &Record) -> Result<()> {
    if !inactive(c)? {
        return c.remove(status);
    }
    let changed =
        c.db.execute(
            "DELETE FROM records WHERE kind='status' AND space=? AND id=? AND rev=?",
            params![c.space, status.id, status.rev],
        )
        .map_err(storage)?;
    if changed != 1 {
        return Err(conflict("retained status changed"));
    }
    let deleted = Record {
        rev: status.rev + 1,
        updated_at: crate::now(),
        ..status.clone()
    };
    c.event(&deleted, "delete")
}
