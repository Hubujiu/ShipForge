use super::{Client, Result};
use crate::{Backend, credentials::Connection};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::PathBuf};

/// Credentials belong to a device profile, server, person and organization/remote resource.
pub(super) fn location(
    client: &Client,
    who: &Value,
    binding: &Value,
    provider: &str,
    external: &str,
) -> Result<Option<PathBuf>> {
    let Some(dir) = &client.directory else {
        return Ok(None);
    };
    let site = match &client.backend {
        Backend::Local(_) => "local",
        Backend::Remote(remote) => remote.credential_site(),
    };
    let actor = who["actor"]
        .as_str()
        .ok_or("unauthorized: current user required")?;
    let org = binding["organization"]
        .as_str()
        .ok_or("invalid: connector organization required")?;
    let digest = Sha256::digest(
        json!([site, actor, org, provider, external])
            .to_string()
            .as_bytes(),
    );
    let name = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok(Some(
        dir.join("vendor-credentials").join(format!("{name}.json")),
    ))
}

pub(super) fn read(path: &Option<PathBuf>) -> Result<Value> {
    let Some(path) = path else {
        return Ok(Value::Null);
    };
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(e) => return Err(format!("storage: {e}")),
    };
    let data: Connection =
        serde_json::from_reader(file).map_err(|_| "storage: invalid vendor credential file")?;
    serde_json::from_str(&data.secret()?).map_err(|_| "storage: invalid vendor credential".into())
}

pub(super) fn save(path: &Option<PathBuf>, values: Option<&Value>) -> Result<Value> {
    let path = path
        .as_ref()
        .ok_or("invalid: saving credentials requires a persistent device profile")?;
    let Some(values) = values else {
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("storage: {e}")),
        }
        return Ok(json!({"saved":false}));
    };
    let dir = path
        .parent()
        .ok_or("storage: missing credential directory")?;
    fs::create_dir_all(dir).map_err(|e| format!("storage: {e}"))?;
    let protected = Connection::new("vendor", &values.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("storage: {e}"))?;
    serde_json::to_writer(&mut file, &protected).map_err(|e| format!("storage: {e}"))?;
    file.flush()
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| format!("storage: {e}"))?;
    file.persist(path).map_err(|e| format!("storage: {e}"))?;
    Ok(json!({"saved":true}))
}
