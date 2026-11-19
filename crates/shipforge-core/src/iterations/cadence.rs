use super::*;

fn step(s: &Settings) -> Duration {
    Duration::days(i64::from(s.weeks * 7 + s.cooldown_days))
}
fn end(s: &Settings, start: NaiveDate) -> NaiveDate {
    start + Duration::days(i64::from(s.weeks) * 7 - 1)
}
fn candidate(s: &Settings, day: NaiveDate) -> NaiveDate {
    if day <= s.anchor_date {
        return s.anchor_date;
    }
    let span = step(s).num_days();
    let n = (day - s.anchor_date).num_days() / span;
    let start = s.anchor_date + Duration::days(n * span);
    if day > end(s, start) {
        start + step(s)
    } else {
        start
    }
}
fn after(s: &Settings, limit: NaiveDate) -> NaiveDate {
    let start = candidate(s, limit);
    if start < limit {
        start + step(s)
    } else {
        start
    }
}
fn open(c: &Context<'_>) -> Result<Vec<Record>> {
    let mut rows = c.records("iteration")?;
    rows.retain(|r| {
        r.fields["cadence"] == true
            && !matches!(r.fields["status"].as_str(), Some("closed" | "cancelled"))
    });
    rows.sort_by(|a, b| {
        a.fields["start_date"]
            .as_str()
            .cmp(&b.fields["start_date"].as_str())
    });
    Ok(rows)
}

fn move_to(c: &Context<'_>, task: &Record, target: Option<&str>) -> Result<()> {
    let mut f = task.fields.clone();
    f["iteration"] = json!(target);
    if target.is_none()
        && !matches!(
            c.status_category(task)?.as_str(),
            "done" | "cancelled" | "duplicate"
        )
        && let Ok(backlog) = crate::tasks::target_status(c, task, "backlog")
    {
        f["status"] = json!(backlog);
    }
    c.update(task, &task.title, f)?;
    Ok(())
}
fn detach(c: &Context<'_>, it: &Record, check_visibility: bool) -> Result<()> {
    for task in c
        .records("task")?
        .iter()
        .filter(|r| r.fields["iteration"] == it.id)
    {
        if check_visibility && !c.visible(task)? {
            return Err(forbidden(
                "cannot remove an iteration containing inaccessible tasks",
            ));
        }
        move_to(c, task, None)?;
    }
    history::forget(c, &it.id)
}

fn close(c: &Context<'_>, it: &Record, next: Option<&Record>) -> Result<()> {
    history::capture(c, it)?;
    for task in c.records("task")? {
        if task.fields["iteration"] == it.id
            && !matches!(
                c.status_category(&task)?.as_str(),
                "done" | "cancelled" | "duplicate"
            )
        {
            move_to(c, &task, next.map(|i| i.id.as_str()))?;
        }
    }
    let mut f = it.fields.clone();
    f["status"] = json!("closed");
    f["closed_at"] = json!(crate::now());
    c.update(it, &it.title, f)?;
    Ok(())
}

pub(super) fn disable(c: &Context<'_>, today: NaiveDate) -> Result<()> {
    let day = today.to_string();
    for row in c.records("iteration")? {
        if matches!(row.fields["status"].as_str(), Some("closed" | "cancelled")) {
            continue;
        }
        let start = row.fields["start_date"]
            .as_str()
            .or(row.fields["start"].as_str());
        if row.fields["status"] == "active" || start.is_some_and(|d| d <= day.as_str()) {
            close(c, &row, None)?;
        } else {
            detach(c, &row, false)?;
            c.remove(&row)?;
        }
    }
    c.db.execute(
        "UPDATE iteration_policies SET next_start=NULL WHERE space=?",
        [c.space],
    )
    .map_err(storage)?;
    Ok(())
}

pub(super) fn release(c: &Context<'_>) -> Result<()> {
    for row in open(c)? {
        let mut f = row.fields.clone();
        f.as_object_mut().expect("fields").remove("cadence");
        if f["status"] == "upcoming" {
            f["status"] = json!("planned");
        }
        c.update(&row, &row.title, f)?;
    }
    Ok(())
}

