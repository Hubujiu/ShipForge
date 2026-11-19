use crate::{Context, Record, Result, conflict, invalid, required, storage};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

mod batches;
mod cloud;
mod signals;
pub(crate) use signals::{linear_relay, uses_project};
mod trackers;
pub use trackers::zentao_installation;
mod bugly;
mod writeback;
pub use bugly::bugly_authorization;
mod tapd;
mod zoho;

pub(crate) fn init(db: &Connection) -> Result<()> {
    signals::init(db)?;
    batches::init(db)?;
    writeback::init(db)?;
    bugly::init(db)
}

pub(crate) fn compact_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(super) fn digits(value: &str) -> bool {
    !value.is_empty() && value.len() <= 40 && value.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn validate(c: &Context<'_>, r: &Record) -> Result<()> {
    let f = &r.fields;
    let old = c.find("connector", &r.id)?;
    if let Some(old) = &old
        && [
            "provider",
            "portal_id",
            "zoho_project",
            "tapd_workspace",
            "tapd_auth",
            "company_id",
            "instance_url",
            "ones_url",
            "remote_scope",
            "scope_kind",
            "enterprise_id",
        ]
        .iter()
        .any(|&k| old.fields[k] != f[k])
        && batches::has_ownership(c, &r.id)?
    {
        return Err(conflict(
            "roll back this project's imported tasks before rebinding",
        ));
    }
    if let Some(old) = &old
        && old.fields["provider"] == "bugly"
        && (f["provider"] != "bugly" || old.fields["product_id"] != f["product_id"])
    {
        bugly::disconnect(c, &r.id)?;
    }
    match f["provider"].as_str() {
        Some("linear" | "clickup" | "ones") => cloud::validate(f)?,
        Some("zentao" | "pingcode") => trackers::validate(f)?,
        Some("tapd") => tapd::validate(f)?,
        Some("zoho") => {
            if !digits(required(f, "portal_id")?)
                || f["zoho_project"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .is_some_and(|v| !digits(v))
            {
                return Err(invalid(
                    "Zoho portal and project IDs must be decimal strings",
                ));
            }
        }
        Some("bugly") => {
            if !compact_id(required(f, "product_id")?) {
                return Err(invalid("invalid Bugly product ID"));
            }
            if let Some(t) = f["default_task_type"].as_str().filter(|v| !v.is_empty()) {
                c.readable("task_type", t)?;
            }
            if f.get("bugly_outbound").is_some_and(|v| !v.is_boolean()) {
                return Err(invalid("bugly_outbound must be a boolean"));
            }
        }
        _ => (),
    }
    Ok(())
}

pub(crate) fn removing(c: &Context<'_>, r: &Record) -> Result<()> {
    if r.kind == "task" {
        bugly::cancel_task(c, &r.id)?;
    }
    if r.kind == "connector" {
        if batches::has_ownership(c, &r.id)? {
            return Err(conflict(
                "roll back this project's imported tasks before deleting its connector",
            ));
        }
        bugly::disconnect(c, &r.id)?;
        c.db.execute(
            "DELETE FROM sf_tracker_signal WHERE space=? AND connector=?",
            params![c.space, r.id],
        )
        .map_err(storage)?;
        c.db.execute(
            "DELETE FROM sf_vendor_push WHERE space=? AND connector=?",
            params![c.space, r.id],
        )
        .map_err(storage)?;
        c.db.execute(
            "DELETE FROM sf_import_binding WHERE space=? AND connector=?",
            params![c.space, r.id],
        )
        .map_err(storage)?;
        c.db.execute(
            "DELETE FROM sf_bugly_link WHERE space=? AND connector=?",
            params![c.space, r.id],
        )
        .map_err(storage)?;
        c.db.execute(
            "DELETE FROM sf_bugly_delivery WHERE space=? AND connector=?",
            params![c.space, r.id],
        )
        .map_err(storage)?;
        c.db.execute(
            "DELETE FROM sf_import_batch WHERE space=? AND connector=?",
            params![c.space, r.id],
        )
        .map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn changed(c: &Context<'_>, old: &Record, new: &Record) -> Result<()> {
    if new.kind == "task"
        && new.fields["visibility"] == "private"
        && old.fields["visibility"] != "private"
    {
        bugly::cancel_task(c, &new.id)?;
    }
    if new.kind == "connector"
        && old.fields["bugly_outbound"] == true
        && new.fields["bugly_outbound"] != true
    {
        c.db.execute("UPDATE sf_bugly_delivery SET state='cancelled',lease=NULL WHERE space=? AND connector=? AND state IN('pending','leased')",params![c.space,new.id]).map_err(storage)?;
    }
    if new.kind == "task" && old.fields["status"] != new.fields["status"] {
        bugly::status(c, new)?;
    }
    if new.kind == "connector"
        && new.fields["provider"] == "bugly"
        && new.fields["enabled"] == false
    {
        bugly::disconnect(c, &new.id)?;
    }
    Ok(())
}

pub(super) fn connector(
    c: &Context<'_>,
    id: &str,
    provider: &str,
    allow_disabled: bool,
) -> Result<Record> {
    c.require_admin()?;
    let r = c.readable("connector", id)?;
    if r.fields["provider"] != provider || (!allow_disabled && r.fields["enabled"] == false) {
        return Err(invalid(
            "the connector is disabled or configured for another provider",
        ));
    }
    Ok(r)
}

pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    let id = required(q, "id")?;
    if op.starts_with("bugly.") {
        let r = connector(
            c,
            id,
            "bugly",
            matches!(op, "bugly.revoke" | "bugly.outbox"),
        )?;
        return bugly::execute(c, &r, q, op);
    }
    c.require_admin()?;
    let r = c.readable("connector", id)?;
    let cleanup = op == "imports.binding" && q["clear_device_credentials"] == true;
    if !matches!(
        r.fields["provider"].as_str(),
        Some("zoho" | "tapd" | "zentao" | "pingcode" | "linear" | "clickup" | "ones")
    ) || (r.fields["enabled"] == false
        && !cleanup
        && !matches!(
            op,
            "imports.history" | "imports.rollback" | "imports.push_history" | "imports.finish_push"
        ))
    {
        return Err(invalid("an enabled batch import connector is required"));
    }
    match op {
        "imports.binding" => batches::binding(c, &r, !cleanup),
        "imports.sync_control" => cloud::sync_control(c, &r, required(q, "action")?),
        "imports.prepare_push" | "imports.finish_push" | "imports.push_history" => {
            writeback::execute(c, &r, op, q)
        }
        "imports.stage" => batches::stage(c, &r, q),
        "imports.commit" => batches::commit(c, &r, required(q, "batch")?),
        "imports.history" => batches::history(c, &r),
        "imports.rollback" => batches::rollback(c, &r, required(q, "batch")?),
        _ => Err(invalid("unknown vendor operation")),
    }
}

pub(crate) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    for (ops, fields) in [
        (
            vec![
                "imports.binding",
                "imports.history",
                "bugly.rotate",
                "bugly.revoke",
                "bugly.outbox",
                "bugly.claim",
                "bugly.binding",
            ],
            vec!["id"],
        ),
        (
            vec!["imports.stage"],
            vec!["id", "project", "issues", "binding_rev", "checkpoint"],
        ),
        (
            vec!["imports.commit", "imports.rollback"],
            vec!["id", "batch"],
        ),
        (vec!["imports.prepare_push"], vec!["id", "task_id"]),
        (vec!["imports.sync_control"], vec!["id", "action"]),
        (
            vec!["imports.finish_push"],
            vec!["id", "delivery", "lease", "result"],
        ),
        (vec!["imports.push_history"], vec!["id"]),
        (
            vec!["bugly.settle"],
            vec!["id", "delivery", "lease", "success"],
        ),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"enum":ops}}},"then":{"required":fields}}));
    }
    for (key, value) in [
        ("batch", json!({"type":"string"})),
        ("issues", json!({"type":"array","maxItems":5000})),
        ("binding_rev", json!({"type":"integer","minimum":1})),
    ] {
        s["properties"][key] = value;
    }
}
