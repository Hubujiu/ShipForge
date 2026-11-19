use super::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{fs, path::PathBuf};

fn db_path(dir: &Path) -> PathBuf {
    dir.join("device-network.sqlite3")
}
fn storage(e: impl std::fmt::Display) -> String {
    format!("storage: device network settings: {e}")
}
fn read(db: &Connection) -> Result<Policy> {
    let data: Option<String> = db
        .query_row("SELECT policy FROM settings WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(storage)?;
    data.map(|s| serde_json::from_str(&s).map_err(storage))
        .transpose()
        .map(Option::unwrap_or_default)
}
fn open(dir: &Path) -> Result<Connection> {
    fs::create_dir_all(dir).map_err(storage)?;
    let path = db_path(dir);
    if !path.exists() {
        match tempfile::NamedTempFile::new_in(dir)
            .map_err(storage)?
            .persist_noclobber(&path)
        {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(storage(e.error)),
        }
    }
    let db =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(storage)?;
    db.busy_timeout(Duration::from_secs(5)).map_err(storage)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),policy TEXT NOT NULL)").map_err(storage)?;
    Ok(db)
}
pub(super) fn load(dir: &Path) -> Result<Policy> {
    if !db_path(dir).try_exists().map_err(storage)? {
        return Ok(Policy::default());
    }
    read(&open(dir)?)
}
pub(super) fn endpoint(raw: &str) -> Result<Url> {
    let u = Url::parse(raw.trim())
        .map_err(|_| "invalid: expected a proxy address with scheme and host")?;
    if !matches!(u.scheme(), "http" | "https" | "socks5")
        || u.host().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || !matches!(u.path(), "" | "/")
        || u.query().is_some()
        || u.fragment().is_some()
        || u.port() == Some(0)
    {
        return Err("invalid: enter an HTTP, HTTPS or SOCKS5 proxy address and put sign-in details in their separate fields".into());
    }
    Ok(u)
}
pub(super) fn edited(prev: &Policy, q: &Value) -> Result<Policy> {
    let mode = serde_json::from_value(q["mode"].clone())
        .map_err(|_| "invalid: choose system, direct or manual")?;
    if mode != Mode::Manual {
        return Ok(Policy {
            mode,
            ..prev.clone()
        });
    }
    let raw = q["endpoint"].as_str().unwrap_or(&prev.endpoint).trim();
    let endpoint = endpoint(raw)?.to_string();
    let username = q["username"].as_str().unwrap_or(&prev.username).to_owned();
    let password = q["password"].as_str().unwrap_or("");
    if username.len() > 256
        || username.chars().any(char::is_control)
        || password.len() > 1024
        || password.chars().any(char::is_control)
    {
        return Err(
            "invalid: proxy sign-in details are too long or contain control characters".into(),
        );
    }
    let credential = if q["clear_password"] == true {
        if !password.is_empty() {
            return Err("invalid: choose to replace or clear the proxy password".into());
        }
        None
    } else if !password.is_empty() {
        if endpoint.is_empty() || username.is_empty() {
            return Err("invalid: a proxy password needs an address and username".into());
        }
        Some(credentials::Connection::new(&endpoint, password)?)
    } else if endpoint == prev.endpoint && username == prev.username {
        prev.credential.clone()
    } else {
        None
    };
    Ok(Policy {
        rev: prev.rev,
        mode,
        endpoint,
        username,
        credential,
    })
}
pub(super) fn save(dir: &Path, q: &Value) -> Result<Value> {
    let mut db = open(dir)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    let last = read(&tx)?;
    if q["expected_rev"].as_u64() != Some(last.rev) {
        return Err("conflict: refresh network settings before saving".into());
    }
    let mut next = edited(&last, q)?;
    next.rev += 1;
    tx.execute("INSERT INTO settings(singleton,policy) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET policy=excluded.policy",
        params![serde_json::to_string(&next).map_err(storage)?]).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(next.public())
}
