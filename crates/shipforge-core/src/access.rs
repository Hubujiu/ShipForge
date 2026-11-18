use super::*;

impl Context<'_> {
    pub(crate) fn role(&self) -> Result<String> {
        let space = self
            .find("space", self.space)?
            .ok_or_else(|| forbidden("space is unavailable"))?;
        if self.actor == "owner" {
            return Ok("owner".into());
        }
        let org_id = space.fields["organization"].as_str().unwrap_or("default");
        let space_member = self.find("member", self.actor)?;
        if org_id == self.space {
            return space_member
                .map(|m| m.fields["role"].as_str().unwrap_or("viewer").to_owned())
                .ok_or_else(|| forbidden("space is unavailable"));
        }
        let org = Context {
            db: self.db,
            actor: self.actor,
            space: org_id,
        };
        let member = org
            .find("member", self.actor)?
            .ok_or_else(|| forbidden("space is unavailable"))?;
        if matches!(member.fields["role"].as_str(), Some("owner" | "admin")) {
            return Ok("admin".into());
        }
        if let Some(m) = space_member {
            return Ok(m.fields["role"].as_str().unwrap_or("viewer").into());
        }
        if space.fields["visibility"] == "public" {
            return Ok("viewer".into());
        }
        Err(forbidden("space is unavailable"))
    }
    pub(crate) fn org_id(&self) -> Result<String> {
        let row = self.get("space", self.space)?;
        Ok(row.fields["organization"]
            .as_str()
            .unwrap_or("default")
            .to_owned())
    }
    pub(crate) fn require_admin(&self) -> Result<()> {
        if matches!(self.role()?.as_str(), "owner" | "admin") {
            Ok(())
        } else {
            Err(forbidden("space administrator required"))
        }
    }
    pub(crate) fn require_edit(&self) -> Result<()> {
        if matches!(
            self.role()?.as_str(),
            "owner" | "admin" | "member" | "editor"
        ) {
            Ok(())
        } else {
            Err(forbidden("space editor required"))
        }
    }
    pub(crate) fn visible(&self, r: &Record) -> Result<bool> {
        let fields = &r.fields;
        if r.space != self.space {
            return Ok(false);
        }
        if r.kind == "notification" && fields["recipient"].as_str() != Some(self.actor) {
            return Ok(false);
        }
        if r.kind != "space"
            && fields["visibility"] == "private"
            && fields["creator"].as_str() != Some(self.actor)
            && fields["owner"].as_str() != Some(self.actor)
        {
            return Ok(false);
        }
        // A private chat, task or document must not be discoverable through its child rows.
        let parent_keys: &[(&str, &str)] = match r.kind.as_str() {
            "message" => &[("chat", "chat")],
            "notification" => &[("task", "task"), ("run_id", "run")],
            "comment" => &[("task", "task")],
            "artifact" => &[
                ("chat", "chat"),
                ("task", "task"),
                ("document", "document"),
                ("run", "run"),
                ("message", "message"),
                ("run_id", "run"),
                ("task_id", "task"),
                ("chat_id", "chat"),
            ],
            "relation" => &[("from", "task"), ("to", "task")],
            "run" => &[("task_id", "task"), ("chat_id", "chat")],
            _ => &[],
        };
        for (key, kind) in parent_keys {
            if let Some(id) = fields[key].as_str() {
                let Some(parent) = self.find(kind, id)? else {
                    return Ok(false);
                };
                if !self.visible(&parent)? {
                    return Ok(false);
                }
            }
        }
        if r.kind == "task" && self.role()? == "guest" {
            return Ok(fields["creator"].as_str() == Some(self.actor)
                || fields["owner"].as_str() == Some(self.actor)
                || fields["assignee"].as_str() == Some(self.actor));
        }
        Ok(true)
    }
    pub(crate) fn authorize_write(&self, kind: &str, existing: Option<&Record>) -> Result<()> {
        if let Some(row) = existing
            && !self.visible(row)?
        {
            return Err(forbidden("record is unavailable"));
        }
        if matches!(
            kind,
            "member"
                | "space"
                | "organization"
                | "status"
                | "field"
                | "task_type"
                | "relation_type"
                | "settings"
                | "connector"
                | "invitation"
        ) {
            return self.require_admin();
        }
        if kind == "notification" {
            return Err(forbidden(
                "use notification.act to change your own notification",
            ));
        }
        if kind == "task"
            && existing.is_none()
            && self.role()? == "guest"
            && self.get("space", self.space)?.fields["triage_enabled"] == true
        {
            return Ok(());
        }
        self.require_edit()?;
        if let Some(row) = existing
            && matches!(kind, "message" | "comment")
            && row.fields["creator"].as_str() != Some(self.actor)
        {
            self.require_admin()?;
        }
        Ok(())
    }
    pub(crate) fn authorize_replay(&self, q: &Value, result: &Value) -> Result<()> {
        let op = required(q, "op")?;
        if op == "create" && matches!(q["kind"].as_str(), Some("space" | "organization")) {
            let anchor = self.org_id()?;
            let org = Context {
                db: self.db,
                actor: self.actor,
                space: &anchor,
            };
            org.require_admin()?;
            let created = Context {
                db: self.db,
                actor: self.actor,
                space: required(result, "id")?,
            };
            created.role()?;
            return Ok(());
        }
        let prior = serde_json::from_value::<Record>(result.clone()).ok();
        if let Some(row) = prior.as_ref() {
            if !self.visible(row)? {
                return Err(forbidden("record is unavailable"));
            }
            match self.find(&row.kind, &row.id)? {
                Some(current) if !self.visible(&current)? => {
                    return Err(forbidden("record is unavailable"));
                }
                None if op != "delete" => return Err(forbidden("record is unavailable")),
                _ => {}
            }
        }
        match op {
            "attachment.upload" => {
                let r = prior
                    .as_ref()
                    .ok_or_else(|| invalid("invalid attachment receipt"))?;
                self.authorize_write(&r.kind, Some(r))?;
            }
            "follow" => {
                self.readable("task", required(q, "id")?)?;
            }
            "notification.act" => {
                self.readable("notification", required(q, "id")?)?;
            }
            "create" | "update" | "delete" => {
                let r = prior
                    .as_ref()
                    .ok_or_else(|| invalid("invalid stored request receipt"))?;
                // An original guest submission remains replayable while its author retains access.
                self.authorize_write(&r.kind, if op == "create" { None } else { Some(r) })?;
            }
            _ => self.require_edit()?,
        }
        Ok(())
    }
}
