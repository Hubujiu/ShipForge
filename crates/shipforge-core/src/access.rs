use super::*;

impl Context<'_> {
    pub(crate) fn role(&self) -> Result<String> {
        self.space_role(false)
    }

    pub(crate) fn metadata_role(&self) -> Result<String> {
        self.space_role(true)
    }

    fn space_role(&self, metadata: bool) -> Result<String> {
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
        if metadata && matches!(member.fields["role"].as_str(), Some("owner" | "admin")) {
            return Ok("admin".into());
        }
        if let Some(m) = space_member {
            return Ok(m.fields["role"].as_str().unwrap_or("viewer").into());
        }
        if metadata && space.fields["visibility"] == "public" && member.fields["role"] != "guest" {
            return Ok("viewer".into());
        }
        Err(forbidden("space is unavailable"))
    }

    pub(crate) fn authorize_scope(&self, op: &str, q: &Value) -> Result<()> {
        if q["kind"] == "space" && matches!(op, "get" | "list" | "update") {
            self.metadata_role()?;
            if op == "update" && q["fields"]["visibility"] == "public" {
                let space = self.get("space", self.space)?;
                if space.fields["visibility"] == "private" {
                    self.require_admin()?;
                }
            }
        } else {
            self.role()?;
        }
        if q["kind"] == "customer_request" {
            crate::spaces::require_module(self, "customer_requests")?;
        }
        if q["kind"] == "iteration"
            || matches!(
                op,
                "iteration.close" | "iteration.preview" | "iteration.timeline"
            )
        {
            crate::spaces::require_module(self, "iterations")?;
        }
        Ok(())
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
        crate::spaces::ensure_active(self)?;
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
        if matches!(r.kind.as_str(), "message" | "run") && !crate::chat::check_inherited(self, r)? {
            return Ok(false);
        }
        self.intrinsic_visibility(r)
    }
    pub(crate) fn intrinsic_visibility(&self, r: &Record) -> Result<bool> {
        let fields = &r.fields;
        if r.kind == "assistant" {
            return Ok(fields["owner"] == self.actor
                && r.space == self.org_id()?
                && self.role().is_ok());
        }
        if r.kind == crate::task_assistants::KIND {
            return Ok(r.space == self.space && self.require_admin().is_ok());
        }
        if r.space != self.space {
            return Ok(false);
        }
        if r.kind == "chat" {
            return crate::chat::can_read(self, r);
        }
        if r.kind == "run"
            && (!crate::worker::automation_runs::run_visible(self, r)?
                || !crate::agents::run_source_visible(self, r)?)
        {
            return Ok(false);
        }
        if matches!(r.kind.as_str(), "agent" | "crew") {
            return crate::agents::visible(self, r);
        }
        if r.kind == "comment" && !crate::discussions::ancestry_visible(self, r)? {
            return Ok(false);
        }
        if r.kind == "invitation" {
            return Ok(self.require_admin().is_ok());
        }
        if matches!(r.kind.as_str(), "message" | "artifact" | "run")
            && let Some(chat) = fields["chat"].as_str().or(fields["chat_id"].as_str())
            && !crate::chat::retained(self, r, chat)?
        {
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
            // A completed Agent reply follows its private conversation's grants.
            // Its run, machine configuration and independently private rows keep their own ACL.
            let shared_reply = if r.kind == "message"
                && fields["author_kind"] == "agent"
                && fields["run"].is_string()
                && let Some(chat) = fields["chat"].as_str()
            {
                self.find("chat", chat)?
                    .is_some_and(|room| room.fields["visibility"] == "private")
            } else {
                false
            };
            if !shared_reply {
                return Ok(false);
            }
        }
        // A private chat, task or document must not be discoverable through its child rows.
        let parent_keys: &[(&str, &str)] = match r.kind.as_str() {
            "message" => &[("chat", "chat")],
            "notification" => &[
                ("automation", "automation"),
                ("task", "task"),
                ("run", "run"),
                ("run_id", "run"),
                ("chat", "chat"),
                ("message", "message"),
            ],
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
                || crate::task_commands::assignees::people(fields).any(|id| id == self.actor));
        }
        Ok(true)
    }
    pub(crate) fn authorize_write(&self, kind: &str, existing: Option<&Record>) -> Result<()> {
        if let Some(row) = existing
            && !self.visible(row)?
        {
            return Err(forbidden("record is unavailable"));
        }
        if let Some(room) = existing.filter(|r| r.kind == "chat") {
            crate::chat::can_edit(self, room)?;
        }
        if let Some(handler) = existing.filter(|r| matches!(r.kind.as_str(), "agent" | "crew")) {
            crate::agents::require_manage(self, handler)?;
        }
        if let Some(view) = existing.filter(|r| r.kind == "view") {
            crate::saved_views::can_edit(self, view)?;
        }
        if let Some(skill) = existing.filter(|r| r.kind == "skill")
            && skill.fields["creator"].as_str().unwrap_or("owner") != self.actor
        {
            self.require_admin()?;
        }
        if kind == "space" {
            return match self.metadata_role()?.as_str() {
                "owner" | "admin" => Ok(()),
                _ => Err(forbidden("space administrator required")),
            };
        }
        if matches!(
            kind,
            "member"
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
        if matches!(
            op,
            "automation.lifecycle" | "automation.conversation" | "automation.retry"
        ) {
            return crate::worker::automation_runs::replay(self, op, q, result);
        }
        if op == "chat.avatar.set" {
            return crate::chat::require_owner(self, &self.readable("chat", required(q, "id")?)?);
        }
        if op == "assistant.send" {
            return crate::chat::assistant::replay(self, result);
        }
        if crate::task_assistants::WRITES.contains(&op) {
            return crate::task_assistants::replay(self, op, q);
        }
        if crate::statuses::WRITES.contains(&op) {
            if op == "task_type.workflow.save" {
                self.require_admin()?;
                self.readable("task_type", required(q, "id")?)?;
            } else {
                let org = self.org_id()?;
                let root = Context {
                    space: &org,
                    ..*self
                };
                root.require_admin()?;
                if op == "status.library.save" {
                    root.readable("status", required(result, "id")?)?;
                }
            }
            return Ok(());
        }
        if crate::conversation_tools::handles(op) {
            return crate::conversation_tools::replay(self, op, q, result);
        }
        if crate::agents::handles(op) {
            return crate::agents::replay(self, q, result);
        }
        if op == "customer_request.convert" {
            self.require_edit()?;
            crate::spaces::require_module(self, "customer_requests")?;
            self.readable("customer_request", required(q, "id")?)?;
            if result["submitted"] == true {
                return Ok(());
            }
            let dest = Context {
                space: required(result, "space")?,
                ..*self
            };
            dest.role()?;
            dest.readable("task", required(result, "id")?)?;
            return Ok(());
        }
        if op == "triage.submit" {
            if crate::triage::intake::outside(self)?
                || matches!(
                    self.role()?.as_str(),
                    "member" | "editor" | "admin" | "owner" | "guest"
                )
            {
                return Ok(());
            }
            return Err(forbidden("space submission is unavailable"));
        }
        if op == "triage.configure" {
            if self.role().is_ok() && matches!(self.metadata_role()?.as_str(), "owner" | "admin") {
                return Ok(());
            }
            return Err(forbidden("queue settings are unavailable"));
        }
        if op == "triage.batch" {
            self.require_edit()?;
            for id in crate::domain::strings(&q["tasks"], "tasks")? {
                self.readable("task", id)?;
            }
            return Ok(());
        }
        if op == "view.organize" {
            self.readable("view", required(q, "id")?)?;
            return Ok(());
        }
        if op == "chat.start" {
            self.require_edit()?;
            self.readable("chat", required(result, "id")?)?;
            if let Some(mode @ ("agent" | "crew")) = q["mode"].as_str() {
                crate::agents::usable(self, mode, required(q, "target")?, None)?;
            }
            return Ok(());
        }
        if op == "view.save_as" {
            self.readable("view", required(q, "id")?)?;
            self.readable("view", required(result, "id")?)?;
            return self.require_edit();
        }
        if matches!(op, "comment.resolve" | "comment.reopen") {
            self.require_edit()?;
            self.readable("task", required(q, "task_id")?)?;
            self.readable("comment", required(q, "id")?)?;
            self.readable("comment", required(result, "id")?)?;
            return Ok(());
        }
        if matches!(
            op,
            "chat.fork" | "chat.convert" | "chat.bind_task" | "chat.organize" | "chat.send"
        ) {
            self.readable("chat", required(q, "id")?)?;
            if op == "chat.fork" {
                self.readable("message", required(q, "message")?)?;
                self.readable("chat", required(result, "id")?)?;
            }
            if op == "chat.bind_task" {
                self.readable("task", required(q, "task")?)?;
            }
            if op == "chat.convert" {
                self.readable("run", required(result, "run")?)?;
            }
            if op == "chat.send" {
                self.readable("message", required(result, "message")?)?;
            }
            return Ok(());
        }
        if op == "iteration.close" {
            self.require_edit()?;
            self.readable("iteration", required(q, "id")?)?;
            for task in result["moved"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                self.readable("task", task)?;
            }
            return Ok(());
        }
        if op == "iteration.configure" {
            return self.require_admin();
        }
        if matches!(op, "space.join" | "space.leave") {
            let org = self.org_id()?;
            return Context {
                space: &org,
                ..*self
            }
            .role()
            .map(|_| ());
        }
        if op == "space.pin" {
            self.role()?;
            return Ok(());
        }
        if matches!(op, "space.deactivate" | "space.restore") {
            return self.require_admin();
        }
        if op == "chat.share.create" {
            let room = self.readable("chat", required(q, "id")?)?;
            crate::chat::require_owner(self, &room)?;
            crate::chat::read_share(self.db, required(result, "share")?, Some(self.actor))?;
            return self.require_edit();
        }
        if matches!(op, "chat.manage" | "chat.preferences" | "chat.share.revoke") {
            // These receipts contain only the requesting actor's acknowledgment.
            return self.role().map(|_| ());
        }
        if op == "task.move" {
            self.require_edit()?;
            let dest = Context {
                space: required(q, "target")?,
                ..*self
            };
            dest.require_edit()?;
            dest.readable("task", required(q, "id")?)?;
            return Ok(());
        }
        if op == "task.command" {
            self.require_edit()?;
            self.readable("task", required(q, "id")?)?;
            self.readable("chat", required(&result["chat"], "id")?)?;
            self.readable("run", required(&result["run"], "id")?)?;
            return Ok(());
        }
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
        if matches!(op, "create" | "update")
            && let Some(row) = prior
                .as_ref()
                .filter(|r| r.kind == "task" && r.space != self.space)
        {
            if op == "create" {
                self.authorize_write("task", None)?;
            } else {
                self.require_edit()?;
            }
            let target = Context {
                space: &row.space,
                ..*self
            };
            target.role()?;
            target.readable("task", &row.id)?;
            return if target.visible(row)? {
                Ok(())
            } else {
                Err(forbidden("routed task unavailable"))
            };
        }
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
            "preferences.update" | "onboarding.display" | "inbox.configure" | "inbox.bulk" => {}
            "tag.apply" => {
                self.require_edit()?;
                for id in crate::domain::strings(&q["tasks"], "tasks")? {
                    self.readable("task", id)?;
                }
            }
            "tag.merge" => {
                let target = self.readable("tag", required(q, "target")?)?;
                if target.fields["scope"] == "organization" {
                    self.require_admin()?;
                } else {
                    self.require_edit()?;
                }
            }
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
            "comment.react" => {
                self.require_edit()?;
                self.readable("comment", required(q, "id")?)?;
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
