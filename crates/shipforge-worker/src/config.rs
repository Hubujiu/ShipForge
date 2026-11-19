use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Runtime {
    #[serde(default)]
    pub slash_commands: Option<Vec<String>>,
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub inherit_env: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    180 * 60
}
fn default_count() -> u64 {
    1
}
fn default_space() -> String {
    "default".into()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub tool_name: String,
    #[serde(flatten)]
    pub runtime: Runtime,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub machine_id: String,
    pub name: String,
    #[serde(default = "default_space")]
    pub space: String,
    #[serde(default = "default_count")]
    pub capacity: u64,
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub allow_direct: bool,
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub profiles: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub direct_profiles: std::collections::BTreeSet<String>,
    pub runtimes: BTreeMap<String, Runtime>,
    #[serde(default)]
    pub skill_roots: BTreeMap<String, Vec<PathBuf>>,
    #[serde(default)]
    pub approved_tools: BTreeMap<String, ToolSpec>,
}
fn abs(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).map_err(|e| e.to_string())
}
impl Config {
    pub(crate) fn permits_direct(&self, id: &str) -> bool {
        self.allow_direct || self.direct_profiles.contains(id)
    }
    pub(crate) fn profile_paths(&self) -> impl Iterator<Item = (&str, &PathBuf)> {
        self.workspace
            .iter()
            .map(|p| ("@workspace", p))
            .chain(self.profiles.iter().map(|(id, p)| (id.as_str(), p)))
    }
    pub(crate) fn approved_directories(&self) -> Vec<serde_json::Value> {
        self.profile_paths().map(|(id,path)|{
            let resolved=path.canonicalize().unwrap_or_else(|_|path.clone());
            let available=resolved.is_dir();
            serde_json::json!({"id":id,"identity":directory_identity(&resolved),"available":available,"direct":available&&self.permits_direct(id),"git":available&&resolved.join(".git").exists()})
        }).collect()
    }
    pub fn load(file: &Path) -> Result<Self> {
        let config: Self = serde_json::from_slice(
            &fs::read(file).map_err(|e| format!("read {}: {e}", file.display()))?,
        )
        .map_err(|e| format!("invalid machine JSON: {e}"))?;
        if config.machine_id.is_empty()
            || config.name.trim().is_empty()
            || config.space.trim().is_empty()
            || !(1..=64).contains(&config.capacity)
        {
            return Err("invalid machine identity, space or capacity (1–64)".into());
        }
        for tool in config.approved_tools.values() {
            if tool.tool_name.trim().is_empty() {
                return Err("an approved MCP tool needs a tool_name".into());
            }
        }
        for runtime in config
            .runtimes
            .values()
            .chain(config.approved_tools.values().map(|t| &t.runtime))
        {
            if runtime.program.as_os_str().is_empty()
                || runtime.timeout_seconds == 0
                || runtime.timeout_seconds > 30 * 86400
            {
                return Err("invalid runtime program or timeout".into());
            }
            if runtime
                .inherit_env
                .iter()
                .any(|key| key.to_ascii_uppercase().starts_with("SHIPFORGE_") || key.contains('='))
            {
                return Err(
                    "ShipForge credentials cannot be inherited by external runtimes".into(),
                );
            }
        }
        if config.profiles.contains_key("@workspace")
            || config.profiles.len() + usize::from(config.workspace.is_some()) > 128
            || config.profiles.keys().any(|id| {
                id.trim().is_empty()
                    || id.len() > 160
                    || id.contains(['/', '\\'])
                    || id.chars().any(char::is_control)
            })
        {
            return Err("use distinct logical names for approved profiles; @workspace names the default directory".into());
        }
        if config
            .workspace
            .iter()
            .chain(config.profiles.values())
            .chain(config.skill_roots.values().flatten())
            .any(|p| !p.is_absolute())
        {
            return Err("approved workspace and profile paths must be absolute".into());
        }
        Ok(config)
    }
    pub fn initialize(
        file: &Path,
        workspace: Option<PathBuf>,
        program: Option<PathBuf>,
        runtime: &str,
        args: Vec<String>,
        example: bool,
        space: String,
    ) -> Result<()> {
        if space.trim().is_empty() {
            return Err("space must not be empty".into());
        }
        let mut runtimes = BTreeMap::new();
        if let Some(program) = program {
            runtimes.insert(
                runtime.to_owned(),
                Runtime {
                    slash_commands: None,
                    program,
                    args,
                    inherit_env: Vec::new(),
                    timeout_seconds: default_timeout(),
                },
            );
        }
        if example {
            if runtimes.contains_key("inspect-context") {
                return Err("inspect-context is reserved for the example runtime".into());
            }
            runtimes.insert(
                "inspect-context".into(),
                Runtime {
                    slash_commands: None,
                    program: std::env::current_exe().map_err(|e| e.to_string())?,
                    args: vec!["inspect-context".into()],
                    inherit_env: Vec::new(),
                    timeout_seconds: 30,
                },
            );
        }
        let cfg = Config {
            machine_id: format!("machine-{}", Uuid::new_v4().simple()),
            name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or("ShipForge worker".into()),
            space,
            capacity: 1,
            shared: false,
            allow_direct: false,
            workspace: workspace.as_deref().map(abs).transpose()?,
            profiles: BTreeMap::new(),
            direct_profiles: Default::default(),
            runtimes,
            approved_tools: BTreeMap::new(),
            skill_roots: BTreeMap::new(),
        };
        let data = serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?;
        write_new(file, &data)
    }
}
pub(crate) fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}
pub fn state_dir(config: &Path) -> Result<PathBuf> {
    let name = config
        .file_name()
        .ok_or("invalid config file")?
        .to_string_lossy();
    let file = abs(config)?;
    let dir = file.with_file_name(format!("{name}.state"));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}
pub(crate) fn directory_identity(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    let value = value.to_lowercase();
    hex_digest(value.as_bytes())
}
pub fn credential(dir: &Path) -> Result<String> {
    let path = dir.join("machine.key");
    match fs::read_to_string(&path) {
        Ok(s) if s.trim().len() >= 32 => return Ok(s.trim().to_owned()),
        Ok(_) => return Err("machine key was truncated".into()),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        Err(_) => (),
    }
    let value = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    if write_new(&path, value.as_bytes()).is_err() {
        return fs::read_to_string(&path).map_err(|e| e.to_string());
    }
    Ok(value)
}
pub fn program_path(requested: &Path) -> Result<PathBuf> {
    if requested.is_absolute() {
        if !requested.is_file() {
            return Err(format!("runtime not found: {}", requested.display()));
        }
        return Ok(requested.to_path_buf());
    }
    if requested.components().count() != 1 {
        return Err("runtime paths must be absolute or a program name on PATH".into());
    }
    for root in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|path| path.is_absolute())
    {
        let candidate = root.join(requested);
        #[cfg(windows)]
        let candidate = if candidate.extension().is_none() {
            candidate.with_extension("exe")
        } else {
            candidate
        };
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("runtime not found: {}", requested.display()))
}

pub fn hex_digest(value: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
