//! The transactional workspace operations used by every ShipForge client.
mod access;
mod actions;
mod defaults;
mod domain;
mod persistence;
mod protocol;
mod providers;
mod query;
mod tasks;
mod transfer;
mod worker;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

pub use protocol::{is_read_only, schema};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub space: String,
    pub kind: String,
    pub id: String,
    pub title: String,
    pub fields: Value,
    pub rev: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct Store {
    db: Connection,
}

pub(crate) struct Context<'a> {
    pub db: &'a Connection,
    pub actor: &'a str,
    pub space: &'a str,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }
        Self::from_connection(Connection::open(path).map_err(storage)?)
    }

    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(storage)?)
    }

    fn from_connection(mut db: Connection) -> Result<Self> {
        persistence::initialize(&mut db)?;
        Ok(Self { db })
    }

    pub fn execute(&mut self, actor: &str, request: Value) -> Result<Value> {
        identifier(actor)?;
        let op = required(&request, "op")?;
        if !protocol::operation_names().contains(&op) {
            return Err(format!("invalid: unknown operation {op}"));
        }
        let space = request.get("space").map_or(Ok("default"), |s| {
            s.as_str().ok_or_else(|| invalid("space must be a string"))
        })?;
        identifier(space)?;
        let read = is_read_only(op);
        let tx = self
            .db
            .transaction_with_behavior(if read {
                TransactionBehavior::Deferred
            } else {
                TransactionBehavior::Immediate
            })
            .map_err(storage)?;
        let c = Context {
            db: &tx,
            actor,
            space,
        };
        if !matches!(op, "space.list" | "organization.list" | "schema") {
            c.role()?;
        }
        // A lease credential or machine token must never enter a general request receipt.
        let cache = !read && !worker::OPS.contains(&op) && op != "provider.ingest";
        let key = if cache {
            request
                .get("request_id")
                .map(|v| {
                    let key = v
                        .as_str()
                        .ok_or_else(|| invalid("request_id must be a string"))?;
                    identifier(key)?;
                    Ok::<_, String>(key)
                })
                .transpose()?
        } else {
            None
        };
        if let Some(key) = key {
            let saved: Option<(String, String)> = tx
                .query_row(
                    "SELECT request,result FROM receipts WHERE space=?1 AND actor=?2 AND key=?3",
                    params![space, actor, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(storage)?;
            if let Some((original, result)) = saved {
                let receipt_request: Value = serde_json::from_str(&original).map_err(storage)?;
                if receipt_request != request {
                    return Err(conflict(
                        "request_id already identifies a different request",
                    ));
                }
                // Replays must still pass current resource permissions.
                let value: Value = serde_json::from_str(&result).map_err(storage)?;
                c.authorize_replay(&request, &value)?;
                return Ok(value);
            }
        }
        let result = match op {
            "schema" => Ok(schema()),
            "whoami" => Ok(json!({"actor":actor,"space":space,"role":c.role()?})),
            "provider.ingest" | "provider.preview" => providers::execute(&c, &request, read),
            "create" | "update" | "delete" => write_request(&c, &request),
            "list" | "get" | "search" | "events" | "space.list" | "organization.list"
            | "view.run" | "dashboard" | "task.relations" | "project.progress"
            | "iteration.preview" | "document.history" | "export" | "import.preview" => {
                query::execute(&c, &request, op)
            }
            "iteration.close" | "triage" | "follow" | "notification.act" | "document.restore"
            | "artifact.save" | "import" => actions::execute(&c, &request, op),
            _ => worker::dispatch(&c, &request).ok_or_else(|| invalid("unknown operation"))?,
        }?;
        if let Some(key) = key {
            tx.execute(
                "INSERT INTO receipts(space,actor,key,request,result) VALUES(?1,?2,?3,?4,?5)",
                params![space, actor, key, request.to_string(), result.to_string()],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}

pub(crate) fn write_request(c: &Context<'_>, q: &Value) -> Result<Value> {
    domain::write(c, q)
}

pub(crate) fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| invalid(&format!("{key} must be a nonempty string")))
}

pub(crate) fn invalid(s: &str) -> String {
    format!("invalid: {s}")
}
pub(crate) fn forbidden(s: &str) -> String {
    format!("forbidden: {s}")
}
pub(crate) fn conflict(s: &str) -> String {
    format!("conflict: {s}")
}
pub(crate) fn storage(e: impl std::fmt::Display) -> String {
    format!("storage: {e}")
}
pub(crate) fn identifier(s: &str) -> Result<()> {
    if s.trim().is_empty()
        || s.len() > 160
        || s.chars().any(|c| c.is_control() || c == '/' || c == '\\')
    {
        Err(invalid(
            "identifier is empty, too long or contains a separator",
        ))
    } else {
        Ok(())
    }
}
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub(crate) fn as_value(row: Record) -> Value {
    json!(row)
}
