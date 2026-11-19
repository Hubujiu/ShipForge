mod accounts;
mod bots;
mod credentials;
mod files;
mod identity;
mod network;
mod remote;
pub mod skill_files;
mod vendors;

pub use bots::qr_png;
pub use bots::qr_png as png_data_url;
pub use files::{save_attachment, upload_file};
pub use identity::link_id as identity_link_id;
pub use network::{DEVICE_OPS, device_command};
pub use shipforge_core::git_remote_url;
pub use shipforge_core::runtime_diagnostics;
pub use shipforge_core::skill_package;
pub use vendors::forge::verify_forge_hook;
pub use vendors::receive_cnb;

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
    directory: Option<PathBuf>,
    service_actor: Option<String>,
}
/// The team service authenticates the caller before delegating one integration operation.
pub fn hosted_integration(op: &str) -> bool {
    vendors::HOSTED.contains(&op)
}

pub fn execute_integration(directory: &Path, actor: &str, request: Value) -> Result<Value, String> {
    if !request["op"].as_str().is_some_and(hosted_integration) {
        return Err("invalid: unsupported hosted integration".into());
    }
    Client::integration_actor(directory, actor)?.execute(request)
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
            "access.personal.issue",
            "access.personal.list",
            "access.personal.revoke",
            "profiles.list",
            "profiles.use",
            "profiles.forget",
            "profiles.remember",
            "sync.status",
            "sync.pending",
            "sync.enqueue",
            "sync.flush",
            "sync.resolve",
            "zoho.projects",
            "zoho.preview",
            "bugly.deliver",
            "zoho.credential",
            "bugly.credentials",
            "tapd.projects",
            "tapd.preview",
            "tapd.credential",
            "zentao.credential",
            "zentao.projects",
            "zentao.preview",
            "pingcode.credential",
            "pingcode.projects",
            "pingcode.preview",
            "imports.push",
            "imports.synchronize",
        ] {
            operations.push(json!(name));
        }
        operations.extend(vendors::CLOUD_OPERATIONS.iter().map(|op| json!(op)));
        operations.extend(bots::LOCAL.iter().map(|op| json!(op)));
        operations.extend(vendors::HOSTED.iter().map(|op| json!(op)));
    }
    vendors::forge::schema(&mut schema);
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
    rules.push(
        json!({"if":{"properties":{"op":{"enum":vendors::HOSTED}}},"then":{"required":["id"]}}),
    );
    rules.push(json!({"if":{"properties":{"op":{"enum":vendors::CLOUD_OPERATIONS}}},"then":{"required":["id"],"properties":{"page_size":{"type":"integer","minimum":1,"maximum":100},"import_mode":{"enum":["history","changes","refresh"]}}}}));
    for (op, required) in [
        ("access.issue", vec!["actor", "scope"]),
        ("access.revoke", vec!["id"]),
        ("access.personal.issue", vec!["label"]),
        ("access.personal.revoke", vec!["id"]),
        ("profiles.use", vec!["id"]),
        ("profiles.forget", vec!["id"]),
        ("profiles.remember", vec!["id", "location"]),
        ("sync.enqueue", vec!["request"]),
        ("sync.resolve", vec!["id"]),
        ("zoho.projects", vec!["id"]),
        ("zoho.preview", vec!["id"]),
        ("bugly.deliver", vec!["id"]),
        ("zoho.credential", vec!["id"]),
        ("bugly.credentials", vec!["id"]),
        ("tapd.projects", vec!["id"]),
        ("tapd.preview", vec!["id"]),
        ("tapd.credential", vec!["id"]),
        ("zentao.credential", vec!["id"]),
        ("zentao.projects", vec!["id"]),
        ("zentao.preview", vec!["id"]),
        ("pingcode.credential", vec!["id"]),
        ("pingcode.projects", vec!["id"]),
        ("pingcode.preview", vec!["id"]),
        ("imports.push", vec!["id", "task_id"]),
        ("imports.synchronize", vec!["id"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":required}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"const":"access.personal.issue"}}},
        "then":{"properties":{"lifetime":{"enum":["30d","90d","1y","never"]},"source":{"enum":["personal","machine_setup"]},
        "label":{"type":"string","minLength":1,"maxLength":100}},"not":{"anyOf":[{"required":["actor"]},{"required":["scope"]}]}}}));
    bots::schema(&mut schema);
    identity::schema(&mut schema);
    network::schema(&mut schema);
    schema
}

pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("SHIPFORGE_DATA_DIR").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe()
        && let Some(program_dir) = exe.parent()
        && let Some(directory) = installed_data_dir(program_dir)?
    {
        return Ok(directory);
    }
    ProjectDirs::from("org", "ShipForge", "ShipForge Workspace")
        .map(|p| p.data_local_dir().to_owned())
        .ok_or_else(|| "storage: cannot locate a user data directory".into())
}

#[cfg(windows)]
fn installed_data_dir(program_dir: &Path) -> Result<Option<PathBuf>, String> {
    let bytes = match std::fs::read(program_dir.join("shipforge-install.ini")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "storage: cannot read installation preferences: {e}"
            ));
        }
    };
    let text = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (pairs, rest) = body.as_chunks::<2>();
        if !rest.is_empty() {
            return Err("storage: incomplete installation preferences".into());
        }
        let chars: Vec<_> = pairs.iter().copied().map(u16::from_le_bytes).collect();
        String::from_utf16(&chars).map_err(|e| format!("storage: {e}"))?
    } else {
        String::from_utf8(bytes).map_err(|e| format!("storage: {e}"))?
    };
    let mut installation = false;
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.starts_with('[') {
            installation = line.eq_ignore_ascii_case("[Installation]");
        } else if installation
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("data_dir")
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value.trim());
            if !path.is_absolute() {
                return Err("storage: installed data directory must be an absolute path".into());
            }
            return Ok(Some(path));
        }
    }
    Ok(None)
}

