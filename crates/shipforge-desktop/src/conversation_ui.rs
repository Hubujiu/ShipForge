use super::*;
use crate::{
    controls::{self, Field},
    forms::{Format, Spec, item},
};
#[path = "conversation_ui/panels.rs"]
mod panels;

#[derive(Default)]
pub struct State {
    task: Option<(String, String)>,
    catalog: Value,
    history: Value,
    more: bool,
    manage: bool,
    slash: Option<(String, String)>,
    options: Value,
    dismissed: Option<(String, String)>,
    embeds: HashMap<(String, String, String), Value>,
    outline: Option<(String, String, Value)>,
    outline_manual: bool,
}
impl State {
    pub(super) fn clear_embed(&mut self, key: (String, String, String)) {
        self.embeds.remove(&key);
    }
    pub(super) fn clear_outline(&mut self) {
        self.outline = None;
    }
}
#[derive(Clone)]
pub enum Action {
    Shortcuts(Record),
    More,
    Manage,
    Run(String, String),
    Organize(String, Value),
    Resume(String),
    Insert(String),
    Retry,
    ShareDocument(String),
    Open(String),
    Embeds(Record),
    Outline(Record),
}
#[derive(Clone)]
pub enum CommandForm {
    Shortcuts(Record),
    Edit(Option<Value>),
    Image(String),
    OpenDocument,
}
impl CommandForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Shortcuts(_) => "任务快捷指令",
            Self::Edit(Some(_)) => "编辑快捷指令",
            Self::Edit(None) => "新建快捷指令",
            Self::Image(_) => "保存分享图片",
            Self::OpenDocument => "打开文档链接",
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::Shortcuts(_) => "关闭",
            Self::Edit(_) | Self::Image(_) => "保存",
            Self::OpenDocument => "打开",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        use Format::*;
        match self {
            Self::Shortcuts(_) => vec![],
            Self::Edit(_) => vec![
                item("title", "名称", Text, ""),
                item("executor", "Agent 或小队", Ref("$executors"), ""),
                item("prompt", "提示词", Body, ""),
                item(
                    "visibility",
                    "可见范围",
                    Choice(&[("private", "仅自己"), ("space", "空间成员")]),
                    "private",
                ),
            ],
            Self::Image(_) => vec![item("path", "保存到 PNG 文件", Text, "")],
            Self::OpenDocument => vec![item("link", "文档链接", Text, "")],
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        let mut m = Map::new();
        if let Self::Edit(Some(record)) = self {
            for k in ["title", "prompt", "visibility"] {
                m.insert(k.into(), record[k].clone());
            }
            m.insert(
                "executor".into(),
                json!(format!(
                    "{}:{}",
                    record["handler_kind"].as_str().unwrap_or(""),
                    record["handler_id"].as_str().unwrap_or("")
                )),
            );
        }
        m
    }
}
pub enum Reply {
    Catalog(String),
    History(String),
    Run,
    Saved,
    Slash(String, String),
    Shared,
    Opened,
    Embeds((String, String, String), u64),
    OutlineLink(Record),
    Outline(Record),
}
impl Workbench {
    pub fn open_document_link(&mut self, link: String) {
        self.send(
            json!({"op":"document.open","link":link}),
            After::Conversation(Reply::Opened),
            false,
        );
    }
    fn shortcut_catalog(&mut self, task: &str) {
        self.send(
            json!({"op":"shortcut.catalog","space":self.loc.space}),
            After::Conversation(Reply::Catalog(task.into())),
            false,
        );
        self.send(
            json!({"op":"shortcut.history","space":self.loc.space,"task":task}),
            After::Conversation(Reply::History(task.into())),
            false,
        );
    }
    pub fn conversation_refresh(&mut self) {
        if let Some((space, task)) = self.communication.task.clone()
            && space == self.loc.space
            && self
                .form
                .as_ref()
                .is_some_and(|f| matches!(f.mode, Mode::Conversation(CommandForm::Shortcuts(_))))
        {
            self.shortcut_catalog(&task);
        }
        let Some(selected) = self.loc.selected.clone() else {
            self.communication.embeds.clear();
            self.communication.outline = None;
            return;
        };
        let mut keep = vec![(
            selected.space.clone(),
            selected.kind.clone(),
            selected.id.clone(),
        )];
        if selected.kind == "task" {
            keep.extend(
                self.list("comment")
                    .iter()
                    .filter(|r| r.field("task") == selected.id)
                    .map(|r| (r.space.clone(), r.kind.clone(), r.id.clone())),
            );
        }
        self.communication.embeds.retain(|k, _| keep.contains(k));
        if self
            .communication
            .outline
            .as_ref()
            .is_some_and(|(s, id, _)| {
                selected.kind != "document" || s != &selected.space || id != &selected.id
            })
        {
            self.communication.outline = None;
            self.communication.outline_manual = false;
        }
        for key in self
            .communication
            .embeds
            .keys()
            .cloned()
            .collect::<Vec<_>>()
        {
            self.fetch_embeds(key);
        }
        if self.communication.outline.is_some() {
            let doc = selected.clone();
            self.request_outline(doc);
        }
    }
    pub fn discover_document_links(&mut self) {
        let Some(source) = self.loc.selected.as_ref() else {
            return;
        };
        let has_reference = |r: &Record| {
            r.field("description").contains("shipforge://document/")
                || r.field("body").contains("shipforge://document/")
        };
        let records = std::iter::once(source).chain(
            self.list("comment")
                .iter()
                .filter(|r| source.kind == "task" && r.field("task") == source.id),
        );
        let new: Vec<_> = records
            .filter(|r| {
                matches!(r.kind.as_str(), "task" | "document" | "comment") && has_reference(r)
            })
            .take(32)
            .map(|r| (r.space.clone(), r.kind.clone(), r.id.clone()))
            .filter(|k| !self.communication.embeds.contains_key(k))
            .collect();
        if source.kind == "document"
            && self.communication.outline.is_none()
            && (source.field("body").len() >= 1500 || source.field("body").lines().count() >= 24)
        {
            self.request_outline(source.clone());
        }
        for key in new {
            self.communication.embeds.insert(key.clone(), Value::Null);
            self.fetch_embeds(key);
        }
    }
    fn fetch_embeds(&mut self, key: (String, String, String)) {
        self.send(
            json!({"op":"document.embeds","space":key.0,"kind":key.1,"id":key.2}),
            After::Conversation(Reply::Embeds(key, self.generation)),
            false,
        );
    }
    fn request_outline(&mut self, doc: Record) {
        self.send(
            json!({"op":"document.share","space":doc.space,"id":doc.id}),
            After::Conversation(Reply::OutlineLink(doc)),
            false,
        );
    }
    pub fn communication_action(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Shortcuts(task) => {
                self.communication.task = Some((task.space.clone(), task.id.clone()));
                self.communication.catalog = Value::Null;
                self.communication.history = Value::Null;
                self.communication.more = false;
                self.communication.manage = false;
                self.open_form(
                    Mode::Conversation(CommandForm::Shortcuts(task.clone())),
                    w,
                    cx,
                );
                self.shortcut_catalog(&task.id);
            }
            Action::More => self.communication.more = !self.communication.more,
            Action::Manage => self.communication.manage = !self.communication.manage,
            Action::Run(id, task) => {
                let request = json!({
                    "op": "shortcut.run", "space": self.loc.space, "id": id, "task": task,
                    "request_id": uuid::Uuid::new_v4().to_string()
                });
                self.send(request, After::Conversation(Reply::Run), true);
            }
            Action::Organize(id, mut change) => {
                change["op"] = json!("shortcut.organize");
                change["id"] = json!(id);
                change["space"] = json!(self.loc.space);
                self.send(change, After::Conversation(Reply::Saved), true);
            }
            Action::Resume(id) => self.send(
                json!({"op":"get","kind":"chat","space":self.loc.space,"id":id}),
                After::OpenChat,
                false,
            ),
            Action::Insert(name) => self.insert_slash(&name, w, cx),
            Action::Retry => {
                self.communication.slash = None;
                self.communication.dismissed = None;
                self.slash_tick(cx);
            }
            Action::Open(link) => self.open_document_link(link),
            Action::ShareDocument(id) => self.send(
                json!({"op":"document.share","space":self.loc.space,"id":id}),
                After::Conversation(Reply::Shared),
                false,
            ),
            Action::Embeds(source) => {
                let key = (source.space, source.kind, source.id);
                self.communication.embeds.insert(key.clone(), Value::Null);
                self.fetch_embeds(key);
            }
            Action::Outline(doc) => {
                self.communication.outline_manual = true;
                self.request_outline(doc);
            }
        }
        cx.notify();
    }
    pub fn communication_reply(
        &mut self,
        reply: Reply,
        v: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = |r: &Record| {
            self.loc
                .selected
                .as_ref()
                .is_some_and(|d| d.kind == r.kind && d.space == r.space && d.id == r.id)
        };
        match reply {
            Reply::Catalog(ref task) | Reply::History(ref task)
                if !self
                    .communication
                    .task
                    .as_ref()
                    .is_some_and(|(s, t)| s == &self.loc.space && t == task) => {}
            Reply::Catalog(_) => self.communication.catalog = v,
            Reply::History(_) => self.communication.history = v,
            Reply::Saved => {
                if let Some(task) = self.loc.selected.clone().filter(|r| r.kind == "task") {
                    self.open_form(
                        Mode::Conversation(CommandForm::Shortcuts(task.clone())),
                        w,
                        cx,
                    );
                    self.shortcut_catalog(&task.id);
                }
            }
            Reply::Run => {
                if let Some(id) = v["chat"].as_str() {
                    self.send(
                        json!({"op":"get","kind":"chat","space":self.loc.space,"id":id}),
                        After::OpenChat,
                        false,
                    );
                }
            }
            Reply::Slash(room, query) => {
                if self.communication.slash.as_ref() == Some(&(room, query)) {
                    self.communication.options = v;
                }
            }
            Reply::Shared => {
                if let Some(s) = v["link"].as_str() {
                    cx.write_to_clipboard(ClipboardItem::new_string(s.into()));
                    self.notify_text("已复制文档链接");
                }
            }
            Reply::Opened => {
                if let Ok(doc) = serde_json::from_value::<Record>(v) {
                    self.close_form(w, cx);
                    if doc.space != self.loc.space {
                        let space =
                            self.find("space", &doc.space)
                                .cloned()
                                .unwrap_or_else(|| Record {
                                    id: doc.space.clone(),
                                    ..Default::default()
                                });
                        self.switch_space(space);
                    }
                    self.open(doc, true);
                }
            }
            Reply::Embeds(key, generation) => {
                if generation == self.generation && self.communication.embeds.contains_key(&key) {
                    self.communication.embeds.insert(key, v);
                }
            }
            Reply::OutlineLink(doc) if selected(&doc) => {
                self.send(
                    json!({"op":"document.preview","link":v["link"]}),
                    After::Conversation(Reply::Outline(doc)),
                    false,
                );
            }
            Reply::Outline(doc) if selected(&doc) => {
                self.communication.outline = Some((doc.space, doc.id, v["outline"].clone()))
            }
            _ => {}
        }
        cx.notify();
    }
    pub fn communication_submit(
        &mut self,
        mode: CommandForm,
        values: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match mode {
            CommandForm::Shortcuts(_) => self.close_form(w, cx),
            CommandForm::OpenDocument => {
                self.open_document_link(values["link"].as_str().unwrap_or("").into())
            }
            CommandForm::Image(key) => {
                let path = PathBuf::from(values["path"].as_str().unwrap_or(""));
                if !path.is_absolute() {
                    return Err("请选择完整文件路径。".into());
                }
                let ticket =
                    self.register(After::Notice(format!("已保存到 {}", path.display())), true);
                self.backend
                    .sender
                    .send(Request::Attachment {
                        ticket,
                        command: json!({"op":"chat.share.image","share":key}),
                        path,
                    })
                    .map_err(|e| e.to_string())?;
            }
            CommandForm::Edit(existing) => {
                let (kind, id) = values["executor"]
                    .as_str()
                    .and_then(|s| s.split_once(':'))
                    .ok_or("请选择 Agent 或小队。")?;
                let mut q = json!({"op":"shortcut.save","space":self.loc.space,"title":values["title"],"prompt":values["prompt"].as_str().unwrap_or(""),"handler_kind":kind,"handler_id":id,"visibility":values["visibility"],"request_id":uuid::Uuid::new_v4().to_string()});
                if let Some(d) = existing {
                    q["id"] = d["id"].clone();
                    q["expected_rev"] = d["rev"].clone();
                }
                self.send(q, After::Conversation(Reply::Saved), true);
            }
        }
        Ok(())
    }
    pub fn slash_tick(&mut self, cx: &mut Context<Self>) {
        let active = self.form.as_ref().and_then(|f| {
            let Mode::Message(room) = &f.mode else {
                return None;
            };
            if !chat_actions::agent_room(room) {
                return None;
            }
            let input = f.entries.iter().find(|e| e.spec.key == "body")?.raw(cx);
            (input.starts_with('/') && !input.chars().any(char::is_whitespace))
                .then(|| (room.id.clone(), input))
        });
        let Some((room, query)) = active else {
            self.communication.slash = None;
            self.communication.dismissed = None;
            self.communication.options = Value::Null;
            return;
        };
        if [
            self.communication.slash.as_ref(),
            self.communication.dismissed.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|(id, q)| id == &room && q == &query)
        {
            return;
        }
        self.communication.slash = Some((room.clone(), query.clone()));
        self.communication.dismissed = None;
        self.communication.options = Value::Null;
        self.send(
            json!({"op":"chat.slash","space":self.loc.space,"id":room,"query":query}),
            After::Conversation(Reply::Slash(room, query)),
            false,
        );
    }
    fn insert_slash(&mut self, token: &str, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.form
            && let Some(e) = f.entries.iter().find(|e| e.spec.key == "body")
            && let Control::Text(Field::Multi(s)) = &e.control
        {
            s.update(cx, |v, cx| v.set_value(format!("{token} "), w, cx));
            e.handle(cx).focus(w, cx);
        }
        self.communication.slash = None;
        self.communication.options = Value::Null;
    }
    pub fn slash_key(&mut self, key: &str, w: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.communication.slash.is_none()
            || !self.form.as_ref().is_some_and(|f| {
                f.entries
                    .iter()
                    .any(|e| e.spec.key == "body" && e.handle(cx).is_focused(w))
            })
        {
            return false;
        }
        match key {
            "escape" => {
                self.communication.dismissed = self.communication.slash.take();
            }
            "tab" => {
                let Some(token) = self.communication.options["items"][0]["token"]
                    .as_str()
                    .map(str::to_owned)
                else {
                    return false;
                };
                self.insert_slash(&token, w, cx);
            }
            _ => return false,
        }
        w.prevent_default();
        cx.stop_propagation();
        cx.notify();
        true
    }
}
