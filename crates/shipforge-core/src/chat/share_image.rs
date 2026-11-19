use crate::{Result, invalid, storage};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use resvg::{tiny_skia, usvg};
use serde_json::{Value, json};
use std::{
    fmt::Write,
    io::Cursor,
    sync::{Arc, OnceLock},
};

pub(super) fn thumbnail(bytes: &[u8], filename: &str) -> Option<Value> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format()?,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::Gif
            | image::ImageFormat::WebP
    ) {
        return None;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let source = reader.decode().ok()?;
    let thumb = source.thumbnail(640, 240);
    let mut data = Cursor::new(Vec::new());
    thumb.write_to(&mut data, image::ImageFormat::Png).ok()?;
    Some(
        json!({"filename":filename,"width":thumb.width(),"height":thumb.height(),"png":STANDARD.encode(data.get_ref())}),
    )
}
fn escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}
fn lines(s: &str, units: usize) -> impl Iterator<Item = String> + '_ {
    s.split('\n').flat_map(move |line| {
        let mut out = Vec::new();
        let mut value = String::new();
        let mut width = 0;
        for ch in line.trim_end_matches('\r').chars() {
            let n = if ch.is_ascii() { 1 } else { 2 };
            if width + n > units {
                out.push(std::mem::take(&mut value));
                width = 0;
            }
            value.push(ch);
            width += n;
        }
        out.push(value);
        out
    })
}
pub fn render(snapshot: &Value) -> Result<Vec<u8>> {
    const WIDTH: u32 = 920;
    const MAX_HEIGHT: u32 = 6800;
    let mut nodes = String::new();
    let mut y = 40u32;
    fn paragraph(nodes: &mut String, y: &mut u32, s: &str, size: u32, color: &str) -> Result<()> {
        let units = if size > 22 { 51 } else { 76 };
        for line in lines(s, units) {
            if *y > MAX_HEIGHT - 50 {
                return Err(invalid("select fewer messages for one image"));
            }
            writeln!(
                nodes,
                r#"<text x="34" y="{}" font-size="{size}" fill="{color}">{}</text>"#,
                *y,
                escape(&line)
            )
            .map_err(storage)?;
            *y += size + 9;
        }
        Ok(())
    }
    paragraph(
        &mut nodes,
        &mut y,
        snapshot["title"].as_str().unwrap_or("ShipForge"),
        26,
        "#182138",
    )?;
    for msg in snapshot["messages"].as_array().into_iter().flatten() {
        y += 14;
        paragraph(
            &mut nodes,
            &mut y,
            msg["author"].as_str().unwrap_or(""),
            19,
            "#59488b",
        )?;
        paragraph(
            &mut nodes,
            &mut y,
            msg["body"].as_str().unwrap_or(""),
            18,
            "#202630",
        )?;
        for name in msg["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            paragraph(&mut nodes, &mut y, name, 16, "#576275")?;
        }
        for img in msg["images"].as_array().into_iter().flatten() {
            let Some(content) = img["png"].as_str() else {
                continue;
            };
            let height = img["height"].as_u64().unwrap_or(0).min(240) as u32;
            if y + height > MAX_HEIGHT - 30 {
                return Err(invalid("select fewer images for one card"));
            }
            writeln!(nodes,r#"<image x="34" y="{y}" width="{}" height="{height}" xlink:href="data:image/png;base64,{}"/>"#,img["width"].as_u64().unwrap_or(0).min(640),escape(content)).map_err(storage)?;
            y += height + 10;
        }
    }
    let h = y + 18;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{WIDTH}" height="{h}"><rect width="100%" height="100%" fill="#ffffff"/><g font-family="Microsoft YaHei, Noto Sans CJK SC, DejaVu Sans, Arial, sans-serif">{nodes}</g></svg>"##
    );
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let db = FONTS
        .get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone();
    let opt = usvg::Options {
        fontdb: db,
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(&svg, &opt).map_err(storage)?;
    let mut pixels =
        tiny_skia::Pixmap::new(WIDTH, h).ok_or_else(|| invalid("invalid image dimensions"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    let bytes = pixels.encode_png().map_err(storage)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid("select fewer attachments for one image"));
    }
    Ok(bytes)
}
