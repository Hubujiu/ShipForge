//! Authenticated external Agent conversations. Provider credentials live on the connected device.
use super::*;
use rusqlite::OptionalExtension;
use uuid::Uuid;
mod deliveries;
mod lark;
mod turns;
pub(crate) use lark::run_files;

pub(crate) const READS: &[&str] = &["bot.bindings"];
pub(crate) const WRITES: &[&str] = &[
    "bot.attach",
    "bot.policy",
    "bot.detach",
    "bot.lark.policy",
    "bot.lark.event",
    "bot.expire",
    "bot.receive",
    "bot.claim",
    "bot.ack",
    "bot.retry",
];
pub(crate) fn handles(op: &str) -> bool {
    READS.contains(&op) || WRITES.contains(&op)
}
pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("
        CREATE TABLE IF NOT EXISTS external_bots (
            id TEXT PRIMARY KEY,space TEXT NOT NULL,owner TEXT NOT NULL,provider TEXT NOT NULL,account TEXT NOT NULL,
            agent TEXT NOT NULL,device TEXT NOT NULL,dm INTEGER NOT NULL,groups_enabled INTEGER NOT NULL,state TEXT NOT NULL);
        CREATE UNIQUE INDEX IF NOT EXISTS external_bot_account ON external_bots(provider,account) WHERE state='connected';
        CREATE TABLE IF NOT EXISTS external_bot_rooms (bot TEXT,scene TEXT,peer TEXT,chat TEXT NOT NULL,PRIMARY KEY(bot,scene,peer));
        CREATE TABLE IF NOT EXISTS external_bot_turns (id TEXT PRIMARY KEY,bot TEXT NOT NULL,provider TEXT NOT NULL,account TEXT NOT NULL,
            scene TEXT NOT NULL,peer TEXT NOT NULL,remote TEXT NOT NULL,digest TEXT NOT NULL,chat TEXT NOT NULL,message TEXT,run TEXT,prepared INTEGER NOT NULL DEFAULT 0,
            UNIQUE(provider,account,scene,peer,remote));
        CREATE INDEX IF NOT EXISTS external_bot_chat ON external_bot_turns(chat,bot);
        CREATE INDEX IF NOT EXISTS external_bot_results ON external_bot_turns(bot,prepared);
        CREATE TABLE IF NOT EXISTS external_bot_deliveries (id TEXT PRIMARY KEY,turn TEXT NOT NULL,part INTEGER NOT NULL,
            payload TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'pending',lease TEXT,expires INTEGER,available INTEGER NOT NULL DEFAULT 0,
            receipt TEXT, UNIQUE(turn,part));").map_err(storage)?;
    lark::init(db)
}
pub(crate) fn external_room(c: &Context<'_>, id: &str) -> Result<bool> {
    c.db.query_row("SELECT EXISTS(SELECT 1 FROM external_bot_turns t JOIN external_bots b ON b.id=t.bot WHERE b.space=? AND t.chat=?)",params![c.space,id],|r|r.get(0)).map_err(storage)
}
fn room_visible(c: &Context<'_>, id: &str) -> Result<bool> {
    match c.find("chat", id)? {
        Some(chat) => c.visible(&chat),
        None => Ok(false),
    }
}
fn short<'a>(q: &'a Value, key: &str) -> Result<&'a str> {
    let s = required(q, key)?;
    if s.is_empty() || s.len() > 512 || s.chars().any(char::is_control) {
        return Err(invalid(&format!("invalid {key}")));
    }
    Ok(s)
}
fn boolean(q: &Value, k: &str, default: bool) -> Result<bool> {
    q.get(k).map_or(Ok(default), |v| {
        v.as_bool()
            .ok_or_else(|| invalid(&format!("{k} must be a boolean")))
    })
}
fn device(q: &Value) -> Result<String> {
    let s = required(q, "device_key")?;
    if !(32..=256).contains(&s.len()) {
        return Err(invalid("invalid adapter key"));
    }
    Ok(crate::worker::digest(s))
}
#[derive(Clone)]
struct Binding {
    id: String,
    space: String,
    owner: String,
    provider: String,
    account: String,
    agent: String,
    device: String,
    dm: bool,
    groups: bool,
    state: String,
}
impl Binding {
    fn summary(&self) -> Value {
        json!({"id":self.id,"space":self.space,"provider":self.provider,"account":self.account,"agent":self.agent,"dm":self.dm,"groups":self.groups,"state":self.state})
    }
    fn permits(&self, scene: &str) -> bool {
        match scene {
            "direct" => self.dm,
            "group" => self.provider == "qq" && self.groups,
            _ => false,
        }
    }
}
const COLS: &str = "id, space, owner, provider, account, agent, device, dm, groups_enabled, state";
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Binding> {
    Ok(Binding {
        id: r.get(0)?,
        space: r.get(1)?,
        owner: r.get(2)?,
        provider: r.get(3)?,
        account: r.get(4)?,
        agent: r.get(5)?,
        device: r.get(6)?,
        dm: r.get(7)?,
        groups: r.get(8)?,
        state: r.get(9)?,
    })
}
fn find(c: &Context<'_>, q: &Value) -> Result<Binding> {
    c.db.query_row(
        &format!("SELECT {COLS} FROM external_bots WHERE id=? AND space=? AND owner=?"),
        params![short(q, "id")?, c.space, c.actor],
        row,
    )
    .optional()
    .map_err(storage)?
    .ok_or_else(|| "not_found: external binding".into())
}
fn attach(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let id = short(q, "id")?;
    identifier(id)?;
    let provider = required(q, "provider")?;
    if !matches!(provider, "wechat" | "qq" | "feishu" | "lark") {
        return Err(invalid("unknown external bot provider"));
    }
    let account = short(q, "account")?;
    let agent = short(q, "agent")?;
    let key = device(q)?;
    crate::agents::usable(c, "agent", agent, None)?;
    let dm = boolean(q, "dm", true)?;
    let groups = boolean(q, "groups", provider != "wechat")?;
    if provider == "wechat" && groups {
        return Err(invalid("personal WeChat accepts direct conversations"));
    }
    if let Some(old) =
        c.db.query_row(
            &format!("SELECT {COLS} FROM external_bots WHERE id=?"),
            [id],
            row,
        )
        .optional()
        .map_err(storage)?
    {
        if old.space == c.space
            && old.owner == c.actor
            && old.provider == provider
            && old.account == account
            && old.agent == agent
            && old.device == key
            && old.state == "connected"
        {
            return Ok(old.summary());
        }
        return Err(conflict("binding identifier is already in use"));
    }
    if lark::provider(provider) && c.db.query_row("SELECT EXISTS(SELECT 1 FROM external_bots WHERE (space=? AND agent=? OR account=?) AND provider=? AND state='connected')",params![c.space,agent,account,provider],|r|r.get::<_,bool>(0)).map_err(storage)?{return Err(conflict("this Agent or application is already connected to this cloud"));}
    if let Some(old)=c.db.query_row(&format!("SELECT {COLS} FROM external_bots WHERE provider=? AND account=? AND state='connected'"),params![provider,account],row).optional().map_err(storage)?{
        if old.space!=c.space||old.owner!=c.actor||provider=="wechat"&&old.agent!=agent{return Err(conflict("disconnect the existing account binding before rebinding"));}
        deactivate(c,&old,"replaced")?;
    }
    c.db.execute("INSERT INTO external_bots(id,space,owner,provider,account,agent,device,dm,groups_enabled,state) VALUES(?,?,?,?,?,?,?,?,?,'connected')",params![id,c.space,c.actor,provider,account,agent,key,dm,groups]).map_err(storage)?;
    Ok(find(c, q)?.summary())
}
fn deactivate(c: &Context<'_>, b: &Binding, state: &str) -> Result<()> {
    c.db.execute(
        "UPDATE external_bots SET state=? WHERE id=?",
        params![state, b.id],
    )
    .map_err(storage)?;
    deliveries::cancel(c, b, None)
}
fn bindings(c: &Context<'_>, q: &Value) -> Result<Value> {
    let mut st =
        c.db.prepare(&format!(
            "SELECT {COLS} FROM external_bots WHERE space=? AND owner=? ORDER BY rowid DESC"
        ))
        .map_err(storage)?;
    let mut items = vec![];
    for row in st
        .query_map(params![c.space, c.actor], row)
        .map_err(storage)?
    {
        let b = row.map_err(storage)?;
        let mut summary = b.summary();
        if lark::provider(&b.provider) {
            summary["settings"] = lark::settings(c, &b)?;
        }
        summary["deliveries"] = deliveries::summary(c, &b)?;
        items.push(summary);
    }
    let mut data = json!({"workspace":crate::saved_views::workspace(c)?,"actor":c.actor,"space":c.space,"items":items});
    if let Some(cloud) = q.get("available_for").and_then(Value::as_str) {
        if !lark::provider(cloud) {
            return Err(invalid("choose feishu or lark"));
        }
        let mut available = Vec::new();
        if c.require_edit().is_ok() {
            let mut used=c.db.prepare("SELECT agent FROM external_bots WHERE space=? AND provider=? AND state='connected'").map_err(storage)?;
            let used = used
                .query_map(params![c.space, cloud], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<rusqlite::Result<std::collections::HashSet<_>>>()
                .map_err(storage)?;
            for agent in c.records("agent")? {
                if !used.contains(&agent.id)
                    && crate::agents::usable(c, "agent", &agent.id, None).is_ok()
                {
                    available.push(json!({"id":agent.id,"title":agent.title}));
                }
            }
        }
        data["available_agents"] = json!(available);
    }
    Ok(data)
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op == "bot.bindings" {
        return bindings(c, q);
    }
    if op == "bot.attach" {
        return attach(c, q);
    }
    c.require_edit()?;
    let b = find(c, q)?;
    if op == "bot.detach" {
        if b.state == "connected" {
            deactivate(c, &b, "disconnected")?;
        }
        return Ok(find(c, q)?.summary());
    }
    if b.state != "connected" {
        return Err(conflict("external binding is disconnected or expired"));
    }
    if op == "bot.lark.policy" || op == "bot.policy" && lark::provider(&b.provider) {
        return lark::policy(c, &b, q);
    }
    if op == "bot.policy" {
        let dm = boolean(q, "dm", b.dm)?;
        let groups = boolean(q, "groups", b.groups)?;
        if b.provider == "wechat" && groups {
            return Err(invalid("personal WeChat has no group events"));
        }
        for (enabled, scene) in [(dm, "direct"), (groups, "group")] {
            if !enabled {
                deliveries::cancel(c, &b, Some(scene))?;
            }
        }
        c.db.execute(
            "UPDATE external_bots SET dm=?,groups_enabled=? WHERE id=?",
            params![dm, groups, b.id],
        )
        .map_err(storage)?;
        return Ok(find(c, q)?.summary());
    }
    if op == "bot.retry" {
        return deliveries::retry(c, &b, q);
    }
    if device(q)? != b.device {
        return Err(forbidden("adapter key does not match this binding"));
    }
    if op == "bot.expire" {
        deactivate(c, &b, "expired")?;
        return Ok(find(c, q)?.summary());
    }
    if matches!(op, "bot.receive" | "bot.claim" | "bot.lark.event") {
        crate::agents::usable(c, "agent", &b.agent, None)?;
    }
    match op {
        "bot.lark.event" => lark::event(c, &b, q),
        "bot.receive" if lark::provider(&b.provider) => {
            Err(invalid("use scoped Feishu/Lark event ingestion"))
        }
        "bot.receive" => turns::receive(c, &b, q),
        "bot.claim" => deliveries::claim(c, &b),
        "bot.ack" => deliveries::ack(c, &b, q),
        _ => Err(invalid("unknown external bot operation")),
    }
}
pub(crate) fn schema(s: &mut Value) {
    lark::schema(s);
    let rules = s["allOf"].as_array_mut().unwrap();
    for (op, req) in [
        (
            "bot.attach",
            vec!["id", "provider", "account", "agent", "device_key"],
        ),
        ("bot.policy", vec!["id"]),
        ("bot.detach", vec!["id"]),
        ("bot.expire", vec!["id", "device_key"]),
        (
            "bot.receive",
            vec![
                "id",
                "device_key",
                "scene",
                "peer",
                "sender",
                "remote",
                "body",
            ],
        ),
        ("bot.claim", vec!["id", "device_key"]),
        (
            "bot.ack",
            vec!["id", "device_key", "delivery", "lease", "outcome"],
        ),
        ("bot.retry", vec!["id", "delivery", "decision"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":req,"properties":{"dm":{"type":"boolean"},"groups":{"type":"boolean"},"provider":{"enum":["qq","wechat","feishu","lark"]},"device_key":{"type":"string","minLength":32,"maxLength":256},"scene":{"enum":["direct","group"]},"outcome":{"enum":["sent","retry","uncertain","failed"]},"decision":{"enum":["resend","confirm","discard"]}}}}));
    }
}
