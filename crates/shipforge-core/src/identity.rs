use super::*;
use uuid::Uuid;

pub(crate) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS account_profiles(actor TEXT PRIMARY KEY,email TEXT UNIQUE NOT NULL, name TEXT NOT NULL,avatar TEXT NOT NULL DEFAULT '',bio TEXT NOT NULL DEFAULT '',rev INTEGER NOT NULL DEFAULT 1);
        CREATE TABLE IF NOT EXISTS account_retirements(actor TEXT PRIMARY KEY, retired INTEGER NOT NULL);").map_err(storage)
}

pub(crate) fn ensure_active(db: &Connection, actor: &str) -> Result<()> {
    if db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM account_retirements WHERE actor=?)",
            [actor],
            |r| r.get::<_, bool>(0),
        )
        .map_err(storage)?
    {
        return Err("unauthorized: account has been closed".into());
    }
    Ok(())
}
pub(crate) fn verified_address(c: &Context<'_>, email: &str) -> Result<bool> {
    c.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM account_profiles WHERE actor=?1 AND email=?2 COLLATE NOCASE)",
        params![c.actor, email],
        |r| r.get(0),
    )
    .map_err(storage)
}
fn profile(db: &Connection, actor: &str) -> Result<Value> {
    db.query_row("SELECT email,name,avatar,bio,rev FROM account_profiles WHERE actor=?",[actor],|r|Ok(json!({"actor":actor,"email":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"avatar":r.get::<_,String>(2)?,"bio":r.get::<_,String>(3)?,"rev":r.get::<_,i64>(4)?}))).optional().map_err(storage)?.ok_or_else(||invalid("this operation requires an email-verified account"))
}
impl Store {
    pub fn has_verified_profile(&self, actor: &str) -> Result<bool> {
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_profiles WHERE actor=?)",
                [actor],
                |r| r.get(0),
            )
            .map_err(storage)
    }
    pub fn verified_actor(&self, email: &str) -> Result<Option<String>> {
        self.db
            .query_row(
                "SELECT actor FROM account_profiles WHERE email=?",
                [email],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)
    }
    /// The authentication adapter calls this only after verifying control of the mailbox.
    pub fn verified_login(&mut self, email: &str) -> Result<Value> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let existing = tx
            .query_row(
                "SELECT actor FROM account_profiles WHERE email=?",
                [email],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(storage)?;
        let actor = if let Some(actor) = existing {
            actor
        } else {
            let actor = format!("acct-{}", Uuid::new_v4());
            let home = format!("org-{}", Uuid::new_v4());
            let c = Context {
                db: &tx,
                actor: &actor,
                space: &home,
            };
            let name = "新成员";
            tx.execute(
                "INSERT INTO account_profiles(actor,email,name) VALUES(?,?,?)",
                params![actor, email, name],
            )
            .map_err(storage)?;
            c.create("space",Some(&home),"我的空间",json!({"organization":home,"creator":actor,"visibility":"private","triage_enabled":false}))?;
            c.create(
                "organization",
                Some(&home),
                "我的组织",
                json!({"creator":actor,"visibility":"space"}),
            )?;
            c.create(
                "member",
                Some(&actor),
                name,
                json!({"creator":actor,"actor":actor,"role":"owner"}),
            )?;
            crate::defaults::seed(&c)?;
            tx.execute("INSERT INTO guide_setup(space) VALUES(?)", [&home])
                .map_err(storage)?;
            actor
        };
        let value = profile(&tx, &actor)?;
        tx.commit().map_err(storage)?;
        Ok(value)
    }
    pub fn account_profile(&self, actor: &str) -> Result<Value> {
        profile(&self.db, actor)
    }
    pub fn update_account_profile(&mut self, actor: &str, q: &Value) -> Result<Value> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let old = profile(&tx, actor)?;
        if q["expected_rev"].as_i64() != old["rev"].as_i64() {
            return Err(conflict("profile changed; reopen before saving"));
        }
        let val = |key: &str| {
            q.get(key)
                .unwrap_or(&old[key])
                .as_str()
                .ok_or_else(|| invalid("profile fields must be text"))
        };
        let (name, avatar, bio) = (val("name")?.trim(), val("avatar")?.trim(), val("bio")?);
        if name.is_empty()
            || name.chars().count() > 100
            || name.chars().any(char::is_control)
            || bio.chars().count() > 1000
            || avatar.len() > 2048
            || (!avatar.is_empty()
                && !url::Url::parse(avatar).is_ok_and(|u| {
                    u.scheme() == "https"
                        && u.host().is_some()
                        && u.username().is_empty()
                        && u.password().is_none()
                }))
        {
            return Err(invalid(
                "check display name, HTTPS avatar URL and biography length",
            ));
        }
        tx.execute(
            "UPDATE account_profiles SET name=?,avatar=?,bio=?,rev=rev+1 WHERE actor=?",
            params![name, avatar, bio, actor],
        )
        .map_err(storage)?;
        let mut stmt = tx
            .prepare("SELECT space FROM records WHERE kind='member' AND id=?")
            .map_err(storage)?;
        let spaces = stmt
            .query_map([actor], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        drop(stmt);
        for space in spaces {
            let c = Context {
                db: &tx,
                actor,
                space: &space,
            };
            let member = c.get("member", actor)?;
            if member.title != name {
                c.update(&member, name, member.fields.clone())?;
            }
        }
        let result = profile(&tx, actor)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
    pub fn account_departure(&self, actor: &str) -> Result<Value> {
        profile(&self.db, actor)?;
        impact(&self.db, actor)
    }
    /// This operation is restricted to the service after a fresh, purpose-bound mailbox challenge.
    pub fn retire_verified_account(&mut self, actor: &str, dissolve: bool) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_retirements WHERE actor=?)",
                [actor],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage)?
        {
            return Ok(());
        }
        profile(&tx, actor)?;
        let preview = impact(&tx, actor)?;
        if !preview["spaces_to_transfer"].as_array().unwrap().is_empty() {
            return Err(conflict(
                "transfer your spaces in surviving organizations before closing the account",
            ));
        }
        if !preview["shared_machines"].as_array().unwrap().is_empty() {
            return Err(conflict("release access to shared machines first"));
        }
        for org in preview["organizations"].as_array().unwrap() {
            if org["action"] == "dissolve_with_members" && !dissolve {
                return Err(conflict(
                    "confirm dissolution of organizations with other members",
                ));
            }
            if org["action"] != "leave" {
                purge_organization(&tx, org["id"].as_str().unwrap())?;
            }
        }
        tx.execute("DELETE FROM records WHERE (kind='member' AND id=?1) OR (kind='notification' AND json_extract(fields,'$.recipient')=?1) OR (kind='machine' AND json_extract(fields,'$.owner')=?1)",[actor]).map_err(storage)?;
        tx.execute(
            "DELETE FROM event_log WHERE kind='member' AND id=?",
            [actor],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM sf_machine_keys WHERE owner=?", [actor])
            .map_err(storage)?;
        for table in [
            "personal_preferences",
            "agent_personal",
            "agent_grants",
            "followers",
            "inbox_settings",
            "delivery_mail",
            "inbox_signals",
            "desktop_notifications",
            "comment_reactions",
            "guide_display",
            "guide_credits",
            "receipts",
            "task_conversations",
            "invitation_uses",
            "invitation_requests",
        ] {
            tx.execute(&format!("DELETE FROM {table} WHERE actor=?"), [actor])
                .map_err(storage)?;
        }
        tx.execute("DELETE FROM account_profiles WHERE actor=?", [actor])
            .map_err(storage)?;
        tx.execute(
            "INSERT INTO account_retirements VALUES(?,unixepoch())",
            [actor],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)
    }
}

