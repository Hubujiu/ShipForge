use super::{After, UiAction, Workbench};
use crate::forms::{Format, Mode, Spec, item};
use gpui_kit::{Context, Window};
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[cfg(all(windows, test))]
#[path = "git_workspace_ui/native_test.rs"]
mod native_test;
#[path = "git_workspace_ui/panels.rs"]
mod panels;

#[derive(Clone)]
enum Scope {
    Organization,
    Space,
    Agent(String),
}

#[derive(Clone)]
pub struct Target {
    space: String,
    scope: Scope,
    from_organization: Option<String>,
}

impl Target {
    pub fn organization(space: &str) -> Self {
        Self {
            space: space.into(),
            scope: Scope::Organization,
            from_organization: None,
        }
    }
    pub fn space(space: &str) -> Self {
        Self {
            space: space.into(),
            scope: Scope::Space,
            from_organization: None,
        }
    }
    pub fn agent(space: &str, id: &str) -> Self {
        Self {
            space: space.into(),
            scope: Scope::Agent(id.into()),
            from_organization: None,
        }
    }
    fn joined_space(&self, id: &str) -> Self {
        Self {
            from_organization: Some(self.space.clone()),
            ..Self::space(id)
        }
    }
    fn request(&self, op: &str) -> Value {
        let level = match self.scope {
            Scope::Organization => "organization",
            Scope::Space => "space",
            Scope::Agent(_) => "agent",
        };
        let mut command = json!({"op":op,"level":level,"space":self.space});
        if let Scope::Agent(id) = &self.scope {
            command["id"] = json!(id);
        }
        command
    }
    fn title(&self) -> &'static str {
        match self.scope {
            Scope::Organization => "组织 Git 设置",
            Scope::Space => "空间 Git 设置",
            Scope::Agent(_) => "Agent Git 设置",
        }
    }
}

pub(super) enum Reply {
    Opened(Target),
    Saved(Target),
}

#[derive(Clone)]
pub(super) enum Panel {
    Overview,
    Author,
    Repository(Option<String>),
    Remove(String),
    Inclusion,
    ResetRepositories,
    Reset,
}

#[derive(Clone)]
pub struct Dialog {
    pub(super) target: Target,
    pub(super) data: Arc<Value>,
    pub(super) panel: Panel,
}

const NAME: &[(&str, &str)] = &[
    ("inherit", "继承上一级"),
    ("trigger", "触发者"),
    ("trigger_and_agent", "触发者与 Agent"),
    ("owner", "Agent 所有者"),
    ("owner_and_agent", "所有者与 Agent"),
    ("machine", "机器 Git 配置"),
    ("manual", "自定义值"),
];
const EMAIL: &[(&str, &str)] = &[
    ("inherit", "继承上一级"),
    ("trigger", "触发者"),
    ("owner", "Agent 所有者"),
    ("machine", "机器 Git 配置"),
    ("manual", "自定义值"),
];
const COAUTHOR: &[(&str, &str)] = &[
    ("inherit", "继承上一级"),
    ("true", "添加共同作者"),
    ("false", "省略共同作者"),
];
const INCLUDE: &[(&str, &str)] = &[
    ("inherit", "继承或自动选择"),
    ("all", "包含全部空间仓库"),
    ("selected", "仅包含选定仓库"),
    ("none", "不包含空间仓库"),
];

fn entries(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

pub(crate) fn hidden_fields(
    entries: &[crate::forms::Editor],
    cx: &gpui_kit::App,
) -> Vec<&'static str> {
    [
        ("name_mode", "manual", "manual_name"),
        ("email_mode", "manual", "manual_email"),
        ("include_mode", "selected", "git_urls"),
    ]
    .into_iter()
    .filter_map(|(key, required, field)| {
        entries
            .iter()
            .find(|entry| entry.spec.key == key)
            .is_some_and(|entry| entry.raw(cx) != required)
            .then_some(field)
    })
    .collect()
}

