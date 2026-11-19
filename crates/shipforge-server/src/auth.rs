use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;
pub(crate) mod identity;
mod personal;

pub struct Auth(Connection, Option<Connection>);
#[derive(Clone)]
pub struct Principal {
    pub actor: String,
    pub scope: String,
    pub id: String,
    pub personal: bool,
    prefix: String,
    resource: Option<String>,
    bound_space: Option<String>,
    binding: Option<String>,
}

fn sql(error: impl std::fmt::Display) -> String {
    format!("storage: {error}")
}
fn fingerprint(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

impl Auth {
    pub fn data_directory(&self) -> Result<&std::path::Path, String> {
        self.0
            .path()
            .and_then(|s| std::path::Path::new(s).parent())
            .ok_or_else(|| "storage: unavailable team directory".into())
    }
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(sql)?;
        let db = Connection::open(dir.join("identities.sqlite3")).map_err(sql)?;
        db.busy_timeout(Duration::from_secs(5)).map_err(sql)?;
        db.execute_batch("PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS tokens(id TEXT PRIMARY KEY, digest BLOB NOT NULL UNIQUE, actor TEXT NOT NULL,
          scope TEXT NOT NULL, label TEXT NOT NULL, created INTEGER NOT NULL, revoked INTEGER);
          CREATE TABLE IF NOT EXISTS token_bindings(token_id TEXT PRIMARY KEY, space TEXT NOT NULL, resource TEXT NOT NULL, binding TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS personal_tokens(
            token_id TEXT PRIMARY KEY,issuer TEXT NOT NULL,prefix TEXT NOT NULL,expires INTEGER,last_used INTEGER,
            lifetime TEXT NOT NULL,source TEXT NOT NULL,machine_space TEXT,machine_id TEXT,request_id TEXT,UNIQUE(issuer,request_id));").map_err(sql)?;
        identity::initialize(&db)?;
        let workspace = dir.join("workspace.sqlite3");
        let retirement = if workspace.is_file() {
            let team =
                Connection::open_with_flags(workspace, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(sql)?;
            let migrated=team.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='account_retirements' AND type='table')",[],|r|r.get::<_,bool>(0)).map_err(sql)?;
            migrated.then_some(team)
        } else {
            None
        };
        Ok(Self(db, retirement))
    }
    pub fn issue(
        &self,
        actor: &str,
        scope: &str,
        label: &str,
        spec: &Value,
    ) -> Result<Value, String> {
        if actor.is_empty()
            || actor.len() > 128
            || actor.chars().any(char::is_control)
            || label.len() > 200
            || !matches!(scope, "read" | "write" | "worker" | "hook")
        {
            return Err("invalid: check actor, scope and label".into());
        }
        let mut binding = None;
        if scope == "hook" {
            let space = spec["space"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or("invalid: hook space is required")?;
            let resource = spec["resource"]
                .as_str()
                .filter(|v| matches!(*v, "automation" | "connector"))
                .ok_or("invalid: hook resource must be automation or connector")?;
            let id = spec["binding"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or("invalid: hook binding ID is required")?;
            binding = Some((space, resource, id));
        }
        let tx = self.0.unchecked_transaction().map_err(sql)?;
        let (id, token) = Self::new_token(&tx, actor, scope, label)?;
        if let Some((space, resource, target)) = binding {
            tx.execute(
                "INSERT INTO token_bindings(token_id,space,resource,binding) VALUES(?1,?2,?3,?4)",
                params![id, space, resource, target],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(json!({"id":id,"actor":actor,"scope":scope,"label":label,"token":token}))
    }
    fn new_token(
        db: &Connection,
        actor: &str,
        scope: &str,
        label: &str,
    ) -> Result<(String, String), String> {
        let id = Uuid::new_v4().to_string();
        let token = format!("sf_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        db.execute("INSERT INTO tokens(id,digest,actor,scope,label,created) VALUES(?1,?2,?3,?4,?5,unixepoch())",
            params![id,fingerprint(&token),actor,scope,label]).map_err(sql)?;
        Ok((id, token))
    }
    pub fn list(&self) -> Result<Value, String> {
        let mut s=self.0.prepare("SELECT id,actor,scope,label,created,revoked,b.space,b.resource,b.binding FROM tokens LEFT JOIN token_bindings b ON b.token_id=tokens.id ORDER BY created,id").map_err(sql)?;
        let rows=s.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"actor":r.get::<_,String>(1)?,
        "scope":r.get::<_,String>(2)?,"label":r.get::<_,String>(3)?,"created":r.get::<_,i64>(4)?,"revoked":r.get::<_,Option<i64>>(5)?,
        "space":r.get::<_,Option<String>>(6)?,"resource":r.get::<_,Option<String>>(7)?,"binding":r.get::<_,Option<String>>(8)?}))).map_err(sql)?;
        Ok(Value::Array(
            rows.collect::<Result<Vec<_>, _>>().map_err(sql)?,
        ))
    }
    pub fn revoke(&self, id: &str) -> Result<Value, String> {
        let n = self
            .0
            .execute(
                "UPDATE tokens SET revoked=COALESCE(revoked,unixepoch()) WHERE id=?1",
                [id],
            )
            .map_err(sql)?;
        if n == 0 {
            return Err("not_found: access token".into());
        }
        Ok(json!({"id":id,"revoked":true}))
    }
    pub fn authenticate(&self, token: &str) -> Result<Principal, String> {
        if token.len() > 512 {
            return Err("unauthorized: invalid or revoked access token".into());
        }
        let principal=self.0.query_row("SELECT t.actor,t.scope,t.id,b.space,b.resource,b.binding,p.token_id IS NOT NULL
            FROM tokens t LEFT JOIN token_bindings b ON b.token_id=t.id LEFT JOIN personal_tokens p ON p.token_id=t.id
            LEFT JOIN identity_sessions s ON s.token_id=t.id
            LEFT JOIN tokens issuer ON p.issuer=issuer.id WHERE t.digest=?1 AND t.revoked IS NULL
            AND (p.expires IS NULL OR p.expires>unixepoch()) AND (s.expires IS NULL OR s.expires>unixepoch()) AND issuer.revoked IS NULL",[fingerprint(token)],
            |r| Ok(Principal{actor:r.get(0)?,scope:r.get(1)?,id:r.get(2)?,bound_space:r.get(3)?,resource:r.get(4)?,binding:r.get(5)?,
                personal:r.get(6)?,prefix:token.chars().take(11).collect()})).optional().map_err(sql)?
            .ok_or_else(||"unauthorized: invalid, expired or revoked access token".to_owned())?;
        if let Some(team) = &self.1
            && team
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM account_retirements WHERE actor=?)",
                    [&principal.actor],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(sql)?
        {
            return Err("unauthorized: account has been closed".into());
        }
        if principal.personal {
            self.0.execute("UPDATE personal_tokens SET last_used=unixepoch() WHERE token_id=?1 AND last_used IS NOT unixepoch()",
                [&principal.id]).map_err(sql)?;
        }
        Ok(principal)
    }
}