impl Client {
    fn integration_actor(directory: &Path, actor: &str) -> Result<Self, String> {
        let mut client = Self::local(directory)?;
        client.service_actor = Some(actor.to_owned());
        Ok(client)
    }
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
            (None, None) => match accounts::connection(&dir)? {
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
            directory: Some(directory.to_path_buf()),
            service_actor: None,
        })
    }
    pub fn memory() -> Result<Self, String> {
        Ok(Self {
            backend: Backend::Local(Store::memory()?),
            directory: None,
            service_actor: None,
        })
    }
    pub fn remote(url: &str, token: &str) -> Result<Self, String> {
        Self::remote_in(&data_dir()?, url, token)
    }
    pub fn remote_in(directory: &Path, url: &str, token: &str) -> Result<Self, String> {
        Ok(Self {
            backend: Backend::Remote(Box::new(remote::Remote::new(directory, url, token)?)),
            directory: Some(directory.to_path_buf()),
            service_actor: None,
        })
    }
    pub fn save_connection(directory: Option<&Path>, url: &str, token: &str) -> Result<(), String> {
        Self::sign_in(directory, url, token, None)
    }
    pub fn sign_in(
        directory: Option<&Path>,
        url: &str,
        token: &str,
        expected: Option<&str>,
    ) -> Result<(), String> {
        let location = match directory {
            Some(p) => p.to_owned(),
            None => data_dir()?,
        };
        let normalized = remote::endpoint(url, token)?;
        let mut client = Self::remote_in(&location, &normalized, token)?;
        let who = client.execute(json!({"op":"access.whoami"}))?;
        accounts::save(&location, &normalized, token, &who, expected)
    }
    pub fn forget_connection(directory: Option<&Path>) -> Result<(), String> {
        let location = match directory {
            Some(p) => p.to_owned(),
            None => data_dir()?,
        };
        accounts::logout(&location, None)
    }
    pub fn desktop_session(&mut self) -> Result<Value, String> {
        let Some(dir) = self.directory.clone() else {
            return Ok(Value::Null);
        };
        if std::env::var_os("SHIPFORGE_SERVER").is_some() {
            return Ok(Value::Null);
        }
        accounts::startup(&dir, self)
    }
    pub fn mode(&self) -> &'static str {
        match self.backend {
            Backend::Local(_) => "local",
            Backend::Remote(_) => "team",
        }
    }
    pub fn execute(&mut self, request: Value) -> Result<Value, String> {
        let _profile = network::ProfileScope::enter(self.directory.as_deref());
        if request["op"]
            .as_str()
            .is_some_and(|op| DEVICE_OPS.contains(&op))
        {
            let dir = self
                .directory
                .as_deref()
                .ok_or("invalid: a device profile is required")?;
            let reply = device_command(dir, &request)?;
            if request["op"] == "network.save"
                && let Backend::Remote(remote) = &mut self.backend
            {
                remote.network_changed();
            }
            return Ok(reply);
        }
        if request["op"]
            .as_str()
            .is_some_and(|op| bots::LOCAL.contains(&op))
        {
            return bots::execute(self, &request);
        }
        if let Some(op) = request["op"].as_str().filter(|op| hosted_integration(op)) {
            if let Backend::Remote(remote) = &mut self.backend {
                return remote.send(&request);
            }
            return vendors::hosted(self, op, &request);
        }
        let request = skill_files::prepare(request, self.directory.as_deref())?;
        if request["op"]
            .as_str()
            .is_some_and(|op| identity::DEVICE_OPS.contains(&op))
        {
            return identity::execute(self, &request);
        }
        if let Some(op) = request["op"]
            .as_str()
            .filter(|op| op.starts_with("profiles."))
        {
            let dir = self
                .directory
                .clone()
                .ok_or("invalid: saved accounts need a device profile")?;
            if op == "profiles.list" {
                return accounts::list(&dir);
            }
            if std::env::var_os("SHIPFORGE_SERVER").is_some() {
                return Err("invalid: this connection is controlled by SHIPFORGE_SERVER".into());
            }
            let id = request["id"]
                .as_str()
                .ok_or("invalid: expected account id")?;
            return match op {
                "profiles.use" => {
                    let (new, login) = accounts::switch(&dir, id)?;
                    *self = new;
                    Ok(login)
                }
                "profiles.forget" => {
                    accounts::logout(&dir, Some(id))?;
                    *self = Self::from_dir(Some(&dir))?;
                    accounts::list(&dir)
                }
                "profiles.remember" => accounts::remember(&dir, &request),
                _ => Err("invalid: unknown saved account operation".into()),
            };
        }
        if matches!(
            request["op"].as_str(),
            Some(
                "zoho.projects"
                    | "zoho.preview"
                    | "bugly.deliver"
                    | "zoho.credential"
                    | "bugly.credentials"
                    | "tapd.projects"
                    | "tapd.preview"
                    | "tapd.credential"
                    | "zentao.credential"
                    | "zentao.projects"
                    | "zentao.preview"
                    | "pingcode.credential"
                    | "pingcode.projects"
                    | "pingcode.preview"
                    | "imports.push"
                    | "imports.synchronize"
            )
        ) || request["op"]
            .as_str()
            .is_some_and(|op| vendors::CLOUD_OPERATIONS.contains(&op))
        {
            return vendors::execute(self, &request);
        }
        if request["op"] == "schema" {
            return Ok(schema());
        }
        match &mut self.backend {
            Backend::Local(store) => match request["op"].as_str() {
                Some("access.whoami") => Ok(
                    json!({"actor":self.service_actor.as_deref().unwrap_or("owner"),"scope":"write","mode":"local"}),
                ),
                Some("sync.status") | Some("sync.flush") => {
                    Ok(json!({"mode":"local","pending":0,"changes":[]}))
                }
                Some(op) if op.starts_with("access.") || op.starts_with("sync.") => {
                    Err("invalid: this operation needs a team connection".into())
                }
                _ => store.execute(self.service_actor.as_deref().unwrap_or("owner"), request),
            },
            Backend::Remote(remote) => remote.execute(request),
        }
    }
}
