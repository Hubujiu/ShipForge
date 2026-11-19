use super::*;
use crate::im::Origin;

pub(crate) struct Binding {
    pub(super) assistant: String,
    epoch: String,
    pub(super) active: bool,
}
pub(crate) fn route(c: &Context<'_>, connector: &str) -> Result<Option<Binding>> {
    c.db.query_row(
        "SELECT assistant,epoch,active FROM sf_im_assistant_routes WHERE space=? AND connector=?",
        params![c.space, connector],
        |r| {
            Ok(Binding {
                assistant: r.get(0)?,
                epoch: r.get(1)?,
                active: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(storage)
}
fn current_binding(
    c: &Context<'_>,
    b: &Binding,
    connector: &Record,
    sender: &str,
) -> Result<Option<String>> {
    if !b.active || connector.fields["enabled"] != true || crate::spaces::ensure_active(c).is_err()
    {
        return Ok(None);
    }
    let profile = c.get(KIND, &b.assistant)?;
    if profile.fields["state"] != "connected"
        || profile.fields["epoch"] != b.epoch
        || profile.fields["identity"] != identity(&connector.fields)
    {
        return Ok(None);
    }
    let Some(operator) = profile.fields["operator"].as_str() else {
        return Ok(None);
    };
    if (Context {
        actor: operator,
        ..*c
    })
    .require_admin()
    .is_err()
    {
        return Ok(None);
    }
    let mapping = profile
        .fields
        .get("sender_map")
        .unwrap_or(&connector.fields["assignee_map"]);
    let Some(actor) = mapping[sender].as_str() else {
        return Ok(None);
    };
    let local = Context { actor, ..*c };
    Ok(
        (local.find("member", actor)?.is_some() && local.require_edit().is_ok())
            .then(|| actor.to_owned()),
    )
}
pub(crate) fn current(c: &Context<'_>, run: &Record) -> Result<bool> {
    let turn = &run.fields["external_request"];
    let Some(connector) = c.find("connector", required(turn, "connector")?)? else {
        return Ok(false);
    };
    let Some(b) = route(c, &connector.id)? else {
        return Ok(false);
    };
    let active = b.assistant == run.fields["handler_id"]
        && b.epoch == turn["epoch"]
        && current_binding(c, &b, &connector, required(turn, "sender")?)?.as_deref()
            == turn["actor"].as_str();
    if active && let Some(target) = turn["target"].as_str() {
        let caller = Context {
            actor: required(turn, "actor")?,
            ..*c
        };
        return Ok(caller
            .readable("task", target)
            .is_ok_and(|r| r.fields["visibility"] != "private"));
    }
    Ok(active)
}
pub(crate) fn deliverable(c: &Context<'_>, source: &str, task: &str, thread: &str) -> Result<bool> {
    let Some(run) = c.find("run", source)? else {
        return Ok(false);
    };
    let address: Value = serde_json::from_str(thread).map_err(storage)?;
    Ok(run.fields["handler_kind"] == KIND
        && run.fields["external_request"]["thread"] == address
        && run.fields["assistant_action"]["task"] == task
        && current(c, &run)?)
}
fn address(
    p: &str,
    payload: &Value,
    f: &Value,
    origin: Option<Origin>,
) -> Result<Option<(Value, String, String, bool)>> {
    if p == "dingtalk" {
        let parsed;
        let d = if payload.get("specVersion").is_some() {
            parsed = serde_json::from_str::<Value>(required(payload, "data")?).map_err(storage)?;
            &parsed
        } else {
            payload
        };
        if d["conversationType"] != "2"
            || f["corp_id"].as_str().is_none()
            || f["chat_id"].as_str().is_none()
        {
            return Ok(None);
        }
        let msg = required(d, "msgId")?.to_string();
        let sender = required(d, "senderId")?.to_owned();
        return Ok(Some((
            json!({"provider":p,"workspace":d["chatbotCorpId"],"channel":d["conversationId"],"thread":msg}),
            msg,
            sender,
            d["isInAtList"] == true,
        )));
    }
    let Some(o) = origin else { return Ok(None) };
    let Some(sender) = o.author.as_ref() else {
        return Ok(None);
    };
    let mentioned = match p {
        "slack" => {
            if f["workspace_id"] != o.workspace
                || f["channel_id"] != o.channel
                || f["app_id"] != payload["api_app_id"]
            {
                return Ok(None);
            }
            let e = &payload["event"];
            e["type"] == "app_mention"
                || f["bot_user_id"].as_str().is_some_and(|id| {
                    e["text"]
                        .as_str()
                        .is_some_and(|t| t.contains(&format!("<@{id}>")))
                })
        }
        "feishu" | "lark" => {
            if f["tenant_key"].as_str() != Some(o.workspace.as_str())
                || f["chat_id"].as_str() != Some(o.channel.as_str())
                || payload["event"]["message"]["chat_type"] != "group"
            {
                return Ok(None);
            }
            let bot = required(f, "bot_open_id")?;
            payload["event"]["message"]["mentions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|m| m["id"]["open_id"] == bot)
        }
        _ => return Ok(None),
    };
    Ok(Some((o.route(), o.message, sender.clone(), mentioned)))
}
fn intent(s: &str) -> Option<&'static str> {
    let text = s.trim().to_lowercase();
    let mut body = text.as_str();
    while body.starts_with("<@") || body.starts_with('@') {
        if body.starts_with("<@") {
            body = body.split_once('>')?.1.trim_start();
        } else {
            body = body.split_once(char::is_whitespace)?.1.trim_start();
        }
    }
    for polite in ["please ", "请帮我", "帮我", "请", "麻烦"] {
        if let Some(rest) = body.strip_prefix(polite) {
            body = rest.trim_start();
            break;
        }
    }
    if body.starts_with('把')
        && body.lines().next().is_some_and(|l| {
            l.trim_end_matches(['。', '！', ' '])
                .ends_with("整理成任务")
        })
    {
        return Some("create");
    }
    for (action, words) in [
        (
            "comment",
            &["comment ", "reply to ", "评论", "回复任务"][..],
        ),
        (
            "update",
            &[
                "update ",
                "rename ",
                "change status ",
                "修改",
                "更新",
                "改为",
            ][..],
        ),
        (
            "show",
            &[
                "show ",
                "look up ",
                "lookup ",
                "status of ",
                "查询",
                "查看",
                "查一下",
            ][..],
        ),
        (
            "create",
            &[
                "create ",
                "make a task",
                "file a task",
                "建单",
                "建任务",
                "创建",
                "整理成任务",
                "新建",
            ][..],
        ),
    ] {
        if words.iter().any(|w| body.starts_with(w)) {
            return Some(action);
        }
    }
    None
}
fn target(c: &Context<'_>, body: &str, prior: Option<String>) -> Result<Option<Record>> {
    let words = body
        .split(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | '_' | '#'))
        .filter(|s| !s.is_empty())
        .collect::<std::collections::HashSet<_>>();
    let mut public = Vec::new();
    for r in c.records("task")? {
        if r.fields["visibility"] != "private" && c.visible(&r)? {
            public.push(r);
        }
    }
    let mut explicit = public.into_iter().filter(|r| {
        words.contains(format!("#{}", r.id).as_str())
            || r.fields["number"]
                .as_str()
                .is_some_and(|n| words.contains(n) || words.contains(format!("#{n}").as_str()))
    });
    let first = explicit.next();
    if explicit.next().is_some() {
        return Err(invalid("specify one task in this space"));
    }
    if first.is_none()
        && words.iter().any(|w| {
            w.starts_with('#')
                || w.rsplit_once('-').is_some_and(|(prefix, no)| {
                    !prefix.is_empty()
                        && prefix.bytes().all(|b| b.is_ascii_uppercase())
                        && !no.is_empty()
                        && no.bytes().all(|b| b.is_ascii_digit())
                })
        })
    {
        return Ok(None);
    }
    let item = match first {
        Some(r) => Some(r),
        None => prior.map(|id| c.get("task", &id)).transpose()?,
    };
    match item {
        Some(r) if r.fields["visibility"] != "private" && c.visible(&r)? => Ok(Some(r)),
        _ => Ok(None),
    }
}
pub(crate) fn receive(
    c: &Context<'_>,
    connector: &Record,
    b: &Binding,
    payload: &Value,
    mapped: &Value,
    origin: Option<Origin>,
    preview: bool,
) -> Result<Value> {
    let ignored = |reason: &str| Ok(json!({"action":"ignored","reason":reason}));
    if !b.active {
        return ignored("task assistant is unbound");
    }
    let p = required(&connector.fields, "provider")?;
    let Some((thread, external, sender, mentioned)) =
        address(p, payload, &connector.fields, origin)?
    else {
        return ignored("channel has no authorized group task-assistant event");
    };
    let Some(actor) = current_binding(c, b, connector, &sender)? else {
        return ignored("unmapped member or inactive binding");
    };
    let body = required(mapped, "description")?;
    if body.len() > 32768 {
        return Err(invalid("task assistant message exceeds 32 KiB"));
    }
    let key = json!([thread["workspace"], thread["channel"], external]).to_string();
    let event = required(mapped, "event_id")?;
    let receipt: Option<String> =
        c.db.query_row(
            "SELECT external FROM sf_im_assistant_events WHERE space=? AND connector=? AND event=?",
            params![c.space, connector.id, event],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if receipt.as_deref().is_some_and(|other| other != key) {
        return Err(conflict(
            "upstream event ID was reused for a different message",
        ));
    }
    let hash = crate::worker::digest(&json!([thread, sender, body]).to_string());
    let route = thread.to_string();
    let old:Option<(String,String,String)>=c.db.query_row("SELECT hash,run,epoch FROM sf_im_assistant_turns WHERE space=? AND connector=? AND external=?",params![c.space,connector.id,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
    if let Some((prior, run, epoch)) = old {
        if epoch != b.epoch {
            return ignored("earlier binding");
        }
        if prior != hash {
            return Err(conflict("external assistant message ID was reused"));
        }
        if !preview {
            c.db.execute(
                "INSERT OR IGNORE INTO sf_im_assistant_events VALUES(?,?,?,?)",
                params![c.space, connector.id, event, key],
            )
            .map_err(storage)?;
        }
        return Ok(json!({"action":"queued","assistant":b.assistant,"run":run}));
    }
    let mut stmt=c.db.prepare("SELECT actor,body,output,task FROM sf_im_assistant_turns WHERE space=? AND assistant=? AND epoch=? AND route=? ORDER BY at DESC,rowid DESC LIMIT 12").map_err(storage)?;
    let mut history = Vec::new();
    let mut linked = None;
    for row in stmt
        .query_map(params![c.space, b.assistant, b.epoch, route], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(storage)?
    {
        let (who, body, out, task) = row.map_err(storage)?;
        if linked.is_none() {
            linked = task;
        }
        history.push(json!({"member":who,"body":body,"action":out.map(|v|serde_json::from_str::<Value>(&v).unwrap_or(Value::Null))}));
    }
    if !mentioned && history.is_empty() {
        return ignored("mention the task assistant in this channel");
    }
    let Some(action) = intent(body) else {
        return ignored("no explicit task action requested");
    };
    history.reverse();
    let caller = Context {
        actor: &actor,
        ..*c
    };
    let item = if action == "create" {
        None
    } else {
        let Some(t) = target(&caller, body, linked)? else {
            return ignored("name a task in this space or reply to its thread");
        };
        Some(t)
    };
    if preview {
        return Ok(
            json!({"action":"preview","intent":action,"assistant":b.assistant,"target":item.as_ref().map(|r|&r.id)}),
        );
    }
    let system = c.get(KIND, &b.assistant)?;
    let operator = required(&system.fields, "operator")?;
    let runner = Context {
        actor: operator,
        ..*c
    };
    let source = json!({"connector":connector.id,"epoch":b.epoch,"thread":thread,"sender":sender,"actor":actor,"intent":action,"target":item.as_ref().map(|t|&t.id),"rev":item.as_ref().map(|t|t.rev)});
    let format = match action {
        "create" => "Return task_draft with action=create, title and description.",
        "update" => {
            "Return task_draft with action=update and only the requested title, description or status."
        }
        "comment" => "Return task_draft with action=comment and body.",
        "show" => "Return task_draft with action=show. The service retrieves the current task.",
        _ => unreachable!(),
    };
    let fields = json!({"origin_kind":"external_task_assistant","visibility":"private","instruction":format!("{body}\n\n{format}"),"external_body":body,"external_request":source,"external_thread":history,"external_task":item.as_ref().map(|t|json!({"id":t.id,"title":t.title,"number":t.fields["number"],"status":t.fields["status"]}))});
    let run = crate::worker::enqueue(
        &runner,
        &format!("im-assistant:{}:{key}", b.epoch),
        &b.assistant,
        KIND,
        &system.title,
        fields,
    )?;
    c.db.execute("INSERT INTO sf_im_assistant_turns(space,connector,external,assistant,epoch,route,actor,sender,body,hash,run,at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",params![c.space,connector.id,key,b.assistant,b.epoch,route,actor,sender,body,hash,run.id,crate::now()]).map_err(storage)?;
    c.db.execute(
        "INSERT OR IGNORE INTO sf_im_assistant_events VALUES(?,?,?,?)",
        params![c.space, connector.id, event, key],
    )
    .map_err(storage)?;
    Ok(json!({"action":"queued","assistant":b.assistant,"run":run.id}))
}
fn apply(c: &Context<'_>, run: &Record, q: &Value) -> Result<Value> {
    let req = &run.fields["external_request"];
    let actor = required(req, "actor")?;
    let caller = Context { actor, ..*c };
    caller.require_edit()?;
    let id = required(&run.fields, "handler_id")?;
    let assistant = c.get(KIND, id)?;
    let action = required(req, "intent")?;
    let fallback = q["result"].as_str().and_then(|s| {
        let s = s.trim();
        let plain = s
            .strip_prefix("```json")
            .and_then(|v| v.strip_suffix("```"))
            .unwrap_or(s);
        serde_json::from_str::<Value>(plain.trim()).ok()
    });
    let draft = q
        .get("task_draft")
        .filter(|v| v.is_object())
        .or(fallback.as_ref())
        .unwrap_or(&Value::Null);
    if let Some(requested) = draft.get("action").and_then(Value::as_str)
        && requested != action
    {
        return Err(forbidden(
            "runtime action differs from the member's request",
        ));
    }
    let audit = json!({"assistant":assistant.id,"name":assistant.title,"provider":assistant.fields["provider"],"member":actor,"run":run.id});
    let task = if action == "create" {
        let title = draft["title"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| invalid("runtime must provide task_draft.title"))?;
        let description = draft["description"]
            .as_str()
            .unwrap_or(run.fields["external_body"].as_str().unwrap_or(""));
        let t = crate::write_request(
            &caller,
            &json!({"op":"create","kind":"task","title":title,"fields":{"description":description,"visibility":"space"}}),
        )?;
        let t = caller.get("task", required(&t, "id")?)?;
        let mut f = t.fields.clone();
        f["created_by_assistant"] = audit.clone();
        caller.update(&t, &t.title, f)?
    } else {
        let target = required(req, "target")?;
        let task = caller.readable("task", target)?;
        if task.fields["visibility"] == "private"
            || draft["task"].as_str().is_some_and(|x| x != target)
        {
            return Err(forbidden("requested task changed or is unavailable"));
        }
        match action {
            "show" => task,
            "update" => {
                let mut command = json!({"op":"update","kind":"task","id":target,"expected_rev":req["rev"],"fields":{}});
                if let Some(v) = draft.get("title") {
                    command["title"] = json!(template::text(v, 500)?);
                }
                for k in ["status", "description"] {
                    if let Some(v) = draft.get(k) {
                        command["fields"][k] = json!(template::text(v, 32768)?);
                    }
                }
                if command["fields"] == json!({}) && command["title"].is_null() {
                    return Err(invalid("runtime must provide title, description or status"));
                }
                crate::write_request(&caller, &command)?;
                caller.get("task", target)?
            }
            "comment" => {
                let body = template::text(&draft["body"], 32768)?;
                let m = crate::write_request(
                    &caller,
                    &json!({"op":"create","kind":"comment","title":assistant.title,"fields":{"task":target,"body":body}}),
                )?;
                let m = caller.get("comment", required(&m, "id")?)?;
                let mut f = m.fields.clone();
                f["created_by_assistant"] = audit.clone();
                caller.update(&m, &m.title, f)?;
                task
            }
            _ => return Err(invalid("unknown assistant action")),
        }
    };
    let system = Context { actor: id, ..*c };
    system.event(&task, &format!("task_assistant.{action}"))?;
    let number = task.fields["number"].as_str().unwrap_or(&task.id);
    let status = task.fields["status"].as_str().unwrap_or("");
    let text = format!("{} · {number} · {} · {status}", assistant.title, task.title);
    let connector = caller.readable("connector", required(req, "connector")?)?;
    if connector.fields["provider"] == "slack" && connector.fields["outbound"] == true {
        c.db.execute("INSERT OR IGNORE INTO sf_im_outbox(id,space,connector,route,task,kind,source,rev,body) VALUES(?,?,?,?,?,'task_assistant',?,1,?)",params![Uuid::new_v4().to_string(),c.space,connector.id,req["thread"].to_string(),task.id,run.id,text]).map_err(storage)?;
    }
    Ok(
        json!({"action":action,"task":task.id,"number":number,"actor":actor,"assistant":id,"result":text}),
    )
}
pub(crate) fn finish(c: &Context<'_>, run: &Record, q: &Value, f: &mut Value) -> Result<()> {
    let result = if f["status"] == "done" {
        c.db.execute_batch("SAVEPOINT space_assistant_apply")
            .map_err(storage)?;
        let result = apply(c, run, q);
        if result.is_err() {
            c.db.execute_batch("ROLLBACK TO space_assistant_apply")
                .map_err(storage)?;
        }
        c.db.execute_batch("RELEASE space_assistant_apply")
            .map_err(storage)?;
        match result {
            Ok(data) => data,
            Err(error) => {
                f["status"] = json!("failed");
                f["result"] = json!(error);
                json!({"action":"failed","error":error})
            }
        }
    } else {
        json!({"action":f["status"]})
    };
    c.db.execute(
        "UPDATE sf_im_assistant_turns SET task=?,output=? WHERE run=?",
        params![result["task"].as_str(), result.to_string(), run.id],
    )
    .map_err(storage)?;
    f["assistant_action"] = result;
    Ok(())
}
pub(super) fn history(c: &Context<'_>, assistant: Option<&Record>) -> Result<Value> {
    let Some(assistant) = assistant else {
        return Ok(json!([]));
    };
    let mut s=c.db.prepare("SELECT run,actor,route,output,at FROM sf_im_assistant_turns WHERE space=? AND assistant=? ORDER BY at DESC,rowid DESC LIMIT 100").map_err(storage)?;
    let mut out = Vec::new();
    for x in s
        .query_map(params![c.space, assistant.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(storage)?
    {
        let (run, actor, thread, result, at) = x.map_err(storage)?;
        out.push(json!({"run":run,"actor":actor,"thread":serde_json::from_str::<Value>(&thread).map_err(storage)?,"result":result.map(|v|serde_json::from_str::<Value>(&v)).transpose().map_err(storage)?,"at":at}));
    }
    Ok(json!(out))
}
