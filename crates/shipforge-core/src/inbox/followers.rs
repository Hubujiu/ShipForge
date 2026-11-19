use super::*;
pub(super) fn members(c: &Context<'_>, task: &Record) -> Result<BTreeSet<String>> {
    let mut stmt =
        c.db.prepare("SELECT actor FROM followers WHERE space=?1 AND task=?2")
            .map_err(storage)?;
    stmt.query_map(params![c.space, task.id], |r| r.get(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
}
fn values(v: &Value) -> impl Iterator<Item = &str> {
    v.as_str()
        .into_iter()
        .chain(v.as_array().into_iter().flatten().filter_map(Value::as_str))
}
fn automatic_people(c: &Context<'_>, fields: &Value) -> Result<BTreeSet<String>> {
    let mut people: BTreeSet<String> = ["creator", "owner", "mentions"]
        .into_iter()
        .flat_map(|k| values(&fields[k]))
        .chain(crate::task_commands::assignees::people(fields))
        .map(str::to_owned)
        .collect();
    if let Some(custom) = fields["custom"].as_object() {
        for (id, value) in custom {
            if let Some(field) = c.find("field", id)?
                && matches!(
                    field.fields["value_type"].as_str(),
                    Some("member" | "members")
                )
            {
                people.extend(values(value).map(str::to_owned));
            }
        }
    }
    Ok(people)
}
pub(super) fn automatic(
    c: &Context<'_>,
    task: &Record,
    row: &Record,
    old: Option<&Record>,
) -> Result<()> {
    let added = if row.kind == "comment" {
        let mut people = BTreeSet::new();
        if !row.fields["authored_by_agent"].is_string() {
            people.insert(c.actor.to_owned());
        }
        people.extend(values(&row.fields["mentions"]).map(str::to_owned));
        people
    } else {
        let previous = old
            .map(|r| automatic_people(c, &r.fields))
            .transpose()?
            .unwrap_or_default();
        automatic_people(c, &task.fields)?
            .difference(&previous)
            .cloned()
            .collect()
    };
    for id in added {
        let person = Context { actor: &id, ..*c };
        if person.role().is_ok() && person.visible(task)? {
            c.db.execute(
                "INSERT OR IGNORE INTO followers(space,task,actor) VALUES(?1,?2,?3)",
                params![c.space, task.id, id],
            )
            .map_err(storage)?;
        }
    }
    Ok(())
}
pub(super) fn execute(c: &Context<'_>, q: &Value, write: bool) -> Result<Value> {
    let task = c.readable("task", required(q, "id")?)?;
    if write {
        let kind = q["target_kind"].as_str().unwrap_or("member");
        let id = required(q, "target")?;
        if kind != "member" || id != c.actor {
            c.require_edit()?;
        }
        let enabled = q["enabled"]
            .as_bool()
            .ok_or_else(|| invalid("enabled must be a boolean"))?;
        if !matches!(kind, "member" | "agent" | "invitation") {
            return Err(invalid("unknown follower kind"));
        }
        if enabled {
            let org = c.org_id()?;
            let scope = if kind == "invitation" {
                Context { space: &org, ..*c }
            } else {
                Context { ..*c }
            };
            let target = scope.readable(kind, id)?;
            if kind == "invitation"
                && (!crate::invitations::active(&target)
                    || target.fields["channel"] != "email"
                    || (c.space != org && !values(&target.fields["spaces"]).any(|s| s == c.space)))
            {
                return Err(invalid(
                    "choose an active addressed invitation to this space",
                ));
            }
            if kind == "agent" && target.fields["archived"] == true {
                return Err(invalid("cannot add archived Agent"));
            }
            if kind == "member" {
                let person = Context { actor: id, ..*c };
                person.role()?;
                if !person.visible(&task)? {
                    return Err(forbidden("member cannot access this task"));
                }
                let added =
                    c.db.execute(
                        "INSERT OR IGNORE INTO followers(space,task,actor) VALUES(?1,?2,?3)",
                        params![c.space, task.id, id],
                    )
                    .map_err(storage)?;
                if added == 1 && id != c.actor {
                    task_notice(c, &task, id, "follower_added", None, false)?;
                }
            } else {
                c.db.execute("INSERT OR IGNORE INTO follower_objects(space,task,kind,id) VALUES(?1,?2,?3,?4)",params![c.space,task.id,kind,id]).map_err(storage)?;
            }
        } else if kind == "member" {
            c.db.execute(
                "DELETE FROM followers WHERE space=?1 AND task=?2 AND actor=?3",
                params![c.space, task.id, id],
            )
            .map_err(storage)?;
        } else {
            c.db.execute(
                "DELETE FROM follower_objects WHERE space=?1 AND task=?2 AND kind=?3 AND id=?4",
                params![c.space, task.id, kind, id],
            )
            .map_err(storage)?;
        }
    }
    let mut ids = members(c, &task)?
        .into_iter()
        .map(|id| ("member".to_owned(), id))
        .collect::<Vec<_>>();
    let mut stmt = c
        .db
        .prepare("SELECT kind,id FROM follower_objects WHERE space=?1 AND task=?2 ORDER BY kind,id")
        .map_err(storage)?;
    ids.extend(
        stmt.query_map(params![c.space, task.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?,
    );
    let mut result = Vec::new();
    let org = c.org_id()?;
    for (kind, id) in ids {
        let scope = if kind == "invitation" {
            Context { space: &org, ..*c }
        } else {
            Context { ..*c }
        };
        if let Ok(r) = scope.readable(&kind, &id) {
            result.push(json!({"kind":kind,"id":id,"title":r.title}));
        }
    }
    Ok(json!({"task":task.id,"followers":result}))
}
pub(crate) fn accept_invited_follower(
    org: &Context<'_>,
    invitation: &str,
    actor: &str,
) -> Result<()> {
    let mut stmt=org.db.prepare("SELECT f.space,f.task FROM follower_objects f JOIN records s ON s.kind='space' AND s.id=f.space AND s.space=f.space WHERE f.kind='invitation' AND f.id=?1 AND COALESCE(json_extract(s.fields,'$.organization'),'default')=?2").map_err(storage)?;
    let tasks = stmt
        .query_map(params![invitation, org.space], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (space, task) in tasks {
        let scope = Context {
            space: &space,
            actor,
            ..*org
        };
        if scope.role().is_ok() && scope.readable("task", &task).is_ok() {
            org.db
                .execute(
                    "INSERT OR IGNORE INTO followers(space,task,actor)VALUES(?1,?2,?3)",
                    params![space, task, actor],
                )
                .map_err(storage)?;
        }
        org.db.execute("DELETE FROM follower_objects WHERE space=?1 AND task=?2 AND kind='invitation' AND id=?3",params![space,task,invitation]).map_err(storage)?;
    }
    Ok(())
}
