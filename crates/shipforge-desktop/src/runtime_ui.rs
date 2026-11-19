use super::*;
use crate::forms::Selection;
use serde_json::{Map, Value, json};

const SWITCHES: [(&str, &str, bool); 3] = [
    ("hours_assignment", "assignment", true),
    ("hours_comment", "comment", true),
    ("hours_chat", "chat", false),
];
pub(super) fn reset_profiles(f: &mut Form, index: usize, next: &str, cx: &App) {
    if f.entries
        .get(index)
        .is_some_and(|e| matches!(e.spec.key, "machine_id" | "directory_mode") && e.raw(cx) != next)
        && let Some(entry) = f.entries.iter_mut().find(|e| e.spec.key == "profiles")
        && let Control::Pick { value, .. } = &mut entry.control
    {
        *value = Selection::Many(vec![]);
    }
}
pub(crate) fn initial(f: &mut Map<String, Value>) {
    let hours = f.get("working_hours").cloned().unwrap_or_default();
    f.insert(
        "hours_mode".into(),
        json!(if hours.is_null() {
            "always"
        } else if hours["preset"] == "off_peak" {
            "off_peak"
        } else {
            "custom"
        }),
    );
    if let Some(zone) = hours["timezone"].as_str() {
        f.insert("hours_zone".into(), json!(zone));
    }
    for (key, trigger, default) in SWITCHES {
        f.insert(
            key.into(),
            json!(hours["triggers"][trigger].as_bool().unwrap_or(default)),
        );
    }
}
pub(crate) fn compile(f: &mut Map<String, Value>) -> Result<(), String> {
    let Some(mode) = f.remove("hours_mode") else {
        return Ok(());
    };
    let zone = f
        .remove("hours_zone")
        .unwrap_or_else(|| json!("Asia/Shanghai"));
    let mut triggers = json!({});
    for (key, trigger, default) in SWITCHES {
        triggers[trigger] = json!(f.remove(key).and_then(|v| v.as_bool()).unwrap_or(default));
    }
    let mut hours = match mode.as_str() {
        Some("off_peak") => json!({"timezone":zone,"preset":"off_peak"}),
        Some("custom") => f
            .remove("working_hours")
            .filter(Value::is_object)
            .ok_or("请填写自定义工作时段")?,
        _ => Value::Null,
    };
    if hours.is_object() {
        hours["triggers"] = triggers;
    }
    f.insert("working_hours".into(), hours);
    Ok(())
}
impl Workbench {
    pub(crate) fn scheduled_banner(&self, r: &Record, cx: &Context<Self>) -> Div {
        use crate::controls::{outlined, row};
        let time = r
            .fields
            .get("next_start_at")
            .and_then(Value::as_i64)
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M %:z")
                    .to_string()
            })
            .unwrap_or_else(|| crate::locale::label("尚未设置可用时段").to_owned());
        let mut banner = row()
            .gap(px(6.))
            .items_center()
            .child(format!("{} · {time}", crate::locale::label("等待工作时段")));
        if self.writable() {
            banner = banner.child(
                outlined(self.button(
                    format!("trigger-run-{}", r.id),
                    "立即触发",
                    UiAction::Apply(
                        json!({"op":"run.trigger_now","id":r.id,"expected_rev":r.rev}),
                        "已进入可执行队列".into(),
                    ),
                    cx,
                ))
                .child(crate::locale::label("立即触发")),
            );
        }
        banner
    }
    pub(super) fn runtime_directory_choices(&self) -> Vec<(String, String)> {
        let Some(f) = self.form.as_ref() else {
            return vec![];
        };
        let choice = |key| {
            f.entries.iter().find_map(|e| {
                if e.spec.key == key
                    && let Control::Pick {
                        value: Selection::One(v),
                        ..
                    } = &e.control
                {
                    Some(v.as_str())
                } else {
                    None
                }
            })
        };
        let machine = choice("machine_id").and_then(|id| self.find("machine", id));
        let mode = if choice("directory_mode") == Some("direct") {
            "direct"
        } else {
            "git"
        };
        machine
            .and_then(|m| m.fields.get("directories"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|d| d["available"] == true && d[mode] == true)
            .filter_map(|d| d["id"].as_str())
            .map(|id| {
                (
                    id.to_owned(),
                    if id == "@workspace" {
                        crate::locale::label("默认工作目录").to_owned()
                    } else {
                        id.to_owned()
                    },
                )
            })
            .collect()
    }
}
