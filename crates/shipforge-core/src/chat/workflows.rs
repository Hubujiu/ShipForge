use super::*;
use crate::{as_value, domain, worker};

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS chat_inherited(space TEXT NOT NULL,message TEXT NOT NULL,source TEXT NOT NULL,PRIMARY KEY(space,message,source));
        CREATE TABLE IF NOT EXISTS chat_conversion(space TEXT NOT NULL,chat TEXT NOT NULL,run TEXT NOT NULL,PRIMARY KEY(space,run));
        CREATE TABLE IF NOT EXISTS chat_run_input(space TEXT NOT NULL,run TEXT NOT NULL,message TEXT NOT NULL,PRIMARY KEY(space,run,message));").map_err(storage)
}
fn origins(c: &Context<'_>, id: &str) -> Result<Vec<String>> {
    let mut stmt =
        c.db.prepare_cached("SELECT source FROM chat_inherited WHERE space=? AND message=?")
            .map_err(storage)?;
    stmt.query_map(params![c.space, id], |r| r.get(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
}
fn creation_seq(c: &Context<'_>, kind: &str, id: &str) -> Result<Option<i64>> {
    c.db.query_row("SELECT seq FROM event_log WHERE space=? AND kind=? AND id=? AND action='create' ORDER BY seq DESC LIMIT 1",params![c.space,kind,id],|r|r.get(0)).optional().map_err(storage)
}
fn dependencies(c: &Context<'_>, row: &Record) -> Result<Vec<String>> {
    let mut messages = if row.kind == "message" {
        origins(c, &row.id)?
    } else {
        Vec::new()
    };
    let run = match row.kind.as_str() {
        "run" if row.fields["chat_id"].is_string() => Some(row.id.as_str()),
        "message" => row.fields["run"].as_str(),
        _ => None,
    };
    if let Some(run) = run {
        let mut stmt =
            c.db.prepare_cached("SELECT message FROM chat_run_input WHERE space=? AND run=?")
                .map_err(storage)?;
        let inputs = stmt
            .query_map(params![c.space, run], |r| r.get::<_, String>(0))
            .map_err(storage)?;
        for message in inputs {
            messages.push(message.map_err(storage)?);
        }
    }
    Ok(messages)
}
pub(crate) fn manager_transcript(c: &Context<'_>, room: &Record) -> Result<Vec<Value>> {
    fn permitted(c: &Context<'_>, room: &Record, message: &Record) -> Result<bool> {
        let mut queue = vec![message.clone()];
        let mut seen = std::collections::HashSet::new();
        while let Some(row) = queue.pop() {
            if !seen.insert(row.id.clone()) {
                continue;
            }
            if row.fields["chat"] != room.id {
                if !c.visible(&row)? {
                    return Ok(false);
                }
                continue;
            }
            if !super::retained(c, &row, &room.id)? {
                return Ok(false);
            }
            if row.fields["visibility"] == "private"
                && row.fields["author_kind"] != "agent"
                && row.fields["creator"] != c.actor
                && row.fields["owner"] != c.actor
            {
                return Ok(false);
            }
            if let Some(id) = row.fields["run"].as_str()
                && let Some(run) = c.find("run", id)?
                && !crate::agents::run_source_visible(c, &run)?
            {
                return Ok(false);
            }
            for id in dependencies(c, &row)? {
                let Some(source) = c.find("message", &id)? else {
                    return Ok(false);
                };
                queue.push(source);
            }
        }
        Ok(true)
    }
    let mut stmt=c.db.prepare_cached("SELECT r.id FROM records r JOIN event_log e ON r.space=e.space AND r.kind=e.kind AND r.id=e.id AND e.action='create' WHERE r.space=? AND r.kind='message' AND json_extract(r.fields,'$.chat')=? ORDER BY e.seq").map_err(storage)?;
    let mut messages = Vec::new();
    for id in stmt
        .query_map(params![c.space, room.id], |r| r.get::<_, String>(0))
        .map_err(storage)?
    {
        let row = c.get("message", &id.map_err(storage)?)?;
        if permitted(c, room, &row)? {
            messages.push(json!({"author":row.fields["author"].as_str().or(row.fields["creator"].as_str()),"body":row.fields["body"],"at":row.created_at}));
        }
    }
    Ok(messages)
}
fn verify_sources(c: &Context<'_>, row: &Record, sharing: bool) -> Result<bool> {
    let mut pending = dependencies(c, row)?;
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(source) = c.find("message", &id)? else {
            return Ok(false);
        };
        if !c.intrinsic_visibility(&source)? {
            return Ok(false);
        }
        if sharing {
            let room = c.readable("chat", required(&source.fields, "chat")?)?;
            super::require_owner(c, &room)?;
        }
        pending.extend(dependencies(c, &source)?);
    }
    Ok(true)
}
pub(crate) fn check_inherited(c: &Context<'_>, row: &Record) -> Result<bool> {
    verify_sources(c, row, false)
}
pub(crate) fn may_share_inherited(c: &Context<'_>, id: &str) -> Result<()> {
    if verify_sources(c, &c.get("message", id)?, true)? {
        Ok(())
    } else {
        Err(forbidden("conversation source content is unavailable"))
    }
}
pub(super) fn ordered(c: &Context<'_>, room: &Record) -> Result<Vec<Record>> {
    let mut stmt=c.db.prepare_cached("SELECT r.id FROM records r JOIN event_log e ON (r.space=e.space AND r.kind=e.kind AND r.id=e.id AND e.action='create') WHERE r.space=? AND r.kind='message' AND json_extract(r.fields,'$.chat')=? ORDER BY e.seq").map_err(storage)?;
    let ids = stmt
        .query_map(params![c.space, room.id], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let mut result = Vec::new();
    for id in ids {
        let message = c.get("message", &id)?;
        if c.visible(&message)? {
            result.push(message);
        }
    }
    Ok(result)
}
pub(crate) fn handler<'a>(c: &Context<'_>, room: &'a Record) -> Result<(&'a str, &'a str)> {
    if super::group(room) {
        return Err(invalid("select an Agent conversation"));
    }
    let kind = room.fields["handler_kind"].as_str().unwrap_or("agent");
    if !matches!(kind, "agent" | "crew" | "assistant") {
        return Err(invalid("an Agent or crew is required"));
    }
    let id = room.fields["handler_id"]
        .as_str()
        .or(room.fields["agent"].as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("an Agent or crew conversation is required"))?;
    crate::agents::usable(c, kind, id, None)?;
    Ok((kind, id))
}
fn bound(room: &Record) -> Option<&str> {
    room.fields["task"]
        .as_str()
        .or(room.fields["task_id"].as_str())
}
pub(crate) fn check_binding(c: &Context<'_>, old: Option<&Record>, room: &Record) -> Result<()> {
    if room.fields.get("directory_fork").is_some()
        && old.is_none_or(|row| row.fields["directory_fork"] != room.fields["directory_fork"])
    {
        return Err(invalid(
            "conversation workspace selections are set when forking",
        ));
    }
    if ["task", "task_id"].iter().any(|key| {
        room.fields
            .get(*key)
            .is_some_and(|v| !v.is_null() && !v.is_string())
    }) {
        return Err(invalid("conversation task references must be IDs"));
    }
    if let (Some(a), Some(b)) = (
        room.fields["task"].as_str(),
        room.fields["task_id"].as_str(),
    ) && a != b
    {
        return Err(invalid("conversation task references disagree"));
    }
    if old.is_some_and(|r| bound(r) != bound(room)) {
        return Err(invalid("use chat.bind_task to bind a conversation"));
    }
    if let Some(id) = bound(room) {
        c.readable("task", id)?;
    }
    Ok(())
}
pub(super) fn fork(c: &Context<'_>, room: &Record, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let (source_kind, source) = handler(c, room)?;
    let agent = q["handler_id"].as_str().unwrap_or(source);
    let kind = q["handler_kind"].as_str().unwrap_or(source_kind);
    crate::agents::usable(c, kind, agent, None)?;
    let anchor = c.readable("message", required(q, "message")?)?;
    if anchor.fields["chat"] != room.id {
        return Err(invalid(
            "the fork point belongs to a different conversation",
        ));
    }
    let default_title = format!("{} · {}", room.title, "分支");
    let title = q
        .get("title")
        .map(|v| v.as_str().unwrap_or("").trim())
        .unwrap_or(&default_title);
    if title.is_empty() || title.chars().count() > 500 {
        return Err(invalid("fork title must have 1–500 characters"));
    }
    let mut fields = json!({"chat_type":"conversation","visibility":"private","creator":c.actor,"owner":c.actor,"handler_kind":kind,"handler_id":agent});
    let workspace = super::fork_directory::for_fork(c, room, q)?;
    if !workspace.is_null() {
        fields["directory_fork"] = workspace;
    }
    if kind == "agent" {
        fields["agent"] = json!(agent);
    }
    if let Some(task) = bound(room) {
        fields["task"] = json!(task);
    }
    for key in ["runtime", "model"] {
        if source == agent && source_kind == kind && room.fields[key].is_string() {
            fields[key] = room.fields[key].clone();
        }
    }
    let branch = c.create("chat", None, title, fields)?;
    for message in ordered(c, room)? {
        let mut fields = json!({"chat":branch.id,"creator":c.actor,"body":message.fields["body"],"author_kind":message.fields["author_kind"].as_str().unwrap_or("human"),"author":message.fields["author"].as_str().or(message.fields["creator"].as_str()),"inherited":true});
        if let Some(files) = message.fields.get("attachments") {
            fields["attachments"] = files.clone();
        }
        let copy = c.create("message", None, &message.title, fields)?;
        c.db.execute(
            "INSERT INTO chat_inherited(space,message,source) VALUES(?,?,?)",
            params![c.space, copy.id, message.id],
        )
        .map_err(storage)?;
        c.db.execute("INSERT OR IGNORE INTO chat_inherited(space,message,source) SELECT space,?2,source FROM chat_inherited WHERE space=?1 AND message=?3",params![c.space,copy.id,message.id]).map_err(storage)?;
        if message.id == anchor.id {
            return Ok(as_value(branch));
        }
    }
    Err(forbidden("fork point is no longer available"))
}
pub(crate) fn content_context(c: &Context<'_>, run: &Record) -> Result<Value> {
    let Some(room_id) = run.fields["chat_id"].as_str() else {
        return Ok(Value::Null);
    };
    let requester = Context {
        actor: required(&run.fields, "requested_by")?,
        ..*c
    };
    requester.role()?;
    let room = requester.readable("chat", room_id)?;
    let anchor = run.fields["chat_message"].as_str();
    if let Some(id) = anchor {
        requester.readable("message", id)?;
    }
    let cutoff = anchor
        .map(|id| creation_seq(c, "message", id))
        .transpose()?
        .flatten();
    let mut messages = Vec::new();
    let mut private = room.fields["visibility"] == "private";
    let mut current = None;
    for row in ordered(&requester, &room)? {
        if current.is_some() {
            let Some(prior) = row.fields["run"].as_str() else {
                continue;
            };
            let Some(cutoff) = cutoff else {
                continue;
            };
            if creation_seq(c, "run", prior)?.is_none_or(|seq| seq >= cutoff) {
                continue;
            }
        }
        let source = row.fields["author"]
            .as_str()
            .or(row.fields["creator"].as_str());
        private |= row.fields["visibility"] == "private" || row.fields["inherited"] == true;
        let mut value = json!({"id":row.id,"author":source,"author_kind":row.fields["author_kind"].as_str().unwrap_or("human"),"body":row.fields["body"].as_str().unwrap_or(&row.title),"attachments":row.fields["attachments"]});
        if row.fields["external_author"].is_string()
            && room.fields["external_provider"] == row.fields["external_provider"]
        {
            value["author"] = row.fields["external_author"].clone();
            value["author_kind"] = json!("external");
            value["external_provider"] = row.fields["external_provider"].clone();
            value["external_scene"] = row.fields["external_scene"].clone();
        }
        if Some(row.id.as_str()) == anchor {
            crate::bots::run_files(&requester, run, &row, &mut value)?;
            current = Some(value);
        } else {
            messages.push(value);
        }
    }
    messages.extend(current);
    c.db.execute(
        "DELETE FROM chat_run_input WHERE space=? AND run=?",
        params![c.space, run.id],
    )
    .map_err(storage)?;
    for msg in &messages {
        c.db.execute(
            "INSERT INTO chat_run_input(space,run,message) VALUES(?,?,?)",
            params![c.space, run.id, required(msg, "id")?],
        )
        .map_err(storage)?;
    }
    Ok(json!({"id":room.id,"title":room.title,"messages":messages,"private":private}))
}
pub(crate) fn claim_still_visible(c: &Context<'_>, run: &Record) -> Result<bool> {
    if !super::fork_directory::can_continue(c, run)? {
        return Ok(false);
    }
    let Some(id) = run.fields["chat_id"].as_str() else {
        return Ok(true);
    };
    let requester = Context {
        actor: required(&run.fields, "requested_by")?,
        ..*c
    };
    if requester.role().is_err() || requester.readable("chat", id).is_err() {
        return Ok(false);
    }
    verify_sources(&requester, run, false)
}
fn attach(c: &Context<'_>, room: &Record, task: &Record) -> Result<Record> {
    match bound(room) {
        Some(id) if id == task.id => return Ok(room.clone()),
        Some(_) => return Err(conflict("this conversation is already bound to a task")),
        None => {}
    }
    let mut fields = room.fields.clone();
    fields["task"] = json!(task.id);
    c.update(room, &room.title, fields)
}
fn pending(c: &Context<'_>, room: &Record) -> Result<Option<String>> {
    c.db.query_row("SELECT x.run FROM chat_conversion x JOIN records r ON r.space=x.space AND r.kind='run' AND r.id=x.run WHERE x.space=? AND x.chat=? AND json_extract(r.fields,'$.status') IN ('queued','running','waiting','waiting_children','cancelling') LIMIT 1",params![c.space,room.id],|r|r.get(0)).optional().map_err(storage)
}
pub(super) fn bind(c: &Context<'_>, room: &Record, q: &Value) -> Result<Value> {
    if super::assistant::is_room(room) {
        return Err(invalid("personal assistants use independent conversations"));
    }
    super::require_owner(c, room)?;
    handler(c, room)?;
    domain::check_rev(q, room)?;
    let target = c.readable("task", required(q, "task")?)?;
    c.authorize_write("task", Some(&target))?;
    if pending(c, room)?.is_some() {
        return Err(conflict(
            "an Agent is already creating a task for this conversation",
        ));
    }
    Ok(as_value(attach(c, room, &target)?))
}
fn queue(
    c: &Context<'_>,
    room: &Record,
    body: &str,
    convert: bool,
    origin: &str,
) -> Result<(Record, Record)> {
    c.require_edit()?;
    let (kind, id) = handler(c, room)?;
    if body.trim().is_empty() || body.len() > 262_144 {
        return Err(invalid(
            "supply a nonempty conversation message under 256 KiB",
        ));
    }
    let header = body.chars().take(55).collect::<String>();
    let message=c.create("message",None,&header,json!({"chat":room.id,"creator":c.actor,"author":c.actor,"author_kind":"human","body":body}))?;
    super::notify(c, &message)?;
    let private = room.fields["visibility"] == "private"
        || ordered(c, room)?
            .iter()
            .any(|m| m.fields["visibility"] == "private" || m.fields["inherited"] == true);
    let mut fields = json!({"instruction":body,"origin_kind":origin,"chat_id":room.id,"chat_message":message.id,"chat_order":creation_seq(c,"message",&message.id)?,"task_id":bound(room),"visibility":if private{json!("private")}else{room.fields["visibility"].clone()},"chat_conversion":convert});
    if let Some(task) = bound(room) {
        fields["task"] = worker::task_context(c, &c.readable("task", task)?)?;
    }
    let run = worker::enqueue(
        c,
        &format!("conversation:{}", message.id),
        id,
        kind,
        &room.title,
        fields,
    )?;
    Ok((message, run))
}
pub(super) fn send(c: &Context<'_>, room: &Record, q: &Value, origin: &str) -> Result<Value> {
    let (message, run) = queue(c, room, required(q, "body")?, false, origin)?;
    Ok(json!({"id":room.id,"message":message.id,"run":run.id}))
}
pub(super) fn convert(c: &Context<'_>, room: &Record, q: &Value) -> Result<Value> {
    super::require_owner(c, room)?;
    domain::check_rev(q, room)?;
    if bound(room).is_some() {
        return Err(conflict("this conversation already has a task"));
    }
    if let Some(run) = pending(c, room)? {
        return Ok(json!({"id":room.id,"run":run}));
    }
    let instruction = "根据此对话的上下文拟定一个可跟踪的任务。请在运行结果的 task_draft 对象中返回 title 和 description 两个字符串，或在 result 正文中返回同样结构的 JSON；系统会创建任务并绑定本会话。";
    let (message, run) = queue(c, room, instruction, true, "chat")?;
    c.db.execute(
        "INSERT INTO chat_conversion(space,chat,run) VALUES(?,?,?)",
        params![c.space, room.id, run.id],
    )
    .map_err(storage)?;
    Ok(json!({"id":room.id,"run":run.id,"message":message.id}))
}
pub(crate) fn apply_conversion(
    c: &Context<'_>,
    run: &Record,
    report: &Value,
    fields: &mut Value,
) -> Result<()> {
    if run.fields["chat_conversion"] != true || fields["status"] != "done" {
        return Ok(());
    }
    c.db.execute_batch("SAVEPOINT chat_task").map_err(storage)?;
    let result = (|| -> Result<Record> {
        let requester = Context {
            actor: required(&run.fields, "requested_by")?,
            ..*c
        };
        let room = requester.readable("chat", required(&run.fields, "chat_id")?)?;
        super::require_owner(&requester, &room)?;
        if bound(&room).is_some() {
            return Err(conflict("conversation is already bound"));
        }
        let draft = if let Some(value) = report.get("task_draft").filter(|v| v.is_object()) {
            value.clone()
        } else {
            let raw = report["result"].as_str().unwrap_or("").trim();
            let raw = raw
                .strip_prefix("```json")
                .and_then(|r| r.strip_suffix("```"))
                .unwrap_or(raw);
            serde_json::from_str(raw.trim())
                .map_err(|_| invalid("Agent must supply a task title and description as JSON"))?
        };
        let title = required(&draft, "title")?;
        let description = required(&draft, "description")?;
        let result = domain::write(
            &requester,
            &json!({"op":"create","kind":"task","title":title,"fields":{"description":description,"visibility":run.fields["visibility"]}}),
        )?;
        let task = requester.readable("task", required(&result, "id")?)?;
        attach(&requester, &room, &task)?;
        Ok(task)
    })();
    match result {
        Ok(task) => {
            fields["created_task"] = json!(task.id);
            fields["result"] = json!(task.title);
        }
        Err(error) => {
            c.db.execute_batch("ROLLBACK TO chat_task")
                .map_err(storage)?;
            fields["status"] = json!("failed");
            fields["result"] = json!(error);
        }
    }
    c.db.execute_batch("RELEASE chat_task").map_err(storage)
}
pub(super) fn schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"enum":["chat.organize","chat.history","chat.fork","chat.bind_task","chat.convert","chat.send"]}}},"then":{"required":["id"]}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"chat.organize"}}},"then":{"required":["action"],"properties":{"action":{"enum":["pin","unpin","hide","undo"]}},"if":{"properties":{"action":{"const":"undo"}}},"then":{"required":["undo_token"]}}}));
    for (op, key) in [
        ("chat.fork", "message"),
        ("chat.bind_task", "task"),
        ("chat.send", "body"),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":[key],"properties":{(key):{"type":"string","minLength":1}}}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"const":"run.report"}}},"then":{"properties":{"task_draft":{"type":["object","null"],"properties":{"title":{"type":"string"},"description":{"type":"string"},"action":{"enum":["create","update","show","comment"]},"status":{"type":"string"},"body":{"type":"string"},"task":{"type":"string"}},"anyOf":[{"required":["title","description"]},{"required":["action"]}]}}}}));
}
