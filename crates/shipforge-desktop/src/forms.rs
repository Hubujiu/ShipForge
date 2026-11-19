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
    SecretJson,
    Number,
    Decimal,
    Bool,
    Csv,
    Json,
    Choice(&'static [(&'static str, &'static str)]),
    Ref(&'static str),
    Many(&'static str),
}
impl Format {
    pub fn label_picker(self) -> bool {
        matches!(
            self,
            Self::Ref("tag" | "tag_group" | "$tag_choices") | Self::Many("tag" | "$tag_choices")
        )
    }
}
#[derive(Clone, Copy)]
pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    pub format: Format,
    pub default: &'static str,
}
pub(crate) fn item(
    key: &'static str,
    label: &'static str,
    format: Format,
    default: &'static str,
) -> Spec {
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
    ("duplicate", "重复"),
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
    GitWorkspace(crate::app::git_workspace_ui::Dialog),
    MachineManagement(crate::app::machine_management::Dialog),
    Device(crate::app::device_ui::Dialog),
    QuickCreate,
    PeopleFilter,
    OrgDefaults(Record),
    SpaceTransfer(Record, Vec<(String, String)>),
    OrgOwnership(Record, &'static str),
    OrgDepart(Record, &'static str),
    StartChat(&'static str),
    Library(crate::app::market_ui::LibraryForm),
    Forge(crate::app::repository_ui::Form),
    Bot(crate::app::personal_bots_ui::BotForm),
    Webhook(crate::app::webhook_ui::HookForm),
    Identity(crate::app::identity_ui::IdentityForm),
    Inbox(crate::app::inbox_ui::InboxForm),
    Directory(crate::app::agent_directory::DirectoryForm),
    Work(crate::app::workspace_ui::WorkForm),
    Conversation(crate::app::conversation_ui::CommandForm),
    Review(crate::app::review_ui::ReviewForm),
    BindChat(Record),
    ChatRow(Record),
    DeactivateSpace {
        space: Record,
        unfinished: u64,
        token: String,
    },
    BulkTags,
    FindLabels,
    MergeTags,
    OpenChatShare,
    Invite {
        email: bool,
    },
    JoinInvite,
    SetupOrganization,
    SetupSpace {
        organization: String,
        slug: String,
    },
    MissingMachine,
    GuideTarget {
        step: &'static str,
        kind: &'static str,
    },
    MoveTask,
    FindInView,
    Create(String),
    Edit(Record),
    Filter,
    Connect,
    Reauthenticate {
        account: String,
        url: String,
    },
    Comment(Record),
    ThreadReply {
        task: String,
        target: Value,
        request_id: String,
    },
    Message(Record),
    IterationEnd(Record),
    Upload(Option<Record>),
    Download(Record, Value),
    Machine {
        stop: bool,
    },
    LocalDirectory {
        diagnose: bool,
    },
    Workflow {
        op: &'static str,
        record: Record,
    },
    ResolveDraft(Value),
    Confirm {
        label: String,
        command: Value,
    },
    Export,
    Import,
    Token,
    PersonalToken {
        machine_setup: bool,
        request_id: String,
    },
    Search,
    Output {
        heading: String,
        content: String,
    },
}
impl Mode {
    pub fn automation(&self) -> bool {
        matches!(self,Self::Create(k) if k=="automation")
            || matches!(self,Self::Edit(r) if r.kind=="automation")
    }

    pub fn heading(&self) -> String {
        use Mode::*;
        match self {
            MachineManagement(f) => f.title().into(),
            Device(f) => f.title().into(),
            Forge(f) => f.title().into(),
            GitWorkspace(f) => f.title().into(),
            Bot(f) => f.title().into(),
            Webhook(f) => f.heading().into(),
            Identity(f) => f.title().into(),
            QuickCreate => "快捷新建".into(),
            PeopleFilter => "查找成员与机器".into(),
            OrgDefaults(_) => "组织默认设置".into(),
            SpaceTransfer(_, _) => "转让空间管理权".into(),
            OrgOwnership(_, action) => match *action {
                "grant" => "添加组织所有者",
                "transfer" => "转让组织所有权",
                _ => "将所有者改为管理员",
            }
            .into(),
            OrgDepart(r, action) => format!(
                "{} · {}（{}）",
                crate::locale::label(if *action == "leave" {
                    "离开组织"
                } else {
                    "删除组织"
                }),
                r.title,
                r.fields["members"]
            ),
            StartChat(kind) => crate::app::navigation::chat_label(kind).into(),
            Directory(f) => f.title().into(),
            Library(f) => f.title().into(),
            Work(f) => f.title(),
            Conversation(f) => f.title().into(),
            Review(f) => f.title().into(),
            Inbox(crate::app::inbox_ui::InboxForm::Followers(_, names)) if !names.is_empty() => {
                format!("{} · {names}", crate::locale::label("更改关注者"))
            }
            Inbox(crate::app::inbox_ui::InboxForm::Reaction(_, counts)) if !counts.is_empty() => {
                format!("{} · {counts}", crate::locale::label("表情回应"))
            }
            Inbox(f) => f.title().into(),
            BindChat(_) => "绑定到已有任务".into(),
            ChatRow(room) => room.title.clone(),
            DeactivateSpace { .. } => "停用空间".into(),
            OpenChatShare => "打开已分享的对话".into(),
            SetupOrganization => "创建你的组织".into(),
            Invite { email: true } => "通过邮箱邀请成员".into(),
            Invite { email: false } => "创建组织邀请链接".into(),
            JoinInvite => "接受组织邀请".into(),
            SetupSpace { .. } => "初始化默认空间".into(),
            MissingMachine => "没有可用的机器".into(),
            LocalDirectory { diagnose } => if *diagnose {
                "本机进程与日志"
            } else {
                "授权本机工作目录"
            }
            .into(),
            GuideTarget { kind, .. } => crate::locale::select_title(noun(kind)),
            MoveTask => "将任务移动到空间".into(),
            FindInView => "在当前视图查找".into(),
            Machine { stop } => if *stop {
                "停止本机工作机"
            } else {
                "启动本机工作机"
            }
            .into(),
            Upload(_) => "上传附件".into(),
            Download(..) => "下载附件".into(),
            Workflow { op, .. } => workflow_name(op).into(),
            Create(k) => crate::locale::new_title(noun(k)),
            Edit(r) => crate::locale::edit_title(noun(&r.kind)),
            Filter => "筛选任务".into(),
            BulkTags => "批量处理标签".into(),
            FindLabels => "查找标签".into(),
            MergeTags => "合并标签".into(),
            Connect => "连接团队服务".into(),
            Reauthenticate { .. } => "重新登录账号".into(),
            Comment(_) => "添加评论".into(),
            ThreadReply { target, .. } => format!(
                "{} · {} / {}",
                crate::locale::label("回复到 Slack 话题"),
                target["thread"]["channel"].as_str().unwrap_or(""),
                target["thread"]["thread"].as_str().unwrap_or("")
            ),
            Message(_) => "发送消息".into(),
            IterationEnd(_) => "结束迭代".into(),
            ResolveDraft(_) => "处理同步冲突".into(),
            Confirm { label, .. } => label.clone(),
            Export => "导出空间".into(),
            Import => "导入空间".into(),
            Token => "创建访问令牌".into(),
            PersonalToken { .. } => "创建个人访问令牌".into(),
            Search => "搜索工作空间".into(),
            Output { heading, .. } => heading.clone(),
        }
    }
    pub fn submit_label(&self) -> &'static str {
        match self {
            Self::MachineManagement(f) => f.submit(),
            Self::Device(f) => f.submit(),
            Self::Forge(f) => f.submit(),
            Self::GitWorkspace(f) => f.submit_label(),
            Self::Bot(f) => f.submit(),
            Self::Webhook(f) => f.submit(),
            Self::Identity(f) => f.submit(),
            Self::QuickCreate => "关闭",
            Self::PeopleFilter => "查找",
            Self::OrgDefaults(_) => "保存",
            Self::SpaceTransfer(_, _) => "确认转让",
            Self::OrgOwnership(_, _) => "确认",
            Self::OrgDepart(_, _) => "确认",
            Self::StartChat(_) => "打开对话",
            Self::Directory(_) => "确认",
            Self::Library(f) => f.submit_label(),
            Self::Work(f) => f.submit_label(),
            Self::Conversation(f) => f.submit(),
            Self::Review(_) => "确认",
            Self::Inbox(f) => f.submit_label(),
            Self::BindChat(_) => "查找",
            Self::ChatRow(_) => "关闭",
            Self::DeactivateSpace { .. } => "停用空间",
            Self::OpenChatShare => "打开",
            Self::SetupOrganization => "继续",
            Self::Invite { .. } => "生成邀请链接",
            Self::JoinInvite => "接受邀请",
            Self::SetupSpace { .. } => "进入工作区",
            Self::MissingMachine => "连接机器",
            Self::LocalDirectory { diagnose } => {
                if *diagnose {
                    "读取本机诊断"
                } else {
                    "批准目录"
                }
            }
            Self::GuideTarget { .. } => "继续",
            Self::MoveTask => "移动",
            Self::FindInView => "查找",
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
            Self::BulkTags => "应用到所选任务",
            Self::FindLabels => "查找",
            Self::MergeTags => "合并至目标标签",
            Self::Connect | Self::Reauthenticate { .. } => "连接",
            Self::Comment(_) | Self::Message(_) | Self::ThreadReply { .. } => "发送",
            Self::Search => "搜索",
            Self::Token => "签发令牌",
            Self::PersonalToken { .. } => "创建令牌",
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
            "tag_group" => "标签分组",
            "invitation" => "成员邀请",
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
            item("filter_labels", "同时含有标签", Many("tag"), ""),
        ]
    };
    match kind {
        "task" => vec![
            item("title", "任务标题", Text, ""),
            body,
            item("type", "类型", Ref("$types"), "task"),
            item("status", "状态", Ref("$task_statuses"), ""),
            item("priority", "优先级", Ref("$priorities"), "none"),
            item("assignees", "人员处理人", Many("member"), ""),
            item("executor", "执行者（Agent 或小队）", Ref("$executors"), ""),
            item("scheduled_at", "预约执行（含时区的日期时间）", Text, ""),
            item("owner", "负责人", Ref("member"), ""),
            item("iteration", "迭代", Ref("iteration"), ""),
            item("project", "专项", Ref("project"), ""),
            item("start_date", "开始日期（YYYY-MM-DD）", Text, ""),
            item("due_date", "截止日期（YYYY-MM-DD）", Text, ""),
            item("planned_start", "计划开始（YYYY-MM-DD）", Text, ""),
            item("planned_end", "计划结束（YYYY-MM-DD）", Text, ""),
            item("labels", "任务标签", Many("$tag_choices"), ""),
            item("estimate", "预估工作量", Decimal, ""),
            item("parent", "父任务", Ref("task"), ""),
            item("custom", "自定义字段值（JSON 对象）", Json, "{}"),
        ],
        "iteration" => vec![
            title,
            item("owner", "迭代负责人", Ref("member"), ""),
            body,
            item("start_date", "开始日期（YYYY-MM-DD）", Text, ""),
            item("end_date", "结束日期（YYYY-MM-DD）", Text, ""),
            item("capacity", "容量（任务数）", Number, ""),
        ],
        "project" => vec![
            title,
            body,
            item("project_owner", "专项负责人", Ref("$project_owners"), ""),
            item(
                "status",
                "专项状态",
                Choice(&[
                    ("draft", "草稿"),
                    ("planned", "计划中"),
                    ("active", "进行中"),
                    ("paused", "已暂停"),
                    ("done", "已完成"),
                    ("cancelled", "已取消"),
                ]),
                "draft",
            ),
            item("priority", "优先级", Ref("$priorities"), "none"),
            item("spaces", "关联空间", Many("space"), ""),
            item("primary_space", "主空间", Ref("space"), ""),
            item("start_date", "开始日期（YYYY-MM-DD）", Text, ""),
            item("due_date", "目标日期（YYYY-MM-DD）", Text, ""),
        ],
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
            item("key", "任务编号前缀", Text, "SF"),
            item(
                "visibility",
                "空间可见性",
                Choice(&[("private", "邀请成员"), ("public", "公开")]),
                "private",
            ),
            item("triage_enabled", "空间分拣台", Bool, "false"),
            item("iterations_enabled", "迭代模块", Bool, "true"),
            item("customer_requests_enabled", "客户请求模块", Bool, "false"),
            item("allow_multiple_assignees", "允许多个处理人", Bool, "false"),
            item("estimates_enabled", "任务估时", Bool, "false"),
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
        "customer_request" => vec![
            title,
            body,
            item("contact", "客户或联系人", Text, ""),
            item(
                "status",
                "处理状态",
                Choice(&[
                    ("open", "待处理"),
                    ("in_progress", "进行中"),
                    ("resolved", "已解决"),
                ]),
                "open",
            ),
        ],
        "document" => vec![title, item("body", "正文", Body, ""), visibility],
        "chat" => vec![
            title,
            item(
                "chat_type",
                "对话类型",
                Choice(&[("conversation", "普通对话"), ("group", "群聊")]),
                "conversation",
            ),
            body,
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
            item(
                "agent_type",
                "Agent 类型",
                Choice(&[("code", "代码开发"), ("office", "日常办公")]),
                "code",
            ),
            item("instructions", "职责与指令", Body, ""),
            item("model", "默认模型", Text, ""),
            item("reasoning_effort", "模型思考深度", Text, ""),
            item("environment", "环境变量（JSON）", SecretJson, ""),
            item("arguments", "自定义参数（JSON）", Json, ""),
            item(
                "variant_permission",
                "谁可以创建分身",
                Choice(&[("use", "可使用者"), ("manage", "可管理者")]),
                "use",
            ),
            item(
                "allow_variant_environment",
                "允许他人的分身继承环境变量",
                Bool,
                "true",
            ),
            item("runtime", "Runtime 名称", Text, "local-tool"),
            item("machine_id", "指定机器", Ref("machine"), ""),
            item(
                "directory_mode",
                "工作目录策略",
                Choice(&[
                    ("isolated", "独立工作树"),
                    ("direct", "指定目录串行"),
                    ("temporary", "纯临时目录"),
                ]),
                "isolated",
            ),
            item("skills", "技能", Many("skill"), ""),
            item(
                "profiles",
                "获准工作目录（依次择空闲）",
                Many("$approved_directories"),
                "",
            ),
            item("tools", "授权工具", Many("tool"), ""),
            item("max_parallel", "任务并发数", Number, ""),
            item("chat_parallel", "对话并发数", Number, ""),
            item(
                "chat_capacity_mode",
                "对话并发策略",
                Choice(&[("independent", "独立对话额度"), ("shared", "共用任务额度")]),
                "independent",
            ),
            item("run_timeout_minutes", "最大运行时长（分钟）", Number, ""),
            item("queue_timeout_hours", "排队超时（小时）", Number, ""),
            item("timeout_seconds", "超时秒数", Number, ""),
            item("working_hours", "工作时段（JSON）", Json, ""),
            item(
                "hours_mode",
                "排班方案",
                Choice(&[
                    ("always", "全天接单"),
                    ("off_peak", "工作日夜间及周末"),
                    ("custom", "自定义工作时段"),
                ]),
                "always",
            ),
            item("hours_zone", "排班时区", Text, "Asia/Shanghai"),
            item("hours_assignment", "指派任务遵守排班", Bool, "true"),
            item("hours_comment", "评论提及遵守排班", Bool, "true"),
            item("hours_chat", "对话消息遵守排班", Bool, "false"),
            visibility,
        ],
        "crew" => vec![
            title,
            body,
            item("leader_id", "队长 Agent", Ref("agent"), ""),
            item("members", "小队成员", Many("agent"), ""),
            item(
                "share_task_directory",
                "同机成员共用任务目录",
                Bool,
                "false",
            ),
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
            item(
                "run_owner",
                "状态或评论触发的会话负责人（留空使用触发者）",
                Ref("member"),
                "",
            ),
            item("project", "关联专项", Ref("project"), ""),
            item("notify", "通知成员", Many("member"), ""),
            item("enabled", "规则状态", Bool, "true"),
        ],
        "connector" => vec![
            title,
            item(
                "provider",
                "平台",
                Choice(&[
                    ("github", "GitHub"),
                    ("jira", "Jira"),
                    ("bugly", "腾讯 Bugly"),
                    ("tapd", "TAPD"),
                    ("zoho", "Zoho BugTracker"),
                    ("zentao", "禅道"),
                    ("pingcode", "PingCode"),
                    ("teambition", "Teambition"),
                    ("cnb", "CNB"),
                    ("gitlab", "GitLab"),
                    ("linear", "Linear"),
                    ("clickup", "ClickUp"),
                    ("ones", "ONES"),
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
            item("repository", "代码托管仓库路径", Text, ""),
            item("project_id", "GitLab 项目 ID", Text, ""),
            item(
                "repository_host",
                "GitLab 主机（默认 gitlab.com）",
                Text,
                "",
            ),
            item("link_tasks", "按任务编号关联提交和合并请求", Bool, "false"),
            item(
                "review_rule_id",
                "新建或重开合并请求时运行的规则（可选）",
                Ref("automation"),
                "",
            ),
            item("project_key", "Jira 项目标识", Text, ""),
            item("portal_id", "Zoho 门户 ID", Text, ""),
            item("instance_url", "禅道安装根地址", Text, ""),
            item(
                "scope_kind",
                "禅道来源",
                Choice(&[("product", "产品"), ("project", "项目")]),
                "product",
            ),
            item(
                "zentao_edition",
                "禅道版本类型",
                Choice(&[
                    ("opensource", "开源版"),
                    ("enterprise", "企业版"),
                    ("ultimate", "旗舰版"),
                ]),
                "opensource",
            ),
            item("zentao_version", "禅道版本号（可选）", Text, ""),
            item("enterprise_id", "供应商企业、团队或工作区 ID", Text, ""),
            item("cnb_group", "CNB 组路径", Text, ""),
            item(
                "cnb_backlinks",
                "向 CNB 议题和合并请求发送任务回链",
                Bool,
                "false",
            ),
            item("vendor_delete", "允许本人确认后删除远端任务", Bool, "false"),
            item("ones_url", "ONES 租户地址", Text, ""),
            item(
                "cloud_auth",
                "云平台凭据方式",
                Choice(&[("key", "个人 API Key"), ("oauth", "OAuth")]),
                "key",
            ),
            item(
                "pingcode_auth",
                "PingCode 凭据方式",
                Choice(&[
                    ("token", "访问令牌"),
                    ("authorization", "用户授权码"),
                    ("application", "应用凭据"),
                ]),
                "token",
            ),
            item("remote_scope", "来源产品或项目 ID", Text, ""),
            item("vendor_write", "允许显式回写来源平台", Bool, "false"),
            item(
                "tapd_auth",
                "TAPD 凭据类型",
                Choice(&[("personal", "个人访问令牌"), ("company", "公司 API 账号")]),
                "personal",
            ),
            item("company_id", "TAPD 公司 ID（公司账号使用）", Text, ""),
            item("tapd_workspace", "TAPD 项目 ID", Text, ""),
            item("zoho_project", "Zoho 项目 ID（可先发现项目）", Text, ""),
            item("product_id", "Bugly 产品 ID", Text, ""),
            item(
                "default_task_type",
                "Bugly 默认任务类型",
                Ref("task_type"),
                "",
            ),
            item("bugly_outbound", "启用 Bugly 状态回写", Bool, "false"),
            item("workspace_id", "Slack 工作区 ID", Text, ""),
            item("channel_id", "Slack 频道 ID", Text, ""),
            item("app_id", "Slack 应用 ID", Text, ""),
            item("bot_id", "Slack 机器人 ID", Text, ""),
            item("outbound", "将任务活动回写 Slack 线程", Bool, "false"),
            item(
                "slack_assignment",
                "允许从 Slack 卡片改派处理人",
                Bool,
                "false",
            ),
            item("chat_id", "飞书或 Telegram 会话 ID", Text, ""),
            item("enabled", "启用", Bool, "true"),
            body,
            item("mapping", "字段映射（JSON）", Json, "{}"),
            item("assignee_map", "外部账号到空间成员映射（JSON）", Json, "{}"),
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
                    ("member", "成员（单选）"),
                    ("members", "成员（多选）"),
                ]),
                "text",
            ),
            item("options", "可用选项（用逗号分隔）", Csv, ""),
            item("default", "默认显示", Bool, "false"),
            item("order", "顺序", Number, ""),
        ],
        "tag" | "tag_group" => {
            let mut fields = vec![
                title,
                body,
                item(
                    "scope",
                    "标签作用范围",
                    Choice(&[("space", "当前空间"), ("organization", "整个组织")]),
                    "space",
                ),
            ];
            if kind == "tag" {
                fields.extend([
                    item("color", "标签颜色（#RRGGBB）", Text, ""),
                    item("group", "标签分组", Ref("tag_group"), ""),
                    item("archived", "归档标签", Bool, "false"),
                ]);
            }
            fields
        }
        "status" => vec![title, item("category", "状态类别", Choice(STATUS), "todo")],
        "task_type" => vec![
            title,
            item("enabled", "启用任务类型", Bool, "true"),
            item("statuses", "状态工作流", Many("$statuses"), ""),
            item("required_fields", "必填字段", Many("field"), ""),
            item("use_bindings", "限制此类型可用字段", Bool, "false"),
            item("bound_ids", "绑定字段（按选择顺序）", Many("field"), ""),
            item("disabled_ids", "暂时隐藏的绑定", Many("field"), ""),
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
        "status.library.save" => "编辑组织状态",
        "status.library.removal" => "预览状态迁移",
        "task_type.workflow.save" => "配置类型工作流",
        "asana.accounts" => "发现 Asana 工作区",
        "asana.credential" => "绑定 Asana 工作区令牌",
        "asana.projects" => "发现 Asana 项目",
        "asana.preview" => "预览 Asana 项目任务",
        "asana.synchronize" => "读取 Asana 项目变更",
        "asana.push" => "回写 Asana 字段或新评论",
        "teambition.application" => "配置 Teambition 组织应用",
        "teambition.authorize" => "生成 Teambition 个人授权地址",
        "teambition.personal" => "绑定我的 Teambition 身份",
        "teambition.projects" => "发现我加入的 Teambition 项目",
        "teambition.preview" => "预览 Teambition 项目任务",
        "teambition.synchronize" => "读取 Teambition 项目变更",
        "teambition.push" => "以我的身份回写 Teambition",
        "cnb.credential" => "配置或校验 CNB 组令牌",
        "cnb.repositories" => "发现 CNB 组内仓库",
        "cnb.pair" => "配对 CNB 仓库或轮换签名密钥",
        "cnb.ci" => "查看 CNB 流水线片段",
        "cnb.activity" => "查看 CNB 事件与回链记录",
        "cnb.deliver" => "发送或核对 CNB 回链",
        "iteration.configure" => "空间迭代设置",
        "chat.share.create" => "分享已选消息",
        "chat.manage" => "管理对话成员",
        "chat.fork.workspace" => "选择分支对话的工作目录",
        "assistant.send" => "给小助手发消息",
        "chat.avatar.set" => "更换群头像",
        "task_assistant.bind" => "绑定空间任务助手",
        "task_assistant.configure" => "设置空间任务助手",
        "task_assistant.template.save" => "编辑任务助手模板",
        "chat.agents" => "管理群内 Agent",
        "provider.preview" => "预览平台事件",
        "zoho.projects" => "发现 Zoho 项目",
        "zentao.projects" => "发现禅道产品或项目",
        "pingcode.projects" => "发现 PingCode 项目",
        "zentao.credential" => "连接禅道实例",
        "pingcode.credential" => "连接 PingCode 企业",
        "zentao.preview" => "预览禅道导入",
        "pingcode.preview" => "预览 PingCode 导入",
        "linear.accounts" | "clickup.accounts" | "ones.accounts" => "读取可访问的供应商账号",
        "linear.credential" | "clickup.credential" | "ones.credential" => "绑定或轮换供应商凭据",
        "linear.projects" | "ones.projects" => "发现可访问项目",
        "clickup.projects" => "发现 ClickUp 列表",
        "linear.preview" | "clickup.preview" | "ones.preview" => "预览供应商任务",
        "linear.authorize" | "clickup.authorize" => "生成 OAuth 授权地址",
        "imports.push" => "回写关联工作项",
        "imports.synchronize" => "立即同步工作项",
        "imports.sync_control" => "暂停或恢复同步",
        "imports.push_history" => "查看工作项回写记录",
        "zoho.credential" => "保存或更新 Zoho 凭据",
        "tapd.credential" => "配置 TAPD 凭据",
        "tapd.projects" => "发现 TAPD 项目",
        "tapd.preview" => "预览 TAPD 导入与更新",
        "bugly.credentials" => "配置 Bugly 回写凭据",
        "zoho.preview" => "预览 Zoho 导入",
        "imports.commit" => "确认导入批次",
        "imports.history" => "查看导入历史",
        "imports.rollback" => "撤销导入批次",
        "bugly.rotate" => "生成或轮换提单地址",
        "bugly.revoke" => "断开 Bugly 提单",
        "bugly.outbox" => "查看状态回写记录",
        "bugly.deliver" => "投递待发状态",
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
        MachineManagement(f) => f.specs(),
        Device(f) => f.specs(),
        Forge(f) => f.specs(),
        GitWorkspace(f) => f.specs(),
        Bot(f) => f.specs(),
        Webhook(f) => f.specs(),
        Identity(f) => f.specs(),
        QuickCreate => vec![],
        PeopleFilter => vec![item("query", "名称或标识", Text, "")],
        OrgDefaults(_) => vec![
            item(
                "default_language",
                "组织默认语言",
                Choice(&[("zh-CN", "简体中文"), ("en", "English")]),
                "zh-CN",
            ),
            item(
                "priority_levels",
                "任务优先级档数",
                Choice(&[("4", "4 级"), ("5", "5 级")]),
                "4",
            ),
        ],
        OrgDepart(_, _) => vec![item("confirmation", "输入组织名称确认", Text, "")],
        SpaceTransfer(_, _) => vec![item("target", "空间成员", Ref("$space_successors"), "")],
        OrgOwnership(_, action) => vec![item(
            "target",
            "组织成员",
            Ref(match *action {
                "grant" => "$org_non_owners",
                "transfer" => "$org_successors",
                _ => "$org_current_owners",
            }),
            "",
        )],
        StartChat("group") => vec![
            item("title", "群聊名称", Text, ""),
            item("members", "真人成员", Many("$other_people"), ""),
            item("agents", "参与 Agent", Many("agent"), ""),
        ],
        StartChat(kind) => vec![item(
            "target",
            "选择对象",
            Ref(if *kind == "member" {
                "$other_people"
            } else {
                kind
            }),
            "",
        )],
        Directory(f) => f.specs(),
        Library(f) => f.specs(),
        Work(f) => f.specs(),
        Conversation(f) => f.specs(),
        Review(f) => f.specs(),
        Inbox(f) => f.specs(),
        BindChat(_) => vec![item("query", "搜索任务标题或编号", Text, "")],
        ChatRow(_) => vec![],
        DeactivateSpace { unfinished, .. } if *unfinished > 0 => vec![
            item(
                "unfinished",
                "未完成任务的处理方式",
                Choice(&[
                    ("", "请选择处理方式"),
                    ("move", "移动到其他空间"),
                    ("cancel", "取消未完成任务"),
                ]),
                "",
            ),
            item("target", "目标空间", Ref("$deactivation_targets"), ""),
        ],
        DeactivateSpace { .. } => vec![],
        OpenChatShare => vec![item("share", "分享链接或分享码", Secret, "")],
        Invite { email } => {
            let mut entries = Vec::new();
            if *email {
                entries.push(item("email", "受邀成员邮箱", Text, ""));
            }
            let roles: &'static [(&str, &str)] = if *email {
                &[("member", "成员"), ("guest", "访客"), ("admin", "管理员")]
            } else {
                &[("member", "成员"), ("guest", "访客")]
            };
            entries.extend([
                item("role", "组织角色", Choice(roles), "member"),
                item("spaces", "同时加入空间", Many("$invite_spaces"), ""),
            ]);
            entries
        }
        JoinInvite => vec![item("link", "邀请链接", Secret, "")],
        FindLabels => vec![item("query", "标签名称", Text, "")],
        BulkTags => vec![
            item("tasks", "所选任务", Many("task"), ""),
            item(
                "action",
                "标签操作",
                Choice(&[
                    ("add", "添加"),
                    ("remove", "移除"),
                    ("replace", "替换全部"),
                    ("clear_group", "清除分组"),
                ]),
                "add",
            ),
            item("labels", "标签", Many("tag"), ""),
            item("group", "要清除的分组", Ref("tag_group"), ""),
        ],
        MergeTags => vec![
            item("source", "待合并标签", Ref("tag"), ""),
            item("target", "目标标签", Ref("$tag_choices"), ""),
        ],
        SetupOrganization => vec![
            item("organization_name", "组织名称", Text, ""),
            item("organization_slug", "组织地址标识", Text, ""),
        ],
        SetupSpace { .. } => vec![
            item(
                "work_type",
                "工作类型",
                Choice(&[
                    ("software", "软件研发"),
                    ("project", "项目管理"),
                    ("other", "其他工作"),
                ]),
                "software",
            ),
            item(
                "task_types",
                "起始任务类型",
                Many("$setup_types"),
                "requirement, bug",
            ),
        ],
        MissingMachine => vec![],
        LocalDirectory { diagnose } => {
            let mut s = vec![item("config", "工作机配置文件（完整路径）", Text, "")];
            if !diagnose {
                s.extend([
                    item("profile", "目录显示名称", Text, ""),
                    item("workspace", "本机工作目录", Text, ""),
                    item("direct", "允许直接写入此目录", Bool, "false"),
                ]);
            }
            s
        }
        GuideTarget { kind, .. } => vec![item("target", "选择对象", Ref(kind), "")],
        MoveTask => vec![
            item("task", "任务", Ref("task"), ""),
            item("target", "目标空间", Ref("$other_spaces"), ""),
        ],
        FindInView => vec![item("query", "搜索标题或任务编号", Text, "")],
        Create(k) if k == "space" => {
            let mut fields = record_specs(k);
            fields.splice(
                2..2,
                [
                    item(
                        "work_type",
                        "工作类型",
                        Choice(&[
                            ("software", "软件研发"),
                            ("project", "项目管理"),
                            ("other", "其他工作"),
                        ]),
                        "software",
                    ),
                    item(
                        "task_types",
                        "起始任务类型",
                        Many("$setup_types"),
                        "requirement, bug",
                    ),
                ],
            );
            fields
        }
        Create(k) => record_specs(k),
        Edit(r)
            if r.kind == "space"
                && r.fields.get("can_manage_content") == Some(&Value::Bool(false)) =>
        {
            record_specs("space")
                .into_iter()
                .filter(|s| matches!(s.key, "title" | "description"))
                .collect()
        }
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
        Workflow { op, record } => match *op {
            "chat.fork.workspace" => {
                let modes: &'static [(&'static str, &'static str)] =
                    match (record.fields["ready"] == true, record.fields["git"] == true) {
                        (true, true) => {
                            &[("reuse", "复用原对话目录"), ("branch", "在独立工作树继续")]
                        }
                        (true, false) => &[("reuse", "复用原对话目录")],
                        _ => &[("branch", "在独立工作树继续")],
                    };
                let mut fields = vec![item("workspace_mode", "工作目录策略", Choice(modes), "")];
                if record.fields["git"] == true {
                    fields.push(item("checkout_branch", "基线分支", Text, ""));
                    if record.fields["targets"]
                        .as_array()
                        .is_some_and(|items| !items.is_empty())
                    {
                        fields.push(item(
                            "target_machine_id",
                            "目标工作机",
                            Ref("$fork_machines"),
                            "",
                        ));
                    }
                }
                fields
            }
            "assistant.send" => vec![
                item("body", "消息", Body, ""),
                item("runtime", "Runtime", Ref("$assistant_runtimes"), ""),
                item("model", "模型（可选）", Text, ""),
            ],
            "chat.avatar.set" => vec![item(
                "path",
                "头像文件（PNG、JPEG、GIF 或 WebP，最大 2 MiB）",
                Text,
                "",
            )],
            "task_assistant.bind" | "task_assistant.configure" | "task_assistant.template.save" => {
                crate::app::task_assistant_ui::specs(op)
            }
            "status.library.save" => vec![
                item("title", "显示名称", Text, ""),
                item("category", "状态类别", Choice(STATUS), "todo"),
                item("zh", "简体中文名称", Text, ""),
                item("en", "英文名称", Text, ""),
                item("color", "状态颜色（#RRGGBB）", Text, ""),
                item("order", "显示顺序", Number, "0"),
                item("collapsed", "列表默认折叠", Bool, "false"),
                item("default", "组织默认状态", Bool, "false"),
            ],
            "status.library.removal" => vec![item(
                "target",
                "迁入组织状态",
                Ref("$org_status_targets"),
                "",
            )],
            "task_type.workflow.save" => vec![
                item(
                    "statuses",
                    "按流转顺序选择状态",
                    Many("$workflow_statuses"),
                    "",
                ),
                item(
                    "default_status",
                    "新任务默认状态",
                    Ref("$workflow_defaults"),
                    "",
                ),
            ],
            "iteration.configure" => vec![
                item(
                    "mode",
                    "迭代模式",
                    Choice(&[
                        ("manual", "手动"),
                        ("automatic", "自动节奏"),
                        ("disabled", "禁用迭代"),
                    ]),
                    "manual",
                ),
                item("timezone", "空间时区（IANA）", Text, "UTC"),
                item("anchor_date", "节奏起始日（YYYY-MM-DD）", Text, ""),
                item("weeks", "迭代时长（周）", Number, "2"),
                item("cooldown_days", "冷却天数", Number, "0"),
                item("future_count", "预创建的未来迭代数", Number, "2"),
                item(
                    "auto_join_active",
                    "自动加入活跃或有截止日期的任务",
                    Bool,
                    "false",
                ),
                item(
                    "auto_join_in_progress",
                    "自动加入进入进行中的任务",
                    Bool,
                    "false",
                ),
                item(
                    "auto_join_completed",
                    "自动加入本期完成的任务",
                    Bool,
                    "false",
                ),
            ],
            "chat.share.create" => vec![
                item(
                    "message_ids",
                    "已发送的消息与已完成的回复",
                    Many("$chat_messages"),
                    "",
                ),
                item(
                    "scope",
                    "谁可以查看",
                    Choice(&[("organization", "本组织成员"), ("link", "获得链接的任何人")]),
                    "organization",
                ),
            ],
            "chat.manage" => {
                let info = record
                    .fields
                    .get("conversation")
                    .cloned()
                    .unwrap_or_default();
                let choices: &'static [(&str, &str)] = if info["group"] == true {
                    &[
                        ("add", "添加成员"),
                        ("remove", "移出成员"),
                        ("transfer", "转让群主"),
                    ]
                } else if info["is_owner"] == true {
                    &[
                        ("invite", "邀请参与"),
                        ("revoke", "撤回授权"),
                        ("transfer", "转让对话"),
                    ]
                } else {
                    &[("invite", "邀请参与")]
                };
                vec![
                    item("action", "操作", Choice(choices), choices[0].0),
                    item("target", "成员", Ref("member"), ""),
                ]
            }
            "chat.agents" => vec![
                item(
                    "action",
                    "操作",
                    Choice(&[("add", "添加 Agent"), ("remove", "移出 Agent")]),
                    "add",
                ),
                item("target", "Agent", Ref("agent"), ""),
            ],
            "provider.preview" | "provider.ingest" => vec![
                item("payload", "平台原始事件（JSON 对象）", Json, "{}"),
                item("event_id", "GitLab 或 Jira 上游投递 ID", Text, ""),
            ],
            "zoho.preview" => vec![item("full", "重新检查全部缺陷", Bool, "false")],
            "zentao.preview" | "pingcode.preview" | "linear.preview" | "clickup.preview"
            | "ones.preview" | "teambition.preview" | "asana.preview" => vec![item(
                "import_mode",
                "导入方式",
                Choice(&[
                    ("history", "历史导入"),
                    ("changes", "检查增量变更"),
                    ("refresh", "重新检查全部条目"),
                ]),
                "history",
            )],
            "zentao.credential" => vec![
                item("account", "禅道账号", Text, ""),
                item("password", "禅道密码", Secret, ""),
                item("clear", "移除本机凭据", Bool, "false"),
            ],
            "pingcode.credential"
                if matches!(
                    record.field("pingcode_auth"),
                    "application" | "authorization"
                ) =>
            {
                let mut specs = vec![
                    item("client_id", "PingCode Client ID", Text, ""),
                    item("client_secret", "PingCode Client Secret", Secret, ""),
                ];
                if record.field("pingcode_auth") == "authorization" {
                    specs.push(item("code", "PingCode 授权码", Secret, ""));
                }
                specs.push(item("clear", "移除本机凭据", Bool, "false"));
                specs
            }
            "pingcode.credential" => vec![
                item("access_token", "PingCode 访问令牌", Secret, ""),
                item("clear", "移除本机凭据", Bool, "false"),
            ],
            "asana.accounts" => vec![item("access_token", "Asana 个人访问令牌", Secret, "")],
            "asana.credential" => vec![
                item("access_token", "Asana 个人访问令牌", Secret, ""),
                item("workspace_id", "Asana 工作区 GID", Text, ""),
                item("clear", "清除组织的 Asana 令牌", Bool, "false"),
            ],
            "asana.push" => vec![
                item("task_id", "已关联的任务", Ref("task"), ""),
                item(
                    "intent",
                    "回写内容",
                    Choice(&[("fields", "标题与描述"), ("comment", "新评论")]),
                    "fields",
                ),
                item("comment_id", "本地评论 ID", Text, ""),
                item("confirm_comment", "核对已发布的远端评论 ID", Text, ""),
            ],
            "teambition.application" => vec![
                item("app_id", "Teambition 应用 ID", Text, ""),
                item("app_secret", "Teambition 应用密钥", Secret, ""),
                item("clear", "清除组织的应用凭据", Bool, "false"),
            ],
            "teambition.authorize" => {
                vec![item("redirect_uri", "已注册的 OAuth 回调地址", Text, "")]
            }
            "teambition.personal" => vec![
                item("access_token", "Teambition 个人访问令牌", Secret, ""),
                item("code", "供应商 OAuth 授权码", Secret, ""),
                item("state", "OAuth 回调 state", Secret, ""),
                item("clear", "解除我的 Teambition 授权", Bool, "false"),
            ],
            "teambition.push" => vec![
                item("task_id", "已关联的任务", Ref("task"), ""),
                item(
                    "intent",
                    "回写内容",
                    Choice(&[
                        ("fields", "标题与描述"),
                        ("status", "任务状态"),
                        ("delete", "删除远端任务"),
                    ]),
                    "fields",
                ),
                item("confirm_external", "删除时确认的远端任务 ID", Text, ""),
                item("confirm_rev", "删除时确认的本地任务修订号", Number, ""),
            ],
            "cnb.credential" => vec![
                item("access_token", "CNB 个人访问令牌", Secret, ""),
                item("clear", "清除组织的组令牌", Bool, "false"),
            ],
            "cnb.pair" => vec![
                item("repository", "CNB 完整仓库路径", Text, ""),
                item("server_url", "ShipForge 团队服务地址", Text, ""),
                item("rotate", "轮换此仓库的流水线密钥", Bool, "false"),
            ],
            "linear.authorize" | "clickup.authorize" => vec![
                item("client_id", "OAuth Client ID", Text, ""),
                item("redirect_uri", "已注册的 OAuth 回调地址", Text, ""),
                item("write", "申请写入权限", Bool, "false"),
            ],
            "imports.sync_control" => vec![item(
                "action",
                "同步操作",
                Choice(&[("pause", "暂停同步"), ("resume", "恢复同步")]),
                if record.fields.get("sync_paused") == Some(&json!(true)) {
                    "resume"
                } else {
                    "pause"
                },
            )],
            "linear.accounts" | "clickup.accounts" | "ones.accounts" | "linear.credential"
            | "clickup.credential" | "ones.credential" => {
                let mut fields = vec![item("access_token", "供应商访问令牌", Secret, "")];
                if record.field("cloud_auth") == "oauth" {
                    fields.extend([
                        item("code", "供应商 OAuth 授权码", Secret, ""),
                        item("state", "OAuth 回调 state", Secret, ""),
                        item("client_secret", "OAuth Client Secret", Secret, ""),
                    ]);
                }
                if op.ends_with(".credential") {
                    fields.extend([
                        item("account_id", "选中的供应商账号 ID", Text, ""),
                        item("clear", "移除本机凭据", Bool, "false"),
                    ]);
                }
                fields
            }
            "imports.push" => {
                let mut specs = vec![item("task_id", "已关联的任务", Ref("task"), "")];
                if matches!(
                    record.field("provider"),
                    "pingcode" | "linear" | "clickup" | "ones"
                ) {
                    specs.extend([
                        item(
                            "intent",
                            "回写内容",
                            Choice(&[("status", "任务状态"), ("comment", "指定评论")]),
                            "status",
                        ),
                        item("comment_id", "需要发布的本地评论", Ref("comment"), ""),
                        item("confirm_comment", "待确认的远程评论 ID", Text, ""),
                    ]);
                }
                specs
            }
            "tapd.preview" => vec![item(
                "import_mode",
                "导入方式",
                Choice(&[
                    ("history", "补导历史，跳过已有条目"),
                    ("changes", "检查增量变更"),
                    ("refresh", "重新检查全部条目"),
                ]),
                "history",
            )],
            "tapd.credential" if record.field("tapd_auth") == "company" => vec![
                item("api_user", "TAPD API 账号", Text, ""),
                item("api_password", "TAPD API 密码", Secret, ""),
                item("clear", "移除本机凭据", Bool, "false"),
            ],
            "tapd.credential" => vec![
                item("access_token", "TAPD 个人访问令牌", Secret, ""),
                item("clear", "移除本机凭据", Bool, "false"),
            ],
            "zoho.credential" => vec![
                item("access_token", "Zoho 访问令牌", Secret, ""),
                item("clear", "移除本机凭据", Bool, "false"),
            ],
            "bugly.credentials" => vec![
                item("api_id", "Bugly 公司 API ID", Text, ""),
                item("api_key", "Bugly 公司 API Key", Secret, ""),
                item("product_key", "Bugly ProductKey", Secret, ""),
                item("clear", "移除本机凭据", Bool, "false"),
            ],
            "imports.commit" | "imports.rollback" => vec![item("batch", "导入批次 ID", Text, "")],
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
        Reauthenticate { .. } => vec![item("token", "访问令牌", Secret, "")],
        Filter => vec![
            item("filter_labels", "同时含有标签", Many("tag"), ""),
            item("query", "标题或编号", Text, ""),
            item("filter_status", "状态", Ref("$statuses"), ""),
            item("filter_priority", "优先级", Ref("$priorities"), ""),
            item("filter_assignee", "处理人", Ref("$handlers"), ""),
        ],
        Comment(task)
            if task.field("external_provider") == "asana"
                && task.field("visibility") != "private" =>
        {
            vec![
                item("body", "内容", Body, ""),
                item("publish_asana", "同时发到 Asana", Bool, "false"),
            ]
        }
        Comment(_) | ThreadReply { .. } => vec![item("body", "内容", Body, "")],
        Message(room) => {
            let mut fields = vec![
                item("body", "内容", Body, ""),
                item("mentions", "提及成员", Many("member"), ""),
            ];
            if room.field("chat_type") == "group" {
                fields.push(item("mention_all", "提及所有人", Bool, "false"));
            }
            fields
        }
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
        PersonalToken { .. } => vec![
            item("label", "用途名称", Text, ""),
            item(
                "lifetime",
                "有效期",
                Choice(&[
                    ("30d", "30 天"),
                    ("90d", "90 天"),
                    ("1y", "1 年"),
                    ("never", "永不过期"),
                ]),
                "30d",
            ),
        ],
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
pub enum Selection {
    One(String),
    Many(Vec<String>),
}
impl Selection {
    pub fn contains(&self, id: &str) -> bool {
        match self {
            Self::One(value) => value == id,
            Self::Many(values) => values.iter().any(|value| value == id),
        }
    }
    pub fn choose(&mut self, id: String) {
        match self {
            Self::One(value) => *value = id,
            Self::Many(values) if id.is_empty() => values.clear(),
            Self::Many(values) if values.contains(&id) => values.retain(|value| value != &id),
            Self::Many(values) => values.push(id),
        }
    }
}
#[derive(Clone)]
pub enum Control {
    Text(Field),
    Pick {
        value: Selection,
        focus: FocusHandle,
    },
}
#[derive(Clone)]
pub struct Editor {
    pub spec: Spec,
    pub control: Control,
    pub trigger: Option<u64>,
    pub bound: Option<crate::app::workspace_ui::BoundInput>,
}
impl Editor {
    pub(crate) fn new(
        spec: Spec,
        initial: &Map<String, Value>,
        trigger: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        let value = initial
            .get(spec.key)
            .map(|v| {
                if spec.format == Format::SecretJson && !v.is_null() {
                    serde_json::to_string(v).unwrap_or_default()
                } else if spec.format == Format::Json && !v.is_null() {
                    serde_json::to_string_pretty(v).unwrap_or_default()
                } else {
                    value_text(v)
                }
            })
            .unwrap_or_else(|| spec.default.to_owned());
        let control = match spec.format {
            Format::Choice(_) | Format::Ref(_) | Format::Many(_) | Format::Bool => Control::Pick {
                value: if matches!(spec.format, Format::Many(_)) {
                    let values =
                        if let Some(items) = initial.get(spec.key).and_then(Value::as_array) {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        } else {
                            value
                                .split([',', '，'])
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned)
                                .collect()
                        };
                    Selection::Many(values)
                } else {
                    Selection::One(value)
                },
                focus: cx.focus_handle().tab_stop(true),
            },
            _ => Control::Text(Field::new(
                &value,
                crate::locale::label(spec.label),
                matches!(spec.format, Format::Body | Format::Json),
                matches!(spec.format, Format::Secret | Format::SecretJson),
                window,
                cx,
            )),
        };
        Self {
            spec,
            control,
            trigger,
            bound: None,
        }
    }
    pub fn raw(&self, cx: &App) -> String {
        match &self.control {
            Control::Text(f) => f.value(cx),
            Control::Pick {
                value: Selection::One(value),
                ..
            } => value.clone(),
            Control::Pick {
                value: Selection::Many(values),
                ..
            } => values.join(", "),
        }
    }
    pub fn handle(&self, cx: &App) -> FocusHandle {
        match &self.control {
            Control::Text(f) => f.handle(cx),
            Control::Pick { focus, .. } => focus.clone(),
        }
    }
    pub(crate) fn parsed(&self, cx: &App) -> Result<Value, String> {
        use Format::*;
        if let Control::Pick {
            value: Selection::Many(values),
            ..
        } = &self.control
        {
            return Ok(json!(values));
        }
        let raw = self.raw(cx);
        let t = raw.trim();
        if self.bound.is_some() && t.is_empty() {
            return Ok(Value::Null);
        }
        Ok(match self.spec.format {
            Json | SecretJson => {
                if t.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_str(t)
                        .map_err(|e| format!("{} JSON 无效：{e}", self.spec.label))?
                }
            }
            Decimal if t.is_empty() => Value::Null,
            Decimal => t
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .ok_or_else(|| format!("{}需要有效数字", self.spec.label))?,
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
            Csv => json!(
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
            Many(_) => unreachable!("multi-select controls store an array"),
        })
    }
}
#[derive(Clone)]
pub struct TriggerDraft {
    pub id: u64,
    pub(crate) original: Map<String, Value>,
}
pub struct Form {
    pub request_id: uuid::Uuid,
    pub task_draft: Map<String, Value>,
    pub bound_task: bool,
    pub triggers: Vec<TriggerDraft>,
    next_trigger: u64,
    pub mode: Mode,
    pub entries: Vec<Editor>,
    pub open_pick: Option<usize>,
    pub picker_search: Option<Field>,
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
            request_id: uuid::Uuid::new_v4(),
            task_draft: initial
                .get("custom")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            bound_task: false,
            mode,
            entries,
            triggers: vec![],
            next_trigger: 0,
            open_pick: None,
            picker_search: None,
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
            .filter(|e| e.trigger.is_none() && e.bound.is_none())
            .map(|e| Ok((e.spec.key.to_owned(), e.parsed(cx)?)))
            .collect::<Result<Map<_, _>, String>>()?;
        if self.bound_task {
            let mut custom = Map::new();
            for e in &self.entries {
                if let Some(b) = &e.bound {
                    custom.insert(b.id.clone(), e.parsed(cx)?);
                }
            }
            fields.insert("custom".into(), Value::Object(custom));
        }
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
pub fn connector_field_visible(key: &str, provider: &str) -> bool {
    match key {
        "link_tasks" | "repository" => matches!(provider, "github" | "gitlab" | "cnb"),
        "review_rule_id" => matches!(provider, "github" | "gitlab"),
        "cnb_group" | "cnb_backlinks" => provider == "cnb",
        "vendor_delete" => provider == "teambition",
        "repository_host" | "project_id" => provider == "gitlab",
        "project_key" => provider == "jira",
        "portal_id" | "zoho_project" => provider == "zoho",
        "instance_url" | "scope_kind" | "zentao_edition" | "zentao_version" => provider == "zentao",
        "enterprise_id" => matches!(
            provider,
            "pingcode" | "linear" | "clickup" | "ones" | "teambition" | "asana"
        ),
        "pingcode_auth" => provider == "pingcode",
        "ones_url" => provider == "ones",
        "cloud_auth" => matches!(provider, "linear" | "clickup"),
        "remote_scope" | "vendor_write" => matches!(
            provider,
            "zentao" | "pingcode" | "linear" | "clickup" | "ones" | "teambition" | "asana"
        ),
        "company_id" | "tapd_auth" | "tapd_workspace" => provider == "tapd",
        "product_id" | "default_task_type" | "bugly_outbound" => provider == "bugly",
        "workspace_id" | "channel_id" | "app_id" | "bot_id" | "outbound" | "slack_assignment" => {
            provider == "slack"
        }
        "chat_id" => matches!(
            provider,
            "telegram" | "feishu" | "lark" | "teams" | "dingtalk"
        ),
        _ => true,
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
    if r.kind == "agent" {
        crate::app::runtime_ui::initial(&mut f);
    }
    if r.kind == "project" && !r.field("owner").is_empty() {
        f.insert(
            "project_owner".into(),
            json!(format!(
                "{}/{}",
                r.fields
                    .get("owner_kind")
                    .and_then(Value::as_str)
                    .unwrap_or("member"),
                r.field("owner")
            )),
        );
    }
    if r.kind == "task_type"
        && let Some(bindings) = r.fields.get("field_bindings").and_then(Value::as_array)
    {
        f.insert("use_bindings".into(), json!(true));
        for (key, flag) in [
            ("bound_ids", "all"),
            ("disabled_ids", "disabled"),
            ("required_fields", "required"),
        ] {
            f.insert(
                key.into(),
                json!(
                    bindings
                        .iter()
                        .filter(|b| flag == "all"
                            || flag == "disabled" && b["enabled"] == false
                            || flag == "required" && b["required"] == true)
                        .map(|b| &b["field"])
                        .collect::<Vec<_>>()
                ),
            );
        }
    }
    if r.kind == "task" {
        crate::assignees::initial(r, &mut f);
    }
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
    creating: bool,
) -> Result<(String, Map<String, Value>), String> {
    let title = fields
        .remove("title")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    if title.trim().is_empty() {
        return Err("请填写名称或标题。".into());
    }
    if kind == "task_type" {
        let explicit = fields.remove("use_bindings") == Some(json!(true));
        let bound = fields
            .remove("bound_ids")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let hidden = fields
            .remove("disabled_ids")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        if explicit {
            let required = fields
                .remove("required_fields")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            if hidden.iter().chain(&required).any(|id| !bound.contains(id)) {
                return Err("必填和隐藏字段须先加入绑定字段。".into());
            }
            fields.insert("field_bindings".into(),json!(bound.iter().map(|id|json!({"field":id,"enabled":!hidden.contains(id),"required":required.contains(id)})).collect::<Vec<_>>()));
        }
    }
    if kind == "agent" {
        crate::app::runtime_ui::compile(&mut fields)?;
    }
    if kind == "connector" {
        let provider = fields
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        fields.retain(|key, value| {
            connector_field_visible(key, &provider) && !(creating && value.as_str() == Some(""))
        });
        if !matches!(provider.as_str(), "github" | "gitlab") {
            fields.insert("link_tasks".into(), json!(false));
        }
        if provider != "slack" {
            fields.insert("outbound".into(), json!(false));
        }
    }
    if kind == "project" {
        let selected = fields.remove("project_owner");
        if let Some((kind, id)) = selected
            .as_ref()
            .and_then(Value::as_str)
            .and_then(|s| s.split_once('/'))
        {
            fields.insert("owner".into(), json!(id));
            fields.insert("owner_kind".into(), json!(kind));
        } else {
            fields.insert("owner".into(), Value::Null);
            fields.remove("owner_kind");
        }
    }
    for key in [
        "due_date",
        "start_date",
        "end_date",
        "planned_start",
        "planned_end",
    ] {
        if fields.get(key).and_then(Value::as_str) == Some("") {
            fields.insert(key.into(), Value::Null);
        }
    }
    if kind == "task" {
        crate::assignees::compile(&mut fields)?;
        if fields.get("owner").is_none_or(Value::is_null) {
            fields.remove("owner");
        }
    }
    if kind == "view" {
        let mut filter = Map::new();
        for key in [
            "type", "status", "priority", "assignee", "project", "labels",
        ] {
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
