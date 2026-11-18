//! Personal settings belong to the authenticated actor across their spaces.
use crate::{Context, Result, conflict, invalid, storage};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const THEMES: &[&str] = &["system", "light", "dark", "magic-blue", "classic-dark"];
pub const FRAME_COLORS: &[&str] = &[
    "none", "color-1", "color-2", "color-3", "color-4", "color-5", "color-6", "color-7", "color-8",
    "color-9", "color-10", "color-11", "color-12",
];
pub const LANGUAGES: &[&str] = &["zh-Hans", "en"];
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("command_menu", "ctrl-k"),
    ("move_task", "ctrl-shift-m"),
    ("find_in_view", "ctrl-f"),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub theme: String,
    pub frame_color: String,
    pub language: String,
    pub send_with: String,
    pub open_after_assignment: bool,
    pub open_after_command: bool,
    pub shortcuts: BTreeMap<String, String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            frame_color: "color-1".into(),
            language: "zh-Hans".into(),
            send_with: "enter".into(),
            open_after_assignment: true,
            open_after_command: true,
            shortcuts: SHORTCUTS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

pub fn normalize_shortcut(value: &str) -> Result<String> {
    let mut keys: Vec<_> = Vec::new();
    let mut modifiers = BTreeSet::new();
    let lower = value.to_ascii_lowercase().replace('+', "-");
    for part in lower.split('-').map(str::trim) {
        match part {
            "ctrl" | "control" => {
                if !modifiers.insert("ctrl") {
                    return Err(invalid("repeated modifier"));
                }
            }
            "shift" | "alt" | "win" => {
                if !modifiers.insert(part) {
                    return Err(invalid("repeated modifier"));
                }
            }
            key => keys.push(key),
        }
    }
    let [key] = keys.as_slice() else {
        return Err(invalid("record one key combination"));
    };
    let function = key
        .strip_prefix('f')
        .and_then(|s| s.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n) && *key == format!("f{n}"));
    let valid = key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric()
        || function
        || matches!(
            *key,
            "left"
                | "right"
                | "up"
                | "down"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
                | "backspace"
                | "delete"
                | "space"
                | "/"
                | "."
                | ","
                | "["
                | "]"
        );
    if !valid
        || !function
            && !modifiers
                .iter()
                .any(|m| matches!(*m, "ctrl" | "alt" | "win"))
    {
        return Err(invalid("use Ctrl or Alt with a key, or a function key"));
    }
    let combo = ["ctrl", "alt", "shift", "win"]
        .into_iter()
        .filter(|m| modifiers.contains(m))
        .chain([*key])
        .collect::<Vec<_>>()
        .join("-");
    if modifiers.contains("win")
        || matches!(
            combo.as_str(),
            "ctrl-n"
                | "ctrl-c"
                | "ctrl-v"
                | "ctrl-x"
                | "ctrl-z"
                | "ctrl-y"
                | "ctrl-a"
                | "alt-left"
                | "alt-right"
                | "alt-f4"
                | "ctrl-alt-delete"
                | "f5"
        )
    {
        return Err(conflict("shortcut is reserved for the system or editing"));
    }
    Ok(combo)
}

pub(crate) fn execute(c: &Context<'_>, request: &Value) -> Result<Value> {
    if request.get("actor").is_some() {
        return Err(invalid(
            "preferences always belong to the authenticated actor",
        ));
    }
    let saved: Option<(i64, String)> =
        c.db.query_row(
            "SELECT rev,settings FROM personal_preferences WHERE actor=?1",
            [c.actor],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let (rev, old) = match saved {
        Some((r, raw)) => (
            r,
            serde_json::from_str::<Preferences>(&raw).map_err(storage)?,
        ),
        None => (1, Preferences::default()),
    };
    if request["op"] == "preferences.get" {
        return Ok(json!({"rev":rev,"preferences":old}));
    }
    if let Some(expected) = request.get("expected_rev")
        && expected.as_i64() != Some(rev)
    {
        return Err(conflict(
            "personal preferences changed; reload before saving",
        ));
    }
    let patch = request["preferences"]
        .as_object()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| invalid("preferences must be a nonempty object"))?;
    let mut data = serde_json::to_value(&old).map_err(storage)?;
    for (k, v) in patch {
        if k == "shortcuts" {
            let changes = v
                .as_object()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| invalid("shortcuts must be a nonempty object"))?;
            for (action, combo) in changes {
                let default = SHORTCUTS
                    .iter()
                    .find(|(k, _)| *k == action)
                    .ok_or_else(|| invalid("unknown shortcut command"))?
                    .1;
                data[k][action] = json!(if combo.is_null() {
                    default.to_string()
                } else {
                    normalize_shortcut(
                        combo
                            .as_str()
                            .ok_or_else(|| invalid("shortcut must be a string or null"))?,
                    )?
                });
            }
        } else {
            data[k] = v.clone();
        }
    }
    let next: Preferences = serde_json::from_value(data).map_err(|e| invalid(&e.to_string()))?;
    for (value, allowed) in [
        (&next.theme, THEMES),
        (&next.frame_color, FRAME_COLORS),
        (&next.language, LANGUAGES),
        (&next.send_with, &["enter", "shift-enter"][..]),
    ] {
        if !allowed.contains(&value.as_str()) {
            return Err(invalid(&format!("unknown preference value {value}")));
        }
    }
    if next.shortcuts.values().collect::<BTreeSet<_>>().len() != SHORTCUTS.len() {
        return Err(conflict("another command already uses this shortcut"));
    }
    if next == old {
        return Ok(json!({"rev":rev,"preferences":old}));
    }
    c.db.execute("INSERT INTO personal_preferences(actor,rev,settings) VALUES(?1,?2,?3) ON CONFLICT(actor) DO UPDATE SET rev=excluded.rev,settings=excluded.settings",params![c.actor,rev+1,serde_json::to_string(&next).map_err(storage)?]).map_err(storage)?;
    Ok(json!({"rev":rev+1,"preferences":next}))
}

pub(crate) fn extend_schema(s: &mut Value) {
    let shortcut: Value = SHORTCUTS
        .iter()
        .map(|(id, _)| (id.to_string(), json!({"type":["string","null"]})))
        .collect::<serde_json::Map<_, _>>()
        .into();
    s["properties"]["preferences"] = json!({"type":"object","minProperties":1,"additionalProperties":false,"properties":{
        "theme":{"enum":THEMES},"frame_color":{"enum":FRAME_COLORS},"language":{"enum":LANGUAGES},"send_with":{"enum":["enter","shift-enter"]},
        "open_after_assignment":{"type":"boolean"},"open_after_command":{"type":"boolean"},"shortcuts":{"type":"object","minProperties":1,"additionalProperties":false,"properties":shortcut}
    }});
    s["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":"preferences.update"}}},"then":{"required":["preferences"]}}));
}
