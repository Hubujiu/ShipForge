//! User-facing rules for credentials issued by a ShipForge team installation.
use super::{Result, identifier, invalid};
use serde_json::Value;

pub struct PersonalTokenSpec<'a> {
    pub name: &'a str,
    pub lifetime: &'a str,
    pub source: &'a str,
    pub expires_at: Option<i64>,
    pub request_id: Option<&'a str>,
}

impl<'a> PersonalTokenSpec<'a> {
    pub fn parse(q: &'a Value, now_seconds: i64) -> Result<Self> {
        if q.get("actor").is_some() || q.get("scope").is_some() {
            return Err(invalid(
                "personal credentials inherit the authenticated identity and permissions",
            ));
        }
        let name = q["label"]
            .as_str()
            .map(str::trim)
            .filter(|s| {
                !s.is_empty() && s.chars().count() <= 100 && !s.chars().any(char::is_control)
            })
            .ok_or_else(|| invalid("a personal token needs a name of 1 to 100 characters"))?;
        let lifetime = q
            .get("lifetime")
            .map_or(Some("30d"), Value::as_str)
            .ok_or_else(|| invalid("invalid token lifetime"))?;
        let days = match lifetime {
            "30d" => Some(30),
            "90d" => Some(90),
            "1y" => Some(365),
            "never" => None,
            _ => return Err(invalid("choose 30d, 90d, 1y or never")),
        };
        let source = match q.get("source").map_or(Some("personal"), Value::as_str) {
            Some("personal") => "personal",
            Some("machine_setup") => "machine_setup",
            _ => return Err(invalid("unknown personal credential source")),
        };
        let request_id = q
            .get("request_id")
            .map(|v| v.as_str().ok_or_else(|| invalid("request_id must be text")))
            .transpose()?;
        if let Some(key) = request_id {
            identifier(key)?;
        }
        Ok(Self {
            name,
            lifetime,
            source,
            expires_at: days.map(|d| now_seconds + d * 86400),
            request_id,
        })
    }
}
