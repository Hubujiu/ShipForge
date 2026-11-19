use super::*;
use rusqlite::{Transaction, TransactionBehavior};
use shipforge_core::Store;
use tiny_http::{Header, Method, Request, Response, StatusCode};
use url::Url;

pub(super) fn initialize(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS identity_challenges(id TEXT PRIMARY KEY,device BLOB NOT NULL,proof BLOB NOT NULL,pair TEXT NOT NULL,purpose TEXT NOT NULL,email TEXT NOT NULL,actor TEXT,created INTEGER NOT NULL,approved INTEGER NOT NULL DEFAULT 0,attempts INTEGER NOT NULL DEFAULT 0);
      CREATE INDEX IF NOT EXISTS identity_challenge_email ON identity_challenges(email,created);
      CREATE TABLE IF NOT EXISTS identity_login_receipts(challenge TEXT PRIMARY KEY,token_id TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS identity_sessions(token_id TEXT PRIMARY KEY,expires INTEGER NOT NULL);
      CREATE TRIGGER IF NOT EXISTS identity_expire_receipt AFTER DELETE ON identity_challenges BEGIN DELETE FROM identity_login_receipts WHERE challenge=OLD.id; END;").map_err(sql)
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or_else(|| format!("invalid: missing {key}"))
}
fn address(input: &str) -> Result<String, String> {
    let input = input.trim().to_ascii_lowercase();
    let good = input.split_once('@').is_some_and(|(a, b)| {
        !a.is_empty()
            && a.len() <= 64
            && !a.starts_with('.')
            && !a.ends_with('.')
            && !a.contains("..")
            && a.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._%+-".contains(&c))
            && b.contains('.')
            && b.split('.').all(|part| {
                !part.is_empty()
                    && !part.starts_with('-')
                    && !part.ends_with('-')
                    && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
    });
    if input.len() > 254 || !good {
        return Err("invalid: enter a valid email address".into());
    }
    Ok(input)
}
fn configured_url(name: &str, default: Option<&str>) -> Result<String, String> {
    let raw = std::env::var(name)
        .ok()
        .or_else(|| default.map(str::to_owned))
        .ok_or("unavailable: email login is not configured")?;
    let url = Url::parse(&raw).map_err(|_| "invalid: invalid identity service URL")?;
    let local = match url.host() {
        Some(url::Host::Ipv4(a)) => a.is_loopback(),
        Some(url::Host::Ipv6(a)) => a.is_loopback(),
        Some(url::Host::Domain(a)) => a == "localhost",
        None => false,
    };
    if !(url.scheme() == "https" || (local && url.scheme() == "http"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid: identity endpoints require HTTPS or loopback HTTP".into());
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}
fn send_mail(email: &str, purpose: &str, link: &str, id: &str) -> Result<(), String> {
    let endpoint = configured_url(
        "SHIPFORGE_IDENTITY_MAIL_URL",
        Some("https://api.resend.com/emails"),
    )?;
    let from = std::env::var("SHIPFORGE_IDENTITY_MAIL_FROM")
        .map_err(|_| "unavailable: email login is not configured")?;
    let key = std::env::var("SHIPFORGE_IDENTITY_MAIL_TOKEN")
        .map_err(|_| "unavailable: email login is not configured")?;
    let action = if purpose == "close" {
        "验证账号注销"
    } else {
        "登录 ShipForge"
    };
    let body = json!({"from":from,"to":[email],"subject":format!("ShipForge · {action}"),"text":format!("请在十分钟内打开以下链接，输入发起操作的设备上显示的配对码。\n{link}\n如果并非你本人操作，可以忽略这封邮件。")});
    let http = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .new_agent();
    let response = http
        .post(endpoint)
        .header("Authorization", format!("Bearer {key}"))
        .header("Idempotency-Key", id)
        .send_json(&body)
        .map_err(|_| "unavailable: verification email could not be sent")?;
    if !response.status().is_success() {
        return Err("unavailable: verification email could not be sent".into());
    }
    Ok(())
}

fn immediate(db: &Connection) -> Result<Transaction<'_>, String> {
    Transaction::new_unchecked(db, TransactionBehavior::Immediate).map_err(sql)
}
fn invalid_challenge() -> String {
    "unauthorized: invalid, expired or used verification".into()
}
impl Auth {
    fn begin(&self, email: &str, purpose: &str, actor: Option<&str>) -> Result<Value, String> {
        let base = configured_url("SHIPFORGE_IDENTITY_BASE_URL", None)?;
        let id = Uuid::new_v4().to_string();
        let device = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let proof = Uuid::new_v4().simple().to_string();
        let pair = Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase();
        let tx = immediate(&self.0)?;
        tx.execute(
            "DELETE FROM identity_challenges WHERE created<=unixepoch()-600",
            [],
        )
        .map_err(sql)?;
        let (same,global):(i64,i64)=tx.query_row("SELECT count(*) FILTER (WHERE email=?),count(*) FROM identity_challenges WHERE created>unixepoch()-60",[email],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sql)?;
        if same >= 3 || global >= 60 {
            return Err("rate_limited: request another email later".into());
        }
        tx.execute("INSERT INTO identity_challenges(id,device,proof,pair,purpose,email,actor,created) VALUES(?,?,?,?,?,?,?,unixepoch())",params![id,fingerprint(&device),fingerprint(&proof),pair,purpose,email,actor]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        let link = format!("{base}/identity/approve?id={id}&proof={proof}");
        if let Err(error) = send_mail(email, purpose, &link, &id) {
            self.0
                .execute("DELETE FROM identity_challenges WHERE id=?", [&id])
                .map_err(sql)?;
            return Err(error);
        }
        Ok(json!({"id":id,"device":device,"pair":pair,"expires_in":600}))
    }
    fn approve(&self, form: &Value) -> Result<(), String> {
        let id = field(form, "id")?;
        let proof = fingerprint(field(form, "proof")?);
        let tx = immediate(&self.0)?;
        let pair:Option<String>=tx.query_row("SELECT pair FROM identity_challenges WHERE id=? AND proof=? AND created>unixepoch()-600 AND attempts<6 AND approved=0",params![id,proof],|r|r.get(0)).optional().map_err(sql)?;
        let valid = pair
            .ok_or_else(invalid_challenge)?
            .eq_ignore_ascii_case(form["pair"].as_str().unwrap_or("").trim());
        tx.execute(
            "UPDATE identity_challenges SET attempts=attempts+1,approved=? WHERE id=?",
            params![valid, id],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        if valid {
            Ok(())
        } else {
            Err(invalid_challenge())
        }
    }
    fn finish(&self, store: &mut Store, q: &Value, p: Option<&Principal>) -> Result<Value, String> {
        let id = field(q, "id")?;
        let device = fingerprint(field(q, "device")?);
        let tx = immediate(&self.0)?;
        let (purpose,email,actor,approved):(String,String,Option<String>,bool)=tx.query_row("SELECT purpose,email,actor,approved FROM identity_challenges WHERE id=? AND device=? AND created>unixepoch()-600 AND attempts<=6",params![id,device],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sql)?.ok_or_else(invalid_challenge)?;
        if (purpose == "close") != p.is_some()
            || p.is_some_and(|user| actor.as_deref() != Some(&user.actor))
        {
            return Err(invalid_challenge());
        }
        if !approved {
            return Err("pending: approve the email from your browser first".into());
        }
        let response = if let Some(p) = p {
            if q["confirmation"] != "注销" {
                return Err("invalid: type 注销 to permanently close the account".into());
            }
            store.retire_verified_account(&p.actor, q["dissolve"] == true)?;
            tx.execute(
                "UPDATE tokens SET revoked=COALESCE(revoked,unixepoch()) WHERE actor=?",
                [&p.actor],
            )
            .map_err(sql)?;
            tx.execute("DELETE FROM identity_challenges WHERE email=?", [&email])
                .map_err(sql)?;
            json!({"closed":true})
        } else {
            if let Some(known) = actor
                && store.verified_actor(&email)?.as_deref() != Some(&known)
            {
                return Err(invalid_challenge());
            }
            let profile = store.verified_login(&email)?;
            let user = field(&profile, "actor")?;
            use base64::Engine;
            let token = format!(
                "sf_{}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(fingerprint(&format!(
                    "shipforge-session-v1/{id}/{}",
                    field(q, "device")?
                )))
            );
            let receipt:Option<(String,bool)>=tx.query_row("SELECT t.actor,t.revoked IS NULL AND s.expires>unixepoch() FROM identity_login_receipts c JOIN tokens t ON c.token_id=t.id JOIN identity_sessions s ON s.token_id=t.id WHERE c.challenge=?",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
            match receipt {
                Some((actor, true)) if actor == user => {}
                Some(_) => return Err(invalid_challenge()),
                None => {
                    let token_id = Uuid::new_v4().to_string();
                    tx.execute("INSERT INTO tokens(id,digest,actor,scope,label,created) VALUES(?,?,?,'write','Verified desktop session',unixepoch())",params![token_id,fingerprint(&token),user]).map_err(sql)?;
                    tx.execute(
                        "INSERT INTO identity_login_receipts VALUES(?,?)",
                        params![id, token_id],
                    )
                    .map_err(sql)?;
                    tx.execute(
                        "INSERT INTO identity_sessions VALUES(?,unixepoch()+30*86400)",
                        [token_id],
                    )
                    .map_err(sql)?;
                    tx.execute(
                        "UPDATE identity_challenges SET actor=? WHERE id=?",
                        params![user, id],
                    )
                    .map_err(sql)?;
                }
            }
            json!({"actor":user,"token":token})
        };
        tx.commit().map_err(sql)?;
        Ok(response)
    }
    pub(super) fn account(
        &self,
        p: &Principal,
        store: &mut Store,
        q: &Value,
    ) -> Result<Value, String> {
        if !matches!(p.scope.as_str(), "write" | "read") || p.personal {
            return Err("forbidden: sign in to the account to manage it".into());
        }
        match q["op"].as_str() {
            Some("access.account.profile") => store.account_profile(&p.actor),
            Some("access.account.impact") => store.account_departure(&p.actor),
            _ if p.scope != "write" => Err("forbidden: a write session is required".into()),
            Some("access.account.update") => store.update_account_profile(&p.actor, q),
            Some("access.account.logout") => {
                store.account_profile(&p.actor)?;
                self.revoke(&p.id)
            }
            Some("access.account.verify") => self.begin(
                field(&store.account_profile(&p.actor)?, "email")?,
                "close",
                Some(&p.actor),
            ),
            Some("access.account.close") => self.finish(store, q, Some(p)),
            _ => Err("invalid: unknown account operation".into()),
        }
    }
}

fn page(title: &str, body: &str) -> String {
    let css = ":root{color-scheme:light dark;font-family:Inter,'Microsoft YaHei UI',system-ui,sans-serif}*{box-sizing:border-box}body{margin:0;background:Canvas;color:CanvasText}main{max-width:32rem;margin:clamp(3rem,14vh,10rem) auto;padding:0 1.5rem}h1{font-size:1.65rem;line-height:1.25}p{line-height:1.7}form{display:grid;gap:1rem}label{display:grid;gap:.45rem}input,button,a{font:inherit}input{width:100%;padding:.7rem;border:1px solid GrayText;border-radius:.4rem;letter-spacing:.15em}button,a{justify-self:start;border:0;border-radius:.4rem;padding:.65rem 1rem;background:#514674;color:white}a{display:inline-block}input:focus-visible,button:focus-visible,a:focus-visible{outline:3px solid #8873c3;outline-offset:3px}";
    format!(
        r#"<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title} · ShipForge</title><style>{css}</style></head><body><main><h1>{title}</h1>{body}</main></body></html>"#
    )
}
fn serve(
    r: &mut Request,
    auth: &Auth,
    store: &mut Store,
) -> Result<(&'static str, String), String> {
    let (path, query) = r.url().split_once('?').unwrap_or((r.url(), ""));
    if path == "/identity/approve" && r.method() == &Method::Get {
        let args = url::form_urlencoded::parse(query.as_bytes())
            .collect::<std::collections::HashMap<_, _>>();
        let id = args.get("id").ok_or_else(invalid_challenge)?;
        let proof = args.get("proof").ok_or_else(invalid_challenge)?;
        if Uuid::parse_str(id).is_err()
            || proof.len() != 32
            || !proof.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(invalid_challenge());
        }
        let purpose:Option<String>=auth.0.query_row("SELECT purpose FROM identity_challenges WHERE id=? AND proof=? AND created>unixepoch()-600 AND approved=0",params![id.as_ref(),fingerprint(proof)],|row|row.get(0)).optional().map_err(sql)?;
        let title = if purpose.ok_or_else(invalid_challenge)? == "close" {
            "验证永久注销账号"
        } else {
            "登录或注册 ShipForge"
        };
        let body = format!(
            r#"<p>请在你本人发起操作的 ShipForge 设备上查看八位配对码，然后在此确认。</p><form method="post" action="approve"><input type="hidden" name="id" value="{id}"><input type="hidden" name="proof" value="{proof}"><label>设备配对码 <input name="pair" required minlength="8" maxlength="8" autocomplete="off"></label><button type="submit">确认验证</button></form>"#
        );
        return Ok(("text/html; charset=utf-8", page(title, &body)));
    }
    let path = path.to_string();
    if r.method() != &Method::Post {
        return Err("method: use POST".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    r.as_reader()
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid: unreadable request")?;
    if bytes.len() > 8192 {
        return Err("invalid: identity request too long".into());
    }
    if path == "/identity/approve" {
        let form: serde_json::Map<_, _> = url::form_urlencoded::parse(&bytes)
            .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
            .collect();
        let form = Value::Object(form);
        auth.approve(&form)?;
        let id = Uuid::parse_str(field(&form, "id")?).map_err(|_| invalid_challenge())?;
        let body = format!(
            r#"<p>返回发起操作的设备完成操作。</p><p><a href="shipforge://identity/{id}">打开 ShipForge 桌面版</a></p>"#
        );
        return Ok(("text/html; charset=utf-8", page("验证成功", &body)));
    }
    let q = serde_json::from_slice(&bytes).map_err(|_| "invalid: expected JSON")?;
    let data = match path.as_str() {
        "/v1/identity/start" => {
            let email = address(field(&q, "email")?)?;
            let expected = q["account"].as_str();
            if expected.is_some_and(|id| {
                !id.strip_prefix("acct-")
                    .is_some_and(|uuid| Uuid::parse_str(uuid).is_ok())
            }) {
                return Err("invalid: expected a verified account ID".into());
            }
            let known = store.verified_actor(&email)?;
            auth.begin(&email, "login", expected.or(known.as_deref()))?
        }
        "/v1/identity/finish" => auth.finish(store, &q, None)?,
        _ => return Err("not_found: unknown identity route".into()),
    };
    Ok((
        "application/json",
        json!({"ok":true,"data":data}).to_string(),
    ))
}
pub(crate) fn respond(mut r: Request, auth: &Auth, store: &mut Store) {
    let (code, kind, body) = match serve(&mut r, auth, store) {
        Ok((kind, body)) => (200, kind, body),
        Err(error) => {
            let (code, message) = error
                .split_once(": ")
                .unwrap_or(("server", "operation failed"));
            let status = match code {
                "unauthorized" => 401,
                "pending" | "conflict" => 409,
                "rate_limited" => 429,
                "unavailable" => 503,
                "not_found" => 404,
                "method" => 405,
                "storage" | "server" => 500,
                _ => 400,
            };
            let message = if status == 500 {
                "identity operation failed"
            } else {
                message
            };
            (
                status,
                "application/json",
                json!({"ok":false,"error":{"code":code,"message":message}}).to_string(),
            )
        }
    };
    let mut response = Response::from_string(body).with_status_code(StatusCode(code));
    for (name, value) in [
        ("Content-Type", kind),
        ("Cache-Control", "no-store"),
        ("Referrer-Policy", "no-referrer"),
        (
            "Content-Security-Policy",
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'",
        ),
        ("X-Content-Type-Options", "nosniff"),
    ] {
        response.add_header(Header::from_bytes(name, value).unwrap());
    }
    let _ = r.respond(response);
}
