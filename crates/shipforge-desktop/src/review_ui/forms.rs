use super::*;
use crate::forms::{Format, Spec, item};
#[derive(Clone)]
pub enum ReviewForm {
    Submit(String),
    Config(Value),
    Rule {
        settings: Value,
        index: Option<usize>,
    },
    Decide {
        rows: Vec<Record>,
        choice: &'static str,
    },
    OpenTask,
}
impl ReviewForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Submit(_) => "提交到分拣台",
            Self::Config(_) => "分拣设置",
            Self::Rule { .. } => "分拣规则",
            Self::Decide { choice, .. } => decision_label(choice),
            Self::OpenTask => "打开任务链接",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        use Format::*;
        match self {
            Self::OpenTask => vec![item("link", "任务链接", Text, "")],
            Self::Submit(_) => vec![
                item("title", "任务标题", Text, ""),
                item("description", "描述", Body, ""),
                item("type", "类型标识（留空使用空间默认值）", Text, ""),
                item("custom", "类型字段值", Json, "{}"),
            ],
            Self::Config(_) => vec![
                item("enabled", "启用分拣台", Bool, "true"),
                item("priority_required", "接受前必须设置优先级", Bool, "false"),
                item("sources", "自动收件来源", Many("$sources"), ""),
                item("types", "适用任务类型（空选代表全部）", Many("$types"), ""),
                item(
                    "duty_mode",
                    "分拣责任",
                    Choice(&[
                        ("none", "无"),
                        ("notify", "通知指定成员"),
                        ("assign", "自动指派指定成员"),
                    ]),
                    "none",
                ),
                item("reviewer", "值班成员", Ref("member"), ""),
            ],
            Self::Rule { .. } => vec![
                item("title", "规则名称", Text, ""),
                item("enabled", "启用规则", Bool, "true"),
                item("match_type", "匹配任务类型", Ref("$types"), ""),
                item("match_content", "标题或描述包含", Text, ""),
                item("match_priority", "匹配优先级", Choice(PRIORITY_FILTER), ""),
                item("match_actor", "匹配创建者", Ref("member"), ""),
                item("match_assignee", "匹配处理人", Ref("member"), ""),
                item(
                    "match_source",
                    "匹配收件来源",
                    Choice(&[
                        ("", "全部来源"),
                        ("guest", "访客"),
                        ("organization", "未加入的组织成员"),
                        ("member", "空间成员"),
                        ("integration", "集成"),
                        ("customer_request", "客户请求"),
                    ]),
                    "",
                ),
                item("match_labels", "必须包含标签", Many("$tag_choices"), ""),
                item("set_space", "移动到空间", Ref("space"), ""),
                item("set_priority", "设置优先级", Choice(PRIORITY_FILTER), ""),
                item("set_status", "转到状态", Ref("$statuses"), ""),
                item("set_assignee", "指派成员", Ref("member"), ""),
                item("set_project", "设置专项", Ref("project"), ""),
                item("set_labels", "设置标签", Many("$tag_choices"), ""),
                item("when", "其他匹配条件（JSON 对象）", Json, "{}"),
                item("set", "其他动作（JSON 对象）", Json, "{}"),
            ],
            Self::Decide {
                choice: "accept", ..
            } => vec![
                item(
                    "priority",
                    "设置优先级",
                    Choice(&[
                        ("", "保留当前设置"),
                        ("none", "无"),
                        ("low", "低"),
                        ("medium", "中"),
                        ("high", "高"),
                        ("urgent", "紧急"),
                    ]),
                    "",
                ),
                item("assignees", "指派给", Many("member"), ""),
                item("project", "加入专项", Ref("project"), ""),
                item("labels", "设置标签", Many("$tag_choices"), ""),
                item("start_date", "开始日期（YYYY-MM-DD）", Text, ""),
                item("due_date", "截止日期（YYYY-MM-DD）", Text, ""),
                item("comment", "接受说明", Body, ""),
                item("subscribe", "关注这些任务", Bool, "false"),
            ],
            Self::Decide {
                choice: "duplicate",
                ..
            } => vec![item("target", "原任务编号、标识或任务链接", Text, "")],
            Self::Decide {
                choice: "defer", ..
            } => vec![
                item(
                    "delay",
                    "返回队列的时间",
                    Choice(&[
                        ("1", "一小时后"),
                        ("24", "一天后"),
                        ("168", "一周后"),
                        ("custom", "指定本地时间"),
                    ]),
                    "24",
                ),
                item("time", "指定时间（YYYY-MM-DD HH:mm）", Text, ""),
            ],
            Self::Decide {
                choice: "reject", ..
            } => vec![item("reason", "拒绝原因", Body, "")],
            _ => vec![],
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        match self {
            Self::Config(s)=>json!({"enabled":s["enabled"],"priority_required":s["priority_required"],"sources":s["sources"].as_array().cloned().unwrap_or_else(||vec![json!("guest"),json!("external")]),"types":s["types"],"duty_mode":s["duty"]["mode"],"reviewer":s["duty"]["member"]}).as_object().unwrap().clone(),
            Self::Rule{settings,index:Some(i)}=>{
                let rule=&settings["rules"][i];
                let mut form=rule.clone();
                for (input,section,field) in RULE_INPUTS {
                    if let Some(v)=form[*section].as_object_mut().and_then(|m|m.remove(*field)){if v.is_null()||v.as_array().is_some_and(|a|a.is_empty()){form[*section][*field]=v;}else{form[*input]=v;}}
                }
                form.as_object().cloned().unwrap_or_default()
            },
            _=>Map::new(),
        }
    }
}
const PRIORITY_FILTER: &[(&str, &str)] = &[
    ("", "留空"),
    ("none", "无"),
    ("low", "低"),
    ("medium", "中"),
    ("high", "高"),
    ("urgent", "紧急"),
];
pub(super) const RULE_INPUTS: &[(&str, &str, &str)] = &[
    ("match_type", "when", "type"),
    ("match_content", "when", "contains"),
    ("match_priority", "when", "priority"),
    ("match_actor", "when", "creator"),
    ("match_assignee", "when", "assignee"),
    ("match_source", "when", "source"),
    ("match_labels", "when", "labels"),
    ("set_space", "set", "space"),
    ("set_priority", "set", "priority"),
    ("set_status", "set", "status"),
    ("set_assignee", "set", "assignee"),
    ("set_project", "set", "project"),
    ("set_labels", "set", "labels"),
];
