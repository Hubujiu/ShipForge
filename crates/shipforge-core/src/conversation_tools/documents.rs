use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::collections::HashSet;
const PREFIX: &str = "shipforge://document/";
pub fn parts(link: &str) -> Result<(String, String, String)> {
    crate::saved_views::resource_link_parts(link, PREFIX)
        .map_err(|_| invalid("invalid document link"))
}
fn link(c: &Context<'_>, r: &Record) -> Result<String> {
    Ok(format!(
        "{PREFIX}{}/{}/{}",
        crate::saved_views::workspace(c)?,
        URL_SAFE_NO_PAD.encode(&r.space),
        URL_SAFE_NO_PAD.encode(&r.id)
    ))
}
pub(crate) fn resolve(c: &Context<'_>, url: &str) -> Result<Record> {
    let (workspace, space, id) = parts(url)?;
    if workspace != crate::saved_views::workspace(c)? {
        return Err(invalid(
            "connect to the workspace which issued the document link",
        ));
    }
    let dest = Context {
        space: &space,
        ..*c
    };
    dest.role()?;
    dest.readable("document", &id)
}
fn preview(doc: &Record) -> Value {
    let body = doc.fields["body"].as_str().unwrap_or_default();
    let mut outline = Vec::new();
    let mut fence = None;
    for (line, line_body) in body.lines().enumerate() {
        let s = line_body.trim_start();
        if let Some(mark) = s.chars().next().filter(|c| *c == '`' || *c == '~') {
            let count = s.chars().take_while(|c| *c == mark).count();
            if count >= 3 {
                if let Some((first, n)) = fence {
                    if mark == first && count >= n {
                        fence = None;
                    }
                } else {
                    fence = Some((mark, count));
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        let level = s.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&level) && s[level..].starts_with(' ') && outline.len() < 256 {
            outline.push(json!({"level":level,"line":line+1,"text":s[level..].trim().trim_end_matches('#').trim_end()}));
        }
    }
    let summary: String = body.chars().take(500).collect();
    json!({"space":doc.space,"id":doc.id,"title":doc.title,"rev":doc.rev,"summary":summary,"outline":outline,"attachments":doc.fields["attachments"].as_array().cloned().unwrap_or_default()})
}
fn embeds(c: &Context<'_>, q: &Value) -> Result<Value> {
    let kind = required(q, "kind")?;
    if !matches!(kind, "document" | "task" | "comment") {
        return Err(invalid(
            "documents may be embedded in a document, task or comment",
        ));
    }
    let source = c.readable(kind, required(q, "id")?)?;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for body in ["body", "description"]
        .into_iter()
        .filter_map(|key| source.fields[key].as_str())
    {
        for (i, _) in body.match_indices(PREFIX) {
            let rest = &body[i + PREFIX.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/')))
                .unwrap_or(rest.len());
            let url = &body[i..i + PREFIX.len() + end];
            if !seen.insert(url) || parts(url).is_err() {
                continue;
            }
            match resolve(c, url) {
                Ok(d) => entries.push(json!({"link":url,"available":true,"preview":preview(&d)})),
                Err(e) if e.starts_with("storage:") => return Err(e),
                Err(_) => entries.push(json!({"link":url,"available":false})),
            }
            if entries.len() == 64 {
                break;
            }
        }
        if entries.len() == 64 {
            break;
        }
    }
    Ok(json!({"kind":kind,"id":source.id,"rev":source.rev,"items":entries}))
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    match op {
        "document.share" => {
            let d = c.readable("document", required(q, "id")?)?;
            let url = link(c, &d)?;
            let path = url.trim_start_matches("shipforge://");
            Ok(json!({"link":url,"path":format!("/{path}"),"id":d.id,"rev":d.rev}))
        }
        "document.open" => Ok(as_value(resolve(c, required(q, "link")?)?)),
        "document.preview" => Ok(preview(&resolve(c, required(q, "link")?)?)),
        "document.embeds" => embeds(c, q),
        _ => Err(invalid("unknown document link operation")),
    }
}
