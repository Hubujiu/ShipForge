use super::*;
use shipforge_client::skill_package;
use tempfile::TempDir;

/// Each runtime receives the exact version queued for it, outside the user's bound source directory.
pub(super) fn stage(state: &Path, context: &mut Value) -> Result<Option<TempDir>> {
    let versions = &context["fields"]["config"]["skill_versions"];
    let Some(skills) = versions.as_array().filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let root = tempfile::Builder::new()
        .prefix("run-skills-")
        .tempdir_in(state)
        .map_err(|e| e.to_string())?;
    let mut dirs = Vec::new();
    for (index, skill) in skills.iter().enumerate() {
        let dir = root.path().join(index.to_string());
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        if let Some(files) = skill.get("files").filter(|v| !v.is_null()) {
            skill_package::validate(files)?;
            for file in files.as_array().unwrap() {
                let target = dir.join(file["path"].as_str().unwrap());
                let parent = target.parent().ok_or("missing skill directory")?;
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                crate::config::write_new(&target, file["text"].as_str().unwrap().as_bytes())?;
            }
        } else {
            let title = skill["title"].as_str().unwrap_or("Skill");
            let body = skill["instructions"].as_str().unwrap_or("");
            crate::config::write_new(
                &dir.join("SKILL.md"),
                format!("# {title}\n\n{body}\n").as_bytes(),
            )?;
        }
        dirs.push(json!({"id":skill["id"],"rev":skill["rev"],"version":skill["version"],"entry_file":dir.join("SKILL.md"),"directory":dir}));
    }
    context["fields"]["skill_directories"] = json!(dirs);
    Ok(Some(root))
}
