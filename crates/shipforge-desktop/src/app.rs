use crate::{
    backend::{Backend, Event, Request, Rows},
    catalog::{Page, Record},
    forms::{Control, Form, Mode, compile_record, initial_record},
    motion::Motion,
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Location {
    pub space: String,
    pub page: Page,
    pub layout: String,
    pub task_type: String,
    pub filter: Map<String, Value>,
    pub mine_created: bool,
    pub view: Option<Record>,
    pub selected: Option<Record>,
    pub section: String,
}
impl Default for Location {
    fn default() -> Self {
        Self {
            space: "default".into(),
            page: Page::Tasks,
            layout: "board".into(),
            task_type: String::new(),
            filter: Map::new(),
            mine_created: false,
            view: None,
            selected: None,
            section: String::new(),
        }
    }
}
#[derive(Clone)]
pub enum UiAction {
    AddTrigger,
    RemoveTrigger(u64),
    Browse { index: usize, save: Option<String> },
    Go(Page),
    Open(Record),
    Back,
    Forward,
    CloseDetail,
    Create(String),
    Edit(Record),
    ToggleSidebar,
    Refresh,
    TaskType(String),
    Layout(String),
    Section(String),
    Mine(bool),
    Filter,
    ResetFilter,
    SaveView,
    Search,
    SelectSpace(Record),
    OpenView(Record),
    Apply(Value, String),
    Form(Mode),
    CloseForm,
    Submit,
    Pick(usize),
    Choose(usize, String),
    Delete(Record),
    Inspect(Value, String),
    OpenSearch(Record),
    Connect,
    Disconnect,
    Versions(Record),
    RestoreVersion(Record, i64),
    Copy(String),
    Export,
    Import,
    RuleFire(Record),
    ViewAgentHistory(Record),
    IterationEnd(Record),
    PendingDrafts,
    Grant,
    Tokens,
}
enum After {
    Saved(String),
    Notice(String),
    Search(String),
    View(String),
    Who,
    Versions(Record),
    Output(String),
    Machine,
    Drafts,
    Dashboard(u64),
    Preview(PathBuf),
    Connected,
    Tokens,
}
struct Pending {
    after: After,
    space: String,
    write: bool,
}
pub struct Workbench {
    pub loc: Location,
    pub rows: Rows,
    pub errors: BTreeMap<String, String>,
    pub form: Option<Form>,
    pub motion: Motion,
    pub root_focus: FocusHandle,
    pub modal_focus: FocusHandle,
    pub sidebar: bool,
    pub revision: u64,
    pub mode: String,
    pub directory: Option<PathBuf>,
    pub smoke: bool,
    pub actor: String,
    pub role: String,
    pub busy: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub notice: Option<(String, Instant)>,
    pub versions: Vec<Value>,
    pub view_tasks: Vec<Record>,
    pub metrics: Value,
    pub drawer_tab: String,
    pub tokens: Vec<Value>,
    pub drafts: Vec<Value>,
    reading: bool,
    history: Vec<Location>,
    pos: usize,
    backend: Backend,
    generation: u64,
    sequence: u64,
    pending: HashMap<u64, Pending>,
    poll_at: Instant,
    search_at: Instant,
}
impl Workbench {
    pub fn new(
        dir: Option<PathBuf>,
        demo: bool,
        smoke: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let loc = Location::default();
        let root_focus = cx.focus_handle();
        root_focus.focus(window, cx);
        let app = Self {
            directory: dir.clone(),
            smoke,
            history: vec![loc.clone()],
            loc,
            rows: Rows::new(),
            errors: BTreeMap::new(),
            form: None,
            motion: Motion::default(),
            root_focus,
            modal_focus: cx.focus_handle(),
            sidebar: true,
            revision: 0,
            mode: if demo { "demo" } else { "local" }.into(),
            actor: "owner".into(),
            role: String::new(),
            busy: false,
            loading: true,
            error: None,
            notice: None,
            versions: vec![],
            view_tasks: vec![],
            metrics: Value::Null,
            drawer_tab: String::new(),
            tokens: vec![],
            drafts: vec![],
            reading: false,
            pos: 0,
            backend: Backend::open(dir, demo),
            generation: 0,
            sequence: 0,
            pending: HashMap::new(),
            poll_at: Instant::now() + Duration::from_secs(4),
            search_at: Instant::now(),
        };
        let weak = cx.entity().downgrade();
        cx.spawn_in(window, async move |_, cx| {
            loop {
                cx.background_executor()
                    .timer(crate::motion::SYSTEM_POLL)
                    .await;
                if cx
                    .update(|window, cx| {
                        let old = cx.reduce_motion();
                        gpui_kit::base::apply_system_reduce_motion(cx);
                        if old != cx.reduce_motion() {
                            window.refresh();
                        }
                        weak.update(cx, |this, cx| {
                            if old != cx.reduce_motion() {
                                this.motion.settle();
                            }
                            this.tick(window, cx)
                        })
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        app
    }
    pub fn list(&self, kind: &str) -> &[Record] {
        self.rows.get(kind).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn find(&self, kind: &str, id: &str) -> Option<&Record> {
        self.list(kind)
            .iter()
            .find(|r| r.id == id || kind == "member" && r.field("actor") == id)
    }
    pub fn name(&self, kind: &str, id: &str) -> String {
        if id.is_empty() {
            return "未设置".into();
        }
        self.find(kind, id)
            .map(|r| r.title.clone())
            .unwrap_or_else(|| id.to_owned())
    }
    pub fn handler_name(&self, id: &str) -> String {
        if id.is_empty() {
            return "未指派".into();
        }
        for k in ["member", "agent", "crew"] {
            if let Some(r) = self.find(k, id) {
                return r.title.clone();
            }
        }
        id.to_owned()
    }
    pub fn space_name(&self) -> String {
        self.find("space", &self.loc.space)
            .map(|r| r.title.clone())
            .unwrap_or_else(|| {
                if self.loc.space == "default" {
                    "默认空间".into()
                } else {
                    self.loc.space.clone()
                }
            })
    }
    pub fn can_back(&self) -> bool {
        self.pos > 0
    }
    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.history.len()
    }
    pub fn writable(&self) -> bool {
        !matches!(self.role.as_str(), "viewer" | "read")
    }
    pub fn notify_text(&mut self, s: impl Into<String>) {
        self.notice = Some((s.into(), Instant::now()));
    }
    fn register(&mut self, after: After, write: bool) -> u64 {
        self.sequence += 1;
        self.pending.insert(
            self.sequence,
            Pending {
                after,
                space: self.loc.space.clone(),
                write,
            },
        );
        if write {
            self.busy = true;
        }
        self.sequence
    }
    fn send(&mut self, command: Value, after: After, write: bool) {
        if write && self.busy {
            return;
        }
        let ticket = self.register(after, write);
        if self
            .backend
            .sender
            .send(Request::Call { ticket, command })
            .is_err()
        {
            self.error = Some("工作区服务已关闭，请重启工作台。".into());
            self.busy = false;
        }
    }
    fn read(&mut self, foreground: bool) {
        self.reading = true;
        self.generation += 1;
        if foreground {
            self.loading = true;
        }
        let mut kinds = vec![self.loc.page.info().kind, "space", "member"];
        if self.loc.page.is_tasks() || self.loc.view.is_some() {
            kinds.extend([
                "task",
                "task_type",
                "status",
                "agent",
                "crew",
                "project",
                "iteration",
                "field",
            ]);
        }
        match self.loc.page {
            Page::Dashboard | Page::Projects | Page::Iterations | Page::Views => kinds.push("task"),
            Page::Assistant | Page::Chats => kinds.extend(["agent", "chat"]),
            Page::Market => kinds.extend(["skill", "agent"]),
            Page::Inbox => kinds.push("task"),
            Page::Spaces | Page::Organizations => kinds.push("organization"),
            Page::Artifacts => kinds.push("artifact"),
            Page::Agents | Page::Crews | Page::Automations | Page::Tools => {
                kinds.extend(["agent", "crew", "machine", "skill", "tool"])
            }
            _ => {}
        }
        if let Some(item) = &self.loc.selected {
            match item.kind.as_str() {
                "task" => kinds.extend([
                    "task",
                    "comment",
                    "relation",
                    "run",
                    "artifact",
                    "iteration",
                    "project",
                    "agent",
                    "crew",
                    "status",
                    "field",
                    "task_type",
                ]),
                "chat" => kinds.extend(["message", "run", "artifact", "agent"]),
                "agent" => kinds.extend(["run", "skill", "tool", "machine", "agent"]),
                "automation" => kinds.extend(["run", "agent", "crew"]),
                "project" | "iteration" => kinds.push("task"),
                "machine" => kinds.push("run"),
                _ => {}
            }
        }
        if self.form.is_some() {
            kinds.extend([
                "agent",
                "crew",
                "status",
                "task_type",
                "field",
                "iteration",
                "project",
                "machine",
                "skill",
                "tool",
                "relation_type",
                "organization",
                "document",
                "task",
            ]);
            if self
                .form
                .as_ref()
                .is_some_and(|f| matches!(f.mode,Mode::Create(ref k) if k=="task"||k=="relation"))
            {
                kinds.push("task");
            }
        }
        kinds.sort_unstable();
        kinds.dedup();
        let _ = self.backend.sender.send(Request::Read {
            generation: self.generation,
            space: self.loc.space.clone(),
            kinds: kinds.into_iter().map(str::to_owned).collect(),
        });
        self.poll_at = Instant::now() + Duration::from_secs(5);
        if self.loc.page == Page::Dashboard || self.loc.layout == "dashboard" {
            let mut filter = self
                .loc
                .view
                .as_ref()
                .and_then(|v| v.fields.get("filter"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for (k, v) in &self.loc.filter {
                if let Some(key) = k.strip_prefix("filter_") {
                    filter.insert(key.into(), v.clone());
                }
            }
            if !self.loc.task_type.is_empty() {
                filter.insert("type".into(), json!(self.loc.task_type));
            }
            if self.loc.page == Page::Mine {
                filter.insert(
                    if self.loc.mine_created {
                        "creator"
                    } else {
                        "assignee"
                    }
                    .into(),
                    json!(self.actor),
                );
            } else if self.loc.page == Page::Triage {
                filter.insert("status_category".into(), json!("triage"));
            } else if self.loc.view.is_none() {
                filter.insert("status_category_not".into(), json!("triage"));
            }
            let query = self
                .loc
                .filter
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("");
            self.send(
                json!({"op":"dashboard","space":self.loc.space,"filter":filter,"query":query}),
                After::Dashboard(self.generation),
                false,
            );
        }

        if let Some(view) = &self.loc.view {
            let id = view.id.clone();
            self.send(
                json!({"op":"view.run","space":self.loc.space,"id":id}),
                After::View(id),
                false,
            );
        }
    }
    fn who(&mut self) {
        self.send(
            json!({"op":"whoami","space":self.loc.space}),
            After::Who,
            false,
        );
    }
    fn tick(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(event) = self.backend.results.try_recv() {
            changed = true;
            match event {
                Event::Ready(result) => match result {
                    Ok(mode) => {
                        self.mode = mode;
                        self.error = None;
                        self.who();
                        self.read(true);
                    }
                    Err(e) => {
                        if self.smoke {
                            crate::print_information(&format!("SHIPFORGE_DESKTOP_ERROR {e}"));
                            std::process::exit(1);
                        }
                        self.loading = false;
                        self.error = Some(e);
                    }
                },
                Event::Rows {
                    generation,
                    space,
                    data,
                    errors,
                } => {
                    if generation != self.generation || space != self.loc.space {
                        continue;
                    }
                    self.loading = false;
                    self.reading = false;
                    self.errors = errors;
                    for k in self.errors.keys() {
                        self.rows.remove(k);
                    }
                    self.rows.extend(data);
                    if let Some(old) = &self.loc.selected
                        && let Some(rows) = self.rows.get(&old.kind)
                    {
                        self.loc.selected = rows.iter().find(|r| r.id == old.id).cloned();
                    }
                    if self.smoke {
                        let ok = self.errors.is_empty() && !self.list("space").is_empty();
                        crate::print_information(if ok {
                            "SHIPFORGE_DESKTOP_READY"
                        } else {
                            "SHIPFORGE_DESKTOP_ERROR"
                        });
                        if !ok {
                            std::process::exit(1);
                        }
                        cx.quit();
                        self.smoke = false;
                    }
                    if self.find("space", &self.loc.space).is_none()
                        && let Some(first) = self.list("space").first().cloned()
                    {
                        self.switch_space(first);
                    }
                }
                Event::Reply { ticket, result } => {
                    if let Some(p) = self.pending.remove(&ticket) {
                        if p.write {
                            self.busy = false;
                        }
                        if p.space != self.loc.space {
                            continue;
                        }
                        match result {
                            Ok(value) => self.reply(p.after, value, w, cx),
                            Err(error) => match p.after {
                                After::Who => {
                                    self.role.clear();
                                }
                                After::Search(ref query) if !self.current_search(query, cx) => {}
                                After::View(_) => {
                                    self.view_tasks.clear();
                                    self.error = Some(error);
                                }
                                _ => {
                                    if let Some(f) = self.form.as_mut() {
                                        f.error = error;
                                    } else {
                                        self.error = Some(error);
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
        if !self.loading && !self.busy && !self.reading && Instant::now() >= self.poll_at {
            self.read(false);
        }
        if self
            .form
            .as_ref()
            .is_some_and(|f| matches!(f.mode, Mode::Search))
            && Instant::now() >= self.search_at
        {
            self.search_at = Instant::now() + Duration::from_millis(300);
            let q = self
                .form
                .as_ref()
                .and_then(|f| f.entries.first())
                .map(|e| e.raw(cx))
                .unwrap_or_default();
            if self.form.as_ref().is_some_and(|f| f.last_search != q) {
                if let Some(f) = &mut self.form {
                    f.last_search = q.clone();
                    f.search_results.clear();
                }
                if !q.trim().is_empty() {
                    self.search(q);
                }
                changed = true;
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(5))
        {
            self.notice = None;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }
    fn current_search(&self, q: &str, cx: &App) -> bool {
        self.form.as_ref().is_some_and(|f| {
            matches!(f.mode, Mode::Search) && f.entries.first().is_some_and(|e| e.raw(cx) == q)
        })
    }
    fn search(&mut self, q: String) {
        self.send(
            json!({"op":"search","space":self.loc.space,"query":q}),
            After::Search(q),
            false,
        );
    }
    fn reply(&mut self, after: After, value: Value, w: &mut Window, cx: &mut Context<Self>) {
        if value.get("queued").and_then(Value::as_bool) == Some(true) {
            self.close_form(w, cx);
            self.notify_text("已保存离线草稿，重连后可同步。");
            return;
        }
        match after {
            After::Saved(kind) => {
                self.close_form(w, cx);
                self.notify_text("已保存");
                if let Ok(r) = serde_json::from_value::<Record>(value)
                    && !r.id.is_empty()
                    && !matches!(kind.as_str(), "message" | "comment")
                {
                    if r.kind == "space" && self.loc.page == Page::Spaces {
                        self.switch_space(r);
                    } else {
                        self.loc.selected = Some(r);
                    }
                }
                self.read(false);
            }
            After::Notice(s) => {
                self.close_form(w, cx);
                self.notify_text(s);
                self.read(false);
            }
            After::Machine => {
                self.close_form(w, cx);
                self.notify_text(value["message"].as_str().unwrap_or("工作机状态已更新"));
                self.read(true);
            }
            After::Search(q) => {
                if self.current_search(&q, cx)
                    && let Some(f) = &mut self.form
                {
                    f.search_results = serde_json::from_value(value).unwrap_or_default();
                }
            }
            After::Drafts => {
                self.drafts = value.as_array().cloned().unwrap_or_default();
                self.loc.section = "drafts".into();
            }
            After::View(id) => {
                if self.loc.view.as_ref().is_some_and(|v| v.id == id) {
                    self.view_tasks = serde_json::from_value(value).unwrap_or_default();
                }
            }
            After::Dashboard(revision) => {
                if revision == self.generation {
                    self.metrics = value;
                }
            }
            After::Who => {
                self.actor = value
                    .get("actor")
                    .and_then(Value::as_str)
                    .unwrap_or("owner")
                    .into();
                self.role = value
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into();
            }
            After::Versions(r) => {
                if self.loc.selected.as_ref().is_some_and(|i| i.id == r.id) {
                    self.versions = value.as_array().cloned().unwrap_or_default();
                    self.drawer_tab = "versions".into();
                }
            }
            After::Output(heading) => {
                self.open_form(
                    Mode::Output {
                        heading,
                        content: serde_json::to_string_pretty(&value).unwrap_or_default(),
                    },
                    w,
                    cx,
                );
            }
            After::Preview(path) => {
                if let Some(f) = &mut self.form {
                    f.preview = Some(json!({"path":path,"summary":value}));
                }
            }
            After::Connected => {
                self.close_form(w, cx);
                self.loc = Location::default();
                self.history = vec![self.loc.clone()];
                self.pos = 0;
                self.rows.clear();
                self.role.clear();
                self.who();
                self.read(true);
                self.notify_text("已更新工作区连接");
            }
            After::Tokens => {
                self.tokens = value.as_array().cloned().unwrap_or_default();
                self.loc.section = "tokens".into();
            }
        }
    }
    fn reset_detail(&mut self) {
        self.drawer_tab.clear();
        self.versions.clear();
        self.error = None;
    }
    fn remember(&mut self) {
        self.history[self.pos] = self.loc.clone();
        self.history.truncate(self.pos + 1);
    }
    fn push(&mut self) {
        self.history.push(self.loc.clone());
        self.pos = self.history.len() - 1;
        self.revision += 1;
    }
    fn navigate(&mut self, page: Page) {
        self.remember();
        let space = self.loc.space.clone();
        self.loc = Location {
            page,
            space,
            ..Location::default()
        };
        self.reset_detail();
        self.push();
        self.read(true);
    }
    fn switch_space(&mut self, r: Record) {
        if self.loc.space != r.id {
            self.rows.clear();
            self.errors.clear();
        }
        self.navigate(Page::Tasks);
        self.loc.space = r.id;
        self.history[self.pos] = self.loc.clone();
        self.who();
        self.read(true);
    }
    fn open(&mut self, r: Record, route: bool) {
        self.remember();
        if route && let Some(page) = Page::from_kind(&r.kind) {
            self.loc.page = page;
            self.loc.view = None;
        }
        self.loc.selected = Some(r);
        self.reset_detail();
        self.push();
        self.read(false);
    }
    fn history(&mut self, forward: bool) {
        let next = if forward {
            (self.pos + 1).min(self.history.len() - 1)
        } else {
            self.pos.saturating_sub(1)
        };
        if next == self.pos {
            return;
        }
        self.history[self.pos] = self.loc.clone();
        let oldspace = self.loc.space.clone();
        self.loc = self.history[next].clone();
        self.pos = next;
        self.revision += 1;
        self.reset_detail();
        if self.loc.space != oldspace {
            self.rows.clear();
            self.who();
        }
        self.read(true);
    }
    pub fn close_form(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(f) = self.form.take() {
            f.origin
                .unwrap_or_else(|| self.root_focus.clone())
                .focus(w, cx);
        }
        self.revision += 1;
    }
    fn open_form(&mut self, mode: Mode, w: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let mut value = Map::new();
        match &mode {
            Mode::Edit(r) => value = initial_record(r),
            Mode::Filter => value = self.loc.filter.clone(),
            Mode::Machine { .. } => {
                if let Ok(path) =
                    crate::machine::default_config(self.directory.as_deref(), &self.loc.space)
                {
                    value.insert("config".into(), json!(path));
                }
            }
            Mode::Create(k) if k == "view" => {
                value = self.loc.filter.clone();
                value.insert("filter_type".into(), json!(self.loc.task_type));
                value.insert("layout".into(), json!(self.loc.layout));
            }
            Mode::Create(k) if k == "task" => {
                if !self.loc.task_type.is_empty() {
                    value.insert("type".into(), json!(self.loc.task_type));
                }
                if self.loc.page == Page::Triage {
                    value.insert("status".into(), json!("triage"));
                }
                if let Some(r) = &self.loc.selected
                    && matches!(r.kind.as_str(), "project" | "iteration")
                {
                    value.insert(r.kind.clone(), json!(r.id));
                }
            }
            _ => {}
        }
        self.form = Some(Form::new(mode, &value, w, cx));
        if let Some(f) = &self.form
            && let Some(e) = f.entries.first()
        {
            e.handle(cx).focus(w, cx);
        } else {
            self.modal_focus.focus(w, cx);
        }
        self.read(false);
        self.revision += 1;
    }
    fn act_detail_close(&mut self) {
        if self.loc.selected.is_none() {
            return;
        }
        if self.pos > 0
            && self.history[self.pos - 1].page == self.loc.page
            && self.history[self.pos - 1].space == self.loc.space
        {
            self.history(false);
        } else {
            self.loc.selected = None;
            self.reset_detail();
            self.read(false);
        }
    }
    pub fn act(&mut self, action: UiAction, w: &mut Window, cx: &mut Context<Self>) {
        use UiAction::*;
        match action{
            Go(p)=>self.navigate(p),Open(r)=>self.open(r,false),OpenSearch(r)=>{self.close_form(w,cx);self.open(r,true);}
            Back=>self.history(false),Forward=>self.history(true),CloseDetail=>self.act_detail_close(),
            Create(k)=>self.open_form(Mode::Create(k),w,cx),Edit(r)=>self.open_form(Mode::Edit(r),w,cx),
            ToggleSidebar=>self.sidebar = !self.sidebar,Refresh=>{self.error=None;self.read(true);},
            TaskType(t)=>{self.loc.task_type=t;self.revision+=1;self.read(false);},Layout(l)=>{self.loc.layout=l;self.revision+=1;self.read(false);},
            Section(s)=>{self.loc.section=s;self.revision+=1;},Mine(v)=>{self.loc.mine_created=v;self.revision+=1;self.read(false);},
            Filter=>self.open_form(Mode::Filter,w,cx),ResetFilter=>{self.loc.filter.clear();self.loc.task_type.clear();self.revision+=1;self.read(false);},
            SaveView=>self.open_form(Mode::Create("view".into()),w,cx),Search=>self.open_form(Mode::Search,w,cx),
            SelectSpace(r)=>self.switch_space(r),OpenView(v)=>{self.navigate(Page::Tasks);self.loc.layout=v.field("layout").to_owned();self.loc.view=Some(v);self.view_tasks.clear();self.read(true);}
            Apply(mut cmd,msg)=>{cmd["space"]=json!(self.loc.space);self.send(cmd,After::Notice(msg),true);}
            Form(mode)=>self.open_form(mode,w,cx),CloseForm=>self.close_form(w,cx),Submit=>if let Err(error)=self.submit(w,cx)&&let Some(f)=&mut self.form{f.error=error;},
            Pick(i)=>if let Some(f)=&mut self.form{f.open_pick=if f.open_pick==Some(i){None}else{Some(i)};},
            Choose(i,v)=>if let Some(f)=&mut self.form && let Some(e)=f.entries.get_mut(i) && let Control::Pick{value,focus}=&mut e.control{
                if matches!(e.spec.format,crate::forms::Format::Many(_)){let mut a:Vec<&str>=value.split(',').map(str::trim).filter(|s|!s.is_empty()).collect();if a.contains(&v.as_str()){a.retain(|s|*s!=v);}else if !v.is_empty(){a.push(&v);}else{a.clear();}*value=a.join(", ");}else{*value=v;f.open_pick=None;}focus.focus(w,cx);
            }
            Delete(r)=>{let command=json!({"op":"delete","space":self.loc.space,"kind":r.kind,"id":r.id,"expected_rev":r.rev});self.open_form(Mode::Confirm{label:format!("删除「{}」",r.title),command},w,cx);}
            AddTrigger=>{if let Some(f)=&mut self.form{f.add_trigger(&json!({"type":"status"}),w,cx);}},
            RemoveTrigger(id)=>{if let Some(f)=&mut self.form{f.remove_trigger(id);}},
            Browse{index,save}=>self.browse(index,save.as_deref(),w,cx),
            Inspect(mut cmd,label)=>{cmd["space"]=json!(self.loc.space);self.send(cmd,After::Output(label),false);}
            Connect=>self.open_form(Mode::Connect,w,cx),Disconnect=>{let ticket=self.register(After::Connected,true);let _=self.backend.sender.send(Request::Disconnect{ticket});}
            Versions(r)=>self.send(json!({"op":"document.history","id":r.id,"space":self.loc.space}),After::Versions(r),false),
            RestoreVersion(r,v)=>self.open_form(Mode::Confirm{label:format!("恢复「{}」版本 {v}",r.title),command:json!({"op":"document.restore","id":r.id,"space":self.loc.space,"version":v,"expected_rev":r.rev})},w,cx),
            Copy(s)=>{cx.write_to_clipboard(ClipboardItem::new_string(s));self.notify_text("已复制");}
            Export=>self.open_form(Mode::Export,w,cx),Import=>self.open_form(Mode::Import,w,cx),
            RuleFire(r)=>{self.open_form(Mode::Confirm{label:format!("运行「{}」",r.title),command:json!({"op":"automation.fire","id":r.id,"space":self.loc.space,"event_id":format!("ui-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())})},w,cx);}
            ViewAgentHistory(r)=>{self.drawer_tab="runs".into();self.loc.selected=Some(r);self.read(false);}
            IterationEnd(r)=>self.open_form(Mode::IterationEnd(r),w,cx),
            PendingDrafts=>self.send(json!({"op":"sync.pending"}),After::Drafts,false),
            Grant=>self.open_form(Mode::Token,w,cx),Tokens=>self.send(json!({"op":"access.list","space":self.loc.space}),After::Tokens,false),
        }
        cx.notify();
    }
    fn browse(&mut self, index: usize, save: Option<&str>, w: &mut Window, cx: &mut Context<Self>) {
        let Some(Control::Text(crate::controls::Field::Single(state))) = self
            .form
            .as_ref()
            .and_then(|f| f.entries.get(index))
            .map(|e| &e.control)
        else {
            return;
        };
        let state = state.clone();
        if let Some(name) = save {
            let initial = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default();
            let result = cx.prompt_for_new_path(&initial, Some(name));
            cx.spawn_in(w, async move |_, cx| {
                if let Ok(Ok(Some(path))) = result.await {
                    let _ = cx.update(|window, cx| {
                        state.update(cx, |s, cx| {
                            s.set_value(path.to_string_lossy().to_string(), window, cx)
                        })
                    });
                }
            })
            .detach();
        } else {
            let result = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("选择文件".into()),
            });
            cx.spawn_in(w, async move |_, cx| {
                if let Ok(Ok(Some(paths))) = result.await
                    && let Some(path) = paths.first()
                {
                    let _ = cx.update(|window, cx| {
                        state.update(cx, |s, cx| {
                            s.set_value(path.to_string_lossy().to_string(), window, cx)
                        })
                    });
                }
            })
            .detach();
        }
    }
    fn submit(&mut self, w: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        if self.busy {
            return Ok(());
        }
        let Some(f) = self.form.as_ref() else {
            return Ok(());
        };
        let mode = f.mode.clone();
        let values = f.values(cx)?;
        let preview = f.preview.clone();
        let val = |k: &str| {
            values
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        match mode{
            Mode::Create(ref k)|Mode::Edit(Record{kind:ref k,..})=>{
                let old=if let Mode::Edit(r)=&mode{Some(r)}else{None};let kind=if let Some(h)=values.get("assignee").and_then(Value::as_str){["agent","crew","member"].into_iter().find(|k|self.find(k,h).is_some())}else{None};
                let(title,mut fields)=compile_record(k,values,kind,old.is_none())?;
                if k=="automation" && let Some(id)=fields.get("handler_id").and_then(Value::as_str){let handler=if self.find("crew",id).is_some(){"crew"}else{"agent"};fields.insert("handler_kind".into(),json!(handler));}
                let mut cmd=json!({"op":if old.is_some(){"update"}else{"create"},"kind":k,"space":self.loc.space,"title":title,"fields":fields});
                if let Some(r)=old{cmd["id"]=json!(r.id);cmd["expected_rev"]=json!(r.rev);}self.send(cmd,After::Saved(k.clone()),true);
            }
            Mode::Comment(ref r)|Mode::Message(ref r)=>{let body=val("body");if body.is_empty(){return Err("请输入内容。".into())}let(kind,key)=if matches!(mode,Mode::Comment(_)){("comment","task")}else{("message","chat")};
                self.send(json!({"op":"create","kind":kind,"space":self.loc.space,"title":body.chars().take(60).collect::<String>(),"fields":{(key):r.id,"body":body}}),After::Saved(kind.into()),true);
            }
            Mode::Filter=>{self.loc.filter=values;self.loc.filter.retain(|_,v|!v.is_null()&&v.as_str()!=Some(""));self.close_form(w,cx);self.read(false);}
            Mode::Connect=>{let (url,token)=(val("url"),val("token"));if url.is_empty()||token.is_empty(){return Err("请输入服务地址和令牌。".into());}let ticket=self.register(After::Connected,true);let _=self.backend.sender.send(Request::Connect{url,token,ticket});}
            Mode::Confirm{command,..}=>self.send(command,After::Notice("操作已完成".into()),true),
            Mode::IterationEnd(r)=>self.send(json!({"op":"iteration.close","space":self.loc.space,"id":r.id,"target":values.get("target"),"expected_rev":r.rev}),After::Notice("迭代已结束".into()),true),
            Mode::Export|Mode::Import=>{let path=PathBuf::from(val("path"));if !path.is_absolute(){return Err("请输入文件的完整路径。".into());}
                let sp=self.loc.space.clone();let req=if matches!(mode,Mode::Export){let ticket=self.register(After::Notice("空间数据已导出".into()),true);Request::Export{ticket,command:json!({"op":"export","space":sp}),path}}
                else{let confirmed=preview.as_ref().and_then(|p|p.get("path")).and_then(Value::as_str)==path.to_str();let after=if confirmed{After::Notice("导入已完成".into())}else{After::Preview(path.clone())};let ticket=self.register(after,true);Request::Import{ticket,command:json!({"op":if confirmed{"import"}else{"import.preview"},"space":sp}),path}};let _=self.backend.sender.send(req);}
            Mode::Upload(_)|Mode::Download(_,_)=>{let path=PathBuf::from(val("path"));if !path.is_absolute(){return Err("请选择完整文件路径。".into())}
                let(command,after)=match &mode{Mode::Upload(record)=>{let mut q=json!({"op":"attachment.upload","kind":"document","space":self.loc.space});if let Some(r)=record{q["id"]=json!(r.id);q["kind"]=json!(r.kind);q["expected_rev"]=json!(r.rev);}else{q["visibility"]=json!(val("visibility"));}(q,After::Saved(record.as_ref().map_or("document",|r|&r.kind).to_owned()))},Mode::Download(r,file)=>(json!({"op":"attachment.download","kind":r.kind,"id":r.id,"attachment_id":file["id"],"space":self.loc.space}),After::Notice(format!("已保存到 {}",path.display()))),_=>unreachable!()};
                let ticket=self.register(after,true);self.backend.sender.send(Request::Attachment{ticket,command,path}).map_err(|e|e.to_string())?;}
            Mode::Machine{stop}=>{let ticket=self.register(After::Machine,true);self.backend.sender.send(Request::Machine{ticket,space:self.loc.space.clone(),fields:Value::Object(values),stop}).map_err(|e|e.to_string())?;}
            Mode::Workflow{op,record}=>{let mut q=Value::Object(values);q["op"]=json!(op);q["space"]=json!(self.loc.space);q["id"]=json!(record.id);
                if op=="tool.authorize"{q["id"]=q["agent"].take();q["tool_id"]=json!(record.id);}
                if op=="triage"{q["decision"]=json!("duplicate");q["expected_rev"]=json!(record.rev);}
                let inspect=matches!(op,"tool.authorize"|"connector.export"|"provider.preview");let after=if inspect{After::Output(crate::forms::workflow_name(op).into())}else{After::Notice("已保存".into())};self.send(q,after,!inspect);}
            Mode::ResolveDraft(draft)=>{let rev=values.get("expected_rev").and_then(Value::as_i64).filter(|v|*v>0).ok_or("请输入有效的服务器版本")?;self.send(json!({"op":"sync.resolve","id":draft["request_id"],"expected_rev":rev}),After::Notice("草稿已更新，可重新同步".into()),true);}
            Mode::Token=>{if val("actor").is_empty(){return Err("请选择成员。".into());}self.send(json!({"op":"access.issue","actor":val("actor"),"scope":val("scope"),"space":self.loc.space}),After::Output("访问令牌（请妥善保存）".into()),true);}
            Mode::Search=>self.search(val("query")),Mode::Output{..}=>self.close_form(w,cx),
        }
        Ok(())
    }
    pub fn options(&self, source: &str) -> Vec<(String, String)> {
        let from = |kind: &str| {
            self.list(kind)
                .iter()
                .map(|r| (r.id.clone(), r.title.clone()))
                .collect::<Vec<_>>()
        };
        match source {
            "$weekdays" => ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                .iter()
                .enumerate()
                .map(|(i, l)| ((i + 1).to_string(), l.to_string()))
                .collect(),
            "$sources" => vec![
                ("guest".into(), "访客".into()),
                ("external".into(), "外部集成".into()),
            ],
            "$handlers" | "$agents" => {
                let mut v = if source == "$handlers" {
                    from("member")
                } else {
                    vec![]
                };
                v.extend(from("agent"));
                v.extend(from("crew"));
                v
            }
            "$types" => {
                let mut v: Vec<_> = crate::catalog::TYPES
                    .iter()
                    .map(|(v, l)| (v.to_string(), l.to_string()))
                    .collect();
                overlay_options(&mut v, from("task_type"));
                v
            }
            "$statuses" => {
                let mut v: Vec<_> = crate::catalog::STATUSES
                    .iter()
                    .map(|(v, l, _)| (v.to_string(), l.to_string()))
                    .collect();
                overlay_options(&mut v, from("status"));
                v
            }
            "$priorities" => crate::catalog::PRIORITIES
                .iter()
                .map(|(v, l)| (v.to_string(), l.to_string()))
                .collect(),
            _ => from(source),
        }
    }
    pub fn task_rows(&self) -> Vec<Record> {
        let source = if self.loc.view.is_some() {
            &self.view_tasks[..]
        } else {
            self.list("task")
        };
        let mut out: Vec<_> = source
            .iter()
            .filter(|r| {
                if self.loc.page == Page::Triage {
                    return r.field("status") == "triage";
                }
                if r.field("status") == "triage"
                    && self.loc.page != Page::Mine
                    && self.loc.view.is_none()
                {
                    return false;
                }
                if self.loc.page == Page::Mine
                    && r.field(if self.loc.mine_created {
                        "creator"
                    } else {
                        "assignee"
                    }) != self.actor
                {
                    return false;
                }
                if !self.loc.task_type.is_empty() && r.field("type") != self.loc.task_type {
                    return false;
                }
                for key in ["status", "priority", "assignee", "project", "type"] {
                    if let Some(value) = self
                        .loc
                        .filter
                        .get(&format!("filter_{key}"))
                        .and_then(Value::as_str)
                        && !value.is_empty()
                        && r.field(key) != value
                    {
                        return false;
                    }
                }
                let q = self
                    .loc
                    .filter
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                q.is_empty()
                    || r.title.to_lowercase().contains(&q)
                    || r.code().to_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        if self.loc.view.is_none() {
            out.sort_by(|a, b| {
                a.fields
                    .get("number")
                    .and_then(Value::as_i64)
                    .cmp(&b.fields.get("number").and_then(Value::as_i64))
            });
        }
        out
    }
    pub fn key(&mut self, e: &KeyDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        let mods = e.keystroke.modifiers;
        let action = if key == "escape" {
            if let Some(f) = &mut self.form
                && f.open_pick.take().is_some()
            {
                cx.notify();
                return;
            }
            if self.form.is_some() {
                Some(UiAction::CloseForm)
            } else if self.loc.selected.is_some() {
                Some(UiAction::CloseDetail)
            } else {
                None
            }
        } else if self.form.is_some() {
            if key == "enter" && mods.control {
                Some(UiAction::Submit)
            } else {
                None
            }
        } else if mods.control && key == "k" {
            Some(UiAction::Search)
        } else if mods.control && key == "n" && self.loc.page.creatable() {
            Some(UiAction::Create(self.loc.page.info().kind.into()))
        } else if mods.alt && key == "left" {
            Some(UiAction::Back)
        } else if mods.alt && key == "right" {
            Some(UiAction::Forward)
        } else if key == "f5" {
            Some(UiAction::Refresh)
        } else {
            None
        };
        if let Some(a) = action {
            cx.stop_propagation();
            self.act(a, w, cx);
        }
    }
}

fn overlay_options(defaults: &mut Vec<(String, String)>, records: Vec<(String, String)>) {
    for (id, title) in records {
        if let Some(entry) = defaults.iter_mut().find(|o| o.0 == id) {
            entry.1 = title;
        } else {
            defaults.push((id, title));
        }
    }
}
