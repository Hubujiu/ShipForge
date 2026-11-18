use crate::{Context, Record, Result, invalid, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::OnceLock};

#[derive(Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub steps: Vec<Step>,
}
#[derive(Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub title: String,
    pub points: u32,
    pub hint: String,
    pub button: String,
}
pub fn groups() -> &'static [Group] {
    static CATALOG: OnceLock<Vec<Group>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("onboarding/catalog.json")).expect("valid guide catalog")
    })
}
pub fn available_points() -> u32 {
    groups()
        .iter()
        .flat_map(|g| &g.steps)
        .map(|s| s.points)
        .sum()
}

pub(crate) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS guide_credits(
        org TEXT NOT NULL,actor TEXT NOT NULL,step TEXT NOT NULL,at INTEGER NOT NULL,
        PRIMARY KEY(org,actor,step));
        CREATE TABLE IF NOT EXISTS guide_display(org TEXT NOT NULL,actor TEXT NOT NULL,
        visible INTEGER NOT NULL DEFAULT 1,expanded INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY(org,actor));
        CREATE TABLE IF NOT EXISTS guide_migrations(version INTEGER PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS guide_points(step TEXT PRIMARY KEY,points INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS guide_setup(space TEXT PRIMARY KEY,finished INTEGER NOT NULL DEFAULT 0);")
        .map_err(storage)?;
    let mut points=db.prepare_cached("INSERT INTO guide_points VALUES(?,?) ON CONFLICT(step) DO UPDATE SET points=excluded.points WHERE points<>excluded.points").map_err(storage)?;
    for step in groups().iter().flat_map(|g| &g.steps) {
        points
            .execute(params![step.id, step.points])
            .map_err(storage)?;
    }
    Ok(())
}

// An upgrade can recover completed work even when its original task has since been deleted.
pub(crate) fn backfill(db: &Connection) -> Result<()> {
    if db
        .execute("INSERT OR IGNORE INTO guide_migrations VALUES(1)", [])
        .map_err(storage)?
        == 0
    {
        return Ok(());
    }
    let mut statement = db
        .prepare(
            "SELECT space,actor,action,snapshot,previous FROM
        (SELECT seq,space,actor,action,snapshot,
         lag(snapshot) OVER(PARTITION BY space,kind,id ORDER BY seq) AS previous FROM event_log)
        WHERE action IN ('create','update') ORDER BY seq",
        )
        .map_err(storage)?;
    let mut rows = statement.query([]).map_err(storage)?;
    while let Some(row) = rows.next().map_err(storage)? {
        let space: String = row.get(0).map_err(storage)?;
        let actor: String = row.get(1).map_err(storage)?;
        let action: String = row.get(2).map_err(storage)?;
        let snapshot: String = row.get(3).map_err(storage)?;
        let previous: Option<String> = row.get(4).map_err(storage)?;
        let current: Record = serde_json::from_str(&snapshot).map_err(storage)?;
        let old: Option<Record> = previous
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(storage)?;
        let c = Context {
            db,
            space: &space,
            actor: &actor,
        };
        if c.find("space", &space)?.is_some() {
            observe(
                &c,
                &current,
                if action == "create" {
                    None
                } else {
                    old.as_ref()
                },
            )?;
        }
    }
    db.execute(
        "INSERT OR IGNORE INTO guide_credits(org,actor,step,at)
       SELECT COALESCE(json_extract(s.fields,'$.organization'),'default'),f.actor,'follow_task',0
       FROM followers f JOIN records s ON s.space=f.space AND s.kind='space' AND s.id=f.space",
        [],
    )
    .map_err(storage)?;
    Ok(())
}

fn award(c: &Context<'_>, ids: &[&str], at: i64) -> Result<()> {
    if !ids.is_empty() {
        let org = c.org_id()?;
        let mut write =
            c.db.prepare_cached(
                "INSERT OR IGNORE INTO guide_credits(org,actor,step,at) VALUES(?,?,?,?)",
            )
            .map_err(storage)?;
        for id in ids {
            write
                .execute(params![org, c.actor, id, at])
                .map_err(storage)?;
        }
    }
    Ok(())
}
pub(crate) fn imported_tasks(c: &Context<'_>) -> Result<()> {
    award(c, &["import_tasks"], crate::now())
}

pub(crate) fn observe(c: &Context<'_>, row: &Record, previous: Option<&Record>) -> Result<()> {
    let f = &row.fields;
    let before = previous.map(|r| &r.fields).unwrap_or(&Value::Null);
    let new = previous.is_none();
    let changed = |key: &str| before[key] != f[key];
    let selected =
        |key: &str| changed(key) && f[key].as_str().is_some_and(|s| !s.trim().is_empty());
    let added = |key: &str| {
        f[key].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| !before[key].as_array().is_some_and(|old| old.contains(item)))
        })
    };
    let mut ids = Vec::new();
    match row.kind.as_str() {
        "machine" if new && f["last_seen"].is_number() => ids.push("connect_machine"),
        "agent" => {
            if new {
                ids.push("create_agent");
            }
            if selected("instructions") {
                ids.push("agent_instructions");
            }
            if changed("visibility") && f["visibility"] == "space" {
                ids.push("share_agent");
            }
            if added("skills") {
                ids.push("equip_skill");
            }
        }
        "task" => {
            if new {
                ids.push("create_task");
            }
            if added("assignees") {
                ids.push("assign_member");
            }
            if selected("assignee") || (changed("assignee_kind") && f["assignee"].is_string()) {
                match f["assignee_kind"].as_str() {
                    Some("agent") => ids.push("assign_agent"),
                    Some("member") => ids.push("assign_member"),
                    _ => {}
                }
            }
            if added("labels") {
                ids.push("label_task");
            }
            if selected("parent") {
                ids.push("subtask");
            }
            if selected("project") {
                ids.push("project_task");
            }
        }
        "invitation"
            if new
                && f["state"] == "active"
                && matches!(f["channel"].as_str(), Some("email" | "link")) =>
        {
            ids.push("invite_member")
        }
        "comment" if new && f["task"].is_string() => ids.push("comment_task"),
        "notification" if f["recipient"] == c.actor && f["task"].is_string() => {
            if changed("read") && f["read"] == true {
                ids.push("handle_inbox");
            }
            if changed("state") && f["state"] == "cleared" {
                ids.push("clear_inbox");
            }
        }
        "message" if new => {
            if let Some(chat_id) = f["chat"].as_str()
                && c.find("chat", chat_id)?
                    .is_some_and(|chat| chat.fields["agent"].is_string())
            {
                ids.push("chat_agent");
            }
        }
        "crew" if new => ids.push("create_crew"),
        "skill" if new && f["template_id"].is_string() => ids.push("install_skill"),
        "tag" if new => ids.push("label_task"),
        "connector"
            if (changed("link_tasks") || changed("enabled") || changed("repository"))
                && f["link_tasks"] == true
                && f["enabled"] != false
                && matches!(f["provider"].as_str(), Some("github" | "gitlab"))
                && f["repository"].as_str().is_some_and(|r| {
                    r.split('/').count() >= 2 && r.split('/').all(|p| !p.is_empty())
                }) =>
        {
            ids.push("git_link")
        }
        "relation" if new => {
            ids.push("relate_task");
            if f["type"] == "parent" {
                ids.push("subtask");
            }
        }
        "view" => {
            if new {
                ids.push("custom_view");
            }
            if changed("visibility") && f["visibility"] == "space" {
                ids.push("team_view");
            }
        }
        "project" if new => ids.push("create_project"),
        "task_type" if added("required_fields") || added("fields") => ids.push("type_fields"),
        "status" if !new || f["system"] != true => ids.push("manage_status"),
        "space" if changed("estimates_enabled") && f["estimates_enabled"] == true => {
            ids.push("estimates")
        }
        "automation" => {
            if new {
                ids.push("create_rule");
            }
            let triggers = |v: &Value| {
                v["triggers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t["type"].as_str().map(str::to_owned))
                    .collect::<BTreeSet<_>>()
            };
            let old = triggers(before);
            for kind in triggers(f).difference(&old) {
                if let Some(id) = match kind.as_str() {
                    "schedule" => Some("schedule_rule"),
                    "status" => Some("status_rule"),
                    "comment" => Some("comment_rule"),
                    "webhook" => Some("webhook_rule"),
                    _ => None,
                } {
                    ids.push(id);
                }
            }
        }
        "run" if new && f["origin_kind"] == "automation" => ids.push("run_rule"),
        _ => {}
    }
    award(c, &ids, row.updated_at)
}

