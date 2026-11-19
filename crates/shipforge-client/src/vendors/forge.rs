//! Repository-scoped GitHub App and GitLab project integrations.
use super::{Client, Host, Result, Url, http, org_location, response, scoped, vault};
use api::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
mod api;
mod auth;
mod issues;

fn digest(v: &str) -> String {
    Sha256::digest(v.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn invoke(client: &mut Client, q: &Value, action: &str, extra: &Value) -> Result<Value> {
    let mut cmd = scoped(q, "forge.internal");
    if let Some(m) = extra.as_object() {
        for (k, v) in m {
            if !matches!(k.as_str(), "op" | "action" | "space" | "id") {
                cmd[k] = v.clone();
            }
        }
    }
    cmd["action"] = json!(action);
    client.execute(cmd)
}
fn managed_api(client: &Client, cfg: &Value, p: &str, b: &Value) -> Result<Api> {
    let api = auth::api(client, cfg, p, false)?;
    let repo = api
        .repos()?
        .into_iter()
        .find(|r| r["repository_id"] == b["repository_id"] && r["repository"] == b["repository"])
        .ok_or("forbidden: repository access was removed or renamed; review the pairing")?;
    if repo["public"] == true && b["public"] != true {
        return Err("conflict: repository became public; review the pairing before sending".into());
    }
    Ok(api)
}
fn pairing(client: &mut Client, q: &Value, cfg: &Value) -> Result<Value> {
    if cfg["can_pair"] != true {
        return Err("forbidden: administration of organization and space required".into());
    }
    if q["direction"] == "off" {
        return invoke(client, q, "pair", q);
    }
    let p = field(q, "id")?;
    let n = id(&q["repository_id"])?;
    let api = auth::api(client, cfg, p, false)?;
    let row = api
        .repos()?
        .into_iter()
        .find(|v| v["repository_id"] == n)
        .ok_or("forbidden: choose an installed repository or a project you maintain")?;
    let seed = auth::hook_seed(client, cfg, p)?;
    let space = q["space"].as_str().unwrap_or("default");
    let active = cfg["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["repository_id"] == n);
    let nonce = active
        .and_then(|v| v["id"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let data = json!([cfg["organization"], p, space, n, nonce]).to_string();
    let sig = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, seed.as_bytes()),
        data.as_bytes(),
    );
    let token = sig
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut cmd = q.clone();
    cmd["credential"] = cfg["account"]["credential"].clone();
    cmd["hook_hash"] = json!(digest(&token));
    cmd["pair_nonce"] = json!(nonce);
    cmd["host"] = json!(
        Url::parse(&api.site)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
    );
    cmd["repository"] = row["repository"].clone();
    cmd["repository_id"] = json!(n);
    cmd["public"] = row["public"].clone();
    let mut paired = invoke(client, q, "pair", &cmd)?;
    paired["webhook_path"] = json!(if p == "github" {
        "/v1/forge/github".into()
    } else {
        format!("/v1/forge/gitlab/{}", field(&paired, "id")?)
    });
    if p != "github" {
        paired["webhook_token"] = json!(token);
    }
    Ok(paired)
}
pub fn verify_forge_hook(dir: &std::path::Path, route: &Value) -> Result<()> {
    let mut client = Client::integration_actor(dir, field(route, "actor")?)?;
    let p = field(route, "platform")?;
    let cfg = invoke(
        &mut client,
        &json!({"id":p,"space":route["space"]}),
        "config",
        &Value::Null,
    )?;
    let pair = cfg["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["id"] == route["id"])
        .ok_or("not_found: inactive repository pairing")?;
    managed_api(&client, &cfg, p, pair)?;
    Ok(())
}
pub(crate) fn execute(client: &mut Client, op: &str, q: &Value) -> Result<Value> {
    let p = field(q, "id")?;
    if !matches!(p, "github" | "gitlab" | "gitlab-self") {
        return Err("invalid: select github, gitlab or gitlab-self".into());
    }
    let cfg = invoke(client, q, "config", &Value::Null)?;
    match op {
        "forge.authorize" => auth::authorize(client, q, &cfg),
        "forge.credential" => auth::credential(client, q, &cfg),
        "forge.disconnect" => auth::disconnect(client, q, &cfg),
        "forge.repositories" => {
            if cfg["can_pair"] != true {
                return Err("forbidden: organization and space administration required".into());
            }
            Ok(json!(auth::api(client, &cfg, p, false)?.repos()?))
        }
        "forge.pair" => pairing(client, q, &cfg),
        "forge.reply" | "forge.deliver" => issues::send(client, q, &cfg, op == "forge.deliver"),
        "forge.refresh" | "forge.push" => issues::sync(client, q, &cfg, op == "forge.push"),
        _ => Err("invalid: unknown repository operation".into()),
    }
}
pub(crate) fn schema(s: &mut Value) {
    for (op, req) in [
        ("forge.authorize", vec!["purpose"]),
        ("forge.credential", vec!["purpose", "token"]),
        ("forge.disconnect", vec!["purpose"]),
        ("forge.repositories", vec![]),
        ("forge.pair", vec!["direction"]),
        ("forge.reply", vec!["task_id", "pair", "body", "request_id"]),
        ("forge.deliver", vec!["pair"]),
        ("forge.refresh", vec!["pair", "task_id"]),
        ("forge.push", vec!["pair", "task_id"]),
    ] {
        let mut keys = vec!["id"];
        keys.extend(req);
        s["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":keys,"properties":{"id":{"enum":["github","gitlab","gitlab-self"]},"purpose":{"enum":["organization","personal"]},"direction":{"enum":["incoming","both","off"]},"body":{"type":"string","maxLength":60000},"backlinks":{"type":"boolean"},"pull_requests":{"type":"boolean"},"commits":{"type":"boolean"},"confirm_public":{"type":"boolean"}}}}));
    }
}
