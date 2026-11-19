//! Reusable organization statuses and explicit links to space-local workflow records.
use crate::{Context, Record, Result, conflict, domain, invalid, required, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;
mod removal;
mod retained;
mod workflow;
pub(crate) use workflow::default_id;
pub(crate) use workflow::validate as validate_workflow;

pub(crate) const READS: &[&str] = &[
    "status.library",
    "status.library.removal",
    "task_type.workflow",
];
pub(crate) const WRITES: &[&str] = &[
    "status.library.save",
    "status.library.remove",
    "task_type.workflow.save",
];
const PROPERTIES: &[&str] = &["category", "names", "color", "collapsed", "order"];

pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS sf_status_link(org TEXT NOT NULL,source TEXT NOT NULL,space TEXT NOT NULL,local TEXT NOT NULL,PRIMARY KEY(org,source,space),UNIQUE(space,local));
    CREATE TABLE IF NOT EXISTS sf_retired_status(org TEXT NOT NULL,id TEXT NOT NULL,PRIMARY KEY(org,id));
    CREATE TRIGGER IF NOT EXISTS sf_status_unlink AFTER DELETE ON records WHEN OLD.kind='status' BEGIN DELETE FROM sf_status_link WHERE space=OLD.space AND local=OLD.id; END;
    INSERT OR IGNORE INTO sf_status_link(org,source,space,local)
    SELECT json_extract(s.fields,'$.organization'),v.id,v.space,v.id FROM records v JOIN records s ON s.kind='space' AND s.space=v.space AND s.id=v.space
    JOIN records o ON o.space=json_extract(s.fields,'$.organization') AND o.kind='status' AND o.id=v.id
    WHERE v.kind='status' AND v.space!=o.space AND v.rev=1 AND json_extract(v.fields,'$.system')=1 AND json_extract(v.fields,'$.category')=v.id AND json_extract(o.fields,'$.system')=1;").map_err(storage)
}
pub(crate) fn retired(c: &Context<'_>, key: &str) -> Result<bool> {
    c.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sf_retired_status WHERE org=? AND id=?)",
        params![c.org_id()?, key],
        |r| r.get(0),
    )
    .map_err(storage)
}
pub(crate) fn terminal(category: &str) -> bool {
    matches!(category, "done" | "cancelled" | "duplicate")
}
pub(crate) fn validate(c: &Context<'_>, old: Option<&Record>, f: &Value) -> Result<()> {
    if f["default"] == true && terminal(required(f, "category")?) {
        return Err(invalid("a terminal status cannot be a default"));
    }
    if f.get("collapsed").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("collapsed must be boolean"));
    }
    if let Some(names) = f.get("names") {
        let names = names
            .as_object()
            .filter(|v| v.len() <= 32)
            .ok_or_else(|| invalid("names must be a language-to-name object"))?;
        let mut languages = BTreeSet::new();
        for (lang, name) in names {
            if !languages.insert(lang.to_ascii_lowercase())
                || lang.len() > 35
                || lang.split('-').any(|p| {
                    p.is_empty() || p.len() > 8 || !p.bytes().all(|b| b.is_ascii_alphanumeric())
                })
                || !name.as_str().is_some_and(|s| {
                    !s.trim().is_empty()
                        && s.chars().count() <= 120
                        && !s.chars().any(char::is_control)
                })
            {
                return Err(invalid("invalid language or status translation"));
            }
        }
    }
    if let Some(v) = f.get("color").filter(|v| !v.is_null() && **v != "")
        && !v.as_str().is_some_and(|s| {
            s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|c| c.is_ascii_hexdigit())
        })
    {
        return Err(invalid("status color must be #RRGGBB"));
    }
    if let Some(old) = old
        && c.org_id()? == c.space
    {
        if old.fields["default"] == true && f["default"] != true {
            return Err(conflict("choose another organization default status first"));
        }
        if old.fields["category"] != f["category"] {
            if c.records("status")?
                .iter()
                .filter(|r| r.fields["category"] == old.fields["category"])
                .count()
                <= 1
            {
                return Err(conflict("every category needs at least one status"));
            }
            let counts = removal::usage(c, &old.id, true)?;
            if counts.tasks > 0 || counts.workflows > 0 {
                return Err(conflict("this organization's status category is in use"));
            }
        }
    }
    Ok(())
}

