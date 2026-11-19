use super::*;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Default, Serialize)]
pub(super) struct Usage {
    pub tasks: usize,
    pub workflows: usize,
    pub spaces: usize,
}
fn explicit(ty: &Record, source: &str) -> bool {
    ty.fields["default_status"] == source
        || ty.fields["statuses"]
            .as_array()
            .is_some_and(|l| l.iter().any(|v| v == source))
}
pub(super) fn catalog_usage(root: &Context<'_>, all: bool) -> Result<BTreeMap<String, Usage>> {
    let mut by_space: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for r in root.records("status")? {
        by_space
            .entry(root.space.to_owned())
            .or_default()
            .insert(r.id.clone(), r.id);
    }
    let mut stmt = root
        .db
        .prepare("SELECT space,local,source FROM sf_status_link WHERE org=?")
        .map_err(storage)?;
    let links = stmt
        .query_map([root.space], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?;
    for row in links {
        let (space, local, source) = row.map_err(storage)?;
        by_space.entry(space).or_default().insert(local, source);
    }
    let mut counts: BTreeMap<String, Usage> = BTreeMap::new();
    for (space, keys) in by_space {
        let c = Context {
            space: &space,
            ..*root
        };
        if !all && c.role().is_err() {
            continue;
        }
        let mut used = BTreeSet::new();
        for task in c.records("task")? {
            if let Some(source) = task.fields["status"].as_str().and_then(|id| keys.get(id))
                && (all || c.visible(&task)?)
            {
                counts.entry(source.clone()).or_default().tasks += 1;
                used.insert(source);
            }
        }
        for ty in c.records("task_type")? {
            let selected = ty.fields["statuses"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(ty.fields.get("default_status"))
                .filter_map(Value::as_str)
                .filter_map(|id| keys.get(id))
                .collect::<BTreeSet<_>>();
            for source in selected {
                counts.entry(source.clone()).or_default().workflows += 1;
                used.insert(source);
            }
        }
        for source in used {
            counts.entry(source.clone()).or_default().spaces += 1;
        }
    }
    Ok(counts)
}
pub(super) fn usage(root: &Context<'_>, source: &str, all: bool) -> Result<Usage> {
    let mut counts = Usage::default();
    for (space, local) in linked_spaces(root, source)? {
        let c = Context {
            space: &space,
            ..*root
        };
        if !all && c.role().is_err() {
            continue;
        }
        let mut active = false;
        for task in c
            .records("task")?
            .into_iter()
            .filter(|r| r.fields["status"] == local)
        {
            if all || c.visible(&task)? {
                counts.tasks += 1;
                active = true;
            }
        }
        let types = c
            .records("task_type")?
            .iter()
            .filter(|r| explicit(r, &local))
            .count();
        counts.workflows += types;
        counts.spaces += usize::from(active || types > 0);
    }
    Ok(counts)
}
struct Segment {
    row: Record,
    tasks: Vec<Record>,
    types: Vec<Record>,
}
fn ordered(rows: &mut [Record]) {
    rows.sort_by(|a, b| a.id.cmp(&b.id));
}
fn signature(rows: &[Record]) -> Vec<(&str, i64)> {
    rows.iter().map(|r| (r.id.as_str(), r.rev)).collect()
}
pub(super) fn execute(root: &Context<'_>, q: &Value, apply: bool) -> Result<Value> {
    let source = root.readable("status", required(q, "id")?)?;
    domain::check_rev(q, &source)?;
    let target = q["target"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| root.readable("status", s))
        .transpose()?;
    if target.as_ref().is_some_and(|r| r.id == source.id) {
        return Err(invalid("choose a different destination status"));
    }
    let mut segments = Vec::new();
    let mut versions = Vec::new();
    let mut count = Usage::default();
    let mut default_used = false;
    let mut triage_disabled = false;
    for (space, local) in linked_spaces(root, &source.id)? {
        let c = Context {
            space: &space,
            ..*root
        };
        let row = c.get("status", &local)?;
        let mut tasks = c
            .records("task")?
            .into_iter()
            .filter(|r| r.fields["status"] == local)
            .collect::<Vec<_>>();
        let mut types = c
            .records("task_type")?
            .into_iter()
            .filter(|r| explicit(r, &local))
            .collect::<Vec<_>>();
        ordered(&mut tasks);
        ordered(&mut types);
        count.tasks += tasks.len();
        count.workflows += types.len();
        count.spaces += usize::from(!tasks.is_empty() || !types.is_empty());
        default_used |= row.fields["default"] == true
            || types.iter().any(|t| {
                t.fields["default_status"] == local
                    || workflow::default_id(&c, Some(t)).as_deref() == Ok(local.as_str())
            });
        if target
            .as_ref()
            .is_some_and(|dest| dest.fields["category"] == "triage")
            && (!tasks.is_empty()
                || row.fields["default"] == true
                || types
                    .iter()
                    .any(|ty| workflow::default_id(&c, Some(ty)).as_deref() == Ok(local.as_str())))
            && c.get("space", c.space)?.fields["triage_enabled"] != true
        {
            triage_disabled = true;
        }
        let destination = if let Some(dest) = target.as_ref() {
            let mapped = local_id(&c, root.space, &dest.id)?;
            let row = c.find("status", mapped.as_deref().unwrap_or(&dest.id))?;
            json!([mapped, row.as_ref().map(|r| (&r.id, r.rev))])
        } else {
            Value::Null
        };
        versions.push(json!([
            space,
            local,
            row.rev,
            destination,
            signature(&tasks),
            signature(&types)
        ]));
        segments.push(Segment { row, tasks, types });
    }
    let category_count = root
        .records("status")?
        .iter()
        .filter(|r| r.fields["category"] == source.fields["category"])
        .count();
    let need_target = count.tasks > 0 || count.workflows > 0 || default_used;
    let reason = if source.fields["default"] == true {
        Some("organization_default")
    } else if category_count <= 1 {
        Some("last_in_category")
    } else if need_target && target.is_none() {
        Some("target_required")
    } else if triage_disabled {
        Some("triage_disabled")
    } else if default_used
        && target
            .as_ref()
            .is_some_and(|r| r.fields["category"].as_str().is_some_and(terminal))
    {
        Some("terminal_default")
    } else {
        None
    };
    let token = crate::worker::digest(
        &json!([
            root.space,
            source.id,
            source.rev,
            target.as_ref().map(|r| (&r.id, r.rev)),
            versions,
            category_count,
            reason
        ])
        .to_string(),
    );
    let report = json!({"id":source.id,"target":target.as_ref().map(|r|&r.id),"rev":source.rev,"usage":count,"can_remove":reason.is_none(),"reason":reason,"requires_target":need_target,"preview":token});
    if !apply {
        return Ok(report);
    }
    if q.get("preview").and_then(Value::as_str) != Some(&token) {
        return Err(conflict(
            "status usage changed or preview is missing. Preview removal again",
        ));
    }
    if let Some(reason) = reason {
        return Err(conflict(&format!("cannot remove status: {reason}")));
    }
    for segment in segments {
        let c = Context {
            space: &segment.row.space,
            ..*root
        };
        let from = &segment.row.id;
        if !segment.tasks.is_empty()
            || !segment.types.is_empty()
            || segment.row.fields["default"] == true
        {
            let target = target
                .as_ref()
                .ok_or_else(|| invalid("a migration target is required"))?;
            let local = attach(&c, target)?;
            for ty in segment.types {
                let mut f = ty.fields.clone();
                if let Some(current) = f["statuses"].as_array_mut() {
                    let mut seen = BTreeSet::new();
                    current.retain_mut(|v| {
                        if v == from {
                            *v = json!(local);
                        }
                        seen.insert(v.as_str().unwrap_or_default().to_owned())
                    });
                }
                if f["default_status"] == *from
                    || workflow::default_id(&c, Some(&ty)).as_deref() == Ok(from.as_str())
                {
                    f["default_status"] = json!(local);
                }
                retained::revise(&c, &ty, &ty.title, f)?;
            }
            for task in segment.tasks {
                let mut fields = task.fields.clone();
                fields["status"] = json!(local);
                let mut next = Record {
                    fields,
                    ..task.clone()
                };
                let active = !retained::inactive(&c)?;
                if active {
                    crate::triage::stamp(&c, Some(&task), &mut next)?;
                }
                let next = retained::revise(&c, &task, &task.title, next.fields)?;
                if active {
                    crate::iterations::supplement(&c, Some(&task), &next)?;
                    crate::worker::changed(&c, Some(&task), &next)?;
                }
            }
            if segment.row.fields["default"] == true {
                set_default(&c, &local)?;
            }
        }
        let current = c.get("status", from)?;
        retained::remove(&c, &current)?;
    }
    if domain::BASE_STATUS.contains(&source.id.as_str()) {
        root.db
            .execute(
                "INSERT OR IGNORE INTO sf_retired_status(org,id) VALUES(?,?)",
                params![root.space, source.id],
            )
            .map_err(storage)?;
    }
    let mut report = report;
    report["removed"] = json!(true);
    Ok(report)
}
