use crate::{Context, Record, Result, domain, forbidden, invalid, json, required};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod author;
mod repositories;

use author::Author;
use repositories::{Inclusion, Repository};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Setup {
    author: Author,
    #[serde(skip_serializing_if = "Option::is_none")]
    repositories: Option<Vec<Repository>>,
    include_space: Option<Inclusion>,
}

impl Setup {
    fn read(record: &Record) -> Result<Self> {
        parse(record.kind.as_str(), &record.fields["git_setup"])
    }

    fn repositories(&self) -> &[Repository] {
        self.repositories.as_deref().unwrap_or_default()
    }
}

fn parse(kind: &str, value: &Value) -> Result<Setup> {
    if value.is_null() {
        return Ok(Setup::default());
    }
    let mut setup: Setup = serde_json::from_value(value.clone())
        .map_err(|_| invalid("invalid Git repository or author settings"))?;
    if kind == "organization" && !setup.repositories().is_empty() {
        return Err(invalid("bind repositories to a space or Agent"));
    }
    if kind != "agent" && setup.include_space.is_some() {
        return Err(invalid("only an Agent can select inherited repositories"));
    }
    setup.author.validate()?;
    repositories::normalize(
        setup.repositories.as_deref_mut().unwrap_or_default(),
        setup.include_space.as_mut(),
    )?;
    Ok(setup)
}

pub(crate) fn validate_record(row: &Record) -> Result<()> {
    if matches!(row.kind.as_str(), "organization" | "space" | "agent") {
        Setup::read(row)?;
    }
    Ok(())
}

fn can_configure(c: &Context<'_>, row: &Record) -> Result<()> {
    crate::spaces::ensure_active(c)?;
    if row.kind == "agent" {
        c.require_edit()?;
        crate::agents::require_manage(c, row)
    } else {
        c.require_admin()
    }
}

struct Layers {
    organization: Setup,
    space: Setup,
    parent: Setup,
    agent: Setup,
}

impl Layers {
    fn author(&self) -> (Author, Value) {
        Author::inherit([
            ("organization", &self.organization.author),
            ("space", &self.space.author),
            ("parent_agent", &self.parent.author),
            ("agent", &self.agent.author),
        ])
    }

    fn repositories(&self, checkout: Option<&str>) -> Vec<Repository> {
        repositories::resolve(
            self.space.repositories(),
            self.agent_repositories(),
            self.agent
                .include_space
                .as_ref()
                .or(self.parent.include_space.as_ref()),
            checkout,
        )
    }

    fn agent_repositories(&self) -> &[Repository] {
        self.agent
            .repositories
            .as_deref()
            .unwrap_or_else(|| self.parent.repositories())
    }

    fn agent_repository_sources(&self) -> Value {
        let repository_source = if self.agent.repositories.is_some() {
            "agent"
        } else if self.parent.repositories.is_some() {
            "parent_agent"
        } else {
            "default"
        };
        let (include_source, include) = if let Some(value) = &self.agent.include_space {
            ("agent", value)
        } else if let Some(value) = &self.parent.include_space {
            ("parent_agent", value)
        } else if self.agent_repositories().is_empty() {
            ("default", &Inclusion::All)
        } else {
            ("default", &Inclusion::None)
        };
        json!({"repositories_source":repository_source,"include_space_source":include_source,"effective_include_space":include})
    }
}

fn layers(c: &Context<'_>, agent: Option<&Record>, space_level: bool) -> Result<Layers> {
    let org_id = c.org_id()?;
    let root = Context {
        space: &org_id,
        ..*c
    };
    let organization = Setup::read(&root.get("organization", &org_id)?)?;
    let space = if space_level {
        Setup::read(&c.get("space", c.space)?)?
    } else {
        Setup::default()
    };
    let parent = agent
        .filter(|row| row.fields["parent_id"].is_string())
        .map(|row| crate::agents::git_setup_parent(c, row).and_then(|base| Setup::read(&base)))
        .transpose()?
        .unwrap_or_default();
    let agent = agent.map(Setup::read).transpose()?.unwrap_or_default();
    Ok(Layers {
        organization,
        space,
        parent,
        agent,
    })
}

pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let org = c.org_id()?;
    let level = required(q, "level")?;
    let scope = Context {
        space: if level == "organization" {
            &org
        } else {
            c.space
        },
        ..*c
    };
    let mut row = match level {
        "organization" => scope.readable("organization", &org)?,
        "space" => scope.readable("space", c.space)?,
        "agent" => scope.readable("agent", required(q, "id")?)?,
        _ => return Err(invalid("choose organization, space or agent Git settings")),
    };
    if op == "git.configure" {
        can_configure(&scope, &row)?;
        if q.get("expected_rev").is_none() {
            return Err(invalid("a current revision is required for Git settings"));
        }
        domain::check_rev(q, &row)?;
        let settings = q
            .get("settings")
            .ok_or_else(|| invalid("Git settings are required"))?;
        let mut fields = row.fields.clone();
        let config = parse(level, settings)?;
        if let Some(Inclusion::Selected { urls }) = &config.include_space {
            let source = Setup::read(&c.get("space", c.space)?)?;
            if !urls
                .iter()
                .all(|url| source.repositories().iter().any(|item| &item.url == url))
            {
                return Err(invalid("select repositories currently bound to this space"));
            }
        }
        fields["git_setup"] = if settings.is_null() {
            Value::Null
        } else {
            json!(config)
        };
        row = scope.update(&row, &row.title, fields)?;
    } else if op != "git.settings" {
        return Err(forbidden("unknown Git settings operation"));
    }
    let agent = (level == "agent").then_some(&row);
    let layers = layers(&scope, agent, level != "organization")?;
    let (author, sources) = layers.author();
    let mut spaces = Vec::new();
    if level == "organization" {
        for space in c.all_spaces()? {
            let member_scope = Context {
                space: &space.id,
                ..*c
            };
            if space.fields["organization"] == org && member_scope.role().is_ok() {
                let settings = Setup::read(&space)?;
                spaces.push(json!({"id":space.id,"title":space.title,"repositories":settings.repositories(),"can_configure":can_configure(&member_scope,&space).is_ok()}));
            }
        }
    }
    let sources_for_repositories = layers.agent_repository_sources();
    Ok(
        json!({"level":level,"id":row.id,"title":row.title,"rev":row.rev,"can_configure":can_configure(&scope,&row).is_ok(),
        "settings":row.fields["git_setup"],"author":author,"author_sources":sources,
        "agent_repositories":layers.agent_repositories(),"repositories_source":sources_for_repositories["repositories_source"],
        "include_space_source":sources_for_repositories["include_space_source"],"effective_include_space":sources_for_repositories["effective_include_space"],
        "space_repositories":layers.space.repositories(),"repositories":layers.repositories(None),
        "spaces":spaces}),
    )
}

pub(crate) fn execution(c: &Context<'_>, run: &Record) -> Result<Value> {
    if run.fields["config"]["directory_mode"] == "temporary"
        || !matches!(run.fields["handler_kind"].as_str(), Some("agent" | "crew"))
    {
        return Ok(Value::Null);
    }
    let person = required(&run.fields, "requested_by")?;
    let requester = Context {
        actor: person,
        ..*c
    };
    let agent = requester.readable("agent", required(&run.fields["config"], "agent_id")?)?;
    let layers = layers(&requester, Some(&agent), true)?;
    let (author, _) = layers.author();
    let repositories = layers.repositories(run.fields["checkout_branch"].as_str());
    Ok(json!({"author":author.resolve(&requester,&agent)?,"repositories":repositories}))
}

pub(crate) fn checkout_branch(q: &Value) -> Result<Option<&str>> {
    q.get("checkout_branch")
        .filter(|value| !value.is_null())
        .map(repositories::branch)
        .transpose()
}

pub(crate) fn schema(schema: &mut Value) {
    let rules = schema["allOf"].as_array_mut().unwrap();
    let name_modes = [
        "trigger",
        "trigger_and_agent",
        "owner",
        "owner_and_agent",
        "machine",
    ];
    let email_modes = ["trigger", "owner", "machine"];
    let source = |modes: &[&str]| {
        json!({
            "oneOf":[
                {"type":"null"},
                {"type":"object","additionalProperties":false,"required":["mode"],"properties":{"mode":{"enum":modes}}},
                {"type":"object","additionalProperties":false,"required":["mode","value"],"properties":{"mode":{"const":"manual"},"value":{"type":"string","minLength":1}}}
            ]
        })
    };
    rules.extend([
        json!({"if":{"properties":{"op":{"enum":["git.settings","git.configure"]}}},"then":{
            "required":["level"],"properties":{"level":{"enum":["organization","space","agent"]}},
            "if":{"properties":{"level":{"const":"agent"}}},"then":{"required":["id"]}
        }}),
        json!({"if":{"properties":{"op":{"const":"git.configure"}}},"then":{
            "required":["expected_rev","settings"],"properties":{"settings":{
                "type":["object","null"],"additionalProperties":false,"properties":{
                    "author":{"type":"object","additionalProperties":false,"properties":{
                        "name":source(&name_modes),"email":source(&email_modes),"coauthor":{"type":["boolean","null"]}
                    }},
                    "repositories":{"type":"array","maxItems":24,"items":{"type":"object","additionalProperties":false,
                        "required":["url"],"properties":{"url":{"type":"string","maxLength":2048},"description":{"type":"string","maxLength":1024},
                            "branch":{"type":["string","null"],"minLength":1,"maxLength":255}}
                    }},
                    "include_space":{"oneOf":[{"type":"null"},
                        {"type":"object","additionalProperties":false,"required":["mode"],"properties":{"mode":{"enum":["none","all"]}}},
                        {"type":"object","additionalProperties":false,"required":["mode","urls"],"properties":{
                            "mode":{"const":"selected"},"urls":{"type":"array","uniqueItems":true,"items":{"type":"string"}}}}
                    ]}
                }
            }}
        }}),
        json!({"if":{"properties":{"op":{"const":"run.enqueue"}}},"then":{"properties":{
            "checkout_branch":{"type":["string","null"],"minLength":1,"maxLength":255}
        }}}),
    ]);
}
