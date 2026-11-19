use super::*;
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
                    || e.contains("QQ credentials rejected") =>
            {
                return Err(e);
            }
            Err(_) => delay = (delay * 2).min(45),
        }
        std::thread::sleep(std::time::Duration::from_secs(delay));
    }
}
