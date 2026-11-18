mod cadence;
mod history;

use crate::{Context, Record, Result, Store, conflict, forbidden, invalid, required, storage};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Manual,
    Automatic,
    Disabled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    mode: Mode,
    timezone: String,
    anchor_date: NaiveDate,
    weeks: u32,
    cooldown_days: u32,
    future_count: u32,
    auto_join_active: bool,
    auto_join_in_progress: bool,
    auto_join_completed: bool,
}

impl Settings {
    fn zone(&self) -> Result<Tz> {
        self.timezone
            .parse()
            .map_err(|_| invalid("provide an IANA timezone such as Asia/Shanghai"))
    }
    fn validate(&self) -> Result<()> {
        self.zone()?;
        if !(1..=8).contains(&self.weeks)
            || self.cooldown_days > 56
            || self.future_count > 26
            || !(1000..=9998).contains(&chrono::Datelike::year(&self.anchor_date))
        {
            return Err(invalid(
                "weeks must be 1–8, cooldown_days 0–56, future_count 0–26 and anchor_date a four-digit calendar year",
            ));
        }
        Ok(())
    }
    fn rules(&self) -> bool {
        self.auto_join_active || self.auto_join_in_progress || self.auto_join_completed
    }
    fn cadence_changed(&self, b: &Self) -> bool {
        self.anchor_date != b.anchor_date
            || self.weeks != b.weeks
            || self.cooldown_days != b.cooldown_days
            || self.timezone != b.timezone
    }
}

pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS iteration_policies(space TEXT PRIMARY KEY, config TEXT NOT NULL, rev INTEGER NOT NULL, next_start TEXT, maintained_on TEXT);
        CREATE TABLE IF NOT EXISTS iteration_closures(space TEXT, iteration TEXT, at INTEGER NOT NULL, PRIMARY KEY(space,iteration));
        CREATE TABLE IF NOT EXISTS iteration_scope_history(space TEXT, iteration TEXT, task TEXT, snapshot TEXT NOT NULL, category TEXT NOT NULL, PRIMARY KEY(space,iteration,task));
        CREATE TABLE IF NOT EXISTS iteration_join_overrides(space TEXT,task TEXT,seq INTEGER NOT NULL,PRIMARY KEY(space,task));").map_err(storage)
}

fn policy(c: &Context<'_>) -> Result<(Settings, i64)> {
    let saved: Option<(String, i64)> =
        c.db.query_row(
            "SELECT config,rev FROM iteration_policies WHERE space=?",
            [c.space],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((raw, rev)) = saved {
        return Ok((serde_json::from_str(&raw).map_err(storage)?, rev));
    }
    let space = c.get("space", c.space)?;
    let zone = space.fields["timezone"]
        .as_str()
        .unwrap_or("UTC")
        .parse::<Tz>()
        .unwrap_or(chrono_tz::UTC);
    Ok((
        Settings {
            mode: if space.fields["iterations_enabled"] == false {
                Mode::Disabled
            } else {
                Mode::Manual
            },
            timezone: zone.to_string(),
            weeks: 2,
            cooldown_days: 0,
            future_count: 2,
            anchor_date: Utc::now().with_timezone(&zone).date_naive(),
            auto_join_active: false,
            auto_join_in_progress: false,
            auto_join_completed: false,
        },
        1,
    ))
}

fn settings_record(c: &Context<'_>, s: &Settings, rev: i64) -> Value {
    json!({"kind":"iteration_settings","id":c.space,"space":c.space,"title":"迭代设置","fields":s,"rev":rev,"created_at":0,"updated_at":0})
}

pub(crate) fn local_today(c: &Context<'_>) -> Result<NaiveDate> {
    Ok(Utc::now().with_timezone(&policy(c)?.0.zone()?).date_naive())
}

pub(crate) fn execute(c: &Context<'_>, q: &Value, op: &str) -> Result<Value> {
    let (old, rev) = policy(c)?;
    match op {
        "iteration.settings" => Ok(settings_record(c, &old, rev)),
        "iteration.configure" => {
            c.require_admin()?;
            if q.get("expected_rev")
                .is_some_and(|v| v.as_i64() != Some(rev))
            {
                return Err(conflict("iteration settings revision changed"));
            }
            let patch = q["fields"]
                .as_object()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| invalid("provide iteration settings in fields"))?;
            let mut data = serde_json::to_value(&old).map_err(storage)?;
            data.as_object_mut()
                .expect("settings object")
                .extend(patch.clone());
            let mut s: Settings =
                serde_json::from_value(data).map_err(|e| invalid(&e.to_string()))?;
            if rev == 1 && !patch.contains_key("anchor_date") {
                s.anchor_date = Utc::now().with_timezone(&s.zone()?).date_naive();
            }
            s.validate()?;
            let space = c.get("space", c.space)?;
            if space.fields["deactivated"] == true {
                return Err(forbidden(
                    "reactivate the space to change iteration settings",
                ));
            }
            let enabled = s.mode != Mode::Disabled;
            if (space.fields["iterations_enabled"] != false) != enabled
                || patch.contains_key("timezone") && space.fields["timezone"] != s.timezone
            {
                let mut fields = space.fields.clone();
                fields["iterations_enabled"] = json!(enabled);
                if patch.contains_key("timezone") {
                    fields["timezone"] = json!(s.timezone);
                }
                c.update(&space, &space.title, fields)?;
            }
            configure(c, &old, &s, rev, Utc::now())?;
            Ok(settings_record(c, &s, rev + 1))
        }
        "iteration.start" => cadence::start(c, q, &old),
        "iteration.history" if q.get("id").is_some() => history::read(c, required(q, "id")?),
        "iteration.history" => history::list(c),
        _ => Err(invalid("unknown iteration operation")),
    }
}

