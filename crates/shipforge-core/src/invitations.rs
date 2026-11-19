use crate::{
    Context, Record, Result, as_value, conflict, forbidden, identifier, invalid, now, required,
    storage,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

const PREFIX: &str = "shipforge://invite/";

pub(crate) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS invitation_keys(id TEXT PRIMARY KEY, org TEXT NOT NULL, secret BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS invitation_uses(id TEXT NOT NULL,actor TEXT NOT NULL,PRIMARY KEY(id,actor));
        CREATE TABLE IF NOT EXISTS invitation_requests(org TEXT NOT NULL,actor TEXT NOT NULL,request TEXT NOT NULL,body TEXT NOT NULL,id TEXT NOT NULL,PRIMARY KEY(org,actor,request));").map_err(storage)
}
fn secret() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| storage("unable to generate invitation"))?;
    Ok(bytes)
}
fn organization<'a>(c: &Context<'a>, id: &'a str) -> Context<'a> {
    Context { space: id, ..*c }
}
fn unavailable() -> String {
    forbidden("invitation is unavailable")
}
fn check_targets(c: &Context<'_>, spaces: &Value) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in crate::domain::strings(spaces, "spaces")? {
        identifier(id)?;
        let target = organization(c, id);
        if !seen.insert(id) || target.org_id()? != c.space {
            return Err(invalid("choose distinct spaces in this organization"));
        }
        target.require_admin()?;
        crate::spaces::ensure_active(&target)?;
    }
    Ok(())
}
fn link(c: &Context<'_>, record: Record) -> Result<Value> {
    let key: Vec<u8> =
        c.db.query_row(
            "SELECT secret FROM invitation_keys WHERE id=? AND org=?",
            params![record.id, c.space],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let signature = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, &key),
        record.id.as_bytes(),
    );
    Ok(
        json!({"link":format!("{PREFIX}{}.{}",record.id,URL_SAFE_NO_PAD.encode(signature.as_ref())),"invitation":record}),
    )
}
pub(crate) fn active(row: &Record) -> bool {
    row.fields["state"] == "active"
        && row.fields["expires_at"]
            .as_i64()
            .is_none_or(|at| at > now())
}
fn issue(c: &Context<'_>, q: &Value) -> Result<Value> {
    let channel = required(q, "channel")?;
    let role = required(q, "role")?;
    if !matches!(
        (channel, role),
        ("link", "member" | "guest") | ("email", "member" | "guest" | "admin")
    ) {
        return Err(invalid(
            "links allow Member or Guest; addressed invitations also allow Admin",
        ));
    }
    let address = if channel == "email" {
        let address = required(q, "email")?.trim().to_ascii_lowercase();
        identifier(&address)?;
        if !address.split_once('@').is_some_and(|(name, domain)| {
            !name.is_empty() && !domain.is_empty() && !domain.contains('@')
        }) || address.chars().any(char::is_whitespace)
        {
            return Err(invalid("enter an email address"));
        }
        Some(address)
    } else {
        None
    };
    let spaces = q.get("spaces").cloned().unwrap_or_else(|| json!([]));
    check_targets(c, &spaces)?;
    let requested_expiry = q
        .get("expires_at")
        .map(|v| {
            v.as_i64()
                .ok_or_else(|| invalid("expires_at must be a Unix millisecond timestamp"))
        })
        .transpose()?;
    let fingerprint = json!([channel, role, address, spaces, requested_expiry]).to_string();
    let request = q
        .get("request_id")
        .map(|_| required(q, "request_id"))
        .transpose()?;
    if let Some(request) = request {
        identifier(request)?;
        let previous: Option<(String, String)> =
            c.db.query_row(
                "SELECT body,id FROM invitation_requests WHERE org=? AND actor=? AND request=?",
                params![c.space, c.actor, request],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((body, id)) = previous {
            if body != fingerprint {
                return Err(conflict("request_id already identifies another invitation"));
            }
            let row = c.get("invitation", &id)?;
            if !active(&row) {
                return Err(unavailable());
            }
            return link(c, row);
        }
    }
    if requested_expiry.is_some_and(|at| at <= now()) {
        return Err(invalid("invitation must expire in the future"));
    }
    let expires =
        requested_expiry.or_else(|| (channel == "email").then(|| now() + 7 * 24 * 60 * 60 * 1000));
    let id = Uuid::new_v4().to_string();
    let title = address.as_deref().unwrap_or("组织邀请链接");
    let row = c.create(
        "invitation",
        Some(&id),
        title,
        json!({"channel":channel,"email":address,"role":role,"spaces":spaces,
        "creator":c.actor,"state":"active","expires_at":expires}),
    )?;
    c.db.execute(
        "INSERT INTO invitation_keys VALUES(?,?,?)",
        params![id, c.space, secret()?.as_slice()],
    )
    .map_err(storage)?;
    if let Some(request) = request {
        c.db.execute(
            "INSERT INTO invitation_requests VALUES(?,?,?,?,?)",
            params![c.space, c.actor, request, fingerprint, id],
        )
        .map_err(storage)?;
    }
    link(c, row)
}
fn manage(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    let org_id = c.org_id()?;
    let org = organization(c, &org_id);
    org.require_admin()?;
    if op == "invitation.issue" {
        return issue(&org, q);
    }
    if op == "invitation.list" {
        return Ok(json!(
            org.records("invitation")?
                .into_iter()
                .map(|mut row| {
                    if row.fields["state"] == "active" && !active(&row) {
                        row.fields["state"] = json!("expired");
                    }
                    row
                })
                .collect::<Vec<_>>()
        ));
    }
    let id = required(q, "id")?;
    let mut row = org.get("invitation", id)?;
    if !matches!(row.fields["channel"].as_str(), Some("email" | "link")) {
        return Err(invalid("legacy invitation has no join link"));
    }
    match op {
        "invitation.copy" => {
            if row.fields["channel"] == "link" && row.fields["state"] == "revoked" {
                check_targets(&org, &row.fields["spaces"])?;
                let mut fields = row.fields.clone();
                fields["state"] = json!("active");
                fields["creator"] = json!(c.actor);
                row = org.update(&row, &row.title, fields)?;
                org.db
                    .execute(
                        "UPDATE invitation_keys SET secret=? WHERE org=? AND id=?",
                        params![secret()?.as_slice(), org_id, id],
                    )
                    .map_err(storage)?;
            }
            if !active(&row) {
                return Err(unavailable());
            }
            link(&org, row)
        }
        "invitation.revoke" => {
            if row.fields["state"] == "active" {
                let mut fields = row.fields.clone();
                fields["state"] = json!("revoked");
                row = org.update(&row, &row.title, fields)?;
            }
            Ok(as_value(row))
        }
        _ => Err(invalid("unknown invitation operation")),
    }
}
pub(crate) fn execute(c: &Context<'_>, op: &str, q: &Value) -> Result<Value> {
    if op != "invitation.accept" {
        return manage(c, op, q);
    }
    let supplied = required(q, "link")?
        .trim()
        .strip_prefix(PREFIX)
        .and_then(|v| v.split_once('.'))
        .ok_or_else(unavailable)?;
    let (id, encoded) = supplied;
    if Uuid::parse_str(id).is_err() || encoded.len() != 43 {
        return Err(unavailable());
    }
    let signature = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| unavailable())?;
    let (org_id, key): (String, Vec<u8>) =
        c.db.query_row(
            "SELECT org,secret FROM invitation_keys WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(unavailable)?;
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, &key),
        id.as_bytes(),
        &signature,
    )
    .map_err(|_| unavailable())?;
    let org = organization(c, &org_id);
    let row = org.get("invitation", id)?;
    if row.fields["channel"] == "email"
        && !row.fields["email"]
            .as_str()
            .is_some_and(|mail| mail.eq_ignore_ascii_case(c.actor))
    {
        return Err(unavailable());
    }
    let prior: bool =
        c.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM invitation_uses WHERE id=? AND actor=?)",
            params![id, c.actor],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if prior
        && (active(&row) || row.fields["state"] == "accepted")
        && org.find("member", c.actor)?.is_some()
    {
        return Ok(json!({"organization":org_id,"actor":c.actor,"joined":false}));
    }
    if !active(&row) || prior {
        return Err(unavailable());
    }
    let issuer = Context {
        actor: row.fields["creator"].as_str().ok_or_else(unavailable)?,
        ..org
    };
    issuer.require_admin().map_err(|_| unavailable())?;
    check_targets(&issuer, &row.fields["spaces"]).map_err(|_| unavailable())?;
    let role = required(&row.fields, "role")?;
    let add = |space: &Context<'_>, role: &str| -> Result<()> {
        if space.find("member", c.actor)?.is_none() {
            space.create(
                "member",
                Some(c.actor),
                c.actor,
                json!({"actor":c.actor,"role":role,"creator":issuer.actor}),
            )?;
        }
        Ok(())
    };
    let joined = org.find("member", c.actor)?.is_none();
    add(&org, role)?;
    let space_role = if role == "guest" { "guest" } else { "member" };
    for space in crate::domain::strings(&row.fields["spaces"], "spaces")? {
        add(&organization(c, space), space_role)?;
    }
    c.db.execute(
        "INSERT INTO invitation_uses VALUES(?,?)",
        params![id, c.actor],
    )
    .map_err(storage)?;
    if row.fields["channel"] == "email" {
        let mut fields = row.fields.clone();
        fields["state"] = json!("accepted");
        org.update(&row, &row.title, fields)?;
        crate::inbox::accept_invited_follower(&org, id, c.actor)?;
    }
    Ok(json!({"organization":org_id,"actor":c.actor,"joined":joined}))
}

pub(crate) fn extend_schema(schema: &mut Value) {
    schema["allOf"].as_array_mut().unwrap().extend([
        json!({"if":{"properties":{"op":{"const":"invitation.issue"}}},"then":{"required":["channel","role"],"properties":{"channel":{"enum":["email","link"]},"role":{"enum":["member","guest","admin"]},"email":{"type":"string","format":"email"},"spaces":{"type":"array","uniqueItems":true,"items":{"type":"string"}},"expires_at":{"type":"integer"}},"if":{"properties":{"channel":{"const":"email"}}},"then":{"required":["email"]},"else":{"properties":{"role":{"enum":["member","guest"]}}}}}),
        json!({"if":{"properties":{"op":{"enum":["invitation.copy","invitation.revoke"]}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"const":"invitation.accept"}}},"then":{"required":["link"],"properties":{"link":{"type":"string","pattern":"^shipforge://invite/"}}}})
    ]);
}
