use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS view_workspace(singleton INTEGER PRIMARY KEY CHECK(singleton=1),reference TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS personal_view_order(space TEXT NOT NULL,actor TEXT NOT NULL,view_id TEXT NOT NULL,position INTEGER NOT NULL,pinned INTEGER NOT NULL,PRIMARY KEY(space,actor,view_id));").map_err(storage)?;
    db.execute(
        "INSERT OR IGNORE INTO view_workspace VALUES(1,?)",
        [Uuid::new_v4().to_string()],
    )
    .map_err(storage)?;
    Ok(())
}
pub(crate) fn can_edit(c: &Context<'_>, view: &Record) -> Result<()> {
    if view.fields["creator"] == c.actor {
        return Ok(());
    }
    let org = c.org_id()?;
    if view.fields["visibility"] != "private"
        && (Context { space: &org, ..*c }).require_admin().is_ok()
    {
        return Ok(());
    }
    Err(forbidden(
        "only the view creator or an organization administrator can update the saved view",
    ))
}
pub(crate) fn forget(c: &Context<'_>, id: &str) -> Result<()> {
    c.db.execute(
        "DELETE FROM personal_view_order WHERE space=? AND view_id=?",
        params![c.space, id],
    )
    .map_err(storage)?;
    Ok(())
}

