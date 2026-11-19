use super::*;
use crate::tasks::{cleanup as validate_delete, validate_relation, validate_task};
use serde_json::Map;
use uuid::Uuid;

pub(crate) const BASE_STATUS: &[&str] = &[
    "triage",
    "backlog",
    "todo",
    "in_progress",
    "done",
    "cancelled",
    "duplicate",
];
pub(crate) fn check_kind(kind: &str) -> Result<()> {
    if protocol::KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(invalid("unknown record kind"))
    }
}
pub(crate) fn fields_object(value: Option<&Value>) -> Result<Map<String, Value>> {
    value.map_or_else(
        || Ok(Map::new()),
        |v| {
            v.as_object()
                .cloned()
                .ok_or_else(|| invalid("fields must be an object"))
        },
    )
}
pub(crate) fn check_rev(q: &Value, old: &Record) -> Result<()> {
    if let Some(v) = q.get("expected_rev") {
        let rev = v
            .as_i64()
            .filter(|i| *i >= 1)
            .ok_or_else(|| invalid("expected_rev must be a positive integer"))?;
        if rev != old.rev {
            return Err(conflict(&format!(
                "expected revision {rev}, current revision {}",
                old.rev
            )));
        }
    }
    Ok(())
}
pub(crate) fn write(c: &Context<'_>, q: &Value) -> Result<Value> {
    write_mode(c, q, true)
}
pub(crate) fn write_mode(c: &Context<'_>, q: &Value, notify: bool) -> Result<Value> {
    write_inner(c, q, notify, None, Origin::Member)
}
pub(crate) fn write_import(c: &Context<'_>, q: &Value, notify: bool) -> Result<Value> {
    write_inner(c, q, notify, None, Origin::Source)
}
pub(crate) fn write_with_files(c: &Context<'_>, q: &Value, files: Value) -> Result<Value> {
    write_inner(c, q, true, Some(files), Origin::Member)
}
pub(crate) fn write_agent_comment(c: &Context<'_>, q: &Value, agent: &str) -> Result<Value> {
    write_inner(c, q, true, None, Origin::Agent(agent))
}
pub(crate) fn write_submission(c: &Context<'_>, q: &Value) -> Result<Value> {
    write_inner(c, q, true, None, Origin::Submission)
}
pub(crate) fn write_review(c: &Context<'_>, q: &Value) -> Result<Value> {
    write_inner(c, q, true, None, Origin::Review)
}
enum Origin<'a> {
    Member,
    Source,
    Submission,
    Review,
    Agent(&'a str),
}
fn write_inner(
    c: &Context<'_>,
    q: &Value,
    notify: bool,
    files: Option<Value>,
    origin: Origin<'_>,
) -> Result<Value> {
    let from_source = matches!(origin, Origin::Source);
    let submission = matches!(origin, Origin::Submission);
    let op = required(q, "op")?;
    let kind = required(q, "kind")?;
    check_kind(kind)?;
    if kind == "invitation" {
        return Err(invalid(
            "use an invitation operation to issue, copy or revoke a join link",
        ));
    }
    if matches!(kind, "space" | "organization") && op == "create" {
        return create_space(c, q);
    }
    let old = if op == "create" {
        None
    } else {
        let id = required(q, "id")?;
        identifier(id)?;
        let row = c.readable(kind, id)?;
        check_rev(q, &row)?;
        Some(row)
    };
    if !(submission && kind == "task" && op == "create" && crate::triage::intake::outside(c)?) {
        c.authorize_write(kind, old.as_ref())?;
    }
    if op == "delete" {
        let r = old.as_ref().ok_or_else(|| invalid("missing record"))?;
        validate_delete(c, r)?;
        if kind == "task" {
            crate::triage::forget(c, &r.id)?;
        }
        if kind == "customer_request" {
            c.db.execute(
                "DELETE FROM triage_conversions WHERE space=? AND request=?",
                params![c.space, r.id],
            )
            .map_err(storage)?;
        }
        if kind == "comment" {
            crate::discussions::delete(c, r)?;
        }
        if kind == "view" {
            crate::saved_views::forget(c, &r.id)?;
        }
        worker::allow_delete(c, r)?;
        if kind == "chat" {
            crate::chat::before_delete(c, r)?;
        }
        c.remove(r)?;
        return Ok(as_value(r.clone()));
    }
    let mut patch = fields_object(q.get("fields"))?;
    if matches!(kind, "agent" | "crew") {
        crate::agents::protect(q, old.as_ref())?;
    }
    match kind {
        "comment" => {
            crate::discussions::protect(c, &mut patch, old.as_ref(), from_source)?;
            if let Origin::Agent(agent) = origin {
                patch.insert("authored_by_agent".into(), json!(agent));
            }
        }
        "task" => crate::task_fields::prepare_task(c, old.as_ref(), &mut patch, q, from_source)?,
        "task_type" => crate::task_fields::prepare_type(c, old.as_ref(), &mut patch, q)?,
        "view" if patch.contains_key("personally_pinned") => {
            return Err(invalid("use view.organize for personal order"));
        }
        _ => {}
    }
    let infer_iteration = kind == "task" && !from_source && !patch.contains_key("iteration");
    let explicit_unplanned = kind == "task" && patch.get("iteration").is_some_and(Value::is_null);
    if kind == "iteration" {
        for field in ["cadence", "sequence", "closed_at"] {
            if from_source {
                patch.remove(field);
            } else if patch.contains_key(field) {
                return Err(invalid(
                    "the iteration scheduler manages cadence and closure metadata",
                ));
            }
        }
    }
    if kind == "space" {
        if old
            .as_ref()
            .is_some_and(|r| r.fields["triage_enabled"] == true)
            && patch.get("triage_enabled") == Some(&json!(false))
            && q["confirm"] != true
        {
            return Err(conflict(
                "confirm moving pending tasks to backlog before disabling triage",
            ));
        }
        crate::triage::protect_space(&patch)?;
        crate::spaces::check_fields(&patch)?;
    }
    if kind == "customer_request" {
        if ["converted_task", "converted_space"]
            .iter()
            .any(|key| patch.contains_key(*key))
        {
            return Err(invalid("use customer_request.convert to link a task"));
        }
        crate::spaces::require_module(c, "customer_requests")?;
    }
    if kind == "task" {
        crate::task_commands::assignees::prepare(c, old.as_ref(), &mut patch, from_source)?;
    }
    if kind == "chat" && patch.contains_key("conversation") {
        return Err(invalid(
            "conversation controls are managed by chat operations",
        ));
    }
    if kind == "task" && !matches!(origin, Origin::Review) {
        for key in [
            "triage_decision",
            "triage_entered_at",
            "triage_duplicate_of",
            "triage_applied_rules",
            "triage_rule_failed",
            "triage_snoozed",
            "deferred_until",
        ] {
            if from_source {
                patch.remove(key);
            } else if patch.contains_key(key) {
                return Err(invalid("use a triage operation to update queue metadata"));
            }
        }
    }
    if kind == "task" && patch.contains_key("following") {
        return Err(invalid("use follow to change your own following status"));
    }
    if kind == "task" && patch.contains_key("im_threads") {
        return Err(invalid("use im.bind to manage external topics"));
    }
    if patch.contains_key("attachments") {
        return Err(invalid("use attachment.upload to attach files"));
    }
    if let Some(files) = files {
        patch.insert("attachments".into(), files);
    }
    if ["creator", "number", "system"]
        .iter()
        .any(|k| patch.contains_key(*k))
    {
        return Err(invalid(
            "creator, number and system are assigned by the workspace",
        ));
    }
    let title = match q.get("title") {
        Some(v) => v
            .as_str()
            .ok_or_else(|| invalid("title must be a string"))?,
        None => old.as_ref().map(|r| r.title.as_str()).unwrap_or(""),
    }
    .trim();
    if title.is_empty() || title.chars().count() > 500 {
        return Err(invalid("title must have 1–500 characters"));
    }
    let external_id = q
        .get("id")
        .map(|v| v.as_str().ok_or_else(|| invalid("id must be a string")))
        .transpose()?;
    let id = if kind == "member" && old.is_none() {
        let id = patch
            .get("actor")
            .and_then(Value::as_str)
            .or(external_id)
            .ok_or_else(|| invalid("member needs fields.actor or id"))?
            .to_owned();
        if external_id.is_some_and(|val| val != id) {
            return Err(invalid("member id must match actor"));
        }
        patch.insert("actor".into(), json!(id));
        id
    } else {
        external_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string())
    };
    identifier(&id)?;
    let mut f = old
        .as_ref()
        .map(|r| r.fields.clone())
        .unwrap_or_else(|| json!({"creator":c.actor}));
    for (k, v) in patch {
        f[&k] = v;
    }
    let at = now();
    let rev = if let Some(previous) = &old {
        previous.rev + 1
    } else {
        c.first_revision(kind, &id)?
    };
    let mut row = Record {
        space: c.space.to_owned(),
        kind: kind.into(),
        id,
        title: title.into(),
        fields: f,
        rev,
        created_at: at,
        updated_at: at,
    };
    let explicit_admin_status =
        !from_source && old.is_none() && kind == "task" && q["fields"]["status"].is_string() && {
            let org = c.org_id()?;
            matches!(
                (Context { space: &org, ..*c }).role().as_deref(),
                Ok("owner" | "admin")
            )
        };
    validate(
        c,
        old.as_ref(),
        &mut row,
        from_source,
        from_source && q["preserve_source_status"] == true || explicit_admin_status,
    )?;
    let entered = crate::triage::stamp(c, old.as_ref(), &mut row)?;
    if infer_iteration {
        crate::iterations::auto_assign(c, old.as_ref(), &mut row)?;
    }
    worker::validate(c, old.as_ref(), &row)?;
    if matches!(kind, "status" | "task_type")
        && row.fields["default"] == true
        && old.as_ref().is_none_or(|r| r.fields["default"] != true)
    {
        for previous in c.records(kind)? {
            if previous.id != row.id && previous.fields["default"] == true {
                let mut f = previous.fields.clone();
                f["default"] = json!(false);
                c.update(&previous, &previous.title, f)?;
            }
        }
    }
    let row = if let Some(previous) = &old {
        c.update_mode(previous, &row.title, row.fields, notify)?
    } else {
        c.insert(&row)?;
        row
    };
    if kind == "task"
        && (explicit_unplanned
            || from_source
                && old.as_ref().is_none_or(|r| {
                    r.fields["status"] != row.fields["status"]
                        || r.fields["due_date"] != row.fields["due_date"]
                }))
    {
        crate::iterations::record_override(c, &row)?;
    }
    crate::iterations::supplement(c, old.as_ref(), &row)?;
    if notify {
        crate::actions::after(c, old.as_ref(), &row)?;
        crate::tags::changed(c, old.as_ref(), &row)?;
        worker::changed(c, old.as_ref(), &row)?;
        if old.is_none() && kind == "comment" {
            crate::im::changed(c, None, &row)?;
        }
        if old.is_none() && kind == "message" {
            crate::chat::notify(c, &row)?;
        }
    }
    if entered && notify && !(from_source && q["preserve_source_status"] == true) {
        let routed = crate::triage::entered(c, &row)?;
        let target = Context {
            space: &routed.space,
            ..*c
        };
        if target.role().is_ok() && target.visible(&routed)? {
            return Ok(as_value(routed));
        }
    }
    Ok(as_value(row))
}

