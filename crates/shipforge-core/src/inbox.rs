use super::*;
use std::collections::BTreeSet;
mod followers;
mod listing;
mod settings;
pub(crate) use followers::accept_invited_follower;

pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS inbox_settings(org TEXT NOT NULL,actor TEXT NOT NULL,settings TEXT NOT NULL,PRIMARY KEY(org,actor));
        CREATE TABLE IF NOT EXISTS inbox_signals(seq INTEGER PRIMARY KEY AUTOINCREMENT,org TEXT NOT NULL,space TEXT NOT NULL,actor TEXT NOT NULL,event TEXT NOT NULL,notice TEXT NOT NULL,source_kind TEXT,source_id TEXT,desktop INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS inbox_desktop_cursor ON inbox_signals(org,actor,desktop,seq);
        CREATE INDEX IF NOT EXISTS inbox_subject ON inbox_signals(space,actor,notice);
        CREATE TABLE IF NOT EXISTS follower_objects(space TEXT NOT NULL,task TEXT NOT NULL,kind TEXT NOT NULL,id TEXT NOT NULL,PRIMARY KEY(space,task,kind,id));
        CREATE TABLE IF NOT EXISTS inbox_migrations(version INTEGER PRIMARY KEY);
        CREATE INDEX IF NOT EXISTS records_recipient ON records(space,json_extract(fields,'$.recipient'),updated_at DESC) WHERE kind='notification';").map_err(storage)?;
    if db
        .execute("INSERT OR IGNORE INTO inbox_migrations VALUES(1)", [])
        .map_err(storage)?
        == 1
    {
        db.execute_batch("INSERT OR IGNORE INTO followers
            SELECT space,id,json_extract(fields,'$.creator') FROM records WHERE kind='task' AND json_type(fields,'$.creator')='text'
            UNION SELECT space,id,json_extract(fields,'$.owner') FROM records WHERE kind='task' AND json_type(fields,'$.owner')='text'
            UNION SELECT r.space,r.id,p.value FROM records r, json_each(r.fields,'$.assignees') p WHERE r.kind='task' AND p.type='text'
            UNION SELECT r.space,r.id,json_extract(r.fields,'$.assignee') FROM records r WHERE r.kind='task' AND COALESCE(json_extract(r.fields,'$.assignee_kind'),'member')='member' AND json_type(r.fields,'$.assignee')='text'
            UNION SELECT r.space,r.id,p.value FROM records r,json_each(r.fields,'$.mentions') p WHERE r.kind='task' AND p.type='text'
            UNION SELECT space,json_extract(fields,'$.task'),json_extract(fields,'$.creator') FROM records WHERE kind='comment' AND json_type(fields,'$.task')='text' AND json_type(fields,'$.creator')='text' AND json_extract(fields,'$.authored_by_agent') IS NULL;").map_err(storage)?;
    }
    Ok(())
}
pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    match op {
        "inbox.settings" | "inbox.configure" => settings::execute(c, q, op == "inbox.configure"),
        "inbox.feed" => listing::feed(c, q),
        "inbox.query" | "inbox.bulk" => listing::query(c, q, op == "inbox.bulk"),
        "inbox.open" | "inbox.locate" => listing::open(c, q),
        "followers.list" | "followers.change" => followers::execute(c, q, op == "followers.change"),
        _ => Err(invalid("unknown inbox operation")),
    }
}
pub(crate) fn after(c: &Context<'_>, old: Option<&Record>, row: &Record) -> Result<()> {
    let task = match row.kind.as_str() {
        "task" => row.clone(),
        "comment" if old.is_none() => c.get("task", required(&row.fields, "task")?)?,
        _ => return Ok(()),
    };
    followers::automatic(c, &task, row, old)?;
    let mut recipients = followers::members(c, &task)?;
    let current = crate::task_commands::assignees::people(&task.fields).collect::<BTreeSet<_>>();
    let previous = if row.kind == "task" {
        old.map(|r| crate::task_commands::assignees::people(&r.fields).collect())
            .unwrap_or_default()
    } else {
        current.clone()
    };
    recipients.extend(current.union(&previous).map(|id| (*id).to_owned()));
    for actor in recipients {
        if actor == c.actor && !row.fields["authored_by_agent"].is_string() {
            continue;
        }
        let mine = current.contains(actor.as_str());
        let before = previous.contains(actor.as_str());
        if row.kind == "comment" {
            let (event, important) = if mentions(row, &actor) {
                ("mention", true)
            } else if row.fields["authored_by_agent"].is_string() {
                ("agent_comment", mine)
            } else if row.fields["parent"].is_string() {
                ("reply", false)
            } else {
                ("comment", false)
            };
            task_notice(
                c,
                &task,
                &actor,
                event,
                Some(("comment", &row.id)),
                important,
            )?;
            continue;
        }
        if let Some(prior) = old {
            for (field, assigned, followed) in [
                ("start_date", "assigned_start", "followed_start"),
                ("due_date", "assigned_due", "followed_due"),
                ("priority", "assigned_priority", "followed_priority"),
            ] {
                if prior.fields[field] != task.fields[field] {
                    task_notice(
                        c,
                        &task,
                        &actor,
                        if mine { assigned } else { followed },
                        None,
                        false,
                    )?;
                }
            }
            if prior.title != task.title
                || prior.fields["description"] != task.fields["description"]
            {
                task_notice(c, &task, &actor, "task_updated", None, false)?;
            }
            if prior.fields["status"] != task.fields["status"] {
                let event = match (mine, c.status_category(&task)? == "done") {
                    (true, true) => "assigned_done",
                    (true, false) => "assigned_status",
                    (false, true) => "followed_done",
                    (false, false) => "followed_status",
                };
                task_notice(c, &task, &actor, event, None, false)?;
            }
        }
        if current != previous {
            let event = match (before, mine) {
                (false, true) => "assigned",
                (true, false) => "unassigned",
                (true, true) => "assignees_changed",
                (false, false) => "followed_assignees",
            };
            task_notice(c, &task, &actor, event, None, event == "assigned")?;
        }
        if mentions(row, &actor) && !old.is_some_and(|r| mentions(r, &actor)) {
            task_notice(c, &task, &actor, "description_mention", None, true)?;
        }
    }
    Ok(())
}
fn mentions(row: &Record, actor: &str) -> bool {
    row.fields["mentions"]
        .as_array()
        .is_some_and(|people| people.iter().any(|person| person == actor))
}
pub(crate) fn task_notice(
    c: &Context<'_>,
    task: &Record,
    recipient: &str,
    event: &str,
    source: Option<(&str, &str)>,
    important: bool,
) -> Result<()> {
    let target = Context {
        actor: recipient,
        ..*c
    };
    if target.role().is_err() || !target.visible(task)? {
        return Ok(());
    }
    if let Some((kind, id)) = source
        && target.readable(kind, id).is_err()
    {
        return Ok(());
    }
    let cfg = settings::Settings::load(c, recipient)?;
    if !cfg.enabled("inbox", event) {
        return Ok(());
    }
    let previous:Option<String>=c.db.query_row("SELECT id FROM records WHERE space=?1 AND kind='notification' AND json_extract(fields,'$.task')=?2 AND json_extract(fields,'$.recipient')=?3 AND json_extract(fields,'$.automation') IS NULL AND json_extract(fields,'$.chat') IS NULL ORDER BY created_at,id LIMIT 1",params![c.space,task.id,recipient],|r|r.get(0)).optional().map_err(storage)?;
    let old = previous.map(|id| c.get("notification", &id)).transpose()?;
    let active = old.as_ref().filter(|r| r.fields["state"] != "cleared");
    let sticky = active.is_some_and(|r| r.fields["importance"] == "important");
    let mut fields = json!({"recipient":recipient,"task":task.id,"event":event,"latest":source.map(|(_,id)|id).unwrap_or(&task.id),"actor":c.actor,"read":false,"state":"inbox","importance":if important||sticky{"important"}else{"other"},"count":active.map_or(0,|r|r.fields["count"].as_u64().unwrap_or(1))+1});
    if let Some((kind, id)) = source {
        fields["source_kind"] = json!(kind);
        fields["source_id"] = json!(id);
    }
    if let Some(t) = active
        .filter(|r| r.fields["state"] == "snoozed" && !important)
        .and_then(|r| r.fields["until"].as_i64())
        .filter(|t| *t > now())
    {
        fields["state"] = json!("snoozed");
        fields["until"] = json!(t);
    }
    let notice = match old {
        Some(r) => c.update(&r, &task.title, fields)?,
        None => c.create("notification", None, &task.title, fields)?,
    };
    signal(c, &notice, event, source, cfg.enabled("desktop", event))
}
fn signal(
    c: &Context<'_>,
    notice: &Record,
    event: &str,
    source: Option<(&str, &str)>,
    desktop: bool,
) -> Result<()> {
    c.db.execute("INSERT INTO inbox_signals(org,space,actor,event,notice,source_kind,source_id,desktop) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![c.org_id()?,c.space,required(&notice.fields,"recipient")?,event,notice.id,source.map(|s|s.0),source.map(|s|s.1),desktop]).map_err(storage)?;
    Ok(())
}
pub(crate) fn channel_enabled(c: &Context<'_>, recipient: &str, event: &str) -> Result<bool> {
    Ok(settings::Settings::load(c, recipient)?.enabled("inbox", event))
}
pub(crate) fn task_run(c: &Context<'_>, run: &Record) -> Result<()> {
    let Some(id) = run.fields["task_id"].as_str() else {
        return Ok(());
    };
    let Some(task) = c.find("task", id)? else {
        return Ok(());
    };
    let event = match run.fields["status"].as_str() {
        Some("done") => "run_done",
        Some("failed") => "run_failed",
        Some("waiting") if run.fields["chat_id"].is_null() => "run_handoff",
        _ => return Ok(()),
    };
    let mut recipients = followers::members(c, &task)?;
    recipients.extend(crate::task_commands::assignees::people(&task.fields).map(str::to_owned));
    if let Some(actor) = run.fields["requested_by"].as_str() {
        recipients.insert(actor.into());
    }
    for actor in recipients {
        let mine = crate::task_commands::assignees::people(&task.fields).any(|id| id == actor)
            || task.fields["owner"] == actor;
        task_notice(
            c,
            &task,
            &actor,
            event,
            Some(("run", &run.id)),
            mine && event != "run_done",
        )?;
    }
    Ok(())
}
pub(crate) fn notify_record(
    c: &Context<'_>,
    notice: &Record,
    event: &str,
    source: Option<(&str, &str)>,
) -> Result<()> {
    let cfg = settings::Settings::load(c, required(&notice.fields, "recipient")?)?;
    signal(c, notice, event, source, cfg.enabled("desktop", event))
}
pub(crate) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    rules.extend([
        json!({"if":{"properties":{"op":{"const":"comment.react"}}},"then":{"required":["id","emoji","enabled"],"properties":{"emoji":{"type":"string","minLength":1,"maxLength":32},"enabled":{"type":"boolean"}}}}),
        json!({"if":{"properties":{"op":{"const":"comment.reactions"}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"enum":["followers.list","followers.change","inbox.open","inbox.locate"]}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"const":"followers.change"}}},"then":{"required":["target","enabled"],"properties":{"target_kind":{"enum":["member","agent","invitation"]},"enabled":{"type":"boolean"}}}}),
        json!({"if":{"properties":{"op":{"const":"inbox.configure"}}},"then":{"anyOf":[{"required":["landing"]},{"required":["channel","enabled"]},{"required":["channel","reset"]}],"properties":{"channel":{"enum":settings::CHANNELS},"landing":{"enum":["important","all","other"]},"reset":{"const":true},"enabled":{"type":"boolean"},"expected_rev":{"type":"integer","minimum":0}}}}),
        json!({"if":{"properties":{"op":{"enum":["inbox.query","inbox.bulk"]}}},"then":{"properties":{"section":{"enum":["all","important","other","snoozed","cleared"]},"include_spaces":{"type":"array","items":{"type":"string"}},"exclude_spaces":{"type":"array","items":{"type":"string"}},"unread":{"type":"boolean"},"statuses":{"type":"array","items":{"type":"string"}}}}}),
        json!({"if":{"properties":{"op":{"const":"inbox.bulk"}}},"then":{"required":["action"],"properties":{"action":{"enum":["read","clear"]}}}})
    ]);
}