pub(crate) fn successful(c: &Context<'_>, op: &str, q: &Value, result: &Value) -> Result<()> {
    let mut steps = Vec::new();
    match op {
        "follow" if q["enabled"] != false => steps.push("follow_task"),
        "machine.register" => steps.push("connect_machine"),
        "provider.ingest" | "connector.ingest"
            if matches!(
                result["action"].as_str(),
                Some("created" | "updated" | "commented")
            ) =>
        {
            let record = c.get("connector", crate::required(q, "id")?)?;
            match record.fields["provider"].as_str() {
                Some("feishu" | "wecom" | "dingtalk") => steps.push("connect_im"),
                Some("slack" | "lark") => steps.push("connect_slack_lark"),
                Some("teams" | "discord" | "telegram" | "github" | "gitlab" | "cnb") => {}
                _ => steps.push("import_tasks"),
            }
        }
        _ => {}
    }
    award(c, &steps, crate::now())
}

pub(crate) fn get(c: &Context<'_>) -> Result<Value> {
    let org = c.org_id()?;
    let (visible, expanded) =
        c.db.query_row(
            "SELECT visible,expanded FROM guide_display WHERE org=? AND actor=?",
            params![org, c.actor],
            |r| Ok((r.get::<_, bool>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()
        .map_err(storage)?
        .unwrap_or((true, false));
    let mut q=c.db.prepare_cached("SELECT step FROM guide_credits JOIN guide_points USING(step) WHERE org=? AND actor=? ORDER BY step").map_err(storage)?;
    let completed = q
        .query_map(params![org, c.actor], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let (points,rank):(u32,u32)=c.db.query_row("WITH scores AS (
         SELECT actor,SUM(points) AS score FROM guide_credits JOIN guide_points USING(step)
          WHERE org=?1 AND (actor=?2 OR actor='owner' OR EXISTS(
            SELECT 1 FROM records m WHERE m.space=?1 AND m.kind='member' AND m.id=guide_credits.actor)) GROUP BY actor),
         mine AS (SELECT COALESCE((SELECT score FROM scores WHERE actor=?2),0) AS score)
         SELECT mine.score,1+(SELECT COUNT(*) FROM scores WHERE scores.score>mine.score) FROM mine",
        params![org,c.actor],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
    let setup_required: bool =
        c.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM guide_setup WHERE space=? AND finished=0)",
            [c.space],
            |r| r.get(0),
        )
        .map_err(storage)?;
    Ok(
        json!({"organization":org,"actor":c.actor,"sidebar_visible":visible,"sidebar_expanded":expanded,
        "completed":completed,"points":points,"max_points":available_points(),"rank":rank,
        "setup_required":setup_required && matches!(c.role()?.as_str(),"owner"|"admin")}),
    )
}

pub(crate) fn display(c: &Context<'_>, q: &Value) -> Result<Value> {
    let flag = |name: &str| {
        q.get(name)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| invalid(&format!("{name} must be boolean")))
            })
            .transpose()
    };
    let visible = flag("sidebar_visible")?;
    let expanded = flag("sidebar_expanded")?;
    if visible.is_none() && expanded.is_none() {
        return Err(invalid("choose a sidebar display setting"));
    }
    c.db.execute("INSERT INTO guide_display(org,actor,visible,expanded)
        VALUES(?1,?2,COALESCE(?3,1),COALESCE(?4,0))
        ON CONFLICT(org,actor) DO UPDATE SET visible=COALESCE(?3,visible),expanded=COALESCE(?4,expanded)",
        params![c.org_id()?,c.actor,visible,expanded]).map_err(storage)?;
    get(c)
}

