//! Transactional email queue. The server owns the transport and the only delivery lease.
use crate::{
    Context, Record, Result, Store, forbidden, identifier, invalid, json, now, required, storage,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use uuid::Uuid;

const RETRY_WINDOW: i64 = 23 * 60 * 60 * 1000;
const NOTIFICATION_AGE: i64 = 24 * 60 * 60 * 1000;
pub(crate) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS delivery_mail(
        id TEXT PRIMARY KEY, org TEXT NOT NULL, space TEXT NOT NULL, actor TEXT NOT NULL,
        address TEXT NOT NULL, language TEXT NOT NULL, event TEXT NOT NULL,
        kind TEXT NOT NULL, target TEXT NOT NULL, source_kind TEXT, source_id TEXT,
        state TEXT NOT NULL DEFAULT 'queued', created INTEGER NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0, first_attempt INTEGER NOT NULL DEFAULT 0,
        due INTEGER NOT NULL DEFAULT 0, lease TEXT NOT NULL DEFAULT '', receipt TEXT);
        CREATE INDEX IF NOT EXISTS delivery_due ON delivery_mail(state,due);
        CREATE INDEX IF NOT EXISTS delivery_actor ON delivery_mail(org,actor,created);
        CREATE UNIQUE INDEX IF NOT EXISTS delivery_invitation ON delivery_mail(org,target) WHERE kind='invitation';").map_err(storage)
}
pub(crate) fn invitation(c: &Context<'_>, row: &Record) -> Result<()> {
    c.db.execute(
        "INSERT INTO delivery_mail(id,org,space,actor,address,language,event,kind,target,created)
        VALUES(?1,?2,?2,?3,?4,?5,'invitation','invitation',?6,?7)",
        params![
            Uuid::new_v4().to_string(),
            c.space,
            c.actor,
            required(&row.fields, "email")?,
            required(&row.fields, "language")?,
            row.id,
            now()
        ],
    )
    .map_err(storage)?;
    Ok(())
}
pub(crate) fn invitation_status(c: &Context<'_>, target: &str) -> Result<Value> {
    c.db.query_row("SELECT id,state,attempts FROM delivery_mail WHERE org=? AND kind='invitation' AND target=?",params![c.space,target],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"attempts":r.get::<_,i64>(2)?}))).optional().map_err(storage).map(|row|row.unwrap_or(Value::Null))
}
pub(crate) fn cancel_invitation(c: &Context<'_>, target: &str) -> Result<()> {
    c.db.execute("UPDATE delivery_mail SET state='canceled',lease='' WHERE org=? AND kind='invitation' AND target=? AND state IN ('queued','sending','retry','failed')",params![c.space,target]).map_err(storage)?;
    Ok(())
}
pub(crate) fn notification(
    c: &Context<'_>,
    recipient: &str,
    event: &str,
    target: (&str, &str),
    source: Option<(&str, &str)>,
) -> Result<()> {
    let email: Option<String> =
        c.db.query_row(
            "SELECT email FROM account_profiles WHERE actor=?",
            [recipient],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if let Some(email) = email {
        let (language, _) = crate::organization::defaults(c)?;
        c.db.execute("INSERT INTO delivery_mail(id,org,space,actor,address,language,event,kind,target,source_kind,source_id,created)
            VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",params![Uuid::new_v4().to_string(),c.org_id()?,c.space,recipient,email,language,event,target.0,target.1,source.map(|s|s.0),source.map(|s|s.1),now()]).map_err(storage)?;
    }
    Ok(())
}

pub struct MailDispatch {
    pub id: String,
    pub recipient: String,
    pub language: String,
    pub invitation: bool,
    pub link: String,
    lease: String,
}
pub enum MailResult {
    Accepted(String),
    Retry { after_seconds: u64 },
    Failed,
}
struct Item {
    id: String,
    org: String,
    space: String,
    actor: String,
    address: String,
    language: String,
    event: String,
    kind: String,
    target: String,
    source: Option<(String, String)>,
    state: String,
    attempts: u32,
    first_attempt: i64,
    created: i64,
}
const COLUMNS: &str = "id,org,space,actor,address,language,event,kind,target,source_kind,source_id,state,attempts,first_attempt,created";
fn item(r: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        org: r.get(1)?,
        space: r.get(2)?,
        actor: r.get(3)?,
        address: r.get(4)?,
        language: r.get(5)?,
        event: r.get(6)?,
        kind: r.get(7)?,
        target: r.get(8)?,
        source: r.get::<_, Option<String>>(9)?.zip(r.get(10)?),
        state: r.get(11)?,
        attempts: r.get(12)?,
        first_attempt: r.get(13)?,
        created: r.get(14)?,
    })
}
impl Item {
    fn expired_retry(&self) -> bool {
        self.first_attempt > 0 && now() - self.first_attempt >= RETRY_WINDOW
    }
    fn brief(&self) -> Value {
        json!({"id":self.id,"space":self.space,"event":self.event,"kind":self.kind,"target":self.target,"state":self.state,"attempts":self.attempts,"created":self.created})
    }
    fn target(&self, c: &Context<'_>) -> Result<Record> {
        c.role()?;
        if let Some((k, id)) = &self.source {
            c.readable(k, id)?;
        }
        c.readable(&self.kind, &self.target)
    }
    fn deep_link(&self, db: &Connection) -> Result<String> {
        let c = Context {
            db,
            actor: &self.actor,
            space: &self.space,
        };
        if self.kind == "invitation" {
            return crate::invitations::mail_link(&c, &self.target);
        }
        if now() - self.created > NOTIFICATION_AGE
            || !crate::identity::verified_address(&c, &self.address)?
            || !crate::inbox::email_enabled(&c, &self.actor, &self.event)?
        {
            return Err(forbidden("email notification is unavailable"));
        }
        self.target(&c)?;
        Ok(format!(
            "shipforge://activity/{}/{}/{}",
            crate::saved_views::workspace(&c)?,
            URL_SAFE_NO_PAD.encode(&self.space),
            URL_SAFE_NO_PAD.encode(&self.id)
        ))
    }
}
pub fn delivery_link_parts(link: &str) -> Result<(String, String, String)> {
    crate::saved_views::resource_link_parts(link, "shipforge://activity/")
}
fn get(db: &Connection, id: &str) -> Result<Item> {
    db.query_row(
        &format!("SELECT {COLUMNS} FROM delivery_mail WHERE id=?"),
        [id],
        item,
    )
    .optional()
    .map_err(storage)?
    .ok_or_else(|| forbidden("delivery is unavailable"))
}
pub(crate) fn command(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op == "delivery.history" {
        let invitations = q["invitations"] == true;
        let org = c.org_id()?;
        let root = Context { space: &org, ..*c };
        root.role()?;
        if invitations {
            root.require_admin()?;
        }
        let query = if invitations {
            "kind='invitation'"
        } else {
            "kind<>'invitation' AND actor=?2"
        };
        let sql = format!(
            "SELECT {COLUMNS} FROM delivery_mail WHERE org=?1 AND {query} ORDER BY created DESC,id"
        );
        let mut stmt = c.db.prepare(&sql).map_err(storage)?;
        let limit = q["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;
        let mut skip = q["offset"].as_u64().unwrap_or(0);
        let mut rows = if invitations {
            stmt.query([&org])
        } else {
            stmt.query(params![org, c.actor])
        }
        .map_err(storage)?;
        let mut visible = Vec::new();
        while let Some(result) = rows.next().map_err(storage)? {
            let row = item(result).map_err(storage)?;
            let scope = Context {
                space: &row.space,
                ..*c
            };
            if invitations || row.target(&scope).is_ok() {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                visible.push(row.brief());
                if visible.len() == limit {
                    break;
                }
            }
        }
        return Ok(json!(visible));
    }
    if op == "delivery.open" {
        let (origin, space, id) = delivery_link_parts(required(q, "link")?)?;
        if origin != crate::saved_views::workspace(c)? {
            return Err(forbidden("connect to the workspace that sent this email"));
        }
        let row = get(c.db, &id)?;
        if row.kind == "invitation" || row.actor != c.actor || row.space != space {
            return Err(forbidden("delivery is unavailable"));
        }
        let scope = Context {
            space: &space,
            ..*c
        };
        return Ok(
            json!({"space":space,"target":row.target(&scope)?,"comment":row.source.filter(|(kind,_)|kind=="comment").map(|(_,id)|id)}),
        );
    }
    let id = required(q, "id")?;
    identifier(id)?;
    let row = get(c.db, id)?;
    if row.org != c.org_id()? {
        return Err(forbidden("delivery is unavailable"));
    }
    let scope = Context {
        space: &row.space,
        ..*c
    };
    if row.kind == "invitation" {
        scope.require_admin()?;
    } else if c.actor == row.actor {
        row.target(&scope)?;
    } else {
        return Err(forbidden("delivery is unavailable"));
    }
    if row.state != "failed" || row.expired_retry() {
        return Err(crate::conflict(
            "only recently rejected deliveries can be retried",
        ));
    }
    row.deep_link(c.db)?;
    c.db.execute(
        "UPDATE delivery_mail SET state='queued',due=0 WHERE id=?",
        [id],
    )
    .map_err(storage)?;
    Ok(get(c.db, id)?.brief())
}

impl Store {
    /// Claims one delivery after checking the current permissions, recipient and preferences.
    pub fn claim_email(&mut self) -> Result<Option<MailDispatch>> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let mut dispatch = None;
        for _ in 0..64 {
            let query = format!(
                "SELECT {COLUMNS} FROM delivery_mail WHERE state IN ('queued','retry','sending') AND due<=? ORDER BY due,created,id LIMIT 1"
            );
            let Some(row) = tx
                .query_row(&query, [now()], item)
                .optional()
                .map_err(storage)?
            else {
                break;
            };
            if row.expired_retry() {
                tx.execute(
                    "UPDATE delivery_mail SET state='unconfirmed',lease='' WHERE id=?",
                    [&row.id],
                )
                .map_err(storage)?;
                continue;
            }
            let link = match row.deep_link(&tx) {
                Ok(link) => link,
                Err(e) if e.starts_with("storage:") => return Err(e),
                Err(_) => {
                    tx.execute(
                        "UPDATE delivery_mail SET state='canceled',lease='' WHERE id=?",
                        [&row.id],
                    )
                    .map_err(storage)?;
                    continue;
                }
            };
            let lease = Uuid::new_v4().to_string();
            let at = now();
            tx.execute("UPDATE delivery_mail SET state='sending',attempts=attempts+1,first_attempt=CASE first_attempt WHEN 0 THEN ?1 ELSE first_attempt END,due=?2,lease=?3 WHERE id=?4",params![at,at+60_000,lease,row.id]).map_err(storage)?;
            dispatch = Some(MailDispatch {
                id: row.id,
                lease,
                recipient: row.address,
                language: row.language,
                invitation: row.kind == "invitation",
                link,
            });
            break;
        }
        tx.commit().map_err(storage)?;
        Ok(dispatch)
    }
    pub fn finish_email(&mut self, job: &MailDispatch, result: MailResult) -> Result<()> {
        let (state, delay, receipt) = match result {
            MailResult::Accepted(receipt) if !receipt.is_empty() && receipt.len() <= 256 => {
                ("accepted", 0, Some(receipt))
            }
            MailResult::Accepted(_) => return Err(invalid("expected a mail provider receipt")),
            MailResult::Retry { after_seconds } => {
                ("retry", after_seconds.clamp(1, 24 * 3600) as i64, None)
            }
            MailResult::Failed => ("failed", 0, None),
        };
        self.db.execute("UPDATE delivery_mail SET state=?1,due=?2+CASE WHEN ?1='retry' THEN MAX(?3,MIN(3600000,1000*(1 << MIN(12,attempts-1)))) ELSE 0 END,receipt=?4,lease='' WHERE id=?5 AND state='sending' AND lease=?6",
            params![state,now(),delay*1000,receipt,job.id,job.lease]).map_err(storage)?;
        Ok(())
    }
}
