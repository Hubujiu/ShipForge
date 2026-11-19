use super::*;
use std::collections::HashSet;
use url::Url;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Repository {
    pub(super) url: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    branch: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Inclusion {
    None,
    All,
    Selected { urls: Vec<String> },
}

pub(super) fn remote(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.len() > 2048 || raw.contains(['\\', '\n', '\r', '\t']) {
        return Err(invalid("invalid Git repository URL"));
    }
    let ssh;
    let shorthand = !raw.contains("://");
    let address = if !shorthand {
        raw
    } else {
        let split = if raw.contains("@[") {
            raw.find("]:").map(|end| raw.split_at(end + 1))
        } else {
            raw.find(':').map(|end| raw.split_at(end))
        };
        let (host, path) = split
            .filter(|(host, path)| host.contains('@') && !host.contains('/') && path.len() > 1)
            .ok_or_else(|| invalid("enter an HTTPS or SSH Git repository URL"))?;
        ssh = format!("ssh://{host}/{}", path[1..].trim_start_matches('/'));
        &ssh
    };
    let url = Url::parse(address).map_err(|_| invalid("invalid Git repository URL"))?;
    let host = url
        .host_str()
        .ok_or_else(|| invalid("Git repository host is required"))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !(matches!(url.scheme(), "https" | "ssh") || (url.scheme() == "http" && loopback))
        || url.password().is_some()
        || (url.scheme() != "ssh" && !url.username().is_empty())
        || (!url.username().is_empty()
            && !url
                .username()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
        || url.username().starts_with('-')
        || host.starts_with('-')
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().trim_matches('/').is_empty()
    {
        return Err(invalid(
            "use an HTTPS or SSH repository URL without embedded credentials",
        ));
    }
    // The shorthand's path is relative to the remote account unless it begins with a slash.
    Ok(if shorthand {
        raw.to_owned()
    } else {
        url.into()
    })
}

pub(super) fn branch(value: &Value) -> Result<&str> {
    let branch = value.as_str().filter(|s| {
        (1..=255).contains(&s.len())
            && !s.starts_with('-')
            && !s.ends_with('.')
            && !matches!(*s, "HEAD" | "@")
            && !s.contains("..")
            && !s.contains("@{")
            && !s
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
            && s.split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
    });
    branch.ok_or_else(|| invalid("invalid Git branch name"))
}

pub(super) fn normalize(repo: &mut [Repository], include: Option<&mut Inclusion>) -> Result<()> {
    if repo.len() > 24 {
        return Err(invalid("a scope can bind up to 24 Git repositories"));
    }
    let mut seen = HashSet::new();
    for item in repo {
        item.url = remote(&item.url)?;
        if item.description.len() > 1024 || !seen.insert(item.url.clone()) {
            return Err(invalid(
                "each Git repository needs a unique URL and short description",
            ));
        }
        if let Some(name) = &item.branch {
            branch(&json!(name))?;
        }
    }
    if let Some(Inclusion::Selected { urls }) = include {
        let mut selected = HashSet::new();
        for url in urls {
            *url = remote(url)?;
            if !selected.insert(url.clone()) {
                return Err(invalid("select each inherited repository once"));
            }
        }
    }
    Ok(())
}

pub(super) fn resolve(
    space: &[Repository],
    agent: &[Repository],
    include: Option<&Inclusion>,
    checkout: Option<&str>,
) -> Vec<Repository> {
    let mut output = agent.to_owned();
    for source in space {
        if let Some(item) = output.iter_mut().find(|item| item.url == source.url) {
            if item.branch.is_none() {
                item.branch.clone_from(&source.branch);
            }
        } else if match include {
            None => agent.is_empty(),
            Some(Inclusion::None) => false,
            Some(Inclusion::All) => true,
            Some(Inclusion::Selected { urls }) => urls.contains(&source.url),
        } {
            output.push(source.clone());
        }
    }
    if let Some(branch) = checkout {
        for item in &mut output {
            item.branch = Some(branch.to_owned());
        }
    }
    output
}
