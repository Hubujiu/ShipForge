use crate::Result;
use chrono::{DateTime, Datelike, FixedOffset, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use std::ops::Range;

pub(super) enum WorkingHours {
    Always,
    Fixed {
        start: u32,
        end: u32,
        offset: FixedOffset,
    },
    Weekly {
        zone: Tz,
        days: [Vec<Range<u32>>; 7],
    },
}

impl WorkingHours {
    pub(super) fn parse(value: &Value) -> Result<Self> {
        if value.is_null() {
            return Ok(Self::Always);
        }
        if let Some(preset) = value.get("preset") {
            if preset != "off_peak"
                || ["week", "start", "end", "utc_offset_minutes"]
                    .iter()
                    .any(|key| value.get(key).is_some())
            {
                return Err("invalid working hours preset".into());
            }
            let zone = value["timezone"]
                .as_str()
                .ok_or("weekly working hours require an IANA timezone")?
                .parse::<Tz>()
                .map_err(|_| "unknown IANA time zone")?;
            let days = std::array::from_fn(|i| {
                if i < 5 {
                    vec![0..480, 1200..1440]
                } else {
                    std::iter::once(0..1440).collect()
                }
            });
            return Ok(Self::Weekly { zone, days });
        }
        let Some(week) = value.get("week") else {
            let hour = |name: &str| {
                value[name]
                    .as_u64()
                    .filter(|h| *h < 24)
                    .map(|h| h as u32)
                    .ok_or("working hours must be hours 0–23")
            };
            let offset = match value.get("utc_offset_minutes") {
                None => 0,
                Some(n) => n
                    .as_i64()
                    .filter(|n| (-840..=840).contains(n))
                    .ok_or("invalid time zone offset")? as i32,
            };
            return Ok(Self::Fixed {
                start: hour("start")?,
                end: hour("end")?,
                offset: FixedOffset::east_opt(offset * 60).ok_or("invalid time zone offset")?,
            });
        };
        if ["start", "end", "utc_offset_minutes"]
            .iter()
            .any(|key| value.get(key).is_some())
        {
            return Err("weekly working hours cannot include a fixed-hour window".into());
        }
        let zone = value["timezone"]
            .as_str()
            .ok_or("weekly working hours require an IANA timezone")?
            .parse::<Tz>()
            .map_err(|_| "unknown IANA timezone")?;
        let mut days: [Vec<Range<u32>>; 7] = Default::default();
        for (key, entries) in week
            .as_object()
            .ok_or("week must be an object indexed by ISO weekday")?
        {
            let day = key
                .parse::<usize>()
                .ok()
                .filter(|d| key.len() == 1 && (1..=7).contains(d))
                .ok_or("weekday must be 1 (Monday) through 7 (Sunday)")?;
            let entries = entries
                .as_array()
                .filter(|entries| entries.len() <= 2)
                .ok_or("each weekday accepts up to two working intervals")?;
            let intervals = &mut days[day - 1];
            for entry in entries {
                let start = minute(&entry["start"], false)?;
                let end = minute(&entry["end"], true)?;
                if start >= end {
                    return Err(
                        "a weekly interval must start before its end on the same weekday".into(),
                    );
                }
                intervals.push(start..end);
            }
            intervals.sort_by_key(|r| r.start);
            if intervals.windows(2).any(|pair| pair[0].end > pair[1].start) {
                return Err("working intervals for a weekday cannot overlap".into());
            }
        }
        Ok(Self::Weekly { zone, days })
    }

    pub(super) fn allows(&self, timestamp: i64) -> bool {
        let Some(utc) = DateTime::<Utc>::from_timestamp_millis(timestamp) else {
            return false;
        };
        match self {
            Self::Always => true,
            Self::Fixed { start, end, offset } => {
                let h = utc.with_timezone(offset).hour();
                if start < end {
                    (*start..*end).contains(&h)
                } else {
                    h >= *start || h < *end
                }
            }
            Self::Weekly { zone, days } => {
                let local = utc.with_timezone(zone);
                let minute = local.hour() * 60 + local.minute();
                days[local.weekday().num_days_from_monday() as usize]
                    .iter()
                    .any(|r| r.contains(&minute))
            }
        }
    }

    pub(super) fn next_start(&self, at: i64) -> Option<i64> {
        if self.allows(at) {
            return Some(at);
        }
        if matches!(self,Self::Weekly{days,..} if days.iter().all(Vec::is_empty)) {
            return None;
        }
        let minute = at.div_euclid(60_000) * 60_000;
        (1..=9 * 24 * 60)
            .map(|n| minute + n * 60_000)
            .find(|t| self.allows(*t))
    }
}

fn minute(value: &Value, end: bool) -> Result<u32> {
    let value = value.as_str().ok_or("working time must be HH:MM")?;
    if end && value == "24:00" {
        return Ok(24 * 60);
    }
    let time = NaiveTime::parse_from_str(value, "%H:%M")
        .ok()
        .filter(|t| t.format("%H:%M").to_string() == value)
        .ok_or("working time must be HH:MM; only an end time can be 24:00")?;
    Ok(time.hour() * 60 + time.minute())
}

pub(super) fn schema() -> Value {
    let hour = json!({"type":"integer","minimum":0,"maximum":23});
    let interval = json!({
        "type":"object", "required":["start","end"],
        "properties":{
            "start":{"type":"string","pattern":"^([01][0-9]|2[0-3]):[0-5][0-9]$"},
            "end":{"type":"string","pattern":"^(([01][0-9]|2[0-3]):[0-5][0-9]|24:00)$"}
        }
    });
    json!({"properties":{"triggers":{"type":"object","additionalProperties":false,"properties":{"assignment":{"type":"boolean"},"comment":{"type":"boolean"},"chat":{"type":"boolean"}}}},"oneOf":[
        {"type":"null"},
        {"type":"object","required":["preset","timezone"],"properties":{"preset":{"const":"off_peak"},"timezone":{"type":"string","minLength":1}},"not":{"anyOf":[{"required":["week"]},{"required":["start"]},{"required":["end"]},{"required":["utc_offset_minutes"]}]}},
        {"type":"object","required":["start","end"],"not":{"required":["week"]},
         "properties":{"start":hour,"end":hour,"utc_offset_minutes":{"type":"integer","minimum":-840,"maximum":840}}},
        {"type":"object","required":["timezone","week"],
         "not":{"anyOf":[{"required":["start"]},{"required":["end"]},{"required":["utc_offset_minutes"]}]},
         "properties":{"timezone":{"type":"string","minLength":1,"description":"IANA time zone"},
                       "week":{"type":"object","propertyNames":{"pattern":"^[1-7]$"},
                               "additionalProperties":{"type":"array","maxItems":2,"items":interval}}}}
    ]})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(when: &str) -> i64 {
        DateTime::parse_from_rfc3339(when)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn weekly_hours_use_local_weekdays_split_shifts_and_exclusive_ends() {
        let h = WorkingHours::parse(&json!({"timezone":"Asia/Shanghai","week":{
            "1":[{"start":"21:30","end":"24:00"},{"start":"09:00","end":"12:15"}],
            "2":[{"start":"00:00","end":"01:30"}], "3":[]
        }}))
        .unwrap();
        for instant in [
            "2026-11-16T01:00:00Z",
            "2026-11-16T13:30:00Z",
            "2026-11-16T16:00:00Z",
        ] {
            assert!(h.allows(at(instant)), "{instant}");
        }
        for instant in [
            "2026-11-16T04:15:00Z",
            "2026-11-16T17:30:00Z",
            "2026-11-18T01:00:00Z",
            "2026-11-19T01:00:00Z",
        ] {
            assert!(!h.allows(at(instant)), "{instant}");
        }
    }

    #[test]
    fn spring_gap_and_both_occurrences_of_a_fall_minute_follow_local_clock() {
        let h = WorkingHours::parse(&json!({"timezone":"America/New_York","week":{
            "7":[{"start":"01:30","end":"02:30"},{"start":"03:30","end":"04:00"}]
        }}))
        .unwrap();
        assert!(h.allows(at("2026-03-08T06:30:00Z")));
        assert!(!h.allows(at("2026-03-08T07:00:00Z")));
        assert!(h.allows(at("2026-03-08T07:30:00Z")));
        assert!(h.allows(at("2026-11-01T05:45:00Z")));
        assert!(h.allows(at("2026-11-01T06:45:00Z")));
        assert!(!h.allows(at("2026-11-01T07:30:00Z")));
    }

    #[test]
    fn legacy_fixed_hours_preserve_overnight_windows_and_equal_endpoints() {
        let h = WorkingHours::parse(&json!({"start":21,"end":3,"utc_offset_minutes":480})).unwrap();
        assert!(h.allows(at("2026-11-16T13:00:00Z")));
        assert!(h.allows(at("2026-11-16T18:59:00Z")));
        assert!(!h.allows(at("2026-11-16T19:00:00Z")));
        let all = WorkingHours::parse(&json!({"start":0,"end":0})).unwrap();
        assert!(all.allows(at("2026-11-16T18:59:00Z")));
    }

    #[test]
    fn default_off_peak_schedule_has_exclusive_ends_and_returns_the_next_opening() {
        let hours =
            WorkingHours::parse(&json!({"preset":"off_peak","timezone":"Asia/Shanghai"})).unwrap();
        let morning = at("2026-11-16T00:00:00Z");
        let evening = at("2026-11-16T12:00:00Z");
        assert!(hours.allows(morning - 1));
        assert!(!hours.allows(morning));
        assert_eq!(hours.next_start(morning), Some(evening));
        assert_eq!(
            hours.next_start(at("2026-11-21T05:00:00Z")),
            Some(at("2026-11-21T05:00:00Z"))
        );
    }

    #[test]
    fn weekly_hours_reject_invalid_ranges_and_an_unknown_zone() {
        for week in [
            json!({"0":[]}),
            json!({"01":[]}),
            json!({"1":[{"start":"10:00","end":"10:00"}]}),
            json!({"1":[{"start":"21:15","end":"07:00"}]}),
            json!({"1":[{"start":"9:00","end":"12:00"}]}),
            json!({"1":[{"start":"24:00","end":"24:01"}]}),
            json!({"1":[{"start":"16:00","end":"19:00"},{"start":"15:00","end":"17:30"}]}),
            json!({"1":[{"start":"00:00","end":"01:00"},{"start":"01:00","end":"02:00"},{"start":"02:00","end":"03:00"}]}),
        ] {
            assert!(WorkingHours::parse(&json!({"week":week,"timezone":"UTC"})).is_err());
        }
        for shape in [
            json!({"week":{},"timezone":"Missing/Zone"}),
            json!({"week":{}}),
            json!({"start":0,"end":23,"timezone":"UTC","week":{}}),
        ] {
            assert!(WorkingHours::parse(&shape).is_err());
        }
    }
}
