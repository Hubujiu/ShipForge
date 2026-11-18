//! Binary content lives outside JSON records. A download must follow an authorized object reference.
use crate::{Context, Record, Result, as_value, conflict, domain, invalid, required, storage};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_BYTES: usize = 4 * 1024 * 1024;
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn receipt(request: &Value, op: &str) -> Value {
    let mut fingerprint = request.clone();
    if op == "attachment.upload"
        && let Some(encoded) = request["content_base64"].as_str()
    {
        fingerprint["content_base64"] = json!({"sha256":hash(encoded.as_bytes())});
    }
    fingerprint
}
fn kind(q: &Value) -> Result<&str> {
    let kind = required(q, "kind")?;
    match kind {
        "artifact" | "document" | "task" | "message" => Ok(kind),
        _ => Err(invalid(
            "attachments belong to documents, artifacts, tasks or messages",
        )),
    }
}
fn store(c: &Context<'_>, q: &Value) -> Result<Value> {
    let filename = required(q, "filename")?;
    if filename.chars().count() > 255
        || matches!(filename, "." | "..")
        || filename
            .chars()
            .any(|c| c.is_control() || "/\\:<>|?*\"".contains(c))
    {
        return Err(invalid("filename must be a single file name"));
    }
    let media_type = q
        .get("media_type")
        .map_or(Ok("application/octet-stream"), |_| {
            required(q, "media_type")
        })?;
    if media_type.len() > 128
        || !media_type.is_ascii()
        || media_type.bytes().any(|b| b.is_ascii_control())
    {
        return Err(invalid("invalid media_type"));
    }
    let encoded = q["content_base64"]
        .as_str()
        .ok_or_else(|| invalid("missing content_base64"))?;
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err(invalid("maximum attachment size is 4 MiB"));
    }
    let content = STANDARD
        .decode(encoded)
        .map_err(|_| invalid("content_base64 must be standard base64"))?;
    if content.len() > MAX_BYTES {
        return Err(invalid("maximum attachment size is 4 MiB"));
    }
    let id = Uuid::new_v4().to_string();
    let metadata = json!({"id":id,"filename":filename,"media_type":media_type,"bytes":content.len(),"sha256":hash(&content)});
    c.db.execute(
        "INSERT INTO attachments(space,id,content) VALUES(?,?,?)",
        params![c.space, id, content],
    )
    .map_err(storage)?;
    Ok(metadata)
}
pub(crate) fn append(c: &Context<'_>, target: &Record, entry: &Value) -> Result<Record> {
    let mut f = target.fields.clone();
    if !f["attachments"].is_array() {
        f["attachments"] = json!([]);
    }
    let files = f["attachments"].as_array_mut().expect("initialized array");
    if files.len() >= 32 {
        return Err(invalid("up to 32 files can be attached to one record"));
    }
    files.push(store(c, entry)?);
    c.update(target, &target.title, f)
}
pub(crate) fn upload(c: &Context<'_>, q: &Value) -> Result<Value> {
    let kind = kind(q)?;
    if kind == "document" && q.get("id").is_none() {
        c.require_edit()?;
        let file = store(c, q)?;
        let title = q
            .get("title")
            .map_or(required(&file, "filename"), |_| required(q, "title"))?;
        return domain::write_with_files(
            c,
            &json!({"op":"create","kind":"document","title":title,"fields":{"visibility":q["visibility"].as_str().unwrap_or("private"),"body":q["body"].as_str().unwrap_or("")}}),
            json!([file]),
        );
    }
    let row = c.readable(kind, required(q, "id")?)?;
    c.authorize_write(kind, Some(&row))?;
    domain::check_rev(q, &row)?;
    let updated = append(c, &row, q)?;
    crate::actions::after(c, Some(&row), &updated)?;
    Ok(as_value(updated))
}
pub(crate) fn download(c: &Context<'_>, q: &Value) -> Result<Value> {
    let k = kind(q)?;
    let object = c.readable(k, required(q, "id")?)?;
    let f = if let Some(version) = q.get("version") {
        if k != "document" {
            return Err(invalid(
                "only document attachments have historical versions",
            ));
        }
        let rev = version
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or_else(|| invalid("version must be a positive integer"))?;
        let raw: Option<String> =
            c.db.query_row(
                "SELECT fields FROM versions WHERE space=? AND id=? AND rev=?",
                params![c.space, object.id, rev],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let fields: Value =
            serde_json::from_str(&raw.ok_or_else(|| "not_found: document version".to_owned())?)
                .map_err(storage)?;
        if !c.visible(&Record {
            fields: fields.clone(),
            ..object.clone()
        })? {
            return Err("not_found: document version".into());
        }
        fields
    } else {
        object.fields
    };
    let id = required(q, "attachment_id")?;
    let mut meta = f["attachments"]
        .as_array()
        .and_then(|files| files.iter().find(|v| v["id"] == id))
        .cloned()
        .ok_or_else(|| "not_found: attachment".to_owned())?;
    let bytes: Vec<u8> =
        c.db.query_row(
            "SELECT content FROM attachments WHERE space=? AND id=?",
            params![c.space, id],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| "not_found: attachment data".to_owned())?;
    if meta["sha256"] != hash(&bytes) {
        return Err(conflict("stored attachment hash mismatch"));
    }
    meta["content_base64"] = json!(STANDARD.encode(bytes));
    Ok(meta)
}
pub(crate) fn prune(c: &Context<'_>) -> Result<()> {
    c.db.execute("DELETE FROM attachments AS b WHERE b.space=?1 AND NOT EXISTS(SELECT 1 FROM records r, json_each(r.fields, '$.attachments') AS a WHERE r.space=?1 AND json_extract(a.value,'$.id')=b.id) AND NOT EXISTS(SELECT 1 FROM versions v, json_each(v.fields, '$.attachments') AS a WHERE v.space=?1 AND json_extract(a.value,'$.id')=b.id)",[c.space]).map_err(storage)?;
    Ok(())
}
