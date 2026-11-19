use super::{After, UiAction, Workbench};
use crate::{catalog::Record, forms::Mode};
use gpui_kit::{Context, Window};
use serde_json::{Value, json};

#[derive(Clone)]
pub enum Action {
    Organize { room: String, action: &'static str },
    Undo,
    Fork { room: String, message: String },
    ForkWorkspace { room: String, message: String },
    Bind { room: Record, task: String },
    Convert(Record),
    Row(Record),
}
pub fn agent_room(r: &Record) -> bool {
    r.field("chat_type") != "group"
        && (!r.field("agent").is_empty() || !r.field("handler_id").is_empty())
}
pub fn organize(room: &Record, action: &'static str) -> UiAction {
    UiAction::Chat(Action::Organize {
        room: room.id.clone(),
        action,
    })
}
impl Workbench {
    pub(super) fn chat_action(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        let key = uuid::Uuid::new_v4().to_string();
        match action {
            Action::Row(room)=>self.open_form(Mode::ChatRow(room),w,cx),
            Action::Organize{room,action}=>self.send(json!({"op":"chat.organize","space":self.loc.space,"id":room,"action":action,"request_id":key}),After::OrganizedChat,true),
            Action::Undo=>{
                if let Some((space,room,token))=&self.chat_undo && space==&self.loc.space {
                    self.send(json!({"op":"chat.organize","space":space,"id":room,"action":"undo","undo_token":token,"request_id":key}),After::OrganizedChat,true);
                }
            }
            Action::Fork{room,message}=>self.send(json!({"op":"chat.fork","space":self.loc.space,"id":room,"message":message,"request_id":key}),After::OpenChat,true),
            Action::ForkWorkspace{room,message}=>{
                let space=self.loc.space.clone();
                self.send(json!({"op":"chat.workspace","space":space,"id":room}),
                    After::ForkWorkspace{room,message,space,revision:self.revision},false);
            }
            Action::Bind{room,task}=>self.send(json!({"op":"chat.bind_task","space":self.loc.space,"id":room.id,"task":task,"expected_rev":room.rev,"request_id":key}),After::OpenChat,true),
            Action::Convert(room)=>self.send(json!({"op":"chat.convert","space":self.loc.space,"id":room.id,"expected_rev":room.rev,"request_id":key}),After::Notice("已向 Agent 提交建单请求".into()),true),
        }
    }
    pub(super) fn fork_options(
        &mut self,
        room: String,
        message: String,
        space: String,
        mut choice: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ready = choice["ready"] == true;
        let targets = choice["targets"]
            .as_array()
            .is_some_and(|items| !items.is_empty());
        if choice["can_reuse"] != true || (!ready && !targets) {
            self.notify_text("当前暂无可接续的工作目录");
            return;
        }
        choice["message"] = json!(message);
        choice["workspace_mode"] = json!(if ready { "reuse" } else { "branch" });
        self.open_form(
            Mode::Workflow {
                op: "chat.fork.workspace",
                record: Record {
                    id: room,
                    space,
                    kind: "chat".into(),
                    fields: choice.as_object().cloned().unwrap_or_default(),
                    ..Default::default()
                },
            },
            w,
            cx,
        );
    }

    pub(super) fn fork_submit(&mut self, record: &Record, values: Value) {
        let mut request = json!({"op":"chat.fork","space":record.space,"id":record.id,"message":record.fields["message"],
            "workspace_mode":values["workspace_mode"],"request_id":uuid::Uuid::new_v4().to_string()});
        if values["workspace_mode"] == "branch" {
            for key in ["checkout_branch", "target_machine_id"] {
                if let Some(value) = values[key]
                    .as_str()
                    .map(str::trim)
                    .filter(|v| !v.is_empty() && *v != "_source")
                {
                    request[key] = json!(value);
                }
            }
        }
        self.send(request, After::OpenChat, true);
    }

    pub(super) fn fork_targets(&self) -> Vec<(String, String)> {
        let Some(crate::forms::Form {
            mode:
                Mode::Workflow {
                    op: "chat.fork.workspace",
                    record,
                },
            ..
        }) = &self.form
        else {
            return Vec::new();
        };
        let mut result = Vec::new();
        if record.fields["ready"] == true {
            result.push(("_source".into(), crate::locale::label("原工作机").into()));
        }
        result.extend(
            record.fields["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    Some((
                        item["id"].as_str()?.to_owned(),
                        item["title"].as_str()?.to_owned(),
                    ))
                }),
        );
        result
    }
    pub(super) fn chat_organized(&mut self, value: &Value, w: &mut Window, cx: &mut Context<Self>) {
        self.close_form(w, cx);
        if let (Some(token), Some(id)) = (value["undo_token"].as_str(), value["id"].as_str()) {
            self.notify_text("已从我的对话列表隐藏");
            self.chat_undo = Some((self.loc.space.clone(), id.to_owned(), token.to_owned()));
        } else {
            self.notify_text("对话列表已更新");
        }
        self.read(false);
    }
}
