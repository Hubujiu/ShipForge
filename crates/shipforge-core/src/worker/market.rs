use super::*;
use crate::{as_value, conflict, forbidden, invalid, skill_package, storage};
mod installation;
mod machines;

pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS catalog_releases(org TEXT,entry TEXT,version INTEGER,owner TEXT,space TEXT,audience TEXT,payload TEXT NOT NULL,PRIMARY KEY(org,entry,version));
        CREATE TABLE IF NOT EXISTS catalog_installs(space TEXT,entry TEXT,slot TEXT,kind TEXT,target TEXT,version INTEGER,installed_rev INTEGER,PRIMARY KEY(space,entry,slot),UNIQUE(space,kind,target));
        CREATE TRIGGER IF NOT EXISTS remove_catalog_install AFTER DELETE ON records WHEN OLD.kind IN ('skill','agent') BEGIN DELETE FROM catalog_installs WHERE space=OLD.space AND target=OLD.id AND kind=OLD.kind; END;
        CREATE TABLE IF NOT EXISTS skill_jobs(space TEXT, machine TEXT, actor TEXT, id TEXT PRIMARY KEY, payload TEXT NOT NULL, state TEXT NOT NULL, result TEXT NOT NULL,created INTEGER NOT NULL);").map_err(storage)
}
fn builtin() -> Vec<Value> {
    let mut entries = automation::catalog().as_array().unwrap().clone();
    for r in &mut entries {
        r["category"] = json!(match r["id"].as_str() {
            Some("change-summary" | "documentation-agent") => "knowledge",
            _ => "engineering",
        });
        r["version"] = json!(1);
        r["audience"] = json!("builtin");
        r["owner"] = Value::Null;
        r["items"] =
            json!([{"slot":"main","kind":r["kind"],"title":r["title"],"fields":r["fields"]}]);
    }
    let bundle = json!({"id":"delivery-skills","kind":"skill","title":"交付技能包","description":"包含变更摘要和验收检查两项技能。","category":"engineering","version":1,"audience":"builtin","items":[
        {"slot":"summary","kind":"skill","title":entries[0]["title"],"fields":entries[0]["fields"]},
        {"slot":"acceptance","kind":"skill","title":entries[1]["title"],"fields":entries[1]["fields"]}]});
    entries.push(bundle);
    entries
}
fn entries(c: &Context<'_>) -> Result<Vec<Value>> {
    let mut out = builtin();
    let mut stmt=c.db.prepare("SELECT payload FROM catalog_releases AS r WHERE org=? AND (space=? OR audience='organization') AND version=(SELECT MAX(version) FROM catalog_releases WHERE org=r.org AND entry=r.entry) ORDER BY entry").map_err(storage)?;
    for row in stmt
        .query_map(params![c.org_id()?, c.space], |r| r.get::<_, String>(0))
        .map_err(storage)?
    {
        out.push(serde_json::from_str(&row.map_err(storage)?).map_err(storage)?);
    }
    Ok(out)
}
fn entry(c: &Context<'_>, q: &Value) -> Result<Value> {
    let id = req(q, "id")?;
    entries(c)?
        .into_iter()
        .find(|x| x["id"] == id)
        .ok_or_else(|| forbidden("catalog entry is unavailable"))
}
fn present(c: &Context<'_>, r: &Value, detail: bool) -> Result<Value> {
    let items = r["items"]
        .as_array()
        .ok_or_else(|| invalid("catalog entry is incomplete"))?;
    let current = installation::installed(c, r)?;
    let mut fields = json!({"category":r["category"],"version":r["version"],"audience":r["audience"],"owner":r["owner"],"source_space":r["space"],"count":items.len(),"installed":current.len()==items.len(),"installed_version":current.iter().map(|(_,v,_)|*v).min(),"can_publish":r["owner"]==c.actor || (r["space"]==c.space && c.require_admin().is_ok())});
    if detail {
        fields["items"] = r["items"].clone();
    }
    Ok(
        json!({"kind":r["kind"],"id":r["id"],"title":r["title"],"description":r["description"],"fields":fields}),
    )
}
fn publish(c: &Context<'_>, q: &Value) -> Result<Value> {
    let id = req(q, "id")?;
    crate::identifier(id)?;
    if builtin().iter().any(|r| r["id"] == id) {
        return Err(conflict("built-in catalog entry"));
    }
    let org = c.org_id()?;
    let previous:Option<(String,String,String,i64,String)>=c.db.query_row("SELECT owner,space,audience,version,payload FROM catalog_releases WHERE org=? AND entry=? ORDER BY version DESC LIMIT 1",params![org,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(storage)?;
    let (owner, version) = if let Some((owner, space, prior_audience, version, payload)) = &previous
    {
        if space != c.space || (owner != c.actor && c.require_admin().is_err()) {
            return Err(forbidden("catalog publishing permission required"));
        }
        if q.get("audience").is_some_and(|v| v != prior_audience) {
            return Err(conflict("catalog release audience is fixed"));
        }
        if q["expected_version"].as_i64() != Some(*version) {
            return Err(conflict(
                "expected_version must match the last published version",
            ));
        }
        let old: Value = serde_json::from_str(payload).map_err(storage)?;
        if old["kind"] != q["kind"] {
            return Err(conflict("catalog entry type cannot change"));
        }
        (owner.to_owned(), version + 1)
    } else {
        if q["expected_version"].as_i64().unwrap_or(0) != 0 {
            return Err(conflict("catalog entry has no previous version"));
        }
        (c.actor.to_owned(), 1)
    };
    let audience = q["audience"]
        .as_str()
        .or_else(|| previous.as_ref().map(|p| p.2.as_str()))
        .unwrap_or("space");
    if !matches!(audience, "space" | "organization") {
        return Err(invalid("unknown publishing audience"));
    }
    let category = q["category"].as_str().unwrap_or("other");
    if !matches!(
        category,
        "engineering" | "office" | "knowledge" | "data" | "content" | "other"
    ) {
        return Err(invalid("unknown market category"));
    }
    let kind = req(q, "kind")?;
    if !matches!(kind, "skill" | "agent") {
        return Err(invalid("select skill or agent"));
    }
    let ids = crate::domain::strings(&q["sources"], "sources")?;
    if ids.is_empty() || ids.len() > 16 || (kind == "agent" && ids.len() != 1) {
        return Err(invalid("choose one Agent or 1 to 16 skills"));
    }
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(invalid("duplicate catalog source"));
        }
        let r = c.readable(kind, id)?;
        c.authorize_write(kind, Some(&r))?;
        let keys: &[&str] = if kind == "skill" {
            &["description", "instructions", "files"]
        } else {
            if r.fields["archived"] == true
                || r.fields["parent_id"].is_string()
                || r.fields["exclusive_crew"].is_string()
            {
                return Err(invalid("select an active original Agent"));
            }
            &[
                "description",
                "instructions",
                "model",
                "runtime",
                "agent_type",
                "timeout_seconds",
                "max_parallel",
                "chat_parallel",
            ]
        };
        let mut fields = json!({});
        for key in keys {
            if let Some(v) = r.fields.get(*key) {
                fields[*key] = v.clone();
            }
        }
        if kind == "agent" {
            let mut bundled = Vec::new();
            for skill in array_ids(&r.fields["skills"])? {
                let row = c.readable("skill", skill)?;
                c.authorize_write("skill", Some(&row))?;
                let mut copy = json!({});
                for key in ["description", "instructions", "files"] {
                    if let Some(v) = row.fields.get(key) {
                        copy[key] = v.clone();
                    }
                }
                bundled.push(json!({"title":row.title,"fields":copy}));
            }
            if !bundled.is_empty() {
                fields["bundled_skills"] = json!(bundled);
            }
        }
        if kind == "skill"
            && let Some(files) = fields.get("files")
        {
            skill_package::validate(files)?;
        }
        items.push(json!({"slot":r.id,"kind":kind,"title":r.title,"fields":fields}));
    }
    let title = req(q, "title")?;
    let payload = json!({"id":id,"kind":kind,"title":title,"description":q["description"].as_str().unwrap_or(""),"version":version,"audience":audience,"owner":owner,"category":category,"space":c.space,"items":items});
    let body = payload.to_string();
    if body.len() > 2 * 1024 * 1024 {
        return Err(invalid("catalog release exceeds 2 MiB"));
    }
    c.db.execute("INSERT INTO catalog_releases(org,entry,version,owner,space,audience,payload) VALUES(?,?,?,?,?,?,?)",params![org,id,version,owner,c.space,audience,body]).map_err(storage)?;
    present(c, &payload, true)
}
pub(super) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    match op {
        "catalog.list" => {
            let query = q["query"].as_str().unwrap_or("").to_lowercase();
            let mut out = Vec::new();
            for r in entries(c)? {
                if q["kind"].as_str().is_some_and(|v| r["kind"] != v)
                    || q["category"].as_str().is_some_and(|v| r["category"] != v)
                    || ![&r["title"], &r["description"], &r["id"]].iter().any(|v| {
                        v.as_str()
                            .is_some_and(|s| s.to_lowercase().contains(&query))
                    })
                {
                    continue;
                }
                out.push(present(c, &r, false)?);
            }
            Ok(json!(out))
        }
        "catalog.detail" => present(c, &entry(c, q)?, true),
        "catalog.publish" => publish(c, q),
        "catalog.install" | "catalog.update" | "catalog.uninstall" | "catalog.instantiate" => {
            installation::change(c, &entry(c, q)?, op, q)
        }
        "skill.import" => installation::import(c, q),
        "skill.bind" => installation::bind(
            c,
            req(q, "agent")?,
            req(q, "id")?,
            q["enabled"].as_bool().unwrap_or(true),
        ),
        _ => machines::execute(c, op, q),
    }
}
pub(super) fn schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().unwrap();
    for (op, args) in [
        ("catalog.detail", vec!["id"]),
        ("catalog.publish", vec!["id", "kind", "sources", "title"]),
        ("catalog.update", vec!["id"]),
        ("catalog.uninstall", vec!["id"]),
        ("catalog.instantiate", vec!["id"]),
        ("skill.bind", vec!["id", "agent"]),
        ("skill.discover", vec!["machine_id", "runtime"]),
        (
            "skill.machine_import",
            vec!["machine_id", "runtime", "source_id"],
        ),
        ("skill.sources", vec!["machine_id", "runtime"]),
        ("skill.jobs", vec!["machine_id", "machine_key"]),
        ("skill.finish", vec!["machine_id", "machine_key", "id"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":args}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"const":"catalog.publish"}}},"then":{"properties":{"sources":{"type":"array","minItems":1,"maxItems":16,"uniqueItems":true,"items":{"type":"string"}},"category":{"enum":["engineering","office","knowledge","data","content","other"]},"audience":{"enum":["space","organization"]},"expected_version":{"type":"integer","minimum":0}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"skill.import"}}},"then":{"oneOf":[{"required":["files"]},{"required":["packages"]},{"required":["path"]},{"required":["url"]}],"properties":{"files":{"type":"array","minItems":1,"maxItems":96,"items":{"type":"object","required":["path","text"],"properties":{"path":{"type":"string"},"text":{"type":"string"}}}},"packages":{"type":"array","minItems":1,"maxItems":16},"path":{"type":"string"},"url":{"type":"string","format":"uri"}}}}));
}