impl Dialog {
    pub(super) fn showing(&self, panel: Panel) -> Self {
        Self {
            panel,
            ..self.clone()
        }
    }
    pub fn title(&self) -> &'static str {
        match &self.panel {
            Panel::Overview => self.target.title(),
            Panel::Author => "编辑提交身份",
            Panel::Repository(None) => "添加 Git 仓库",
            Panel::Repository(Some(_)) => "编辑 Git 仓库",
            Panel::Remove(_) => "移除仓库绑定",
            Panel::Inclusion => "选择继承的空间仓库",
            Panel::ResetRepositories => "恢复仓库清单继承",
            Panel::Reset => "重置本层 Git 设置",
        }
    }
    pub fn submit_label(&self) -> &'static str {
        match self.panel {
            Panel::Overview => "关闭",
            Panel::Remove(_) => "确认移除绑定",
            Panel::ResetRepositories | Panel::Reset => "确认重置",
            _ => "保存",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        match self.panel {
            Panel::Author => vec![
                item("name_mode", "提交者姓名", Format::Choice(NAME), "inherit"),
                item("manual_name", "自定义提交者姓名", Format::Text, ""),
                item("email_mode", "提交者邮箱", Format::Choice(EMAIL), "inherit"),
                item("manual_email", "自定义提交者邮箱", Format::Text, ""),
                item(
                    "coauthor_mode",
                    "共同作者标记",
                    Format::Choice(COAUTHOR),
                    "inherit",
                ),
            ],
            Panel::Repository(_) => vec![
                item("url", "Git 仓库 URL", Format::Text, ""),
                item("description", "仓库说明", Format::Text, ""),
                item("branch", "检出分支（可选）", Format::Text, ""),
            ],
            Panel::Inclusion => vec![
                item(
                    "include_mode",
                    "空间仓库来源",
                    Format::Choice(INCLUDE),
                    "inherit",
                ),
                item(
                    "git_urls",
                    "供此 Agent 使用的空间仓库",
                    Format::Many("$git_space_repositories"),
                    "",
                ),
            ],
            _ => vec![],
        }
    }
    fn own_repositories(&self) -> &[Value] {
        if matches!(self.target.scope, Scope::Agent(_)) {
            entries(&self.data["agent_repositories"])
        } else {
            entries(&self.data["space_repositories"])
        }
    }
    pub fn space_choices(&self) -> Vec<(String, String)> {
        entries(&self.data["space_repositories"])
            .iter()
            .map(|repo| {
                let url = string(repo, "url").to_owned();
                let description = string(repo, "description");
                let name = if description.is_empty() {
                    url.clone()
                } else {
                    format!("{description} · {url}")
                };
                (url, name)
            })
            .collect()
    }
    pub fn prefill(&self) -> Map<String, Value> {
        let settings = &self.data["settings"];
        let mut result = Map::new();
        match &self.panel {
            Panel::Author => {
                for (field, mode, input) in [
                    ("name", "name_mode", "manual_name"),
                    ("email", "email_mode", "manual_email"),
                ] {
                    let selection = &settings["author"][field];
                    result.insert(
                        mode.into(),
                        json!(selection["mode"].as_str().unwrap_or("inherit")),
                    );
                    result.insert(input.into(), json!(string(selection, "value")));
                }
                let coauthor = settings["author"]["coauthor"]
                    .as_bool()
                    .map_or("inherit", |v| if v { "true" } else { "false" });
                result.insert("coauthor_mode".into(), json!(coauthor));
            }
            Panel::Repository(Some(url)) => {
                if let Some(repo) = self
                    .own_repositories()
                    .iter()
                    .find(|repo| repo["url"] == *url)
                {
                    result.extend(repo.as_object().cloned().unwrap_or_default());
                }
            }
            Panel::Inclusion => {
                let stored = &settings["include_space"];
                result.insert(
                    "include_mode".into(),
                    json!(stored["mode"].as_str().unwrap_or("inherit")),
                );
                let possible = self.space_choices();
                let selected = entries(&self.data["effective_include_space"]["urls"])
                    .iter()
                    .filter(|url| possible.iter().any(|(id, _)| url.as_str() == Some(id)));
                result.insert("git_urls".into(), Value::Array(selected.cloned().collect()));
            }
            _ => {}
        }
        result
    }
    fn updated(&self, values: &Value) -> Result<Value, String> {
        let mut result = Value::Object(
            self.data["settings"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
        );
        match &self.panel {
            Panel::Author => {
                for (field, mode, manual) in [
                    ("name", "name_mode", "manual_name"),
                    ("email", "email_mode", "manual_email"),
                ] {
                    result["author"][field] = match string(values, mode) {
                        "inherit" => Value::Null,
                        "manual" if string(values, manual).trim().is_empty() => {
                            return Err(crate::locale::label("请填入自定义的提交者信息。").into());
                        }
                        "manual" => json!({"mode":"manual","value":string(values,manual).trim()}),
                        choice => json!({"mode":choice}),
                    };
                }
                result["author"]["coauthor"] = match string(values, "coauthor_mode") {
                    "true" => json!(true),
                    "false" => json!(false),
                    _ => Value::Null,
                };
            }
            Panel::Repository(previous) => {
                let url = string(values, "url").trim();
                if url.is_empty() {
                    return Err(crate::locale::label("请输入 Git 仓库 URL。").into());
                }
                let branch = string(values, "branch").trim();
                let repo = json!({"url":url,"description":string(values,"description").trim(),"branch":(!branch.is_empty()).then_some(branch)});
                let mut items = self.own_repositories().to_vec();
                if let Some(previous) = previous {
                    let current = items
                        .iter_mut()
                        .find(|r| r["url"] == *previous)
                        .ok_or_else(|| {
                            crate::locale::label("仓库清单已变更，请重新读取。").to_owned()
                        })?;
                    *current = repo;
                } else {
                    items.push(repo);
                }
                result["repositories"] = json!(items);
            }
            Panel::Remove(url) => {
                result["repositories"] = Value::Array(
                    self.own_repositories()
                        .iter()
                        .filter(|r| r["url"] != *url)
                        .cloned()
                        .collect(),
                );
            }
            Panel::Inclusion => {
                result["include_space"] = match string(values, "include_mode") {
                    "selected" => json!({"mode":"selected","urls":values["git_urls"]}),
                    "all" => json!({"mode":"all"}),
                    "none" => json!({"mode":"none"}),
                    _ => Value::Null,
                };
            }
            Panel::ResetRepositories => {
                result.as_object_mut().unwrap().remove("repositories");
            }
            Panel::Reset => return Ok(Value::Null),
            Panel::Overview => {}
        }
        Ok(result)
    }
}

