use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub(crate) struct Connection {
    pub url: String,
    token: Vec<u8>,
}

impl Connection {
    pub fn read(directory: &Path) -> Result<Option<(String, String)>, String> {
        let path = directory.join("connection.json");
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("storage: {e}")),
        };
        let saved: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("storage: invalid connection file: {e}"))?;
        let token = String::from_utf8(unseal(&saved.token)?)
            .map_err(|_| "storage: invalid saved credential".to_string())?;
        Ok(Some((saved.url, token)))
    }

    pub fn save(directory: &Path, url: &str, token: &str) -> Result<(), String> {
        fs::create_dir_all(directory).map_err(|e| format!("storage: {e}"))?;
        let saved = Self {
            url: url.to_owned(),
            token: seal(token.as_bytes())?,
        };
        let mut file =
            tempfile::NamedTempFile::new_in(directory).map_err(|e| format!("storage: {e}"))?;
        let bytes = serde_json::to_vec(&saved).map_err(|e| format!("storage: {e}"))?;
        file.write_all(&bytes)
            .and_then(|()| file.as_file().sync_all())
            .map_err(|e| format!("storage: {e}"))?;
        file.persist(directory.join("connection.json"))
            .map_err(|e| format!("storage: {e}"))?;
        Ok(())
    }
}

#[cfg(windows)]
fn seal(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(data, true)
}
#[cfg(windows)]
fn unseal(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(data, false)
}

#[cfg(windows)]
fn dpapi(data: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len())
            .map_err(|_| "invalid: credential too long".to_string())?,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // Windows reads the input during the call and allocates a result for LocalFree.
    let success = unsafe {
        if protect {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(format!(
            "storage: Windows credential protection: {}",
            std::io::Error::last_os_error()
        ));
    }
    if output.pbData.is_null() {
        return Err("storage: Windows returned an empty credential".into());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

// NamedTempFile creates the per-user connection file with mode 0600 on Unix.
#[cfg(not(windows))]
fn seal(data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}
#[cfg(not(windows))]
fn unseal(data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}
