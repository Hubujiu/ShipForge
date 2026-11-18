use crate::{
    app::Workbench,
    catalog::{Record, value_text},
    controls::Field,
};
use gpui_kit::*;

use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Body,
    Secret,
    Number,
    Bool,
    Csv,
    Json,
    Choice(&'static [(&'static str, &'static str)]),
    Ref(&'static str),
    Many(&'static str),
}
#[derive(Clone, Copy)]
pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    pub format: Format,
    pub default: &'static str,
}
fn item(key: &'static str, label: &'static str, format: Format, default: &'static str) -> Spec {
    Spec {
        key,
        label,
        format,
        default,
    }
}
const VIS: &[(&str, &str)] = &[("space", "空间共享"), ("private", "仅自己")];
const LAYOUT: &[(&str, &str)] = &[("board", "看板"), ("list", "列表"), ("table", "表格")];
const STATUS: &[(&str, &str)] = &[
    ("backlog", "待规划"),
    ("todo", "待开始"),
    ("in_progress", "进行中"),
    ("done", "已完成"),
    ("cancelled", "已取消"),
    ("triage", "待分拣"),
];
const TRIGGERS: &[(&str, &str)] = &[
    ("status", "任务状态变化"),
    ("comment", "任务新增评论"),
    ("schedule", "定时运行"),
    ("webhook", "Webhook"),
];
#[derive(Clone)]
pub enum Mode {
    Create(String),
    Edit(Record),
    Filter,
    Connect,
    Comment(Record),
    Message(Record),
    IterationEnd(Record),
    Upload(Option<Record>),
    Download(Record, Value),
    Machine { stop: bool },
    Workflow { op: &'static str, record: Record },
    ResolveDraft(Value),
    Confirm { label: String, command: Value },
    Export,
    Import,
    Token,
    Search,
    Output { heading: String, content: String },
}
impl Mode {
    pub fn automation(&self) -> bool {
        matches!(self,Self::Create(k) if k=="automation")
            || matches!(self,Self::Edit(r) if r.kind=="automation")
    }

    pub fn heading(&self) -> String {
        use Mode::*;
        match self {
            Machine { stop } => if *stop {
                "停止本机工作机"
            } else {
                "启动本机工作机"
            }
            .into(),
            Upload(_) => "上传附件".into(),
            Download(..) => "下载附件".into(),
            Workflow { op, .. } => workflow_name(op).into(),
            Create(k) => format!("新建{}", noun(k)),
            Edit(r) => format!("编辑{}", noun(&r.kind)),
            Filter => "筛选任务".into(),
            Connect => "连接团队服务".into(),
            Comment(_) => "添加评论".into(),
            Message(_) => "发送消息".into(),
            IterationEnd(_) => "结束迭代".into(),
            ResolveDraft(_) => "处理同步冲突".into(),
            Confirm { label, .. } => label.clone(),
            Export => "导出空间".into(),
            Import => "导入空间".into(),
            Token => "创建访问令牌".into(),
            Search => "搜索工作空间".into(),
            Output { heading, .. } => heading.clone(),
        }
    }
    pub fn submit_label(&self) -> &'static str {
        match self {
            Self::Machine { stop } => {
                if *stop {
                    "停止"
                } else {
                    "启动"
                }
            }
            Self::Upload(_) => "上传",
            Self::Download(..) => "保存",
            Self::Workflow { .. } => "确认",
            Self::Create(_) => "创建",
            Self::Filter => "应用筛选",
            Self::Connect => "连接",
            Self::Comment(_) | Self::Message(_) => "发送",
            Self::Search => "搜索",
            Self::Token => "签发令牌",
            Self::ResolveDraft(_) => "更新草稿",
            Self::Export => "导出",
            Self::Import => "预检",
            Self::Confirm { .. } => "确认",
            Self::IterationEnd(_) => "结束迭代",
            Self::Output { .. } => "关闭",
            Self::Edit(_) => "保存修改",
        }
    }
}
pub fn noun(kind: &str) -> &'static str {
    crate::catalog::Page::from_kind(kind)
        .map(|p| p.info().noun)
        .unwrap_or(match kind {
            "relation_type" => "关联类型",
            "tag" => "标签",
            "artifact" => "产物",
            _ => "记录",
        })
}

