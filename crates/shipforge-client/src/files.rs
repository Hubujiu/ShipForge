use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

const MAX_FILE: usize = 4 * 1024 * 1024;

/// Read a local file for the shared attachment API. The caller supplies kind, space and ID.
pub fn upload_file(path: &Path) -> Result<Value, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid: valid file name required")?;
    let mut content = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_FILE as u64 + 1).read_to_end(&mut content))
        .map_err(|e| format!("storage: {e}"))?;
    if content.len() > MAX_FILE {
        return Err("invalid: maximum attachment size is 4 MiB".into());
    }
    Ok(json!({"op":"attachment.upload","filename":name,"content_base64":STANDARD.encode(&content)}))
}

/// Publish a verified download at the user-selected destination, preserving any existing file.
pub fn save_attachment(path: &Path, response: &Value) -> Result<(), String> {
    let encoded = response["content_base64"]
        .as_str()
        .ok_or("invalid: attachment response has no content")?;
    if encoded.len() > MAX_FILE.div_ceil(3) * 4 {
        return Err("invalid: maximum attachment size is 4 MiB".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| format!("invalid: {e}"))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if bytes.len() > MAX_FILE
        || response["bytes"].as_u64() != Some(bytes.len() as u64)
        || response["sha256"].as_str() != Some(&hash)
    {
        return Err("conflict: attachment size or checksum mismatch".into());
    }
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|e| format!("storage: {e}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| format!("storage: {e}"))?;
    file.persist_noclobber(path)
        .map_err(|e| format!("storage: {e}"))?;
    Ok(())
}
