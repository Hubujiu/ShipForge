use crate::{
    Context, Record, Result, as_value, conflict, domain, forbidden, invalid, now, required,
    storage, task_commands, tasks, worker,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

const MODULES: [&str; 5] = [
    "iterations",
    "triage",
    "views",
    "projects",
    "customer_requests",
];
pub(crate) fn check_fields(fields: &serde_json::Map<String, Value>) -> Result<()> {
    if [
        "deactivated",
        "deactivated_at",
        "joined",
        "can_join",
        "can_leave",
        "can_manage",
        "can_manage_content",
        "can_edit_content",
        "navigation",
    ]
    .iter()
    .any(|key| fields.contains_key(*key))
    {
        return Err(invalid(
            "space lifecycle and membership are managed by space operations",
        ));
    }
    Ok(())
}
pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS space_navigation(space TEXT NOT NULL,actor TEXT NOT NULL,module TEXT NOT NULL,pinned INTEGER NOT NULL,PRIMARY KEY(space,actor,module))").map_err(storage)
}
pub(crate) fn ensure_active(c: &Context<'_>) -> Result<()> {
    if c.get("space", c.space)?.fields["deactivated"] == true {
        return Err(forbidden("this space is deactivated and read-only"));
    }
    Ok(())
}
pub(crate) fn guarded(op: &str) -> bool {
    !matches!(
        op,
        "space.restore"
            | "space.deactivate"
            | "space.pin"
            | "chat.organize"
            | "preferences.update"
            | "inbox.configure"
            | "inbox.bulk"
            | "inbox.open"
            | "notification.act"
            | "onboarding.display"
            | "invitation.accept"
    )
}
pub(crate) fn enabled(space: &Record, module: &str) -> bool {
    match module {
        "iterations" => space.fields["iterations_enabled"] != false,
        "triage" => space.fields["triage_enabled"] == true,
        "customer_requests" => space.fields["customer_requests_enabled"] == true,
        _ => true,
    }
}
pub(crate) fn require_module(c: &Context<'_>, module: &str) -> Result<()> {
    if !enabled(&c.get("space", c.space)?, module) {
        return Err(invalid(&format!(
            "enable the {module} module in space settings first"
        )));
    }
    Ok(())
}
pub(crate) fn navigation(c: &Context<'_>) -> Result<Value> {
    c.role()?;
    let space = c.get("space", c.space)?;
    Ok(json!({"space":c.space,"actor":c.actor,"modules":module_states(c,&space)?}))
}
fn module_states(c: &Context<'_>, space: &Record) -> Result<Vec<Value>> {
    let software = space.fields["work_type"].as_str().unwrap_or("software") == "software";
    let mut sql =
        c.db.prepare_cached("SELECT module,pinned FROM space_navigation WHERE space=? AND actor=?")
            .map_err(storage)?;
    let pins = sql
        .query_map(params![c.space, c.actor], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()
        .map_err(storage)?;
    let modules: Vec<_> = MODULES
        .iter()
        .map(|&id| {
            let available = enabled(space, id);
            let pinned = pins.get(id).copied().unwrap_or(
                id == "views"
                    || (software && id == "iterations")
                    || (!software && id == "projects"),
            );
            json!({"id":id,"enabled":available,"pinned":available&&pinned})
        })
        .collect();
    Ok(modules)
}
fn pin(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.role()?;
    let module = required(q, "module")?;
    if !MODULES.contains(&module) {
        return Err(invalid("unknown space module"));
    }
    let value = q["pinned"]
        .as_bool()
        .ok_or_else(|| invalid("pinned must be a boolean"))?;
    if value {
        require_module(c, module)?;
    }
    c.db.execute("INSERT INTO space_navigation(space,actor,module,pinned) VALUES(?,?,?,?) ON CONFLICT(space,actor,module) DO UPDATE SET pinned=excluded.pinned",params![c.space,c.actor,module,value]).map_err(storage)?;
    navigation(c)
}

pub(crate) fn listing(c: &Context<'_>, mut space: Record, metadata_role: &str) -> Result<Record> {
    let dest = Context {
        db: c.db,
        actor: c.actor,
        space: &space.id,
    };
    let role = dest.role().ok();
    let joined = role.is_some();
    if joined {
        space.fields["navigation"] = json!(module_states(&dest, &space)?);
    }
    space.fields["joined"] = json!(joined);
    space.fields["can_join"] = json!(
        !joined && space.fields["visibility"] == "public" && space.fields["deactivated"] != true
    );
    space.fields["can_leave"] = json!(
        joined
            && c.actor != "owner"
            && space.fields["organization"] != space.id
            && space.fields["creator"] != c.actor
            && space.fields["deactivated"] != true
    );
    space.fields["can_manage"] = json!(matches!(metadata_role, "owner" | "admin"));
    space.fields["can_manage_content"] = json!(matches!(role.as_deref(), Some("owner" | "admin")));
    space.fields["can_edit_content"] = json!(
        matches!(
            role.as_deref(),
            Some("owner" | "admin" | "member" | "editor")
        ) && space.fields["deactivated"] != true
    );
    Ok(space)
}
fn membership(c: &Context<'_>, join: bool) -> Result<Value> {
    ensure_active(c)?;
    if !join && c.actor == "owner" {
        return Err(forbidden("space owner membership is permanent"));
    }
    let space = c.get("space", c.space)?;
    let org_id = c.org_id()?;
    if c.space == org_id {
        return Err(invalid(
            "organization membership is managed at the organization level",
        ));
    }
    let org = Context {
        space: &org_id,
        ..*c
    };
    let role = org.role()?;
    let old = c.find("member", c.actor)?;
    if join {
        if old.is_none() {
            if space.fields["visibility"] != "public" || role == "guest" {
                return Err(forbidden("membership in this space requires an invitation"));
            }
            let source = org.get("member", c.actor)?;
            let role = if role == "viewer" { "viewer" } else { "member" };
            c.create(
                "member",
                Some(c.actor),
                &source.title,
                json!({"actor":c.actor,"role":role,"creator":c.actor}),
            )?;
        }
    } else if let Some(member) = old {
        tasks::cleanup(c, &member)?;
        c.remove(&member)?;
    } else {
        c.metadata_role()?;
    }
    Ok(json!({"space":c.space,"actor":c.actor,"joined":join}))
}
fn unfinished(c: &Context<'_>) -> Result<Vec<Record>> {
    let mut work = Vec::new();
    for task in c.records("task")? {
        if !matches!(
            c.status_category(&task)?.as_str(),
            "done" | "cancelled" | "duplicate"
        ) {
            work.push(task);
        }
    }
    Ok(work)
}
fn preview_token(tasks: &[Record]) -> String {
    let mut versions: Vec<_> = tasks.iter().map(|r| (&r.id, r.rev)).collect();
    versions.sort_unstable();
    worker::digest(&json!(versions).to_string())
}
fn preview(c: &Context<'_>) -> Result<Value> {
    c.require_admin()?;
    let space = c.get("space", c.space)?;
    let tasks = unfinished(c)?;
    Ok(
        json!({"space":c.space,"deactivated":space.fields["deactivated"]==true,"rev":space.rev,"unfinished":tasks.len(),"preview_token":preview_token(&tasks)}),
    )
}
fn lifecycle(c: &Context<'_>, q: &Value, restore: bool) -> Result<Value> {
    c.require_admin()?;
    let space = c.get("space", c.space)?;
    domain::check_rev(q, &space)?;
    let was_closed = space.fields["deactivated"] == true;
    if restore != was_closed {
        return Ok(as_value(space));
    }
    let mut fields = space.fields.clone();
    if restore {
        fields["deactivated"] = json!(false);
        fields["deactivated_at"] = Value::Null;
    } else {
        let pending = unfinished(c)?;
        if let Some(token) = q.get("preview_token")
            && token.as_str() != Some(&preview_token(&pending))
        {
            return Err(conflict(
                "unfinished tasks changed; preview this space again",
            ));
        }
        let decision = q
            .get("unfinished")
            .map(|v| {
                v.as_str()
                    .filter(|s| matches!(*s, "move" | "cancel"))
                    .ok_or_else(|| invalid("unfinished must be move or cancel"))
            })
            .transpose()?;
        if !pending.is_empty() && decision.is_none() {
            return Err(conflict(
                "choose move or cancel for the unfinished tasks before deactivating",
            ));
        }
        if decision == Some("move") {
            let target = Context {
                space: required(q, "target")?,
                ..*c
            };
            if target.space == c.space || target.org_id()? != c.org_id()? {
                return Err(invalid("choose a different space in this organization"));
            }
            target.require_edit()?;
            for task in pending {
                task_commands::move_task(
                    c,
                    &json!({"id":task.id,"target":target.space,"expected_rev":task.rev}),
                )?;
            }
        } else {
            for task in pending {
                let mut next = task.fields.clone();
                next["status"] = json!(tasks::target_status(c, &task, "cancelled")?);
                let changed = c.update(&task, &task.title, next)?;
                crate::actions::after(c, Some(&task), &changed)?;
                worker::changed(c, Some(&task), &changed)?;
            }
        }
        worker::cancel_space_runs(c)?;
        fields["deactivated"] = json!(true);
        fields["deactivated_at"] = json!(now());
    }
    Ok(as_value(c.update(&space, &space.title, fields)?))
}
pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    if op != "space.navigation" {
        required(q, "space")?;
    }
    match op {
        "space.navigation" => navigation(c),
        "space.pin" => pin(c, q),
        "space.join" | "space.leave" => membership(c, op == "space.join"),
        "space.deactivate.preview" => preview(c),
        "space.deactivate" | "space.restore" => lifecycle(c, q, op == "space.restore"),
        _ => Err(invalid("unknown space operation")),
    }
}
pub(crate) fn schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"const":"space.pin"}}},"then":{"required":["space","module","pinned"],"properties":{"module":{"enum":MODULES},"pinned":{"type":"boolean"}}}}));
    rules.push(json!({"if":{"properties":{"op":{"enum":["space.join","space.leave","space.deactivate","space.deactivate.preview","space.restore"]}}},"then":{"required":["space"]}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"space.deactivate"}}},"then":{"properties":{"preview_token":{"type":"string","pattern":"^[0-9a-f]{64}$"},"unfinished":{"enum":["cancel","move"]}},"if":{"properties":{"unfinished":{"const":"move"}},"required":["unfinished"]},"then":{"required":["target"]}}}));
    rules.push(json!({"if":{"required":["kind"],"properties":{"kind":{"const":"customer_request"},"op":{"enum":["create","update"]}}},"then":{"properties":{"fields":{"type":"object","properties":{"status":{"enum":["open","in_progress","resolved"]},"contact":{"type":"string"},"description":{"type":"string"}}}}}}));
    rules.push(json!({"if":{"required":["kind"],"properties":{"kind":{"const":"space"},"op":{"enum":["create","update"]}}},"then":{"properties":{"fields":{"type":"object","properties":{"triage_enabled":{"type":"boolean"},"iterations_enabled":{"type":"boolean"},"customer_requests_enabled":{"type":"boolean"}}}}}}));
}
