use crate::{
    Context, Record, Result, conflict, domain, forbidden, identifier, invalid, required, storage,
};
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

const COLORS: [&str; 8] = [
    "#5B78D4", "#368E81", "#BB7147", "#8963BA", "#C36788", "#417DAB", "#838644", "#758399",
];
pub(crate) fn upgrade(db: &rusqlite::Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS label_migrations(version INTEGER PRIMARY KEY)")
        .map_err(storage)?;
    if db
        .execute("INSERT OR IGNORE INTO label_migrations VALUES(1)", [])
        .map_err(storage)?
        == 0
    {
        return Ok(());
    }
    let mut query=db.prepare("SELECT DISTINCT t.space,token.value FROM records t,json_each(t.fields,'$.labels') token WHERE t.kind='task' AND token.type='text'").map_err(storage)?;
    let legacy = query
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    drop(query);
    let mut spaces: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (space, name) in legacy {
        spaces.entry(space).or_default().push(name);
    }
    for (space, names) in spaces {
        let c = Context {
            db,
            space: &space,
            actor: "owner",
        };
        let mut catalog_ids = HashSet::new();
        let mut titles = HashMap::new();
        for tag in catalog(&c, "tag")? {
            catalog_ids.insert(tag.id.clone());
            titles.entry(tag.title.to_lowercase()).or_insert(tag.id);
        }
        let mut replacements = HashMap::new();
        for name in names {
            if name.trim().is_empty() || name.chars().count() > 500 || catalog_ids.contains(&name) {
                continue;
            }
            let folded = name.to_lowercase();
            let id = if let Some(id) = titles.get(&folded) {
                id.clone()
            } else {
                let id = id_for_title(&c, &name)?;
                c.create("tag",Some(&id),&name,json!({"scope":"space","color":default_color(&name),"archived":false,"imported_label":true}))?;
                titles.insert(folded, id.clone());
                catalog_ids.insert(id.clone());
                id
            };
            if name != id {
                replacements.insert(name, id);
            }
        }
        if !replacements.is_empty() {
            for kind in ["task", "view"] {
                for row in c.records(kind)? {
                    let mut fields = row.fields.clone();
                    if kind == "task" {
                        if let Some(labels) = fields.get_mut("labels") {
                            migrate_filter(labels, &replacements);
                        }
                    } else if let Some(filter) = fields.get_mut("filter") {
                        for key in ["labels", "labels_not"] {
                            if let Some(labels) = filter.get_mut(key) {
                                migrate_filter(labels, &replacements);
                            }
                        }
                    }
                    if fields != row.fields {
                        // Only the storage key changes. Keep the user's edit history, timestamps and onboarding credits.
                        db.execute(
                            "UPDATE records SET fields=?4 WHERE space=?1 AND kind=?2 AND id=?3",
                            params![space, kind, row.id, fields.to_string()],
                        )
                        .map_err(storage)?;
                    }
                }
            }
        }
    }
    Ok(())
}
fn migrate_filter(value: &mut Value, aliases: &HashMap<String, String>) {
    if let Some(items) = value.as_array_mut() {
        for item in items.iter_mut() {
            if let Some(id) = item.as_str().and_then(|s| aliases.get(s)) {
                *item = json!(id);
            }
        }
        let mut seen = HashSet::new();
        items.retain(|v| !v.is_string() || seen.insert(v.clone()));
    } else if let Some(id) = value.as_str().and_then(|s| aliases.get(s)) {
        *value = json!(id);
    }
}
fn is_org(row: &Record) -> bool {
    row.fields["scope"] == "organization"
}
fn group_id(row: &Record) -> Option<&str> {
    row.fields["group"].as_str().filter(|s| !s.is_empty())
}
fn default_color(id: &str) -> &'static str {
    let index = id.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as usize)
    });
    COLORS[index % COLORS.len()]
}
fn id_for_title(c: &Context<'_>, title: &str) -> Result<String> {
    if identifier(title).is_ok() && c.find("tag", title)?.is_none() {
        let retired:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM event_log WHERE space=?1 AND kind='tag' AND id=?2 AND action='delete')",
            params![c.space,title],|r|r.get(0)).map_err(storage)?;
        if !retired {
            return Ok(title.to_owned());
        }
    }
    Ok(uuid::Uuid::new_v4().to_string())
}
pub(crate) fn catalog(c: &Context<'_>, kind: &str) -> Result<Vec<Record>> {
    let mut rows = Vec::new();
    for row in c.records(kind)? {
        if c.visible(&row)? {
            rows.push(row);
        }
    }
    let org = c.org_id()?;
    if org != c.space {
        let anchor = Context { space: &org, ..*c };
        for row in anchor.records(kind)?.into_iter().filter(is_org) {
            if anchor.visible(&row)? {
                rows.push(row);
            }
        }
    }
    rows.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
    Ok(rows)
}
fn find(c: &Context<'_>, kind: &str, id: &str) -> Result<Option<Record>> {
    if let Some(local) = c.find(kind, id)? {
        return if c.visible(&local)? {
            Ok(Some(local))
        } else {
            Err(forbidden("label is unavailable"))
        };
    }
    let org = c.org_id()?;
    if org == c.space {
        return Ok(None);
    }
    let anchor = Context { space: &org, ..*c };
    if let Some(tag) = anchor.find(kind, id)?.filter(is_org)
        && anchor.visible(&tag)?
    {
        return Ok(Some(tag));
    }
    Ok(None)
}
pub(crate) fn validate(c: &Context<'_>, old: Option<&Record>, row: &mut Record) -> Result<()> {
    for key in ["scope", "color", "group"] {
        if row
            .fields
            .get(key)
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err(invalid("label scope, color and group must be text"));
        }
    }
    let scope = row.fields["scope"].as_str().unwrap_or("space").to_owned();
    if !matches!(scope.as_str(), "space" | "organization")
        || old.is_some_and(|r| r.fields["scope"].as_str().unwrap_or("space") != scope)
    {
        return Err(invalid(
            "label scope must be space or organization and cannot change",
        ));
    }
    if scope == "organization" {
        if c.org_id()? != c.space {
            return Err(forbidden(
                "manage organization labels in the organization settings",
            ));
        }
        c.require_admin()?;
    } else {
        c.require_edit()?;
    }
    if row.fields["visibility"] == "private"
        && old.is_none_or(|r| r.fields["visibility"] != "private")
    {
        return Err(invalid("labels are shared within their scope"));
    }
    row.fields["scope"] = json!(scope);
    row.fields["archived"] = json!(match row.fields.get("archived") {
        Some(v) => v
            .as_bool()
            .ok_or_else(|| invalid("archived must be boolean"))?,
        None => false,
    });
    if old.is_none() {
        if scope == "organization" {
            let conflict:bool=c.db.query_row("SELECT EXISTS(SELECT 1 FROM records s JOIN records t ON t.space=s.id WHERE s.kind='space' AND json_extract(s.fields,'$.organization')=?1 AND s.id<>?1 AND t.kind=?2 AND t.id=?3)",
                params![c.space,row.kind,row.id],|r|r.get(0)).map_err(storage)?;
            if conflict {
                return Err(crate::conflict(
                    "a space already uses this label identifier",
                ));
            }
        } else if find(c, &row.kind, &row.id)?.is_some() {
            return Err(conflict("label identifier is already in use"));
        }
    }
    let duplicate = c.records(&row.kind)?.iter().any(|r| {
        r.id != row.id
            && r.fields["scope"].as_str().unwrap_or("space") == scope
            && r.title.eq_ignore_ascii_case(&row.title)
    });
    if duplicate {
        return Err(conflict(
            "a label with this name already exists in this scope",
        ));
    }
    if row.kind == "tag" {
        let color = row.fields["color"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default_color(&row.id));
        if color.len() != 7
            || !color.starts_with('#')
            || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err(invalid("label color must be a six-digit hexadecimal color"));
        }
        row.fields["color"] = json!(color);
        if let Some(id) = group_id(row) {
            let group = c
                .find("tag_group", id)?
                .ok_or_else(|| invalid("label group is unavailable in this scope"))?;
            if is_org(&group) != is_org(row) {
                return Err(invalid("a label and its group must have the same scope"));
            }
        }
    }
    Ok(())
}

