use super::*;
use crate::domain::strings;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

fn important(row: &Record) -> bool {
    row.fields["importance"] == "important" || row.fields["important"] == true
}
fn keys(q: &Value, key: &str) -> Result<Option<Vec<String>>> {
    q.get(key)
        .map(|v| strings(v, key).map(|items| items.into_iter().map(str::to_owned).collect()))
        .transpose()
}
fn search(c: &Context<'_>, r: &Record, needle: &str) -> Result<bool> {
    if needle.is_empty() || r.title.to_lowercase().contains(needle) {
        return Ok(true);
    }
    let mut stmt=c.db.prepare("SELECT source_kind,source_id FROM inbox_signals WHERE space=?1 AND actor=?2 AND notice=?3 AND source_kind IN ('comment','message') ORDER BY seq DESC").map_err(storage)?;
    let mut sources = stmt
        .query_map(params![c.space, c.actor, r.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    if let Some(id) = r.fields["message"].as_str() {
        sources.push(("message".into(), id.into()));
    } else if let Some(id) = r.fields["latest"].as_str() {
        sources.push(("comment".into(), id.into()));
    }
    for (kind, id) in sources {
        if let Ok(source) = c.readable(&kind, &id)
            && (source.title.to_lowercase().contains(needle)
                || source.fields["body"]
                    .as_str()
                    .is_some_and(|body| body.to_lowercase().contains(needle)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn query(c: &Context<'_>, q: &Value, write: bool) -> Result<Value> {
    let org = c.org_id()?;
    let cfg = settings::Settings::load(c, c.actor)?;
    let section = q["section"]
        .as_str()
        .unwrap_or(if cfg.landing == "important" {
            "important"
        } else {
            "all"
        });
    if !matches!(
        section,
        "important" | "other" | "all" | "snoozed" | "cleared"
    ) {
        return Err(invalid("unknown inbox section"));
    }
    let include = keys(q, "include_spaces")?;
    let exclude = keys(q, "exclude_spaces")?.unwrap_or_default();
    let statuses = keys(q, "statuses")?;
    let search_term = q["query"].as_str().unwrap_or("").trim().to_lowercase();
    let mut rows = Vec::new();
    let mut counts = [0u64; 2];
    for space in c.all_spaces()? {
        if space.fields["organization"].as_str().unwrap_or("default") != org
            || exclude.contains(&space.id)
            || include.as_ref().is_some_and(|s| !s.contains(&space.id))
        {
            continue;
        }
        let scope = Context {
            space: &space.id,
            ..*c
        };
        if scope.role().is_err() {
            continue;
        }
        for r in crate::query::visible_records(&scope, "notification")? {
            if !search(&scope, &r, &search_term)? {
                continue;
            }
            if let Some(ref statuses) = statuses {
                let Some(id) = r.fields["task"].as_str() else {
                    continue;
                };
                let task = scope.readable("task", id)?;
                if !statuses.iter().any(|s| {
                    task.fields["status"] == *s
                        || scope.status_category(&task).as_ref().ok() == Some(s)
                }) {
                    continue;
                }
            }
            let strong = important(&r);
            let state = r.fields["state"].as_str().unwrap_or("inbox");
            let unread = r.fields["read"] != true;
            if state == "inbox" && unread {
                counts[usize::from(strong)] += 1;
            }
            let selected = match section {
                "important" => state == "inbox" && strong,
                "other" => state == "inbox" && !strong,
                "all" => state == "inbox",
                other => state == other,
            };
            if selected && (q["unread"] != true || unread) {
                rows.push(r);
            }
        }
    }
    rows.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.space.cmp(&b.space))
            .then_with(|| a.id.cmp(&b.id))
    });
    if write {
        let action = required(q, "action")?;
        if !matches!(action, "read" | "clear") {
            return Err(invalid("bulk action must be read or clear"));
        }
        let mut changed = 0;
        for notice in rows {
            if (action == "read" && notice.fields["read"] == true)
                || (action == "clear" && notice.fields["state"] == "cleared")
            {
                continue;
            }
            let scope = Context {
                space: &notice.space,
                ..*c
            };
            super::super::actions::notification(&scope, &json!({"id":notice.id,"action":action}))?;
            changed += 1;
        }
        if action == "clear" {
            let mut cfg = cfg;
            cfg.remember_cleanup(c, statuses)?;
        }
        return Ok(json!({"changed":changed}));
    }
    let total = rows.len();
    let offset = q["offset"].as_u64().unwrap_or(0) as usize;
    let limit = q["limit"].as_u64().unwrap_or(500).clamp(1, 5000) as usize;
    Ok(
        json!({"section":section,"rows":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":total,"unread":{"all":counts[0]+counts[1],"important":counts[1],"other":counts[0]},"clear_statuses":cfg.clear_statuses}),
    )
}
pub(super) fn open(c: &Context<'_>, q: &Value) -> Result<Value> {
    if let Some(origin) = q.get("origin")
        && origin.as_str() != Some(crate::saved_views::workspace(c)?.as_str())
    {
        return Err(forbidden(
            "connect to the workspace that issued this notification",
        ));
    }
    let row = c.readable("notification", required(q, "id")?)?;
    let mut target = None;
    for (field, kind) in [
        ("chat", "chat"),
        ("automation", "automation"),
        ("task", "task"),
        ("run", "run"),
    ] {
        if let Some(id) = row.fields[field].as_str()
            && let Ok(r) = c.readable(kind, id)
        {
            target = Some(r);
            break;
        }
    }
    let target = target.ok_or_else(|| invalid("notification target is unavailable"))?;
    let comment = row.fields["source_id"]
        .as_str()
        .or(row.fields["latest"].as_str())
        .and_then(|id| c.readable("comment", id).ok())
        .filter(|r| r.fields["task"] == target.id);
    if q["op"] == "inbox.open" && row.fields["read"] != true {
        crate::actions::notification(c, &json!({"id":row.id,"action":"read"}))?;
    }
    Ok(json!({"target":target,"comment":comment.map(|r|r.id),"space":c.space}))
}
pub(super) fn feed(c: &Context<'_>, q: &Value) -> Result<Value> {
    let org = c.org_id()?;
    let max:i64=c.db.query_row("SELECT COALESCE(MAX(seq),0) FROM inbox_signals WHERE org=?1 AND actor=?2 AND desktop=1",params![org,c.actor],|r|r.get(0)).map_err(storage)?;
    let Some(after) = q.get("after") else {
        return Ok(json!({"cursor":max,"items":[]}));
    };
    let mut cursor = after
        .as_i64()
        .filter(|s| *s >= 0)
        .ok_or_else(|| invalid("after must be a nonnegative cursor"))?;
    let mut stmt=c.db.prepare("SELECT seq,space,notice,event,source_kind,source_id FROM inbox_signals WHERE org=?1 AND actor=?2 AND desktop=1 AND seq>?3 ORDER BY seq LIMIT 100").map_err(storage)?;
    let events = stmt
        .query_map(params![org, c.actor, cursor], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(storage)?;
    let cfg = settings::Settings::load(c, c.actor)?;
    let mut items = Vec::new();
    let origin = crate::saved_views::workspace(c)?;
    for event in events {
        let (seq, space, id, event, kind, source) = event.map_err(storage)?;
        cursor = seq;
        let scope = Context {
            space: &space,
            ..*c
        };
        if !cfg.enabled("desktop", &event) || scope.role().is_err() {
            continue;
        }
        if let (Some(k), Some(id)) = (kind, source)
            && scope.readable(&k, &id).is_err()
        {
            continue;
        }
        if let Ok(row) = scope.readable("notification", &id)
            && row.fields["read"] != true
            && row.fields["state"] != "cleared"
        {
            let link = format!(
                "shipforge://inbox/{origin}/{}/{}",
                URL_SAFE_NO_PAD.encode(&space),
                URL_SAFE_NO_PAD.encode(&id)
            );
            items.push(json!({"seq":seq,"id":id,"space":space,"title":row.title,"event":event,"link":link}));
        }
    }
    Ok(json!({"cursor":cursor,"items":items}))
}