fn create_space(c: &Context<'_>, q: &Value) -> Result<Value> {
    let is_org = q["kind"] == "organization";
    let default_org = c.org_id()?;
    let org_id = q["fields"]["organization"].as_str().unwrap_or(&default_org);
    let org = Context {
        db: c.db,
        space: org_id,
        actor: c.actor,
    };
    org.get("organization", org_id)?;
    org.require_admin()?;
    let title = required(q, "title")?.trim();
    if title.chars().count() > 500 {
        return Err(invalid("space title is too long"));
    }
    let id = q["id"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    identifier(&id)?;
    let dest = Context {
        db: c.db,
        actor: c.actor,
        space: &id,
    };
    let mut f = Value::Object(fields_object(q.get("fields"))?);
    crate::triage::protect_space(f.as_object().unwrap())?;
    crate::spaces::check_fields(f.as_object().unwrap())?;
    f["creator"] = json!(c.actor);
    f["organization"] = if is_org { json!(&id) } else { json!(org_id) };
    if f["visibility"].is_null() {
        f["visibility"] = json!("private");
    }
    validate_space(&f)?;
    let start_types = f
        .as_object_mut()
        .expect("space fields")
        .remove("task_types");
    let work = f["work_type"].as_str().map(str::to_owned);
    if start_types.is_some() && work.is_none() {
        return Err(invalid(
            "select a work type before choosing starting task types",
        ));
    }
    if let Some(ref work) = work {
        f["agent_type_default"] = json!(if work == "software" { "code" } else { "office" });
    }
    let row = dest.create("space", Some(&id), title, f)?;
    dest.create(
        "member",
        Some(c.actor),
        c.actor,
        json!({"actor":c.actor,"role":if c.actor=="owner"{"owner"}else{"admin"},"creator":c.actor}),
    )?;
    crate::defaults::seed(&dest)?;
    if let Some(work) = work {
        crate::defaults::starting_task_types(&dest, &work, start_types.as_ref())?;
    }
    if is_org {
        return Ok(as_value(dest.create(
            "organization",
            Some(&id),
            title,
            json!({"creator":c.actor,"visibility":"space"}),
        )?));
    }
    Ok(as_value(row))
}
pub(crate) fn validate_space(f: &Value) -> Result<()> {
    if !matches!(f["visibility"].as_str(), Some("private" | "public")) {
        return Err(invalid("space visibility must be private or public"));
    }
    if f.get("key").is_some_and(|v| {
        !v.as_str().is_some_and(|s| {
            !s.is_empty() && s.len() <= 12 && s.chars().all(|c| c.is_ascii_alphanumeric())
        })
    }) {
        return Err(invalid("space key must be 1–12 ASCII letters or digits"));
    }
    for k in [
        "triage_enabled",
        "triage_priority_required",
        "estimates_enabled",
        "allow_multiple_assignees",
        "iterations_enabled",
        "customer_requests_enabled",
    ] {
        if f.get(k).is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("space feature settings must be boolean"));
        }
    }
    if let Some(v) = f.get("triage_sources") {
        for key in strings(v, "triage_sources")? {
            if !matches!(key, "guest" | "external") {
                return Err(invalid("triage sources are guest and external"));
            }
        }
    }
    if let Some(v) = f.get("triage_types") {
        for id in strings(v, "triage_types")? {
            identifier(id)?;
        }
    }
    Ok(())
}