pub(crate) fn setup(c: &Context<'_>, q: &Value) -> Result<Value> {
    c.require_admin()?;
    let org_id = c.org_id()?;
    if org_id != c.space {
        return Err(invalid("initialize the organization's default space"));
    }
    let name = crate::required(q, "organization_name")?.trim();
    let slug = crate::required(q, "organization_slug")?;
    if name.chars().count() > 100
        || slug.len() > 63
        || !slug.split('-').all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
    {
        return Err(invalid(
            "enter a name and a lowercase organization address with optional hyphens",
        ));
    }
    let work = crate::required(q, "work_type")?.trim();
    // Configure exactly the empty-install setup offered by this workspace. Never reset an older profile.
    if c.db
        .execute(
            "UPDATE guide_setup SET finished=1 WHERE space=? AND finished=0",
            [c.space],
        )
        .map_err(storage)?
        != 1
    {
        return Err(crate::conflict(
            "the workspace has already been initialized",
        ));
    }
    let prior = c.get("organization", &org_id)?;
    let mut org_fields = prior.fields.clone();
    org_fields["slug"] = json!(slug);
    c.update(&prior, name, org_fields)?;
    let space = c.get("space", c.space)?;
    let mut f = space.fields.clone();
    f["work_type"] = json!(work);
    f["agent_type_default"] = json!(if work == "software" { "code" } else { "office" });
    c.update(&space, &space.title, f)?;
    crate::defaults::starting_task_types(c, work, Some(&q["task_types"]))?;
    get(c)
}