fn configure(
    c: &Context<'_>,
    old: &Settings,
    s: &Settings,
    rev: i64,
    at: DateTime<Utc>,
) -> Result<()> {
    let day = at.with_timezone(&s.zone()?).date_naive();
    c.db.execute("INSERT INTO iteration_policies(space,config,rev) VALUES(?,?,?) ON CONFLICT(space) DO UPDATE SET config=excluded.config,rev=excluded.rev,maintained_on=NULL",params![c.space,serde_json::to_string(s).map_err(storage)?,rev+1]).map_err(storage)?;
    match s.mode {
        Mode::Disabled => cadence::disable(c, day)?,
        Mode::Manual if old.mode == Mode::Automatic => cadence::release(c)?,
        Mode::Automatic => {
            if old.mode != Mode::Automatic
                || s.cadence_changed(old)
                || s.future_count < old.future_count
            {
                cadence::reschedule(c, s, day)?;
            }
            cadence::maintain(c, s, day)?;
            if s.rules() {
                history::backfill(c, s, at)?;
            }
        }
        Mode::Manual => {}
    }
    Ok(())
}

pub(crate) fn module_changed(c: &Context<'_>, old: Option<&Record>, row: &Record) -> Result<()> {
    if row.kind != "space" || row.id != c.space {
        return Ok(());
    }
    let Some(before) = old else { return Ok(()) };
    let toggled = (before.fields["iterations_enabled"] == false)
        != (row.fields["iterations_enabled"] == false);
    let timezone_changed = before.fields["timezone"] != row.fields["timezone"];
    if !toggled && !timezone_changed {
        return Ok(());
    }
    c.require_admin()?;
    let (previous, rev) = policy(c)?;
    let mut s = previous.clone();
    if timezone_changed {
        s.timezone = if row.fields["timezone"].is_null() {
            "UTC"
        } else {
            required(&row.fields, "timezone")?
        }
        .to_owned();
        s.validate()?;
    }
    if toggled {
        s.mode = if row.fields["iterations_enabled"] == false {
            Mode::Disabled
        } else {
            Mode::Manual
        };
    }
    if !toggled && rev == 1 {
        return Ok(());
    }
    configure(c, &previous, &s, rev, Utc::now())
}

impl Store {
    /// Advance each configured space from the service clock, including missed local midnights.
    pub fn maintain_iterations(&mut self) -> Result<()> {
        let spaces = {
            let mut stmt=self.db.prepare("SELECT space FROM iteration_policies WHERE json_extract(config,'$.mode')='automatic'").map_err(storage)?;
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?
        };
        let mut result = Ok(());
        for space in spaces {
            if let Err(error) = maintain_space(&mut self.db, &space, Utc::now()) {
                result = Err(error);
            }
        }
        result
    }
}

