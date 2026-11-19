use super::*;
use crate::{
    appearance::paint,
    controls::{ERROR, LINE, headline},
    locale::label,
};

impl Workbench {
    pub(super) fn storage_health_panel(&self, data: &Value, cx: &Context<Self>) -> Div {
        let health = &data["health"];
        let state = match health["state"].as_str() {
            Some("quota") => "接近托管容量上限",
            Some("low_disk") => "磁盘可用空间过低",
            Some("quota_and_low_disk") => "托管容量和可用磁盘空间均已触发提醒",
            Some("ok") => "存储空间正常",
            _ => "尚无完整磁盘统计",
        };
        let mut body = col()
            .w_full()
            .gap(px(6.))
            .p(px(10.))
            .border_1()
            .border_color(paint(LINE))
            .rounded(px(7.))
            .child(headline("存储健康", 14.))
            .child(
                text(
                    state,
                    13.,
                    if health["pressure"] == true {
                        ERROR
                    } else {
                        INK
                    },
                )
                .id("storage-health-state")
                .role(Role::Status)
                .aria_label(label(state).to_owned()),
            );
        for (name, value) in [
            ("磁盘可用", health["available_bytes"].as_u64()),
            (
                "托管容量上限",
                health["quota_bytes"].as_u64().filter(|n| *n > 0),
            ),
            ("符合保留策略的容量", data["policy_eligible_bytes"].as_u64()),
        ] {
            if let Some(bytes) = value {
                body = body.child(row().gap(px(7.)).child(text(name, 12., MUTED)).child(text(
                    format_bytes(bytes),
                    13.,
                    INK,
                )));
            }
        }
        if data["stale"] == true && data["scanned_at"].is_number() {
            body = body.child(text("显示上一次统计，磁盘状态可能已变化。", 12., MUTED));
        }
        body = body.child(text(
            if data["policy"]["enabled"] == true {
                "已启用定时清理"
            } else {
                "仅手动清理"
            },
            12.,
            MUTED,
        ));
        if self.access_identity["scope"] != "read" {
            body = body.child(
                row().child(
                    outlined(self.button(
                        "machine-storage-policy",
                        "存储配额与清理策略",
                        UiAction::Form(Mode::MachineManagement(Dialog::StoragePolicy(
                            data.clone(),
                        ))),
                        cx,
                    ))
                    .child(label("存储配额与清理策略")),
                ),
            );
        }
        body
    }
}
