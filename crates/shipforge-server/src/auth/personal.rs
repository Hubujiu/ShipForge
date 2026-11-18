use super::{Auth, Principal, sql};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use shipforge_core::{Store, personal_tokens::PersonalTokenSpec};

impl Auth {
    pub(super) fn personal_request(
        &self,
        p: &Principal,
        store: &mut Store,
        op: &str,
        q: &Value,
    ) -> Result<Value, String> {
        if !matches!(p.scope.as_str(), "read" | "write") {
            return Err("forbidden: a member identity is required".into());
        }
        match op {
            "access.personal.issue" if !p.personal => self.issue_personal(p, q),
            "access.personal.issue" => {
                Err("forbidden: use your team sign-in credential to issue a personal token".into())
            }
            "access.personal.list" => self.list_personal(p, store),
            "access.personal.revoke" => {
                let id = q["id"].as_str().ok_or("invalid: token id is required")?;
                if p.personal && id != p.id {
                    return Err(
                        "forbidden: use your team sign-in credential to manage another token"
                            .into(),
                    );
                }
                let count=self.0.execute("UPDATE tokens SET revoked=COALESCE(revoked,unixepoch()) WHERE id=?1 AND actor=?2
                    AND EXISTS(SELECT 1 FROM personal_tokens WHERE token_id=tokens.id)",params![id,p.actor]).map_err(sql)?;
                if count == 0 {
                    return Err("not_found: personal access token".into());
                }
                Ok(json!({"id":id,"revoked":true}))
            }
            _ => Err("invalid: unknown personal access operation".into()),
        }
    }
    fn issue_personal(&self, p: &Principal, q: &Value) -> Result<Value, String> {
        let tx =
            Transaction::new_unchecked(&self.0, TransactionBehavior::Immediate).map_err(sql)?;
        let now = tx
            .query_row("SELECT unixepoch()", [], |r| r.get(0))
            .map_err(sql)?;
        let spec = PersonalTokenSpec::parse(q, now)?;
        if spec.source == "machine_setup" && p.scope != "write" {
            return Err("forbidden: connecting a machine requires write access".into());
        }
        if let Some(key) = spec.request_id {
            let prior=tx.query_row("SELECT t.id,t.label,p.lifetime,p.source FROM personal_tokens p JOIN tokens t ON t.id=p.token_id WHERE p.issuer=?1 AND p.request_id=?2",
                params![p.id,key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional().map_err(sql)?;
            if let Some((id, name, lifetime, source)) = prior {
                if name != spec.name || lifetime != spec.lifetime || source != spec.source {
                    return Err("conflict: request_id was used for different token settings".into());
                }
                return Ok(json!({"id":id,"already_issued":true}));
            }
        }
        let (id, token) = Self::new_token(&tx, &p.actor, &p.scope, spec.name)?;
        let prefix = &token[..11];
        tx.execute(
            "INSERT INTO personal_tokens(token_id,issuer,prefix,expires,lifetime,source,request_id)
            VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                p.id,
                prefix,
                spec.expires_at,
                spec.lifetime,
                spec.source,
                spec.request_id
            ],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(
            json!({"id":id,"label":spec.name,"prefix":prefix,"expires_at":spec.expires_at,"created_at":now,"token":token}),
        )
    }
    fn list_personal(&self, p: &Principal, store: &mut Store) -> Result<Value, String> {
        let mut stmt=self.0.prepare("SELECT t.id,t.label,t.created,p.prefix,p.expires,p.last_used,p.source,p.machine_space,p.machine_id
            FROM personal_tokens p JOIN tokens t ON t.id=p.token_id JOIN tokens issuer ON issuer.id=p.issuer
            WHERE t.actor=?1 AND t.revoked IS NULL AND issuer.revoked IS NULL AND (p.expires IS NULL OR p.expires>unixepoch())
            ORDER BY t.created DESC,t.id").map_err(sql)?;
        let rows=stmt.query_map([&p.actor],|r|Ok((json!({
            "id":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?,"prefix":r.get::<_,String>(3)?,
            "expires_at":r.get::<_,Option<i64>>(4)?,"last_used_at":r.get::<_,Option<i64>>(5)?,"source":r.get::<_,String>(6)?,
            "can_revoke":!p.personal || r.get::<_,String>(0)?==p.id
        }),r.get::<_,Option<String>>(7)?,r.get::<_,Option<String>>(8)?))).map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (mut token, space, id) = row.map_err(sql)?;
            if let (Some(space), Some(id)) = (space, id) {
                match store.execute(
                    &p.actor,
                    json!({"op":"get","kind":"machine","space":space,"id":id}),
                ) {
                    Ok(machine) => {
                        token["machine"] = json!({"id":id,"space":space,"name":machine["title"],"online":shipforge_core::machine_is_online(&machine["fields"]),
                        "disabled":machine["fields"]["disabled"],"last_seen":machine["fields"]["last_seen"]});
                    }
                    Err(e) if e.starts_with("not_found:") || e.starts_with("forbidden:") => {}
                    Err(e) => return Err(e),
                }
            }
            out.push(token);
        }
        Ok(Value::Array(out))
    }
    pub(crate) fn associate_machine(
        &self,
        p: &Principal,
        space: &str,
        id: &str,
    ) -> Result<(), String> {
        if p.personal {
            self.0.execute("UPDATE personal_tokens SET machine_space=?2,machine_id=?3 WHERE token_id=?1 AND source='machine_setup' AND machine_id IS NULL",
                params![p.id,space,id]).map_err(sql)?;
        }
        Ok(())
    }
}
