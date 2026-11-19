use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt::Write;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Record {
    pub id: String,
    pub space: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub fields: Map<String, Value>,
    pub rev: i64,
}
impl Record {
    pub fn localized_title(&self) -> &str {
        if matches!(self.kind.as_str(), "status" | "status_library") {
            shipforge_core::status_display_name(
                &self.id,
                &self.title,
                self.fields.get("names"),
                self.fields.get("system").and_then(Value::as_bool) == Some(true),
                crate::locale::language(),
            )
        } else {
            &self.title
        }
    }
    pub fn field(&self, name: &str) -> &str {
        self.fields.get(name).and_then(Value::as_str).unwrap_or("")
    }
    pub fn display(&self, name: &str) -> String {
        self.fields.get(name).map(value_text).unwrap_or_default()
    }
    pub fn code(&self) -> String {
        match self.fields.get("number") {
            Some(Value::Number(n)) => format!("SF-{n}"),
            Some(Value::String(n)) if !n.is_empty() => n.clone(),
            _ => self.id.clone(),
        }
    }
}
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(value_text).collect::<Vec<_>>().join(", "),
        Value::Object(_) => serde_json::to_string_pretty(v).unwrap_or_default(),
        _ => v.to_string(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    GettingStarted,
    Tags,
    TagGroups,
    Tasks,
    Mine,
    Triage,
    Iterations,
    Projects,
    Views,
    Dashboard,
    Inbox,
    Chats,
    Assistant,
    Docs,
    Members,
    Spaces,
    SpaceModules,
    CustomerRequests,
    Agents,
    Crews,
    Skills,
    Market,
    Machines,
    Runs,
    Automations,
    Connectors,
    Settings,
    Fields,
    Relations,
    Statuses,
    Types,
    Transfer,
    Tools,
    Organizations,
    RelationTypes,
    Artifacts,
}
pub struct PageInfo {
    pub label: &'static str,
    pub icon: &'static str,
    pub kind: &'static str,
    pub noun: &'static str,
    pub empty: &'static str,
}
impl Page {
    pub fn info(self) -> PageInfo {
        use Page::*;
        let (label, icon, kind, noun, empty) = match self {
            GettingStarted => (
                "开始使用",
                "compass",
                "space",
                "设置向导",
                "开始你的第一项工作。",
            ),
            Tags => (
                "任务标签",
                "tag",
                "tag",
                "标签",
                "创建标签，按主题和来源整理任务。",
            ),
            TagGroups => (
                "标签分组",
                "layers",
                "tag_group",
                "标签分组",
                "将互斥的标签归入一组。",
            ),
            Tasks => (
                "任务",
                "list-checks",
                "task",
                "任务",
                "创建第一条任务，安排当前空间的工作。",
            ),
            Mine => (
                "我的任务",
                "circle-dot",
                "task",
                "任务",
                "分配给你的工作会出现在这里。",
            ),
            Dashboard => (
                "任务仪表盘",
                "chart-no-axes-column-increasing",
                "task",
                "任务",
                "还没有可统计的任务。",
            ),
            Triage => (
                "分拣台",
                "inbox",
                "task",
                "待分拣任务",
                "访客与外部集成提交的任务会在此等待审核。",
            ),
            Iterations => (
                "迭代",
                "calendar-range",
                "iteration",
                "迭代",
                "为一段时间内要交付的任务安排迭代。",
            ),
            Projects => (
                "专项",
                "layers",
                "project",
                "专项",
                "围绕目标、版本或客户交付组织跨迭代的任务。",
            ),
            Views => (
                "视图",
                "panels-top-left",
                "view",
                "视图",
                "保存筛选与布局，下次打开时看到最新任务。",
            ),
            Inbox => (
                "收件箱",
                "inbox",
                "notification",
                "提醒",
                "需要你处理的回复和任务动态会出现在这里。",
            ),
            Chats => (
                "对话",
                "messages-square",
                "chat",
                "对话",
                "选择 Agent 或创建对话，保存消息与结果。",
            ),
            Assistant => (
                "小助手",
                "bot",
                "chat",
                "对话",
                "新建一个私人会话，向工作伙伴交代任务。",
            ),
            Docs => (
                "文档库",
                "book-open",
                "document",
                "文档",
                "在个人或团队文档中保存决策、说明和交付成果。",
            ),
            Members => (
                "成员",
                "users",
                "member",
                "成员",
                "将成员加入空间，并设置合适的角色。",
            ),
            Spaces => (
                "空间",
                "boxes",
                "space",
                "空间",
                "创建空间以独立管理成员、任务和资料。",
            ),
            SpaceModules => (
                "更多",
                "boxes",
                "space",
                "空间功能",
                "选择要打开或固定的空间功能。",
            ),
            CustomerRequests => (
                "客户请求",
                "inbox",
                "customer_request",
                "客户请求",
                "记录客户反馈和请求的处理进度。",
            ),
            Agents => (
                "Agents",
                "bot",
                "agent",
                "Agent",
                "配置指令、技能与执行机器，让 Agent 参与任务。",
            ),
            Crews => (
                "小队",
                "users-round",
                "crew",
                "小队",
                "将多个 Agent 编成小队，选择队长统筹工作。",
            ),
            Skills => (
                "技能",
                "blocks",
                "skill",
                "技能",
                "把可重复使用的工作方法保存为技能。",
            ),
            Market => (
                "市场",
                "compass",
                "catalog",
                "技能",
                "在空间中查找可复用的技能和 Agent 模板。",
            ),
            Machines => (
                "机器",
                "monitor",
                "machine",
                "机器",
                "在工作机上启动 ShipForge Worker，即可查看在线能力。",
            ),
            Runs => (
                "运行历史",
                "clock-arrow-left",
                "run",
                "运行",
                "任务、对话或自动化生成的运行会出现在这里。",
            ),
            Automations => (
                "自动化",
                "zap",
                "automation",
                "自动化",
                "通过任务事件、定时或 Webhook 自动指派工作。",
            ),
            Connectors => (
                "外部应用集成",
                "plug",
                "connector",
                "连接器",
                "配置来源与字段映射，同步外部事项。",
            ),
            Tools => (
                "工具与连接",
                "puzzle",
                "tool",
                "工具",
                "登记可供 Agent 显式使用的工具与连接。",
            ),
            Settings => (
                "全部设置",
                "settings",
                "space",
                "空间设置",
                "管理此设备与团队服务的连接。",
            ),
            Fields => (
                "自定义字段",
                "sliders-horizontal",
                "field",
                "自定义字段",
                "添加任务类型所需填写的字段。",
            ),
            Relations => (
                "任务关联",
                "git-branch",
                "relation",
                "关联",
                "建立任务的阻塞、相关和重复关系。",
            ),
            Statuses => (
                "任务状态",
                "circle-dot",
                "status",
                "状态",
                "为不同工作阶段定义状态。",
            ),
            Types => (
                "任务类型",
                "type",
                "task_type",
                "任务类型",
                "维护空间的任务类型和必填字段。",
            ),
            Organizations => (
                "组织",
                "boxes",
                "organization",
                "组织",
                "创建组织，管理成员与空间。",
            ),
            RelationTypes => (
                "关联类型",
                "git-branch",
                "relation_type",
                "关联类型",
                "定义任务关系的正向与逆向名称。",
            ),
            Artifacts => (
                "运行产物",
                "file-text",
                "artifact",
                "产物",
                "工作机返回的文本、链接与产物可以保存为文档。",
            ),
            Transfer => (
                "导入与导出",
                "arrow-down-to-line",
                "task",
                "任务",
                "在文件与当前空间之间交换数据。",
            ),
        };
        PageInfo {
            label: crate::locale::label(label),
            icon,
            kind,
            noun: crate::locale::label(noun),
            empty: crate::locale::label(empty),
        }
    }
    pub fn from_kind(kind: &str) -> Option<Self> {
        const PAGES: &[Page] = &[
            Page::Tags,
            Page::TagGroups,
            Page::Tasks,
            Page::Iterations,
            Page::Projects,
            Page::Views,
            Page::Inbox,
            Page::Chats,
            Page::Docs,
            Page::Members,
            Page::Spaces,
            Page::CustomerRequests,
            Page::Agents,
            Page::Crews,
            Page::Skills,
            Page::Machines,
            Page::Runs,
            Page::Automations,
            Page::Connectors,
            Page::Fields,
            Page::Relations,
            Page::Statuses,
            Page::Types,
            Page::Tools,
            Page::Organizations,
            Page::RelationTypes,
            Page::Artifacts,
        ];
        PAGES.iter().copied().find(|p| p.info().kind == kind)
    }
    pub fn is_tasks(self) -> bool {
        matches!(
            self,
            Self::Tasks | Self::Mine | Self::Triage | Self::Dashboard
        )
    }
    pub fn creatable(self) -> bool {
        !matches!(
            self,
            Self::Market
                | Self::GettingStarted
                | Self::SpaceModules
                | Self::Artifacts
                | Self::Inbox
                | Self::Machines
                | Self::Runs
                | Self::Settings
                | Self::Transfer
                | Self::Dashboard
        )
    }
}
pub const STATUSES: &[(&str, &str, u32)] = &[
    ("duplicate", "重复", 0x878a93),
    ("triage", "待分拣", 0xa88bd3),
    ("backlog", "待规划", 0x9a9dab),
    ("todo", "待开始", 0x858b99),
    ("in_progress", "进行中", 0xd29b2a),
    ("done", "已完成", 0x6474bc),
    ("cancelled", "已取消", 0x92929b),
];
pub const PRIORITIES: &[(&str, &str)] = &[
    ("none", "无优先级"),
    ("urgent", "紧急"),
    ("high", "高"),
    ("medium", "中"),
    ("low", "低"),
    ("lowest", "最低"),
];
pub const TYPES: &[(&str, &str)] = &[("task", "任务"), ("requirement", "需求"), ("bug", "缺陷")];
pub fn label_value(key: &str, v: &str) -> String {
    let choices: &[(&str, &str)] = match key {
        "waiting_reason" => &[
            ("outside_working_hours", "等待工作时段"),
            ("scheduled_start", "等待预约时间"),
            ("agent_capacity", "Agent 并发已满"),
            ("machine_capacity", "机器并发已满"),
            ("conversation_busy", "对话中还有运行"),
            ("task_busy", "此任务有活动运行"),
            ("crew_workspace_busy", "小队成员正在使用任务目录"),
            ("directory_busy", "获准目录均在使用"),
            ("directory_unavailable", "工作目录不可用"),
            ("no_machine", "没有可用的机器"),
            ("machine_offline", "工作机离线"),
            ("runtime_unavailable", "运行时不可用"),
            ("queued", "排队中"),
        ],
        "type" => TYPES,
        "priority" => PRIORITIES,
        "visibility" => &[("space", "空间共享"), ("private", "仅自己")],
        "role" => &[
            ("owner", "所有者"),
            ("admin", "管理员"),
            ("member", "成员"),
            ("viewer", "只读"),
            ("guest", "访客"),
        ],
        "layout" => &[("board", "看板"), ("list", "列表"), ("table", "表格")],
        "status" => {
            return STATUSES
                .iter()
                .find(|s| s.0 == v)
                .map(|s| crate::locale::label(s.1).to_owned())
                .unwrap_or_else(|| {
                    crate::locale::label(match v {
                        "scheduled" => "已计划",
                        "active" => "进行中",
                        "closed" => "已结束",
                        "queued" => "排队中",
                        "running" => "运行中",
                        "failed" => "失败",
                        "success" | "succeeded" => "成功",
                        "cancelled" => "已取消",
                        _ => v,
                    })
                    .to_string()
                });
        }
        _ => &[],
    };
    choices
        .iter()
        .find(|(k, _)| *k == v)
        .map(|(id, s)| crate::locale::choice(key, id, s).to_owned())
        .unwrap_or_else(|| v.to_owned())
}
pub fn summary(record: &Record) -> String {
    for k in [
        "description",
        "body",
        "instruction",
        "instructions",
        "result",
        "reason",
        "provider",
        "role",
        "runtime",
    ] {
        if let Some(first) = record.field(k).lines().find(|l| !l.trim().is_empty()) {
            return first.chars().take(140).collect();
        }
    }
    let mut out = String::new();
    for key in ["start_date", "end_date", "status", "version", "visibility"] {
        let v = record.display(key);
        if !v.is_empty() {
            if !out.is_empty() {
                out.push_str(" · ");
            }
            let _ = write!(out, "{}", label_value(key, &v));
        }
    }
    out
}
