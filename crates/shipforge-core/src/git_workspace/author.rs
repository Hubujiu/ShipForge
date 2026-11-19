use super::*;
use rusqlite::OptionalExtension;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Author {
    name: Option<Name>,
    email: Option<Email>,
    coauthor: Option<bool>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "mode",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Name {
    Trigger,
    TriggerAndAgent,
    Owner,
    OwnerAndAgent,
    Machine,
    Manual(String),
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "mode",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Email {
    Trigger,
    Owner,
    Machine,
    Manual(String),
}

fn safe_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 200
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>'))
}

fn safe_email(email: &str) -> bool {
    email.len() <= 254
        && email
            .split_once('@')
            .is_some_and(|(user, host)| !user.is_empty() && !host.is_empty() && !host.contains('@'))
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
}

impl Author {
    pub(super) fn validate(&self) -> Result<()> {
        if let Some(Name::Manual(name)) = &self.name
            && !safe_name(name)
        {
            return Err(invalid(
                "enter a Git author name without control characters or brackets",
            ));
        }
        if let Some(Email::Manual(email)) = &self.email
            && !safe_email(email)
        {
            return Err(invalid("enter a Git author email address"));
        }
        Ok(())
    }

    pub(super) fn inherit(layers: [(&str, &Self); 4]) -> (Self, Value) {
        let mut result = Self {
            name: Some(Name::TriggerAndAgent),
            email: Some(Email::Machine),
            coauthor: Some(true),
        };
        let mut sources = json!({"name":"default","email":"default","coauthor":"default"});
        for (source, layer) in layers {
            if layer.name.is_some() {
                result.name.clone_from(&layer.name);
                sources["name"] = json!(source);
            }
            if layer.email.is_some() {
                result.email.clone_from(&layer.email);
                sources["email"] = json!(source);
            }
            if layer.coauthor.is_some() {
                result.coauthor = layer.coauthor;
                sources["coauthor"] = json!(source);
            }
        }
        (result, sources)
    }

    pub(super) fn resolve(&self, requester: &Context<'_>, agent: &Record) -> Result<Value> {
        let owner = agent.fields["owner"]
            .as_str()
            .or(agent.fields["creator"].as_str())
            .unwrap_or("owner");
        let with_agent = |member: Option<String>| {
            let label = if safe_name(&agent.title) {
                &agent.title
            } else {
                "Agent"
            };
            member.map_or_else(|| label.to_owned(), |name| format!("{name} & {label}"))
        };
        let name = match self.name.as_ref().unwrap_or(&Name::TriggerAndAgent) {
            Name::Trigger => member_name(requester, requester.actor)?,
            Name::TriggerAndAgent => Some(with_agent(member_name(requester, requester.actor)?)),
            Name::Owner => member_name(requester, owner)?,
            Name::OwnerAndAgent => Some(with_agent(member_name(requester, owner)?)),
            Name::Machine => None,
            Name::Manual(value) => Some(value.clone()),
        };
        let email = match self.email.as_ref().unwrap_or(&Email::Machine) {
            Email::Trigger => member_email(requester, requester.actor)?,
            Email::Owner => member_email(requester, owner)?,
            Email::Machine => None,
            Email::Manual(value) => Some(value.clone()),
        };
        Ok(json!({"name":name,"email":email,"coauthor":self.coauthor.unwrap_or(true)}))
    }
}

fn member_name(c: &Context<'_>, id: &str) -> Result<Option<String>> {
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    Ok(root
        .find("member", id)?
        .or(c.find("member", id)?)
        .map(|r| r.title)
        .filter(|name| safe_name(name)))
}

fn member_email(c: &Context<'_>, id: &str) -> Result<Option<String>> {
    let org = c.org_id()?;
    let root = Context { space: &org, ..*c };
    if root.find("member", id)?.is_none() {
        return Ok(None);
    }
    c.db.query_row(
        "SELECT email FROM account_profiles WHERE actor=?",
        [id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|email| email.filter(|value| safe_email(value)))
    .map_err(crate::storage)
}