pub fn execute(
    auth: &Auth,
    principal: &Principal,
    store: &mut shipforge_core::Store,
    request: &Value,
) -> Result<Value, String> {
    let op = request["op"].as_str().unwrap_or("");
    if op == "access.whoami" {
        let verified = store.has_verified_profile(&principal.actor)?;
        let mut value = json!({"actor":principal.actor,"scope":principal.scope,"token_id":principal.id,"prefix":principal.prefix,"personal":principal.personal,"email_account":verified,"mode":"team"});
        if verified && !principal.personal && matches!(principal.scope.as_str(), "read" | "write") {
            let profile = store.account_profile(&principal.actor)?;
            value["display"] = json!(format!(
                "{} · {}",
                profile["name"].as_str().unwrap_or_default(),
                profile["email"].as_str().unwrap_or_default()
            ));
        }
        return Ok(value);
    }
    if op.starts_with("access.personal.") {
        return auth.personal_request(principal, store, op, request);
    }
    if op.starts_with("access.account.") {
        return auth.account(principal, store, request);
    }
    if principal.personal || principal.actor != "owner" || principal.scope != "write" {
        return Err("forbidden: only the installation owner can manage access tokens".into());
    }
    match op {
        "access.issue" => auth.issue(
            request["actor"].as_str().unwrap_or(""),
            request["scope"].as_str().unwrap_or("read"),
            request["label"].as_str().unwrap_or(""),
            request,
        ),
        "access.list" => auth.list(),
        "access.revoke" => auth.revoke(request["id"].as_str().unwrap_or("")),
        _ => Err("invalid: unknown access operation".into()),
    }
}

pub fn authorize(principal: &Principal, request: &Value) -> Result<(), String> {
    let op = request["op"].as_str().unwrap_or("");
    if matches!(
        op,
        "organization.depart" | "organization.ownership" | "space.transfer"
    ) && principal.personal
    {
        return Err(
            "forbidden: sign in with a member session to change ownership or depart".into(),
        );
    }
    let allowed = match principal.scope.as_str() {
        "write" => true,
        "read" => shipforge_core::is_read_only(op),
        "worker" => matches!(
            op,
            "machine.register"
                | "machine.heartbeat"
                | "machine.stopped"
                | "machine.runtime.jobs"
                | "machine.runtime.finish"
                | "machine.runtime.publish"
                | "skill.jobs"
                | "skill.finish"
                | "run.claim"
                | "run.heartbeat"
                | "run.report"
                | "run.discussion"
                | "run.tool"
                | "automation.tick"
        ),
        "hook" => {
            principal.bound_space.as_deref() == request["space"].as_str()
                && principal.binding.as_deref() == request["id"].as_str()
                && matches!(
                    (principal.resource.as_deref(), op),
                    (Some("automation"), "automation.fire")
                        | (Some("connector"), "provider.ingest")
                )
        }
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err("forbidden: token scope does not allow this operation".into())
    }
}
