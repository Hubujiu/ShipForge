use super::*;
#[path = "review_ui/panels.rs"]
mod panels;
use chrono::Local;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct State {
    space: String,
    settings: Value,
    queue: Value,
    pub(super) timeline: Value,
    pub(super) basis: String,
    newest: bool,
    deferred: bool,
    codes: bool,
    dates: bool,
    selected: BTreeSet<String>,
    previous_m: Option<Instant>,
}
impl State {
    pub(super) fn clear_queue(&mut self) {
        self.settings = Value::Null;
        self.queue = Value::Null;
        self.selected.clear();
    }
}
#[path = "review_ui/forms.rs"]
mod forms;
use forms::RULE_INPUTS;
pub use forms::ReviewForm;
fn decision_label(choice: &str) -> &'static str {
    match choice {
        "accept" => "接受",
        "reject" => "拒绝",
        "defer" => "搁置",
        "duplicate" => "标记重复",
        "restore" => "恢复到队列",
        _ => "处置",
    }
}
#[derive(Clone)]
pub enum Action {
    Toggle(&'static str),
    Select(String),
    Batch(&'static str),
    Shift(usize, i32),
    ToggleRule(usize),
    DeleteRule(usize),
    Share(String),
    Open(Record),
    Basis(&'static str),
}
pub enum Response {
    Settings,
    Queue(u64),
    Timeline(String, String),
    Opened,
    Shared,
    Submitted,
}

impl Workbench {
    pub fn review_refresh(&mut self) {
        if self.review.space != self.loc.space {
            self.review = State {
                space: self.loc.space.clone(),
                codes: true,
                dates: true,
                ..State::default()
            };
        }
        if self.loc.page == Page::Triage {
            self.send(
                json!({"op":"triage.settings","space":self.loc.space}),
                After::Review(Response::Settings),
                false,
            );
            if self.review.settings["enabled"] == true {
                self.refresh_queue();
            }
        }
        if let Some(project) = self.loc.selected.as_ref().filter(|r| r.kind == "project") {
            let basis = if self.review.basis.is_empty() {
                "schedule"
            } else {
                &self.review.basis
            };
            self.send(json!({"op":"project.timeline","space":self.loc.space,"id":project.id,"basis":basis}),After::Review(Response::Timeline(project.id.clone(),basis.to_owned())),false);
        }
    }
    fn refresh_queue(&mut self) {
        let filters = self.view_changes();
        let mut q = json!({"op":"triage.queue","space":self.loc.space,"filter":filters["filter"],"order":if self.review.newest{"newest"}else{"oldest"},"include_deferred":self.review.deferred,"limit":5000});
        if let Some(query) = filters["query"].as_str() {
            q["query"] = json!(query);
        }
        self.send(q, After::Review(Response::Queue(self.generation)), false);
    }
    pub fn review_response(
        &mut self,
        response: Response,
        v: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match response {
            Response::Settings if self.loc.page == Page::Triage => {
                let first = self.review.settings["enabled"] != true;
                self.review.settings = v;
                if self.review.settings["enabled"] == true && first {
                    self.refresh_queue();
                }
            }
            Response::Queue(generation)
                if generation == self.generation && self.loc.page == Page::Triage =>
            {
                self.review.selected.retain(|id| {
                    v["items"]
                        .as_array()
                        .is_some_and(|ls| ls.iter().any(|r| r["id"] == *id))
                });
                self.review.queue = v;
            }
            Response::Timeline(id, basis)
                if self.loc.selected.as_ref().is_some_and(|r| r.id == id)
                    && (self.review.basis.is_empty() || self.review.basis == basis) =>
            {
                self.review.timeline = v
            }
            Response::Shared => {
                if let Some(link) = v["link"].as_str() {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.to_owned()));
                    self.notify_text("已复制任务链接");
                }
            }
            Response::Opened => {
                if v["submitted"] == true {
                    self.close_form(w, cx);
                    self.notify_text("请求已转为任务");
                    return;
                }
                let task = serde_json::from_value::<Record>(v).ok();
                if let Some(task) = task {
                    self.close_form(w, cx);
                    self.open_review_task(task, true);
                }
            }
            Response::Submitted => {
                self.close_form(w, cx);
                self.notify_text(format!("已提交任务 {}", v["number"].as_str().unwrap_or("")));
            }
            _ => {}
        }
        cx.notify();
    }
    pub fn open_task_link(&mut self, link: String) {
        self.send(
            json!({"op":"task.open","link":link}),
            After::Review(Response::Opened),
            false,
        );
    }
    fn open_review_task(&mut self, r: Record, route: bool) {
        if r.space != self.loc.space {
            let space = self
                .find("space", &r.space)
                .cloned()
                .unwrap_or_else(|| Record {
                    id: r.space.clone(),
                    ..Record::default()
                });
            self.switch_space(space);
        }
        self.open(r, route);
    }
    fn decide_form(rows: Vec<Record>, choice: &'static str) -> UiAction {
        UiAction::Form(Mode::Review(ReviewForm::Decide { rows, choice }))
    }
    fn queue_rows(&self) -> Vec<Record> {
        serde_json::from_value(self.review.queue["items"].clone()).unwrap_or_default()
    }
    fn can_review(&self) -> bool {
        matches!(self.role.as_str(), "owner" | "admin" | "editor" | "member")
            && !self.current_archived()
    }
    pub fn review_action(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Toggle(key) => {
                match key {
                    "newest" => self.review.newest = !self.review.newest,
                    "deferred" => self.review.deferred = !self.review.deferred,
                    "codes" => self.review.codes = !self.review.codes,
                    "dates" => self.review.dates = !self.review.dates,
                    _ => {}
                }
                if matches!(key, "newest" | "deferred") {
                    self.refresh_queue();
                }
            }
            Action::Select(id) => {
                if !self.review.selected.remove(&id) {
                    self.review.selected.insert(id);
                }
            }
            Action::Batch(choice) => {
                let rows = self
                    .queue_rows()
                    .into_iter()
                    .filter(|r| self.review.selected.contains(&r.id))
                    .collect::<Vec<_>>();
                if !rows.is_empty() {
                    self.act(Self::decide_form(rows, choice), w, cx);
                }
            }
            Action::Open(r) => self.open_review_task(r, self.loc.page == Page::Projects),
            Action::Share(id) => self.send(
                json!({"op":"task.share","space":self.loc.space,"id":id}),
                After::Review(Response::Shared),
                false,
            ),
            Action::Basis(basis) => {
                self.review.basis = basis.into();
                self.review_refresh();
            }
            a @ (Action::Shift(..) | Action::ToggleRule(_) | Action::DeleteRule(_)) => {
                let settings = &self.review.settings;
                let mut rules = settings["rules"].as_array().cloned().unwrap_or_default();
                match a {
                    Action::Shift(i, d) => {
                        let Some(j) = i
                            .checked_add_signed(d as isize)
                            .filter(|j| *j < rules.len())
                        else {
                            return;
                        };
                        if i >= rules.len() {
                            return;
                        }
                        rules.swap(i, j);
                    }
                    Action::ToggleRule(i) => {
                        let Some(rule) = rules.get_mut(i) else {
                            return;
                        };
                        rule["enabled"] = json!(rule["enabled"] == false);
                    }
                    Action::DeleteRule(i) => {
                        if i >= rules.len() {
                            return;
                        }
                        rules.remove(i);
                    }
                    _ => unreachable!(),
                }
                let cmd = json!({"op":"triage.configure","space":self.loc.space,"expected_rev":settings["rev"],"rules":rules});
                if matches!(a, Action::DeleteRule(_)) {
                    self.open_form(
                        Mode::Confirm {
                            label: "删除分拣规则".into(),
                            command: cmd,
                        },
                        w,
                        cx,
                    );
                } else {
                    self.send(cmd, After::Notice("规则已更新".into()), true);
                }
            }
        }
        cx.notify();
    }
    pub fn review_submit(
        &mut self,
        form: ReviewForm,
        values: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let v = Value::Object(values);
        let get = |key: &str| v[key].as_str().unwrap_or("").trim();
        let mut q = json!({"space":self.loc.space});
        match form {
            ReviewForm::OpenTask => {
                self.open_task_link(get("link").into());
                return Ok(());
            }
            ReviewForm::Submit(space) => {
                q = json!({"op":"triage.submit","space":space,"title":get("title"),"description":get("description"),"custom":v["custom"],"request_id":uuid::Uuid::new_v4().to_string()});
                if !get("type").is_empty() {
                    q["type"] = v["type"].clone();
                }
                self.send(q, After::Review(Response::Submitted), true);
                return Ok(());
            }
            ReviewForm::Config(s) => {
                q["op"] = json!("triage.configure");
                q["expected_rev"] = s["rev"].clone();
                for k in ["enabled", "priority_required", "sources"] {
                    q[k] = v[k].clone();
                }
                q["types"] = v["types"]
                    .as_array()
                    .filter(|ls| !ls.is_empty())
                    .map_or(Value::Null, |ls| json!(ls));
                let mode = get("duty_mode");
                q["duty"] = if mode == "none" || mode.is_empty() {
                    json!({"mode":"none"})
                } else {
                    json!({"mode":mode,"member":get("reviewer")})
                };
                if s["enabled"] == true && q["enabled"] == false {
                    q["confirm"] = json!(true);
                    self.open_form(
                        Mode::Confirm {
                            label: "关闭分拣并将等待任务移至待规划".into(),
                            command: q,
                        },
                        w,
                        cx,
                    );
                    return Ok(());
                }
            }
            ReviewForm::Rule { settings, index } => {
                let mut rules = settings["rules"].as_array().cloned().unwrap_or_default();
                let id = index
                    .and_then(|i| rules.get(i))
                    .map(|r| r["id"].clone())
                    .unwrap_or_else(|| json!(uuid::Uuid::new_v4().to_string()));
                if get("title").is_empty() {
                    return Err("请输入规则名称".into());
                }
                let mut item = json!({"id":id,"title":get("title"),"enabled":v["enabled"],"when":v["when"],"set":v["set"]});
                for (input, section, field) in RULE_INPUTS {
                    let value = &v[*input];
                    if value.as_str().is_some_and(|s| !s.is_empty())
                        || value.as_array().is_some_and(|a| !a.is_empty())
                    {
                        item[*section][*field] = value.clone();
                    }
                }

                if let Some(i) = index {
                    *rules.get_mut(i).ok_or("规则已变更，请刷新")? = item;
                } else {
                    rules.push(item);
                }
                q = json!({"op":"triage.configure","space":self.loc.space,"expected_rev":settings["rev"],"rules":rules});
            }
            ReviewForm::Decide { rows, choice } => {
                let Some(first) = rows.first() else {
                    return Err("请选择待分拣任务".into());
                };
                q["decision"] = json!(choice);
                if rows.len() == 1 {
                    q["op"] = json!("triage");
                    q["id"] = json!(first.id);
                    q["expected_rev"] = json!(first.rev);
                } else {
                    q["op"] = json!("triage.batch");
                    q["tasks"] = json!(rows.iter().map(|r| &r.id).collect::<Vec<_>>());
                    q["revisions"] = json!(
                        rows.iter()
                            .map(|r| (&r.id, r.rev))
                            .collect::<BTreeMap<_, _>>()
                    );
                }
                for (k, val) in v.as_object().unwrap() {
                    if !matches!(k.as_str(), "time" | "delay")
                        && !val.is_null()
                        && val.as_str() != Some("")
                        && val.as_array().is_none_or(|a| !a.is_empty())
                    {
                        q[k] = val.clone();
                    }
                }
                if choice == "defer" {
                    let until = if get("delay") == "custom" {
                        let naive =
                            chrono::NaiveDateTime::parse_from_str(get("time"), "%Y-%m-%d %H:%M")
                                .map_err(|_| "请输入有效的本地日期与时间")?;
                        naive
                            .and_local_timezone(Local)
                            .single()
                            .ok_or("本地时间不存在或不唯一")?
                            .timestamp_millis()
                    } else {
                        Local::now().timestamp_millis()
                            + get("delay").parse::<i64>().map_err(|_| "请选择搁置时间")? * 3_600_000
                    };
                    q["until"] = json!(until);
                }
            }
        }
        self.send(q, After::Notice("已保存".into()), true);
        Ok(())
    }
    pub fn review_keys(&mut self, key: &str) -> Option<UiAction> {
        if self.loc.page != Page::Triage || self.review.settings["enabled"] != true {
            return None;
        }
        if key == "c" && (self.can_review() || self.role == "guest") {
            return Some(UiAction::Form(Mode::Review(ReviewForm::Submit(
                self.loc.space.clone(),
            ))));
        }
        if !self.can_review() {
            return None;
        }
        let rows = self.queue_rows();
        let index = self
            .loc
            .selected
            .as_ref()
            .and_then(|task| rows.iter().position(|r| r.id == task.id));
        if matches!(key, "j" | "k") {
            let next = match (index, key) {
                (Some(i), "j") => (i + 1).min(rows.len().saturating_sub(1)),
                (Some(i), _) => i.saturating_sub(1),
                _ => 0,
            };
            return rows
                .get(next)
                .cloned()
                .map(|r| UiAction::Review(Action::Open(r)));
        }
        let task = rows.get(index?)?.clone();
        if key == "m" {
            let previous = self.review.previous_m.take();
            if previous.is_some_and(|t| t.elapsed() <= Duration::from_millis(750)) {
                return Some(Self::decide_form(vec![task], "duplicate"));
            }
            self.review.previous_m = Some(Instant::now());
            return None;
        }
        self.review.previous_m = None;
        match key {
            "1" => Some(Self::decide_form(vec![task], "accept")),
            "2" => Some(Self::decide_form(vec![task], "duplicate")),
            "3" => Some(Self::decide_form(vec![task], "reject")),
            "h" => Some(Self::decide_form(vec![task], "defer")),
            _ => None,
        }
    }
}
