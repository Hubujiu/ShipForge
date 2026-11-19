use super::*;
use shipforge_client::skill_files::{checked_meta, from_path, metadata};
use std::collections::BTreeMap;

fn roots(config: &Config, runtime: &str) -> Result<Vec<(PathBuf, &'static str)>> {
    if !config.runtimes.contains_key(runtime) {
        return Err("runtime is not approved on this machine".into());
    }
    let mut roots = Vec::new();
    for dir in config.skill_roots.get(runtime).into_iter().flatten() {
        if checked_meta(dir)?.is_dir() {
            roots.push((dir.canonicalize().map_err(|e| e.to_string())?, "runtime"));
        }
    }
    for source in config.workspace.iter().chain(config.profiles.values()) {
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        for name in [".agent", ".agents", ".claude", ".codex"] {
            let parent = source.join(name);
            let dir = parent.join("skills");
            if dir.exists() && checked_meta(&parent)?.is_dir() && checked_meta(&dir)?.is_dir() {
                let dir = dir.canonicalize().map_err(|e| e.to_string())?;
                if dir.starts_with(&source) {
                    roots.push((dir, "project"));
                }
            }
        }
    }
    Ok(roots)
}
fn candidates(config: &Config, runtime: &str) -> Result<BTreeMap<String, (PathBuf, Value)>> {
    let mut found = BTreeMap::new();
    for (root, origin) in roots(config, runtime)? {
        for (index, e) in fs::read_dir(root).map_err(|e| e.to_string())?.enumerate() {
            if index >= 4096 {
                return Err("too many entries in the approved skills directory".into());
            }
            let path = e.map_err(|e| e.to_string())?.path();
            let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(meta) = checked_meta(&path) else {
                continue;
            };
            let entry = if meta.is_dir() && path.join("SKILL.md").is_file() {
                path.join("SKILL.md")
            } else if meta.is_file()
                && (name == "SKILL.md" || name.ends_with(".skill") || name.ends_with(".zip"))
            {
                path.clone()
            } else {
                continue;
            };
            let Ok(packages) = from_path(&entry) else {
                continue;
            };
            if packages.len() != 1 {
                continue;
            }
            let Ok(mut data) = metadata(&packages[0]) else {
                continue;
            };
            let id = crate::config::hex_digest(json!([runtime, path]).to_string().as_bytes());
            data["id"] = json!(id);
            data["origin"] = json!(origin);
            found.insert(id, (path, data));
            if found.len() > 2048 {
                return Err("more than 2048 local skills; narrow the approved directory".into());
            }
        }
    }
    Ok(found)
}
pub(super) fn poll(host: &Host, client: &mut Client) -> Result<()> {
    let queue = match host.call(client, json!({"op":"skill.jobs"})) {
        Ok(value) => value,
        Err(e)
            if e.contains("unknown operation skill.jobs")
                || e.contains("token scope does not allow this operation") =>
        {
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    for job in queue.as_array().into_iter().flatten() {
        let mut response = json!({"op":"skill.finish","id":job["id"]});
        let run = (|| {
            let mut found = candidates(
                &host.cfg,
                job["runtime"].as_str().ok_or("missing skill runtime")?,
            )?;
            if job["op"] == "skill.discover" {
                response["sources"] =
                    json!(found.into_values().map(|(_, v)| v).collect::<Vec<_>>());
            } else {
                let (path, _) = found
                    .remove(job["source_id"].as_str().unwrap_or(""))
                    .ok_or("skill source no longer exists")?;
                let package = from_path(&path)?;
                if package.len() != 1 {
                    return Err("machine import needs one package".to_owned());
                }
                response["files"] = package[0]["files"].clone();
            }
            Ok::<_, String>(())
        })();
        if let Err(e) = run {
            response["error"] = json!(e);
        }
        host.call(client, response)?;
    }
    Ok(())
}
