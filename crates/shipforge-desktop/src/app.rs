use crate::{
    backend::{Backend, Event, Request, Rows},
    catalog::{Page, Record},
    forms::{Control, Form, Mode, compile_record, initial_record},
    motion::Motion,
};
use gpui_kit::{prelude::*, *};
#[path = "account_ui.rs"]
mod account_ui;
#[path = "agent_directory.rs"]
pub(crate) mod agent_directory;
#[path = "chat_actions.rs"]
pub(crate) mod chat_actions;
#[path = "conversation_ui.rs"]
pub(crate) mod conversation_ui;
#[path = "identity_ui.rs"]
pub(crate) mod identity_ui;
#[path = "inbox_ui.rs"]
pub(crate) mod inbox_ui;
#[path = "market_ui.rs"]
pub(crate) mod market_ui;
#[path = "onboarding.rs"]
pub(crate) mod onboarding;
#[path = "personal.rs"]
pub(crate) mod personal;
#[path = "review_ui.rs"]
pub(crate) mod review_ui;
#[path = "space_ui.rs"]
pub(crate) mod space_ui;
#[path = "workspace_ui.rs"]
pub(crate) mod workspace_ui;
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
    LibraryDetails(String),
    IdentityProfile,
    IdentityClose,
    Inbox(inbox_ui::Action),
    Directory(agent_directory::Action),
    Work(workspace_ui::Action),
    Review(review_ui::Action),
    Conversation(conversation_ui::Action),
    Chat(chat_actions::Action),
    Space(space_ui::Action),
    CopyInvitation(String),
    Guide(onboarding::Action),
    Personal(crate::personal::Action),
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
    OpenSourceUrl(String),
    Export,
    Import,
    RuleFire(Record),
    ViewAgentHistory(Record),
    IterationEnd(Record),
    Probe,
    PendingDrafts,
    Grant,
    Tokens,
    Accounts,
    SwitchAccount(String),
    PersonalTokens,
    NewPersonalToken(bool),
}
enum After {
    Library(market_ui::Reply),
    Identity(identity_ui::Reply),
    Inbox(inbox_ui::Reply),
    DirectoryOpen,
    DirectoryTranscript,
    Work(workspace_ui::Response),
    Review(review_ui::Response),
    Conversation(conversation_ui::Reply),
    OrganizedChat,
    OpenChat,
    SpacePreview(Record),
    SpaceMembership,
    ChatShareCreated,
    ChatShareOpened,
    InvitationLink,
    JoinedOrganization,
    CheckAgent(u64),
    SetupFinished,
    Guide,
    Personal,
    TaskConversation,
    RunConversation,
    MovedTask,
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
    AccessIdentity,
    Accounts,
    PersonalTokens,
    PersonalTokenIssued,
}
struct Pending {
    after: After,
    space: String,
    write: bool,
}
pub struct Workbench {
    pub inbox: inbox_ui::State,
    pub identity_link: Option<String>,
    pub agent_query: String,
    pub library: market_ui::State,
    pub agent_every_space: bool,
    pub work: workspace_ui::State,
    pub communication: conversation_ui::State,
    pub review: review_ui::State,
    pub guide: onboarding::Guide,
    pub personal: crate::personal::Personal,
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
    pub chat_undo: Option<(String, String, String)>,
    pub versions: Vec<Value>,
    pub view_tasks: Vec<Record>,
    pub metrics: Value,
    pub drawer_tab: String,
    pub tokens: Vec<Value>,
    pub access_identity: Value,
    pub account_rows: Vec<Value>,
    pub active_profile: Option<String>,
    saved_place: Value,
    session_started: bool,
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
            agent_query: String::new(),
            library: market_ui::State::default(),
            agent_every_space: false,
            communication: Default::default(),
            work: workspace_ui::State::default(),
            review: review_ui::State::default(),
            inbox: Default::default(),
            identity_link: None,
            guide: onboarding::Guide::default(),
            personal: crate::personal::Personal::default(),
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
            chat_undo: None,
            versions: vec![],
            view_tasks: vec![],
            metrics: Value::Null,
            drawer_tab: String::new(),
            tokens: vec![],
            access_identity: Value::Null,
            account_rows: vec![],
            active_profile: None,
            saved_place: Value::Null,
            session_started: false,
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
        !matches!(self.role.as_str(), "viewer" | "read") && !self.current_archived()
    }
    pub fn notify_text(&mut self, s: impl Into<String>) {
        self.inbox.banner = None;
        self.chat_undo = None;
        let message = s.into();
        self.notice = Some((crate::locale::label(&message).to_owned(), Instant::now()));
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
        let mut kinds = vec![self.loc.page.info().kind, "space", "member", "chat_sidebar"];
        if self.loc.page.is_tasks() || self.loc.view.is_some() {
            kinds.extend([
                "view",
                "tag",
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
            Page::Members if matches!(self.role.as_str(), "admin" | "owner") => {
                kinds.push("member_invitation")
            }
            Page::Iterations => kinds.extend(["task", "iteration_settings"]),
            Page::Dashboard | Page::Projects | Page::Views => kinds.push("task"),
            Page::Assistant | Page::Chats => kinds.extend(["agent", "chat", "chat_invitation"]),
            Page::Market => kinds.extend(["skill", "agent"]),
            Page::Skills => kinds.extend(["machine", "agent"]),
            Page::Inbox => kinds.push("task"),
            Page::Spaces | Page::Organizations => kinds.push("organization"),
            Page::Artifacts => kinds.push("artifact"),
            Page::Agents | Page::Crews | Page::Automations | Page::Tools => {
                if self.loc.page == Page::Agents {
                    kinds.extend(["agent_directory", "agent_archived"]);
                }
                if self.loc.page == Page::Crews {
                    kinds.extend(["crew_directory", "crew_archived"]);
                }
                kinds.extend(["agent", "crew", "machine", "skill", "tool"])
            }
            _ => {}
        }
        if let Some(item) = &self.loc.selected {
            match item.kind.as_str() {
                "task" => kinds.extend([
                    "chat",
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
                "chat" => kinds.extend(["message", "run", "artifact", "agent", "task"]),
                "agent" | "crew" => {
                    kinds.extend([
                        "skill",
                        "tool",
                        "machine",
                        "agent",
                        "crew",
                        "agent_settings",
                    ]);
                    if self.drawer_tab == "runs" {
                        kinds.push("agent_history");
                    }
                }
                "automation" => kinds.extend(["run", "agent", "crew"]),
                "project" | "iteration" => kinds.push("task"),
                "machine" => kinds.push("run"),
                _ => {}
            }
        }
        if self.form.is_some() {
            if self
                .form
                .as_ref()
                .is_some_and(|f| matches!(f.mode, Mode::Inbox(inbox_ui::InboxForm::Followers(..))))
                && matches!(self.role.as_str(), "admin" | "owner")
            {
                kinds.push("member_invitation");
            }
            kinds.extend([
                "tag",
                "tag_group",
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
        if self.module_disabled(Page::Iterations) && self.loc.page != Page::Iterations {
            kinds.retain(|k| *k != "iteration");
        }
        kinds.sort_unstable();
        kinds.dedup();
        let _ = self.backend.sender.send(Request::Read {
            handler: self
                .loc
                .selected
                .as_ref()
                .filter(|r| matches!(r.kind.as_str(), "agent" | "crew"))
                .map(|r| (r.kind.clone(), r.id.clone())),
            chat: self
                .loc
                .selected
                .as_ref()
                .filter(|r| r.kind == "chat")
                .map(|r| r.id.clone()),
            generation: self.generation,
            space: self.loc.space.clone(),
            kinds: kinds.into_iter().map(str::to_owned).collect(),
        });
        self.poll_at = Instant::now() + Duration::from_secs(5);
        self.conversation_refresh();
        self.review_refresh();
        self.inbox_refresh();
        self.send(
            json!({"op":"onboarding.get","space":self.loc.space}),
            After::Guide,
            false,
        );
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
                        "assignees"
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
                json!({"op":"view.run","space":self.loc.space,"id":id,"changes":self.view_changes()}),
                After::View(id),
                false,
            );
        }
    }
    fn who(&mut self) {
        if self.mode == "team" {
            self.send(json!({"op":"access.whoami"}), After::AccessIdentity, false);
        }
        self.send(
            json!({"op":"whoami","space":self.loc.space}),
            After::Who,
            false,
        );
    }
    fn tick(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.slash_tick(cx);
        self.view_preview(cx);
        crate::appearance::apply(&self.personal.options, w, cx, false);
        let mut changed = false;
        while let Ok(event) = self.backend.results.try_recv() {
            changed = true;
            match event {
                Event::Ready(result) => match result {
                    Ok((mode, session)) => {
                        self.restore_session(&session);
                        self.guide = onboarding::Guide::default();
                        self.review = review_ui::State::default();
                        self.communication = Default::default();
                        self.mode = mode;
                        self.access_identity = Value::Null;
                        self.tokens.clear();
                        self.personal = crate::personal::Personal::default();
                        self.send(json!({"op":"preferences.get"}), After::Personal, false);
                        self.error = None;
                        self.who();
                        self.read(true);
                        if self.loc.section == "accounts" {
                            self.send(json!({"op":"profiles.list"}), After::Accounts, false);
                        }
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
                    self.discover_document_links();
                    self.sync_bound_fields(w, cx);
                    if self.chat_undo.as_ref().is_some_and(|(s, id, _)| {
                        s == &self.loc.space
                            && self.list("chat_sidebar").iter().any(|room| &room.id == id)
                    }) {
                        self.chat_undo = None;
                        self.notice = None;
                    }
                    if let Some(view) = &self.loc.view
                        && let Some(catalog) = self.rows.get("view")
                    {
                        self.loc.view = catalog.iter().find(|v| v.id == view.id).cloned();
                        if self.loc.view.is_none() {
                            self.view_tasks.clear();
                        }
                    }
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
                    if self
                        .find("space", &self.loc.space)
                        .is_none_or(|s| s.fields.get("joined") == Some(&Value::Bool(false)))
                        && let Some(first) = self
                            .list("space")
                            .iter()
                            .find(|s| s.fields.get("joined") == Some(&Value::Bool(true)))
                            .cloned()
                    {
                        self.switch_space(first);
                    }
                }
                Event::Reply { ticket, result } => {
                    if let Some(p) = self.pending.remove(&ticket) {
                        if p.write {
                            self.busy = false;
                        }
                        if p.space != self.loc.space && !matches!(p.after, After::Personal) {
                            continue;
                        }
                        match result {
                            Ok(value) => self.reply(p.after, value, w, cx),
                            Err(error) => match p.after {
                                After::Inbox(inbox_ui::Reply::List(generation))
                                    if generation == self.generation =>
                                {
                                    self.inbox.clear_rows();
                                    self.error = Some(error);
                                }
                                After::Inbox(inbox_ui::Reply::Feed(_)) => {
                                    if self.inbox.banner.take().is_some() {
                                        self.notice = None;
                                    }
                                }
                                After::Review(
                                    review_ui::Response::Settings | review_ui::Response::Queue(_),
                                ) => {
                                    self.review.clear_queue();
                                    self.error = Some(error);
                                }
                                After::Review(review_ui::Response::Timeline(..)) => {
                                    self.review.timeline = Value::Null;
                                    self.error = Some(error);
                                }
                                After::Conversation(conversation_ui::Reply::Embeds(key, _)) => {
                                    self.communication.clear_embed(key);
                                }
                                After::Conversation(
                                    conversation_ui::Reply::Outline(_)
                                    | conversation_ui::Reply::OutlineLink(_),
                                ) => {
                                    self.communication.clear_outline();
                                    self.error = Some(error);
                                }
                                After::Guide => self.guide.failed(error),
                                After::Personal => {
                                    if p.write {
                                        self.send(
                                            json!({"op":"preferences.get"}),
                                            After::Personal,
                                            false,
                                        );
                                    }
                                    self.notify_text(format!(
                                        "{} · {error}",
                                        crate::locale::label("个人设置未同步")
                                    ));
                                }
                                After::Connected if self.loc.section == "accounts" => {
                                    self.error = Some(error);
                                    self.send(
                                        json!({"op":"profiles.list"}),
                                        After::Accounts,
                                        false,
                                    );
                                }
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
        if !self.loading && !self.busy && !self.reading {
            self.remember_place();
        }
        if !self.loading && !self.busy && !self.reading && Instant::now() >= self.poll_at {
            self.read(false);
        }
        if self
            .form
            .as_ref()
            .is_some_and(|f| matches!(f.mode, Mode::Search | Mode::BindChat(_)))
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
            self.chat_undo = None;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }
    fn current_search(&self, q: &str, cx: &App) -> bool {
        self.form.as_ref().is_some_and(|f| {
            matches!(f.mode, Mode::Search | Mode::BindChat(_))
                && f.entries.first().is_some_and(|e| e.raw(cx) == q)
        })
    }
    fn search(&mut self, q: String) {
        let mut request = json!({"op":"search","space":self.loc.space,"query":q});
        if self
            .form
            .as_ref()
            .is_some_and(|f| matches!(f.mode, Mode::BindChat(_)))
        {
            request["kind"] = json!("task");
        }
        self.send(request, After::Search(q), false);
    }
    fn reply(&mut self, after: After, value: Value, w: &mut Window, cx: &mut Context<Self>) {
        if value.get("queued").and_then(Value::as_bool) == Some(true) {
            self.close_form(w, cx);
            self.notify_text("已保存离线草稿，重连后可同步。");
            return;
        }
        match after {
            After::Library(reply) => self.library_reply(reply, value, w, cx),
            After::Identity(reply) => self.identity_reply(reply, value, w, cx),
            After::Inbox(reply) => self.inbox_reply(reply, value, w, cx),
            After::DirectoryOpen => {
                self.close_form(w, cx);
                if let Ok(row) = serde_json::from_value(value) {
                    self.open_handler(row);
                }
            }
            After::DirectoryTranscript => {
                let content = value["messages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|m| {
                        format!(
                            "{}\n{}",
                            m["author"].as_str().unwrap_or(""),
                            m["body"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                self.open_form(
                    Mode::Output {
                        heading: value["title"].as_str().unwrap_or("运行历史").into(),
                        content,
                    },
                    w,
                    cx,
                );
            }
            After::Conversation(r) => self.communication_reply(r, value, w, cx),
            After::Review(r) => self.review_response(r, value, w, cx),
            After::Work(r) => self.work_reply(r, value, w, cx),
            After::OrganizedChat => self.chat_organized(&value, w, cx),
            After::OpenChat => {
                self.close_form(w, cx);
                if let Ok(room) = serde_json::from_value(value) {
                    self.open(room, true);
                }
            }
            After::SpacePreview(mut space) => {
                space.rev = value["rev"].as_i64().unwrap_or(space.rev);
                self.open_form(
                    Mode::DeactivateSpace {
                        space,
                        unfinished: value["unfinished"].as_u64().unwrap_or_default(),
                        token: value["preview_token"].as_str().unwrap_or_default().into(),
                    },
                    w,
                    cx,
                );
            }
            After::SpaceMembership => self.space_membership_reply(&value, w, cx),
            After::RunConversation => {
                if let Ok(chat) = serde_json::from_value::<Record>(value["chat"].clone()) {
                    self.open(chat, true);
                }
            }
            After::Personal => {
                if let Ok(prefs) = serde_json::from_value(value["preferences"].clone()) {
                    self.personal.options = prefs;
                    crate::locale::select(&self.personal.options.language);
                    self.personal.rev = value["rev"].as_i64().unwrap_or(1);
                    crate::appearance::apply(&self.personal.options, w, cx, true);
                }
            }
            After::TaskConversation => {
                self.close_form(w, cx);
                if let Ok(chat) = serde_json::from_value::<Record>(value) {
                    self.open(chat, true);
                }
            }
            After::MovedTask => {
                self.close_form(w, cx);
                if let Ok(task) = serde_json::from_value::<Record>(value) {
                    if let Some(space) = self.find("space", &task.space).cloned() {
                        self.switch_space(space);
                    }
                    self.open(task, true);
                }
            }
            After::Saved(kind) => {
                if kind == "view"
                    && let Ok(view) = serde_json::from_value::<Record>(value.clone())
                {
                    self.close_form(w, cx);
                    self.act(UiAction::OpenView(view), w, cx);
                    self.notify_text("已保存视图");
                    return;
                }
                let open_assignment = self.personal.options.open_after_assignment
                    && kind == "task"
                    && matches!(
                        value["fields"]["assignee_kind"].as_str(),
                        Some("agent" | "crew")
                    )
                    && value["fields"]["assignee"].is_string()
                    && self.form.as_ref().is_some_and(|f| match &f.mode {
                        Mode::Create(k) => k == "task",
                        Mode::Edit(old) => {
                            old.fields["assignee"] != value["fields"]["assignee"]
                                || old.fields["assignee_kind"] != value["fields"]["assignee_kind"]
                        }
                        _ => false,
                    });
                let new_task = value["id"].as_str().map(str::to_owned);
                self.close_form(w, cx);
                self.notify_text("已保存");
                if let Ok(r) = serde_json::from_value::<Record>(value)
                    && !r.id.is_empty()
                    && !matches!(kind.as_str(), "message" | "comment")
                {
                    if r.kind == "space"
                        && self.loc.page == Page::Spaces
                        && !self
                            .find("space", &r.id)
                            .is_some_and(|s| s.fields.get("joined") == Some(&Value::Bool(false)))
                    {
                        self.switch_space(r);
                    } else if r.kind == "space" {
                        self.loc.selected = None;
                    } else {
                        self.loc.selected = Some(r);
                    }
                }
                self.read(false);
                if open_assignment && let Some(id) = new_task {
                    self.personal_conversation(&id);
                }
            }
            After::Notice(s) => {
                self.close_form(w, cx);
                self.notify_text(s);
                self.read(false);
                if self.loc.page == Page::Settings && self.loc.section == "drafts" {
                    self.send(json!({"op":"sync.pending"}), After::Drafts, false);
                }
                if self.loc.page == Page::Settings && self.loc.section == "tokens" {
                    self.send(json!({"op":"access.list"}), After::Tokens, false);
                }
                if self.loc.page == Page::Settings && self.loc.section == "personal_tokens" {
                    self.send(
                        json!({"op":"access.personal.list"}),
                        After::PersonalTokens,
                        false,
                    );
                }
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
            After::Guide => {
                self.guide.accept(value);
                if self.mode != "demo"
                    && !self.smoke
                    && self.form.is_none()
                    && self.loc.section != "accounts"
                    && self.guide.offer_setup()
                {
                    self.open_form(Mode::SetupOrganization, w, cx);
                }
            }
            After::CheckAgent(revision) if self.revision == revision && self.form.is_none() => {
                let ready = value.as_array().is_some_and(|machines| {
                    machines.iter().any(|m| m["fields"]["disabled"] != true)
                });
                self.open_form(
                    if ready {
                        Mode::Create("agent".into())
                    } else {
                        Mode::MissingMachine
                    },
                    w,
                    cx,
                );
            }
            After::CheckAgent(_) => {}
            After::SetupFinished => {
                self.guide.accept(value);
                self.close_form(w, cx);
                self.navigate(Page::GettingStarted);
            }
            After::Who => {
                self.actor = value
                    .get("actor")
                    .and_then(Value::as_str)
                    .unwrap_or("owner")
                    .into();
                self.inbox_ready();
                if let Some(id) = self.identity_link.take() {
                    self.send(
                        json!({"op":"profiles.verification","id":id}),
                        After::Identity(identity_ui::Reply::Resume),
                        false,
                    );
                }
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
            After::ChatShareCreated => {
                if let Some(link) = value["link"].as_str().or(value["share"].as_str()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.to_owned()));
                    self.open_form(
                        Mode::Output {
                            heading: if self.mode == "team" {
                                "已复制对话分享链接"
                            } else {
                                "已复制本工作区分享码"
                            }
                            .into(),
                            content: link.to_owned(),
                        },
                        w,
                        cx,
                    );
                    self.read(false);
                }
            }
            After::ChatShareOpened => {
                self.open_form(
                    Mode::Output {
                        heading: "已分享的对话".into(),
                        content: shipforge_core::share_text(&value),
                    },
                    w,
                    cx,
                );
            }
            After::InvitationLink => {
                if let Some(link) = value["link"].as_str() {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.to_owned()));
                    self.open_form(
                        Mode::Output {
                            heading: "邀请链接已复制".into(),
                            content: link.into(),
                        },
                        w,
                        cx,
                    );
                    self.read(false);
                }
            }
            After::JoinedOrganization => {
                self.close_form(w, cx);
                if let Some(org) = value["organization"].as_str() {
                    self.loc.space = org.into();
                    self.rows.clear();
                    self.errors.clear();
                    self.guide = onboarding::Guide::default();
                    self.who();
                    self.navigate(Page::Members);
                    self.notify_text("已加入组织");
                }
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
            After::AccessIdentity => self.access_identity = value,
            After::Accounts => {
                self.account_rows = value["accounts"].as_array().cloned().unwrap_or_default();
                self.loc.section = "accounts".into();
            }
            After::PersonalTokens => {
                self.tokens = value.as_array().cloned().unwrap_or_default();
                self.loc.section = "personal_tokens".into();
            }
            After::PersonalTokenIssued => {
                if let Some(token) = value["token"].as_str() {
                    self.open_form(
                        Mode::Output {
                            heading: crate::locale::label("复制个人令牌并妥善保存").into(),
                            content: token.to_owned(),
                        },
                        w,
                        cx,
                    );
                } else {
                    self.close_form(w, cx);
                    self.notify_text("此令牌已签发，完整值仅在创建时显示一次。");
                }
                if self.loc.page == Page::Settings {
                    self.send(
                        json!({"op":"access.personal.list"}),
                        After::PersonalTokens,
                        false,
                    );
                }
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
        self.guide.close_menu();
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
            self.guide.change_space();
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
        if self.history[next].space != self.loc.space {
            if !self.rows.contains_key("space") {
                return;
            }
            if !self
                .find("space", &self.history[next].space)
                .is_some_and(|s| s.fields.get("joined") == Some(&Value::Bool(true)))
            {
                self.history.remove(next);
                if next < self.pos {
                    self.pos -= 1;
                }
                return;
            }
        }
        self.history[self.pos] = self.loc.clone();
        let oldspace = self.loc.space.clone();
        self.loc = self.history[next].clone();
        self.pos = next;
        self.revision += 1;
        self.reset_detail();
        if self.loc.space != oldspace {
            self.rows.clear();
            self.guide.change_space();
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
        self.open_prefilled(mode, Map::new(), w, cx);
    }
    pub fn open_invitation(&mut self, link: String, w: &mut Window, cx: &mut Context<Self>) {
        self.open_prefilled(
            Mode::JoinInvite,
            Map::from_iter([("link".into(), json!(link))]),
            w,
            cx,
        );
    }
    fn open_prefilled(
        &mut self,
        mode: Mode,
        preset: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        let mut value = Map::new();
        match &mode {
            Mode::Library(market_ui::LibraryForm::Search) => {
                value.insert("query".into(), json!(self.library.query));
                value.insert("category".into(), json!(self.library.category));
            }
            Mode::Library(_) => {
                if let Some(agent) = self.loc.selected.as_ref().filter(|r| r.kind == "agent") {
                    value.insert("agent".into(), json!(agent.id));
                }
            }
            Mode::FindInView | Mode::FindLabels => value = self.loc.filter.clone(),
            Mode::MoveTask => {
                if let Some(task) = self.loc.selected.as_ref().filter(|r| r.kind == "task") {
                    value.insert("task".into(), json!(task.id));
                }
            }
            Mode::Edit(r) => {
                value = initial_record(r);
                if r.kind == "task"
                    && let Some(bindings) = self
                        .find("task_type", r.field("type"))
                        .and_then(|ty| ty.fields.get("field_bindings"))
                        .and_then(Value::as_array)
                    && let Some(custom) = value.get_mut("custom").and_then(Value::as_object_mut)
                {
                    custom.retain(|k, _| {
                        bindings
                            .iter()
                            .any(|b| b["field"] == *k && b["enabled"] != false)
                    });
                }
            }
            Mode::Identity(f) => value = f.prefill(),
            Mode::Conversation(f) => value = f.prefill(),
            Mode::Review(f) => value = f.prefill(),
            Mode::Work(workspace_ui::WorkForm::SaveAs(view)) => {
                value.insert("title".into(), json!(format!("{} 副本", view.title)));
            }
            Mode::BulkTags => {
                if let Some(task) = self.loc.selected.as_ref().filter(|r| r.kind == "task") {
                    value.insert("tasks".into(), json!([task.id]));
                }
            }
            Mode::MergeTags => {
                if let Some(tag) = self.loc.selected.as_ref().filter(|r| r.kind == "tag") {
                    value.insert("source".into(), json!(tag.id));
                }
            }
            Mode::Workflow {
                op: "iteration.configure",
                record,
            } => value = record.fields.clone(),
            Mode::Create(k) if k == "agent" => {
                if let Some(default) = self
                    .find("space", &self.loc.space)
                    .and_then(|s| s.fields.get("agent_type_default"))
                {
                    value.insert("agent_type".into(), default.clone());
                }
            }
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
                if let Some(t) = self
                    .list("task_type")
                    .iter()
                    .find(|r| r.fields.get("default") == Some(&Value::Bool(true)))
                {
                    value.insert("type".into(), json!(t.id));
                }
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
        value.extend(preset);
        let is_task = matches!(&mode,Mode::Create(k) if k=="task")
            || matches!(&mode,Mode::Edit(r) if r.kind=="task");
        let show_estimate = self
            .find("space", &self.loc.space)
            .is_some_and(|r| r.fields.get("estimates_enabled") == Some(&Value::Bool(true)));
        let mut form = Form::new(mode, &value, w, cx);
        if is_task {
            self.configure_assignees(&mut form, &value);
            self.task_field_editors(&mut form, w, cx);
        }
        if is_task && !show_estimate {
            form.entries.retain(|e| e.spec.key != "estimate");
        }
        self.form = Some(form);
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
            && (self.loc.page == Page::Inbox || self.history[self.pos - 1].space == self.loc.space)
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
        if self.archived_action(&action) {
            self.notify_text("该空间已停用，历史记录为只读。");
            cx.notify();
            return;
        }
        match action {
            IdentityProfile => self.send(
                json!({"op":"access.account.profile"}),
                After::Identity(identity_ui::Reply::Profile),
                false,
            ),
            IdentityClose => self.send(
                json!({"op":"access.account.impact"}),
                After::Identity(identity_ui::Reply::Impact),
                false,
            ),
            LibraryDetails(id) => self.send(
                json!({"op":"catalog.detail","id":id,"space":self.loc.space}),
                After::Library(market_ui::Reply::Detail),
                false,
            ),
            Directory(a) => self.directory_action(a, w, cx),
            Conversation(a) => self.communication_action(a, w, cx),
            Review(a) => self.review_action(a, w, cx),
            Work(a) => self.work_action(a, w, cx),
            Inbox(a) => self.inbox_action(a, w, cx),
            Chat(action) => self.chat_action(action, w, cx),
            Space(action) => self.space_action(action, w, cx),
            CopyInvitation(id) => self.send(
                json!({"op":"invitation.copy","space":self.loc.space,"id":id}),
                After::InvitationLink,
                true,
            ),
            Guide(action) => self.guide_action(action, w, cx),
            Personal(action) => self.personal_action(action, w, cx),
            Go(Page::GettingStarted) => {
                self.navigate(Page::GettingStarted);
                self.loc.section = "settings".into();
                self.history[self.pos] = self.loc.clone();
            }
            Go(p) => self.navigate_space_page(p),
            Open(r) => self.open(r, false),
            OpenSearch(r) => {
                if r.kind == "comment" {
                    self.work_action(
                        workspace_ui::Action::Locate {
                            task: r.field("task").into(),
                            comment: r.id,
                        },
                        w,
                        cx,
                    );
                    return;
                }
                self.close_form(w, cx);
                self.open(r, true);
            }

            Back => self.history(false),
            Forward => self.history(true),
            CloseDetail => self.act_detail_close(),

            Create(k) if k == "agent" => self.send(
                json!({"op":"list","kind":"machine","space":self.loc.space}),
                After::CheckAgent(self.revision),
                false,
            ),
            Create(k) => self.open_form(Mode::Create(k), w, cx),
            Edit(r) => self.open_form(Mode::Edit(r), w, cx),

            ToggleSidebar => self.sidebar = !self.sidebar,
            Refresh => {
                self.error = None;
                self.read(true);
            }
            TaskType(t) => {
                self.loc.view = None;
                self.loc.task_type = t;
                self.revision += 1;
                self.read(false);
            }
            Layout(l) => {
                self.loc.layout = l;
                self.revision += 1;
                self.read(false);
            }
            Section(s) => {
                self.loc.section = s;
                self.revision += 1;
            }
            Mine(v) => {
                self.loc.mine_created = v;
                self.revision += 1;
                self.read(false);
            }
            Filter => self.open_form(Mode::Filter, w, cx),
            ResetFilter => {
                self.loc.filter.clear();
                self.loc.task_type.clear();
                self.revision += 1;
                self.read(false);
            }
            SaveView => self.open_form(
                self.loc
                    .view
                    .clone()
                    .map(|v| Mode::Work(workspace_ui::WorkForm::SaveAs(v)))
                    .unwrap_or(Mode::Create("view".into())),
                w,
                cx,
            ),
            Search => self.open_form(Mode::Search, w, cx),

            SelectSpace(r) => self.switch_space(r),
            OpenView(v) => {
                self.close_form(w, cx);
                if self.loc.space != v.space {
                    self.loc.space = v.space.clone();
                    self.rows.clear();
                    self.role.clear();
                    self.who();
                }
                self.navigate(Page::Tasks);
                self.loc.task_type.clear();
                self.loc.filter.clear();
                self.loc.selected = None;
                self.loc.layout = v.field("layout").to_owned();
                self.loc.view = Some(v);
                self.view_tasks.clear();
                self.read(true);
            }

            Apply(mut cmd, msg) => {
                cmd["space"] = json!(self.loc.space);
                self.send(cmd, After::Notice(msg), true);
            }

            Form(mode) => self.open_form(mode, w, cx),
            CloseForm => self.close_form(w, cx),
            Submit => {
                if let Err(error) = self.submit(w, cx)
                    && let Some(f) = &mut self.form
                {
                    f.error = error;
                }
            }
            Pick(i) => {
                if let Some(f) = &mut self.form {
                    f.open_pick = (f.open_pick != Some(i)).then_some(i);
                    f.picker_search = f
                        .open_pick
                        .and_then(|index| f.entries.get(index))
                        .filter(|entry| entry.spec.format.label_picker())
                        .map(|_| {
                            crate::controls::Field::new(
                                "",
                                crate::locale::label("搜索标签"),
                                false,
                                false,
                                w,
                                cx,
                            )
                        });
                    if let Some(search) = &f.picker_search {
                        search.handle(cx).focus(w, cx);
                    }
                }
            }
            Choose(i, v) => {
                let type_changed = self.form.as_ref().is_some_and(|f| {
                    (matches!(&f.mode,Mode::Edit(r) if r.kind=="task")
                        || matches!(&f.mode,Mode::Create(k) if k=="task"))
                        && f.entries
                            .get(i)
                            .is_some_and(|e| e.spec.key == "type" && e.raw(cx) != v)
                });
                let setup_defaults = self
                    .form
                    .as_ref()
                    .is_some_and(|f| {
                        (matches!(f.mode, Mode::SetupSpace { .. })
                            || matches!(&f.mode,Mode::Create(k) if k=="space"))
                            && f.entries
                                .get(i)
                                .is_some_and(|e| e.spec.key == "work_type" && e.raw(cx) != v)
                    })
                    .then_some(if matches!(v.as_str(), "software" | "project") {
                        vec!["requirement".to_string(), "bug".to_string()]
                    } else {
                        vec!["task".to_string()]
                    });
                if let Some(f) = &mut self.form
                    && let Some(e) = f.entries.get_mut(i)
                    && let Control::Pick { value, focus } = &mut e.control
                {
                    value.choose(v);
                    if !matches!(value, crate::forms::Selection::Many(_)) {
                        f.open_pick = None;
                        f.picker_search = None;
                    }
                    if let Some(search) = &f.picker_search {
                        search.handle(cx).focus(w, cx);
                    } else {
                        focus.focus(w, cx);
                    }
                }
                if let Some(defaults) = setup_defaults
                    && let Some(entry) = self
                        .form
                        .as_mut()
                        .and_then(|f| f.entries.iter_mut().find(|e| e.spec.key == "task_types"))
                    && let Control::Pick { value, .. } = &mut entry.control
                {
                    *value = crate::forms::Selection::Many(defaults);
                }
                if type_changed && let Some(mut form) = self.form.take() {
                    self.task_field_editors(&mut form, w, cx);
                    self.form = Some(form);
                }
            }

            Delete(r) => {
                let command = json!({
                "op":"delete",
                "space":if matches!(r.kind.as_str(),"tag"|"tag_group"){&r.space}else{&self.loc.space},
                "kind":r.kind,
                "id":r.id,
                "expected_rev":r.rev}
                );
                self.open_form(
                    Mode::Confirm {
                        label: format!("删除「{}」", r.title),
                        command,
                    },
                    w,
                    cx,
                );
            }

            AddTrigger => {
                if let Some(f) = &mut self.form {
                    f.add_trigger(
                        &json!({
                        "type":"status"}
                        ),
                        w,
                        cx,
                    );
                }
            }
            RemoveTrigger(id) => {
                if let Some(f) = &mut self.form {
                    f.remove_trigger(id);
                }
            }
            Browse { index, save } => self.browse(index, save.as_deref(), w, cx),

            Inspect(mut cmd, label) => {
                cmd["space"] = json!(self.loc.space);
                self.send(cmd, After::Output(label), false);
            }

            Connect => self.open_form(Mode::Connect, w, cx),
            Disconnect => {
                let ticket = self.register(After::Connected, true);
                let _ = self.backend.sender.send(Request::Disconnect { ticket });
            }

            Versions(r) => self.send(
                json!({
                "op":"document.history",
                "id":r.id,
                "space":self.loc.space}
                ),
                After::Versions(r),
                false,
            ),

            RestoreVersion(r, v) => self.open_form(
                Mode::Confirm {
                    label: format!("恢复「{}」版本 {v}", r.title),
                    command: json!({
                    "op":"document.restore",
                    "id":r.id,
                    "space":self.loc.space,
                    "version":v,
                    "expected_rev":r.rev}
                    ),
                },
                w,
                cx,
            ),

            Copy(s) => {
                cx.write_to_clipboard(ClipboardItem::new_string(s));
                self.notify_text("已复制");
            }
            OpenSourceUrl(url) => {
                if url.starts_with("https://") && !url.chars().any(char::is_control) {
                    cx.open_url(&url);
                }
            }

            Export => self.open_form(Mode::Export, w, cx),
            Import => self.open_form(Mode::Import, w, cx),

            RuleFire(r) => {
                self.open_form(Mode::Confirm{
label:format!("运行「{}」",
r.title),
command:json!({
"op":"automation.fire","id":r.id,"space":self.loc.space,"event_id":format!("ui-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())}
)}
,
w,
cx);
            }

            ViewAgentHistory(r) => {
                self.drawer_tab = "runs".into();
                self.loc.selected = Some(r);
                self.read(false);
            }

            IterationEnd(r) => self.open_form(Mode::IterationEnd(r), w, cx),

            Probe => {
                let ticket = self.register(After::Output("本机 Runtime 探测结果".into()), false);
                let request = Request::Machine {
                    ticket,
                    space: self.loc.space.clone(),
                    fields: json!({"probe":true}),
                    stop: false,
                };
                if self.backend.sender.send(request).is_err() {
                    self.error = Some("工作区服务已关闭".into());
                }
            }
            PendingDrafts => self.send(
                json!({
                "op":"sync.pending"}
                ),
                After::Drafts,
                false,
            ),

            Grant => self.open_form(Mode::Token, w, cx),
            Accounts => {
                self.remember_place();
                self.navigate(Page::Settings);
                self.loc.section = "accounts".into();
                self.send(json!({"op":"profiles.list"}), After::Accounts, false);
            }
            SwitchAccount(id) => {
                self.remember_place();
                self.send(json!({"op":"profiles.use","id":id}), After::Connected, true);
            }
            PersonalTokens => self.send(
                json!({"op":"access.personal.list"}),
                After::PersonalTokens,
                false,
            ),
            NewPersonalToken(machine_setup) => self.open_form(
                Mode::PersonalToken {
                    machine_setup,
                    request_id: uuid::Uuid::new_v4().to_string(),
                },
                w,
                cx,
            ),
            Tokens => self.send(
                json!({
                "op":"access.list",
                "space":self.loc.space}
                ),
                After::Tokens,
                false,
            ),
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
                files: !matches!(
                    self.form.as_ref().map(|f| &f.mode),
                    Some(Mode::Library(market_ui::LibraryForm::Directory))
                ),
                directories: matches!(
                    self.form.as_ref().map(|f| &f.mode),
                    Some(Mode::Library(market_ui::LibraryForm::Directory))
                ),
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
        match mode {
            Mode::BindChat(_) => {
                self.search(val("query"));
            }
            Mode::ChatRow(_) => self.close_form(w, cx),
            Mode::DeactivateSpace {
                space,
                unfinished,
                token,
            } => {
                let mut command = json!({"op":"space.deactivate","space":space.id,"expected_rev":space.rev,"preview_token":token});
                if unfinished > 0 {
                    let choice = val("unfinished");
                    if !matches!(choice.as_str(), "move" | "cancel") {
                        return Err(crate::locale::label("请选择未完成任务的处理方式。").into());
                    }
                    command["unfinished"] = json!(choice);
                    if choice == "move" {
                        let target = val("target");
                        if target.is_empty() {
                            return Err(crate::locale::label("请选择目标空间。").into());
                        }
                        command["target"] = json!(target);
                    }
                }
                self.send(command, After::SpaceMembership, true);
            }
            Mode::Invite { email } => {
                let mut q = json!({"op":"invitation.issue","space":self.loc.space,"channel":if email{"email"}else{"link"},"role":val("role"),"spaces":values["spaces"]});
                if email {
                    q["email"] = json!(val("email"));
                }
                self.send(q, After::InvitationLink, true);
            }
            Mode::JoinInvite => self.send(
                json!({"op":"invitation.accept","link":val("link")}),
                After::JoinedOrganization,
                true,
            ),
            Mode::OpenChatShare => {
                let pasted = val("share");
                let share = pasted.trim().rsplit('/').next().unwrap_or_default();
                self.send(
                    json!({"op":"chat.share.read","share":share}),
                    After::ChatShareOpened,
                    false,
                );
            }
            Mode::SetupOrganization => {
                let organization = val("organization_name");
                let slug = val("organization_slug");
                if organization.is_empty() || slug.is_empty() {
                    return Err("请填写组织名称和地址标识".into());
                }
                self.open_form(Mode::SetupSpace { organization, slug }, w, cx);
            }
            Mode::SetupSpace { organization, slug } => {
                self.send(json!({"op":"onboarding.setup","space":self.loc.space,"organization_name":organization,
                    "organization_slug":slug,"work_type":val("work_type"),"task_types":values.get("task_types")}),After::SetupFinished,true);
            }
            Mode::MissingMachine => {
                self.close_form(w, cx);
                self.guide_action(onboarding::Action::Start("connect_machine"), w, cx);
            }
            Mode::Identity(f) => self.identity_submit(f, Value::Object(values))?,
            Mode::Directory(f) => self.directory_submit(f, values, w, cx)?,
            Mode::Library(f) => self.library_submit(f, values, w, cx)?,
            Mode::Conversation(f) => self.communication_submit(f, values, w, cx)?,
            Mode::Review(f) => self.review_submit(f, values, w, cx)?,
            Mode::Work(f) => self.work_submit(f, values, w, cx)?,
            Mode::Inbox(f) => self.inbox_submit(f, values, w, cx)?,
            Mode::BulkTags => {
                let mut cmd = Value::Object(values);
                cmd["op"] = json!("tag.apply");
                cmd["space"] = json!(self.loc.space);
                self.send(cmd, After::Notice("标签已更新".into()), true);
            }
            Mode::MergeTags => {
                let source = self.find("tag", &val("source")).ok_or("请选择待合并标签")?;
                let target = self.find("tag", &val("target")).ok_or("请选择目标标签")?;
                let label = format!("{} → {}", source.title, target.title);
                let command = json!({"op":"tag.merge","id":source.id,"target":target.id,"space":source.space,"expected_rev":source.rev});
                self.open_form(Mode::Confirm { label, command }, w, cx);
            }
            Mode::GuideTarget { step, kind } => {
                let record = self
                    .find(kind, &val("target"))
                    .cloned()
                    .ok_or("请先选择对象")?;
                self.guide_target(step, record, w, cx);
            }
            Mode::FindInView | Mode::FindLabels => {
                let query = val("query");
                if query.is_empty() {
                    self.loc.filter.remove("query");
                } else {
                    self.loc.filter.insert("query".into(), json!(query));
                }
                self.close_form(w, cx);
                self.read(false);
            }
            Mode::MoveTask => {
                let id = val("task");
                let target = val("target");
                let task = self.find("task", &id).ok_or("请选择要移动的任务")?;
                if target.is_empty() {
                    return Err("请选择目标空间".into());
                }
                self.send(json!({"op":"task.move","id":id,"expected_rev":task.rev,"space":self.loc.space,"target":target}),After::MovedTask,true);
            }
            Mode::Create(ref k) | Mode::Edit(Record { kind: ref k, .. }) => {
                let old = if let Mode::Edit(r) = &mode {
                    Some(r)
                } else {
                    None
                };
                let (title, mut fields) = compile_record(k, values, old.is_none())?;
                if k == "view"
                    && let Some(view) = old
                {
                    workspace_ui::retain_other_filters(view, &mut fields);
                }
                if k == "task"
                    && old.is_some_and(|r| {
                        r.fields.get("iteration").unwrap_or(&Value::Null)
                            == fields.get("iteration").unwrap_or(&Value::Null)
                    })
                {
                    fields.remove("iteration");
                }

                if k == "automation"
                    && let Some(id) = fields.get("handler_id").and_then(Value::as_str)
                {
                    let handler = if self.find("crew", id).is_some() {
                        "crew"
                    } else {
                        "agent"
                    };
                    fields.insert("handler_kind".into(), json!(handler));
                }

                let mut cmd = json!({
                "op":if old.is_some(){
                "update"}
                else{
                "create"}
                ,
                "kind":k,
                "space":self.loc.space,
                "title":title,
                "fields":fields}
                );

                if let Some(r) = old {
                    cmd["id"] = json!(r.id);
                    cmd["expected_rev"] = json!(r.rev);
                    if k == "space" {
                        cmd["space"] = json!(r.id);
                        if r.fields.get("triage_enabled") == Some(&json!(true))
                            && cmd["fields"]["triage_enabled"] == false
                        {
                            cmd["confirm"] = json!(true);
                            self.open_form(
                                Mode::Confirm {
                                    label: "关闭分拣并将等待任务移至待规划".into(),
                                    command: cmd,
                                },
                                w,
                                cx,
                            );
                            return Ok(());
                        }
                    }
                } else if k == "space" {
                    let organization = cmd["fields"]["organization"]
                        .as_str()
                        .or_else(|| {
                            self.find("space", &self.loc.space)
                                .map(|s| s.field("organization"))
                        })
                        .unwrap_or("default");
                    cmd["space"] = json!(organization);
                }
                if matches!(k.as_str(), "tag" | "tag_group") {
                    if let Some(r) = old {
                        cmd["space"] = json!(r.space);
                    } else if cmd["fields"]["scope"] == "organization" {
                        let org = self
                            .find("space", &self.loc.space)
                            .ok_or("请先选择所属空间")?
                            .field("organization");
                        cmd["space"] = json!(org);
                    }
                }
                if self.field_change(k, old, &cmd) {
                    return Ok(());
                }
                self.send(cmd, After::Saved(k.clone()), true);
            }

            Mode::ThreadReply {
                task,
                target,
                request_id,
            } => {
                let body = val("body");
                if body.trim().is_empty() {
                    return Err("请输入回复内容。".into());
                }
                self.send(json!({"op":"im.reply","space":self.loc.space,"id":target["id"],"task_id":task,"thread":target["thread"],"body":body,"request_id":request_id}),After::Saved("comment".into()),true);
            }
            Mode::Comment(ref r) | Mode::Message(ref r) => {
                let body = val("body");
                if body.is_empty() {
                    return Err("请输入内容。".into());
                }
                if matches!(mode, Mode::Message(_)) && chat_actions::agent_room(r) {
                    let op = if body.starts_with('/') {
                        "chat.slash.send"
                    } else {
                        "chat.send"
                    };
                    self.send(json!({"op":op,"space":self.loc.space,"id":r.id,"body":body,"request_id":uuid::Uuid::new_v4().to_string()}),After::Notice("消息已排队".into()),true);
                    return Ok(());
                }
                if matches!(mode, Mode::Comment(_)) && body == "已审阅，没问题" {
                    self.send(json!({"op":"task.review","space":r.space,"id":r.id,"body":body,"request_id":uuid::Uuid::new_v4().to_string()}),After::Saved("comment".into()),true);
                    return Ok(());
                }
                let (kind, key) = if matches!(mode, Mode::Comment(_)) {
                    ("comment", "task")
                } else {
                    ("message", "chat")
                };
                let mut fields = values;
                fields.insert(key.into(), json!(r.id));
                fields.insert("body".into(), json!(body));

                self.send(
                    json!({
                    "op":"create",
                    "kind":kind,
                    "space":self.loc.space,
                    "title":body.chars().take(60).collect::<String>(),
                    "fields":fields
                    }
                    ),
                    After::Saved(kind.into()),
                    true,
                );
            }

            Mode::Filter => {
                self.loc.filter = values;
                self.loc.filter.retain(|_, v| {
                    !v.is_null()
                        && v.as_str() != Some("")
                        && !v.as_array().is_some_and(Vec::is_empty)
                });
                self.close_form(w, cx);
                self.read(false);
            }

            Mode::Connect | Mode::Reauthenticate { .. } => {
                let (url, account) = match mode {
                    Mode::Reauthenticate { url, account } => (url, Some(account)),
                    _ => (val("url"), None),
                };
                let token = val("token");
                if url.is_empty() || token.is_empty() {
                    return Err("请输入服务地址和令牌。".into());
                }
                let ticket = self.register(After::Connected, true);
                let _ = self.backend.sender.send(Request::Connect {
                    url,
                    token,
                    account,
                    ticket,
                });
            }

            Mode::Confirm { command, .. } => {
                let after = if matches!(
                    command["op"].as_str(),
                    Some("space.restore" | "space.leave")
                ) {
                    After::SpaceMembership
                } else if command["op"] == "customer_request.convert" {
                    After::Review(review_ui::Response::Opened)
                } else {
                    After::Notice("操作已完成".into())
                };
                self.send(command, after, true)
            }

            Mode::IterationEnd(r) => self.send(
                json!({
                "op":"iteration.close",
                "space":self.loc.space,
                "id":r.id,
                "target":values.get("target"),
                "expected_rev":r.rev}
                ),
                After::Notice("迭代已结束".into()),
                true,
            ),

            Mode::Export | Mode::Import => {
                let path = PathBuf::from(val("path"));
                if !path.is_absolute() {
                    return Err("请输入文件的完整路径。".into());
                }

                let sp = self.loc.space.clone();
                let req = if matches!(mode, Mode::Export) {
                    let ticket = self.register(After::Notice("空间数据已导出".into()), true);
                    Request::Export {
                        ticket,
                        command: json!({
                        "op":"export","space":sp}
                        ),
                        path,
                    }
                } else {
                    let confirmed = preview
                        .as_ref()
                        .and_then(|p| p.get("path"))
                        .and_then(Value::as_str)
                        == path.to_str();
                    let after = if confirmed {
                        After::Notice("导入已完成".into())
                    } else {
                        After::Preview(path.clone())
                    };
                    let ticket = self.register(after, true);
                    Request::Import {
                        ticket,
                        command: json!({
                        "op":if confirmed{
                        "import"}
                        else{
                        "import.preview"}
                        ,"space":sp}
                        ),
                        path,
                    }
                };
                let _ = self.backend.sender.send(req);
            }

            Mode::Upload(_) | Mode::Download(_, _) => {
                let path = PathBuf::from(val("path"));
                if !path.is_absolute() {
                    return Err("请选择完整文件路径。".into());
                }

                let (command, after) = match &mode {
                    Mode::Upload(record) => {
                        let mut q = json!({
                        "op":"attachment.upload","kind":"document","space":self.loc.space}
                        );
                        if let Some(r) = record {
                            q["id"] = json!(r.id);
                            q["kind"] = json!(r.kind);
                            q["expected_rev"] = json!(r.rev);
                        } else {
                            q["visibility"] = json!(val("visibility"));
                        }
                        (
                            q,
                            After::Saved(
                                record.as_ref().map_or("document", |r| &r.kind).to_owned(),
                            ),
                        )
                    }
                    Mode::Download(r, file) => (
                        json!({
                        "op":"attachment.download","kind":r.kind,"id":r.id,"attachment_id":file["id"],"space":self.loc.space}
                        ),
                        After::Notice(format!("已保存到 {}", path.display())),
                    ),
                    _ => unreachable!(),
                };

                let ticket = self.register(after, true);
                self.backend
                    .sender
                    .send(Request::Attachment {
                        ticket,
                        command,
                        path,
                    })
                    .map_err(|e| e.to_string())?;
            }

            Mode::Machine { stop } => {
                let ticket = self.register(After::Machine, true);
                self.backend
                    .sender
                    .send(Request::Machine {
                        ticket,
                        space: self.loc.space.clone(),
                        fields: Value::Object(values),
                        stop,
                    })
                    .map_err(|e| e.to_string())?;
            }

            Mode::Workflow { op, record } => {
                let mut q = Value::Object(values);
                if op == "iteration.configure" {
                    q = json!({"fields":q,"expected_rev":record.rev});
                }
                if op == "iteration.configure"
                    && q["fields"]["mode"] == "disabled"
                    && record.field("mode") != "disabled"
                {
                    q["op"] = json!(op);
                    q["space"] = json!(self.loc.space);
                    self.open_form(
                        Mode::Confirm {
                            label: "关闭迭代、结束当前期并移除未开始的期".into(),
                            command: q,
                        },
                        w,
                        cx,
                    );
                    return Ok(());
                }
                q["op"] = json!(op);
                q["space"] = json!(self.loc.space);
                q["id"] = json!(record.id);

                if op == "chat.agents" {
                    q["op"] = json!("chat.manage");
                    q["target_kind"] = json!("agent");
                }
                if matches!(op, "chat.manage" | "chat.agents") {
                    q["expected_rev"] = json!(record.rev);
                }

                if op == "tool.authorize" {
                    q["id"] = q["agent"].take();
                    q["tool_id"] = json!(record.id);
                }

                if op == "triage" {
                    q["decision"] = json!("duplicate");
                    q["expected_rev"] = json!(record.rev);
                }

                let inspect = matches!(
                    op,
                    "tool.authorize"
                        | "connector.export"
                        | "provider.preview"
                        | "zoho.projects"
                        | "zentao.credential"
                        | "zentao.projects"
                        | "zentao.preview"
                        | "pingcode.credential"
                        | "pingcode.projects"
                        | "pingcode.preview"
                        | "linear.authorize"
                        | "linear.accounts"
                        | "linear.credential"
                        | "linear.projects"
                        | "linear.preview"
                        | "clickup.authorize"
                        | "clickup.accounts"
                        | "clickup.credential"
                        | "clickup.projects"
                        | "clickup.preview"
                        | "ones.accounts"
                        | "ones.credential"
                        | "ones.projects"
                        | "ones.preview"
                        | "imports.push"
                        | "imports.push_history"
                        | "imports.sync_control"
                        | "imports.synchronize"
                        | "zoho.preview"
                        | "imports.commit"
                        | "imports.history"
                        | "imports.rollback"
                        | "bugly.rotate"
                        | "bugly.revoke"
                        | "bugly.outbox"
                        | "bugly.deliver"
                        | "bugly.credentials"
                        | "zoho.credential"
                        | "tapd.credential"
                        | "tapd.preview"
                        | "tapd.projects"
                );
                let after = if op == "chat.share.create" {
                    q["include_questions"] = json!(true);
                    After::ChatShareCreated
                } else if inspect {
                    After::Output(crate::forms::workflow_name(op).into())
                } else {
                    After::Notice("已保存".into())
                };
                self.send(q, after, !inspect);
            }

            Mode::ResolveDraft(draft) => {
                let rev = values
                    .get("expected_rev")
                    .and_then(Value::as_i64)
                    .filter(|v| *v > 0)
                    .ok_or("请输入有效的服务器版本")?;
                self.send(
                    json!({
                    "op":"sync.resolve",
                    "id":draft["request_id"],
                    "expected_rev":rev}
                    ),
                    After::Notice("草稿已更新，可重新同步".into()),
                    true,
                );
            }

            Mode::PersonalToken {
                machine_setup,
                request_id,
            } => {
                if val("label").trim().is_empty() {
                    return Err(crate::locale::label("请输入令牌的用途名称。").into());
                }
                self.send(json!({"op":"access.personal.issue","label":val("label"),"lifetime":val("lifetime"),
                    "source":if machine_setup{"machine_setup"}else{"personal"},"request_id":request_id}),After::PersonalTokenIssued,true);
            }
            Mode::Token => {
                if val("actor").is_empty() {
                    return Err("请选择成员。".into());
                }
                self.send(
                    json!({
                    "op":"access.issue",
                    "actor":val("actor"),
                    "scope":val("scope"),
                    "space":self.loc.space}
                    ),
                    After::Output("访问令牌（请妥善保存）".into()),
                    true,
                );
            }

            Mode::Search => self.search(val("query")),
            Mode::Output { .. } => self.close_form(w, cx),
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
            "$machine_skills" => self.library.sources.clone(),
            "$library_runtimes" => self
                .list("machine")
                .iter()
                .filter_map(|r| r.fields.get("runtimes").and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|v| (v.to_owned(), v.to_owned()))
                .collect(),
            "$tag_choices" => {
                let current = self.form.as_ref().and_then(|form| match &form.mode {
                    Mode::Edit(task) if task.kind == "task" => {
                        task.fields.get("labels")?.as_array()
                    }
                    _ => None,
                });
                self.list("tag")
                    .iter()
                    .filter(|tag| {
                        tag.fields.get("archived") != Some(&Value::Bool(true))
                            || current.is_some_and(|ids| ids.iter().any(|id| id == &tag.id))
                    })
                    .map(|tag| (tag.id.clone(), tag.title.clone()))
                    .collect()
            }
            "$deactivation_targets" => self.space_move_targets(),
            "$chat_messages" => {
                let room = self.loc.selected.as_ref().map(|s| s.id.as_str());
                self.list("message")
                    .iter()
                    .filter(|m| {
                        Some(m.field("chat")) == room
                            && (m.field("run").is_empty()
                                || self
                                    .find("run", m.field("run"))
                                    .is_some_and(|r| r.field("status") == "done"))
                    })
                    .map(|m| {
                        let body = m.field("body");
                        (
                            m.id.clone(),
                            if body.is_empty() {
                                m.title.clone()
                            } else {
                                body.chars().take(80).collect()
                            },
                        )
                    })
                    .collect()
            }
            "$invite_spaces" => {
                let org = self
                    .find("space", &self.loc.space)
                    .map(|s| s.field("organization"))
                    .unwrap_or("default");
                self.list("space")
                    .iter()
                    .filter(|s| {
                        s.field("organization") == org
                            && s.id != org
                            && s.fields.get("deactivated") != Some(&Value::Bool(true))
                    })
                    .map(|s| (s.id.clone(), s.title.clone()))
                    .collect()
            }
            "$other_spaces" => self.space_move_options(&self.loc.space),
            "$weekdays" => ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                .iter()
                .enumerate()
                .map(|(i, l)| ((i + 1).to_string(), l.to_string()))
                .collect(),
            "$sources" => vec![
                ("guest".into(), "访客".into()),
                ("external".into(), "外部集成".into()),
            ],
            "$project_owners" => {
                let mut rows = from("member")
                    .into_iter()
                    .map(|(id, name)| (format!("member/{id}"), name))
                    .collect::<Vec<_>>();
                rows.extend(
                    from("agent")
                        .into_iter()
                        .map(|(id, name)| (format!("agent/{id}"), format!("Agent · {name}"))),
                );
                rows
            }
            "$executors" => self.executor_choices(),
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
            "$setup_types" => [
                ("requirement", "需求"),
                ("bug", "缺陷"),
                ("task", "任务"),
                ("story", "用户故事"),
            ]
            .iter()
            .map(|(v, l)| (v.to_string(), crate::locale::label(l).to_owned()))
            .collect(),
            "$types" if !self.list("task_type").is_empty() => self
                .list("task_type")
                .iter()
                .filter(|r| r.fields.get("enabled") != Some(&json!(false)))
                .map(|r| (r.id.clone(), r.title.clone()))
                .collect(),
            "$types" => {
                let mut v: Vec<_> = crate::catalog::TYPES
                    .iter()
                    .map(|(v, l)| (v.to_string(), l.to_string()))
                    .collect();
                overlay_options(&mut v, from("task_type"));
                v.retain(|(id, _)| {
                    self.find("task_type", id)
                        .is_none_or(|r| r.fields.get("enabled") != Some(&Value::Bool(false)))
                });
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
                    && if self.loc.mine_created {
                        r.field("creator") != self.actor
                    } else {
                        !r.members().contains(&self.actor.as_str())
                    }
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
                        && !r.matches_filter(key, value)
                    {
                        return false;
                    }
                }
                if let Some(wanted) = self
                    .loc
                    .filter
                    .get("filter_labels")
                    .and_then(Value::as_array)
                    && !wanted.iter().all(|id| {
                        r.fields
                            .get("labels")
                            .and_then(Value::as_array)
                            .is_some_and(|a| a.contains(id))
                    })
                {
                    return false;
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
                    || r.fields
                        .get("labels")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .any(|id| self.name("tag", id).to_lowercase().contains(&q))
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
        if self.slash_key(e.keystroke.key.as_str(), w, cx) || self.personal_key(e, w, cx) {
            return;
        }
        let key = e.keystroke.key.as_str();
        let mods = e.keystroke.modifiers;
        let action = if key == "escape" {
            if let Some(f) = &mut self.form
                && let Some(index) = f.open_pick.take()
            {
                f.picker_search = None;
                if let Some(entry) = f.entries.get(index) {
                    entry.handle(cx).focus(w, cx);
                }
                cx.stop_propagation();
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
        } else if !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.shift
            && let Some(review) = self.review_keys(key)
        {
            Some(review)
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

#[cfg(all(test, windows))]
#[path = "agent_directory_ui_test.rs"]
mod agent_directory_ui_test;
#[cfg(all(test, windows))]
#[path = "chat_ui_test.rs"]
mod chat_ui_test;
#[cfg(all(test, windows))]
#[path = "cloud_tracker_ui_test.rs"]
mod cloud_tracker_ui_test;
#[cfg(all(test, windows))]
#[path = "conversation_ui_test.rs"]
mod conversation_ui_test;
#[cfg(all(test, windows))]
#[path = "market_ui_test.rs"]
mod market_ui_test;
#[cfg(all(test, windows))]
#[path = "onboarding_ui.rs"]
pub(crate) mod onboarding_ui;
#[cfg(all(test, windows))]
#[path = "review_ui_test.rs"]
mod review_ui_test;
#[cfg(all(test, windows))]
#[path = "space_ui_test.rs"]
mod space_ui_test;
#[cfg(all(test, windows))]
#[path = "tag_ui.rs"]
mod tag_ui;
#[cfg(all(test, windows))]
#[path = "tracker_ui_test.rs"]
mod tracker_ui_test;
#[cfg(all(test, windows))]
#[path = "workspace_ui_test.rs"]
mod workspace_ui_test;
