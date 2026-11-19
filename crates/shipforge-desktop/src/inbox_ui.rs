use super::*;
use crate::{
    appearance::paint as rgb,
    controls::*,
    forms::{Format, Spec, item},
};
use chrono::{Days, Local, NaiveDateTime, TimeZone};
#[path = "inbox_display.rs"]
mod display;
#[cfg(all(test, target_os = "windows"))]
#[path = "inbox_ui_test.rs"]
mod native_test;
#[derive(Default)]
pub struct State {
    rows: Vec<Record>,
    counts: Value,
    total: usize,
    settings: Value,
    section: String,
    channel: Option<String>,
    org: String,
    cursor: Option<i64>,
    pub banner: Option<String>,
    pub initial_link: Option<String>,
}
#[derive(Clone)]
pub enum Action {
    Settings(Option<String>),
    Section(String),
    Toggle {
        channel: String,
        key: String,
        enabled: bool,
    },
    Reset(String),
    Filter,
    Clear,
    ReadAll,
    Page(i64),
    Open(Record),
    Change(Record, String),
    Followers(Record),
    Reactions(Record),
    Link(String),
}
pub enum Reply {
    Settings,
    List(u64),
    Feed(String),
    Opened,
    Changed,
    Followers(Record),
    Reactions(Record),
}
impl State {
    pub(super) fn clear_rows(&mut self) {
        self.rows.clear();
        self.counts = Value::Null;
        self.total = 0;
    }
}
#[derive(Clone)]
pub enum InboxForm {
    Filter,
    Clear,
    Snooze(Record),
    Followers(Record, String),
    Reaction(Record, String),
}
impl InboxForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Filter => "筛选收件箱",
            Self::Clear => "清理当前分区",
            Self::Snooze(_) => "稍后提醒",
            Self::Followers(..) => "更改关注者",
            Self::Reaction(..) => "表情回应",
        }
    }
    pub fn submit_label(&self) -> &'static str {
        match self {
            Self::Filter => "应用筛选",
            Self::Clear => "确认清理",
            Self::Snooze(_) => "确认提醒时间",
            Self::Followers(..) | Self::Reaction(..) => "保存",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        use Format::*;
        match self {
            Self::Filter => vec![
                item("query", "搜索任务标题或评论", Text, ""),
                item("include_spaces", "仅查看这些空间", Many("space"), ""),
                item("exclude_spaces", "排除这些空间", Many("space"), ""),
                item("unread", "仅显示未读", Bool, "false"),
            ],
            Self::Clear => vec![
                item(
                    "range",
                    "清理范围",
                    Choice(&[("all", "当前筛选全部"), ("states", "仅指定任务状态")]),
                    "all",
                ),
                item("statuses", "任务状态", Many("status"), "done"),
            ],
            Self::Snooze(_) => vec![
                item(
                    "when",
                    "提醒时间",
                    Choice(&[
                        ("20m", "20 分钟后"),
                        ("2h", "2 小时后"),
                        ("tomorrow", "明天 09:00"),
                        ("week", "下周一 09:00"),
                        ("custom", "自定义时间"),
                    ]),
                    "20m",
                ),
                item("custom", "本地时间（YYYY-MM-DD HH:MM）", Text, ""),
            ],
            Self::Followers(..) => vec![
                item(
                    "enabled",
                    "操作",
                    Choice(&[("add", "添加"), ("remove", "移除")]),
                    "add",
                ),
                item("member", "成员", Ref("member"), ""),
                item("agent", "Agent", Ref("agent"), ""),
                item("invitation", "邀请中成员", Ref("member_invitation"), ""),
            ],
            Self::Reaction(..) => vec![
                item("emoji", "表情", Text, "👍"),
                item(
                    "enabled",
                    "操作",
                    Choice(&[("add", "添加"), ("remove", "移除")]),
                    "add",
                ),
            ],
        }
    }
}
fn until(v: &Value) -> Result<i64, String> {
    use chrono::Datelike;
    let now = Local::now();
    match v["when"].as_str() {
        Some("20m") => Ok((now + chrono::Duration::minutes(20)).timestamp_millis()),
        Some("2h") => Ok((now + chrono::Duration::hours(2)).timestamp_millis()),
        Some(option @ ("tomorrow" | "week" | "custom")) => {
            let local = if option == "custom" {
                NaiveDateTime::parse_from_str(v["custom"].as_str().unwrap_or(""), "%Y-%m-%d %H:%M")
                    .map_err(|_| "请输入正确的本地日期、小时和分钟")?
            } else {
                let days = if option == "tomorrow" {
                    1
                } else {
                    7 - u64::from(now.weekday().num_days_from_monday())
                };
                now.date_naive()
                    .checked_add_days(Days::new(days))
                    .and_then(|d| d.and_hms_opt(9, 0, 0))
                    .ok_or("提醒日期无效")?
            };
            let dt = Local
                .from_local_datetime(&local)
                .single()
                .ok_or("当地时区的这个时间不存在或有两个对应时刻")?;
            if dt <= now {
                return Err("请选择未来的时间".into());
            }
            Ok(dt.timestamp_millis())
        }
        _ => Err("请选择提醒时间".into()),
    }
}
impl Workbench {
    fn inbox_call(&mut self, q: Value, reply: Reply, write: bool) {
        self.send(q, After::Inbox(reply), write);
    }
    fn inbox_query(&self, op: &str) -> Value {
        let mut q = Value::Object(self.loc.filter.clone());
        q["op"] = json!(op);
        q["space"] = json!(self.loc.space);
        if !self.loc.section.is_empty() {
            q["section"] = json!(self.loc.section);
        }
        q
    }
    pub(super) fn inbox_ready(&mut self) {
        if let Some(link) = self.inbox.initial_link.take() {
            self.inbox_link(&link);
        }
    }
    pub(super) fn inbox_refresh(&mut self) {
        if self.actor.is_empty() {
            return;
        }
        let org = self
            .find("space", &self.loc.space)
            .map(|r| r.field("organization").to_owned())
            .unwrap_or_default();
        if !org.is_empty() {
            if org != self.inbox.org {
                self.inbox.cursor = None;
                self.inbox.banner = None;
                self.inbox.rows.clear();
                self.inbox.org = org.clone();
            }
            if !self.smoke
                && self.mode != "demo"
                && !self
                    .pending
                    .values()
                    .any(|p| matches!(p.after, After::Inbox(Reply::Feed(_))))
            {
                let mut q = json!({"op":"inbox.feed","space":self.loc.space});
                if let Some(after) = self.inbox.cursor {
                    q["after"] = json!(after);
                }
                self.inbox_call(q, Reply::Feed(org), false);
            }
        }
        if self.loc.page == Page::Inbox {
            self.inbox_call(
                self.inbox_query("inbox.query"),
                Reply::List(self.generation),
                false,
            );
        }
        if self.loc.page == Page::Settings && self.loc.section == "notifications" {
            self.inbox_call(
                json!({"op":"inbox.settings","space":self.loc.space}),
                Reply::Settings,
                false,
            );
        }
    }
    fn inbox_can_write(&self) -> bool {
        self.mode != "team" || self.access_identity["scope"] == "write"
    }
    fn inbox_link(&mut self, link: &str) {
        if let Ok((origin, space, id)) = shipforge_core::inbox_link_parts(link) {
            self.inbox_call(json!({"op":if self.inbox_can_write(){"inbox.open"}else{"inbox.locate"},"origin":origin,"space":space,"id":id}),Reply::Opened,self.inbox_can_write());
        }
    }
    pub(super) fn inbox_action(&mut self, a: Action, w: &mut Window, cx: &mut Context<Self>) {
        match a {
            Action::Settings(channel)=>{if self.loc.page!=Page::Settings{self.navigate(Page::Settings);}
                self.loc.section="notifications".into();self.inbox.channel=channel;
                self.inbox_call(json!({"op":"inbox.settings","space":self.loc.space}),Reply::Settings,false);}
            Action::Section(section)=>{self.loc.section=section.clone();self.loc.filter.remove("offset");
                if self.inbox_can_write() && matches!(section.as_str(),"all"|"important"|"other"){
                    self.inbox_call(json!({"op":"inbox.configure","space":self.loc.space,"landing":section}),Reply::Settings,true);
                }
                self.read(false);}
            Action::Toggle{channel,key,enabled}=>self.inbox_call(json!({"op":"inbox.configure","space":self.loc.space,"channel":channel,"key":key,"enabled":enabled,"expected_rev":self.inbox.settings["rev"]}),Reply::Settings,true),
            Action::Reset(channel)=>self.inbox_call(json!({"op":"inbox.configure","space":self.loc.space,"channel":channel,"reset":true,"expected_rev":self.inbox.settings["rev"]}),Reply::Settings,true),
            Action::Filter=>self.open_prefilled(Mode::Inbox(InboxForm::Filter),self.loc.filter.clone(),w,cx),
            Action::Clear=>{
                let statuses=&self.inbox.settings["clear_statuses"];
                let mut values=Map::new();if statuses.is_array(){values.insert("statuses".into(),statuses.clone());values.insert("range".into(),json!("states"));}
                self.open_prefilled(Mode::Inbox(InboxForm::Clear),values,w,cx);
            }
            Action::ReadAll=>{let mut q=self.inbox_query("inbox.bulk");q["action"]=json!("read");self.inbox_call(q,Reply::Changed,true);}
            Action::Page(offset)=>{self.loc.filter.insert("offset".into(),json!(offset.max(0)));self.read(false);}
            Action::Link(link)=>self.inbox_link(&link),
            Action::Open(r)=>self.inbox_call(json!({"op":if self.inbox_can_write(){"inbox.open"}else{"inbox.locate"},"space":r.space,"id":r.id}),Reply::Opened,self.inbox_can_write()),
            Action::Change(r,act) if act=="snooze"=>self.open_form(Mode::Inbox(InboxForm::Snooze(r)),w,cx),
            Action::Change(r,act)=>self.inbox_call(json!({"op":"notification.act","space":r.space,"id":r.id,"action":act,"expected_rev":r.rev}),Reply::Changed,true),
            Action::Followers(r)=>self.inbox_call(json!({"op":"followers.list","space":r.space,"id":r.id}),Reply::Followers(r),false),
            Action::Reactions(r)=>self.inbox_call(json!({"op":"comment.reactions","space":r.space,"id":r.id}),Reply::Reactions(r),false),
        }
        cx.notify();
    }
    pub(super) fn inbox_submit(
        &mut self,
        f: InboxForm,
        values: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let v = Value::Object(values);
        match f {
            InboxForm::Filter=>{self.loc.filter=v.as_object().unwrap().clone();self.loc.filter.retain(|_,v|!v.is_null()&& v.as_str()!=Some("") && !v.as_array().is_some_and(Vec::is_empty));self.close_form(w,cx);self.read(false);}
            InboxForm::Clear=>{let mut q=self.inbox_query("inbox.bulk");q["action"]=json!("clear");
                if v["range"]=="states"{if v["statuses"].as_array().is_none_or(Vec::is_empty){return Err("请选择要清理的状态".into());}q["statuses"]=v["statuses"].clone();} self.inbox_call(q,Reply::Changed,true);}
            InboxForm::Snooze(r)=>{let until=until(&v)?;self.inbox_call(json!({"op":"notification.act","space":r.space,"id":r.id,"expected_rev":r.rev,"action":"snooze","until":until}),Reply::Changed,true);}
            InboxForm::Followers(r,_)=>{
                let picked:Vec<_>=["member","agent","invitation"].into_iter().filter_map(|k|v[k].as_str().filter(|s|!s.is_empty()).map(|id|(k,id))).collect();
                let [(kind,id)]=picked.as_slice() else{return Err("请选择一个成员、Agent 或邀请中的成员".into());};
                self.inbox_call(json!({"op":"followers.change","space":r.space,"id":r.id,"target_kind":kind,"target":id,"enabled":v["enabled"]=="add"}),Reply::Changed,true);
            }
            InboxForm::Reaction(r,_)=>self.inbox_call(json!({"op":"comment.react","space":r.space,"id":r.id,"enabled":v["enabled"]=="add","emoji":v["emoji"]}),Reply::Changed,true),
        }
        Ok(())
    }
    pub(super) fn inbox_reply(
        &mut self,
        reply: Reply,
        v: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Reply::Settings => self.inbox.settings = v,
            Reply::List(generation)
                if self.loc.page == Page::Inbox && generation == self.generation =>
            {
                self.inbox.rows = serde_json::from_value(v["rows"].clone()).unwrap_or_default();
                self.inbox.total = v["total"].as_u64().unwrap_or(0) as usize;
                self.inbox.counts = v["unread"].clone();
                self.inbox.section = v["section"].as_str().unwrap_or("all").into();
                self.inbox.settings["clear_statuses"] = v["clear_statuses"].clone();
            }
            Reply::Feed(org) if org == self.inbox.org => {
                self.inbox.cursor = v["cursor"].as_i64();
                if let Some(items) = v["items"].as_array() {
                    for item in items {
                        if let (Some(title), Some(link)) =
                            (item["title"].as_str(), item["link"].as_str())
                        {
                            let label = display::event_label(item["event"].as_str().unwrap_or(""))
                                .to_owned();
                            if !w.is_window_active() {
                                display::system_banner(title, &label, link);
                            }
                            self.notify_text(format!("{label} · {title}"));
                            self.inbox.banner = Some(link.into());
                        }
                    }
                }
            }
            Reply::Opened => {
                let Ok(target) = serde_json::from_value::<Record>(v["target"].clone()) else {
                    return;
                };
                self.remember();
                let changed = self.loc.space != target.space;
                self.loc.space = target.space.clone();
                self.loc.page = Page::Inbox;
                self.loc.selected = Some(target.clone());
                self.reset_detail();
                self.push();
                if changed {
                    self.rows.clear();
                    self.guide.change_space();
                    self.who();
                }
                self.read(false);
                self.inbox.banner = None;
                self.notice = None;
                if let Some(comment) = v["comment"].as_str() {
                    self.work_action(
                        workspace_ui::Action::Locate {
                            task: target.id,
                            comment: comment.into(),
                        },
                        w,
                        cx,
                    );
                }
            }
            Reply::Changed => {
                self.close_form(w, cx);
                self.read(false);
            }
            Reply::Followers(r) => {
                let names = v["followers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["title"].as_str())
                    .collect::<Vec<_>>()
                    .join("、");
                self.open_form(Mode::Inbox(InboxForm::Followers(r, names)), w, cx);
            }
            Reply::Reactions(r) => {
                let summary = v["reactions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| format!("{} × {}", a["emoji"].as_str().unwrap_or(""), a["count"]))
                    .collect::<Vec<_>>()
                    .join("  ");
                self.open_form(Mode::Inbox(InboxForm::Reaction(r, summary)), w, cx);
            }
            _ => {}
        }
        cx.notify();
    }
}
