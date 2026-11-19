//! Per-space external task assistants have separate identities from user-configured Agents.
use crate::{Context, Record, Result, conflict, forbidden, invalid, required, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;
mod template;
mod turns;
pub(crate) use turns::{current, deliverable, finish, receive, route};
pub(crate) const KIND: &str = "im_assistant";
const PROVIDERS: [&str; 5] = ["feishu", "lark", "slack", "wecom", "dingtalk"];
pub(crate) const READS: &[&str] = &[
    "task_assistant.catalog",
    "task_assistant.settings",
    "task_assistant.template",
    "task_assistant.history",
];
pub(crate) const WRITES: &[&str] = &[
    "task_assistant.bind",
    "task_assistant.configure",
    "task_assistant.unbind",
    "task_assistant.remove",
    "task_assistant.template.save",
];
pub(crate) fn handles(op: &str) -> bool {
    READS.contains(&op) || WRITES.contains(&op)
}
pub(crate) fn post(raw: &Value) -> Result<String> {
    let post = if raw["content"].is_array() {
        raw
    } else {
        raw.get("zh_cn")
            .or_else(|| raw.get("en_us"))
            .or_else(|| {
                raw.as_object()
                    .and_then(|o| o.values().find(|v| v["content"].is_array()))
            })
            .ok_or_else(|| invalid("missing Feishu rich-text content"))?
    };
    let mut lines = Vec::new();
    for line in post["content"]
        .as_array()
        .ok_or_else(|| invalid("invalid Feishu rich-text content"))?
    {
        let mut text = String::new();
        for n in line
            .as_array()
            .ok_or_else(|| invalid("invalid Feishu paragraph"))?
        {
            let value = n["text"].as_str().unwrap_or("");
            match n["tag"].as_str() {
                Some("text" | "md") => text.push_str(value),
                Some("a") => {
                    text.push_str(value);
                    text.push(' ');
                    text.push_str(n["href"].as_str().unwrap_or(""));
                }
                Some("at") => {
                    text.push('@');
                    text.push_str(
                        n["user_name"]
                            .as_str()
                            .or(n["user_id"].as_str())
                            .unwrap_or("bot"),
                    );
                    text.push(' ');
                }
                Some("code_block") => text.push_str(&format!(
                    "\n~~~{}\n{}\n~~~",
                    n["language"].as_str().unwrap_or(""),
                    value
                )),
                _ => {}
            }
        }
        lines.push(text);
    }
    Ok(lines.join("\n"))
}
pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_im_assistant_templates(organization TEXT PRIMARY KEY,rev INTEGER NOT NULL,templates TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS sf_im_assistant_slots(space TEXT NOT NULL,provider TEXT NOT NULL,assistant TEXT NOT NULL,PRIMARY KEY(space,provider));
        CREATE TABLE IF NOT EXISTS sf_im_assistant_routes(space TEXT NOT NULL,connector TEXT NOT NULL,assistant TEXT NOT NULL,epoch TEXT NOT NULL,active INTEGER NOT NULL,PRIMARY KEY(space,connector));
        CREATE TABLE IF NOT EXISTS sf_im_assistant_events(space TEXT NOT NULL,connector TEXT NOT NULL,event TEXT NOT NULL,external TEXT NOT NULL,PRIMARY KEY(space,connector,event));
        CREATE TABLE IF NOT EXISTS sf_im_assistant_turns(space TEXT NOT NULL,connector TEXT NOT NULL,external TEXT NOT NULL,assistant TEXT NOT NULL,epoch TEXT NOT NULL,route TEXT NOT NULL,actor TEXT NOT NULL,sender TEXT NOT NULL,body TEXT NOT NULL,hash TEXT NOT NULL,run TEXT NOT NULL UNIQUE,task TEXT,output TEXT,at INTEGER NOT NULL,PRIMARY KEY(space,connector,external));
        CREATE INDEX IF NOT EXISTS sf_im_assistant_context ON sf_im_assistant_turns(space,assistant,epoch,route,at);").map_err(storage)
}
fn label<'a>(p: &'a str, lang: &str) -> &'a str {
    match (p, lang) {
        ("feishu", "en") => "Feishu",
        ("feishu", _) => "飞书",
        ("wecom", "en") => "WeCom",
        ("wecom", _) => "企业微信",
        ("dingtalk", "en") => "DingTalk",
        ("dingtalk", _) => "钉钉",
        ("slack", _) => "Slack",
        ("lark", _) => "Lark",
        _ => p,
    }
}
fn provider(q: &Value) -> Result<&str> {
    let p = required(q, "provider")?;
    if PROVIDERS.contains(&p) {
        Ok(p)
    } else {
        Err(invalid("unsupported task assistant channel"))
    }
}
fn slot(c: &Context<'_>, p: &str) -> Result<Option<Record>> {
    let id: Option<String> =
        c.db.query_row(
            "SELECT assistant FROM sf_im_assistant_slots WHERE space=? AND provider=?",
            params![c.space, p],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    id.map(|id| c.get(KIND, &id)).transpose()
}
fn title(c: &Context<'_>, p: &str, lang: &str) -> Result<String> {
    let area = c.get("space", c.space)?.title;
    Ok(if lang == "en" {
        format!("{} task assistant · {area}", label(p, lang))
    } else {
        format!("{}任务助手 · {area}", label(p, lang))
    })
}
fn view(c: &Context<'_>, r: Option<&Record>, p: &str, lang: &str, detail: bool) -> Result<Value> {
    let mut v = json!({"space":c.space,"space_name":c.get("space",c.space)?.title,"provider":p,"title":title(c,p,lang)?,"id":r.map(|a|&a.id),"rev":r.map_or(0,|a|a.rev),"state":r.map(|a|&a.fields["state"]).unwrap_or(&Value::Null),"can_manage":c.require_admin().is_ok()});
    let org = c.org_id()?;
    v["can_edit_template"] = json!((Context { space: &org, ..*c }).require_admin().is_ok());
    v["transport"] = json!(match p {
        "slack" => "signed_slack_events",
        "wecom" => "configuration_only",
        _ => "authenticated_relay",
    });
    if r.is_none() {
        v["state"] = json!("unbound");
    } else if let Some(r) = r {
        v["connector"] = r.fields["connector"].clone();
        v["runtime"] = r.fields["runtime"].clone();
        if detail {
            c.require_admin()?;
            v["effective"] = template::effective(c, r, lang)?;
            v["settings"] = r.fields.clone();
        }
    }
    Ok(v)
}
fn catalog(c: &Context<'_>, q: &Value) -> Result<Value> {
    let lang = template::locale(c, q)?;
    let org = c.org_id()?;
    let mut rows = Vec::new();
    for area in c.all_spaces()? {
        if area.fields["organization"] != org || area.fields["deactivated"] == true {
            continue;
        }
        let local = Context {
            space: &area.id,
            ..*c
        };
        if local.role().is_err() || c.actor != "owner" && local.find("member", c.actor)?.is_none() {
            continue;
        }
        for p in PROVIDERS {
            if q.get("provider").is_some() && provider(q)? != p {
                continue;
            }
            rows.push(view(&local, slot(&local, p)?.as_ref(), p, &lang, false)?);
        }
    }
    Ok(json!(rows))
}
fn bounded_id(q: &Value, key: &str) -> Result<String> {
    let id = required(q, key)?;
    crate::identifier(id)?;
    Ok(id.into())
}
fn settings(c: &Context<'_>, q: &Value, f: &mut Value) -> Result<()> {
    if let Some(v) = q.get("sender_map") {
        let members = v
            .as_object()
            .filter(|m| m.len() <= 2048)
            .ok_or_else(|| invalid("map external user IDs to space members"))?;
        for (external, value) in members {
            if external.trim().is_empty()
                || external.len() > 256
                || external.chars().any(char::is_control)
            {
                return Err(invalid("invalid external user ID"));
            }
            c.readable("member", template::text(value, 256)?)?;
        }
        f["sender_map"] = v.clone();
    }
    if let Some(v) = q.get("runtime") {
        let rt = template::text(v, 128)?;
        if rt.chars().any(char::is_whitespace) {
            return Err(invalid("invalid Runtime"));
        }
        f["runtime"] = json!(rt);
    }
    if let Some(v) = q.get("model") {
        f["model"] = if v.is_null() {
            Value::Null
        } else {
            json!(template::text(v, 256)?)
        };
    }
    if let Some(v) = q.get("machine_id") {
        if let Some(id) = v.as_str() {
            c.readable("machine", id)?;
        } else if !v.is_null() {
            return Err(invalid("invalid machine"));
        }
        f["machine_id"] = v.clone();
    }
    if let Some(id) = f["machine_id"].as_str() {
        let m = c.readable("machine", id)?;
        if m.fields["disabled"] == true
            || !m.fields["runtimes"]
                .as_array()
                .is_some_and(|v| v.contains(&f["runtime"]))
        {
            return Err(invalid(
                "the selected machine does not advertise this Runtime",
            ));
        }
    }
    if let Some(v) = q.get("skills") {
        let ids = crate::domain::strings(v, "skills")?;
        if ids.len() > 64 {
            return Err(invalid("choose up to 64 skills"));
        }
        for id in ids {
            let s = c.readable("skill", id)?;
            if s.fields["visibility"] == "private" {
                return Err(forbidden("space task assistants require shared skills"));
            }
        }
        f["skills"] = v.clone();
    }
    let lang = template::locale(c, q)?;
    for (key, limit) in [("description", 4096), ("instructions", 32768)] {
        if let Some(v) = q.get(key) {
            if v.is_null() {
                if let Some(m) = f["overrides"][&lang].as_object_mut() {
                    m.remove(key);
                }
            } else {
                f["overrides"][&lang][key] = json!(template::text(v, limit)?);
            }
        }
    }
    Ok(())
}
fn identity(f: &Value) -> Value {
    json!([
        f["provider"],
        f["workspace_id"],
        f["tenant_key"],
        f["channel_id"],
        f["chat_id"],
        f["app_id"],
        f["bot_id"],
        f["bot_user_id"],
        f["bot_open_id"],
        f["corp_id"]
    ])
}
pub(crate) fn connector_changed(c: &Context<'_>, old: &Record, new: &Record) -> Result<()> {
    if new.kind != "connector"
        || new.fields["enabled"] == true && identity(&old.fields) == identity(&new.fields)
    {
        return Ok(());
    }
    let Some(row) = route(c, &new.id)? else {
        return Ok(());
    };
    if row.active {
        let assistant = c.get(KIND, &row.assistant)?;
        disarm(c, &assistant)?;
        let mut f = assistant.fields.clone();
        f["state"] = json!("disconnected");
        f["connector"] = Value::Null;
        c.update(&assistant, &assistant.title, f)?;
    }
    Ok(())
}
fn disarm(c: &Context<'_>, r: &Record) -> Result<()> {
    c.db.execute(
        "UPDATE sf_im_assistant_routes SET active=0 WHERE space=? AND assistant=?",
        params![c.space, r.id],
    )
    .map_err(storage)?;
    let mut stmt=c.db.prepare("SELECT r.id FROM sf_im_assistant_turns t JOIN records r ON r.space=t.space AND r.kind='run' AND r.id=t.run WHERE t.space=? AND t.assistant=? AND t.output IS NULL AND json_extract(r.fields,'$.status') IN ('queued','running','waiting','cancelling')").map_err(storage)?;
    let active = stmt
        .query_map(params![c.space, r.id], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for id in active {
        let run = crate::worker::cancel(c, &c.get("run", &id)?)?;
        if run.fields["status"] == "cancelled" {
            c.db.execute(
                "UPDATE sf_im_assistant_turns SET output=? WHERE run=?",
                params![json!({"action":"cancelled"}).to_string(), id],
            )
            .map_err(storage)?;
        }
    }
    c.db.execute("UPDATE sf_im_outbox SET state='cancelled',lease=NULL WHERE space=? AND source IN(SELECT run FROM sf_im_assistant_turns WHERE space=? AND assistant=?) AND kind='task_assistant' AND state IN('pending','leased','uncertain','failed')",params![c.space,c.space,r.id]).map_err(storage)?;
    Ok(())
}
pub(crate) fn replay(c: &Context<'_>, op: &str, _q: &Value) -> Result<()> {
    if op == "task_assistant.template.save" {
        let org = c.org_id()?;
        Context { space: &org, ..*c }.require_admin()
    } else {
        c.require_admin()
    }
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op == "task_assistant.catalog" {
        return catalog(c, q);
    }
    if matches!(
        op,
        "task_assistant.template" | "task_assistant.template.save"
    ) {
        return template::execute(c, q, op.ends_with(".save"));
    }
    c.require_admin()?;
    let p = provider(q)?;
    let lang = template::locale(c, q)?;
    let old = slot(c, p)?;
    if op == "task_assistant.settings" {
        return view(c, old.as_ref(), p, &lang, true);
    }
    if op == "task_assistant.history" {
        return turns::history(c, old.as_ref());
    }
    if q["expected_rev"].as_i64() != Some(old.as_ref().map_or(0, |r| r.rev)) {
        return Err(conflict("refresh the space task assistant first"));
    }
    let mut f=old.as_ref().map(|r|r.fields.clone()).unwrap_or_else(||json!({"provider":p,"visibility":"space","agent_type":"office","directory_mode":"isolated","max_parallel":1,"chat_parallel":1,"skills":[],"tools":[],"overrides":{},"creator":c.actor}));
    match op {
        "task_assistant.bind" => {
            let connector = bounded_id(q, "connector")?;
            let b = c.readable("connector", &connector)?;
            if b.fields["enabled"] != true || b.fields["provider"] != p {
                return Err(invalid(
                    "choose an enabled connector of the same channel and space",
                ));
            }
            let fields: &[&str] = match p {
                "slack" => &["workspace_id", "channel_id", "app_id", "bot_id"],
                "feishu" | "lark" => &["tenant_key", "chat_id", "bot_open_id"],
                "dingtalk" => &["corp_id", "chat_id"],
                "wecom" => &["corp_id", "app_id"],
                _ => &[],
            };
            for name in fields {
                required(&b.fields, name)?;
            }
            settings(c, q, &mut f)?;
            required(&f, "runtime")?;
            if let Some(r) = old.as_ref() {
                if r.fields["state"] == "connected" {
                    return Err(conflict("unbind the current connector first"));
                }
                disarm(c, r)?;
            }
            f["operator"] = json!(c.actor);
            f["connector"] = json!(connector);
            f["epoch"] = json!(Uuid::new_v4().to_string());
            f["identity"] = identity(&b.fields);
            f["state"] = json!("connected");
        }
        "task_assistant.configure" => {
            if old.is_none() {
                return Err(invalid("bind this space assistant first"));
            }
            settings(c, q, &mut f)?;
        }
        "task_assistant.unbind" | "task_assistant.remove" => {
            let r = old
                .as_ref()
                .ok_or_else(|| invalid("the assistant does not exist"))?;
            disarm(c, r)?;
            f["connector"] = Value::Null;
            f["state"] = json!(if op.ends_with(".remove") {
                "removed"
            } else {
                "disconnected"
            });
        }
        _ => return Err(invalid("unknown task assistant operation")),
    }
    let name = title(c, p, &lang)?;
    let r = if let Some(r) = old {
        c.update(&r, &name, f)?
    } else {
        c.create(KIND, None, &name, f)?
    };
    if op == "task_assistant.bind" {
        c.db.execute("INSERT INTO sf_im_assistant_slots VALUES(?,?,?) ON CONFLICT(space,provider) DO UPDATE SET assistant=excluded.assistant",params![c.space,p,r.id]).map_err(storage)?;
        c.db.execute("INSERT INTO sf_im_assistant_routes VALUES(?,?,?,?,1) ON CONFLICT(space,connector) DO UPDATE SET assistant=excluded.assistant,epoch=excluded.epoch,active=1",params![c.space,r.fields["connector"].as_str(),r.id,r.fields["epoch"].as_str()]).map_err(storage)?;
    } else if op == "task_assistant.remove" {
        c.db.execute(
            "DELETE FROM sf_im_assistant_slots WHERE space=? AND provider=?",
            params![c.space, p],
        )
        .map_err(storage)?;
        return view(c, None, p, &lang, false);
    }
    view(c, Some(&r), p, &lang, true)
}
pub(crate) fn snapshot(c: &Context<'_>, id: &str) -> Result<Value> {
    let r = c.readable(KIND, id)?;
    if r.fields["state"] != "connected" {
        return Err(conflict("task assistant is unbound"));
    }
    let mut config = r.fields.clone();
    let selected = template::effective(c, &r, &template::locale(c, &Value::Null)?)?;
    config["instructions"] = selected["instructions"].clone();
    config["description"] = selected["description"].clone();
    config["agent_id"] = json!(id);
    let mut skills = Vec::new();
    for id in config["skills"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let skill = c.readable("skill", id)?;
        if skill.fields["visibility"] == "private" {
            return Err(forbidden("assistant skill became private"));
        }
        skills.push(json!({"id":id,"rev":skill.rev,"title":skill.title,"instructions":skill.fields["instructions"],"fields":skill.fields}));
    }
    config["skill_versions"] = json!(skills);
    Ok(config)
}
pub(crate) fn extend_schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().unwrap();
    for (ops, keys) in [
        (
            vec!["task_assistant.settings", "task_assistant.history"],
            vec!["provider"],
        ),
        (
            vec!["task_assistant.bind"],
            vec!["provider", "connector", "runtime", "expected_rev"],
        ),
        (
            vec![
                "task_assistant.configure",
                "task_assistant.unbind",
                "task_assistant.remove",
            ],
            vec!["provider", "expected_rev"],
        ),
        (
            vec!["task_assistant.template.save"],
            vec!["language", "expected_rev"],
        ),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"enum":ops}}},"then":{"required":keys,"properties":{"provider":{"enum":PROVIDERS},"language":{"enum":["zh-CN","en"]}}}}));
    }
}