fn impact(db: &Connection, actor: &str) -> Result<Value> {
    let mut stmt=db.prepare("SELECT o.id,o.title,(json_extract(mine.fields,'$.role')='owner' AND NOT EXISTS(SELECT 1 FROM records other WHERE other.kind='member' AND other.space=o.id AND other.id<>?1 AND json_extract(other.fields,'$.role')='owner')),
      (SELECT count(*) FROM records m WHERE m.kind='member' AND m.space=o.id)
      FROM records o JOIN records mine ON mine.space=o.id AND mine.kind='member' AND mine.id=?1 WHERE o.kind='organization' ORDER BY o.id").map_err(storage)?;
    let organizations=stmt.query_map([actor],|r|{
        let owner:bool=r.get(2)?;let members:i64=r.get(3)?;
        let action=match (owner,members){(true,1)=>"dissolve_solo",(true,_)=>"dissolve_with_members",_=>"leave"};
        Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"members":members,"action":action}))
    }).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
    let mut stmt=db.prepare("SELECT m.space,m.id,m.title FROM records m JOIN records member ON member.space=m.space AND member.kind='member' AND member.id=?1 WHERE m.kind='machine' AND json_extract(m.fields,'$.shared')=1 AND COALESCE(json_extract(m.fields,'$.disabled'),0)=0 ORDER BY m.space,m.id").map_err(storage)?;
    let machines=stmt.query_map([actor],|r|Ok(json!({"space":r.get::<_,String>(0)?,"id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
    let mut spaces_to_transfer = Vec::new();
    let mut children = db.prepare("SELECT id,title FROM records WHERE kind='space' AND id<>?1 AND json_extract(fields,'$.organization')=?1 AND json_extract(fields,'$.creator')=?2 ORDER BY id").map_err(storage)?;
    for org in organizations.iter().filter(|row| row["action"] == "leave") {
        let id = org["id"].as_str().unwrap();
        let rows=children.query_map(params![id,actor],|r|Ok(json!({"organization":id,"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?}))).map_err(storage)?;
        for row in rows {
            spaces_to_transfer.push(row.map_err(storage)?);
        }
    }
    Ok(
        json!({"organizations":organizations,"shared_machines":machines,"spaces_to_transfer":spaces_to_transfer}),
    )
}

pub(crate) fn purge_organization(db: &Connection, org: &str) -> Result<()> {
    if org == "default" {
        return Err(forbidden("the installation workspace cannot be dissolved"));
    }
    let mut stmt = db
        .prepare(
            "SELECT id FROM records WHERE kind='space' AND json_extract(fields,'$.organization')=?",
        )
        .map_err(storage)?;
    let spaces = stmt
        .query_map([org], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    db.execute(
        "DELETE FROM invitation_uses WHERE id IN (SELECT id FROM invitation_keys WHERE org=?)",
        [org],
    )
    .map_err(storage)?;
    // Include extension tables so a later feature cannot retain the dissolved workspace's data.
    let mut stmt=db.prepare("SELECT s.name, p.name FROM sqlite_schema s JOIN pragma_table_info(s.name) p WHERE s.type='table' AND s.name NOT LIKE 'sqlite_%' AND p.name IN ('org','space')").map_err(storage)?;
    let tables = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (table, column) in tables {
        let table = table.replace('"', "\"\"");
        let sql = format!("DELETE FROM \"{table}\" WHERE {column}=?");
        if column == "org" {
            db.execute(&sql, [org]).map_err(storage)?;
        } else {
            for space in &spaces {
                db.execute(&sql, [space]).map_err(storage)?;
            }
        }
    }
    Ok(())
}
