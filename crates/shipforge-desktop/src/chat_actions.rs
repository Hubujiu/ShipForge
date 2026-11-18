use super::{After, UiAction, Workbench};
use crate::{catalog::Record, forms::Mode};
use gpui_kit::{Context, Window};
use serde_json::{Value, json};

#[derive(Clone)]
pub enum Action {
    Organize { room: String, action: &'static str },
    Undo,
    Fork { room: String, message: String },
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
            Action::Bind{room,task}=>self.send(json!({"op":"chat.bind_task","space":self.loc.space,"id":room.id,"task":task,"expected_rev":room.rev,"request_id":key}),After::OpenChat,true),
            Action::Convert(room)=>self.send(json!({"op":"chat.convert","space":self.loc.space,"id":room.id,"expected_rev":room.rev,"request_id":key}),After::Notice("已向 Agent 提交建单请求".into()),true),
        }
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
