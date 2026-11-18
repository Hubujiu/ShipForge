use crate::Result;
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::Value;

pub(super) const BATCH: usize = 64;
pub(super) enum Schedule {
    Interval(i64),
    Calendar { at: NaiveTime, zone: Tz, days: u8 },
}
pub(super) struct Occurrence {
    pub(super) at: i64,
    pub(super) event: String,
}
impl Schedule {
    pub(super) fn parse(t: &Value) -> Result<Self> {
        match t["frequency"].as_str().unwrap_or("interval") {
            "interval" => {
                let minutes = t["interval_minutes"]
                    .as_i64()
                    .filter(|v| (1..=525600).contains(v))
                    .ok_or("scheduled interval must be 1 to 525600 minutes")?;
                Ok(Self::Interval(minutes * 60_000))
            }
            kind @ ("daily" | "weekly") => {
                let local = t["local_time"]
                    .as_str()
                    .filter(|s| s.len() == 5)
                    .ok_or("local_time must be HH:MM")?;
                let at = NaiveTime::parse_from_str(local, "%H:%M")
                    .map_err(|_| "local_time must be HH:MM")?;
                let zone = t["timezone"]
                    .as_str()
                    .ok_or("calendar trigger requires an IANA timezone")?
                    .parse::<Tz>()
                    .map_err(|_| "unknown IANA timezone")?;
                let mut days = 127;
                if kind == "weekly" {
                    let weekdays = t["weekdays"]
                        .as_array()
                        .filter(|v| !v.is_empty() && v.len() <= 7)
                        .ok_or("weekly triggers require weekdays 1 (Monday) through 7 (Sunday)")?;
                    days = 0;
                    for day in weekdays {
                        let n = day
                            .as_u64()
                            .filter(|v| (1..=7).contains(v))
                            .ok_or("weekday must be 1 to 7")?;
                        let bit = 1 << (n - 1);
                        if (days & bit) != 0 {
                            return Err("a weekly trigger cannot repeat a weekday".into());
                        }
                        days |= bit;
                    }
                }
                Ok(Self::Calendar { at, zone, days })
            }
            _ => Err("unknown schedule frequency".into()),
        }
    }
    pub(super) fn signature(&self, t: &Value) -> String {
        let id = t["id"].as_str().unwrap_or("");
        let definition = match self {
            Self::Interval(ms) => format!("interval/{ms}"),
            Self::Calendar { at, zone, days } => format!("calendar/{zone}/{at}/{days}"),
        };
        super::digest(&serde_json::json!([id, definition]).to_string())
    }
    pub(super) fn start_cursor(&self, since: i64) -> i64 {
        match self {
            Self::Interval(ms) => since.div_euclid(*ms) * ms - 1,
            Self::Calendar { .. } => since.saturating_sub(1),
        }
    }
    pub(super) fn occurrences(
        &self,
        after: i64,
        through: i64,
        idx: usize,
        key: &str,
    ) -> Result<Vec<Occurrence>> {
        let mut out = Vec::new();
        if through <= after {
            return Ok(out);
        }
        match self {
            Self::Interval(ms) => {
                let mut bucket = after.div_euclid(*ms) + 1;
                while bucket * ms <= through && out.len() < BATCH {
                    out.push(Occurrence {
                        at: bucket * ms,
                        event: format!("schedule:{idx}:{ms}:{bucket}"),
                    });
                    bucket += 1;
                }
            }
            Self::Calendar { at, zone, days } => {
                let date_at = |timestamp| {
                    DateTime::<Utc>::from_timestamp_millis(timestamp)
                        .map(|t| t.with_timezone(zone).date_naive())
                        .ok_or("schedule date out of range")
                };
                let mut date = date_at(after)?;
                let last = date_at(through)?;
                while date <= last && out.len() < BATCH {
                    if days & (1 << date.weekday().num_days_from_monday()) != 0
                        && let Some(instant) = instant_on(date, *at, *zone)
                        && instant > after
                        && instant <= through
                    {
                        out.push(Occurrence {
                            at: instant,
                            event: format!("calendar:{key}:{date}"),
                        });
                    }
                    date = date.succ_opt().ok_or("schedule date out of range")?;
                }
            }
        }
        Ok(out)
    }
}
fn instant_on(date: NaiveDate, at: NaiveTime, zone: Tz) -> Option<i64> {
    let mut local = date.and_time(at);
    loop {
        match zone.from_local_datetime(&local) {
            LocalResult::Single(t) => return Some(t.timestamp_millis()),
            LocalResult::Ambiguous(a, b) => return Some(a.min(b).timestamp_millis()),
            LocalResult::None => {
                local = local.checked_add_signed(Duration::minutes(1))?;
                if local.date() != date {
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn ts(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }
    #[test]
    fn calendar_handles_dst_cross_midnight_and_weekday_selection() {
        let spring = Schedule::parse(
            &json!({"frequency":"daily","timezone":"America/New_York","local_time":"02:30"}),
        )
        .unwrap();
        let due = spring
            .occurrences(
                ts("2026-03-07T12:00:00Z"),
                ts("2026-03-09T12:00:00Z"),
                0,
                "x",
            )
            .unwrap();
        assert_eq!(
            due.iter().map(|d| d.at).collect::<Vec<_>>(),
            [ts("2026-03-08T07:00:00Z"), ts("2026-03-09T06:30:00Z")]
        );
        let fall = Schedule::parse(
            &json!({"frequency":"daily","timezone":"America/New_York","local_time":"01:30"}),
        )
        .unwrap();
        let due = fall
            .occurrences(
                ts("2026-11-01T00:00:00Z"),
                ts("2026-11-01T10:00:00Z"),
                0,
                "x",
            )
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].at, ts("2026-11-01T05:30:00Z"));
        let weekly=Schedule::parse(&json!({"frequency":"weekly","timezone":"Asia/Shanghai","local_time":"00:15","weekdays":[1,4]})).unwrap();
        let due = weekly
            .occurrences(
                ts("2026-11-15T15:00:00Z"),
                ts("2026-11-19T00:00:00Z"),
                0,
                "x",
            )
            .unwrap();
        assert_eq!(
            due.iter().map(|d| d.at).collect::<Vec<_>>(),
            [ts("2026-11-15T16:15:00Z"), ts("2026-11-18T16:15:00Z")]
        );
    }
    #[test]
    fn missed_calendar_runs_are_caught_up_once_across_worker_restarts() {
        use crate::{Context, Store};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("calendar.sqlite3");
        let mut store = Store::open(&path).unwrap();
        store
            .execute(
                "owner",
                json!({"op":"create","kind":"agent","id":"a","title":"A","fields":{}}),
            )
            .unwrap();
        store.execute("owner",json!({"op":"create","kind":"automation","id":"calendar","title":"Daily","fields":{"enabled":true,"handler_id":"a","instruction":"Prepare daily report","triggers":[{"type":"schedule","frequency":"daily","timezone":"America/Los_Angeles","local_time":"09:15"}]}})).unwrap();
        let last = ts("2026-03-06T18:00:00Z");
        let through = ts("2026-03-10T18:00:00Z");
        store
            .db
            .execute("UPDATE sf_schedule_cursors SET seen=?", [last])
            .unwrap();
        let tick = |store: &mut Store| {
            let tx = store.db.transaction().unwrap();
            let c = Context {
                db: &tx,
                space: "default",
                actor: "owner",
            };
            let result = super::super::tick_at(&c, through).unwrap();
            tx.commit().unwrap();
            result
        };
        assert_eq!(tick(&mut store)["runs"].as_array().unwrap().len(), 4);
        drop(store);
        let mut restarted = Store::open(&path).unwrap();
        tick(&mut restarted);
        let result = restarted
            .execute("owner", json!({"op":"list","kind":"run"}))
            .unwrap();
        let mut times = result
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["fields"]["scheduled_for"].as_i64().unwrap())
            .collect::<Vec<_>>();
        times.sort_unstable();
        assert_eq!(
            times,
            [
                ts("2026-03-07T17:15:00Z"),
                ts("2026-03-08T16:15:00Z"),
                ts("2026-03-09T16:15:00Z"),
                ts("2026-03-10T16:15:00Z")
            ]
        );
    }
}
