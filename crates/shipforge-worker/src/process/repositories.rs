use super::*;
use crate::config::hex_digest;
use serde::Deserialize;

#[derive(Deserialize)]
struct Repo {
    url: String,
    branch: Option<String>,
}

pub(super) fn fingerprint(settings: &Value) -> Value {
    json!(
        settings
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| json!([item["url"], item["branch"]]))
            .collect::<Vec<_>>()
    )
}

pub(super) fn checkout(
    state: &Path,
    path: &Path,
    settings: &Value,
    pulse: &mut impl FnMut() -> Result<bool>,
    until: Instant,
) -> Result<()> {
    let repos: Vec<Repo> = serde_json::from_value(settings.clone())
        .map_err(|_| "invalid Git repository configuration from workspace")?;
    let cache = state.join("git-cache");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let cache = crew_workspace::checked_dir(&cache)?;
    let multi = repos.len() > 1;
    if multi {
        fs::create_dir(path).map_err(|e| e.to_string())?;
    }
    let branch = format!("shipforge/{}", Uuid::new_v4().simple());
    for repo in repos {
        let key = hex_digest(repo.url.as_bytes());
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join(format!("{key}.lock")))
            .map_err(|e| e.to_string())?;
        wait(&lock, pulse, until)?;
        crate::host::mark_cache_used(state, &key)?;
        let source = cache.join(&key);
        if !source.exists() {
            let mut init = command()?;
            init.args(["init", "--bare"])
                .arg(dunce::simplified(&source));
            run(&mut init, pulse, until, "initialize repository cache")?;
        }
        let source = crew_workspace::checked_dir(&source)?;
        run(
            at(&source)?
                .args(["config", "remote.origin.url"])
                .arg(&repo.url),
            pulse,
            until,
            "configure Git remote",
        )?;
        run(
            at(&source)?.args([
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ]),
            pulse,
            until,
            "configure Git tracking",
        )?;
        let mut fetch = at(&source)?;
        if repo.url.starts_with("http://") {
            fetch
                .env("GIT_ALLOW_PROTOCOL", "http")
                .args(["-c", "http.followRedirects=false"]);
        }
        run(
            fetch.args([
                "fetch",
                "--atomic",
                "--prune",
                "--tags",
                "origin",
                "+refs/heads/*:refs/remotes/origin/*",
                "+HEAD:refs/shipforge/default",
            ]),
            pulse,
            until,
            "fetch Git repository",
        )?;
        let reference = repo.branch.as_ref().map_or_else(
            || "refs/shipforge/default".to_owned(),
            |branch| format!("refs/remotes/origin/{branch}"),
        );
        let output = if multi {
            let name = repo
                .url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("repository");
            let name = name
                .trim_end_matches(".git")
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
                .take(48)
                .collect::<String>();
            path.join(format!("{name}-{}", &key[..10]))
        } else {
            path.to_owned()
        };
        run(
            at(&source)?
                .args(["worktree", "add", "-b", &branch])
                .arg(dunce::simplified(&output))
                .arg(reference),
            pulse,
            until,
            "check out Git branch",
        )?;
    }
    Ok(())
}

pub(super) fn wait(
    file: &File,
    pulse: &mut impl FnMut() -> Result<bool>,
    until: Instant,
) -> Result<()> {
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < until => {
                if pulse()? {
                    return Err("cancelled".into());
                }
                thread::sleep(Duration::from_millis(75));
            }
            Err(e) => return Err(format!("wait for Git repository cache: {e}")),
        }
    }
}

fn command() -> Result<Command> {
    let mut command = Command::new(program_path(Path::new("git"))?);
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "https:ssh")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    Ok(command)
}

fn at(source: &Path) -> Result<Command> {
    let mut git = command()?;
    git.arg("--git-dir").arg(dunce::simplified(source));
    Ok(git)
}

fn run(
    command: &mut Command,
    pulse: &mut impl FnMut() -> Result<bool>,
    until: Instant,
    action: &str,
) -> Result<()> {
    if pulse()? {
        return Err("cancelled".into());
    }
    if Instant::now() >= until {
        return Err(format!("{action}: runtime timeout"));
    }
    let mut child = command.spawn().map_err(|e| format!("{action}: {e}"))?;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(format!(
                    "{action} failed. Check the machine's Git access and the requested branch."
                ));
            }
            Ok(None) => {}
            Err(error) => break format!("{action}: {error}"),
        }
        match pulse() {
            Ok(false) if Instant::now() < until => thread::sleep(Duration::from_millis(75)),
            Ok(false) => break format!("{action}: runtime timeout"),
            Ok(true) => break "cancelled".into(),
            Err(error) => break error,
        }
    };
    stop(&mut child);
    let _ = child.wait();
    Err(result)
}
