use super::*;
pub(super) fn provider(s: &str) -> bool {
    matches!(s, "feishu" | "lark")
}
pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS lark_agent_per_cloud ON external_bots(space,agent,provider) WHERE provider IN('feishu','lark') AND state='connected';
      CREATE TABLE IF NOT EXISTS lark_bot_config(bot TEXT PRIMARY KEY,settings TEXT NOT NULL,tenant TEXT);
      CREATE TABLE IF NOT EXISTS lark_bot_events(bot TEXT,event TEXT,result TEXT NOT NULL,PRIMARY KEY(bot,event));
      CREATE TABLE IF NOT EXISTS lark_bot_targets(bot TEXT,turn TEXT PRIMARY KEY,tenant TEXT NOT NULL,channel TEXT NOT NULL,source TEXT NOT NULL,root TEXT NOT NULL,in_thread INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS lark_bot_source ON lark_bot_targets(bot,tenant,source);").map_err(storage)
}
fn defaults() -> Value {
    json!({"plain_chats":[],"reactions":false,"trigger_emoji":"THUMBSUP","stop_emoji":"CrossMark","thread_followup":false})
}
pub(super) fn settings(c: &Context<'_>, b: &Binding) -> Result<Value> {
    let s: Option<String> =
        c.db.query_row(
            "SELECT settings FROM lark_bot_config WHERE bot=?",
            [&b.id],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    s.map(|raw| serde_json::from_str(&raw).map_err(storage))
        .transpose()
        .map(|s| s.unwrap_or_else(defaults))
}
pub(super) fn policy(c: &Context<'_>, b: &Binding, q: &Value) -> Result<Value> {
    if !provider(&b.provider) {
        return Err(invalid("expected a Feishu or Lark binding"));
    }
    let mut s = settings(c, b)?;
    for key in ["reactions", "thread_followup"] {
        s[key] = json!(boolean(q, key, s[key] == true)?);
    }
    for key in ["trigger_emoji", "stop_emoji"] {
        if q.get(key).is_some() {
            let e = short(q, key)?;
            if e.len() > 64 || !e.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_') {
                return Err(invalid("use the platform emoji type"));
            }
            s[key] = json!(e);
        }
    }
    if s["stop_emoji"]
        .as_str()
        .unwrap()
        .eq_ignore_ascii_case(s["trigger_emoji"].as_str().unwrap())
    {
        return Err(invalid("use distinct start and stop emoji"));
    }
    if let Some(chats) = q.get("plain_chats") {
        let list = chats
            .as_array()
            .filter(|a| a.len() <= 64)
            .ok_or_else(|| invalid("plain_chats must contain up to 64 chat IDs"))?;
        let mut ids = std::collections::BTreeSet::new();
        for v in list {
            let id = v
                .as_str()
                .filter(|id| {
                    id.starts_with("oc_")
                        && id.len() <= 128
                        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                .ok_or_else(|| invalid("a specific oc_ chat ID is required"))?;
            ids.insert(id);
        }
        s["plain_chats"] = json!(ids);
    }
    let dm = boolean(q, "dm", b.dm)?;
    let groups = boolean(q, "groups", b.groups)?;
    if !dm {
        deliveries::cancel(c, b, Some("direct"))?;
    }
    if !groups && s["plain_chats"] == json!([]) && s["reactions"] == false {
        deliveries::cancel(c, b, Some("group"))?;
    }
    c.db.execute(
        "UPDATE external_bots SET dm=?,groups_enabled=? WHERE id=?",
        params![dm, groups, b.id],
    )
    .map_err(storage)?;
    c.db.execute("INSERT INTO lark_bot_config(bot,settings) VALUES(?,?) ON CONFLICT(bot) DO UPDATE SET settings=excluded.settings",params![b.id,s.to_string()]).map_err(storage)?;
    let mut result = find(c, q)?.summary();
    result["settings"] = s;
    Ok(result)
}
fn remember(c: &Context<'_>, b: &Binding, event: &str, data: Value) -> Result<Value> {
    c.db.execute(
        "INSERT INTO lark_bot_events(bot,event,result) VALUES(?,?,?)",
        params![b.id, event, data.to_string()],
    )
    .map_err(storage)?;
    Ok(data)
}
pub(super) fn event(c: &Context<'_>, b: &Binding, q: &Value) -> Result<Value> {
    if !provider(&b.provider) || q["app_id"] != b.account {
        return Err(forbidden("event does not belong to the bound application"));
    }
    let tenant = short(q, "tenant")?;
    let event = short(q, "event_id")?;
    let source = short(q, "source")?;
    let channel = short(q, "channel")?;
    let speaker = short(q, "sender")?;
    let s = settings(c, b)?;
    let verified: Option<String> =
        c.db.query_row(
            "SELECT tenant FROM lark_bot_config WHERE bot=?",
            [&b.id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(storage)?
        .flatten();
    if verified.as_deref().is_some_and(|prior| prior != tenant) {
        return Ok(json!({"ignored":"other_tenant"}));
    }
    c.db.execute("INSERT INTO lark_bot_config(bot,settings,tenant) VALUES(?,?,?) ON CONFLICT(bot) DO UPDATE SET tenant=excluded.tenant WHERE tenant IS NULL",params![b.id,s.to_string(),tenant]).map_err(storage)?;
    if let Some(raw) =
        c.db.query_row(
            "SELECT result FROM lark_bot_events WHERE bot=? AND event=?",
            params![b.id, event],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(storage)?
    {
        let mut data: Value = serde_json::from_str(&raw).map_err(storage)?;
        data["duplicate"] = json!(true);
        return Ok(data);
    }
    let scene = required(q, "scene")?;
    if !matches!(scene, "direct" | "group") {
        return Err(invalid("invalid message scene"));
    }
    let root = q["root"].as_str().unwrap_or("");
    if root.len() > 128 || root.chars().any(char::is_control) {
        return Err(invalid("invalid conversation topic"));
    }
    let emoji = q["reaction"].as_str();
    let in_topic = q["topic_reply"].as_bool().unwrap_or(!root.is_empty());
    let kind = q["trigger"].as_str().unwrap_or("message");
    if kind == "reaction"
        && emoji.is_some_and(|e| {
            s["stop_emoji"]
                .as_str()
                .is_some_and(|stop| e.eq_ignore_ascii_case(stop))
        })
    {
        let run:Option<String>=c.db.query_row("SELECT t.run FROM lark_bot_targets x JOIN external_bot_turns t ON t.id=x.turn WHERE x.bot=? AND x.tenant=? AND x.channel=? AND x.source=? AND t.run IS NOT NULL ORDER BY t.rowid DESC LIMIT 1",params![b.id,tenant,channel,source],|r|r.get(0)).optional().map_err(storage)?;
        let mut changed = false;
        if let Some(id) = run
            && let Some(run) = c.find("run", &id)?
            && c.visible(&run)?
            && matches!(
                run.fields["status"].as_str(),
                Some("running" | "queued" | "waiting")
            )
        {
            crate::worker::cancel(c, &run)?;
            changed = true;
        }
        return remember(c, b, event, json!({"stopped":changed,"duplicate":false}));
    }
    let permitted = match (scene, kind) {
        ("direct", "message") => b.dm,
        ("group", "message") if q["mentions_bot"] == true => b.groups,
        ("group", "message") => {
            q["mentions_other"] != true
                && s["plain_chats"]
                    .as_array()
                    .is_some_and(|v| v.contains(&json!(channel)))
                && (!in_topic || s["thread_followup"] == true)
        }
        ("group", "reaction") => {
            s["reactions"] == true
                && emoji.is_some_and(|e| {
                    s["trigger_emoji"]
                        .as_str()
                        .is_some_and(|start| e.eq_ignore_ascii_case(start))
                })
        }
        _ => false,
    };
    if !permitted {
        return remember(c, b, event, json!({"ignored":"trigger_disabled"}));
    }
    let body = q["body"].as_str().unwrap_or("").trim();
    if body.is_empty() {
        return remember(c, b, event, json!({"ignored":"no_readable_content"}));
    }
    let peer = json!([tenant, channel, root]).to_string();
    let remote = if kind == "reaction" {
        format!("reaction-{event}")
    } else {
        source.into()
    };
    let out = turns::receive(
        c,
        b,
        &json!({"scene":scene,"peer":peer,"sender":speaker,"remote":remote,"body":body,"files":q["files"]}),
    )?;
    if let Some(turn) = out["turn"].as_str() {
        c.db.execute("INSERT INTO lark_bot_targets(bot,turn,tenant,channel,source,root,in_thread) VALUES(?,?,?,?,?,?,?) ON CONFLICT(turn) DO NOTHING",params![b.id,turn,tenant,channel,source,root,scene=="group"&&(in_topic||kind=="reaction"||q["topic_group"]==true)]).map_err(storage)?;
    }
    remember(c, b, event, out)
}
pub(super) fn destination(c: &Context<'_>, b: &Binding, id: &str) -> Result<Value> {
    c.db.query_row("SELECT x.source,x.channel,x.root,x.in_thread FROM external_bot_deliveries d JOIN lark_bot_targets x ON x.turn=d.turn WHERE d.id=? AND x.bot=?",params![id,b.id],|r|Ok(json!({"source":r.get::<_,String>(0)?,"channel":r.get::<_,String>(1)?,"root":r.get::<_,String>(2)?,"in_thread":r.get::<_,bool>(3)?}))).map_err(storage)
}
pub(crate) fn run_files(
    c: &Context<'_>,
    run: &Record,
    message: &Record,
    context: &mut Value,
) -> Result<()> {
    let Some(items) = message.fields["attachments"]
        .as_array()
        .filter(|a| !a.is_empty())
    else {
        return Ok(());
    };
    if !message.fields["external_provider"]
        .as_str()
        .is_some_and(|p| provider(p) || p == "wecom")
    {
        return Ok(());
    }
    let bound:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM external_bot_turns t JOIN external_bots b ON b.id=t.bot WHERE t.message=? AND t.run=? AND b.space=? AND b.owner=? AND (t.provider='wecom' OR t.provider IN('feishu','lark') AND EXISTS(SELECT 1 FROM lark_bot_targets d WHERE d.turn=t.id)))",params![message.id,run.id,c.space,c.actor],|r|r.get(0)).map_err(storage)?;
    if !bound {
        return Ok(());
    }
    let mut bytes = 0;
    let mut files = Vec::new();
    for item in items.iter().take(6) {
        bytes += item["bytes"].as_u64().unwrap_or(0);
        if bytes > 4 * 1024 * 1024 {
            break;
        }
        files.push(crate::attachments::download(
            c,
            &json!({"kind":"message","id":message.id,"attachment_id":item["id"]}),
        )?);
    }
    if !files.is_empty() {
        context["input_files"] = json!(files);
    }
    Ok(())
}
pub(super) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"const":"bot.bindings"}}},"then":{"properties":{"available_for":{"enum":["feishu","lark","wecom"]}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"bot.lark.event"}}},"then":{"required":["id","device_key","app_id","tenant","event_id","source","channel","scene","sender"],"properties":{"trigger":{"enum":["message","reaction"]},"scene":{"enum":["direct","group"]},"root":{"type":"string"},"mentions_bot":{"type":"boolean"},"mentions_other":{"type":"boolean"},"topic_group":{"type":"boolean"},"topic_reply":{"type":"boolean"},"reaction":{"type":"string"}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"bot.lark.policy"}}},"then":{"required":["id"],"properties":{"dm":{"type":"boolean"},"groups":{"type":"boolean"},"plain_chats":{"type":"array","maxItems":64,"items":{"type":"string","pattern":"^oc_[A-Za-z0-9_]+$"}},"reactions":{"type":"boolean"},"thread_followup":{"type":"boolean"},"trigger_emoji":{"type":"string"},"stop_emoji":{"type":"string"}}}}));
}