fn local_id(c: &Context<'_>, org: &str, source: &str) -> Result<Option<String>> {
    if c.space == org {
        return Ok(Some(source.to_owned()));
    }
    c.db.query_row(
        "SELECT local FROM sf_status_link WHERE org=? AND source=? AND space=?",
        params![org, source, c.space],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}
fn linked_spaces(root: &Context<'_>, source: &str) -> Result<Vec<(String, String)>> {
    let mut s = root
        .db
        .prepare("SELECT space,local FROM sf_status_link WHERE org=? AND source=? ORDER BY space")
        .map_err(storage)?;
    let mut list = vec![(root.space.to_owned(), source.to_owned())];
    list.extend(
        s.query_map(params![root.space, source], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?,
    );
    Ok(list)
}
fn properties(source: &Record, mut fields: Value) -> Value {
    for prop in PROPERTIES {
        if let Some(v) = source.fields.get(*prop) {
            fields[*prop] = v.clone();
        } else {
            fields.as_object_mut().unwrap().remove(*prop);
        }
    }
    fields
}

fn projected_title<'a>(source: &'a Record, local: &'a Record) -> &'a str {
    if source.fields["system"] == true
        && source.fields.get("names").is_none()
        && let Some((zh, en)) = crate::defaults::status_names(&source.id)
        && [zh, en].contains(&source.title.as_str())
        && [zh, en].contains(&local.title.as_str())
    {
        &local.title
    } else {
        &source.title
    }
}
fn attach(c: &Context<'_>, source: &Record) -> Result<String> {
    if let Some(id) = local_id(c, &source.space, &source.id)? {
        return Ok(id);
    }
    let existing = c.find("status", &source.id)?;
    let existing_seed = existing.as_ref().is_some_and(|r| {
        r.fields["system"] == true
            && r.rev == 1
            && r.fields["category"] == source.id
            && source.fields["system"] == true
    });
    let id = if existing_seed {
        source.id.clone()
    } else {
        let original_free = existing.is_none() && c.first_revision("status", &source.id).is_ok();
        if original_free {
            source.id.clone()
        } else {
            Uuid::new_v4().to_string()
        }
    };
    if let Some(row) = existing.filter(|_| existing_seed) {
        let fields = properties(source, row.fields.clone());
        let title = projected_title(source, &row);
        if row.title != title || fields != row.fields {
            retained::revise(c, &row, title, fields)?;
        }
    } else {
        retained::create(
            c,
            &id,
            &source.title,
            properties(source, json!({"creator":c.actor,"default":false})),
        )?;
    }
    c.db.execute(
        "INSERT INTO sf_status_link(org,source,space,local) VALUES(?,?,?,?)",
        params![source.space, source.id, c.space, id],
    )
    .map_err(storage)?;
    Ok(id)
}
pub(crate) fn seed(c: &Context<'_>) -> Result<()> {
    let org = c.org_id()?;
    if c.space == org {
        return Ok(());
    }
    let root = Context { space: &org, ..*c };
    for source in root.records("status")? {
        let local = attach(c, &source)?;
        if source.fields["default"] == true {
            set_default(c, &local)?;
        }
    }
    Ok(())
}
fn set_default(c: &Context<'_>, id: &str) -> Result<()> {
    for r in c.records("status")? {
        let wanted = r.id == id;
        if (r.fields["default"] == true) != wanted {
            let mut f = r.fields.clone();
            f["default"] = json!(wanted);
            retained::revise(c, &r, &r.title, f)?;
        }
    }
    Ok(())
}
pub(crate) fn changed(c: &Context<'_>, old: Option<&Record>, new: &Record) -> Result<()> {
    let org = c.org_id()?;
    if c.space != org {
        if let Some(old) = old
            && (old.title != new.title
                || PROPERTIES.iter().any(|k| old.fields[*k] != new.fields[*k]))
        {
            c.db.execute(
                "DELETE FROM sf_status_link WHERE space=? AND local=?",
                params![c.space, new.id],
            )
            .map_err(storage)?;
        }
        return Ok(());
    }
    for (space, local) in linked_spaces(c, &new.id)?
        .into_iter()
        .filter(|(s, _)| s != &org)
    {
        let scope = Context {
            space: &space,
            ..*c
        };
        let row = scope.get("status", &local)?;
        let fields = properties(new, row.fields.clone());
        let title = projected_title(new, &row);
        if fields != row.fields || row.title != title {
            retained::revise(&scope, &row, title, fields)?;
        }
    }
    if new.fields["default"] == true && old.is_none_or(|r| r.fields["default"] != true) {
        for space in c
            .all_spaces()?
            .into_iter()
            .filter(|s| s.id != org && s.fields["organization"] == org)
        {
            let scope = Context {
                space: &space.id,
                ..*c
            };
            let current = scope
                .records("status")?
                .into_iter()
                .find(|r| r.fields["default"] == true);
            if let Some(prior) = current
                && !scope
                    .db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sf_status_link WHERE space=? AND local=?)",
                        params![scope.space, prior.id],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(storage)?
            {
                continue;
            }
            let id = attach(&scope, new)?;
            set_default(&scope, &id)?;
        }
    }
    Ok(())
}
pub(crate) fn protect_delete(c: &Context<'_>, r: &Record) -> Result<()> {
    if c.space == c.org_id()?
        && (r.fields["default"] == true
            || linked_spaces(c, &r.id)?.len() > 1
            || c.records("status")?
                .iter()
                .filter(|s| s.fields["category"] == r.fields["category"])
                .count()
                <= 1)
    {
        return Err(conflict(
            "preview organization status removal and choose a migration target if required",
        ));
    }
    Ok(())
}
pub(crate) fn preferred(status: &Record, language: &str) -> String {
    display_name(
        &status.id,
        &status.title,
        status.fields.get("names"),
        status.fields["system"] == true,
        language,
    )
    .to_owned()
}
pub fn display_name<'a>(
    id: &str,
    title: &'a str,
    names: Option<&'a Value>,
    system: bool,
    language: &str,
) -> &'a str {
    if let Some(value) = names
        .and_then(Value::as_object)
        .and_then(|m| m.iter().find(|(l, _)| l.eq_ignore_ascii_case(language)))
        .and_then(|(_, v)| v.as_str())
    {
        return value;
    }
    if names.is_none()
        && system
        && let Some((zh, en)) = crate::defaults::status_names(id)
        && (title == zh || title == en)
    {
        return match language {
            "en" => en,
            "zh-CN" => zh,
            _ => title,
        };
    }
    title
}
fn catalog(c: &Context<'_>, root: &Context<'_>, q: &Value) -> Result<Value> {
    let language = q["language"].as_str().unwrap_or("zh-CN");
    let can_manage = root.require_admin().is_ok();
    let mut rows = root.records("status")?;
    rows.sort_by_key(|r| (r.fields["order"].as_i64().unwrap_or(0), r.id.clone()));
    let mut statuses = Vec::new();
    let mut usage = removal::catalog_usage(root, can_manage)?;
    for row in rows {
        let stats = usage.remove(&row.id).unwrap_or_default();
        statuses.push(json!({"id":row.id,"local_id":local_id(c,root.space,&row.id)?,"title":row.title,"name":preferred(&row,language),"fields":row.fields,"rev":row.rev,"usage":stats}));
    }
    Ok(json!({"organization":root.space,"can_manage":can_manage,"statuses":statuses}))
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    c.role()?;
    root.role()?;
    match op {
        "status.library" => catalog(c, &root, q),
        "task_type.workflow" | "task_type.workflow.save" => workflow::execute(c, &root, op, q),
        "status.library.removal" | "status.library.remove" => {
            root.require_admin()?;
            removal::execute(&root, q, op == "status.library.remove")
        }
        "status.library.save" => {
            root.require_admin()?;
            let key = q["id"].as_str();
            let existing = key.map(|id| root.find("status", id)).transpose()?.flatten();
            if existing.is_none() && q.get("expected_rev").is_some() {
                return Err(conflict("the status to edit no longer exists"));
            }
            let mut cmd = json!({"op":if existing.is_some(){"update"}else{"create"},"kind":"status","fields":q.get("fields").cloned().unwrap_or_else(||json!({}))});
            if let Some(id) = key {
                cmd["id"] = json!(id);
            }
            if let Some(t) = q.get("title") {
                cmd["title"] = t.clone();
            }
            if let Some(row) = existing {
                if q.get("expected_rev").is_none() {
                    return Err(invalid(
                        "expected_rev is required when editing an organization status",
                    ));
                }
                domain::check_rev(q, &row)?;
                cmd["expected_rev"] = json!(row.rev);
            }
            domain::write(&root, &cmd)
        }
        _ => Err(invalid("unknown workflow operation")),
    }
}

pub(crate) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    let status_ref = json!({"oneOf":[{"type":"string","minLength":1},{"type":"object","required":["library"],"properties":{"library":{"type":"string","minLength":1}},"additionalProperties":false},{"type":"object","required":["local"],"properties":{"local":{"type":"string","minLength":1}},"additionalProperties":false}]});
    rules.extend([
        json!({"if":{"properties":{"op":{"enum":["status.library.removal","task_type.workflow"]}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"const":"status.library.remove"}}},"then":{"required":["id","preview"]}}),
        json!({"if":{"properties":{"op":{"const":"task_type.workflow.save"}}},"then":{"required":["id","expected_rev","statuses"],"properties":{"statuses":{"type":"array","maxItems":256,"uniqueItems":true,"items":status_ref},"default_status":{"anyOf":[status_ref,{"type":"null"}]}}}}),
        json!({"if":{"properties":{"op":{"const":"status.library.save"}}},"then":{"properties":{"fields":{"type":"object","properties":{"category":{"enum":domain::BASE_STATUS},"names":{"type":"object","additionalProperties":{"type":"string"}},"order":{"type":"integer"},"collapsed":{"type":"boolean"},"default":{"type":"boolean"}}}}}})
    ]);
}
