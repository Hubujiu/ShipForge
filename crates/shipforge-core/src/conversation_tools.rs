use super::*;
pub(crate) mod documents;
pub(crate) mod reviews;
mod schema;
mod shortcuts;
mod slash;
pub(crate) use schema::extend as schema;
pub(crate) use slash::advertised_commands;
pub(crate) const READS: &[&str] = &[
    "document.share",
    "document.open",
    "document.preview",
    "document.embeds",
    "chat.slash",
    "shortcut.catalog",
    "shortcut.history",
    "task.review.state",
    "chat.share.image",
];
pub(crate) const WRITES: &[&str] = &[
    "chat.slash.send",
    "shortcut.save",
    "shortcut.delete",
    "shortcut.organize",
    "shortcut.run",
    "task.review",
    "chat.complete",
];
pub(crate) fn handles(op: &str) -> bool {
    READS.contains(&op) || WRITES.contains(&op)
}
pub(crate) fn init(db: &Connection) -> Result<()> {
    shortcuts::init(db)?;
    reviews::init(db)
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    match op {
        op if op.starts_with("document.") => documents::execute(c, op, q),
        op if op.starts_with("shortcut.") => shortcuts::execute(c, op, q),
        "chat.slash" | "chat.slash.send" => slash::execute(c, op, q),
        "task.review" | "task.review.state" | "chat.complete" => reviews::execute(c, op, q),
        "chat.share.image" => crate::chat::share_image(c, q),
        _ => Err(invalid("unknown conversation operation")),
    }
}
pub(crate) fn replay(c: &Context<'_>, op: &str, q: &Value, v: &Value) -> Result<()> {
    c.require_edit()?;
    match op {
        "shortcut.run" | "chat.slash.send" => {
            if let Some(task) = q["task"].as_str() {
                c.readable("task", task)?;
            }
            c.readable("chat", required(v, "chat")?)?;
            c.readable("message", required(v, "message")?)?;
            c.readable("run", required(v, "run")?)?;
        }
        "shortcut.delete" => {
            if v["owner"] != c.actor {
                return Err(forbidden("shortcut unavailable"));
            }
        }
        "shortcut.save" | "shortcut.organize" => {
            shortcuts::check_receipt(c, q, v)?;
        }
        "task.review" => {
            c.readable("task", required(q, "id")?)?;
            c.readable("comment", required(v, "comment")?)?;
            for run in v["reviewed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                c.readable("run", run)?;
            }
        }
        "chat.complete" => {
            c.readable("chat", required(q, "id")?)?;
            if let Some(id) = v["task"].as_str() {
                c.readable("task", id)?;
            }
        }
        _ => {}
    }
    Ok(())
}