pub(crate) fn maintain_space(db: &mut Connection, space: &str, at: DateTime<Utc>) -> Result<()> {
    let due = |db: &Connection| -> Result<Option<(Settings, NaiveDate)>> {
        let row:Option<(String,Option<String>)>=db.query_row("SELECT p.config,p.maintained_on FROM iteration_policies p JOIN records s ON s.space=p.space AND s.kind='space' AND s.id=p.space WHERE p.space=? AND json_extract(p.config,'$.mode')='automatic' AND coalesce(json_extract(s.fields,'$.deactivated'),0)=0 AND coalesce(json_extract(s.fields,'$.iterations_enabled'),1)<>0",[space],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
        let Some((config, last)) = row else {
            return Ok(None);
        };
        let s: Settings = serde_json::from_str(&config).map_err(storage)?;
        let day = at.with_timezone(&s.zone()?).date_naive();
        Ok((last.as_deref() != Some(&day.to_string())).then_some((s, day)))
    };
    if due(db)?.is_none() {
        return Ok(());
    }
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    if let Some((s, day)) = due(&tx)? {
        cadence::maintain(
            &Context {
                db: &tx,
                space,
                actor: "owner",
            },
            &s,
            day,
        )?;
    }
    tx.commit().map_err(storage)
}

pub(crate) fn manual_fields(
    c: &Context<'_>,
    old: Option<&Record>,
    row: &mut Record,
    from_source: bool,
) -> Result<()> {
    let (s, rev) = policy(c)?;
    let f = &mut row.fields;
    if old.is_none() && s.mode != Mode::Manual && !from_source {
        return Err(invalid(
            "switch to manual iteration mode to create an iteration",
        ));
    }
    if old.is_none() && f.get("owner").is_none() {
        f["owner"] = json!(c.actor);
    }
    if f.get("owner")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        return Err(invalid("iteration owner must be a member ID or null"));
    }
    if let Some(owner) = f["owner"].as_str()
        && !(owner == "owner" && c.actor == "owner")
        && old.is_none_or(|r| r.fields["owner"] != owner)
    {
        c.readable("member", owner)?;
    }
    if old.is_none() && rev > 1 {
        if f["start_date"].is_null() && f["start"].is_null() {
            f["start_date"] = json!(Utc::now().with_timezone(&s.zone()?).date_naive());
        }
        if f["end_date"].is_null() && f["end"].is_null() {
            let start = date(
                f["start_date"]
                    .as_str()
                    .or(f["start"].as_str())
                    .unwrap_or_default(),
            )?;
            f["end_date"] = json!(start + Duration::days(i64::from(s.weeks) * 7 - 1));
        }
    }
    if let Some(previous) = old {
        if previous.fields["status"] == "closed" && f["status"] != "closed" {
            return Err(invalid("a completed iteration cannot be reopened"));
        }
        if previous.fields["cadence"] == true
            && ["start", "end", "start_date", "end_date", "status", "owner"]
                .iter()
                .any(|k| f[*k] != previous.fields[*k])
        {
            return Err(invalid(
                "change automatic dates through iteration settings or iteration.start",
            ));
        }
    }
    Ok(())
}

fn date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| invalid("expected a date in YYYY-MM-DD format"))
}
#[cfg(test)]
mod tests;
pub(crate) use cadence::remove_manual;
pub(crate) use history::{auto_assign, capture, closed_scope, record_override, supplement};

pub(crate) fn configured_manual(c: &Context<'_>) -> Result<bool> {
    let (s, rev) = policy(c)?;
    Ok(rev > 1 && s.mode == Mode::Manual)
}
pub(crate) fn extend_schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().expect("request rules");
    rules.push(json!({"if":{"properties":{"op":{"const":"iteration.configure"}}},"then":{"required":["fields"],"properties":{"fields":{"type":"object","minProperties":1,"additionalProperties":false,"properties":{
        "mode":{"enum":["manual","automatic","disabled"]},"timezone":{"type":"string","minLength":1},"anchor_date":{"type":"string","format":"date"},
        "weeks":{"type":"integer","minimum":1,"maximum":8},"cooldown_days":{"type":"integer","minimum":0,"maximum":56},"future_count":{"type":"integer","minimum":0,"maximum":26},
        "auto_join_active":{"type":"boolean"},"auto_join_in_progress":{"type":"boolean"},"auto_join_completed":{"type":"boolean"}
    }}}}}));
    rules.push(
        json!({"if":{"properties":{"op":{"const":"iteration.start"}}},"then":{"required":["id"]}}),
    );
    rules.push(json!({"if":{"properties":{"op":{"const":"iteration.timeline"}}},"then":{"properties":{"owner":{"type":"string","minLength":1}}}}));
}