pub(super) fn reschedule(c: &Context<'_>, s: &Settings, today: NaiveDate) -> Result<()> {
    let rows = open(c)?;
    let current = rows.iter().find(|r| r.fields["status"] == "active");
    let mut cursor = if let Some(row) = current {
        after(
            s,
            date(required(&row.fields, "end_date")?)?
                + Duration::days(i64::from(s.cooldown_days) + 1),
        )
    } else {
        candidate(s, today)
    };
    let mut future = 0;
    for row in rows
        .iter()
        .filter(|r| current.is_none_or(|active| active.id != r.id))
    {
        if cursor > today && future >= s.future_count {
            detach(c, row, false)?;
            c.remove(row)?;
            continue;
        }
        let mut f = row.fields.clone();
        f["start_date"] = json!(cursor);
        f["end_date"] = json!(end(s, cursor));
        if f != row.fields {
            c.update(row, &row.title, f)?;
        }
        if cursor > today {
            future += 1;
        }
        cursor += step(s);
    }
    c.db.execute(
        "UPDATE iteration_policies SET next_start=? WHERE space=?",
        params![cursor.to_string(), c.space],
    )
    .map_err(storage)?;
    Ok(())
}

pub(super) fn maintain(c: &Context<'_>, s: &Settings, today: NaiveDate) -> Result<()> {
    let saved: Option<String> =
        c.db.query_row(
            "SELECT next_start FROM iteration_policies WHERE space=?",
            [c.space],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let mut start = saved
        .map(|v| date(&v))
        .transpose()?
        .unwrap_or_else(|| candidate(s, today));
    let day = today.to_string();
    let mut future = open(c)?
        .iter()
        .filter(|r| {
            r.fields["start_date"]
                .as_str()
                .is_some_and(|d| d > day.as_str())
        })
        .count() as u32;
    let mut ordinal:i64=c.db.query_row("SELECT coalesce(max(json_extract(fields,'$.sequence')),0) FROM records WHERE space=? AND kind='iteration' AND json_extract(fields,'$.cadence')=1",[c.space],|r|r.get(0)).map_err(storage)?;
    while start <= today || future < s.future_count {
        ordinal += 1;
        c.create("iteration",None,&format!("迭代 {ordinal}"),json!({"creator":"owner","cadence":true,"sequence":ordinal,"status":"planned","start_date":start,"end_date":end(s,start)}))?;
        if start > today {
            future += 1;
        }
        start += step(s);
    }
    let rows = open(c)?;
    let mut upcoming = false;
    for (i, row) in rows.iter().enumerate() {
        if required(&row.fields, "end_date")? < day.as_str() {
            close(c, row, rows.get(i + 1))?;
            continue;
        }
        let status = if required(&row.fields, "start_date")? <= day.as_str() {
            "active"
        } else if !upcoming {
            upcoming = true;
            "upcoming"
        } else {
            "planned"
        };
        if row.fields["status"] != status {
            let mut f = row.fields.clone();
            f["status"] = json!(status);
            c.update(row, &row.title, f)?;
        }
    }
    c.db.execute(
        "UPDATE iteration_policies SET next_start=?,maintained_on=? WHERE space=?",
        params![start.to_string(), day, c.space],
    )
    .map_err(storage)?;
    Ok(())
}

pub(super) fn start(c: &Context<'_>, q: &Value, s: &Settings) -> Result<Value> {
    c.require_edit()?;
    let row = c.readable("iteration", required(q, "id")?)?;
    crate::domain::check_rev(q, &row)?;
    if s.mode == Mode::Disabled
        || matches!(
            row.fields["status"].as_str(),
            Some("closed" | "cancelled" | "active")
        )
    {
        return Err(conflict("choose an unstarted iteration"));
    }
    let today = Utc::now().with_timezone(&s.zone()?).date_naive();
    let mut f = row.fields.clone();
    if row.fields["cadence"] == true {
        c.require_admin()?;
        if row.fields["status"] != "upcoming" {
            return Err(invalid("choose the next upcoming iteration"));
        }
        for current in open(c)?.iter().filter(|r| r.fields["status"] == "active") {
            close(c, current, Some(&row))?;
        }
        f["end_date"] = json!(end(s, today));
    } else if f["end_date"]
        .as_str()
        .or(f["end"].as_str())
        .is_none_or(|v| v < today.to_string().as_str())
    {
        f["end_date"] = json!(end(s, today));
    }
    f["start_date"] = json!(today);
    f["status"] = json!("active");
    let active = c.update(&row, &row.title, f)?;
    if active.fields["cadence"] == true {
        let mut changed = s.clone();
        changed.anchor_date = today;
        c.db.execute(
            "UPDATE iteration_policies SET config=?,rev=rev+1 WHERE space=?",
            params![serde_json::to_string(&changed).map_err(storage)?, c.space],
        )
        .map_err(storage)?;
        reschedule(c, &changed, today)?;
        maintain(c, &changed, today)?;
    }
    Ok(json!(active))
}

pub(crate) fn remove_manual(c: &Context<'_>, r: &Record) -> Result<()> {
    if r.fields["cadence"] == true {
        return Err(invalid(
            "manage automatic iterations through iteration settings",
        ));
    }
    detach(c, r, true)
}
