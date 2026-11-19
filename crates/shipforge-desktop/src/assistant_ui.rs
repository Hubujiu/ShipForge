use super::{UiAction, Workbench};
use crate::{
    catalog::{Page, Record},
    controls::*,
    forms::Mode,
    locale,
};
use gpui_kit::{base::TestSupportExt, prelude::*, *};
use serde_json::{Map, Value, json};

pub(crate) fn avatar(value: Option<&Value>, size: f32) -> Option<AnyElement> {
    let data = value?.get("icon")?.as_str()?;
    let image = Image::from_bytes(ImageFormat::Png, shipforge_client::png_data_url(data)?);
    Some(
        div()
            .id("chat-group-avatar")
            .test_support()
            .role(Role::Image)
            .aria_label(locale::label("群头像"))
            .size(px(size))
            .flex_shrink_0()
            .rounded(px(size / 2.))
            .overflow_hidden()
            .child(img(std::sync::Arc::new(image)).size(px(size)))
            .into_any_element(),
    )
}

impl Workbench {
    pub(super) fn assistant_runtimes(&self) -> Vec<(String, String)> {
        self.list("assistant_home")
            .first()
            .and_then(|r| r.fields.get("runtimes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|v| v["online"] == true)
            .filter_map(|v| v["id"].as_str())
            .map(|name| (name.into(), name.into()))
            .collect()
    }
    pub(super) fn assistant_values(&self, room: &Record, cx: &App) -> Map<String, Value> {
        let mut fields = Map::new();
        let home = self.list("assistant_home").first();
        let runtime = room.field("runtime");
        let room_online = self
            .assistant_runtimes()
            .iter()
            .any(|(id, _)| id == runtime);
        let defaults = if room.kind == "chat" && room_online {
            Some(room)
        } else {
            home
        };
        if let Some(defaults) = defaults {
            for key in ["model", "runtime"] {
                if let Some(value) = defaults.fields.get(key) {
                    fields.insert(key.to_owned(), value.clone());
                }
            }
        }
        if room.kind != "chat" {
            fields.insert("body".into(), json!(self.assistant_draft.value(cx)));
        }
        fields
    }
    pub(crate) fn assistant_page(&mut self, w: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let home = self
            .list("assistant_home")
            .first()
            .cloned()
            .unwrap_or_default();
        let writable = self.writable();
        let draft = self.assistant_draft.clone();
        let mut panel = col()
            .id("personal-assistant")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(20.))
            .gap(px(12.));
        if writable {
            panel = panel.child(self.field(
                "personal-assistant-body",
                locale::label("给小助手发消息"),
                &draft,
                w,
                cx,
            ));
            let runtime = home.field("runtime");
            let mut tools = row().gap(px(8.));
            if runtime.is_empty() {
                tools = tools
                    .child(text("连接自己的工作机后即可发送消息。", 13., MUTED))
                    .child(
                        outlined(self.button(
                            "assistant-machines",
                            "工作机",
                            UiAction::Go(Page::Machines),
                            cx,
                        ))
                        .child(locale::label("工作机")),
                    );
            } else {
                let enabled = !draft.value(cx).trim().is_empty() && !self.busy;
                tools = tools
                    .child(
                        outlined(self.button(
                            "assistant-options",
                            "选择 Runtime 和模型",
                            UiAction::Form(Mode::Workflow {
                                op: "assistant.send",
                                record: home.clone(),
                            }),
                            cx,
                        ))
                        .child(runtime.to_owned()),
                    )
                    .child(
                        primary(self.button(
                            "assistant-send",
                            "发送",
                            UiAction::SendAssistantDraft,
                            cx,
                        ))
                        .disabled(!enabled)
                        .child(locale::label("发送")),
                    );
            }
            panel = panel.child(tools);
        }
        let recent = home
            .fields
            .get("rooms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|raw| serde_json::from_value::<Record>(raw.clone()).ok())
            .collect::<Vec<_>>();
        if !recent.is_empty() {
            panel = panel.child(headline("最近对话", 14.));
        }
        for room in recent {
            let label = room.title.clone();
            panel = panel.child(
                self.button(
                    format!("assistant-recent-{}", room.id),
                    label.clone(),
                    UiAction::OpenSearch(room),
                    cx,
                )
                .w_full()
                .justify_start()
                .child(icon("messages-square", 14.))
                .child(text(label, 13., INK).truncate()),
            );
        }
        panel.into_any_element()
    }
}