fn lookup(rows: &[Record], label: &str) -> Option<Record> {
    rows.iter()
        .find(|r| r.id == label)
        .or_else(|| rows.iter().find(|r| r.title.eq_ignore_ascii_case(label)))
        .cloned()
}
pub(crate) fn normalize(c: &Context<'_>, previous: &Value, labels: &Value) -> Result<Value> {
    let values = domain::strings(labels, "labels")?;
    if values.is_empty() {
        return Ok(json!([]));
    }
    if previous == labels {
        return Ok(labels.clone());
    }
    let mut rows = catalog(c, "tag")?;
    let mut selected = Vec::<Record>::new();
    for input in values {
        let value = input.trim();
        if value.is_empty() {
            return Err(invalid("empty task label"));
        }
        let tag = if let Some(tag) = lookup(&rows, value) {
            tag
        } else {
            // Untyped labels in earlier workspaces remain usable by their original text.
            c.require_edit()?;
            let id = id_for_title(c, value)?;
            let record = domain::write(
                c,
                &json!({"op":"create","kind":"tag","id":id,"title":value}),
            )?;
            let row: Record = serde_json::from_value(record).map_err(storage)?;
            rows.push(row.clone());
            row
        };
        if tag.fields["archived"] == true
            && !previous
                .as_array()
                .is_some_and(|old| old.iter().any(|l| l == &tag.id || l == &tag.title))
        {
            return Err(invalid("an archived label cannot be newly applied"));
        }
        if let Some(group) = group_id(&tag) {
            selected.retain(|r| group_id(r) != Some(group) || r.space != tag.space);
        }
        if !selected.iter().any(|r| r.id == tag.id) {
            selected.push(tag);
        }
    }
    Ok(json!(
        selected.into_iter().map(|r| r.id).collect::<Vec<_>>()
    ))
}
pub(crate) fn accessible(c: &Context<'_>, kind: &str, id: &str) -> Result<Record> {
    find(c, kind, id)?.ok_or_else(|| format!("not_found: {kind} is unavailable"))
}
pub(crate) fn move_labels(source: &Context<'_>, old: &Record) -> Result<Value> {
    let available = catalog(source, "tag")?;
    Ok(json!(
        old.fields["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|id| lookup(&available, id))
            .filter(is_org)
            .map(|r| r.id)
            .collect::<Vec<_>>()
    ))
}
fn scope_spaces(c: &Context<'_>, tag: &Record) -> Result<Vec<String>> {
    if is_org(tag) {
        return Ok(c
            .all_spaces()?
            .into_iter()
            .filter(|s| s.fields["organization"] == tag.space)
            .map(|s| s.id)
            .collect());
    }
    Ok(vec![tag.space.clone()])
}
fn rewrite(c: &Context<'_>, source: &Record, target: Option<&Record>) -> Result<()> {
    for space in scope_spaces(c, source)? {
        let dest = Context {
            space: &space,
            ..*c
        };
        let rows = catalog(&dest, "tag")?;
        let aliases: HashSet<&str> = rows
            .iter()
            .filter(|r| {
                target.is_some_and(|t| {
                    r.id != t.id
                        && group_id(t).is_some()
                        && group_id(r) == group_id(t)
                        && r.space == t.space
                })
            })
            .map(|r| r.id.as_str())
            .collect();
        for task in dest.records("task")? {
            let Some(labels) = task.fields["labels"].as_array() else {
                continue;
            };
            let matches_source = |label: &str| {
                label == source.id
                    || (label == source.title
                        && lookup(&rows, label).is_some_and(|r| r.id == source.id))
            };
            if !labels.iter().filter_map(Value::as_str).any(matches_source) {
                continue;
            }
            let mut next = labels
                .iter()
                .filter_map(Value::as_str)
                .filter(|id| !matches_source(id) && !aliases.contains(id))
                .collect::<Vec<_>>();
            if let Some(to) = target
                && !next.contains(&to.id.as_str())
            {
                next.push(&to.id);
            }
            let mut fields = task.fields.clone();
            fields["labels"] = json!(next);
            if fields != task.fields {
                dest.update(&task, &task.title, fields)?;
            }
        }
        for view in dest.records("view")? {
            let mut fields = view.fields.clone();
            for key in ["labels", "labels_not"] {
                let Some(filter) = fields["filter"].get_mut(key) else {
                    continue;
                };
                if let Some(items) = filter.as_array_mut() {
                    if items.iter().any(|v| v == &source.id) {
                        items.retain(|v| v != &source.id);
                        if let Some(tag) = target
                            && !items.iter().any(|v| v == &tag.id)
                        {
                            items.push(json!(tag.id));
                        }
                    }
                } else if *filter == source.id {
                    *filter = target.map_or_else(|| json!([]), |tag| json!(tag.id));
                }
            }
            if fields != view.fields {
                dest.update(&view, &view.title, fields)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn changed(c: &Context<'_>, before: Option<&Record>, tag: &Record) -> Result<()> {
    if tag.kind == "tag"
        && group_id(tag).is_some()
        && before.is_some_and(|r| group_id(r) != group_id(tag))
    {
        rewrite(c, tag, Some(tag))?;
    }
    Ok(())
}
pub(crate) fn cleanup(c: &Context<'_>, tag: &Record) -> Result<()> {
    c.authorize_write(&tag.kind, Some(tag))?;
    if is_org(tag) {
        c.require_admin()?;
    }
    if tag.kind == "tag_group" {
        if c.records("tag")?
            .iter()
            .any(|r| group_id(r) == Some(&tag.id))
        {
            return Err(conflict("move the group's labels before deleting it"));
        }
        return Ok(());
    }
    rewrite(c, tag, None)
}
pub(crate) fn merge(c: &Context<'_>, q: &Value) -> Result<Value> {
    let source = c.readable("tag", required(q, "id")?)?;
    c.authorize_write("tag", Some(&source))?;
    let target = c.readable("tag", required(q, "target")?)?;
    if is_org(&source) {
        c.require_admin()?;
    }
    domain::check_rev(q, &source)?;
    if is_org(&target) != is_org(&source)
        || target.id == source.id
        || target.fields["archived"] == true
    {
        return Err(invalid("choose a different active label in the same scope"));
    }
    rewrite(c, &source, Some(&target))?;
    c.remove(&source)?;
    Ok(json!({"merged":source.id,"target":target.id}))
}
pub(crate) fn apply(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_edit()?;
    let ids = domain::strings(&q["tasks"], "tasks")?;
    if ids.is_empty() || ids.len() > 1000 || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(invalid("select 1 to 1000 distinct tasks"));
    }
    let action = required(q, "action")?;
    let catalog = catalog(c, "tag")?;
    let selected = if action == "clear_group" {
        let group = accessible(c, "tag_group", required(q, "group")?)?;
        catalog
            .iter()
            .filter(|r| r.space == group.space && group_id(r) == Some(&group.id))
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    } else {
        if !matches!(action, "add" | "remove" | "replace") {
            return Err(invalid("unknown bulk label action"));
        }
        domain::strings(&q["labels"], "labels")?
            .iter()
            .map(|id| lookup(&catalog, id).map_or_else(|| (*id).to_owned(), |r| r.id))
            .collect()
    };
    for id in &ids {
        let task = c.readable("task", id)?;
        c.authorize_write("task", Some(&task))?;
        let mut labels: Vec<String> = domain::strings(&task.fields["labels"], "labels")?
            .into_iter()
            .map(str::to_owned)
            .collect();
        match action {
            "add" => labels.extend(selected.iter().cloned()),
            "replace" => labels = selected.clone(),
            _ => labels.retain(|v| !selected.contains(v)),
        }
        let labels = normalize(c, &task.fields["labels"], &json!(labels))?;
        if labels != task.fields["labels"] {
            domain::write(
                c,
                &json!({"op":"update","kind":"task","id":id,"expected_rev":task.rev,"fields":{"labels":labels}}),
            )?;
        }
    }
    Ok(json!({"tasks":ids,"action":action}))
}
