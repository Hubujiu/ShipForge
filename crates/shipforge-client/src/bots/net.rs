use super::*;
use std::time::Duration;
use url::{Host, Url};

pub(super) fn fixture() -> R<Option<Url>> {
    fixture_from("SHIPFORGE_BOT_LOOPBACK")
}
pub(super) fn fixture_from(name: &str) -> R<Option<Url>> {
    let Ok(s) = std::env::var(name) else {
        return Ok(None);
    };
    let u = Url::parse(&s).map_err(|_| "invalid: adapter fixture origin")?;
    let local = matches!(u.host(),Some(Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(u.host(),Some(Host::Ipv6(ip)) if ip.is_loopback());
    if u.scheme() != "http"
        || !local
        || u.path() != "/"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("invalid: adapter fixture requires an HTTP loopback IP origin".into());
    }
    Ok(Some(u))
}
pub(super) fn root(official: &str) -> R<String> {
    Ok(fixture()?.map_or_else(
        || official.to_owned(),
        |u| u.as_str().trim_end_matches('/').to_owned(),
    ))
}
pub(super) fn trusted(input: &str, scheme: &str, domains: &[&str]) -> R<Url> {
    trusted_with(input, scheme, domains, fixture()?)
}
pub(super) fn trusted_with(
    input: &str,
    scheme: &str,
    domains: &[&str],
    fixture: Option<Url>,
) -> R<Url> {
    let u = Url::parse(input).map_err(|_| "provider: invalid service address")?;
    let local = fixture.is_some_and(|f| {
        u.host() == f.host()
            && u.port_or_known_default() == f.port_or_known_default()
            && u.scheme() == if scheme == "wss" { "ws" } else { "http" }
    });
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || (!local
            && (u.scheme() != scheme
                || u.port().is_some()
                || !domains
                    .iter()
                    .any(|d| host == *d || host.ends_with(&format!(".{d}")))))
    {
        return Err("provider: unexpected service address".into());
    }
    Ok(u)
}
pub(super) fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(39)))
        .build()
        .new_agent()
}
pub(super) fn request(url: &str, body: Option<Value>, headers: &[(&str, String)]) -> R<Value> {
    let mut res = if let Some(body) = body {
        let mut b = http().post(url);
        for (k, v) in headers {
            b = b.header(*k, v);
        }
        b.send_json(&body)
    } else {
        let mut b = http().get(url);
        for (k, v) in headers {
            b = b.header(*k, v);
        }
        b.call()
    }
    .map_err(|_| "network: provider connection or response interrupted")?;
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("provider: HTTP {status}"));
    }
    res.body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_json()
        .map_err(|_| "network: provider response could not be confirmed".into())
}
pub(super) fn endpoint(base: &str, path: &str) -> String {
    format!("{}{path}", base.trim_end_matches('/'))
}
