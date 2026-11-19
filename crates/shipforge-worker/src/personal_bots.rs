use super::*;
pub(super) fn stage_inputs(
    state: &Path,
    context: &mut serde_json::Value,
) -> Result<Option<tempfile::TempDir>> {
    use base64::Engine;
    let Some(files) = context
        .get_mut("fields")
        .and_then(|f| f.get_mut("chat_context"))
        .and_then(|f| f.get_mut("messages"))
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|msgs| msgs.last_mut())
        .and_then(|msg| msg.get_mut("input_files"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(None);
    };
    if files.len() > 6 {
        return Err("too many incoming message files".into());
    }
    let directory = tempfile::Builder::new()
        .prefix("message-files-")
        .tempdir_in(state)
        .map_err(|e| e.to_string())?;
    let mut total = 0;
    for (index, file) in files.iter_mut().enumerate() {
        let encoded = file
            .as_object_mut()
            .and_then(|f| f.remove("content_base64"))
            .ok_or("missing message file content")?;
        let encoded = encoded
            .as_str()
            .filter(|s| s.len() <= (4 * 1024 * 1024usize).div_ceil(3) * 4)
            .ok_or("message file is too large")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "invalid message file")?;
        total += bytes.len();
        if total > 4 * 1024 * 1024 {
            return Err("message files exceed 4 MiB".into());
        }
        let suffix = file["filename"]
            .as_str()
            .and_then(|s| s.rsplit_once('.'))
            .map(|(_, ext)| ext)
            .filter(|e| {
                !e.is_empty() && e.len() <= 8 && e.bytes().all(|c| c.is_ascii_alphanumeric())
            })
            .unwrap_or("bin");
        let path = directory
            .path()
            .join(format!("input-{}.{suffix}", index + 1));
        crate::config::write_new(&path, &bytes)?;
        file["local_path"] = json!(path);
    }
    Ok(Some(directory))
}
pub(super) fn run(
    dir: Option<&Path>,
    space: &str,
    id: &str,
    watch: bool,
) -> Result<serde_json::Value> {
    let mut client = Client::from_dir(dir)?;
    let q = json!({"op":"bot.sync","space":space,"id":id});
    if !watch {
        return client.execute(q);
    }
    let id = uuid::Uuid::parse_str(id).map_err(|_| "invalid: binding must be a UUID")?;
    let directory = match dir {
        Some(dir) => dir.to_owned(),
        None => shipforge_client::data_dir()?,
    };
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(false)
        .create(true)
        .open(directory.join(format!("personal-bot-{id}.lock")))
        .map_err(|e| e.to_string())?;
    if lock.try_lock().is_err() {
        return Ok(json!({"id":id.to_string(),"state":"already_running"}));
    }
    let mut delay = 1;
    loop {
        match client.execute(q.clone()) {
            Ok(_) => delay = 1,
            Err(e)
                if e.starts_with("forbidden:")
                    || e.starts_with("not_found:")
                    || e.starts_with("unauthorized:")
                    || e.contains("disconnected or replaced")
                    || e.contains("WeChat session expired")
                    || e.contains("QQ credentials rejected")
                    || e.contains("Lark application revoked")
                    || e.contains("WeCom authorization rejected")
                    || e.contains("WeCom session replaced") =>
            {
                return Err(e);
            }
            Err(_) => delay = (delay * 2).min(45),
        }
        std::thread::sleep(std::time::Duration::from_secs(delay));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_files_exist_during_the_run_and_leave_no_copies_in_the_workspace() {
        let state = tempfile::tempdir().unwrap();
        let mut run = json!({"fields":{"chat_context":{"messages":[{"input_files":[{"filename":"../../original.png","content_base64":"aGVsbG8="}]}]}}});
        let inputs = stage_inputs(state.path(), &mut run).unwrap().unwrap();
        let file = &run["fields"]["chat_context"]["messages"][0]["input_files"][0];
        let path = std::path::Path::new(file["local_path"].as_str().unwrap());
        assert_eq!(path.parent(), Some(inputs.path()));
        assert_eq!(std::fs::read(path).unwrap(), b"hello");
        assert!(file.get("content_base64").is_none());
        drop(inputs);
        assert!(!path.exists());
        let mut other = json!({"fields":{"runtime":"test"}});
        let original = other.clone();
        assert!(stage_inputs(state.path(), &mut other).unwrap().is_none());
        assert_eq!(other, original);
    }
}
