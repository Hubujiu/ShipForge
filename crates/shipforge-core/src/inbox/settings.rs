use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const CHANNELS: &[&str] = &[
    "desktop", "mobile", "browser", "feishu", "dingtalk", "slack", "lark", "inbox", "email",
];
// Stable event IDs are shared by the producers, the Windows settings and the CLI.
const EVENTS: &[(&str, &str)] = &[
    ("assigned", "assignment"),
    ("unassigned", "assignment"),
    ("assignees_changed", "assignment"),
    ("followed_assignees", "assignment"),
    ("follower_added", "assignment"),
    ("assigned_status", "status"),
    ("followed_status", "status"),
    ("assigned_priority", "status"),
    ("followed_priority", "status"),
    ("assigned_start", "status"),
    ("followed_start", "status"),
    ("assigned_due", "status"),
    ("followed_due", "status"),
    ("assigned_done", "status"),
    ("followed_done", "status"),
    ("comment", "discussion"),
    ("reply", "discussion"),
    ("reaction", "discussion"),
    ("chat", "discussion"),
    ("mention", "mentions"),
    ("chat_mention", "mentions"),
    ("description_mention", "mentions"),
    ("agent_comment", "agent"),
    ("run_failed", "agent"),
    ("run_blocked", "agent"),
    ("run_done", "agent"),
    ("run_handoff", "agent"),
    ("automation_result", "agent"),
    ("triage_duty", "triage"),
    ("task_updated", "task_details"),
];

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Settings {
    rev: i64,
    overrides: BTreeMap<String, BTreeMap<String, bool>>,
    pub(super) landing: String,
    pub(super) clear_statuses: Option<Vec<String>>,
}
fn group(event: &str) -> Option<&'static str> {
    EVENTS
        .iter()
        .find_map(|(id, group)| (*id == event).then_some(*group))
}
impl Settings {
    pub(super) fn load(c: &Context<'_>, actor: &str) -> Result<Self> {
        let raw: Option<String> =
            c.db.query_row(
                "SELECT settings FROM inbox_settings WHERE org=?1 AND actor=?2",
                params![c.org_id()?, actor],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        raw.map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()
            .map(Option::unwrap_or_default)
    }
    pub(super) fn enabled(&self, channel: &str, event: &str) -> bool {
        let off = self.overrides.get(channel);
        let on = |key: &str| off.and_then(|map| map.get(key)).copied().unwrap_or(true);
        on("all") && on(group(event).unwrap_or("discussion")) && on(event)
    }
    fn store(&mut self, c: &Context<'_>) -> Result<()> {
        self.rev += 1;
        c.db.execute("INSERT INTO inbox_settings(org,actor,settings) VALUES(?1,?2,?3) ON CONFLICT(org,actor) DO UPDATE SET settings=excluded.settings",params![c.org_id()?,c.actor,serde_json::to_string(self).map_err(storage)?]).map_err(storage)?;
        Ok(())
    }
    fn view(&self) -> Value {
        json!({"rev":self.rev,"overrides":self.overrides,"channels":CHANNELS,"events":EVENTS,"landing":if self.landing=="important" {"important"} else {"all"},"clear_statuses":self.clear_statuses})
    }
    pub(super) fn remember_cleanup(
        &mut self,
        c: &Context<'_>,
        statuses: Option<Vec<String>>,
    ) -> Result<()> {
        if self.clear_statuses != statuses {
            self.clear_statuses = statuses;
            self.store(c)?;
        }
        Ok(())
    }
}
pub(super) fn execute(c: &Context<'_>, q: &Value, write: bool) -> Result<Value> {
    let mut cfg = Settings::load(c, c.actor)?;
    if !write {
        return Ok(cfg.view());
    }
    if q.get("expected_rev")
        .is_some_and(|rev| rev.as_i64() != Some(cfg.rev))
    {
        return Err(conflict(
            "notification settings changed; reload before saving",
        ));
    }
    if let Some(landing) = q.get("landing") {
        cfg.landing = match landing.as_str() {
            Some("important") => "important",
            Some("other" | "all") => "all",
            _ => return Err(invalid("unknown inbox section")),
        }
        .into();
    } else {
        let channel = required(q, "channel")?;
        if !CHANNELS.contains(&channel) {
            return Err(invalid("unknown notification channel"));
        }
        if q["reset"] == true {
            cfg.overrides.remove(channel);
        } else {
            let key = q["key"].as_str().unwrap_or("all");
            if key != "all"
                && !EVENTS
                    .iter()
                    .any(|(event, group)| key == *event || key == *group)
            {
                return Err(invalid("unknown notification event or group"));
            }
            let enabled = q["enabled"]
                .as_bool()
                .ok_or_else(|| invalid("enabled must be a boolean"))?;
            let values = cfg.overrides.entry(channel.into()).or_default();
            if enabled {
                values.remove(key);
            } else {
                values.insert(key.into(), false);
            }
        }
    }
    cfg.store(c)?;
    Ok(cfg.view())
}
