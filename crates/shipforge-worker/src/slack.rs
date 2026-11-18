use crate::config::Result;
use serde_json::{Value, json};
use shipforge_client::Client;
use std::time::Duration;
use ureq::http::Response;

#[cfg(test)]
mod tests;

struct Slack<'a> {
    endpoint: &'a str,
    token: &'a str,
    http: ureq::Agent,
}
impl<'a> Slack<'a> {
    fn new(endpoint: &'a str, token: &'a str) -> Self {
        let http = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self {
            endpoint,
            token,
            http,
        }
    }
    fn post(&self, method: &str, body: Value) -> Result<Response<ureq::Body>> {
        self.http
            .post(format!("{}/{method}", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.token))
            .send_json(body)
            .map_err(|_| "Slack transport failed".to_owned())
    }
    fn identity(&self, fields: &Value) -> Result<()> {
        let mut res = self.post("auth.test", json!({}))?;
        if !res.status().is_success() {
            return Err(format!(
                "Slack auth.test returned {}",
                res.status().as_u16()
            ));
        }
        let data: Value = res
            .body_mut()
            .with_config()
            .limit(65536)
            .read_json()
            .map_err(|_| "invalid Slack auth.test response")?;
        if data["ok"] != true
            || data["team_id"] != fields["workspace_id"]
            || data["bot_id"] != fields["bot_id"]
        {
            return Err(
                "Slack credential does not identify the configured workspace and bot".into(),
            );
        }
        Ok(())
    }
    fn send(&self, item: &Value) -> Value {
        let channel = &item["thread"]["channel"];
        let body = json!({"channel":channel,"thread_ts":item["thread"]["thread"],"text":item["text"],
            "mrkdwn":false,"parse":"none","link_names":false,"unfurl_links":false,"unfurl_media":false,
            "metadata":{"event_type":"shipforge_delivery","event_payload":{"delivery_id":item["delivery_id"],"thread_ts":item["thread"]["thread"]}}});
        let uncertain =
            || json!({"outcome":"uncertain","error":"Slack acceptance could not be verified"});
        let Ok(mut response) = self.post("chat.postMessage", body) else {
            return uncertain();
        };
        let status = response.status().as_u16();
        if status == 429 {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(60)
                .max(1);
            return if delay <= 86400 {
                json!({"outcome":"retry","retry_after":delay,"error":"ratelimited"})
            } else {
                json!({"outcome":"failed","error":"rate limit exceeds one day; check Slack before retrying"})
            };
        }
        if status >= 500 || status == 408 {
            return uncertain();
        }
        if !(200..300).contains(&status) {
            return json!({"outcome":"failed","error":format!("Slack HTTP {status}")});
        }
        let Ok(data) = response
            .body_mut()
            .with_config()
            .limit(262144)
            .read_json::<Value>()
        else {
            return uncertain();
        };
        if data["ok"] == false {
            let code = data["error"]
                .as_str()
                .filter(|s| {
                    s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                .unwrap_or("unrecognized_error");
            return match code {
                "internal_error" | "fatal_error" | "request_timeout" | "service_unavailable" => {
                    uncertain()
                }
                "ratelimited" => json!({"outcome":"retry","retry_after":60,"error":code}),
                _ => json!({"outcome":"failed","error":code}),
            };
        }
        if data["ok"] == true
            && data["channel"] == *channel
            && data["ts"].as_str().is_some_and(|s| {
                s.split_once('.').is_some_and(|(a, b)| {
                    !a.is_empty()
                        && a.len() <= 13
                        && !b.is_empty()
                        && b.len() <= 6
                        && a.bytes().chain(b.bytes()).all(|n| n.is_ascii_digit())
                })
            })
        {
            return json!({"outcome":"sent","remote_id":data["ts"]});
        }
        uncertain()
    }
}

fn batch(client: &mut Client, space: &str, id: &str, api: &Slack<'_>) -> Result<Value> {
    let connector = client.execute(json!({"op":"get","space":space,"kind":"connector","id":id}))?;
    let f = &connector["fields"];
    if f["provider"] != "slack"
        || f["enabled"] != true
        || f["outbound"] != true
        || f["bot_id"].as_str().is_none()
    {
        return Err(
            "enable Slack outbound and configure its workspace, channel, app and bot IDs first"
                .into(),
        );
    }
    let pending=client.execute(json!({"op":"im.outbox","space":space,"id":id,"limit":1,"state":"pending","available_only":true}))?;
    if pending.as_array().is_some_and(|items| items.is_empty()) {
        return Ok(json!({"deliveries":[]}));
    }
    api.identity(f)?;
    let mut delivered = Vec::new();
    for _ in 0..100 {
        let item = client.execute(json!({"op":"im.claim","space":space,"id":id}))?;
        if item.is_null() {
            break;
        }
        // Scope can be changed between the connector read, the identity probe and the claim.
        let settings =
            client.execute(json!({"op":"get","space":space,"kind":"connector","id":id}))?;
        let current = &settings["fields"];
        let mut outcome = if item["thread"]["workspace"] == f["workspace_id"]
            && item["thread"]["channel"] == f["channel_id"]
            && current["bot_id"] == f["bot_id"]
            && current["app_id"] == f["app_id"]
            && current["enabled"] == true
            && current["outbound"] == true
            && current["workspace_id"] == f["workspace_id"]
            && current["channel_id"] == f["channel_id"]
        {
            api.send(&item)
        } else {
            json!({"outcome":"failed","error":"connector scope changed before delivery"})
        };
        outcome["op"] = json!("im.ack");
        outcome["space"] = json!(space);
        outcome["id"] = json!(id);
        outcome["delivery_id"] = item["delivery_id"].clone();
        outcome["lease"] = item["lease"].clone();
        let acknowledged = client.execute(outcome)?;
        let state = if acknowledged["state"] == "pending" {
            json!("retry")
        } else {
            acknowledged["state"].clone()
        };
        delivered.push(json!({"delivery_id":item["delivery_id"],"outcome":state}));
        if state == "retry" {
            break;
        }
    }
    Ok(json!({"deliveries":delivered}))
}

pub fn run(
    client: &mut Client,
    space: &str,
    id: &str,
    variable: &str,
    watch: bool,
) -> Result<Value> {
    let token = std::env::var(variable)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or("Slack bot token environment variable is unset")?;
    let api = Slack::new("https://slack.com/api", &token);
    loop {
        let result = match batch(client, space, id, &api) {
            Ok(result) => result,
            Err(error)
                if watch
                    && (error == "Slack transport failed"
                        || error.starts_with("offline:")
                        || error.starts_with("Slack auth.test returned 5")
                        || error == "Slack auth.test returned 429") =>
            {
                eprintln!("{error}");
                std::thread::sleep(Duration::from_secs(60));
                continue;
            }
            Err(error) => return Err(error),
        };
        if !watch {
            return Ok(result);
        }
        if result["deliveries"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        {
            println!("{result}");
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}
