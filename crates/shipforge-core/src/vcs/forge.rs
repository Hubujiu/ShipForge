//! Repository subscriptions and verified upstream identities. Secrets are on the service host.
use super::*;
use crate::{domain, forbidden};
mod events;
mod hooks;
mod publish;
pub(crate) fn require_legacy(c: &Context<'_>, id: &str) -> Result<()> {
    if c.db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM forge_pairs WHERE space=? AND id=?)",
            params![c.space, id],
            |r| r.get::<_, bool>(0),
        )
        .map_err(storage)?
    {
        Err(forbidden("use the verified repository connection"))
    } else {
        Ok(())
    }
}
pub(crate) const READS: &[&str] = &["forge.status", "forge.thread"];
pub(crate) const WRITES: &[&str] = &["forge.internal", "forge.unlink"];
pub(super) fn init(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS forge_installers(installation TEXT PRIMARY KEY,actor TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS forge_accounts(org TEXT NOT NULL,platform TEXT NOT NULL,owner TEXT NOT NULL,account TEXT NOT NULL,login TEXT NOT NULL,instance TEXT NOT NULL,installation TEXT NOT NULL,credential TEXT NOT NULL,expires TEXT,PRIMARY KEY(org,platform,owner));
      CREATE UNIQUE INDEX IF NOT EXISTS forge_github_install ON forge_accounts(installation) WHERE owner='' AND platform='github';
      CREATE UNIQUE INDEX IF NOT EXISTS forge_person ON forge_accounts(org,platform,instance,account) WHERE owner!='';
      CREATE TABLE IF NOT EXISTS forge_pairs(id TEXT PRIMARY KEY,org TEXT NOT NULL,space TEXT NOT NULL,platform TEXT NOT NULL,instance TEXT NOT NULL,installation TEXT NOT NULL,repository TEXT NOT NULL,path TEXT NOT NULL,public INTEGER NOT NULL,direction TEXT NOT NULL,backlink INTEGER NOT NULL,pr INTEGER NOT NULL,commits INTEGER NOT NULL,started INTEGER NOT NULL,actor TEXT NOT NULL,hook_hash TEXT NOT NULL,active INTEGER NOT NULL);
      CREATE UNIQUE INDEX IF NOT EXISTS forge_pair ON forge_pairs(org,platform,instance,repository,space) WHERE active=1;
      CREATE UNIQUE INDEX IF NOT EXISTS forge_twoway ON forge_pairs(space,platform) WHERE active=1 AND direction='both';
      CREATE TABLE IF NOT EXISTS forge_issues(pair TEXT NOT NULL,number INTEGER NOT NULL,task TEXT NOT NULL,url TEXT NOT NULL,active INTEGER NOT NULL,PRIMARY KEY(pair,number));
      CREATE TABLE IF NOT EXISTS forge_issue_authors(pair TEXT NOT NULL,number INTEGER NOT NULL,account TEXT NOT NULL,login TEXT NOT NULL,PRIMARY KEY(pair,number));
      CREATE TABLE IF NOT EXISTS forge_notes(pair TEXT NOT NULL,number INTEGER NOT NULL,note TEXT NOT NULL,author TEXT NOT NULL,login TEXT NOT NULL,body TEXT NOT NULL,updated INTEGER NOT NULL,PRIMARY KEY(pair,number,note));
      CREATE TABLE IF NOT EXISTS forge_note_deletions(pair TEXT NOT NULL,number INTEGER NOT NULL,note TEXT NOT NULL,PRIMARY KEY(pair,number,note));
      CREATE TABLE IF NOT EXISTS forge_outbox(id TEXT PRIMARY KEY,pair TEXT NOT NULL,number INTEGER NOT NULL,task TEXT NOT NULL,author TEXT NOT NULL,body TEXT NOT NULL,kind TEXT NOT NULL,remote TEXT,phase TEXT NOT NULL,lease TEXT,expires INTEGER NOT NULL,request TEXT NOT NULL,UNIQUE(pair,author,request));
      CREATE TABLE IF NOT EXISTS forge_sent(delivery TEXT PRIMARY KEY,identity TEXT NOT NULL,fallback INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS forge_receipts(pair TEXT NOT NULL,event TEXT NOT NULL,fingerprint TEXT NOT NULL,result TEXT NOT NULL,PRIMARY KEY(pair,event));").map_err(storage)
}
fn platform(q: &Value) -> Result<&str> {
    let p = required(q, "id")?;
    if matches!(p, "github" | "gitlab" | "gitlab-self") {
        Ok(p)
    } else {
        Err(invalid("select github, gitlab or gitlab-self"))
    }
}
fn org_admin(c: &Context<'_>) -> Result<()> {
    let org = c.org_id()?;
    Context { space: &org, ..*c }
        .require_admin()
        .map_err(|_| forbidden("organization Owner or Admin required"))
}
fn provider(p: &str) -> &str {
    if p == "github" { "github" } else { "gitlab" }
}
fn account(c: &Context<'_>, p: &str, owner: &str) -> Result<Option<Value>> {
    c.db.query_row("SELECT account,login,instance,installation,credential,expires FROM forge_accounts WHERE org=? AND platform=? AND owner=?",params![c.org_id()?,p,owner],|r|Ok(json!({"account":r.get::<_,String>(0)?,"login":r.get::<_,String>(1)?,"instance":r.get::<_,String>(2)?,"installation":r.get::<_,String>(3)?,"credential":r.get::<_,String>(4)?,"expires":r.get::<_,Option<String>>(5)?}))).optional().map_err(storage)
}
const PAIR: &str = "SELECT id,space,platform,instance,installation,repository,path,public,direction,backlink,pr,commits,started,actor,active FROM forge_pairs";
fn pair_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":r.get::<_,String>(0)?,"space":r.get::<_,String>(1)?,"platform":r.get::<_,String>(2)?,"instance":r.get::<_,String>(3)?,"installation":r.get::<_,String>(4)?,"repository_id":r.get::<_,String>(5)?,"repository":r.get::<_,String>(6)?,"public":r.get::<_,bool>(7)?,"direction":r.get::<_,String>(8)?,"backlinks":r.get::<_,bool>(9)?,"pull_requests":r.get::<_,bool>(10)?,"commits":r.get::<_,bool>(11)?,"started":r.get::<_,i64>(12)?,"actor":r.get::<_,String>(13)?,"active":r.get::<_,bool>(14)?}),
    )
}
fn pair(c: &Context<'_>, p: &str, id: &str) -> Result<Value> {
    c.db.query_row(
        &format!("{PAIR} WHERE id=? AND space=? AND org=? AND platform=? AND active=1"),
        params![id, c.space, c.org_id()?, p],
        pair_row,
    )
    .optional()
    .map_err(storage)?
    .ok_or_else(|| "not_found: active repository pairing".into())
}
fn config(c: &Context<'_>, p: &str, internal: bool) -> Result<Value> {
    let org = c.org_id()?;
    let mut a = account(c, p, "")?.unwrap_or(Value::Null);
    let mut who = account(c, p, c.actor)?.unwrap_or(Value::Null);
    if !internal {
        for v in [&mut a, &mut who] {
            if let Some(m) = v.as_object_mut() {
                m.remove("credential");
            }
        }
    }
    let mut stmt =
        c.db.prepare(&format!(
            "{PAIR} WHERE space=? AND org=? AND platform=? AND active=1 ORDER BY path"
        ))
        .map_err(storage)?;
    let pairs = stmt
        .query_map(params![c.space, org, p], pair_row)
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    Ok(
        json!({"organization":org,"actor":c.actor,"can_admin":org_admin(c).is_ok(),"can_pair":org_admin(c).is_ok()&&c.require_admin().is_ok(),"account":a,"personal":who,"pairs":pairs}),
    )
}
fn disconnect_pairs(c: &Context<'_>, org: &str, p: &str, pair: Option<&str>) -> Result<()> {
    c.db.execute("UPDATE forge_outbox SET phase='cancelled',lease=NULL WHERE phase NOT IN('sent','cancelled') AND pair IN(SELECT id FROM forge_pairs WHERE org=?1 AND platform=?2 AND (?3 IS NULL OR id=?3))",params![org,p,pair]).map_err(storage)?;
    c.db.execute(
        "UPDATE forge_pairs SET active=0 WHERE org=?1 AND platform=?2 AND (?3 IS NULL OR id=?3)",
        params![org, p, pair],
    )
    .map_err(storage)?;
    Ok(())
}
fn change_account(c: &Context<'_>, p: &str, q: &Value, disconnect: bool) -> Result<Value> {
    let org = c.org_id()?;
    let owner = match required(q, "purpose")? {
        "organization" => {
            org_admin(c)?;
            ""
        }
        "personal" => {
            c.require_edit()?;
            c.actor
        }
        _ => return Err(invalid("invalid connection purpose")),
    };
    let old = account(c, p, owner)?;
    let current_org = account(c, p, "")?.unwrap_or(Value::Null);
    if current_org["credential"] != q["expected_org"]
        || old.as_ref().unwrap_or(&Value::Null)["credential"] != q["expected_owner"]
    {
        return Err(conflict("source authorization changed; start again"));
    }

    if disconnect {
        let mut stmt=c.db.prepare("SELECT credential FROM forge_accounts WHERE org=? AND platform=? AND (?3='' OR owner=?3)").map_err(storage)?;
        let removed = stmt
            .query_map(params![org, p, owner], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        drop(stmt);
        if owner.is_empty() {
            disconnect_pairs(c, &org, p, None)?;
            c.db.execute(
                "DELETE FROM forge_accounts WHERE org=? AND platform=?",
                params![org, p],
            )
            .map_err(storage)?;
        } else {
            c.db.execute(
                "DELETE FROM forge_accounts WHERE org=? AND platform=? AND owner=?",
                params![org, p, owner],
            )
            .map_err(storage)?;
        }
        return Ok(json!({"connected":false,"generations":removed}));
    }
    let instance = required(q, "instance")?;
    let user = required(q, "account")?;
    let installation = required(q, "installation")?;
    if !owner.is_empty() {
        let a = account(c, p, "")?.ok_or_else(|| invalid("connect the organization first"))?;
        if a["instance"] != instance || a["installation"] != installation {
            return Err(conflict("organization connection changed"));
        }
        if c.db.query_row("SELECT EXISTS(SELECT 1 FROM forge_accounts WHERE org=? AND platform=? AND instance=? AND account=? AND owner!='' AND owner!=?)",params![org,p,instance,user,owner],|r|r.get::<_,bool>(0)).map_err(storage)?{return Err(conflict("this platform identity already belongs to a different member"));}
    } else if old.as_ref().is_some_and(|a| {
        a["instance"] != instance || a["installation"] != installation || a["account"] != user
    }) {
        return Err(conflict(
            "disconnect the existing organization account before replacing it",
        ));
    }
    let mut replaced = Value::Null;
    if !owner.is_empty() && matches!(p, "gitlab" | "gitlab-self") {
        let other = if p == "gitlab" {
            "gitlab-self"
        } else {
            "gitlab"
        };
        if let Some(prior) = account(c, other, owner)? {
            c.db.execute(
                "DELETE FROM forge_accounts WHERE org=? AND platform=? AND owner=?",
                params![org, other, owner],
            )
            .map_err(storage)?;
            replaced = json!({"platform":other,"credential":prior["credential"]});
        }
    }
    c.db.execute("INSERT INTO forge_accounts(org,platform,owner,account,login,instance,installation,credential,expires) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(org,platform,owner) DO UPDATE SET account=excluded.account,login=excluded.login,instance=excluded.instance,installation=excluded.installation,credential=excluded.credential,expires=excluded.expires",params![org,p,owner,user,required(q,"login")?,instance,installation,required(q,"credential")?,q["expires"].as_str()]).map_err(storage)?;
    Ok(
        json!({"connected":true,"account":user,"login":q["login"],"purpose":q["purpose"],"expires":q["expires"],"replaced":replaced}),
    )
}
fn pairing(c: &Context<'_>, p: &str, q: &Value) -> Result<Value> {
    org_admin(c)?;
    c.require_admin()?;
    let org = c.org_id()?;
    if q["direction"] == "off" {
        let b = pair(c, p, required(q, "pair")?)?;
        disconnect_pairs(c, &org, p, b["id"].as_str())?;
        return Ok(json!({"active":false,"pair":b["id"]}));
    }
    let a = account(c, p, "")?.ok_or_else(|| invalid("connect the organization first"))?;
    if a["credential"] != q["credential"] {
        return Err(conflict("organization authorization changed"));
    }
    let direction = required(q, "direction")?;
    if !matches!(direction, "incoming" | "both") {
        return Err(invalid("choose incoming or both"));
    }
    let public = q["public"]
        .as_bool()
        .ok_or_else(|| invalid("repository visibility required"))?;
    let backlinks = q["backlinks"].as_bool().unwrap_or(!public);
    if public
        && (direction == "both" || backlinks)
        && c.get("space", c.space)?.fields["visibility"] != "public"
        && q["confirm_public"] != true
    {
        return Err(conflict(
            "confirm_public is required to publish private-space data to a public repository",
        ));
    }
    let repo = required(q, "repository_id")?;
    let path = required(q, "repository")?;
    let current:Option<String>=c.db.query_row("SELECT id FROM forge_pairs WHERE org=? AND space=? AND platform=? AND instance=? AND repository=? AND active=1",params![org,c.space,p,a["instance"].as_str(),repo],|r|r.get(0)).optional().map_err(storage)?;
    let id = current
        .clone()
        .unwrap_or_else(|| q["pair_nonce"].as_str().unwrap_or("").to_owned());
    if direction=="both"&&c.db.query_row("SELECT EXISTS(SELECT 1 FROM forge_pairs WHERE space=? AND platform=? AND direction='both' AND active=1 AND id!=?)",params![c.space,p,id],|r|r.get::<_,bool>(0)).map_err(storage)?{return Err(conflict("only one two-way repository per space and platform"));}
    Uuid::parse_str(&id).map_err(|_| invalid("invalid pairing ID"))?;
    let pr = q["pull_requests"].as_bool().unwrap_or(true);
    let commits = q["commits"].as_bool().unwrap_or(true);
    if current.is_some() {
        c.db.execute("UPDATE forge_pairs SET direction=?,public=?,backlink=?,pr=?,commits=?,path=?,actor=? WHERE id=?",params![direction,public,backlinks,pr,commits,path,c.actor,id]).map_err(storage)?;
        let original = c.get("connector", &id)?;
        if original.fields["repository"] != path {
            let mut fields = original.fields.clone();
            fields["repository"] = json!(path);
            c.update(&original, path, fields)?;
        }
    } else {
        let host = required(q, "host")?;
        let fields = json!({"provider":provider(p),"repository":path,"link_tasks":true,"enabled":true,"forge_pair":id,"repository_host":if p=="github"{Value::Null}else{json!(host)},"project_id":if p=="github"{Value::Null}else{json!(repo)}});
        domain::write(
            c,
            &json!({"op":"create","kind":"connector","id":id,"title":path,"fields":fields}),
        )?;
        c.db.execute("INSERT INTO forge_pairs(id,org,space,platform,instance,installation,repository,path,public,direction,backlink,pr,commits,started,actor,hook_hash,active) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1)",params![id,org,c.space,p,a["instance"].as_str(),a["installation"].as_str(),repo,path,public,direction,backlinks,pr,commits,crate::now(),c.actor,required(q,"hook_hash")?]).map_err(storage)?;
    }
    if direction != "both" {
        c.db.execute("UPDATE forge_outbox SET phase='cancelled',lease=NULL WHERE pair=? AND kind='reply' AND phase NOT IN('sent','cancelled')",[&id]).map_err(storage)?;
    }
    if !backlinks {
        c.db.execute("UPDATE forge_outbox SET phase='cancelled',lease=NULL WHERE pair=? AND kind IN('backlink','review_link') AND phase NOT IN('sent','cancelled')",[&id]).map_err(storage)?;
    }
    pair(c, p, &id)
}
pub(crate) fn command(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let p = platform(q)?;
    match op {
        "forge.status" => config(c, p, false),
        "forge.thread" => events::thread(c, p, q),
        "forge.unlink" => events::unlink(c, p, q),
        "forge.internal" => match required(q, "action")? {
            "config" => config(c, p, true),
            "installer" => {
                org_admin(c)?;
                let actor: Option<String> =
                    c.db.query_row(
                        "SELECT actor FROM forge_installers WHERE installation=?",
                        [required(q, "installation")?],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(storage)?;
                Ok(json!(actor))
            }
            "account" => change_account(c, p, q, false),
            "disconnect" => change_account(c, p, q, true),
            "pair" => pairing(c, p, q),
            "push_plan" | "refresh_plan" | "sync_event" => {
                let map = events::mapping(c, p, q)?;
                let row = c.readable("task", required(q, "task_id")?)?;
                c.authorize_write("task", Some(&row))?;
                if q["action"] == "push_plan" && map["pair"]["direction"] != "both" {
                    return Err(forbidden("outgoing issue changes disabled"));
                }
                if q["action"] == "sync_event" {
                    let num = if p == "github" {
                        &q["payload"]["issue"]["number"]
                    } else {
                        &q["payload"]["object_attributes"]["iid"]
                    };
                    if *num != map["number"] {
                        return Err(forbidden("an existing mapped issue is required"));
                    }
                    events::ingest(
                        &Context {
                            actor: required(&map["pair"], "actor")?,
                            ..*c
                        },
                        p,
                        q,
                    )
                } else {
                    Ok(map)
                }
            }
            "event" => events::ingest(c, p, q),
            "reserve" | "settle" | "drain" | "can_publish" => publish::command(c, p, q),
            "mapping" => events::mapping(c, p, q),
            _ => Err(invalid("unknown repository service action")),
        },
        _ => Err(invalid("unknown repository operation")),
    }
}
pub(crate) fn schema(s: &mut Value) {
    let rules = s["allOf"].as_array_mut().unwrap();
    for (op, req) in [
        ("forge.status", vec!["id"]),
        ("forge.thread", vec!["id", "task_id"]),
        ("forge.unlink", vec!["id", "task_id", "pair"]),
        ("forge.internal", vec!["id", "action"]),
    ] {
        rules.push(json!({"if":{"properties":{"op":{"const":op}}},"then":{"required":req,"properties":{"id":{"enum":["github","gitlab","gitlab-self"]}}}}));
    }
}