pub(crate) fn ref_record(
    c: &Context<'_>,
    f: &Value,
    k: &str,
    kind: &str,
) -> Result<Option<Record>> {
    match f.get(k).filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => {
            let id = v
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid(&format!("{k} must be a record id")))?;
            c.readable(kind, id).map(Some)
        }
    }
}
fn validate(
    c: &Context<'_>,
    old: Option<&Record>,
    row: &mut Record,
    from_source: bool,
    historical: bool,
) -> Result<()> {
    if row.kind == "iteration" {
        crate::iterations::manual_fields(c, old, row, from_source)?;
    }
    let f = &mut row.fields;
    if let Some(ex) = f.get("external_id").filter(|v| !v.is_null())
        && (!ex.as_str().is_some_and(|s| !s.is_empty())
            || !f["connector"].as_str().is_some_and(|s| !s.is_empty()))
    {
        return Err(invalid(
            "external records require connector and external_id",
        ));
    }
    match row.kind.as_str() {
        "customer_request" => {
            if f["status"].is_null() {
                f["status"] = json!("open");
            }
            if !matches!(
                f["status"].as_str(),
                Some("open" | "in_progress" | "resolved")
            ) {
                return Err(invalid("unknown customer request status"));
            }
            if ["contact", "description"]
                .iter()
                .any(|key| f.get(*key).is_some_and(|v| !v.is_string()))
            {
                return Err(invalid(
                    "customer request contact and description must be strings",
                ));
            }
        }
        "tag" | "tag_group" => return crate::tags::validate(c, old, row),
        "task" => validate_task(c, old, &row.id, f, historical)?,
        "agent" => {
            if old.is_none() && f["agent_type"].is_null() && !f["parent_id"].is_string() {
                let space = c.get("space", c.space)?;
                f["agent_type"] = json!(
                    space.fields["agent_type_default"]
                        .as_str()
                        .unwrap_or("code")
                );
            }
            if !f["agent_type"].is_null()
                && !matches!(f["agent_type"].as_str(), Some("code" | "office"))
            {
                return Err(invalid("agent_type must be code or office"));
            }
        }
        "space" => {
            if let Some(prior) = old
                && f["organization"] != prior.fields["organization"]
            {
                return Err(invalid("space organization is immutable"));
            }
            if let Some(prior) = old
                && [
                    ("iterations_enabled", true),
                    ("triage_enabled", false),
                    ("customer_requests_enabled", false),
                ]
                .iter()
                .any(|(key, default)| {
                    f[*key].as_bool().unwrap_or(*default)
                        != prior.fields[*key].as_bool().unwrap_or(*default)
                })
            {
                c.require_admin()?;
            }
            validate_space(f)?;
        }
        "organization" => {
            if row.id != c.space || c.org_id()? != c.space {
                return Err(invalid("organization updates belong in its root space"));
            }
        }
        "member" => {
            let org_id = c.org_id()?;
            if org_id != c.space && row.id != "owner" {
                let anchor = Context {
                    db: c.db,
                    actor: c.actor,
                    space: &org_id,
                };
                if anchor.find("member", &row.id)?.is_none() {
                    return Err(invalid(
                        "add the actor to the organization before assigning a space role",
                    ));
                }
            }
            if f["actor"].as_str() != Some(&row.id) {
                return Err(invalid("member identity is immutable"));
            }
            if !matches!(
                f["role"].as_str(),
                Some("admin" | "member" | "viewer" | "guest" | "editor" | "owner")
            ) {
                return Err(invalid("unknown member role"));
            }
            if (row.id == "owner") != (f["role"] == "owner") {
                return Err(forbidden("the workspace owner role is reserved"));
            }
        }
        "relation" => validate_relation(c, &row.id, f)?,
        "status" => {
            let category = required(f, "category")?;
            if !BASE_STATUS.contains(&category) {
                return Err(invalid("unknown status category"));
            }
            if BASE_STATUS.contains(&row.id.as_str()) && (old.is_none() || category != row.id) {
                return Err(invalid("built-in status categories are fixed"));
            }
            if let Some(prev) = old
                && prev.fields["category"] != f["category"]
                && c.records("task")?
                    .iter()
                    .any(|r| r.fields["status"] == row.id)
            {
                return Err(conflict("status category is in use"));
            }
        }
        "task_type" => {
            if let Some(v) = f.get("statuses") {
                for id in strings(v, "statuses")? {
                    c.category_for(id)?;
                }
            }
            if let Some(v) = f.get("required_fields") {
                for id in strings(v, "required_fields")? {
                    c.readable("field", id)?;
                }
            }
        }
        "field" => {
            if !f["value_type"].is_null()
                && !matches!(
                    f["value_type"].as_str(),
                    Some("text" | "number" | "boolean" | "date" | "select" | "member" | "members")
                )
            {
                return Err(invalid("unknown custom field value_type"));
            }
            if let Some(options) = f.get("options") {
                strings(options, "options")?;
            }
        }
        "project" => {
            crate::project_timeline::validate(c, old, f)?;
            let org = c.org_id()?;
            f["organization"] = json!(org);
            if let Some(spaces) = f.get("spaces") {
                for id in strings(spaces, "spaces")? {
                    let target = Context {
                        db: c.db,
                        space: id,
                        actor: c.actor,
                    };
                    if target.org_id()? != org {
                        return Err(invalid(
                            "project spaces must belong to the same organization",
                        ));
                    }
                    target.require_edit()?;
                }
            }
            if let Some(previous) = old
                && let Some(spaces) = previous.fields["spaces"].as_array()
            {
                for removed in spaces.iter().filter_map(Value::as_str).filter(|old_id| {
                    !f["spaces"]
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|id| id == old_id))
                }) {
                    if removed == c.space {
                        continue;
                    }
                    let scope = Context {
                        db: c.db,
                        space: removed,
                        actor: c.actor,
                    };
                    if scope
                        .records("task")?
                        .iter()
                        .any(|t| t.fields["project"] == row.id)
                    {
                        return Err(conflict(
                            "remove the space's task assignments before unlinking its project",
                        ));
                    }
                }
            }
        }
        "iteration" => {
            // Timeline statistics are computed for the requesting member.
            f.as_object_mut().expect("record fields").remove("planning");
            if let Some(capacity) = f.get("capacity").filter(|v| !v.is_null())
                && capacity.as_u64().is_none_or(|n| n == 0 || n > 1_000_000)
            {
                return Err(invalid(
                    "iteration capacity must be a positive task count up to 1000000",
                ));
            }
            for key in ["start", "end", "start_date", "end_date"] {
                if let Some(v) = f.get(key).filter(|v| !v.is_null()) {
                    valid_date(v)?;
                }
            }
            if let (Some(start), Some(end)) = (
                f["start_date"].as_str().or(f["start"].as_str()),
                f["end_date"].as_str().or(f["end"].as_str()),
            ) && start > end
            {
                return Err(invalid("iteration end precedes start"));
            }
            if old.is_none() && f["status"].is_null() {
                f["status"] = json!("planned");
            }
            if !from_source
                && f["status"] == "closed"
                && old.is_none_or(|r| r.fields["status"] != "closed")
            {
                return Err(invalid("use iteration.close to finish an iteration"));
            }
        }
        "comment" => {
            let _: Record =
                ref_record(c, f, "task", "task")?.ok_or_else(|| invalid("comment needs a task"))?;
            if let Some(p) = ref_record(c, f, "parent", "comment")?
                && p.fields["task"] != f["task"]
            {
                return Err(invalid("reply must belong to the same task"));
            }
            validate_body(f, &row.title)?;
            validate_mentions(c, f)?;
        }
        "message" => {
            let room =
                ref_record(c, f, "chat", "chat")?.ok_or_else(|| invalid("message needs a chat"))?;
            if old.is_some_and(|r| r.fields["chat"] != f["chat"]) {
                return Err(invalid("a message cannot change its conversation"));
            }
            crate::chat::validate_mentions(c, &room, f)?;
            if old.is_none() {
                crate::agents::chat_available(c, &room)?;
            }
            validate_body(f, &row.title)?;
        }
        "artifact" => {
            for (k, t) in [
                ("task", "task"),
                ("chat", "chat"),
                ("message", "message"),
                ("document", "document"),
                ("run", "run"),
            ] {
                ref_record(c, f, k, t)?;
            }
        }
        "view" => {
            if f.get("filter").is_some_and(|v| !v.is_object()) {
                return Err(invalid("view filter must be an object"));
            }
            if !f["layout"].is_null()
                && !matches!(f["layout"].as_str(), Some("board" | "list" | "table"))
            {
                return Err(invalid("unknown view layout"));
            }
        }
        "document" | "note" if f.get("body").is_some_and(|v| !v.is_string()) => {
            return Err(invalid("document body must be text"));
        }
        _ => {}
    }
    if matches!(row.kind.as_str(), "status" | "task_type" | "relation_type") {
        if f.get("order").is_some_and(|v| !v.is_i64()) {
            return Err(invalid("order must be an integer"));
        }
        if f.get("default").is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("default must be a boolean"));
        }
    }
    if matches!(row.kind.as_str(), "view" | "document" | "note" | "chat") {
        if f["visibility"].is_null() {
            f["visibility"] = json!(if row.kind == "chat" {
                "private"
            } else {
                "space"
            });
        }
        if !matches!(
            f["visibility"].as_str(),
            Some("private" | "space" | "team" | "public")
        ) {
            return Err(invalid("unknown visibility"));
        }
        // Private records are owned by their creator. Prevent changing ownership by patch.
        if f["visibility"] == "private" {
            f["owner"] = f["creator"].clone();
        }
    }
    if row.kind == "chat" {
        crate::chat::check_binding(c, old, row)?;
        crate::chat::validate(old, row)?;
    }
    Ok(())
}
pub(crate) fn strings<'a>(v: &'a Value, key: &str) -> Result<Vec<&'a str>> {
    v.as_array()
        .ok_or_else(|| invalid(&format!("{key} must be a string array")))?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid(&format!("{key} contains an invalid string")))
        })
        .collect()
}
pub(crate) fn valid_date(value: &Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| invalid("date must use YYYY-MM-DD"))?;
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| invalid("date must use YYYY-MM-DD"))?;
    Ok(())
}
fn validate_mentions(c: &Context<'_>, f: &Value) -> Result<()> {
    if let Some(v) = f.get("mentions") {
        for actor in strings(v, "mentions")? {
            c.readable("member", actor)?;
        }
    }
    Ok(())
}
fn validate_body(f: &mut Value, title: &str) -> Result<()> {
    if f["body"].is_null() {
        f["body"] = json!(title);
    }
    required(f, "body")?;
    Ok(())
}
impl Context<'_> {
    pub(crate) fn category_for(&self, status: &str) -> Result<String> {
        if BASE_STATUS.contains(&status) {
            return Ok(status.to_owned());
        }
        Ok(required(&self.readable("status", status)?.fields, "category")?.to_owned())
    }
    pub(crate) fn status_category(&self, task: &Record) -> Result<String> {
        self.category_for(required(&task.fields, "status")?)
    }
}
