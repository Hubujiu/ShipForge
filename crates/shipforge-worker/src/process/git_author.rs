use super::*;
use std::{collections::BTreeMap, ffi::OsString};

const PLATFORM_EMAIL: &str = "shipforge@users.noreply.github.com";
const HOOKS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "post-rewrite",
    "post-index-change",
    "reference-transaction",
    "pre-auto-gc",
    "pre-receive",
    "update",
    "post-receive",
    "post-update",
    "push-to-checkout",
    "proc-receive",
    "sendemail-validate",
    "p4-pre-submit",
    "fsmonitor-watchman",
];
const SHIM: &str = r#"#!/bin/sh
name=${0##*/}
export GIT_CONFIG_COUNT="$SHIPFORGE_GIT_CONFIG_BASE"
hooks=$(git config --path --get core.hooksPath 2>/dev/null) ||
    hooks=$(git rev-parse --path-format=absolute --git-path hooks 2>/dev/null) || exit 1
if [ -x "$hooks/$name" ]; then
    if [ "$name" != prepare-commit-msg ]; then exec "$hooks/$name" "$@"; fi
    "$hooks/$name" "$@" || exit $?
fi
if [ "$name" = prepare-commit-msg ] && ! git interpret-trailers --parse -- "$1" | grep -Fxiq 'Co-authored-by: ShipForge <shipforge@users.noreply.github.com>'; then
    git interpret-trailers --in-place --trailer 'Co-authored-by: ShipForge <shipforge@users.noreply.github.com>' -- "$1"
fi
"#;

#[derive(Default)]
pub(super) struct Environment {
    pub(super) values: BTreeMap<OsString, OsString>,
    _hooks: Option<tempfile::TempDir>,
}

fn get(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(program_path(Path::new("git")).ok()?)
        .arg("-C")
        .arg(dunce::simplified(dir))
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|v| !v.is_empty() && !v.chars().any(|c| c.is_control() || matches!(c, '<' | '>')))
}

fn inherited_value(git_path: &Path, field: &str, local: bool, value: &Value) -> Option<String> {
    let key = format!("user.{field}");
    let from_local = local
        .then(|| get(git_path, &["config", "--local", "--get", &key]))
        .flatten();
    from_local
        .or_else(|| value.as_str().map(str::to_owned))
        .or_else(|| get(git_path, &["config", "--get", &key]))
}

fn prepare_shims(state: &Path) -> Result<tempfile::TempDir> {
    let dir = tempfile::Builder::new()
        .prefix("git-hooks-")
        .tempdir_in(state)
        .map_err(|e| e.to_string())?;
    for hook in HOOKS {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o700);
        }
        opts.open(dir.path().join(hook))
            .and_then(|mut file| file.write_all(SHIM.as_bytes()))
            .map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

pub(super) fn prepare(
    workspace: &WorkingDir,
    state: &Path,
    settings: &Value,
    remote: &Value,
    inherit_env: &[String],
) -> Result<Environment> {
    if settings.is_null() {
        return Ok(Environment::default());
    }
    let directory = &workspace.path;
    let in_git =
        get(directory, &["rev-parse", "--is-inside-work-tree"]).is_some_and(|v| v == "true");
    let mut env = Environment::default();
    let has_remote = remote.as_array().is_some_and(|items| !items.is_empty());
    if !in_git && !has_remote {
        return Ok(env);
    }
    let mut author = |key: &str, field: &str, fallback: &str| {
        let value = inherited_value(
            directory,
            field,
            workspace.local_repository,
            &settings[field],
        )
        .unwrap_or_else(|| fallback.to_owned());
        env.values.insert(key.into(), value.into());
    };
    author("GIT_AUTHOR_NAME", "name", "ShipForge Agent");
    author("GIT_AUTHOR_EMAIL", "email", PLATFORM_EMAIL);
    if settings["coauthor"] == true {
        let hooks = prepare_shims(state)?;
        let base = if inherit_env
            .iter()
            .any(|k| k.eq_ignore_ascii_case("GIT_CONFIG_COUNT"))
        {
            std::env::var("GIT_CONFIG_COUNT")
                .ok()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
        } else {
            0
        };
        let count = base
            .checked_add(1)
            .ok_or("invalid Git configuration count")?;
        env.values.extend([
            ("SHIPFORGE_GIT_CONFIG_BASE".into(), base.to_string().into()),
            ("GIT_CONFIG_COUNT".into(), count.to_string().into()),
            (
                format!("GIT_CONFIG_KEY_{base}").into(),
                "core.hooksPath".into(),
            ),
            (
                format!("GIT_CONFIG_VALUE_{base}").into(),
                dunce::simplified(hooks.path()).as_os_str().to_owned(),
            ),
        ]);
        env._hooks = Some(hooks);
    }
    Ok(env)
}
