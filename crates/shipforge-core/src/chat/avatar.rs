use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::Cursor;

const MAX: usize = 2 * 1024 * 1024;
pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_chat_avatar(space TEXT, chat TEXT, digest TEXT NOT NULL,png BLOB NOT NULL,icon BLOB NOT NULL,PRIMARY KEY(space,chat));").map_err(storage)
}
fn normalize(q: &Value) -> Result<(Vec<u8>, Vec<u8>)> {
    let content = required(q, "content_base64")?;
    if content.len() > MAX.div_ceil(3) * 4 {
        return Err(invalid("avatar exceeds 2 MiB"));
    }
    let bytes = STANDARD
        .decode(content)
        .map_err(|_| invalid("invalid avatar encoding"))?;
    if bytes.len() > MAX {
        return Err(invalid("avatar exceeds 2 MiB"));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid("invalid image"))?;
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Gif
                | image::ImageFormat::WebP
        )
    ) {
        return Err(invalid("choose a PNG, JPEG, GIF or WebP avatar"));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let source = reader
        .decode()
        .map_err(|_| invalid("avatar is too large or unreadable"))?;
    let edge = source.width().min(source.height());
    let center = source.crop_imm(
        (source.width() - edge) / 2,
        (source.height() - edge) / 2,
        edge,
        edge,
    );
    let square = center.resize_exact(128, 128, image::imageops::FilterType::Triangle);
    let encode = |image: &image::DynamicImage| -> Result<Vec<u8>> {
        let mut buf = Cursor::new(Vec::new());
        image
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(storage)?;
        Ok(buf.into_inner())
    };
    Ok((encode(&square)?, encode(&square.thumbnail(32, 32))?))
}
pub(super) fn icon(c: &Context<'_>, room: &Record) -> Result<Value> {
    if !group(room) {
        return Ok(Value::Null);
    }
    let row =
        c.db.query_row(
            "SELECT digest,icon FROM sf_chat_avatar WHERE space=? AND chat=?",
            params![c.space, room.id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(storage)?;
    Ok(row.map(|(digest,bytes)|json!({"sha256":digest,"icon":format!("data:image/png;base64,{}",STANDARD.encode(bytes))})).unwrap_or(Value::Null))
}
pub(super) fn execute(c: &Context<'_>, room: &Record, op: &str, q: &Value) -> Result<Value> {
    if !group(room) {
        return Err(invalid("group avatar requires a group conversation"));
    }
    if op == "chat.avatar" {
        let data =
            c.db.query_row(
                "SELECT digest,png FROM sf_chat_avatar WHERE space=? AND chat=?",
                params![c.space, room.id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(storage)?;
        return Ok(match data {
            Some((sha, bytes)) => {
                json!({"id":room.id,"rev":room.rev,"avatar":{"filename":"group-avatar.png","media_type":"image/png","width":128,"height":128,"sha256":sha,"bytes":bytes.len(),"content_base64":STANDARD.encode(bytes)}})
            }
            None => json!({"id":room.id,"rev":room.rev,"avatar":null}),
        });
    }
    require_owner(c, room)?;
    if q["expected_rev"].as_i64() != Some(room.rev) {
        return Err(conflict("refresh the group before changing its avatar"));
    }
    let n = if q["remove"] == true {
        if q.get("content_base64").is_some() {
            return Err(invalid("choose an image or remove the current avatar"));
        }
        c.db.execute(
            "DELETE FROM sf_chat_avatar WHERE space=? AND chat=?",
            params![c.space, room.id],
        )
        .map_err(storage)?
    } else {
        let (png, small) = normalize(q)?;
        let sha = crate::attachments::hash(&png);
        c.db.execute("INSERT INTO sf_chat_avatar(space,chat,digest,png,icon) VALUES(?,?,?,?,?) ON CONFLICT(space,chat) DO UPDATE SET digest=excluded.digest,png=excluded.png,icon=excluded.icon WHERE digest!=excluded.digest",params![c.space,room.id,sha,png,small]).map_err(storage)?
    };
    let rev = if n > 0 {
        c.update(room, &room.title, room.fields.clone())?.rev
    } else {
        room.rev
    };
    Ok(json!({"id":room.id,"rev":rev,"avatar":icon(c,room)?}))
}
