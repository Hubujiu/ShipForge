use super::*;
use chrono::{DateTime, Local};

fn date(value: &Value, empty: &'static str) -> String {
    value
        .as_i64()
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        .map(|utc| {
            utc.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| crate::locale::label(empty).into())
}

impl Workbench {
    pub(super) fn personal_tokens_panel(&self, cx: &Context<Self>) -> Div {
        let mut panel = col().w_full().gap(px(8.));
        panel = panel.child(
            row()
                .w_full()
                .gap(px(8.))
                .child(headline("个人访问令牌", 15.))
                .child(div().flex_1())
                .when(self.access_identity["personal"] == false, |r| {
                    r.child(
                        primary(self.button(
                            "personal-token-new",
                            "创建个人访问令牌",
                            UiAction::NewPersonalToken(false),
                            cx,
                        ))
                        .child(crate::locale::label("创建令牌")),
                    )
                }),
        );
        if self.tokens.is_empty() {
            panel = panel.child(text("没有有效的个人令牌。", 13., MUTED));
        }
        for token in &self.tokens {
            let id = token["id"].as_str().unwrap_or_default();
            let mut item = col()
                .w_full()
                .gap(px(4.))
                .border_b_1()
                .border_color(rgb(LINE))
                .py(px(7.));
            item = item.child(
                row()
                    .gap(px(8.))
                    .child(text(
                        token["label"].as_str().unwrap_or_default().to_owned(),
                        14.,
                        INK,
                    ))
                    .child(text(
                        token["prefix"].as_str().unwrap_or_default().to_owned(),
                        12.,
                        MUTED,
                    ))
                    .child(text(
                        if token["source"] == "machine_setup" {
                            "机器安装"
                        } else {
                            "个人创建"
                        },
                        12.,
                        DIM,
                    ))
                    .child(div().flex_1())
                    .when(token["can_revoke"] == true, |r| {
                        r.child(
                            outlined(self.button(
                                format!("personal-revoke-{id}"),
                                "吊销令牌",
                                UiAction::Form(Mode::Confirm {
                                    label: format!(
                                        "{}「{}」",
                                        crate::locale::label("吊销令牌"),
                                        token["label"].as_str().unwrap_or_default()
                                    ),
                                    command: json!({"op":"access.personal.revoke","id":id}),
                                }),
                                cx,
                            ))
                            .child(crate::locale::label("撤销")),
                        )
                    }),
            );
            let dates = [
                ("创建时间", "created_at", ""),
                ("最近使用", "last_used_at", "从未使用"),
                ("过期时间", "expires_at", "永不过期"),
            ];
            item = item.child(text(
                dates
                    .iter()
                    .map(|(label, key, empty)| {
                        format!(
                            "{} {}",
                            crate::locale::label(label),
                            date(&token[key], empty)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ·  "),
                12.,
                MUTED,
            ));
            if let Some(machine) = token["machine"].as_object() {
                let state = if machine.get("disabled") == Some(&Value::Bool(true)) {
                    "已停用"
                } else if machine.get("online") == Some(&Value::Bool(true)) {
                    "在线"
                } else {
                    "离线"
                };
                item = item.child(text(
                    format!(
                        "{} · {}",
                        machine.get("name").and_then(Value::as_str).unwrap_or(""),
                        crate::locale::label(state)
                    ),
                    12.,
                    MUTED,
                ));
            }
            panel = panel.child(item);
        }
        panel
    }
}
