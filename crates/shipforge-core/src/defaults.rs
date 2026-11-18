use crate::{Context, Result, json, storage};

const STATUSES: &[(&str, &str)] = &[
    ("triage", "待分拣"),
    ("backlog", "待规划"),
    ("todo", "待处理"),
    ("in_progress", "进行中"),
    ("done", "已完成"),
    ("cancelled", "已取消"),
];
const TYPES: &[(&str, &str)] = &[("task", "任务"), ("requirement", "需求"), ("bug", "缺陷")];
const RELATIONS: &[(&str, &str, &str)] = &[
    ("related", "关联", "关联"),
    ("blocks", "阻塞", "被阻塞"),
    ("duplicate", "重复于", "重复项"),
    ("parent", "父任务", "子任务"),
];

pub(crate) fn builtin(kind: &str, id: &str) -> bool {
    match kind {
        "status" => STATUSES.iter().any(|(k, _)| *k == id),
        "task_type" => TYPES.iter().any(|(k, _)| *k == id),
        "relation_type" => RELATIONS.iter().any(|(k, _, _)| *k == id),
        _ => false,
    }
}
pub(crate) fn seed(c: &Context<'_>) -> Result<()> {
    for (order, (id, title)) in STATUSES.iter().enumerate() {
        if c.find("status", id)?.is_none() {
            c.create("status",Some(id),title,json!({"creator":c.actor,"system":true,"category":id,"order":order,"default":*id=="todo"}))?;
        }
    }
    for (order, (id, title)) in TYPES.iter().enumerate() {
        if c.find("task_type", id)?.is_none() {
            c.create("task_type",Some(id),title,json!({"creator":c.actor,"system":true,"statuses":[],"required_fields":[],"order":order,"default":*id=="task"}))?;
        }
    }
    for (order, (id, forward, reverse)) in RELATIONS.iter().enumerate() {
        if c.find("relation_type", id)?.is_none() {
            c.create("relation_type",Some(id),forward,json!({"creator":c.actor,"system":true,"forward":forward,"reverse":reverse,"order":order}))?;
        }
    }
    c.db.execute(
        "INSERT OR IGNORE INTO space_defaults(space) VALUES (?)",
        [c.space],
    )
    .map_err(storage)?;
    Ok(())
}
pub(crate) fn selected(c: &Context<'_>, kind: &str, fallback: &str) -> Result<String> {
    Ok(c.records(kind)?
        .into_iter()
        .find(|r| r.fields["default"] == true)
        .map(|r| r.id)
        .unwrap_or_else(|| fallback.to_owned()))
}
