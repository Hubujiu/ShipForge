use super::*;
use crate::{
    appearance::{paint as rgb, surface as white},
    controls::*,
    forms::{Format, Spec, item},
};
const CATEGORIES: &[(&str, &str)] = &[
    ("", "全部分类"),
    ("engineering", "工程研发"),
    ("office", "办公效率"),
    ("knowledge", "知识管理"),
    ("data", "数据分析"),
    ("content", "内容创作"),
    ("other", "其他"),
];
#[derive(Default)]
pub struct State {
    pub sources: Vec<(String, String)>,
    pub(super) query: String,
    pub(super) category: String,
}
#[derive(Clone)]
pub enum LibraryForm {
    Add,
    Manual,
    Search,
    File,
    Directory,
    Url,
    Bind,
    Machine,
    Pick(Value),
    Publish {
        kind: String,
        previous: Option<Record>,
    },
    Install(Record),
    Instantiate(Record),
}
pub enum Reply {
    History,
    Detail,
    Results,
    Sources(Value),
    Queued,
}
impl LibraryForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Add => "添加技能",
            Self::Manual => "手动创建技能",
            Self::Search => "搜索市场",
            Self::File => "导入技能文件",
            Self::Directory => "导入技能文件夹",
            Self::Url => "从 URL 导入技能",
            Self::Bind => "绑定空间技能",
            Self::Machine => "从机器发现技能",
            Self::Pick(_) => "选择机器技能",
            Self::Publish { .. } => "发布市场版本",
            Self::Install(_) => "安装技能",
            Self::Instantiate(_) => "从模板创建 Agent",
        }
    }
    pub fn submit_label(&self) -> &'static str {
        match self {
            Self::File | Self::Directory | Self::Url | Self::Pick(_) => "导入",
            Self::Publish { .. } => "发布",
            Self::Instantiate(_) => "创建",
            _ => "确认",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        use Format::*;
        let agent = item("agent", "绑定 Agent（可选）", Ref("agent"), "");
        match self {
            Self::Add => vec![item(
                "source",
                "技能来源",
                Choice(&[
                    ("space", "空间技能"),
                    ("manual", "手动创建技能"),
                    ("file", "导入技能文件"),
                    ("folder", "导入技能文件夹"),
                    ("url", "从 URL 导入技能"),
                    ("machine", "从机器发现技能"),
                ]),
                "space",
            )],
            Self::Manual => vec![
                item("name", "技能名称", Text, ""),
                item("body", "技能指令", Body, ""),
                agent,
            ],
            Self::Search => vec![
                item("query", "名称或描述", Text, ""),
                item("category", "分类", Choice(CATEGORIES), ""),
            ],
            Self::File => vec![
                item("path", "选择 SKILL.md、.skill 或 ZIP", Text, ""),
                agent,
            ],
            Self::Directory => vec![item("path", "选择包含技能的文件夹", Text, ""), agent],
            Self::Url => vec![
                item("url", "GitHub 文件或 Gist 原始 HTTPS 地址", Text, ""),
                agent,
            ],
            Self::Bind => vec![
                item("id", "空间技能", Ref("skill"), ""),
                item("agent", "Agent", Ref("agent"), ""),
                item("enabled", "绑定此技能", Bool, "true"),
            ],
            Self::Install(_) => vec![agent],
            Self::Instantiate(_) => vec![item("instance_title", "新 Agent 名称（可选）", Text, "")],
            Self::Machine => vec![
                item("machine_id", "选择机器", Ref("machine"), ""),
                item("runtime", "Runtime", Ref("$library_runtimes"), ""),
                item(
                    "action",
                    "操作",
                    Choice(&[
                        ("scan", "请求机器扫描"),
                        ("list", "查看扫描结果"),
                        ("history", "查看导入进度"),
                    ]),
                    "list",
                ),
                item("query", "按名称或描述筛选", Text, ""),
            ],
            Self::Pick(_) => vec![
                item("source_id", "发现的技能", Ref("$machine_skills"), ""),
                agent,
            ],
            Self::Publish { kind, previous } => {
                let mut v = vec![
                    item("title", "市场条目名称", Text, ""),
                    item("description", "条目说明", Body, ""),
                    item("category", "分类", Choice(&CATEGORIES[1..]), "engineering"),
                ];
                if previous.is_none() {
                    v.push(item(
                        "audience",
                        "发布范围",
                        Choice(&[("space", "当前空间"), ("organization", "整个组织")]),
                        "space",
                    ));
                }
                v.push(if kind == "agent" {
                    item("source", "模板来源 Agent", Ref("agent"), "")
                } else {
                    item("sources", "收录技能", Many("skill"), "")
                });
                v
            }
        }
    }
}
impl Workbench {
    fn library_button(
        &self,
        id: impl Into<ElementId>,
        label: &'static str,
        action: UiAction,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        outlined(self.button(id, label, action, cx)).child(crate::locale::label(label))
    }
    pub(crate) fn skill_toolbar(&self, cx: &Context<Self>) -> Div {
        let mut bar =
            row()
                .flex_wrap()
                .gap(px(6.))
                .px(px(14.))
                .py(px(8.))
                .child(self.library_button(
                    "skill-connectors",
                    "空间连接器",
                    UiAction::Go(Page::Connectors),
                    cx,
                ));
        if self.writable() {
            for (id, form) in [
                ("file", LibraryForm::File),
                ("directory", LibraryForm::Directory),
                ("url", LibraryForm::Url),
                ("machine", LibraryForm::Machine),
                ("bind", LibraryForm::Bind),
            ] {
                bar = bar.child(self.library_button(
                    format!("skill-{id}"),
                    form.title(),
                    UiAction::Form(Mode::Library(form)),
                    cx,
                ));
            }
        }
        bar
    }
    pub(crate) fn market_page(&self, cx: &Context<Self>) -> AnyElement {
        let agents = self.loc.section == "agents";
        let kind = if agents { "agent" } else { "skill" };
        let mut header = row()
            .flex_wrap()
            .gap(px(7.))
            .child(self.pill(
                "market-skills".into(),
                "技能",
                UiAction::Section(String::new()),
                !agents,
                cx,
            ))
            .child(self.pill(
                "market-agents".into(),
                "Agent 模板",
                UiAction::Section("agents".into()),
                agents,
                cx,
            ))
            .child(self.library_button(
                "market-search",
                "搜索市场",
                UiAction::Form(Mode::Library(LibraryForm::Search)),
                cx,
            ));
        if self.writable() {
            header = header.child(self.library_button(
                "market-publish",
                "发布市场版本",
                UiAction::Form(Mode::Library(LibraryForm::Publish {
                    kind: kind.into(),
                    previous: None,
                })),
                cx,
            ));
        }
        let category = |id: &str| {
            CATEGORIES
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, v)| crate::locale::label(v))
                .unwrap_or("")
        };
        let mut body = col()
            .id("market-list")
            .w_full()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .p(px(15.))
            .gap(px(10.))
            .child(header);
        if !self.library.category.is_empty() || !self.library.query.is_empty() {
            body = body.child(text(
                format!(
                    "{}  {}",
                    category(&self.library.category),
                    self.library.query
                ),
                12.,
                MUTED,
            ));
        }
        let mut count = 0;
        for t in self.list("catalog").iter().filter(|t| {
            t.kind == kind
                && (self.library.category.is_empty()
                    || t.field("category") == self.library.category)
                && (self.library.query.is_empty()
                    || format!("{} {} {}", t.title, t.description, t.id)
                        .to_lowercase()
                        .contains(&self.library.query.to_lowercase()))
        }) {
            count += 1;
            let version = t.fields.get("version").and_then(Value::as_i64).unwrap_or(1);
            let installed = t.fields.get("installed").is_some_and(|v| v == true);
            let old = t.fields.get("installed_version").and_then(Value::as_i64);
            let mut actions = row().flex_wrap().gap(px(6.)).child(self.library_button(
                format!("market-detail-{}", t.id),
                "查看详情",
                UiAction::LibraryDetails(t.id.clone()),
                cx,
            ));
            if self.writable() {
                if agents {
                    actions = actions.child(self.library_button(
                        format!("market-create-{}", t.id),
                        "从模板创建 Agent",
                        UiAction::Form(Mode::Library(LibraryForm::Instantiate(t.clone()))),
                        cx,
                    ));
                } else if !installed {
                    actions = actions.child(self.library_button(
                        format!("market-install-{}", t.id),
                        "安装技能",
                        UiAction::Form(Mode::Library(LibraryForm::Install(t.clone()))),
                        cx,
                    ));
                }
                if !agents && let Some(old) = old {
                    if old < version {
                        actions = actions.child(self.library_button(
                            format!("market-update-{}", t.id),
                            "更新已安装技能",
                            UiAction::Apply(
                                json!({"op":"catalog.update","id":t.id}),
                                "技能已更新".into(),
                            ),
                            cx,
                        ));
                    }
                    actions = actions.child(self.library_button(
                        format!("market-remove-{}", t.id),
                        "卸载技能",
                        UiAction::Form(Mode::Confirm {
                            label: "卸载技能".into(),
                            command: json!({"op":"catalog.uninstall","id":t.id}),
                        }),
                        cx,
                    ));
                }
                if t.fields.get("can_publish").is_some_and(|v| v == true) {
                    actions = actions.child(self.library_button(
                        format!("market-release-{}", t.id),
                        "发布新版本",
                        UiAction::Form(Mode::Library(LibraryForm::Publish {
                            kind: kind.into(),
                            previous: Some(t.clone()),
                        })),
                        cx,
                    ));
                }
            }
            body = body.child(
                col()
                    .w_full()
                    .gap(px(5.))
                    .p(px(12.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(white())
                    .rounded(px(8.))
                    .child(
                        row()
                            .gap(px(9.))
                            .child(headline(t.title.clone(), 14.))
                            .child(text(
                                format!("{} · v{version}", category(t.field("category"))),
                                12.,
                                MUTED,
                            ))
                            .when(installed, |r| r.child(text("已安装", 12., MUTED))),
                    )
                    .child(text(t.description.clone(), 13., MUTED))
                    .child(actions),
            );
        }
        if count == 0 {
            body = body.child(text("没有匹配的市场条目", 13., MUTED));
        }
        body.into_any_element()
    }
    pub(super) fn library_submit(
        &mut self,
        form: LibraryForm,
        values: Map<String, Value>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let v = Value::Object(values);
        let mut q = json!({"space":self.loc.space});
        let mut reply = Reply::Results;
        match form {
            LibraryForm::Add => {
                let next = match v["source"].as_str() {
                    Some("manual") => LibraryForm::Manual,
                    Some("file") => LibraryForm::File,
                    Some("folder") => LibraryForm::Directory,
                    Some("url") => LibraryForm::Url,
                    Some("machine") => LibraryForm::Machine,
                    _ => LibraryForm::Bind,
                };
                self.open_form(Mode::Library(next), w, cx);
                return Ok(());
            }
            LibraryForm::Manual => {
                let title = v["name"].as_str().unwrap_or("").trim();
                let body = v["body"].as_str().unwrap_or("").trim();
                if title.is_empty() || body.is_empty() {
                    return Err(crate::locale::label("请输入名称和技能指令").into());
                }
                q["op"] = json!("skill.import");
                q["files"] = json!([{"path":"SKILL.md","text":format!("# {title}\n\n{body}")}]);
                q["agent"] = v["agent"].clone();
            }
            LibraryForm::Search => {
                self.library.query = v["query"].as_str().unwrap_or("").trim().into();
                self.library.category = v["category"].as_str().unwrap_or("").into();
                self.close_form(w, cx);
                return Ok(());
            }
            LibraryForm::File | LibraryForm::Directory | LibraryForm::Url => {
                q["op"] = json!("skill.import");
                let key = if matches!(form, LibraryForm::Url) {
                    "url"
                } else {
                    "path"
                };
                q[key] = v[key].clone();
                q["agent"] = v["agent"].clone();
            }
            LibraryForm::Bind => {
                q["op"] = json!("skill.bind");
                for k in ["id", "agent", "enabled"] {
                    q[k] = v[k].clone();
                }
            }
            LibraryForm::Install(r) => {
                q["op"] = json!("catalog.install");
                q["id"] = json!(r.id);
                q["agent"] = v["agent"].clone();
            }
            LibraryForm::Instantiate(r) => {
                q["op"] = json!("catalog.instantiate");
                q["id"] = json!(r.id);
                q["target_id"] = json!(uuid::Uuid::new_v4().to_string());
                if v["instance_title"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty())
                {
                    q["instance_title"] = v["instance_title"].clone();
                }
            }
            LibraryForm::Publish { kind, previous } => {
                q["op"] = json!("catalog.publish");
                q["kind"] = json!(kind);
                q["title"] = v["title"].clone();
                q["description"] = v["description"].clone();
                q["category"] = v["category"].clone();
                q["sources"] = if kind == "agent" {
                    json!([v["source"]])
                } else {
                    v["sources"].clone()
                };
                if let Some(r) = previous {
                    q["id"] = json!(r.id);
                    q["expected_version"] = r.fields.get("version").cloned().unwrap_or(json!(1));
                    if q["title"].as_str().is_none_or(|s| s.trim().is_empty()) {
                        q["title"] = json!(r.title);
                    }
                } else {
                    q["id"] = json!(uuid::Uuid::new_v4().to_string());
                    q["audience"] = v["audience"].clone();
                }
            }
            LibraryForm::Machine => {
                q["machine_id"] = v["machine_id"].clone();
                q["runtime"] = v["runtime"].clone();
                q["query"] = v["query"].clone();
                if v["action"] == "scan" {
                    q["op"] = json!("skill.discover");
                    reply = Reply::Queued;
                } else {
                    q["op"] = json!("skill.sources");
                    reply = if v["action"] == "history" {
                        Reply::History
                    } else {
                        Reply::Sources(q.clone())
                    };
                }
            }
            LibraryForm::Pick(mut args) => {
                args["op"] = json!("skill.machine_import");
                args["source_id"] = v["source_id"].clone();
                args["agent"] = v["agent"].clone();
                q = args;
                reply = Reply::Queued;
            }
        }
        self.send(q, After::Library(reply), true);
        Ok(())
    }
    pub(super) fn library_reply(
        &mut self,
        reply: Reply,
        v: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (heading, content) = match reply {
            Reply::History => {
                let lines = v["jobs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|j| {
                        let label = crate::locale::label(match j["state"].as_str() {
                            Some("queued") => "排队中",
                            Some("failed") => "失败",
                            _ => "已完成",
                        });
                        let what = crate::locale::label(if j["operation"] == "skill.discover" {
                            "请求机器扫描"
                        } else {
                            "导入"
                        });
                        let names = j["result"]["results"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|s| s["title"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!(
                            "{what} · {label}  {}{names}",
                            j["result"]["error"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>();
                (
                    "查看导入进度",
                    if lines.is_empty() {
                        crate::locale::label("暂无记录").to_owned()
                    } else {
                        lines.join("\n")
                    },
                )
            }
            Reply::Queued => (
                "机器技能队列",
                crate::locale::label(if v["machine_online"] == false {
                    "机器离线，已排队，机器上线后处理。"
                } else {
                    "已排队。可在查看扫描结果中检查进度。"
                })
                .to_owned(),
            ),
            Reply::Results => {
                let text = if let Some(results) = v["results"].as_array() {
                    results
                        .iter()
                        .map(|x| {
                            format!(
                                "{} · {}",
                                x["title"].as_str().unwrap_or(""),
                                crate::locale::label(match x["status"].as_str() {
                                    Some("imported") => "已导入",
                                    Some("linked") => "已关联",
                                    Some("skipped") => "已跳过",
                                    _ => "已存在",
                                })
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    v["title"]
                        .as_str()
                        .unwrap_or(crate::locale::label("操作已完成"))
                        .to_owned()
                };
                self.read(true);
                ("处理结果", text)
            }
            Reply::Detail => {
                let mut content = format!(
                    "{}\n{}",
                    v["title"].as_str().unwrap_or(""),
                    v["description"].as_str().unwrap_or("")
                );
                for item in v["fields"]["items"].as_array().into_iter().flatten() {
                    content.push_str(&format!(
                        "\n\n{}\n{}",
                        item["title"].as_str().unwrap_or(""),
                        item["fields"]["instructions"].as_str().unwrap_or("")
                    ));
                }
                ("市场条目详情", content)
            }
            Reply::Sources(args) => {
                self.library.sources = v["sources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        Some((
                            s["id"].as_str()?.into(),
                            format!(
                                "{} · {}",
                                s["title"].as_str()?,
                                s["description"].as_str().unwrap_or("")
                            ),
                        ))
                    })
                    .collect();
                if !self.library.sources.is_empty() {
                    self.open_form(Mode::Library(LibraryForm::Pick(args)), w, cx);
                    return;
                }
                let mut text = crate::locale::label("没有匹配的机器技能").to_owned();
                for job in v["jobs"].as_array().into_iter().flatten().take(4) {
                    text.push_str(&format!(
                        "\n{}  {}",
                        crate::locale::label(match job["state"].as_str() {
                            Some("queued") => "排队中",
                            Some("failed") => "失败",
                            _ => "已完成",
                        }),
                        job["result"]["error"].as_str().unwrap_or("")
                    ));
                }
                ("机器技能", text)
            }
        };
        self.open_form(
            Mode::Output {
                heading: crate::locale::label(heading).into(),
                content,
            },
            w,
            cx,
        );
    }
}
