//! The transactional workspace operations used by every ShipForge client.
mod access;
mod actions;
mod agents;
mod attachments;
mod bots;
mod chat;
mod conversation_tools;
mod defaults;
mod discussions;
pub use conversation_tools::documents::parts as document_link_parts;
mod domain;
mod git_workspace;
mod identity;
mod im;
mod inbox;
mod invitations;
mod iterations;
mod jira;
mod mail;
pub mod onboarding;
mod organization;
pub use mail::{MailDispatch, MailResult, delivery_link_parts};
mod persistence;
pub mod personal_tokens;
pub mod preferences;
mod project_timeline;
mod protocol;
mod providers;
mod query;
pub mod runtime_diagnostics;
mod saved_views;
pub mod skill_package;
mod spaces;
mod statuses;
pub use statuses::display_name as status_display_name;
mod tags;
mod task_assistants;
mod task_commands;
mod task_fields;
mod tasks;
mod transfer;
mod triage;
mod vcs;
mod vendors;
mod worker;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

pub use chat::render_share_png;
pub use chat::share_text;
pub use protocol::{is_read_only, schema};
pub use vendors::{bugly_authorization, zentao_installation};
pub fn machine_is_online(fields: &Value) -> bool {
    worker::machine_is_online_at(fields, now())
}
pub use saved_views::view_link_parts;
pub fn inbox_link_parts(link: &str) -> Result<(String, String, String)> {
    saved_views::resource_link_parts(link, "shipforge://inbox/")
}
pub use triage::intake::parts as task_link_parts;

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

    /// Read only the content explicitly frozen for this share. A caller supplies a
    /// verified identity for organization links; a public link can be opened anonymously.
    pub fn shared_conversation(&self, share: &str, actor: Option<&str>) -> Result<Value> {
        chat::read_share(&self.db, share, actor)
    }

    /// Accept a Slack bot receipt after the native HTTP adapter verifies the request signature.
    pub fn confirm_slack_delivery(
        &mut self,
        actor: &str,
        space: &str,
        connector: &str,
        payload: &Value,
    ) -> Result<Value> {
        self.verified_slack(actor, space, connector, |c, connector| {
            im::echo(c, connector, payload)
        })
    }

    /// Apply an interactive message only after the native adapter authenticates Slack and its route.
    pub fn slack_card_action(
        &mut self,
        actor: &str,
        space: &str,
        connector: &str,
        payload: &Value,
    ) -> Result<Value> {
        self.verified_slack(actor, space, connector, |c, connector| {
            im::click(c, connector, payload)
        })
    }

    fn verified_slack(
        &mut self,
        actor: &str,
        space: &str,
        connector: &str,
        apply: impl FnOnce(&Context<'_>, &Record) -> Result<Value>,
    ) -> Result<Value> {
        identifier(actor)?;
        identifier(space)?;
        identifier(connector)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let c = Context {
            db: &tx,
            actor,
            space,
        };
        c.require_admin()?;
        spaces::ensure_active(&c)?;
        let connector = c.readable("connector", connector)?;
        let answer = apply(&c, &connector)?;
        tx.commit().map_err(storage)?;
        Ok(answer)
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
        iterations::maintain_space(&mut self.db, space, chrono::Utc::now())?;
        worker::maintain_queue(&mut self.db, Some(space), now())?;
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
        identity::ensure_active(&tx, actor)?;
        if !matches!(
            op,
            "space.list"
                | "space.join"
                | "space.leave"
                | "organization.list"
                | "schema"
                | "preferences.get"
                | "preferences.update"
                | "invitation.accept"
                | "chat.share.read"
                | "view.open"
                | "task.open"
                | "delivery.open"
                | "document.open"
                | "document.preview"
                | "chat.share.image"
                | "triage.submit"
                | "triage.settings"
                | "triage.configure"
                | "jira.read"
                | "jira.write"
        ) {
            c.authorize_scope(op, &request)?;
        }
        if !read && spaces::guarded(op) {
            spaces::ensure_active(&c)?;
        }
        // A lease credential or machine token must never enter a general request receipt.
        let cache = !read
            && !worker::OPS.contains(&op)
            && op != "provider.ingest"
            && !op.starts_with("cnb.internal.")
            && !op.starts_with("forge.")
            && op != "jira.write"
            && op != "assistant.open"
            && !matches!(
                op,
                "organization.depart" | "organization.configure" | "organization.ownership"
            )
            && !matches!(op, "delivery.retry" | "space.transfer" | "git.configure")
            && !op.starts_with("imports.")
            && !op.starts_with("bugly.")
            && !op.starts_with("im.")
            && !op.starts_with("bot.")
            && !matches!(op, "inbox.open" | "followers.change")
            && !op.starts_with("invitation.");
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
        let fingerprint = key.map(|_| attachments::receipt(&request, op));
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
                if Some(&receipt_request) != fingerprint.as_ref() {
                    return Err(conflict(
                        "request_id already identifies a different request",
                    ));
                }
                // Replays must still pass current resource permissions.
                let mut value: Value = serde_json::from_str(&result).map_err(storage)?;
                c.authorize_replay(&request, &value)?;
                triage::redact(&c, &mut value)?;
                current_following(&c, &mut value)?;
                return Ok(value);
            }
        }
        let mut result = match op {
            op if statuses::READS.contains(&op) || statuses::WRITES.contains(&op) => {
                statuses::execute(&c, op, &request)
            }
            "delivery.history" | "delivery.open" | "delivery.retry" => {
                mail::command(&c, op, &request)
            }
            "git.settings" | "git.configure" => git_workspace::execute(&c, op, &request),
            "organization.settings"
            | "organization.configure"
            | "organization.depart"
            | "organization.ownership" => organization::execute(&c, &request, op),
            op if op.starts_with("space.") && op != "space.list" => {
                spaces::execute(&c, &request, op)
            }
            "jira.read" | "jira.write" => jira::execute(&c, &request, read),
            op if op.starts_with("cnb.internal.") => vcs::cnb_command(&c, &request, op),
            op if op.starts_with("forge.") => vcs::forge::command(&c, op, &request),
            op if op.starts_with("imports.") || op.starts_with("bugly.") => {
                vendors::execute(&c, &request, op)
            }
            op if op.starts_with("invitation.") => invitations::execute(&c, op, &request),
            op if agents::handles(op) => agents::execute(&c, op, &request),
            "schema" => Ok(schema()),
            op if op.starts_with("triage.")
                || matches!(op, "task.share" | "task.open" | "customer_request.convert") =>
            {
                triage::execute(&c, op, &request)
            }
            op if task_assistants::handles(op) => task_assistants::execute(&c, op, &request),
            op if bots::handles(op) => bots::execute(&c, op, &request),
            op if conversation_tools::handles(op) => conversation_tools::execute(&c, op, &request),
            "project.timeline" => project_timeline::timeline(&c, &request),
            "task.type.preview" => task_fields::preview_switch(&c, &request),
            "task_type.bindings.preview" => task_fields::preview_bindings(&c, &request),
            "comment.thread" => discussions::thread(&c, &request),
            "comment.resolve" | "comment.reopen" => {
                discussions::mark(&c, &request, None, op == "comment.resolve")
            }
            "view.catalog" | "view.organize" | "view.preview" | "view.share" | "view.open"
            | "view.save_as" => saved_views::execute(&c, op, &request),
            "iteration.settings"
            | "iteration.configure"
            | "iteration.start"
            | "iteration.history" => iterations::execute(&c, &request, op),
            "task.conversation" => task_commands::conversation(&c, &request),
            "task.assign" => task_commands::assignees::assign(&c, &request),
            "task.command" => task_commands::command(&c, &request),
            "comment.reactions" | "comment.react" => {
                discussions::reactions(&c, &request, op == "comment.react")
            }
            "task.move" => task_commands::move_task(&c, &request),
            "onboarding.catalog" => Ok(json!(onboarding::groups())),
            "onboarding.get" => onboarding::get(&c),
            "tag.apply" => tags::apply(&c, &request),
            "tag.merge" => tags::merge(&c, &request),
            "onboarding.display" => onboarding::display(&c, &request),
            "onboarding.setup" => onboarding::setup(&c, &request),
            "preferences.get" | "preferences.update" => preferences::execute(&c, &request),
            "whoami" => Ok(json!({"actor":actor,"space":space,"role":c.role()?})),
            "attachment.upload" => attachments::upload(&c, &request),
            "attachment.download" => attachments::download(&c, &request),
            "provider.ingest" | "provider.preview" => providers::execute(&c, &request, read),
            op if op.starts_with("im.") => im::execute(&c, &request),
            op if op.starts_with("inbox.") || op.starts_with("followers.") => {
                inbox::execute(&c, &request, op)
            }
            op if op.starts_with("chat.") => chat::execute(&c, &request, op),
            op if op.starts_with("assistant.") => chat::assistant::execute(&c, &request, op),
            "create" | "update" | "delete" => write_request(&c, &request),
            "list" | "get" | "search" | "directory.people" | "events" | "space.list"
            | "organization.list" | "view.run" | "dashboard" | "task.relations"
            | "project.progress" | "iteration.preview" | "iteration.timeline"
            | "document.history" | "export" | "import.preview" => query::execute(&c, &request, op),
            "iteration.close" | "triage" | "follow" | "notification.act" | "document.restore"
            | "artifact.save" | "import" => actions::execute(&c, &request, op),
            _ => worker::dispatch(&c, &request).ok_or_else(|| invalid("unknown operation"))?,
        }?;
        if !read {
            onboarding::successful(&c, op, &request, &result)?;
        }
        if let Some(key) = key {
            tx.execute(
                "INSERT INTO receipts(space,actor,key,request,result) VALUES(?1,?2,?3,?4,?5)",
                params![
                    space,
                    actor,
                    key,
                    fingerprint.expect("receipt exists for key").to_string(),
                    result.to_string()
                ],
            )
            .map_err(storage)?;
        }
        if matches!(
            op,
            "get" | "list" | "space.list" | "update" | "search" | "task.open" | "triage"
        ) {
            triage::redact(&c, &mut result)?;
        }
        current_following(&c, &mut result)?;
        if request["kind"] == "chat" && matches!(op, "get" | "list") {
            chat::annotate(&c, &mut result)?;
        }
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}

fn current_following(c: &Context<'_>, result: &mut Value) -> Result<()> {
    if result["kind"] == "task"
        && result["fields"].is_object()
        && let Some(id) = result["id"].as_str()
    {
        let scope = Context {
            space: result["space"].as_str().unwrap_or(c.space),
            ..*c
        };
        let threads = crate::im::options(&scope, id)?;
        let waiting = conversation_tools::reviews::awaiting(&scope, id)?;
        let follows: bool =
            c.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM followers WHERE space=? AND actor=? AND task=?)",
                params![result["space"].as_str().unwrap_or(c.space), c.actor, id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        result["fields"]["following"] = json!(follows);
        result["fields"]["awaiting_review"] = json!(waiting);
        if threads.as_array().is_some_and(|v| !v.is_empty()) {
            result["fields"]["im_threads"] = threads;
        } else if let Some(fields) = result["fields"].as_object_mut() {
            fields.remove("im_threads");
        }
    }
    Ok(())
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
