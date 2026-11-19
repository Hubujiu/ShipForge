//! A managed skill stores a small UTF-8 file tree. Relative names survive on Windows and Unix.
use crate::{Result, invalid};
use serde_json::{Value, json};
use std::collections::HashSet;
pub const MAX_FILES: usize = 96;
pub const MAX_BYTES: usize = 512 * 1024;
pub const MAX_FILE_BYTES: usize = 192 * 1024;

pub fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 240
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
    {
        return Err(invalid("invalid skill file path"));
    }
    for piece in path.split('/') {
        let lower = piece.to_lowercase();
        let stem = lower.split('.').next().unwrap_or("");
        if piece.is_empty()
            || piece.starts_with('.')
            || piece.ends_with([' ', '.'])
            || piece.contains(['<', '>', '"', '|', '?', '*'])
            || matches!(
                stem,
                "aux"
                    | "con"
                    | "prn"
                    | "nul"
                    | "credentials"
                    | "secrets"
                    | "token"
                    | "id_rsa"
                    | "id_ed25519"
                    | "id_ecdsa"
            )
            || (stem.len() == 4
                && (stem.starts_with("com") || stem.starts_with("lpt"))
                && stem.as_bytes()[3].is_ascii_digit())
            || ["pem", "key", "p12", "pfx", "kdbx", "env"]
                .iter()
                .any(|ext| lower.ends_with(&format!(".{ext}")))
        {
            return Err(invalid(
                "hidden, sensitive or invalid path in skill package",
            ));
        }
    }
    Ok(())
}
pub fn validate(files: &Value) -> Result<Value> {
    let files = files
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= MAX_FILES)
        .ok_or_else(|| invalid("skill package needs 1 to 96 files"))?;
    let mut names = HashSet::new();
    let mut total = 0;
    let mut entry = None;
    for file in files {
        let path = file["path"]
            .as_str()
            .ok_or_else(|| invalid("skill file needs path"))?;
        relative(path)?;
        if !names.insert(path.to_lowercase()) {
            return Err(invalid("duplicate skill file path"));
        }
        let text = file["text"]
            .as_str()
            .ok_or_else(|| invalid("skill file needs UTF-8 text"))?;
        total += text.len();
        if text.len() > MAX_FILE_BYTES
            || total > MAX_BYTES
            || text
                .chars()
                .any(|x| x.is_control() && !matches!(x, '\n' | '\r' | '\t'))
        {
            return Err(invalid("skill package contains binary or oversized text"));
        }
        if path == "SKILL.md" {
            entry = Some(text.trim_start_matches('\u{feff}'));
        }
    }
    for path in &names {
        if path
            .match_indices('/')
            .any(|(i, _)| names.contains(&path[..i]))
        {
            return Err(invalid("a skill path is both a file and directory"));
        }
    }
    let document = entry.ok_or_else(|| invalid("skill package needs SKILL.md at its root"))?;
    let normalized = document.replace("\r\n", "\n");
    let mut title = String::new();
    let mut description = String::new();
    let body = if let Some(header) = normalized.strip_prefix("---\n") {
        let (meta, body) = header
            .split_once("\n---\n")
            .ok_or_else(|| invalid("unterminated SKILL.md frontmatter"))?;
        let mut lines = meta.lines().peekable();
        while let Some(line) = lines.next() {
            if line.starts_with(char::is_whitespace) {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let target = match key.trim() {
                "name" => &mut title,
                "description" => &mut description,
                _ => continue,
            };
            let value = value.trim();
            *target = if matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+") {
                let mut parts = Vec::new();
                while lines
                    .peek()
                    .is_some_and(|line| line.starts_with(char::is_whitespace) || line.is_empty())
                {
                    parts.push(lines.next().unwrap().trim());
                }
                parts.join(if value.starts_with('|') { "\n" } else { " " })
            } else {
                value.trim_matches(['\'', '"']).to_owned()
            };
        }
        body.trim()
    } else {
        normalized.trim()
    };
    let title = (!title.is_empty())
        .then_some(title.as_str())
        .or_else(|| body.lines().find_map(|l| l.strip_prefix("# ")))
        .filter(|v| !v.trim().is_empty() && v.chars().count() <= 150)
        .ok_or_else(|| invalid("SKILL.md needs a name or a Markdown heading"))?;
    if body.is_empty() {
        return Err(invalid("SKILL.md needs instructions"));
    }
    Ok(json!({"title":title,"description":description,"instructions":body,"files":files}))
}
pub(crate) fn edit(
    old: Option<&crate::Record>,
    patch: &mut serde_json::Map<String, Value>,
) -> Result<()> {
    if let Some(files) = patch.get("files") {
        let spec = validate(files)?;
        patch.insert("instructions".into(), spec["instructions"].clone());
    } else if let Some(instructions) = patch.get("instructions").and_then(Value::as_str)
        && let Some(r) = old
        && let Some(files) = r.fields["files"].as_array()
    {
        let mut files = files.clone();
        let doc = files
            .iter_mut()
            .find(|f| f["path"] == "SKILL.md")
            .ok_or_else(|| invalid("skill entry file is missing"))?;
        let original = doc["text"].as_str().unwrap_or("").replace("\r\n", "\n");
        let header = original
            .strip_prefix("---\n")
            .and_then(|v| v.split_once("\n---"))
            .map(|(m, _)| format!("---\n{m}\n---\n"))
            .unwrap_or_else(|| format!("# {}\n\n", r.title));
        doc["text"] = json!(format!("{header}{instructions}"));
        let files = json!(files);
        validate(&files)?;
        patch.insert("files".into(), files);
    }
    Ok(())
}
