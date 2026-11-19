use super::*;
use crate::{
    appearance::paint as rgb,
    controls::*,
    forms::{Format, Spec, item},
};
#[derive(Clone)]
pub enum Action {
    Open(Record),
    Chat(Record),
    Scope,
    Settings,
    Target { kind: String, id: String },
    Clear,
}
#[derive(Clone)]
pub enum DirectoryForm {
    Search,
    Variant(Record),
    Override(Record),
    Access(Record),
    Mention(Record),
}
const SETTINGS: &[(&str, &str)] = &[
    ("instructions", "职责与指令"),
    ("description", "描述"),
    ("model", "默认模型"),
    ("runtime", "Runtime 名称"),
    ("machine_id", "指定机器"),
    ("profile", "工作目录配置"),
    ("directory_mode", "工作目录策略"),
    ("agent_type", "Agent 类型"),
    ("max_parallel", "任务并发"),
    ("chat_parallel", "对话并发"),
    ("working_hours", "工作时间"),
    ("environment", "环境变量"),
    ("arguments", "自定义参数"),
    ("timeout_seconds", "超时秒数"),
    ("repository", "代码库"),
    ("visibility", "可见范围"),
    ("avatar", "头像"),
    ("tools", "授权工具"),
];
impl DirectoryForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Search => "搜索目录",
            Self::Variant(_) => "创建分身",
            Self::Override(_) => "覆盖或恢复继承",
            Self::Access(_) => "成员访问权限",
            Self::Mention(_) => "在评论中提及 Agent",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        use Format::*;
        match self {
            Self::Search => vec![item("query", "名称或描述", Text, "")],
            Self::Variant(_) => vec![
                item("target_space", "目标空间", Ref("space"), ""),
                item("title", "分身名称", Text, ""),
            ],
            Self::Override(_) => vec![
                item("field", "设置项", Choice(SETTINGS), "instructions"),
                item(
                    "action",
                    "变更方式",
                    Choice(&[("set", "覆盖"), ("inherit", "恢复继承")]),
                    "set",
                ),
                item("value", "设置值", Body, ""),
            ],
            Self::Access(r) => vec![
                item("actor", "成员", Ref("member"), ""),
                item(
                    "permission",
                    "权限",
                    if r.kind == "agent" {
                        Choice(&[
                            ("use", "可使用"),
                            ("manage", "可管理"),
                            ("remove", "撤销授权"),
                            ("transfer", "转让所有权"),
                        ])
                    } else {
                        Choice(&[("transfer", "转让所有权")])
                    },
                    if r.kind == "agent" { "use" } else { "transfer" },
                ),
            ],
            Self::Mention(_) => vec![
                item("agent", "Agent", Ref("agent"), ""),
                item("body", "评论内容", Body, ""),
            ],
        }
    }
}
impl Workbench {
    pub(super) fn open_handler(&mut self, row: Record) {
        if row.space != self.loc.space
            && let Some(target) = self.find("space", &row.space).cloned()
        {
            self.switch_space(target);
        }
        self.open(row, true);
    }
    pub(super) fn directory_action(&mut self, a: Action, _w: &mut Window, cx: &mut Context<Self>) {
        match a {
            Action::Open(r) => self.open_handler(r),
            Action::Chat(r) => {
                let mut fields =
                    json!({"handler_kind":r.kind,"handler_id":r.id,"visibility":"private"});
                if r.kind == "agent" {
                    fields["agent"] = json!(r.id);
                }
                self.send(json!({"op":"create","kind":"chat","space":r.space,"title":r.title,"fields":fields,"request_id":uuid::Uuid::new_v4().to_string()}),After::DirectoryOpen,true);
            }
            Action::Scope => self.agent_every_space = !self.agent_every_space,
            Action::Clear => self.agent_query.clear(),
            Action::Settings => {
                self.drawer_tab.clear();
                self.read(false);
            }
            Action::Target { kind, id } if kind == "manager_transcript" => {
                if let Some(handler) = &self.loc.selected {
                    self.send(json!({"op":"agent.history","space":self.loc.space,"id":handler.id,"chat":id}),After::DirectoryTranscript,false);
                }
            }
            Action::Target { kind, id } => self.send(
                json!({"op":"get","space":self.loc.space,"kind":kind,"id":id}),
                After::DirectoryOpen,
                false,
            ),
        }
        cx.notify();
    }
    pub(super) fn directory_submit(
        &mut self,
        f: DirectoryForm,
        values: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let val = Value::Object(values);
        let (r, mut q) = match f {
            DirectoryForm::Search => {
                self.agent_query = val["query"].as_str().unwrap_or("").trim().into();
                self.close_form(w, cx);
                return Ok(());
            }
            DirectoryForm::Variant(r) => {
                let mut q = json!({"op":"agent.variant","target_space":val["target_space"]});
                if let Some(title) = val["title"].as_str().filter(|v| !v.trim().is_empty()) {
                    q["title"] = json!(title);
                }
                (r, q)
            }
            DirectoryForm::Override(r) => {
                let field = val["field"].as_str().unwrap_or("");
                let text = val["value"].as_str().unwrap_or("");
                let value = if val["action"] == "inherit" {
                    Value::Null
                } else if matches!(
                    field,
                    "environment"
                        | "arguments"
                        | "working_hours"
                        | "tools"
                        | "max_parallel"
                        | "chat_parallel"
                        | "timeout_seconds"
                ) {
                    serde_json::from_str(text).map_err(|_| "请输入该设置对应的 JSON 值")?
                } else {
                    json!(text)
                };
                (
                    r,
                    json!({"op":"agent.override","field":field,"value":value}),
                )
            }
            DirectoryForm::Access(r) => (
                r,
                json!({"op":"agent.access","actor":val["actor"],"permission":val["permission"]}),
            ),
            DirectoryForm::Mention(task) => {
                let q = json!({"op":"create","kind":"comment","space":task.space,"title":"Agent 提及","fields":{"task":task.id,"body":val["body"],"agent_mentions":[val["agent"]]},"request_id":uuid::Uuid::new_v4().to_string()});
                self.send(q, After::Notice("已发布评论".into()), true);
                return Ok(());
            }
        };
        q["space"] = json!(r.space);
        q["id"] = json!(r.id);
        q["kind"] = json!(r.kind);
        q["expected_rev"] = json!(r.rev);
        q["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        let next = if matches!(q["op"].as_str(), Some("agent.variant" | "agent.override")) {
            After::DirectoryOpen
        } else {
            After::Notice("设置已更新".into())
        };
        self.send(q, next, true);
        Ok(())
    }
    fn directory_button(
        &self,
        id: String,
        label: &'static str,
        a: UiAction,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        outlined(self.button(id, label, a, cx)).child(crate::locale::label(label))
    }
    pub(crate) fn agent_directory_page(&self, cx: &Context<Self>) -> Div {
        let crew = self.loc.page == Page::Crews;
        let kind = if crew { "crew" } else { "agent" };
        let section = if self.loc.section.is_empty() {
            "all"
        } else {
            &self.loc.section
        };
        let mut head = row()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(9.))
            .border_b_1()
            .border_color(rgb(LINE))
            .child(headline(if crew { "小队" } else { "Agent 目录" }, 18.))
            .child(div().flex_1())
            .child(self.directory_button(
                "directory-scope".into(),
                if self.agent_every_space {
                    "全部可访问空间"
                } else {
                    "当前空间"
                },
                UiAction::Directory(Action::Scope),
                cx,
            ))
            .child(self.directory_button(
                "directory-search".into(),
                "搜索目录",
                UiAction::Form(Mode::Directory(DirectoryForm::Search)),
                cx,
            ));
        if self.writable() {
            head = head.child(self.directory_button(
                "directory-create".into(),
                if crew { "新建小队" } else { "新建 Agent" },
                UiAction::Create(kind.into()),
                cx,
            ));
        }
        let mut filters = row().flex_wrap().gap(px(5.)).px(px(16.)).py(px(8.));
        for (key, label) in [
            ("all", "全部"),
            ("mine", "我的"),
            ("shared", "共享"),
            ("private", "私有"),
            ("favorite", "收藏"),
            ("archived", "已归档"),
        ] {
            filters = filters.child(
                outlined(self.button(
                    format!("directory-tab-{key}"),
                    label,
                    UiAction::Section(key.into()),
                    cx,
                ))
                .when(section == key, |b| b.bg(rgb(0xe8e7f3)))
                .child(crate::locale::label(label)),
            );
        }
        if !self.agent_query.is_empty() {
            filters = filters
                .child(self.directory_button(
                    "clear-agent-search".into(),
                    "清除搜索",
                    UiAction::Directory(Action::Clear),
                    cx,
                ))
                .child(text(self.agent_query.clone(), 12., MUTED));
        }
        let mut list = col()
            .id("agent-directory-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(16.));
        let needle = self.agent_query.to_lowercase();
        let mut count = 0;
        for r in self.list(&format!(
            "{kind}_{}",
            if section == "archived" {
                "archived"
            } else {
                "directory"
            }
        )) {
            if !self.agent_every_space && r.space != self.loc.space {
                continue;
            }
            if section == "mine" && r.field("creator") != self.actor
                || section == "private" && r.field("visibility") != "private"
                || section == "shared" && r.field("visibility") == "private"
                || section == "favorite"
                    && r.fields
                        .get("personal")
                        .is_none_or(|v| v["favorite"] != true)
            {
                continue;
            }
            if !needle.is_empty()
                && !r.title.to_lowercase().contains(&needle)
                && !r.field("description").to_lowercase().contains(&needle)
            {
                continue;
            }
            count += 1;
            let primary = self
                .button(
                    format!("directory-{kind}-{}-{}", r.space, r.id),
                    r.title.clone(),
                    UiAction::Directory(Action::Open(r.clone())),
                    cx,
                )
                .flex_1()
                .justify_start()
                .child(icon(
                    if crew {
                        "users"
                    } else if r.field("parent_id").is_empty() {
                        "bot"
                    } else {
                        "git-fork"
                    },
                    16.,
                ))
                .child(text(r.title.clone(), 14., INK));
            list = list.child(
                row()
                    .items_center()
                    .gap(px(8.))
                    .min_h(px(43.))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(primary)
                    .child(text(r.field("space_title").to_owned(), 12., MUTED))
                    .child(self.personal_handler_buttons(r, cx)),
            );
        }
        if count == 0 {
            list = list.child(text("没有符合条件的成员", 13., MUTED).py(px(16.)));
        }
        col().size_full().child(head).child(filters).child(list)
    }
    fn personal_handler_buttons(&self, r: &Record, cx: &Context<Self>) -> Div {
        let mut line = row().gap(px(4.));
        for (key, on, off) in [
            ("pinned", ("unpin", "取消置顶"), ("pin", "置顶")),
            ("favorite", ("unfavorite", "取消收藏"), ("favorite", "收藏")),
        ] {
            let (action, label) = if r.fields.get("personal").is_some_and(|v| v[key] == true) {
                on
            } else {
                off
            };
            let q = json!({"op":"agent.organize","space":r.space,"kind":r.kind,"id":r.id,"action":action});
            line = line.child(self.directory_button(
                format!("handler-{action}-{}-{}", r.space, r.id),
                label,
                UiAction::Apply(q, "列表已更新".into()),
                cx,
            ));
        }
        line
    }
    pub(crate) fn handler_panel(&self, r: &Record, cx: &Context<Self>) -> Div {
        let mut panel = col().gap(px(8.));
        let info = self
            .list("agent_settings")
            .iter()
            .find(|v| v.id == r.id)
            .map(|v| &v.fields);
        let settings = info
            .and_then(|v| v.get("record"))
            .and_then(|v| serde_json::from_value::<Record>(v.clone()).ok())
            .unwrap_or_else(|| r.clone());
        let manage =
            self.writable() && settings.fields.get("can_manage") == Some(&Value::Bool(true));
        if self.drawer_tab == "runs" {
            panel = panel.child(self.directory_button(
                "handler-settings".into(),
                "返回设置",
                UiAction::Directory(Action::Settings),
                cx,
            ));
            if settings.field("visibility") == "private"
                && settings.field("directory_owner") != self.actor
            {
                return panel.child(text("只有所有者可以查看私有成员的运行记录", 13., MUTED));
            }
            for item in self.list("agent_history") {
                if item.fields.get("masked") == Some(&Value::Bool(true)) {
                    panel = panel.child(text("私人对话", 14., MUTED));
                } else {
                    panel = panel.child(
                        self.button(
                            format!("handler-history-{}", item.id),
                            item.title.clone(),
                            UiAction::Directory(Action::Target {
                                kind: item.field("kind").into(),
                                id: item.field("id").into(),
                            }),
                            cx,
                        )
                        .justify_start()
                        .child(text(item.title.clone(), 14., INK)),
                    );
                }
            }
            if self.list("agent_history").is_empty() {
                panel = panel.child(text("暂无运行记录", 13., MUTED));
            }
            return panel;
        }
        panel = panel.child(self.personal_handler_buttons(&settings, cx));
        if let Some(data) = info {
            panel = panel.child(
                row()
                    .gap(px(7.))
                    .child(text("累计 Token", 13., MUTED))
                    .child(text(
                        data.get("total_tokens")
                            .and_then(Value::as_u64)
                            .map_or_else(|| "—".into(), |v| v.to_string()),
                        13.,
                        INK,
                    )),
            );
        }
        if self.writable()
            && !settings.fields.get("archived").is_some_and(|v| v == true)
            && settings.field("exclusive_crew").is_empty()
        {
            panel = panel.child(self.directory_button(
                "start-handler-chat".into(),
                "发起对话",
                UiAction::Directory(Action::Chat(settings.clone())),
                cx,
            ));
        }
        if manage {
            let archived = settings.fields.get("archived") == Some(&Value::Bool(true));
            let label = if archived {
                "恢复使用"
            } else {
                "归档成员"
            };
            let q = json!({"op":if archived{"agent.restore"}else{"agent.archive"},"space":r.space,"kind":r.kind,"id":r.id,"expected_rev":settings.rev});
            panel = panel
                .child(self.directory_button(
                    "archive-handler".into(),
                    label,
                    UiAction::Form(Mode::Confirm {
                        label: label.into(),
                        command: q,
                    }),
                    cx,
                ))
                .child(self.directory_button(
                    "access-handler".into(),
                    "成员访问权限",
                    UiAction::Form(Mode::Directory(DirectoryForm::Access(settings.clone()))),
                    cx,
                ));
        }
        if settings.fields.get("can_variant") == Some(&Value::Bool(true)) && self.writable() {
            panel = panel.child(self.directory_button(
                "variant-handler".into(),
                "创建分身",
                UiAction::Form(Mode::Directory(DirectoryForm::Variant(settings.clone()))),
                cx,
            ));
        }
        if r.kind == "agent" {
            let is_variant = !settings.field("parent_id").is_empty();
            if manage && !is_variant {
                panel = panel.child(self.directory_button(
                    "agent-add-skill".into(),
                    "添加技能",
                    UiAction::Form(Mode::Library(super::market_ui::LibraryForm::Add)),
                    cx,
                ));
            }
            if is_variant && manage {
                panel = panel.child(self.directory_button(
                    "variant-override".into(),
                    "覆盖或恢复继承",
                    UiAction::Form(Mode::Directory(DirectoryForm::Override(settings.clone()))),
                    cx,
                ));
            }
            if let Some(eff) = info.and_then(|v| v.get("effective")) {
                for &(field, label) in SETTINGS {
                    if let Some(v) = eff.get(field) {
                        let state = if is_variant {
                            if settings.fields.get(field).is_some_and(|v| !v.is_null()) {
                                "已覆盖"
                            } else {
                                "继承"
                            }
                        } else {
                            ""
                        };
                        panel = panel.child(
                            row()
                                .min_h(px(28.))
                                .gap(px(8.))
                                .child(text(label, 12., MUTED))
                                .child(text(crate::catalog::value_text(v), 13., INK).flex_1())
                                .child(text(state, 12., MUTED)),
                        );
                    }
                }
            }
        } else {
            let shared = settings.fields.get("share_task_directory") == Some(&Value::Bool(true));
            let label = if shared {
                "关闭同机共享任务目录"
            } else {
                "开启同机共享任务目录"
            };
            if manage {
                panel = panel.child(self.directory_button("crew-workspace".into(), label, UiAction::Apply(
                    json!({"op":"update","kind":"crew","space":r.space,"id":r.id,"expected_rev":settings.rev,"fields":{"share_task_directory":!shared}}),
                    "已更新小队目录设置".into()), cx));
            }
            for member in info
                .and_then(|v| v.get("members"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let id = member["id"].as_str().unwrap_or("");
                let mut line = row().gap(px(6.)).child(text(
                    member["title"].as_str().unwrap_or(id).to_owned(),
                    13.,
                    INK,
                ));
                let exclusive = member["exclusive"] == true;
                if exclusive {
                    line = line.child(text("小队专属", 12., MUTED));
                }
                if manage && member["leader"] != true {
                    let label = if exclusive {
                        "取消小队专属"
                    } else {
                        "设为小队专属"
                    };
                    let command = json!({"op":"crew.exclusive","space":r.space,"id":r.id,"agent":id,"exclusive":!exclusive,"expected_rev":settings.rev});
                    line = line.child(self.directory_button(
                        format!("crew-exclusive-{id}"),
                        label,
                        UiAction::Form(Mode::Confirm {
                            label: label.into(),
                            command,
                        }),
                        cx,
                    ));
                }
                panel = panel.child(line);
            }
        }
        panel
    }
}
