use super::*;
use crate::controls::*;

pub(crate) fn chat_label(kind: &str) -> &'static str {
    match kind {
        "member" => "与真人对话",
        "agent" => "与 Agent 对话",
        "crew" => "与小队对话",
        _ => "新建群聊",
    }
}

impl Workbench {
    pub(crate) fn priority_options(&self) -> &'static [(&'static str, &'static str)] {
        let options = crate::catalog::PRIORITIES;
        let legacy = match self.form.as_ref().map(|f| &f.mode) {
            Some(Mode::Create(_)) => false,
            Some(Mode::Edit(r)) => r.field("priority") == "lowest",
            _ => self
                .list("task")
                .iter()
                .any(|r| r.field("priority") == "lowest"),
        };
        if legacy
            || self
                .find("space", &self.loc.space)
                .is_some_and(|r| r.fields["priority_levels"] == 5)
        {
            options
        } else {
            &options[..options.len() - 1]
        }
    }
    pub(crate) fn org_controls(&self, cx: &Context<Self>) -> Div {
        let mut actions = row().flex_wrap().gap(px(5.));
        let Some(org) = self.list("organization_settings").first() else {
            return actions;
        };
        if org.fields["can_configure"] == true && self.access_identity["scope"] != "read" {
            actions = actions.child(
                outlined(self.button(
                    "org-defaults",
                    "组织默认设置",
                    UiAction::Form(Mode::OrgDefaults(org.clone())),
                    cx,
                ))
                .child(crate::locale::label("组织默认设置")),
            );
        }
        if self.access_identity["personal"] != true && self.access_identity["scope"] != "read" {
            if org.fields.get("can_manage_owners") == Some(&Value::Bool(true)) {
                for (action, label) in [
                    ("grant", "添加组织所有者"),
                    ("transfer", "转让组织所有权"),
                    ("revoke", "将所有者改为管理员"),
                ] {
                    if (action == "transfer" && self.actor == "owner")
                        || (action == "revoke"
                            && org
                                .fields
                                .get("owners")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                <= 1)
                    {
                        continue;
                    }
                    actions = actions.child(
                        outlined(self.button(
                            format!("org-owners-{action}"),
                            label,
                            UiAction::Form(Mode::OrgOwnership(org.clone(), action)),
                            cx,
                        ))
                        .child(crate::locale::label(label)),
                    );
                }
            }
            for (permission, label, action) in [
                ("can_leave", "离开组织", "leave"),
                ("can_dissolve", "删除组织", "dissolve"),
            ] {
                if org.fields[permission] == true {
                    actions = actions.child(
                        outlined(self.button(
                            format!("org-{action}"),
                            label,
                            UiAction::Form(Mode::OrgDepart(org.clone(), action)),
                            cx,
                        ))
                        .child(crate::locale::label(label)),
                    );
                }
            }
        }
        actions
    }
    pub(crate) fn people_panel(&self, cx: &Context<Self>) -> Div {
        let kinds = [
            ("agent", "Agent"),
            ("crew", "小队"),
            ("member", "真人"),
            ("machine", "机器"),
        ];
        let mut tabs = row().flex_wrap().gap(px(5.));
        for (kind, label) in std::iter::once(("", "全部")).chain(kinds) {
            tabs = tabs.child(
                outlined(self.button(
                    format!("people-tab-{kind}"),
                    label,
                    UiAction::SetPeople(kind),
                    cx,
                ))
                .when(self.people_kind == kind, |b| {
                    b.font_weight(FontWeight::SEMIBOLD)
                })
                .child(crate::locale::label(label)),
            );
        }
        let mut body = col()
            .id("people-directory-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(4.))
            .p(px(14.));
        for (kind, label) in kinds {
            let rows: Vec<_> = self
                .list("people_directory")
                .iter()
                .filter(|r| r.kind == kind)
                .collect();
            if rows.is_empty() {
                continue;
            }
            body = body.child(headline(label, 14.).mt(px(7.)));
            for (i, r) in rows.into_iter().enumerate() {
                body = body.child(
                    self.button(
                        format!("people-row-{kind}-{i}"),
                        r.title.clone(),
                        UiAction::OpenSearch(r.clone()),
                        cx,
                    )
                    .w_full()
                    .justify_start()
                    .gap(px(8.))
                    .child(text(r.title.clone(), 13., INK))
                    .child(div().flex_1())
                    .child(text(
                        format!("{} · {}", self.name("space", &r.space), r.id),
                        11.,
                        MUTED,
                    )),
                );
            }
        }
        if self.list("people_directory").is_empty() {
            body = body.child(text("没有匹配的对象", 13., MUTED));
        }
        col()
            .size_full()
            .child(
                row()
                    .flex_wrap()
                    .gap(px(8.))
                    .p(px(12.))
                    .child(headline("成员目录", 16.))
                    .child(div().flex_1())
                    .child(
                        outlined(self.button(
                            "people-find",
                            "查找成员与机器",
                            UiAction::Form(Mode::PeopleFilter),
                            cx,
                        ))
                        .child(crate::locale::label("查找成员与机器")),
                    )
                    .child(
                        outlined(self.button(
                            "people-manage",
                            "空间成员",
                            UiAction::Go(Page::Members),
                            cx,
                        ))
                        .child(crate::locale::label("空间成员")),
                    ),
            )
            .child(tabs.px(px(12.)))
            .when(!self.people_query.is_empty(), |b| {
                b.child(text(self.people_query.clone(), 12., MUTED).px(px(12.)))
            })
            .child(body)
    }

    pub(super) fn joined_space(&self, id: &str) -> bool {
        self.find("space", id)
            .is_some_and(|s| s.fields.get("joined") == Some(&Value::Bool(true)))
    }
    pub(super) fn open_search_target(
        &mut self,
        scope: String,
        record: Record,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_form(w, cx);
        if self.loc.space != scope {
            let Some(area) = self.find("space", &scope).cloned() else {
                return;
            };
            if !self.joined_space(&scope) {
                return;
            }
            self.switch_space(area);
        }
        match record.kind.as_str() {
            "comment" => self.work_action(workspace_ui::Action::Locate { task:record.field("task").into(),comment:record.id },w,cx),
            "message" if !record.field("chat").is_empty() => self.send(json!({"op":"get","kind":"chat","space":scope,"id":record.field("chat"),"allow_cached":false}),After::OpenChat,false),
            _ => self.open(record,true),
        }
    }

    pub(crate) fn navigation_commands(&self, query: &str, quick: bool, cx: &Context<Self>) -> Div {
        let mut panel = col().gap(px(5.));
        let terms = query.trim().to_lowercase();
        let matches = |label: &str| {
            terms.is_empty()
                || label.to_lowercase().contains(&terms)
                || crate::locale::label(label).to_lowercase().contains(&terms)
        };
        let mut groups = Vec::new();
        if self.writable() {
            let mut items = vec![("新建任务", UiAction::Create("task".into()))];
            if self.role != "guest" {
                for kind in ["agent", "member", "group", "crew"] {
                    items.push((chat_label(kind), UiAction::Form(Mode::StartChat(kind))));
                }
            }
            groups.push(("快捷新建", items));
        }
        if !quick {
            let mut actions = vec![("在当前视图查找", UiAction::Personal(personal::Action::Find))];
            if self.writable() && self.role != "guest" {
                actions.push((
                    "将任务移动到空间",
                    UiAction::Personal(personal::Action::Move),
                ));
            }
            groups.push(("操作", actions));
            let pages = [
                Page::Tasks,
                Page::Mine,
                Page::Inbox,
                Page::Chats,
                Page::Members,
                Page::Projects,
                Page::Iterations,
                Page::Docs,
                Page::Agents,
                Page::Crews,
                Page::Machines,
                Page::Market,
                Page::GettingStarted,
                Page::Settings,
            ];
            groups.push((
                "前往",
                pages
                    .into_iter()
                    .filter(|p| !self.module_disabled(*p))
                    .map(|p| (p.info().label, UiAction::Jump(p)))
                    .collect(),
            ));
        }
        for (section, items) in groups {
            let mut row_items = row().flex_wrap().gap(px(5.));
            let mut used = false;
            for (i, (label, action)) in items
                .into_iter()
                .enumerate()
                .filter(|(_, (l, _))| matches(l))
            {
                row_items = row_items.child(
                    outlined(self.button(format!("nav-command-{section}-{i}"), label, action, cx))
                        .child(crate::locale::label(label)),
                );
                used = true;
            }
            if used {
                if !quick {
                    panel = panel.child(headline(section, 12.));
                }
                panel = panel.child(row_items);
            }
        }
        panel
    }
    pub(super) fn observe_space_access(&mut self, access: Result<String, String>, rows: &Rows) {
        self.offline = access
            .as_ref()
            .is_err_and(|error| error.starts_with("offline:"));
        match access {
            Ok(role) => {
                if !self.role.is_empty() && self.role != role {
                    self.form = None;
                    self.tokens.clear();
                    self.versions.clear();
                }
                self.role = role;
                if let Some((space, kind, id, revision)) = self.resume_detail.take()
                    && self.loc.space == space
                    && self.revision == revision
                    && self.loc.selected.is_none()
                    && self.form.is_none()
                {
                    self.send(
                        json!({"op":"get","kind":kind,"id":id,"space":space,"allow_cached":false}),
                        After::SearchTarget { space, revision },
                        false,
                    );
                }
            }
            Err(_) if self.offline => {}
            Err(_) => {
                self.resume_detail = None;
                self.role.clear();
                self.rows.clear();
                self.pending.clear();
                self.busy = false;
                self.form = None;
                self.metrics = Value::Null;
                self.view_tasks.clear();
                self.drafts.clear();
                self.library = Default::default();
                self.reset_live_detail();
                self.work = Default::default();
                self.guide = Default::default();
                self.inbox = Default::default();
                self.tokens.clear();
                self.chat_undo = None;
                self.notice = None;
                self.loc = Location {
                    space: self.loc.space.clone(),
                    page: Page::Spaces,
                    ..Default::default()
                };
                self.history = vec![self.loc.clone()];
                self.pos = 0;
                if !rows.get("space").is_some_and(|spaces| {
                    spaces
                        .iter()
                        .any(|s| s.fields.get("joined") == Some(&Value::Bool(true)))
                }) {
                    self.notify_text("当前账号没有可访问的空间");
                }
            }
        }
    }

    fn reset_live_detail(&mut self) {
        self.loc.selected = None;
        self.form = None;
        self.versions.clear();
        self.drawer_tab.clear();
        self.communication = Default::default();
        self.review = Default::default();
    }

    pub(super) fn reconcile_selected(&mut self) {
        let Some(old) = &self.loc.selected else {
            return;
        };
        if let Some(record) = self.find(&old.kind, &old.id) {
            self.loc.selected = Some(record.clone());
            return;
        }
        if self.rows.contains_key(&old.kind) || self.errors.contains_key(&old.kind) {
            let (space, kind, id) = (self.loc.space.clone(), old.kind.clone(), old.id.clone());
            if self.offline
                && self
                    .errors
                    .get(&kind)
                    .is_some_and(|e| e.starts_with("offline:"))
            {
                self.resume_detail =
                    Some((space.clone(), kind.clone(), id.clone(), self.revision + 1));
            }
            for route in &mut self.history {
                if route.space == space
                    && route
                        .selected
                        .as_ref()
                        .is_some_and(|r| r.kind == kind && r.id == id)
                {
                    route.selected = None;
                }
            }
            self.reset_live_detail();
            self.revision += 1;
        }
    }
}