fn catalog(c: &Context<'_>) -> Result<Vec<Record>> {
    let mut stmt =
        c.db.prepare(
            "SELECT view_id,position,pinned FROM personal_view_order WHERE space=? AND actor=?",
        )
        .map_err(storage)?;
    let orders: HashMap<String, (i64, bool)> = stmt
        .query_map(params![c.space, c.actor], |r| {
            Ok((r.get(0)?, (r.get(1)?, r.get(2)?)))
        })
        .map_err(storage)?
        .collect::<std::result::Result<_, _>>()
        .map_err(storage)?;
    let mut views = crate::query::visible_records(c, "view")?;
    let order = |r: &Record| {
        let (rank, pin) = orders
            .get(&r.id)
            .copied()
            .filter(|_| r.fields["visibility"] == "private" && r.fields["creator"] == c.actor)
            .unwrap_or((i64::MAX, false));
        (!pin, rank)
    };
    views.sort_by(|a, b| {
        order(a)
            .cmp(&order(b))
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    for view in &mut views {
        view.fields["personally_pinned"] = json!(!order(view).0);
        view.fields["can_edit_view"] = json!(c.require_edit().is_ok() && can_edit(c, view).is_ok());
    }
    Ok(views)
}

fn organize(c: &Context<'_>, q: &Value) -> Result<Value> {
    let view = c.readable("view", required(q, "id")?)?;
    if view.fields["visibility"] != "private" || view.fields["creator"] != c.actor {
        return Err(forbidden(
            "only your personal views can be pinned or reordered",
        ));
    }
    let mut rows: Vec<_> = catalog(c)?
        .into_iter()
        .filter(|v| v.fields["visibility"] == "private" && v.fields["creator"] == c.actor)
        .collect();
    let current = rows
        .iter()
        .position(|r| r.id == view.id)
        .ok_or_else(|| invalid("view is unavailable"))?;
    let mut item = rows.remove(current);
    let pin = q
        .get("pinned")
        .map(|v| v.as_bool().ok_or_else(|| invalid("pinned must be boolean")))
        .transpose()?;
    if let Some(value) = pin {
        item.fields["personally_pinned"] = json!(value);
    }
    let has_before = q.get("before").is_some();
    let has_after = q.get("after_view").is_some();
    if has_before && has_after || pin.is_none() && !has_before && !has_after {
        return Err(invalid("choose pinned, before or after_view"));
    }
    let position = if has_before || has_after {
        let anchor = required(q, if has_before { "before" } else { "after_view" })?;
        let idx = rows
            .iter()
            .position(|r| {
                r.id == anchor && r.fields["personally_pinned"] == item.fields["personally_pinned"]
            })
            .ok_or_else(|| {
                invalid("target must be another personal view in the same pinned group")
            })?;
        idx + usize::from(has_after)
    } else {
        current
    };
    rows.insert(position, item);
    for (i, v) in rows.iter().enumerate() {
        c.db.execute("INSERT INTO personal_view_order VALUES(?,?,?,?,?) ON CONFLICT(space,actor,view_id) DO UPDATE SET position=excluded.position,pinned=excluded.pinned",params![c.space,c.actor,v.id,i as i64,v.fields["personally_pinned"]==true]).map_err(storage)?;
    }
    Ok(json!({"id":view.id}))
}

fn configuration(base: &Value, changes: &Value) -> Result<Value> {
    let mut f = json!({});
    for allowed in [
        "description",
        "layout",
        "group",
        "sort",
        "direction",
        "query",
        "filter",
        "display_fields",
        "secondary_group",
        "include_triage",
    ] {
        if let Some(value) = base.get(allowed) {
            f[allowed] = value.clone();
        }
    }
    if changes.is_null() {
        return Ok(f);
    }
    for (key, v) in changes
        .as_object()
        .ok_or_else(|| invalid("changes must be an object"))?
    {
        if !matches!(
            key.as_str(),
            "description"
                | "layout"
                | "group"
                | "sort"
                | "direction"
                | "query"
                | "filter"
                | "display_fields"
                | "secondary_group"
                | "include_triage"
        ) {
            return Err(invalid("unknown saved view setting"));
        }
        if key == "filter" {
            let patch = v
                .as_object()
                .ok_or_else(|| invalid("filter must be an object"))?;
            if !f["filter"].is_object() {
                f["filter"] = json!({});
            }
            let filters = f["filter"].as_object_mut().unwrap();
            for (k, value) in patch {
                if value.is_null() {
                    filters.remove(k);
                } else {
                    filters.insert(k.clone(), value.clone());
                }
            }
        } else {
            f[key] = v.clone();
        }
    }
    Ok(f)
}

pub(crate) fn query(c: &Context<'_>, base: &Value, q: &Value) -> Result<Value> {
    let mut config = configuration(base, &q["changes"])?;
    for key in ["limit", "offset"] {
        if let Some(v) = q.get(key) {
            config[key] = v.clone();
        }
    }
    if config["include_triage"] != true
        && config["filter"].get("status").is_none()
        && config["filter"].get("status_category").is_none()
    {
        if !config["filter"].is_object() {
            config["filter"] = json!({});
        }
        if config["filter"].get("status_category_not").is_none() {
            config["filter"]["status_category_not"] = json!("triage");
        }
    }
    if q["op"] == "view.preview" && q["with_total"] == true {
        let (items, total) = crate::query::matching_page(c, "task", &config)?;
        Ok(json!({"items":items,"total":total}))
    } else {
        Ok(json!(crate::query::matching(c, "task", &config)?))
    }
}

pub(crate) fn workspace(c: &Context<'_>) -> Result<String> {
    c.db.query_row(
        "SELECT reference FROM view_workspace WHERE singleton=1",
        [],
        |r| r.get(0),
    )
    .map_err(storage)
}
pub fn view_link_parts(link: &str) -> Result<(String, String, String)> {
    resource_link_parts(link, "shipforge://view/")
}
pub(crate) fn resource_link_parts(link: &str, prefix: &str) -> Result<(String, String, String)> {
    let parts: Vec<_> = link
        .strip_prefix(prefix)
        .ok_or_else(|| invalid("invalid saved view link"))?
        .split('/')
        .collect();
    if parts.len() != 3 || Uuid::parse_str(parts[0]).is_err() {
        return Err(invalid("invalid saved view link"));
    }
    let id = |s: &str| -> Result<String> {
        if s.len() > 860 {
            return Err(invalid("invalid view link identifier"));
        }
        let value = String::from_utf8(
            URL_SAFE_NO_PAD
                .decode(s)
                .map_err(|_| invalid("invalid view link encoding"))?,
        )
        .map_err(|_| invalid("invalid view link identifier"))?;
        identifier(&value)?;
        Ok(value)
    };
    Ok((parts[0].into(), id(parts[1])?, id(parts[2])?))
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    match op {
        "view.catalog" => Ok(json!(catalog(c)?)),
        "view.organize" => organize(c, q),
        "view.preview" => query(c, &q["fields"], q),
        "view.share" => {
            let view = c.readable("view", required(q, "id")?)?;
            Ok(
                json!({"link":format!("shipforge://view/{}/{}/{}",workspace(c)?,URL_SAFE_NO_PAD.encode(c.space),URL_SAFE_NO_PAD.encode(view.id))}),
            )
        }
        "view.open" => {
            let (origin, space, id) = view_link_parts(required(q, "link")?)?;
            if workspace(c)? != origin {
                return Err(invalid(
                    "connect to the workspace that issued this saved view link",
                ));
            }
            let scope = Context {
                space: &space,
                ..*c
            };
            scope.role()?;
            let view = scope.readable("view", &id)?;
            let tasks = query(&scope, &view.fields, q)?;
            Ok(json!({"view":view,"tasks":tasks}))
        }
        "view.save_as" => {
            let source = c.readable("view", required(q, "id")?)?;
            let mut fields = configuration(&source.fields, &q["changes"])?;
            let visibility = q.get("visibility").map_or(Ok("private"), |v| {
                v.as_str()
                    .filter(|s| matches!(*s, "private" | "space"))
                    .ok_or_else(|| invalid("visibility must be private or space"))
            })?;
            fields["visibility"] = json!(visibility);
            crate::domain::write(
                c,
                &json!({"op":"create","kind":"view","title":required(q,"title")?,"fields":fields}),
            )
        }
        _ => Err(invalid("unknown view operation")),
    }
}
