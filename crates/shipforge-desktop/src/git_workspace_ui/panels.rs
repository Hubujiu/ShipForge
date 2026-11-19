use super::{
    COAUTHOR, Dialog, EMAIL, INCLUDE, NAME, Panel, Scope, Target, UiAction, Workbench, entries,
    string,
};
use crate::{
    appearance::paint,
    controls::{INK, LINE, MUTED, PALE, VIOLET, chip, col, headline, icon, outlined, row, text},
    forms::Mode,
    locale::label,
};
use gpui_kit::{base::Button, prelude::*, *};
use serde_json::Value;

fn option<'a>(options: &'a [(&str, &str)], selected: &str) -> &'a str {
    options
        .iter()
        .find(|(value, _)| *value == selected)
        .map_or("继承上一级", |(_, name)| name)
}

fn source(value: &Value) -> &'static str {
    match value.as_str() {
        Some("organization") => "组织",
        Some("space") => "空间",
        Some("agent") => "此 Agent",
        Some("parent_agent") => "Agent 本体",
        Some("default") => "系统默认",
        _ => "继承上一级",
    }
}

fn section(title: &'static str) -> Div {
    col()
        .w_full()
        .min_w_0()
        .gap(px(8.))
        .border_1()
        .border_color(paint(LINE))
        .rounded(px(8.))
        .px(px(12.))
        .py(px(10.))
        .child(headline(title, 14.))
}

fn effective_author(data: &Value) -> Div {
    let mut identity = section("生效的提交身份");
    for (key, title, options) in [
        ("name", "提交者姓名", NAME),
        ("email", "提交者邮箱", EMAIL),
        ("coauthor", "共同作者标记", COAUTHOR),
    ] {
        let field = &data["author"][key];
        let value = if key == "coauthor" {
            label(option(
                options,
                if field == true { "true" } else { "false" },
            ))
            .to_owned()
        } else if field["mode"] == "manual" {
            string(field, "value").to_owned()
        } else {
            label(option(options, string(field, "mode"))).to_owned()
        };
        identity = identity.child(
            row()
                .w_full()
                .gap(px(8.))
                .flex_wrap()
                .child(text(title, 12., MUTED).w(px(88.)))
                .child(text(value, 13., INK).min_w_0().flex_1())
                .child(chip(source(&data["author_sources"][key]), PALE, VIOLET)),
        );
    }
    identity
}

fn repository_row(repo: &Value, space_branch: Option<&str>) -> Div {
    let url = string(repo, "url");
    let branch = match (repo["branch"].as_str(), space_branch) {
        (Some(own), _) => own.to_owned(),
        (None, Some(inherited)) => format!("{} · {inherited}", label("继承空间分支")),
        _ => label("远程默认分支").to_owned(),
    };
    let mut line = col()
        .w_full()
        .min_w_0()
        .gap(px(2.))
        .border_t_1()
        .border_color(paint(LINE))
        .pt(px(7.))
        .child(text(url.to_owned(), 13., INK))
        .child(
            row()
                .gap(px(5.))
                .child(icon("git-branch", 12.))
                .child(text(branch, 12., MUTED)),
        );
    if !string(repo, "description").is_empty() {
        line = line.child(text(string(repo, "description").to_owned(), 12., MUTED));
    }
    line
}

