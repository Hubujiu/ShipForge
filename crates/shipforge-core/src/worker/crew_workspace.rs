use super::*;

// The service assigns the namespace. A run or adapter cannot supply a directory or join another crew.
pub(super) fn snapshot(c: &Context<'_>, handler: &Record, fields: &Value) -> Result<Value> {
    let Some(task_id) = fields["task_id"].as_str() else {
        return Ok(Value::Null);
    };
    let crew = if handler.kind == "crew" && handler.fields["share_task_directory"] == true {
        handler.id.as_str()
    } else if let Some(parent) = fields["parent_id"].as_str() {
        let parent = c.readable("run", parent)?;
        let scope = &parent.fields["crew_workspace"];
        if parent.fields["handler_kind"] != "crew"
            || parent.fields["task_id"] != task_id
            || !scope.is_object()
        {
            return Ok(Value::Null);
        }
        if fields["visibility"] == "private" && parent.fields["visibility"] != "private" {
            return Ok(Value::Null);
        }
        return Ok(scope.clone());
    } else {
        return Ok(Value::Null);
    };
    let task = c.readable("task", task_id)?;
    let room = fields["chat_id"]
        .as_str()
        .map(|id| c.readable("chat", id))
        .transpose()?;
    let private = fields["visibility"] == "private" || task.fields["visibility"] == "private";
    let audience = json!([
        private.then_some(c.actor),
        room.as_ref()
            .filter(|r| r.fields["visibility"] == "private")
            .map(|r| &r.id)
    ])
    .to_string();
    c.db.execute(
        "INSERT OR IGNORE INTO crew_workspaces(space,crew,task,audience,token) VALUES(?,?,?,?,?)",
        params![
            c.space,
            crew,
            task_id,
            audience,
            Uuid::new_v4().simple().to_string()
        ],
    )
    .map_err(db_error)?;
    let token: String =
        c.db.query_row(
            "SELECT token FROM crew_workspaces WHERE space=? AND crew=? AND task=? AND audience=?",
            params![c.space, crew, task_id, audience],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    Ok(json!({"crew":crew,"task":task_id,"token":token}))
}
