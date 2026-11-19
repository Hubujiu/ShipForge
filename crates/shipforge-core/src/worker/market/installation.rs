use super::*;

type Install = (Record, i64, i64);
pub(super) fn installed(c: &Context<'_>, r: &Value) -> Result<Vec<Install>> {
    let id = req(r, "id")?;
    let mut stmt=c.db.prepare("SELECT kind,target,version,installed_rev FROM catalog_installs WHERE space=? AND entry=? ORDER BY slot").map_err(storage)?;
    let mut out = Vec::new();
    for row in stmt
        .query_map(params![c.space, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get(3)?,
            ))
        })
        .map_err(storage)?
    {
        let (kind, target, version, rev) = row.map_err(storage)?;
        if let Ok(record) = c.readable(&kind, &target) {
            out.push((record, version, rev));
        }
    }
    // Before versioned releases, each built-in was a single deterministic record.
    if out.is_empty()
        && r["audience"] == "builtin"
        && r["items"].as_array().is_some_and(|v| v.len() == 1)
        && let Some(existing) = c.find(req(r, "kind")?, &format!("template-{id}"))?
        && existing.fields["template_id"] == id
        && c.visible(&existing)?
    {
        let rev = existing.rev;
        out.push((existing, 1, rev));
    }
    Ok(out)
}
pub(super) fn bind(c: &Context<'_>, agent: &str, skill: &str, enabled: bool) -> Result<Value> {
    c.readable("skill", skill)?;
    let r = c.readable("agent", agent)?;
    crate::agents::require_manage(c, &r)?;
    if r.fields["parent_id"].is_string() {
        return Err(invalid("edit skills on the original Agent"));
    }
    let mut ids = super::super::array_ids(&r.fields["skills"])?;
    if ids.contains(&skill) == enabled {
        return Ok(as_value(r));
    }
    ids.retain(|id| *id != skill);
    if enabled {
        ids.push(skill);
    }
    crate::write_request(
        c,
        &json!({"op":"update","kind":"agent","id":agent,"fields":{"skills":ids},"expected_rev":r.rev}),
    )
}
fn write_item(
    c: &Context<'_>,
    r: &Value,
    item: &Value,
    q: &Value,
    target: &str,
    old: Option<&Record>,
) -> Result<Record> {
    let mut fields = item["fields"].clone();
    if let Some(bundled) = fields
        .as_object_mut()
        .and_then(|f| f.remove("bundled_skills"))
    {
        let mut ids = Vec::new();
        for skill in bundled.as_array().into_iter().flatten() {
            let copy = crate::write_request(
                c,
                &json!({"op":"create","kind":"skill","title":skill["title"],"fields":skill["fields"]}),
            )?;
            ids.push(copy["id"].clone());
        }
        fields["skills"] = json!(ids);
    }
    fields["template_id"] = r["id"].clone();
    fields["version"] = json!(r["version"].to_string());
    if item["kind"] == "agent"
        && let Some(runtime) = q["runtime"].as_str()
    {
        fields["runtime"] = json!(runtime);
    }
    let v = crate::write_request(
        c,
        &json!({"op":if old.is_some(){"update"}else{"create"},"id":target,"kind":item["kind"],"title":q.get("instance_title").filter(|v|v.is_string()).unwrap_or(&item["title"]),"fields":fields}),
    )?;
    serde_json::from_value(v).map_err(storage)
}
pub(super) fn change(c: &Context<'_>, r: &Value, op: &str, q: &Value) -> Result<Value> {
    let entry = req(r, "id")?;
    let release = r["version"].as_i64().unwrap_or(1);
    let items = r["items"]
        .as_array()
        .ok_or_else(|| invalid("empty release"))?;
    if op == "catalog.instantiate" {
        if r["kind"] != "agent" {
            return Err(invalid("select an Agent template"));
        }
        let id = q["target_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        return Ok(as_value(write_item(c, r, &items[0], q, &id, None)?));
    }
    let current = installed(c, r)?;
    if op == "catalog.uninstall" {
        let mut removed = Vec::new();
        for (record, _, _) in current {
            crate::write_request(
                c,
                &json!({"op":"delete","kind":record.kind,"id":record.id,"expected_rev":record.rev}),
            )?;
            removed.push(record.id);
        }
        return Ok(json!({"removed":removed}));
    }
    if op == "catalog.update" && (r["kind"] == "agent" || current.is_empty()) {
        return Err(invalid("select an installed skill package"));
    }
    let mut result = Vec::new();
    let mut targets = HashSet::new();
    for item in items {
        let slot = req(item, "slot")?;
        let kind = req(item, "kind")?;
        let target = if r["audience"] == "builtin" && slot == "main" {
            format!("template-{entry}")
        } else {
            format!(
                "market-{}",
                &super::super::digest(&json!([c.org_id()?, entry, slot]).to_string())[..40]
            )
        };
        let next =
            if let Some((old, version, rev)) = current.iter().find(|(v, _, _)| v.id == target) {
                if op == "catalog.update" && *version < release {
                    if old.rev != *rev && q["force"] != true {
                        return Err(conflict(
                            "installed skill was edited; confirm replacement with force",
                        ));
                    }
                    write_item(c, r, item, q, &target, Some(old))?
                } else {
                    old.clone()
                }
            } else {
                write_item(c, r, item, q, &target, None)?
            };
        if !current.iter().any(|(v, version, _)| {
            v.id == next.id && (op != "catalog.update" || *version == release)
        }) {
            c.db.execute("INSERT INTO catalog_installs(space,entry,slot,kind,target,version,installed_rev) VALUES(?,?,?,?,?,?,?) ON CONFLICT(space,entry,slot) DO UPDATE SET version=excluded.version,installed_rev=excluded.installed_rev",params![c.space,entry,slot,kind,target,release,next.rev]).map_err(storage)?;
        }
        if let Some(agent) = q["agent"].as_str().filter(|v| !v.is_empty()) {
            if kind != "skill" {
                return Err(invalid("only skills can bind to an Agent"));
            }
            bind(c, agent, &next.id, true)?;
        }
        targets.insert(next.id.clone());
        result.push(as_value(next));
    }
    if op == "catalog.update" {
        for (old, _, rev) in current
            .into_iter()
            .filter(|(v, _, _)| !targets.contains(&v.id))
        {
            if old.rev != rev && q["force"] != true {
                return Err(conflict(
                    "removed package member was edited; confirm replacement with force",
                ));
            }
            crate::write_request(
                c,
                &json!({"op":"delete","kind":old.kind,"id":old.id,"expected_rev":old.rev}),
            )?;
        }
    }
    Ok(if result.len() == 1 {
        result.remove(0)
    } else {
        json!({"records":result,"version":release})
    })
}
pub(super) fn import(c: &Context<'_>, q: &Value) -> Result<Value> {
    let packages = if let Some(v) = q.get("packages") {
        v.as_array()
            .filter(|v| !v.is_empty() && v.len() <= 16)
            .cloned()
            .ok_or_else(|| invalid("select 1 to 16 skill packages"))?
    } else {
        vec![json!({"files":q["files"]})]
    };
    let target = q["agent"].as_str().filter(|v| !v.is_empty());
    if let Some(agent) = target {
        crate::agents::require_manage(c, &c.readable("agent", agent)?)?;
    }
    let mut out = Vec::new();
    for package in packages {
        let skill = skill_package::validate(&package["files"])?;
        let title = req(&skill, "title")?;
        let mut existing = None;
        for row in c.records("skill")? {
            if row.title.to_lowercase() == title.to_lowercase() && c.visible(&row)? {
                existing = Some(row);
                break;
            }
        }
        let (record, status) = if let Some(r) = existing {
            let status = if let Some(agent) = target {
                if array_ids(&c.readable("agent", agent)?.fields["skills"])?
                    .contains(&r.id.as_str())
                {
                    "exists"
                } else {
                    "linked"
                }
            } else {
                "skipped"
            };
            (r, status)
        } else {
            let fields = json!({"description":skill["description"],"instructions":skill["instructions"],"files":skill["files"],"version":"1"});
            let r = crate::write_request(
                c,
                &json!({"op":"create","kind":"skill","title":title,"fields":fields}),
            )?;
            (serde_json::from_value(r).map_err(storage)?, "imported")
        };
        if let Some(agent) = target {
            bind(c, agent, &record.id, true)?;
        }
        out.push(json!({"id":record.id,"title":record.title,"status":status}));
    }
    Ok(json!({"results":out}))
}
