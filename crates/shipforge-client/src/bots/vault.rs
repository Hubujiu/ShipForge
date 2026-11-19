use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
pub(super) struct Vault {
    db: Connection,
    scope: String,
    locks: PathBuf,
}
fn store(e: impl std::fmt::Display) -> String {
    format!("storage: local bot profile: {e}")
}
fn seal(v: &Value) -> R<Vec<u8>> {
    Ok(crate::credentials::Connection::new("", &v.to_string())?.token)
}
fn open(v: Vec<u8>) -> R<Value> {
    let raw = crate::credentials::Connection {
        url: String::new(),
        token: v,
    }
    .secret()?;
    serde_json::from_str(&raw).map_err(store)
}
impl Vault {
    pub(super) fn new(dir: &Path, scope: String) -> R<Self> {
        let path = dir.join("personal-bots.sqlite3");
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&path)
                .map_err(store)?;
        }
        let db = Connection::open(path).map_err(store)?;
        db.busy_timeout(std::time::Duration::from_secs(4))
            .map_err(store)?;
        db.execute_batch("PRAGMA secure_delete=ON; CREATE TABLE IF NOT EXISTS profiles(scope TEXT,id TEXT,secret BLOB NOT NULL,PRIMARY KEY(scope,id));CREATE TABLE IF NOT EXISTS wecom_completed(scope TEXT NOT NULL,id TEXT NOT NULL,source TEXT NOT NULL,at INTEGER NOT NULL,PRIMARY KEY(scope,id,source));CREATE TABLE IF NOT EXISTS wecom_frames(scope TEXT NOT NULL,id TEXT NOT NULL,source TEXT NOT NULL,payload BLOB NOT NULL,PRIMARY KEY(scope,id,source));CREATE TABLE IF NOT EXISTS revoked(scope TEXT,id TEXT,PRIMARY KEY(scope,id));CREATE TABLE IF NOT EXISTS replies(scope TEXT,id TEXT,peer TEXT,remote TEXT,secret BLOB NOT NULL,PRIMARY KEY(scope,id,peer,remote));").map_err(store)?;
        let locks = dir.join("personal-bot-locks");
        std::fs::create_dir_all(&locks).map_err(store)?;
        Ok(Self { db, scope, locks })
    }
    pub(super) fn save(&self, id: &str, s: &Value) -> R<()> {
        let changed=self.db.execute("INSERT INTO profiles(scope,id,secret) SELECT ?1,?2,?3 WHERE NOT EXISTS(SELECT 1 FROM revoked WHERE scope=?1 AND id=?2) ON CONFLICT(scope,id) DO UPDATE SET secret=excluded.secret",params![self.scope,id,seal(s)?]).map_err(store)?;
        if changed == 0 {
            return Err("conflict: binding disconnected or replaced".into());
        }
        Ok(())
    }
    pub(super) fn load(&self, id: &str) -> R<Value> {
        let b: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT secret FROM profiles WHERE scope=? AND id=?",
                params![self.scope, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(store)?;
        open(b.ok_or("not_found: bot setup on this device")?)
    }
    pub(super) fn lock(&self, id: &str) -> R<std::fs::File> {
        let exists: bool = self
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM profiles WHERE scope=? AND id=?)",
                params![self.scope, id],
                |r| r.get(0),
            )
            .map_err(store)?;
        if !exists {
            return Err("not_found: bot setup on this device".into());
        }
        self.lock_path(&json!([self.scope, id]))
    }
    pub(super) fn stream_lock(&self, id: &str) -> R<std::fs::File> {
        self.lock_path(&json!([self.scope, id, "wecom-socket"]))
    }
    fn lock_path(&self, key: &Value) -> R<std::fs::File> {
        let digest = Sha256::digest(key.to_string().as_bytes());
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(
                self.locks.join(
                    digest
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                ),
            )
            .map_err(store)?;
        file.try_lock()
            .map_err(|_| "conflict: another adapter is already active")?;
        Ok(file)
    }
    pub(super) fn context(&self, id: &str, peer: &str, remote: &str, secret: &str) -> R<()> {
        self.db.execute("INSERT INTO replies(scope,id,peer,remote,secret) SELECT ?1,?2,?3,?4,?5 WHERE EXISTS(SELECT 1 FROM profiles WHERE scope=?1 AND id=?2) ON CONFLICT(scope,id,peer,remote) DO UPDATE SET secret=excluded.secret",params![self.scope,id,peer,remote,seal(&json!(secret))?]).map_err(store)?;
        Ok(())
    }
    pub(super) fn reply(&self, id: &str, peer: &str, remote: &str) -> R<String> {
        let b: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT secret FROM replies WHERE scope=? AND id=? AND peer=? AND remote=?",
                params![self.scope, id, peer, remote],
                |r| r.get(0),
            )
            .optional()
            .map_err(store)?;
        open(b.ok_or("provider: missing WeChat reply context")?)?
            .as_str()
            .map(str::to_owned)
            .ok_or("storage: invalid reply context".into())
    }
    pub(super) fn buffer_wecom(&self, id: &str, frame: &Value) -> R<()> {
        let b = &frame["body"];
        let Some(source) = b["msgid"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 512)
        else {
            return Ok(());
        };
        let valid = |v: &Value| {
            v.as_str().is_some_and(|s| {
                !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
            })
        };
        if source.chars().any(char::is_control)
            || !valid(&b["from"]["userid"])
            || !matches!(b["chattype"].as_str(), Some("single" | "group"))
            || b["chattype"] == "group" && !valid(&b["chatid"])
        {
            return Ok(());
        }
        let key = json!([b["chattype"], b["chatid"], b["from"]["userid"], source]).to_string();
        self.db
            .execute(
                "DELETE FROM wecom_completed WHERE at<?",
                [chrono::Utc::now().timestamp() - 604800],
            )
            .map_err(store)?;
        if self
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM wecom_completed WHERE scope=? AND id=? AND source=?)",
                params![self.scope, id, key],
                |r| r.get::<_, bool>(0),
            )
            .map_err(store)?
        {
            return Ok(());
        }
        let count: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM wecom_frames WHERE scope=? AND id=?",
                params![self.scope, id],
                |r| r.get(0),
            )
            .map_err(store)?;
        if count >= 1024 {
            return Err("storage: backlog of WeCom callbacks exceeds 1024".into());
        }
        self.db.execute("INSERT OR IGNORE INTO wecom_frames(scope,id,source,payload) SELECT ?1,?2,?3,?4 WHERE NOT EXISTS(SELECT 1 FROM revoked WHERE scope=?1 AND id=?2)",params![self.scope,id,key,seal(&json!({"frame":frame,"at":chrono::Utc::now().timestamp()}))?]).map_err(store)?;
        Ok(())
    }
    pub(super) fn wecom_pending(&self, id: &str) -> R<Vec<(String, Value)>> {
        let mut st=self.db.prepare("SELECT source,payload FROM wecom_frames WHERE scope=? AND id=? ORDER BY rowid LIMIT 32").map_err(store)?;
        let mut rows = Vec::new();
        for row in st
            .query_map(params![self.scope, id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(store)?
        {
            let (key, data) = row.map_err(store)?;
            rows.push((key, open(data)?));
        }
        Ok(rows)
    }
    pub(super) fn wecom_complete(&self, id: &str, key: &str) -> R<()> {
        let tx = self.db.unchecked_transaction().map_err(store)?;
        tx.execute(
            "INSERT OR REPLACE INTO wecom_completed VALUES(?,?,?,?)",
            params![self.scope, id, key, chrono::Utc::now().timestamp()],
        )
        .map_err(store)?;
        tx.execute(
            "DELETE FROM wecom_frames WHERE scope=? AND id=? AND source=?",
            params![self.scope, id, key],
        )
        .map_err(store)?;
        tx.commit().map_err(store)
    }
    pub(super) fn remove(&self, id: &str) -> R<()> {
        let tx = self.db.unchecked_transaction().map_err(store)?;
        tx.execute("INSERT OR IGNORE INTO revoked(scope,id) SELECT scope,id FROM profiles WHERE scope=? AND id=?",params![self.scope,id]).map_err(store)?;
        tx.execute(
            "DELETE FROM wecom_completed WHERE scope=? AND id=?",
            params![self.scope, id],
        )
        .map_err(store)?;
        tx.execute(
            "DELETE FROM wecom_frames WHERE scope=? AND id=?",
            params![self.scope, id],
        )
        .map_err(store)?;
        tx.execute(
            "DELETE FROM replies WHERE scope=? AND id=?",
            params![self.scope, id],
        )
        .map_err(store)?;
        tx.execute(
            "DELETE FROM profiles WHERE scope=? AND id=?",
            params![self.scope, id],
        )
        .map_err(store)?;
        tx.commit().map_err(store)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revoked_profiles_cannot_be_recreated_by_inflight_pollers() {
        let profile = tempfile::tempdir().unwrap();
        let a = Vault::new(profile.path(), "a".into()).unwrap();
        let b = Vault::new(profile.path(), "a".into()).unwrap();
        let s = json!({"secret":"fixture"});
        a.save("bot", &s).unwrap();
        let guard = a.lock("bot").unwrap();
        assert!(b.lock("bot").is_err());
        a.context("bot", "peer", "message", "opaque").unwrap();
        b.remove("bot").unwrap();
        assert!(a.save("bot", &s).is_err());
        a.context("bot", "peer", "message", "late").unwrap();
        assert!(a.reply("bot", "peer", "message").is_err());
        assert!(a.load("bot").is_err());
        drop(guard);
    }
}
