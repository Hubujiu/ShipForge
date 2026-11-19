use crate::{Client, accounts, credentials::Connection, remote};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, time::Duration};
use uuid::Uuid;

pub(super) const DEVICE_OPS: &[&str] = &[
    "profiles.login.start",
    "profiles.login.finish",
    "profiles.close.start",
    "profiles.close.finish",
    "profiles.verification",
    "profiles.logout",
];
pub fn link_id(link: &str) -> Option<Uuid> {
    let id = link.strip_prefix("shipforge://identity/")?;
    let parsed = Uuid::parse_str(id).ok()?;
    (parsed.to_string() == id).then_some(parsed)
}
pub(super) fn schema(s: &mut Value) {
    let ops = s["properties"]["op"]["enum"].as_array_mut().unwrap();
    for op in DEVICE_OPS.iter().copied().chain([
        "access.account.profile",
        "access.account.update",
        "access.account.impact",
        "access.account.verify",
        "access.account.close",
        "access.account.logout",
    ]) {
        ops.push(json!(op));
    }
    let rules = s["allOf"].as_array_mut().unwrap();
    for (op, fields) in [
        ("profiles.login.start", vec!["url", "email"]),
        ("profiles.login.finish", vec!["id"]),
        ("profiles.close.finish", vec!["id", "confirmation"]),
        ("access.account.update", vec!["expected_rev"]),
        ("access.account.close", vec!["id", "device", "confirmation"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":fields}}));
    }
    rules.push(json!({"if":{"properties":{"op":{"enum":["profiles.close.finish","access.account.close"]}}},"then":{"properties":{"confirmation":{"const":"注销"},"dissolve":{"type":"boolean"}}}}));
    rules.push(json!({"if":{"properties":{"op":{"const":"profiles.verification"}}},"then":{"required":["id"]}}));
}
fn required<'a>(q: &'a Value, key: &str) -> Result<&'a str, String> {
    q[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("invalid: missing {key}"))
}
fn post(address: &str, suffix: &str, q: &Value) -> Result<Value, String> {
    let agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .new_agent();
    let mut response = agent
        .post(format!("{address}/v1/identity/{suffix}"))
        .send_json(q)
        .map_err(|e| format!("offline: {e}"))?;
    let reply: Value = response
        .body_mut()
        .with_config()
        .limit(32 * 1024)
        .read_json()
        .map_err(|_| "server: invalid login response")?;
    if reply["ok"] == true {
        return Ok(reply["data"].clone());
    }
    Err(format!(
        "{}: {}",
        reply["error"]["code"].as_str().unwrap_or("server"),
        reply["error"]["message"].as_str().unwrap_or("login failed")
    ))
}
#[derive(Deserialize, Serialize)]
struct Pending {
    credential: Connection,
    profile: Option<String>,
    closing: bool,
    pair: String,
    dissolve: bool,
}
fn profile(book: &Value, id: &str) -> Result<Value, String> {
    book["accounts"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["id"] == id))
        .cloned()
        .ok_or_else(|| "not_found: saved account".into())
}
fn saved(dir: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    let id = Uuid::parse_str(id).map_err(|_| "invalid: invalid verification ID")?;
    Ok(dir.join("pending-identity").join(format!("{id}.json")))
}
fn retain(
    dir: &Path,
    mut result: Value,
    address: &str,
    profile: Option<String>,
    closing: bool,
    dissolve: bool,
) -> Result<Value, String> {
    let pending = Pending {
        credential: Connection::new(address, required(&result, "device")?)?,
        profile,
        closing,
        pair: required(&result, "pair")?.to_owned(),
        dissolve,
    };
    let target = saved(dir, required(&result, "id")?)?;
    let parent = target.parent().unwrap();
    fs::create_dir_all(parent).map_err(|e| format!("storage: {e}"))?;
    if let Ok(all) = fs::read_dir(parent) {
        for file in all.flatten() {
            if file.file_type().is_ok_and(|k| k.is_file())
                && file.path().extension().is_some_and(|e| e == "json")
                && file
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().is_ok_and(|a| a > Duration::from_secs(3600)))
            {
                let _ = fs::remove_file(file.path());
            }
        }
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("storage: {e}"))?;
    serde_json::to_writer(&mut temp, &pending).map_err(|e| format!("storage: {e}"))?;
    temp.flush().map_err(|e| format!("storage: {e}"))?;
    temp.persist_noclobber(target)
        .map_err(|e| format!("storage: {e}"))?;
    result.as_object_mut().unwrap().remove("device");
    Ok(result)
}
pub(super) fn execute(client: &mut Client, q: &Value) -> Result<Value, String> {
    if std::env::var_os("SHIPFORGE_SERVER").is_some() {
        return Err("invalid: use a device profile for mailbox login".into());
    }
    let dir = client
        .directory
        .clone()
        .ok_or("invalid: email login needs a device profile")?;
    let book = accounts::list(&dir)?;
    let op = required(q, "op")?;
    if op == "profiles.login.start" {
        let address = remote::endpoint(required(q, "url")?, "login")?;
        let expected = q["account"].as_str();
        let actor = if let Some(id) = expected {
            let saved = profile(&book, id)?;
            if saved["url"] != address || saved["email_account"] != true {
                return Err("forbidden: use the saved email account and its server".into());
            }
            Some(required(&saved, "actor")?.to_owned())
        } else {
            None
        };
        let result = post(
            &address,
            "start",
            &json!({"email":required(q,"email")?,"account":actor}),
        )?;
        return retain(
            &dir,
            result,
            &address,
            expected.map(str::to_owned),
            false,
            false,
        );
    }
    if matches!(op, "profiles.close.start" | "profiles.logout") {
        let active = required(&book, "active")?;
        let account = profile(&book, active)?;
        let who = client.execute(json!({"op":"access.whoami"}))?;
        if who["mode"] != "team" || who["actor"] != account["actor"] {
            return Err("conflict: active account changed; reopen the account list".into());
        }
        if op == "profiles.logout" {
            client.execute(json!({"op":"access.account.logout"}))?;
            accounts::logout(&dir, Some(active))?;
            *client = Client::from_dir(Some(&dir))?;
            return Ok(json!({"logged_out":true}));
        }
        let result = client.execute(json!({"op":"access.account.verify"}))?;
        return retain(
            &dir,
            result,
            required(&account, "url")?,
            Some(active.to_string()),
            true,
            q["dissolve"] == true,
        );
    }
    let id = required(q, "id")?;
    let target = saved(&dir, id)?;
    let pending: Pending = serde_json::from_slice(
        &fs::read(&target).map_err(|_| "not_found: verification request on this device")?,
    )
    .map_err(|_| "storage: invalid verification request")?;
    let expected = pending.profile.as_deref();
    if op == "profiles.verification" {
        if pending.closing && book["active"].as_str() != expected {
            return Err("forbidden: switch back to the account requesting closure".into());
        }
        if fs::metadata(target)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|a| a > Duration::from_secs(600)))
        {
            return Err("invalid: email verification expired".into());
        }
        return Ok(
            json!({"id":id,"pair":pending.pair,"closing":pending.closing,"dissolve":pending.dissolve}),
        );
    }
    if pending.closing != (op == "profiles.close.finish") {
        return Err("invalid: verification purpose mismatch".into());
    }
    let credential = pending.credential.secret()?;
    let result = if pending.closing {
        if book["active"].as_str() != expected {
            return Err("forbidden: switch back to the account requesting closure".into());
        }
        if q["confirmation"] != "注销" {
            return Err("invalid: type 注销 to permanently close the account".into());
        }
        let response=client.execute(json!({"op":"access.account.close","id":id,"device":credential,"confirmation":q["confirmation"],"dissolve":q["dissolve"]}))?;
        accounts::logout(&dir, expected)?;
        *client = Client::from_dir(Some(&dir))?;
        response
    } else {
        let address = &pending.credential.url;
        let reply = post(address, "finish", &json!({"id":id,"device":credential}))?;
        let token = required(&reply, "token")?;
        if let Err(error) = Client::sign_in(Some(&dir), address, token, expected) {
            if let Ok(mut rejected) = Client::remote_in(&dir, address, token) {
                let _ = rejected.execute(json!({"op":"access.account.logout"}));
            }
            return Err(error);
        }
        *client = Client::from_dir(Some(&dir))?;
        json!({"actor":reply["actor"],"signed_in":true})
    };
    let _ = fs::remove_file(target);
    Ok(result)
}
