use super::*;

struct PublicationError {
    state: &'static str,
    message: String,
}
impl From<String> for PublicationError {
    fn from(message: String) -> Self {
        Self {
            state: "failed",
            message,
        }
    }
}

fn check_scope(api: &Api, item: &Value, b: &Value, key: &str) -> Result<()> {
    if id(&item["id"])? != key {
        return Err("forbidden: source returned a different work item".into());
    }
    let bound = &b["project"];
    let scope = if api.zen {
        &item[b["scope_kind"]
            .as_str()
            .ok_or("invalid: missing source scope")?]
    } else {
        &item["project"]
    };
    let value = if scope.is_object() {
        &scope["id"]
    } else {
        scope
    };
    if Value::String(id(value)?) != *bound {
        return Err("forbidden: source work item does not belong to the project".into());
    }
    Ok(())
}
fn status(api: &Api, item: &Value) -> Result<String> {
    if api.zen {
        id(&item["status"])
    } else {
        id(&item["state"]["id"])
    }
}

fn send(api: &Api, b: &Value, plan: &Value) -> std::result::Result<String, PublicationError> {
    let key = id(&plan["key"])?;
    let item_url = if api.zen {
        format!("/bugs/{key}")
    } else {
        format!("/pjm/workitems/{key}")
    };
    let before = api.get(&item_url, &[])?;
    check_scope(api, &before, b, &key)?;
    if plan["intent"] == "comment" {
        if let Some(known) = plan["confirm_comment"].as_str() {
            let detail = api.get(
                &format!("/comments/{known}"),
                &[
                    ("principal_type", "workitem".into()),
                    ("principal_id", key.clone()),
                ],
            )?;
            let wrong_task = detail["principal_id"].as_str().is_some_and(|s| s != key)
                || detail["principal_type"]
                    .as_str()
                    .is_some_and(|s| s != "workitem");
            if wrong_task
                || detail["id"] != known
                || detail["content"] != plan["body"]
                || detail["is_deleted"] == 1
                || detail["is_deleted"] == true
            {
                return Err(PublicationError {
                    state: "uncertain",
                    message: "conflict: the supplied comment does not match this publication"
                        .into(),
                });
            }
            return Ok(known.to_owned());
        }
        let response = api
            .update(
                "POST",
                "/comments",
                &json!({"principal_type":"workitem","principal_id":key,"content":plan["body"]}),
            )
            .map_err(|message| {
                let rejected = message.starts_with("vendor: upstream returned HTTP 4");
                PublicationError {
                    state: if rejected { "failed" } else { "uncertain" },
                    message,
                }
            })?;
        return id(&response["id"]).map_err(|message| PublicationError {
            state: "uncertain",
            message,
        });
    }
    let target = id(&plan["target"])?;
    let current = status(api, &before)?;
    if current == target {
        return Ok(key);
    }
    if plan["source_state"]
        .as_str()
        .is_some_and(|s| !s.is_empty() && s != current)
    {
        return Err(
            "conflict: source status changed independently; preview current changes before writing"
                .to_owned()
                .into(),
        );
    }
    if api.zen {
        let (action, body) = match target.as_str() {
            "resolved" => (
                "resolve",
                json!({"resolution":plan["resolution"],"resolvedBuild":plan["resolved_build"]}),
            ),
            "closed" if current == "resolved" => ("close", json!({})),
            "closed" => {
                return Err("conflict: resolve this ZenTao bug before closing it"
                    .to_owned()
                    .into());
            }
            "active" => ("active", json!({"openedBuild":"trunk"})),
            _ => {
                return Err("invalid: unsupported ZenTao bug transition"
                    .to_owned()
                    .into());
            }
        };
        api.update("POST", &format!("{item_url}/{action}"), &body)?;
    } else {
        api.update("PATCH", &item_url, &json!({"state_id":target}))?;
    }
    let after = api.get(&item_url, &[])?;
    check_scope(api, &after, b, &key)?;
    if status(api, &after)? != target {
        return Err("vendor: upstream did not confirm the requested state"
            .to_owned()
            .into());
    }
    Ok(key)
}

pub(super) fn run(client: &mut Client, q: &Value, api: &Api, binding: &Value) -> Result<Value> {
    let mut claim = scoped(q, "imports.prepare_push");
    for key in ["task_id", "intent", "comment_id", "confirm_comment"] {
        if let Some(v) = q.get(key) {
            claim[key] = v.clone();
        }
    }
    let receipt = client.execute(claim)?;
    if receipt["state"] == "sent" {
        return Ok(receipt);
    }
    let outcome = send(api, binding, &receipt["plan"]);
    let mut finish = scoped(q, "imports.finish_push");
    finish["delivery"] = receipt["delivery"].clone();
    finish["lease"] = receipt["lease"].clone();
    match outcome {
        Ok(remote) => {
            finish["result"] = json!("sent");
            finish["remote_id"] = json!(remote);
            client.execute(finish)
        }
        Err(err) => {
            // A failed verification of a previous ambiguous comment must never enable another POST.
            finish["result"] = json!(if receipt["plan"]["confirm_comment"].is_string() {
                "uncertain"
            } else {
                err.state
            });
            client.execute(finish)?;
            Err(err.message)
        }
    }
}
