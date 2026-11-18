//! Slack single-person assignment. A signed click resolves through a previously delivered card.
use crate::{Context, Record, Result, conflict, forbidden, invalid, required, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS sf_slack_cards(
        delivery TEXT PRIMARY KEY, assignees TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS sf_slack_clicks(
        delivery TEXT NOT NULL, actor TEXT NOT NULL, stamp TEXT NOT NULL, chosen TEXT NOT NULL,
        PRIMARY KEY(delivery,actor,stamp));",
    )
    .map_err(storage)
}
const PREFIX: &str = "shipforge-assignee-";
const ACTION: &str = "shipforge.assign";
pub(super) fn initial(
    c: &Context<'_>,
    connector: &Record,
    route: &str,
    task: &Record,
) -> Result<()> {
    if connector.fields["slack_assignment"] != true {
        return Ok(());
    }
    let address: Value = serde_json::from_str(route).map_err(storage)?;
    if !super::outbox::scoped(connector, &address) {
        return Ok(());
    }
    let delivery = Uuid::new_v4().to_string();
    let text = format!("{} · {}", required(&task.fields, "number")?, task.title);
    c.db.execute(
        "INSERT INTO sf_im_outbox(id,space,connector,route,task,kind,source,rev,body)
        VALUES(?1,?2,?3,?4,?5,'card',?1,?6,?7)",
        params![
            delivery,
            c.space,
            connector.id,
            route,
            task.id,
            task.rev,
            text
        ],
    )
    .map_err(storage)?;
    Ok(())
}
pub(super) fn blocks(
    c: &Context<'_>,
    connector: &Record,
    task: &Record,
    delivery: &str,
    text: &str,
) -> Result<Value> {
    let people = &task.fields["assignees"];
    c.db.execute("INSERT INTO sf_slack_cards(delivery,assignees) VALUES(?,?) ON CONFLICT(delivery) DO UPDATE SET assignees=excluded.assignees",
        params![delivery,people.to_string()]).map_err(storage)?;
    let mut picker = json!({"type":"users_select","action_id":ACTION,"placeholder":{"type":"plain_text","text":"选择一人，替换任务处理人"}});
    if let Some(first) = people
        .get(0)
        .filter(|_| people.as_array().is_some_and(|list| list.len() == 1))
        && let Some((user, _)) = connector.fields["assignee_map"]
            .as_object()
            .into_iter()
            .flatten()
            .find(|(_, actor)| *actor == first)
    {
        picker["initial_user"] = json!(user);
    }
    Ok(
        json!([{"type":"section","text":{"type":"plain_text","text":text.chars().take(2990).collect::<String>()}},
        {"type":"actions","block_id":format!("{PREFIX}{delivery}"),"elements":[picker]}]),
    )
}
pub(super) fn forget(c: &Context<'_>, field: &str, id: &str) -> Result<()> {
    let column = match field {
        "task" => "task",
        "connector" => "connector",
        _ => return Ok(()),
    };
    c.db.execute(&format!("DELETE FROM sf_slack_cards WHERE delivery IN(SELECT id FROM sf_im_outbox WHERE space=? AND {column}=?)"),params![c.space,id]).map_err(storage)?;
    Ok(())
}
pub(super) fn unbind(c: &Context<'_>, connector: &str, route: &str) -> Result<()> {
    c.db.execute("DELETE FROM sf_slack_cards WHERE delivery IN(SELECT id FROM sf_im_outbox WHERE space=? AND connector=? AND route=?)",params![c.space,connector,route]).map_err(storage)?;
    Ok(())
}
pub(crate) fn click(c: &Context<'_>, connector: &Record, p: &Value) -> Result<Value> {
    let f = &connector.fields;
    let addr = &p["container"];
    let msg = &p["message"];
    if f["slack_assignment"] != true
        || p["type"] != "block_actions"
        || p["team"]["id"] != f["workspace_id"]
        || p["api_app_id"] != f["app_id"]
        || p["channel"]["id"] != f["channel_id"]
        || addr["type"] != "message"
        || addr["channel_id"] != f["channel_id"]
        || msg["bot_id"] != f["bot_id"]
        || msg.get("app_id").is_some_and(|id| id != &f["app_id"])
        || addr["message_ts"] != msg["ts"]
        || p["user"]
            .get("team_id")
            .is_some_and(|id| id != &f["workspace_id"])
    {
        return Err(forbidden("Slack card does not match this active connector"));
    }
    let actions = p["actions"]
        .as_array()
        .filter(|actions| actions.len() == 1)
        .ok_or_else(|| invalid("expected one Slack card action"))?;
    let action = &actions[0];
    if action["action_id"] != ACTION || action["type"] != "users_select" {
        return Err(invalid("unsupported Slack card action"));
    }
    let stamp = required(action, "action_ts")?;
    let remote = required(msg, "ts")?;
    if !super::origin::slack_ts(stamp) || !super::origin::slack_ts(remote) {
        return Err(invalid("invalid Slack action timestamp"));
    }
    let delivery = required(action, "block_id")?
        .strip_prefix(PREFIX)
        .filter(|v| Uuid::parse_str(v).is_ok())
        .ok_or_else(|| invalid("unrecognized Slack assignment card"))?;
    let record:Option<(String,String,String)>=c.db.query_row(
        "SELECT o.task,o.route,b.assignees FROM sf_im_outbox o JOIN sf_slack_cards b ON b.delivery=o.id
        JOIN sf_im_threads t ON t.space=o.space AND t.connector=o.connector AND t.route=o.route AND t.task=o.task AND t.active=1
        WHERE o.space=? AND o.connector=? AND o.id=? AND o.state='sent' AND o.remote=?",
        params![c.space,connector.id,delivery,remote],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
    let (id, route, expected) =
        record.ok_or_else(|| forbidden("this Slack card is unavailable"))?;
    let route: Value = serde_json::from_str(&route).map_err(storage)?;
    if !super::outbox::scoped(connector, &route) || route["thread"] != msg["thread_ts"] {
        return Err(forbidden("Slack conversation is unavailable"));
    }
    let external = required(&p["user"], "id")?;
    let chosen = required(action, "selected_user")?;
    let actor = f["assignee_map"][external]
        .as_str()
        .ok_or_else(|| forbidden("no workspace member is mapped to the clicking Slack account"))?;
    let dest = f["assignee_map"][chosen]
        .as_str()
        .ok_or_else(|| invalid("choose a mapped workspace member"))?;
    let caller = Context { actor, ..*c };
    caller.require_edit()?;
    caller.readable("member", actor)?;
    caller.readable("member", dest)?;
    let task = super::shared_task(&caller, &id)?;
    if let Some(old) =
        c.db.query_row(
            "SELECT chosen FROM sf_slack_clicks WHERE delivery=? AND actor=? AND stamp=?",
            params![delivery, external, stamp],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(storage)?
    {
        if old != chosen {
            return Err(conflict(
                "Slack action was already used with another selection",
            ));
        }
        return Ok(json!({"ok":true}));
    }
    if serde_json::from_str::<Value>(&expected).map_err(storage)? != task.fields["assignees"] {
        return Err(conflict(
            "task assignees changed; use the latest assignment card",
        ));
    }
    let people = json!([dest]);
    if task.fields["assignees"] != people {
        crate::task_commands::assignees::assign(
            &caller,
            &json!({"id":id,"assignees":people,"expected_rev":task.rev}),
        )?;
    }
    c.db.execute(
        "INSERT INTO sf_slack_clicks(delivery,actor,stamp,chosen) VALUES(?,?,?,?)",
        params![delivery, external, stamp, chosen],
    )
    .map_err(storage)?;
    Ok(json!({"ok":true}))
}
