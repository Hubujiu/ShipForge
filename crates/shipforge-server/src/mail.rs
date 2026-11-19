use crate::auth::identity::configured_url;
use serde_json::{Value, json};
use shipforge_core::{MailDispatch, MailResult, Store};
use std::{path::Path, time::Duration};
use tiny_http::{Header, Method, Request, Response};

pub(crate) struct Transport {
    http: ureq::Agent,
    endpoint: String,
    from: String,
    key: String,
}
impl Transport {
    pub(crate) fn load() -> Result<Self, String> {
        let setting = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| "unavailable: team email is not configured".to_string())
        };
        Ok(Self {
            http: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_global(Some(Duration::from_secs(12)))
                .build()
                .new_agent(),
            endpoint: configured_url(
                "SHIPFORGE_IDENTITY_MAIL_URL",
                Some("https://api.resend.com/emails"),
            )?,
            from: setting("SHIPFORGE_IDENTITY_MAIL_FROM")?,
            key: setting("SHIPFORGE_IDENTITY_MAIL_TOKEN")?,
        })
    }
    fn submit(
        &self,
        key: &str,
        to: &str,
        subject: &str,
        text: &str,
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        let body = json!({"from":self.from,"to":[to],"subject":subject,"text":text});
        self.http
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.key))
            .header("Idempotency-Key", key)
            .send_json(&body)
    }
    pub(crate) fn verify(
        &self,
        key: &str,
        to: &str,
        subject: &str,
        text: &str,
    ) -> Result<(), String> {
        if self
            .submit(key, to, subject, text)
            .is_ok_and(|r| r.status().is_success())
        {
            Ok(())
        } else {
            Err("unavailable: verification email could not be sent".into())
        }
    }
    fn send(&self, key: &str, to: &str, subject: &str, text: &str) -> MailResult {
        let response = self.submit(key, to, subject, text);
        let Ok(mut response) = response else {
            return MailResult::Retry { after_seconds: 2 };
        };
        let code = response.status().as_u16();
        if (200..300).contains(&code) {
            let data: Option<Value> = response
                .body_mut()
                .with_config()
                .limit(65536)
                .read_json()
                .ok();
            if let Some(id) = data
                .as_ref()
                .and_then(|v| v["id"].as_str())
                .filter(|id| !id.is_empty() && id.len() <= 256)
            {
                return MailResult::Accepted(id.into());
            }
            return MailResult::Retry { after_seconds: 5 };
        }
        if matches!(code, 408 | 409 | 425 | 429 | 500..=599) {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(2);
            return MailResult::Retry {
                after_seconds: delay,
            };
        }
        MailResult::Failed
    }
    fn deliver(&self, base: &str, job: &MailDispatch) -> MailResult {
        let english = job.language == "en";
        let (subject, description) = match (job.invitation, english) {
            (true, true) => (
                "Organization invitation",
                "Open the invitation in ShipForge and sign in with this email address.",
            ),
            (true, false) => (
                "组织邀请",
                "打开 ShipForge 并用收到邀请的邮箱登录，以查看并接受组织邀请。",
            ),
            (false, true) => (
                "Workspace activity",
                "Open the update in ShipForge with your team account.",
            ),
            (false, false) => (
                "工作区动态",
                "使用你的团队账号打开 ShipForge，查看这条动态。",
            ),
        };
        let Some(route) = job.link.strip_prefix("shipforge://") else {
            return MailResult::Failed;
        };
        let locale = if english { "en" } else { "zh-CN" };
        let link = format!("{base}/open/{locale}/{route}");
        self.send(
            &format!("shipforge-{}", job.id),
            &job.recipient,
            &format!("ShipForge · {subject}"),
            &format!("{description}\n{link}"),
        )
    }
}
pub(crate) fn start(database: &Path) -> Result<(), String> {
    // With no team mail settings a self-hosted or local-only workspace can still use copyable invitations.
    if std::env::var_os("SHIPFORGE_IDENTITY_MAIL_FROM").is_none()
        || std::env::var_os("SHIPFORGE_IDENTITY_MAIL_TOKEN").is_none()
        || std::env::var_os("SHIPFORGE_IDENTITY_BASE_URL").is_none()
    {
        return Ok(());
    }
    let transport = Transport::load()?;
    let base = configured_url("SHIPFORGE_IDENTITY_BASE_URL", None)?;
    let mut store = Store::open(database)?;
    std::thread::Builder::new()
        .name("shipforge-mail-delivery".into())
        .spawn(move || {
            loop {
                match store.claim_email() {
                    Ok(Some(job)) => {
                        let result = transport.deliver(&base, &job);
                        if let Err(error) = store.finish_email(&job, result) {
                            eprintln!("Email receipt: {error}");
                        }
                    }
                    Ok(None) => std::thread::sleep(Duration::from_secs(1)),
                    Err(error) => {
                        eprintln!("Email queue: {error}");
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn valid_invitation(link: &str) -> bool {
    link.strip_prefix("shipforge://invite/")
        .and_then(|v| v.split_once('.'))
        .is_some_and(|(id, proof)| {
            uuid::Uuid::parse_str(id).is_ok()
                && id.len() == 36
                && proof.len() == 43
                && proof
                    .bytes()
                    .all(|v| v.is_ascii_alphanumeric() || matches!(v, b'-' | b'_'))
        })
}
pub(crate) fn landing(request: Request) {
    let name = request
        .url()
        .strip_prefix("/open/")
        .and_then(|s| s.split_once('/'));
    let content=name.and_then(|(language,route)|{
        if !matches!(language,"zh-CN"|"en") || route.len()>2500 || !route.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'/'|b'-'|b'_'|b'.')) {return None;}
        let link=format!("shipforge://{route}");
        if !valid_invitation(&link) && shipforge_core::delivery_link_parts(&link).is_err(){return None;}
        let (title,action)=match (language,route.starts_with("invite/")){
            ("en",true)=>("Organization invitation","View invitation in ShipForge"),("en",false)=>("Workspace activity","Open in ShipForge"),
            (_,true)=>("组织邀请","在 ShipForge 中查看邀请"),(_,false)=>("工作区动态","在 ShipForge 中查看"),
        };
        Some(format!(r#"<!doctype html><html lang="{language}"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title} · ShipForge</title><style>:root{{font:16px system-ui;color-scheme:light dark}}main{{margin:14vh auto;max-width:32rem;padding:1rem}}a{{display:inline-block;padding:.8rem 1.2rem;color:white;background:#514674;border-radius:6px}}</style><main><h1>{title}</h1><a href="{link}">{action}</a></main></html>"#))
    });
    let (code, html) = if request.method() != &Method::Get {
        (405, String::new())
    } else {
        content
            .map(|html| (200, html))
            .unwrap_or((404, String::new()))
    };
    let response=Response::from_string(html).with_status_code(code)
        .with_header(Header::from_bytes("Content-Type","text/html; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Cache-Control","no-store").unwrap())
        .with_header(Header::from_bytes("Referrer-Policy","no-referrer").unwrap())
        .with_header(Header::from_bytes("Content-Security-Policy","default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'").unwrap())
        .with_header(Header::from_bytes("X-Content-Type-Options","nosniff").unwrap());
    let _ = request.respond(response);
}
