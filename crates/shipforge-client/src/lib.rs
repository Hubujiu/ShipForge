mod credentials;
mod remote;

use directories::ProjectDirs;
use serde_json::{Value, json};
use shipforge_core::Store;
use std::path::{Path, PathBuf};

enum Backend {
    Local(Store),
    Remote(Box<remote::Remote>),
}

pub struct Client {
    backend: Backend,
}

/// Complete request schema at the desktop / CLI boundary, including transport commands.
pub fn schema() -> Value {
    let mut schema = shipforge_core::schema();
    if let Some(operations) = schema["properties"]["op"]["enum"].as_array_mut() {
        for name in [
            "access.whoami",
            "access.issue",
            "access.list",
            "access.revoke",
            "sync.status",
            "sync.pending",
            "sync.enqueue",
            "sync.flush",
            "sync.resolve",
        ] {
            operations.push(json!(name));
        }
    }
    let props = schema["properties"]
        .as_object_mut()
        .expect("core schema properties");
    props.insert(
        "scope".into(),
        json!({"enum":["read","write","worker","hook"]}),
    );
    props.insert("actor".into(), json!({"type":"string"}));
    props.insert("allow_cached".into(), json!({"type":"boolean"}));
    props.insert("queue_if_offline".into(), json!({"type":"boolean"}));
    let rules = schema["allOf"].as_array_mut().expect("core schema rules");
    for (op, required) in [
        ("access.issue", vec!["actor", "scope"]),
        ("access.revoke", vec!["id"]),
        ("sync.enqueue", vec!["request"]),
        ("sync.resolve", vec!["id"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":required}}));
    }
    schema
}

pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("SHIPFORGE_DATA_DIR").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    ProjectDirs::from("org", "ShipForge", "ShipForge Workspace")
        .map(|p| p.data_local_dir().to_owned())
        .ok_or_else(|| "storage: cannot locate a user data directory".into())
}

impl Client {
    pub fn from_env() -> Result<Self, String> {
        Self::from_dir(None)
    }
    pub fn from_dir(directory: Option<&Path>) -> Result<Self, String> {
        let dir = match directory {
            Some(p) => p.to_owned(),
            None => data_dir()?,
        };
        match (
            std::env::var("SHIPFORGE_SERVER").ok(),
            std::env::var("SHIPFORGE_TOKEN").ok(),
        ) {
            (Some(url), Some(token)) => Self::remote_in(&dir, &url, &token),
            (None, None) => match credentials::Connection::read(&dir)? {
                Some((url, token)) => Self::remote_in(&dir, &url, &token),
                None => Self::local(&dir),
            },
            _ => Err(
                "invalid: SHIPFORGE_SERVER and SHIPFORGE_TOKEN must be supplied together".into(),
            ),
        }
    }
    pub fn local(directory: &Path) -> Result<Self, String> {
        Ok(Self {
            backend: Backend::Local(Store::open(&directory.join("workspace.sqlite3"))?),
        })
    }
    pub fn memory() -> Result<Self, String> {
        Ok(Self {
            backend: Backend::Local(Store::memory()?),
        })
    }
    pub fn remote(url: &str, token: &str) -> Result<Self, String> {
        Self::remote_in(&data_dir()?, url, token)
    }
    pub fn remote_in(directory: &Path, url: &str, token: &str) -> Result<Self, String> {
        Ok(Self {
            backend: Backend::Remote(Box::new(remote::Remote::new(directory, url, token)?)),
        })
    }
    pub fn save_connection(directory: Option<&Path>, url: &str, token: &str) -> Result<(), String> {
        let location = match directory {
            Some(p) => p.to_owned(),
            None => data_dir()?,
        };
        let normalized = remote::endpoint(url, token)?;
        let mut client = Self::remote_in(&location, &normalized, token)?;
        client.execute(json!({"op":"access.whoami"}))?;
        credentials::Connection::save(&location, &normalized, token)
    }
    pub fn forget_connection(directory: Option<&Path>) -> Result<(), String> {
        let location = match directory {
            Some(p) => p.to_owned(),
            None => data_dir()?,
        };
        match std::fs::remove_file(location.join("connection.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("storage: {e}")),
        }
    }
    pub fn mode(&self) -> &'static str {
        match self.backend {
            Backend::Local(_) => "local",
            Backend::Remote(_) => "team",
        }
    }
    pub fn execute(&mut self, request: Value) -> Result<Value, String> {
        if request["op"] == "schema" {
            return Ok(schema());
        }
        match &mut self.backend {
            Backend::Local(store) => match request["op"].as_str() {
                Some("access.whoami") => {
                    Ok(json!({"actor":"owner","scope":"write","mode":"local"}))
                }
                Some("sync.status") | Some("sync.flush") => {
                    Ok(json!({"mode":"local","pending":0,"changes":[]}))
                }
                Some(op) if op.starts_with("access.") || op.starts_with("sync.") => {
                    Err("invalid: this operation needs a team connection".into())
                }
                _ => store.execute("owner", request),
            },
            Backend::Remote(remote) => remote.execute(request),
        }
    }
}