pub fn record_specs(kind: &str) -> Vec<Spec> {
    use Format::*;
    let title = item("title", "名称", Text, "");
    let body = item("description", "描述", Body, "");
    let visibility = item("visibility", "可见范围", Choice(VIS), "space");
    let filter = || {
        vec![
            item("query", "搜索关键词", Text, ""),
            item("filter_type", "任务类型", Ref("$types"), ""),
            item("filter_status", "状态", Ref("$statuses"), ""),
            item("filter_priority", "优先级", Ref("$priorities"), ""),
            item("filter_assignee", "处理人", Ref("$handlers"), ""),
            item("filter_project", "专项", Ref("project"), ""),
        ]
    };
    match kind {
        "task" => vec![
            item("title", "任务标题", Text, ""),
            body,
            item("type", "类型", Ref("$types"), "task"),
            item("status", "状态", Ref("$statuses"), "todo"),
            item(
                "priority",
                "优先级",
                Choice(crate::catalog::PRIORITIES),
                "none",
            ),
            item("assignee", "处理人", Ref("$handlers"), ""),
            item("owner", "负责人", Ref("member"), ""),
            item("iteration", "迭代", Ref("iteration"), ""),
            item("project", "专项", Ref("project"), ""),
            item("due_date", "截止日期（YYYY-MM-DD）", Text, ""),
            item("labels", "标签（用逗号分隔）", Csv, ""),
            item("parent", "父任务", Ref("task"), ""),
            item("custom", "自定义字段值（JSON 对象）", Json, "{}"),
        ],
        "iteration" => vec![
            title,
            body,
            item("start_date", "开始日期（YYYY-MM-DD）", Text, ""),
            item("end_date", "结束日期（YYYY-MM-DD）", Text, ""),
        ],
        "project" => vec![title, body, item("owner", "负责人", Ref("member"), "")],
        "view" => {
            let mut fields = vec![
                title,
                visibility,
                item("layout", "布局", Choice(LAYOUT), "board"),
                item(
                    "group",
                    "分组",
                    Choice(&[
                        ("status", "状态"),
                        ("priority", "优先级"),
                        ("assignee", "处理人"),
                        ("type", "任务类型"),
                    ]),
                    "status",
                ),
                item(
                    "sort",
                    "排序",
                    Choice(&[
                        ("updated_at", "最近更改"),
                        ("title", "名称"),
                        ("priority", "优先级"),
                        ("created_at", "创建时间"),
                    ]),
                    "updated_at",
                ),
            ];
            fields.extend(filter());
            fields
        }
        "organization" => vec![title, body],
        "space" => vec![
            title,
            body,
            item("organization", "所属组织", Ref("organization"), ""),
            item(
                "visibility",
                "空间可见性",
                Choice(&[("private", "邀请成员"), ("public", "公开")]),
                "private",
            ),
            item("triage_enabled", "空间分拣台", Bool, "false"),
            item(
                "triage_sources",
                "需要分拣的来源",
                Many("$sources"),
                "guest, external",
            ),
            item(
                "triage_types",
                "需要分拣的任务类型",
                Many("$types"),
                "task, requirement, bug",
            ),
            item(
                "triage_priority_required",
                "分拣时要求优先级",
                Bool,
                "false",
            ),
        ],
        "document" => vec![title, item("body", "正文", Body, ""), visibility],
        "chat" => vec![
            title,
            item("agent", "参与 Agent", Ref("agent"), ""),
            item("visibility", "可见范围", Choice(VIS), "private"),
        ],
        "member" => vec![
            item("title", "姓名", Text, ""),
            item("actor", "成员账号标识", Text, ""),
            item(
                "role",
                "空间角色",
                Choice(&[
                    ("member", "成员"),
                    ("admin", "管理员"),
                    ("viewer", "只读"),
                    ("guest", "访客"),
                ]),
                "member",
            ),
        ],
        "agent" => vec![
            title,
            body,
            item("parent_id", "继承其他 Agent", Ref("agent"), ""),
            item("instructions", "职责与指令", Body, ""),
            item("runtime", "Runtime 名称", Text, "local-tool"),
            item("machine_id", "指定机器", Ref("machine"), ""),
            item(
                "directory_mode",
                "工作目录策略",
                Choice(&[("isolated", "独立工作树"), ("direct", "指定目录串行")]),
                "isolated",
            ),
            item("skills", "技能", Many("skill"), ""),
            item("tools", "授权工具", Many("tool"), ""),
            item("max_parallel", "任务并发数", Number, ""),
            item("chat_parallel", "对话并发数", Number, ""),
            item("timeout_seconds", "超时秒数", Number, ""),
            item("working_hours", "工作时段（JSON）", Json, ""),
            visibility,
        ],
        "crew" => vec![
            title,
            body,
            item("leader_id", "队长 Agent", Ref("agent"), ""),
            item("members", "小队成员", Many("agent"), ""),
            visibility,
        ],
        "skill" => vec![
            title,
            item("instructions", "技能指令", Body, ""),
            item("version", "版本", Text, "1"),
            visibility,
        ],
        "tool" => vec![
            title,
            body,
            item(
                "type",
                "工具类型",
                Choice(&[
                    ("document", "文档"),
                    ("connector", "连接器"),
                    ("runtime", "工作机工具"),
                ]),
                "document",
            ),
            item("document", "授权文档", Ref("document"), ""),
            item("enabled", "启用", Bool, "true"),
            visibility,
        ],
        "automation" => vec![
            title,
            item("instruction", "执行指令", Body, ""),
            item("handler_id", "执行者", Ref("$agents"), ""),
            item("notify", "通知成员", Many("member"), ""),
            item("enabled", "规则状态", Bool, "true"),
        ],
        "connector" => vec![
            title,
            item(
                "provider",
                "平台",
                Choice(&[
                    ("github", "GitHub Issues"),
                    ("jira", "Jira"),
                    ("gitlab", "GitLab"),
                    ("linear", "Linear"),
                    ("asana", "Asana"),
                    ("teams", "Microsoft Teams"),
                    ("discord", "Discord"),
                    ("wecom", "企业微信"),
                    ("dingtalk", "钉钉"),
                    ("feishu", "飞书"),
                    ("slack", "Slack"),
                    ("telegram", "Telegram"),
                    ("custom", "自定义来源"),
                ]),
                "github",
            ),
            item(
                "repository",
                "GitHub 或 GitLab 仓库（owner/repository）",
                Text,
                "",
            ),
            item("project_id", "GitLab 项目 ID", Text, ""),
            item("project_key", "Jira 项目标识", Text, ""),
            item("workspace_id", "Slack 工作区 ID", Text, ""),
            item("channel_id", "Slack 频道 ID", Text, ""),
            item("chat_id", "飞书或 Telegram 会话 ID", Text, ""),
            item("enabled", "启用", Bool, "true"),
            body,
            item("mapping", "字段映射（JSON）", Json, "{}"),
        ],
        "field" => vec![
            title,
            item(
                "value_type",
                "字段类型",
                Choice(&[
                    ("text", "文本"),
                    ("number", "数值"),
                    ("boolean", "布尔"),
                    ("date", "日期"),
                    ("select", "选项"),
                ]),
                "text",
            ),
            item("options", "可用选项（用逗号分隔）", Csv, ""),
            item("default", "默认显示", Bool, "false"),
            item("order", "顺序", Number, ""),
        ],
        "status" => vec![title, item("category", "状态类别", Choice(STATUS), "todo")],
        "task_type" => vec![
            title,
            item("statuses", "状态工作流", Many("$statuses"), ""),
            item("required_fields", "必填字段", Many("field"), ""),
        ],
        "relation_type" => vec![
            title,
            item("forward", "正向名称", Text, ""),
            item("reverse", "反向名称", Text, ""),
        ],
        "relation" => vec![
            item("title", "关联说明", Text, "任务关联"),
            item("from", "来源任务", Ref("task"), ""),
            item("type", "关系", Ref("relation_type"), "related"),
            item("to", "目标任务", Ref("task"), ""),
        ],
        _ => vec![title, body],
    }
}
pub fn workflow_name(op: &str) -> &'static str {
    match op {
        "provider.preview" => "预览平台事件",
        "provider.ingest" => "接收平台事件",
        "tool.authorize" => "验证 Agent 工具权限",
        "run.reply" => "回复工作机",
        "connector.ingest" => "导入外部事项",
        "connector.export" => "导出映射任务",
        "triage" => "合并至重复任务",
        _ => "操作",
    }
}
fn modal_specs(mode: &Mode) -> Vec<Spec> {
    use Format::*;
    use Mode::*;
    match mode {
        Create(k) => record_specs(k),
        Edit(r) => record_specs(&r.kind),
        Machine { stop } => {
            let mut s = vec![item("config", "工作机配置文件（完整路径）", Text, "")];
            if !stop {
                s.extend([
                    item("workspace", "新配置的工作目录（可选）", Text, ""),
                    item("program", "新配置的外部程序绝对路径（可选）", Text, ""),
                    item("runtime", "外部 Runtime 名称", Text, "local-tool"),
                    item("args", "外部程序固定参数（JSON 数组）", Json, "[]"),
                ]);
            }
            s
        }
        Upload(record) => {
            let mut s = vec![item("path", "文件路径（最大 4 MiB）", Text, "")];
            if record.is_none() {
                s.push(item("visibility", "文档可见范围", Choice(VIS), "private"));
            }
            s
        }
        Download(..) => vec![item("path", "保存到新文件", Text, "")],
        Workflow { op, .. } => match *op {
            "provider.preview" | "provider.ingest" => vec![
                item("payload", "平台原始事件（JSON 对象）", Json, "{}"),
                item("event_id", "GitLab 或 Jira 上游投递 ID", Text, ""),
            ],
            "run.reply" => vec![item("text", "回复内容", Body, "")],
            "tool.authorize" => vec![item("agent", "Agent", Ref("agent"), "")],
            "connector.ingest" => vec![
                item("external_id", "外部事项编号", Text, ""),
                item("event_id", "事件编号", Text, ""),
                item("source_version", "来源版本", Text, ""),
                item("title", "事项标题", Text, ""),
                item("description", "描述", Body, ""),
                item("closed", "已关闭", Bool, "false"),
            ],
            "connector.export" => vec![item("task_id", "要导出的任务", Ref("task"), "")],
            "triage" => vec![item("target", "重复的目标任务", Ref("task"), "")],
            _ => vec![],
        },
        Connect => vec![
            item("url", "服务地址", Text, ""),
            item("token", "访问令牌", Secret, ""),
        ],
        Filter => vec![
            item("query", "标题或编号", Text, ""),
            item("filter_status", "状态", Ref("$statuses"), ""),
            item("filter_priority", "优先级", Ref("$priorities"), ""),
            item("filter_assignee", "处理人", Ref("$handlers"), ""),
        ],
        Comment(_) | Message(_) => vec![item("body", "内容", Body, "")],
        IterationEnd(_) => vec![item(
            "target",
            "未完成任务转入（留空退回待规划）",
            Ref("iteration"),
            "",
        )],
        ResolveDraft(_) => vec![item(
            "expected_rev",
            "用来重新提交的服务器版本号",
            Number,
            "",
        )],
        Export | Import => vec![item("path", "JSON 文件完整路径", Text, "")],
        Token => vec![
            item("actor", "成员账号", Ref("member"), ""),
            item(
                "scope",
                "令牌权限",
                Choice(&[("read", "只读"), ("write", "读写"), ("worker", "工作机")]),
                "read",
            ),
        ],
        Search => vec![item("query", "搜索任务、文档、Agent…", Text, "")],
        Confirm { .. } | Output { .. } => vec![],
    }
}
#[derive(Clone)]
pub enum Control {
    Text(Field),
    Pick { value: String, focus: FocusHandle },
}
#[derive(Clone)]
pub struct Editor {
    pub spec: Spec,
    pub control: Control,
    pub trigger: Option<u64>,
}
impl Editor {
    fn new(
        spec: Spec,
        initial: &Map<String, Value>,
        trigger: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        let value = initial
            .get(spec.key)
            .map(|v| {
                if spec.format == Format::Json && !v.is_null() {
                    serde_json::to_string_pretty(v).unwrap_or_default()
                } else {
                    value_text(v)
                }
            })
            .unwrap_or_else(|| spec.default.to_owned());
        let control = match spec.format {
            Format::Choice(_) | Format::Ref(_) | Format::Many(_) | Format::Bool => Control::Pick {
                value,
                focus: cx.focus_handle().tab_stop(true),
            },
            _ => Control::Text(Field::new(
                &value,
                spec.label,
                matches!(spec.format, Format::Body | Format::Json),
                spec.format == Format::Secret,
                window,
                cx,
            )),
        };
        Self {
            spec,
            control,
            trigger,
        }
    }
    pub fn raw(&self, cx: &App) -> String {
        match &self.control {
            Control::Text(f) => f.value(cx),
            Control::Pick { value, .. } => value.clone(),
        }
    }
    pub fn handle(&self, cx: &App) -> FocusHandle {
        match &self.control {
            Control::Text(f) => f.handle(cx),
            Control::Pick { focus, .. } => focus.clone(),
        }
    }
    fn parsed(&self, cx: &App) -> Result<Value, String> {
        use Format::*;
        let raw = self.raw(cx);
        let t = raw.trim();
        Ok(match self.spec.format {
            Json => {
                if t.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_str(t)
                        .map_err(|e| format!("{} JSON 无效：{e}", self.spec.label))?
                }
            }
            Number => {
                if t.is_empty() {
                    Value::Null
                } else {
                    json!(
                        t.parse::<i64>()
                            .map_err(|_| format!("{}需要整数", self.spec.label))?
                    )
                }
            }
            Bool => json!(t == "true"),
            Csv | Many(_) => json!(
                t.split([',', '，'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            ),
            Ref(_) | Choice(_) => {
                if t.is_empty() {
                    Value::Null
                } else {
                    json!(t)
                }
            }
            Text | Body | Secret => json!(t),
        })
    }
}
#[derive(Clone)]
pub struct TriggerDraft {
    pub id: u64,
    original: Map<String, Value>,
}
pub struct Form {
    pub triggers: Vec<TriggerDraft>,
    next_trigger: u64,
    pub mode: Mode,
    pub entries: Vec<Editor>,
    pub open_pick: Option<usize>,
    pub error: String,
    pub origin: Option<FocusHandle>,
    pub search_results: Vec<Record>,
    pub last_search: String,
    pub preview: Option<Value>,
}
impl Form {
    pub fn new(
        mode: Mode,
        initial: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        let origin = window.focused(cx);
        let entries = modal_specs(&mode)
            .into_iter()
            .map(|spec| Editor::new(spec, initial, None, window, cx))
            .collect();
        let automation = mode.automation();
        let mut this = Self {
            mode,
            entries,
            triggers: vec![],
            next_trigger: 0,
            open_pick: None,
            error: String::new(),
            origin,
            search_results: vec![],
            last_search: String::new(),
            preview: None,
        };
        if automation {
            if let Some(triggers) = initial.get("triggers").and_then(Value::as_array) {
                for t in triggers {
                    this.add_trigger(t, window, cx);
                }
            } else {
                this.add_trigger(&json!({"type":"status","enabled":true}), window, cx);
            }
        }
        this
    }
    pub fn add_trigger(&mut self, raw: &Value, window: &mut Window, cx: &mut Context<Workbench>) {
        if self.triggers.len() >= 32 {
            self.error = "最多可配置 32 个触发器。".into();
            return;
        }
        use Format::*;
        let original = raw.as_object().cloned().unwrap_or_default();
        let id = self.next_trigger;
        self.next_trigger += 1;
        for spec in [
            item("type", "触发方式", Choice(TRIGGERS), "status"),
            item("status", "任务进入状态", Ref("$statuses"), ""),
            item("keyword", "评论包含文字（可选）", Text, ""),
            item(
                "frequency",
                "重复",
                Choice(&[
                    ("interval", "按间隔"),
                    ("daily", "每天"),
                    ("weekly", "每周"),
                ]),
                "interval",
            ),
            item("interval_minutes", "间隔分钟数", Number, "60"),
            item("local_time", "当地时间（HH:MM）", Text, "09:00"),
            item(
                "timezone",
                "时区",
                Choice(&[
                    ("Asia/Shanghai", "上海"),
                    ("Asia/Tokyo", "东京"),
                    ("Asia/Kolkata", "加尔各答"),
                    ("Europe/London", "伦敦"),
                    ("Europe/Berlin", "柏林"),
                    ("America/New_York", "纽约"),
                    ("America/Los_Angeles", "洛杉矶"),
                    ("UTC", "UTC"),
                ]),
                "Asia/Shanghai",
            ),
            item("weekdays", "每周执行日", Many("$weekdays"), "1, 2, 3, 4, 5"),
            item("enabled", "启用此触发器", Bool, "true"),
        ] {
            self.entries
                .push(Editor::new(spec, &original, Some(id), window, cx));
        }
        self.triggers.push(TriggerDraft { id, original });
        self.open_pick = None;
    }
    pub fn remove_trigger(&mut self, id: u64) {
        self.triggers.retain(|t| t.id != id);
        self.entries.retain(|e| e.trigger != Some(id));
        self.open_pick = None;
    }
    pub fn values(&self, cx: &App) -> Result<Map<String, Value>, String> {
        let mut fields = self
            .entries
            .iter()
            .filter(|e| e.trigger.is_none())
            .map(|e| Ok((e.spec.key.to_owned(), e.parsed(cx)?)))
            .collect::<Result<Map<_, _>, String>>()?;
        if self.mode.automation() {
            let mut triggers = Vec::new();
            for t in &self.triggers {
                let get = |key: &str| -> Result<Value, String> {
                    self.entries
                        .iter()
                        .find(|e| e.trigger == Some(t.id) && e.spec.key == key)
                        .ok_or_else(|| "触发条件缺少字段".to_owned())?
                        .parsed(cx)
                };
                let kind = get("type")?;
                let freq = get("frequency")?;
                let mut trigger = t.original.clone();
                for k in [
                    "type",
                    "enabled",
                    "frequency",
                    "status",
                    "keyword",
                    "interval_minutes",
                    "local_time",
                    "timezone",
                    "weekdays",
                ] {
                    trigger.remove(k);
                }
                trigger.insert("type".into(), kind.clone());
                trigger.insert("enabled".into(), get("enabled")?);
                for key in [
                    "status",
                    "keyword",
                    "frequency",
                    "interval_minutes",
                    "local_time",
                    "timezone",
                    "weekdays",
                ] {
                    if !trigger_field_visible(
                        key,
                        kind.as_str().unwrap_or(""),
                        freq.as_str().unwrap_or(""),
                    ) {
                        continue;
                    }
                    let mut val = get(key)?;
                    if key == "frequency" && val == "interval" {
                        continue;
                    }
                    if key == "weekdays" {
                        let days = val
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| v.as_str())
                            .map(|d| d.parse::<u8>().map_err(|_| "请选择星期".to_owned()))
                            .collect::<Result<Vec<_>, _>>()?;
                        val = json!(days);
                    }
                    if !val.is_null() && val.as_str() != Some("") {
                        trigger.insert(key.into(), val);
                    }
                }
                triggers.push(Value::Object(trigger));
            }
            fields.insert("triggers".into(), json!(triggers));
        }
        Ok(fields)
    }
}
pub fn trigger_field_visible(key: &str, kind: &str, frequency: &str) -> bool {
    match key {
        "status" => kind == "status",
        "keyword" => kind == "comment",
        "frequency" => kind == "schedule",
        "interval_minutes" => kind == "schedule" && frequency == "interval",
        "local_time" | "timezone" => kind == "schedule" && frequency != "interval",
        "weekdays" => kind == "schedule" && frequency == "weekly",
        _ => true,
    }
}
pub fn initial_record(r: &Record) -> Map<String, Value> {
    let mut f = r.fields.clone();
    f.insert("title".into(), json!(r.title));
    if let Some(filters) = r.fields.get("filter").and_then(Value::as_object) {
        for (k, v) in filters {
            f.insert(format!("filter_{k}"), v.clone());
        }
    }
    f
}
pub fn compile_record(
    kind: &str,
    mut fields: Map<String, Value>,
    assignee_kind: Option<&str>,
    creating: bool,
) -> Result<(String, Map<String, Value>), String> {
    let title = fields
        .remove("title")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    if title.trim().is_empty() {
        return Err("请填写名称或标题。".into());
    }
    for key in ["due_date", "start_date", "end_date"] {
        if fields.get(key).and_then(Value::as_str) == Some("") {
            fields.insert(key.into(), Value::Null);
        }
    }
    if kind == "task" {
        if fields.get("assignee").and_then(Value::as_str).is_some() {
            fields.insert(
                "assignee_kind".into(),
                json!(assignee_kind.unwrap_or("member")),
            );
        }
        if fields.get("owner").is_none_or(Value::is_null) {
            fields.remove("owner");
        }
    }
    if kind == "view" {
        let mut filter = Map::new();
        for key in ["type", "status", "priority", "assignee", "project"] {
            if let Some(v) = fields.remove(&format!("filter_{key}"))
                && !v.is_null()
            {
                filter.insert(key.into(), v);
            }
        }
        fields.insert("filter".into(), Value::Object(filter));
    }
    if creating {
        fields.retain(|_, v| !v.is_null() && v.as_str() != Some(""));
    }
    Ok((title, fields))
}
