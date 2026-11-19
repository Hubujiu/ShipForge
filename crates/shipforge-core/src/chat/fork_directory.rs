use super::*;

fn mode(q: &Value) -> Result<&str> {
    match q.get("workspace_mode") {
        None => Ok("default"),
        Some(value) => value
            .as_str()
            .filter(|mode| matches!(*mode, "default" | "reuse" | "branch"))
            .ok_or_else(|| invalid("unknown conversation workspace mode")),
    }
}

fn latest(c: &Context<'_>, room: &Record) -> Result<Option<Record>> {
    let mut runs=c.db.prepare("SELECT id FROM records WHERE space=? AND kind='run'
        AND json_extract(fields,'$.chat_id')=? AND json_type(fields,'$.workspace_checkpoint')='object'
        ORDER BY COALESCE(json_extract(fields,'$.finished_at'),updated_at) DESC,id LIMIT 50").map_err(storage)?;
    let id = runs
        .query_map(params![c.space, room.id], |row| row.get::<_, String>(0))
        .map_err(storage)?;
    for id in id {
        let row = c.get("run", &id.map_err(storage)?)?;
        if c.visible(&row)? {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

pub(super) fn available(c: &Context<'_>, room: &Record) -> Result<Value> {
    if require_owner(c, room).is_err() || c.require_edit().is_err() {
        return Ok(json!({"ready":false,"can_reuse":false}));
    }
    let Some(run) = latest(c, room)? else {
        return Ok(json!({"ready":false}));
    };
    let snapshot = &run.fields["workspace_checkpoint"];
    let machine = snapshot["machine"]
        .as_str()
        .map(|id| c.find("machine", id))
        .transpose()?
        .flatten();
    let machine = machine.filter(|m| m.fields["disabled"] != true && c.visible(m).unwrap_or(false));
    let mut targets = Vec::new();
    if snapshot["repository"].is_string() {
        for target in c.records("machine")? {
            if target.id != snapshot["machine"]
                && target.fields["disabled"] != true
                && c.visible(&target)?
            {
                targets.push(json!({"id":target.id,"title":target.title}));
            }
        }
        targets.sort_by(|a, b| a["title"].as_str().cmp(&b["title"].as_str()));
    }
    Ok(
        json!({"ready":machine.is_some(),"machine":machine.map(|m|json!({"id":m.id,"title":m.title})),
        "git":snapshot["git"],"repository":snapshot["repository"],
        "can_reuse":true,"targets":targets}),
    )
}

pub(super) fn for_fork(c: &Context<'_>, room: &Record, q: &Value) -> Result<Value> {
    let mode = mode(q)?;
    let branch = crate::git_workspace::checkout_branch(q)?;
    let target = q
        .get("target_machine_id")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("invalid target machine"))
        })
        .transpose()?;
    if mode == "default" {
        if branch.is_some() || target.is_some() {
            return Err(invalid("choose branch mode to set a baseline branch"));
        }
        return Ok(Value::Null);
    }
    require_owner(c, room)?;
    let run = latest(c, room)?
        .ok_or_else(|| invalid("start a conversation on a connected work machine first"))?;
    let prepared = &run.fields["workspace_checkpoint"];
    if mode == "reuse" && branch.is_some() {
        return Err(invalid(
            "choose an independent worktree to specify a baseline branch",
        ));
    }
    if mode == "branch" && prepared["git"] != true {
        return Err(invalid(
            "the source conversation workspace has no Git repository",
        ));
    }
    let source_machine = required(prepared, "machine")?;
    let machine = target.unwrap_or(source_machine);
    let repository = if machine != source_machine {
        if mode != "branch" || branch.is_none() {
            return Err(invalid(
                "choose an accessible branch to continue on a different machine",
            ));
        }
        let destination = c.readable("machine", machine)?;
        if destination.fields["disabled"] == true {
            return Err(forbidden("the target machine is disabled"));
        }
        Some(required(prepared, "repository")?)
    } else {
        None
    };
    Ok(
        json!({"mode":mode,"source_chat":room.id,"source_run":run.id,"source_attempt":prepared["attempt"],
        "machine":machine,"source_machine":source_machine,"repository":repository,"checkout_branch":branch}),
    )
}

pub(crate) fn pin(c: &Context<'_>, chat_id: &Value, config: &mut Value) -> Result<Value> {
    let Some(id) = chat_id.as_str() else {
        return Ok(Value::Null);
    };
    let Some(room) = c.find("chat", id)? else {
        return Ok(Value::Null);
    };
    let selection = &room.fields["directory_fork"];
    if selection.is_null() {
        return Ok(Value::Null);
    }
    c.readable("chat", id)?;
    validate_source(c, selection)?;
    config["machine_id"] = selection["machine"].clone();
    config["field_spaces"]["machine_id"] = json!(c.space);
    config["chat_parallel"] = json!(1);
    Ok(selection.clone())
}

fn validate_source(c: &Context<'_>, selection: &Value) -> Result<()> {
    let chat = c.readable("chat", required(selection, "source_chat")?)?;
    let run = c.readable("run", required(selection, "source_run")?)?;
    if run.fields["chat_id"] != chat.id
        || run.fields["workspace_checkpoint"]["machine"] != selection["source_machine"]
        || run.fields["workspace_checkpoint"]["attempt"] != selection["source_attempt"]
    {
        return Err(forbidden("original workspace is unavailable"));
    }
    Ok(())
}

pub(crate) fn for_claim(c: &Context<'_>, run: &Record, machine: &Record) -> Result<Value> {
    let plan = &run.fields["directory_fork"];
    if plan["mode"] != "branch" {
        return Ok(plan.clone());
    }
    let requester = Context {
        actor: required(&run.fields, "requested_by")?,
        ..*c
    };
    let room = requester.readable("chat", required(&run.fields, "chat_id")?)?;
    let existing = latest(&requester, &room)?;
    if let Some(prior) =
        existing.filter(|prior| prior.fields["workspace_checkpoint"]["machine"] == machine.id)
    {
        return Ok(
            json!({"mode":"reuse","source_chat":room.id,"source_run":prior.id,"source_attempt":prior.fields["workspace_checkpoint"]["attempt"],"source_machine":machine.id,"machine":machine.id}),
        );
    }
    Ok(plan.clone())
}

pub(crate) fn can_continue(c: &Context<'_>, run: &Record) -> Result<bool> {
    let choice = &run.fields["directory_fork"];
    if choice.is_null() {
        return Ok(true);
    }
    let requester = Context {
        actor: required(&run.fields, "requested_by")?,
        ..*c
    };
    match validate_source(&requester, choice) {
        Ok(()) => Ok(true),
        Err(error) if error.starts_with("forbidden:") || error.starts_with("not_found:") => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn checkpoint(run: &Record, report: &Value) -> Result<Value> {
    let Some(checkpoint) = report
        .get("workspace_checkpoint")
        .filter(|value| !value.is_null())
    else {
        return Ok(Value::Null);
    };
    if !run.fields["chat_id"].is_string() {
        return Ok(Value::Null);
    }
    let git = checkpoint["git"]
        .as_bool()
        .ok_or_else(|| invalid("Git capability required for a workspace checkpoint"))?;
    let machine = required(&run.fields, "machine_id")?;
    let repository = checkpoint
        .get("repository")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid("invalid Git repository identity"))
                .and_then(crate::git_workspace::remote_url)
        })
        .transpose()?;
    if !git && repository.is_some() {
        return Err(invalid(
            "a Git repository identity requires a Git workspace",
        ));
    }
    Ok(json!({"machine":machine,"git":git,"repository":repository,"attempt":run.fields["attempt"]}))
}

pub(crate) fn schema(schema: &mut Value) {
    schema["allOf"].as_array_mut().unwrap().extend([
        json!({"if":{"properties":{"op":{"const":"chat.workspace"}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"const":"chat.fork"}}},"then":{"required":["id","message"],"properties":{
            "workspace_mode":{"enum":["default","reuse","branch"]},
            "target_machine_id":{"type":["string","null"],"minLength":1},
            "checkout_branch":{"type":["string","null"],"maxLength":255}
        }}}),
        json!({"if":{"properties":{"op":{"const":"run.report"}}},"then":{"properties":{"workspace_checkpoint":{
            "type":"object","required":["git"],"properties":{"git":{"type":"boolean"},"repository":{"type":["string","null"]}}
        }}}}),
    ]);
}