impl Workbench {
    fn git_can_edit(&self, dialog: &Dialog) -> bool {
        dialog.data["can_configure"] == true && self.access_identity["scope"] != "read"
    }
    pub(super) fn git_open(&mut self, target: Target) {
        self.send(
            target.request("git.settings"),
            After::GitWorkspace(Reply::Opened(target)),
            false,
        );
    }
    pub(super) fn git_reply(
        &mut self,
        reply: Reply,
        data: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Reply::Opened(target) => self.open_form(
                Mode::GitWorkspace(Dialog {
                    target,
                    data: Arc::new(data),
                    panel: Panel::Overview,
                }),
                w,
                cx,
            ),
            Reply::Saved(target) => {
                self.notify_text("Git 设置已保存");
                self.git_open(target);
            }
        }
    }
    pub(super) fn git_submit(
        &mut self,
        dialog: Dialog,
        values: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if matches!(dialog.panel, Panel::Overview) {
            self.close_form(w, cx);
            return Ok(());
        }
        if !self.git_can_edit(&dialog) {
            return Err(crate::locale::label("当前身份仅可查看 Git 设置。").into());
        }
        let mut command = dialog.target.request("git.configure");
        command["expected_rev"] = dialog.data["rev"].clone();
        command["settings"] = dialog.updated(&values)?;
        self.send(
            command,
            After::GitWorkspace(Reply::Saved(dialog.target)),
            true,
        );
        Ok(())
    }
}