impl Workbench {
    fn git_button(
        &self,
        id: String,
        name: &'static str,
        action: UiAction,
        cx: &Context<Self>,
    ) -> Button {
        outlined(self.button(id, name, action, cx)).child(label(name))
    }
    pub(crate) fn git_entry(&self, target: Target, cx: &Context<Self>) -> Button {
        let id = match &target.scope {
            Scope::Organization => format!("git-entry-organization-{}", target.space),
            Scope::Space => format!("git-entry-space-{}", target.space),
            Scope::Agent(id) => format!("git-entry-agent-{id}"),
        };
        let title = target.title();
        outlined(self.button(id, title, UiAction::GitWorkspace(target), cx))
            .child(icon("git-branch", 13.))
            .child(label(title))
    }
    fn git_panel_button(
        &self,
        id: &str,
        title: &'static str,
        f: &Dialog,
        panel: Panel,
        cx: &Context<Self>,
    ) -> Button {
        self.git_button(
            format!("git-panel-{id}"),
            title,
            UiAction::Form(Mode::GitWorkspace(f.showing(panel))),
            cx,
        )
    }
    fn git_org_spaces(&self, f: &Dialog, cx: &Context<Self>) -> Div {
        let mut list = section("已加入空间的仓库");
        let spaces = entries(&f.data["spaces"]);
        if spaces.is_empty() {
            return list.child(text("尚未加入空间。", 13., MUTED));
        }
        for space in spaces {
            let title = string(space, "title");
            let id = string(space, "id");
            let repos = entries(&space["repositories"]);
            let mut tile = col()
                .w_full()
                .min_w_0()
                .gap(px(3.))
                .pt(px(6.))
                .border_t_1()
                .border_color(paint(LINE))
                .child(
                    row()
                        .w_full()
                        .gap(px(6.))
                        .flex_wrap()
                        .child(headline(title.to_owned(), 13.).flex_1())
                        .when(
                            space["can_configure"] != true
                                || self.access_identity["scope"] == "read",
                            |row| row.child(chip("只读", PALE, MUTED)),
                        )
                        .child(
                            outlined(self.button(
                                format!("git-joined-{id}"),
                                format!("{} · {title}", label("查看空间仓库")),
                                UiAction::GitWorkspace(f.target.joined_space(id)),
                                cx,
                            ))
                            .child(label("查看空间仓库")),
                        ),
                );
            if repos.is_empty() {
                tile = tile.child(text("此空间尚未绑定仓库。", 12., MUTED));
            }
            for repo in repos {
                tile = tile.child(text(string(repo, "url").to_owned(), 12., MUTED));
            }
            list = list.child(tile);
        }
        list
    }
    fn git_bound_repos(&self, f: &Dialog, cx: &Context<Self>) -> Div {
        let is_agent = matches!(f.target.scope, Scope::Agent(_));
        let mut panel = section(if is_agent {
            "Agent 仓库清单"
        } else {
            "绑定的空间仓库"
        });
        if is_agent {
            panel = panel.child(
                row()
                    .gap(px(6.))
                    .child(text("清单来源", 12., MUTED))
                    .child(chip(source(&f.data["repositories_source"]), PALE, VIOLET)),
            );
        }
        if f.own_repositories().is_empty() {
            panel = panel.child(text("尚未绑定仓库。", 13., MUTED));
        }
        let editable = self.git_can_edit(f);
        for repo in f.own_repositories() {
            let space_branch = if is_agent {
                entries(&f.data["space_repositories"])
                    .iter()
                    .find(|space_repo| space_repo["url"] == repo["url"])
                    .and_then(|space_repo| space_repo["branch"].as_str())
            } else {
                None
            };
            let mut tile = repository_row(repo, space_branch);
            if editable {
                let url = string(repo, "url");
                let mut actions = row().gap(px(5.)).flex_wrap();
                for (id, title, panel) in [
                    ("edit", "编辑仓库", Panel::Repository(Some(url.into()))),
                    ("remove", "移除仓库绑定", Panel::Remove(url.into())),
                ] {
                    actions = actions.child(
                        outlined(self.button(
                            format!("git-{id}-{url}"),
                            format!("{} · {url}", label(title)),
                            UiAction::Form(Mode::GitWorkspace(f.showing(panel))),
                            cx,
                        ))
                        .child(label(title)),
                    );
                }
                tile = tile.child(actions);
            }
            panel = panel.child(tile);
        }
        if editable {
            let mut actions = row().gap(px(5.)).flex_wrap().child(self.git_panel_button(
                "add",
                "添加 Git 仓库",
                f,
                Panel::Repository(None),
                cx,
            ));
            if is_agent && f.data["settings"]["repositories"].is_array() {
                actions = actions.child(self.git_panel_button(
                    "reinherit",
                    "恢复仓库清单继承",
                    f,
                    Panel::ResetRepositories,
                    cx,
                ));
            }
            panel = panel.child(actions);
        }
        panel
    }
    fn git_agent_checkout(&self, f: &Dialog, cx: &Context<Self>) -> Div {
        let policy = &f.data["effective_include_space"];
        let mut panel = section("执行时检出的仓库").child(
            row()
                .w_full()
                .flex_wrap()
                .gap(px(6.))
                .child(text(option(INCLUDE, string(policy, "mode")), 13., INK))
                .child(chip(source(&f.data["include_space_source"]), PALE, VIOLET)),
        );
        for url in entries(&policy["urls"]).iter().filter_map(Value::as_str) {
            panel = panel.child(text(url.to_owned(), 12., MUTED));
        }
        if self.git_can_edit(f) {
            panel = panel.child(row().child(self.git_panel_button(
                "inclusion",
                "选择继承的空间仓库",
                f,
                Panel::Inclusion,
                cx,
            )));
        }
        let resolved = entries(&f.data["repositories"]);
        if resolved.is_empty() {
            return panel.child(text("当前任务没有待检出的 Git 仓库。", 13., MUTED));
        }
        for repo in resolved {
            panel = panel.child(repository_row(repo, None));
        }
        panel
    }
    pub(crate) fn git_content(&self, f: &Dialog, cx: &Context<Self>) -> AnyElement {
        let mut panel = col().w_full().min_w_0().gap(px(10.));
        let mut context = row()
            .w_full()
            .flex_wrap()
            .gap(px(7.))
            .child(icon("git-branch", 15.))
            .child(headline(string(&f.data, "title").to_owned(), 14.).flex_1());
        if !self.git_can_edit(f) {
            context = context.child(chip("只读", PALE, MUTED));
        }
        context = context.child(self.git_button(
            "git-reread".into(),
            "重新读取 Git 设置",
            UiAction::GitWorkspace(f.target.clone()),
            cx,
        ));
        panel = panel.child(context);
        match &f.panel {
            Panel::Overview => {
                let mut identity = effective_author(&f.data);
                if self.git_can_edit(f) {
                    identity = identity.child(row().child(self.git_panel_button(
                        "author",
                        "编辑提交身份",
                        f,
                        Panel::Author,
                        cx,
                    )));
                }
                panel = panel.child(identity);
                match &f.target.scope {
                    Scope::Organization => panel = panel.child(self.git_org_spaces(f, cx)),
                    Scope::Space => panel = panel.child(self.git_bound_repos(f, cx)),
                    Scope::Agent(_) => {
                        panel = panel
                            .child(self.git_bound_repos(f, cx))
                            .child(self.git_agent_checkout(f, cx))
                    }
                }
                let mut tail = row().gap(px(5.)).flex_wrap();
                if let Some(origin) = &f.target.from_organization {
                    tail = tail.child(self.git_button(
                        "git-back-org".into(),
                        "返回组织 Git 设置",
                        UiAction::GitWorkspace(Target::organization(origin)),
                        cx,
                    ));
                }
                if self.git_can_edit(f) && f.data["settings"].is_object() {
                    tail = tail.child(self.git_panel_button(
                        "reset",
                        "重置本层 Git 设置",
                        f,
                        Panel::Reset,
                        cx,
                    ));
                }
                panel = panel.child(tail);
            }
            Panel::Author => panel = panel.child(effective_author(&f.data)),
            Panel::Repository(_) => {
                panel = panel.child(text(
                    if matches!(f.target.scope, Scope::Agent(_)) {
                        "分支留空时优先继承空间中同地址仓库的分支，再使用远程默认分支。"
                    } else {
                        "分支留空时使用远程默认分支。"
                    },
                    13.,
                    MUTED,
                ));
                if f.data["repositories_source"] == "parent_agent" {
                    panel = panel.child(text(
                        "保存后，本体的现有清单将成为此 Agent 的独立清单。",
                        13.,
                        MUTED,
                    ));
                }
            }
            Panel::Inclusion => {
                panel = panel.child(text(
                    "自动选择会在 Agent 仓库清单为空时包含全部空间仓库。",
                    13.,
                    MUTED,
                ))
            }
            Panel::Remove(url) => {
                panel = panel
                    .child(text("确认从这份清单移除以下仓库绑定。", 13., INK))
                    .child(text(url.to_owned(), 13., MUTED))
            }
            Panel::ResetRepositories => {
                panel = panel.child(text(
                    "此 Agent 将重新使用继承的仓库清单。提交身份和空间仓库选择将被保留。",
                    13.,
                    INK,
                ))
            }
            Panel::Reset => {
                panel = panel.child(text(
                    "确认清除本层的提交身份与仓库配置，并重新计算生效设置。",
                    13.,
                    INK,
                ))
            }
        }
        panel.into_any_element()
    }
}
