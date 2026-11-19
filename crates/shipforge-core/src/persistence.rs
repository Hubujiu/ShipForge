use super::*;
use rusqlite::{Row, params};
use std::time::Duration;
use uuid::Uuid;

const APP_ID: i64 = 0x53465753;
const V1:&str = "
CREATE TABLE IF NOT EXISTS records(
 space TEXT NOT NULL, kind TEXT NOT NULL, id TEXT NOT NULL,
 title TEXT NOT NULL, fields TEXT NOT NULL, rev INTEGER NOT NULL,
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 PRIMARY KEY(space,kind,id));
CREATE INDEX IF NOT EXISTS record_order ON records(space,kind,updated_at DESC,id);
CREATE UNIQUE INDEX IF NOT EXISTS project_identity ON records(json_extract(fields,'$.organization'),id) WHERE kind='project';
CREATE UNIQUE INDEX IF NOT EXISTS external_ref ON records(space,kind,json_extract(fields,'$.connector'),json_extract(fields,'$.external_id')) WHERE json_extract(fields,'$.external_id') IS NOT NULL AND json_extract(fields,'$.connector') IS NOT NULL;
CREATE TABLE IF NOT EXISTS receipts(space TEXT NOT NULL, actor TEXT NOT NULL, key TEXT NOT NULL, request TEXT NOT NULL, result TEXT NOT NULL, PRIMARY KEY(space,actor,key));
CREATE TABLE IF NOT EXISTS event_log(seq INTEGER PRIMARY KEY AUTOINCREMENT,space TEXT NOT NULL,kind TEXT NOT NULL,id TEXT NOT NULL,rev INTEGER NOT NULL,action TEXT NOT NULL,actor TEXT NOT NULL,at INTEGER NOT NULL,snapshot TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS events_by_space ON event_log(space,seq);
CREATE INDEX IF NOT EXISTS record_history ON event_log(space,kind,id,seq);
CREATE TABLE IF NOT EXISTS versions(space TEXT NOT NULL,id TEXT NOT NULL,rev INTEGER NOT NULL,title TEXT NOT NULL,fields TEXT NOT NULL,actor TEXT NOT NULL,at INTEGER NOT NULL,PRIMARY KEY(space,id,rev));
CREATE TABLE IF NOT EXISTS sequences(space TEXT PRIMARY KEY,n INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS space_defaults(space TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS personal_preferences(actor TEXT PRIMARY KEY, rev INTEGER NOT NULL, settings TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS discussion_calls(space TEXT NOT NULL,run TEXT NOT NULL,call_id TEXT NOT NULL,payload BLOB NOT NULL,comment TEXT NOT NULL,PRIMARY KEY(space,run,call_id));
CREATE TABLE IF NOT EXISTS discussion_authors(space TEXT NOT NULL,comment TEXT NOT NULL,agent TEXT NOT NULL,PRIMARY KEY(space,comment));
CREATE TABLE IF NOT EXISTS comment_reactions(space TEXT NOT NULL,comment TEXT NOT NULL,actor TEXT NOT NULL,emoji TEXT NOT NULL,PRIMARY KEY(space,comment,actor,emoji));
CREATE TRIGGER IF NOT EXISTS remove_comment_reactions AFTER DELETE ON records WHEN OLD.kind='comment' BEGIN DELETE FROM comment_reactions WHERE space=OLD.space AND comment=OLD.id; END;
CREATE TABLE IF NOT EXISTS task_conversations(space TEXT NOT NULL, actor TEXT NOT NULL, task TEXT NOT NULL, kind TEXT NOT NULL, handler TEXT NOT NULL, chat TEXT NOT NULL, PRIMARY KEY(space,actor,task,kind,handler));
CREATE TABLE IF NOT EXISTS attachments(space TEXT NOT NULL,id TEXT NOT NULL,content BLOB NOT NULL,PRIMARY KEY(space,id));
CREATE TABLE IF NOT EXISTS followers(space TEXT NOT NULL,task TEXT NOT NULL,actor TEXT NOT NULL,PRIMARY KEY(space,task,actor));
";

pub(crate) fn initialize(db: &mut Connection) -> Result<()> {
    db.busy_timeout(Duration::from_secs(15)).map_err(storage)?;
    let (app,version,tables):(i64,i64,i64)=db.query_row(
        "SELECT (SELECT application_id FROM pragma_application_id), (SELECT user_version FROM pragma_user_version), count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))
    ).map_err(storage)?;
    if (app != APP_ID && (app != 0 || tables != 0)) || version > 1 {
        return Err(invalid(
            "unsupported database format; open a ShipForge workspace database",
        ));
    }
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(storage)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    tx.execute_batch(V1).map_err(storage)?;
    crate::statuses::init(&tx)?;
    crate::identity::initialize(&tx)?;
    crate::organization::migrate_owners(&tx)?;
    crate::spaces::init(&tx)?;
    crate::saved_views::init(&tx)?;
    crate::inbox::init(&tx)?;
    crate::triage::init(&tx)?;
    crate::bots::init(&tx)?;
    crate::conversation_tools::init(&tx)?;
    tx.pragma_update(None, "application_id", APP_ID)
        .map_err(storage)?;
    tx.pragma_update(None, "user_version", 1).map_err(storage)?;
    crate::worker::init(&tx)?;
    crate::chat::init(&tx)?;
    crate::iterations::init(&tx)?;
    crate::onboarding::initialize(&tx)?;
    crate::invitations::initialize(&tx)?;
    crate::mail::initialize(&tx)?;
    if tables == 0 {
        tx.execute(
            "INSERT OR IGNORE INTO guide_setup(space) VALUES('default')",
            [],
        )
        .map_err(storage)?;
    }
    crate::im::init(&tx)?;
    crate::vcs::init(&tx)?;
    crate::vendors::init(&tx)?;
    let c = Context {
        db: &tx,
        actor: "owner",
        space: "default",
    };
    if c.find("space", "default")?.is_none() {
        c.create(
            "space",
            Some("default"),
            "默认空间",
            json!({"creator":"owner","key":"SF","visibility":"private","triage_enabled":false,"organization":"default"}),
        )?;
        c.create(
            "member",
            Some("owner"),
            "我",
            json!({"actor":"owner","role":"owner","creator":"owner"}),
        )?;
    }
    if c.find("organization", "default")?.is_none() {
        c.create(
            "organization",
            Some("default"),
            "我的组织",
            json!({"creator":"owner","visibility":"space"}),
        )?;
    }
    let mut unseeded = tx.prepare("SELECT id FROM records WHERE kind='space' AND id NOT IN (SELECT space FROM space_defaults)").map_err(storage)?;
    let spaces = unseeded
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    drop(unseeded);
    for space in spaces {
        crate::defaults::seed(&Context {
            db: &tx,
            actor: "owner",
            space: &space,
        })?;
    }
    crate::onboarding::backfill(&tx)?;
    crate::tags::upgrade(&tx)?;
    tx.commit().map_err(storage)
}

const COLUMNS: &str = "space,kind,id,title,fields,rev,created_at,updated_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Record> {
    let text: String = r.get(4)?;
    let mut fields = serde_json::from_str(&text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let kind: String = r.get(1)?;
    if kind == "task" {
        crate::task_commands::assignees::upgrade(&mut fields);
    }
    Ok(Record {
        space: r.get(0)?,
        kind,
        id: r.get(2)?,
        title: r.get(3)?,
        fields,
        rev: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

impl Context<'_> {
    pub(crate) fn own_notifications(&self) -> Result<Vec<Record>> {
        let mut stmt=self.db.prepare(&format!("SELECT {COLUMNS} FROM records WHERE space=?1 AND kind='notification' AND json_extract(fields,'$.recipient')=?2 ORDER BY updated_at DESC,id")).map_err(storage)?;
        stmt.query_map(params![self.space, self.actor], from_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }
    pub(crate) fn find(&self, kind: &str, id: &str) -> Result<Option<Record>> {
        self.db
            .query_row(
                &format!("SELECT {COLUMNS} FROM records WHERE space=?1 AND kind=?2 AND id=?3"),
                params![self.space, kind, id],
                from_row,
            )
            .optional()
            .map_err(storage)
    }
    pub(crate) fn get(&self, kind: &str, id: &str) -> Result<Record> {
        self.find(kind, id)?
            .ok_or_else(|| format!("not_found: {kind} {id}"))
    }
    pub(crate) fn readable(&self, kind: &str, id: &str) -> Result<Record> {
        let r = self.get(kind, id)?;
        if self.visible(&r)? {
            Ok(r)
        } else {
            Err(format!("not_found: {kind} {id}"))
        }
    }
    pub(crate) fn records(&self, kind: &str) -> Result<Vec<Record>> {
        let mut stmt=self.db.prepare(&format!("SELECT {COLUMNS} FROM records WHERE space=?1 AND kind=?2 ORDER BY updated_at DESC,id")).map_err(storage)?;
        stmt.query_map(params![self.space, kind], from_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)
    }
    pub(crate) fn all_spaces(&self) -> Result<Vec<Record>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {COLUMNS} FROM records WHERE kind='space' ORDER BY title,id"
            ))
            .map_err(storage)?;
        stmt.query_map([], from_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)
    }
    pub(crate) fn create(
        &self,
        kind: &str,
        id: Option<&str>,
        title: &str,
        fields: Value,
    ) -> Result<Record> {
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let at = now();
        let rev = self.first_revision(kind, &id)?;
        let mut row = Record {
            space: self.space.to_owned(),
            kind: kind.to_owned(),
            id,
            title: title.to_owned(),
            fields,
            rev,
            created_at: at,
            updated_at: at,
        };
        if kind == "task" && row.fields.get("iteration").is_none() {
            crate::iterations::auto_assign(self, None, &mut row)?;
        }
        self.insert(&row)?;
        Ok(row)
    }
    pub(crate) fn first_revision(&self, kind: &str, id: &str) -> Result<i64> {
        let retired:Option<i64>=self.db.query_row("SELECT rev FROM event_log WHERE space=?1 AND kind=?2 AND id=?3 AND action='delete' ORDER BY seq DESC LIMIT 1", params![self.space,kind,id],|r|r.get(0)).optional().map_err(storage)?;
        match retired {
            Some(last) if kind == "member" => Ok(last + 1),
            Some(_) => Err(conflict("record identifier has been retired")),
            None => Ok(1),
        }
    }
    pub(crate) fn insert(&self, row: &Record) -> Result<()> {
        if row.kind != "space" {
            crate::spaces::ensure_active(self)?;
        }
        let data = row.fields.to_string();
        self.db.execute("INSERT INTO records(space,kind,id,title,fields,rev,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![row.space,row.kind,row.id,row.title,data,row.rev,row.created_at,row.updated_at]).map_err(store_constraint)?;
        self.event(row, "create")?;
        if row.kind == "message" {
            crate::chat::new_message(self, row)?;
        }
        crate::onboarding::observe(self, row, None)
    }
    pub(crate) fn update(&self, old: &Record, title: &str, fields: Value) -> Result<Record> {
        if old.kind == "task" && fields["iteration"].is_null() && old.fields["iteration"].is_null()
        {
            let mut row = old.clone();
            row.fields = fields;
            crate::iterations::auto_assign(self, Some(old), &mut row)?;
            return self.update_mode(old, title, row.fields, true);
        }
        self.update_mode(old, title, fields, true)
    }
    pub(crate) fn update_mode(
        &self,
        old: &Record,
        title: &str,
        fields: Value,
        notify: bool,
    ) -> Result<Record> {
        let mut row = old.clone();
        if !(old.kind == "space"
            && old.fields["deactivated"] == true
            && fields["deactivated"] == false)
        {
            crate::spaces::ensure_active(self)?;
        }
        row.title = title.to_owned();
        row.fields = fields;
        row.rev = old.rev + 1;
        row.updated_at = now();
        let n=self.db.execute("UPDATE records SET title=?1,fields=?2,rev=?3,updated_at=?4 WHERE space=?5 AND kind=?6 AND id=?7 AND rev=?8",params![row.title,row.fields.to_string(),row.rev,row.updated_at,self.space,row.kind,row.id,old.rev]).map_err(store_constraint)?;
        if n != 1 {
            return Err(conflict("record revision changed"));
        }
        self.event(&row, "update")?;
        crate::onboarding::observe(self, &row, Some(old))?;
        crate::vendors::changed(self, old, &row)?;
        if notify || row.fields["visibility"] == "private" || row.kind == "connector" {
            crate::im::changed(self, Some(old), &row)?;
        }
        Ok(row)
    }
    pub(crate) fn remove(&self, old: &Record) -> Result<()> {
        if old.kind == "comment" {
            self.db
                .execute(
                    "DELETE FROM discussion_authors WHERE space=? AND comment=?",
                    params![self.space, old.id],
                )
                .map_err(storage)?;
        }
        crate::vendors::removing(self, old)?;
        if old.kind != "member" {
            crate::spaces::ensure_active(self)?;
        }
        let n = self
            .db
            .execute(
                "DELETE FROM records WHERE space=?1 AND kind=?2 AND id=?3 AND rev=?4",
                params![self.space, old.kind, old.id, old.rev],
            )
            .map_err(storage)?;
        if n != 1 {
            return Err(conflict("record revision changed"));
        }
        let mut tombstone = old.clone();
        if old.kind == "member" {
            crate::chat::forget_grants(self, &old.id)?;
            crate::agents::forget(self, &old.id)?;
        }
        tombstone.rev += 1;
        tombstone.updated_at = now();
        self.event(&tombstone, "delete")?;
        crate::im::removed(self, old)?;
        if old.kind == "document" || old.fields["attachments"].is_array() {
            crate::attachments::prune(self)?;
        }
        Ok(())
    }
    pub(super) fn event(&self, row: &Record, action: &str) -> Result<()> {
        self.db.execute("INSERT INTO event_log(space,kind,id,rev,action,actor,at,snapshot) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![self.space,row.kind,row.id,row.rev,action,self.actor,row.updated_at,serde_json::to_string(row).map_err(storage)?]).map_err(storage)?;
        if row.kind == "document" && action != "delete" {
            self.db.execute("INSERT INTO versions(space,id,rev,title,fields,actor,at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![self.space,row.id,row.rev,row.title,row.fields.to_string(),self.actor,row.updated_at]).map_err(storage)?;
        }
        Ok(())
    }
    pub(crate) fn task_number(&self) -> Result<String> {
        let seq:i64=self.db.query_row("INSERT INTO sequences(space,n) VALUES(?1,1) ON CONFLICT(space) DO UPDATE SET n=n+1 RETURNING n",[self.space],|r|r.get(0)).map_err(storage)?;
        let space = self.get("space", self.space)?;
        Ok(format!(
            "{}-{seq}",
            space.fields["key"].as_str().unwrap_or("SF")
        ))
    }
}
fn store_constraint(e: rusqlite::Error) -> String {
    if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
        conflict("identifier or external reference already exists")
    } else {
        storage(e)
    }
}
