//! Device-wide transport policy. Reading or changing it never opens a project database.
mod adapters;
mod settings;
#[cfg(windows)]
mod windows;
pub(crate) use adapters::{ProfileScope, provider};

use crate::credentials;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use url::{Host, Url};

type Result<T> = std::result::Result<T, String>;
pub const DEVICE_OPS: &[&str] = &[
    "network.settings",
    "network.save",
    "network.test",
    "device.about",
    "device.update",
];
const RELEASES: &str = "https://github.com/Hubujiu/ShipForge/releases";
const RELEASE_API: &str = "https://api.github.com/repos/Hubujiu/ShipForge/releases/latest";

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    System,
    Direct,
    Manual,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Policy {
    rev: u64,
    mode: Mode,
    endpoint: String,
    username: String,
    credential: Option<credentials::Connection>,
}
impl Policy {
    fn public(&self) -> Value {
        json!({"rev":self.rev,"mode":self.mode,"endpoint":self.endpoint,"username":self.username,"has_password":self.credential.is_some()})
    }
    fn route(&self, target: &Url) -> Result<Option<ureq::Proxy>> {
        if loopback(target) || self.mode == Mode::Direct {
            return Ok(None);
        }
        if self.mode == Mode::System {
            // The process environment remains authoritative for CLI deployments.
            if let Some(proxy) = ureq::Proxy::try_from_env() {
                return Ok(Some(proxy));
            }
            #[cfg(windows)]
            {
                return windows::proxy(target);
            }
            #[cfg(not(windows))]
            {
                return Ok(None);
            }
        }
        let mut url = settings::endpoint(&self.endpoint)?;
        if url.scheme() == "socks5" {
            // Keep private team hostnames on the proxy's DNS, including on split-DNS networks.
            url.set_scheme("socks5h")
                .map_err(|_| "invalid: SOCKS5 proxy address")?;
        }
        url.set_username(&self.username)
            .map_err(|_| "invalid: proxy username")?;
        if let Some(credential) = &self.credential {
            url.set_password(Some(&credential.secret()?))
                .map_err(|_| "invalid: proxy password")?;
        }
        Ok(Some(
            ureq::Proxy::new(url.as_str()).map_err(|_| "invalid: proxy address")?,
        ))
    }
}

fn loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(addr)) => addr.is_loopback(),
        Some(Host::Ipv6(addr)) => {
            addr.is_loopback() || addr.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
        Some(Host::Domain(name)) => name.trim_end_matches('.').eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn transport(policy: &Policy, target: &Url, seconds: u64) -> Result<ureq::Agent> {
    Ok(routed(policy.route(target)?, seconds))
}
fn routed(proxy: Option<ureq::Proxy>, seconds: u64) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .proxy(proxy)
        .tls_config(tls)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(seconds)))
        .build()
        .new_agent()
}
pub(crate) fn http(directory: Option<&Path>, address: &str, seconds: u64) -> Result<ureq::Agent> {
    let target = Url::parse(address).map_err(|_| "invalid: network destination")?;
    let policy = match directory {
        Some(dir) => settings::load(dir)?,
        None => Policy::default(),
    };
    transport(&policy, &target, seconds)
}
pub(super) struct TeamTransport {
    directory: std::path::PathBuf,
    target: Url,
    cached: Option<(u64, Instant, ureq::Agent)>,
}
impl TeamTransport {
    pub(super) fn new(directory: &Path, address: &str) -> Result<Self> {
        Ok(Self {
            directory: directory.to_owned(),
            target: Url::parse(address).map_err(|_| "invalid: network destination")?,
            cached: None,
        })
    }
    pub(super) fn agent(&mut self) -> Result<&ureq::Agent> {
        let policy = settings::load(&self.directory)?;
        let refresh = self.cached.as_ref().is_none_or(|(rev, since, _)| {
            *rev != policy.rev
                || (policy.mode == Mode::System && since.elapsed() > Duration::from_secs(120))
        });
        if refresh {
            self.cached = Some((
                policy.rev,
                Instant::now(),
                transport(&policy, &self.target, 12)?,
            ));
        }
        Ok(&self.cached.as_ref().unwrap().2)
    }
}
pub(crate) fn failure(error: ureq::Error) -> String {
    let reason = match error {
        ureq::Error::ConnectProxyFailed(ref s) if s.contains("407") => {
            "proxy authentication required"
        }
        ureq::Error::ConnectProxyFailed(_) => "proxy rejected the connection",
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => {
            "TLS certificate or handshake failed"
        }
        ureq::Error::Timeout(_) => "connection timed out",
        ureq::Error::HostNotFound => "host could not be resolved",
        _ => "network connection failed",
    };
    format!("offline: {reason}")
}
fn test_address(q: &Value) -> Result<Url> {
    let url = Url::parse(q["test_url"].as_str().unwrap_or("https://api.github.com/"))
        .map_err(|_| "invalid: test address")?;
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback(&url)))
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid: test address requires HTTPS or loopback HTTP without credentials or query parameters".into());
    }
    Ok(url)
}
fn probe(policy: &Policy, target: Url) -> Result<Value> {
    let started = Instant::now();
    let response = transport(policy, &target, 10)?
        .head(target.as_str())
        .header("User-Agent", "ShipForge")
        .call()
        .map_err(failure)?;
    let status = response.status().as_u16();
    if status == 407 {
        return Err("offline: proxy authentication required".into());
    }
    Ok(
        json!({"reachable":true,"status":status,"elapsed_ms":started.elapsed().as_millis(),"loopback":loopback(&target)}),
    )
}
fn release_info(info: Value) -> Result<Value> {
    if info["draft"] != false || info["prerelease"] != false {
        return Err("server: release is not stable".into());
    }
    let raw = info["tag_name"]
        .as_str()
        .and_then(|s| s.strip_prefix('v'))
        .ok_or("server: invalid release tag")?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| "invalid: application version")?;
    let latest = semver::Version::parse(raw).map_err(|_| "server: invalid release tag")?;
    if !latest.pre.is_empty() || !latest.build.is_empty() {
        return Err("server: invalid stable release tag".into());
    }
    let filename = format!("shipforge-{latest}-windows-x64-setup.exe");
    let ready = info["assets"].as_array().is_some_and(|items| {
        items.iter().any(|a| a["name"] == filename)
            && items
                .iter()
                .any(|a| a["name"] == format!("{filename}.sha256"))
    });
    Ok(
        json!({"installed":current.to_string(),"latest":latest.to_string(),"available":latest>current && ready,
        "assets_ready":ready,"release_url":format!("{RELEASES}/tag/v{latest}")}),
    )
}
pub fn device_command(directory: &Path, q: &Value) -> Result<Value> {
    match q["op"].as_str() {
        Some("network.settings") => Ok(settings::load(directory)?.public()),
        Some("network.save") => settings::save(directory, q),
        Some("network.test") => {
            let saved = settings::load(directory)?;
            let policy = if q.get("mode").is_some() {
                settings::edited(&saved, q)?
            } else {
                saved
            };
            probe(&policy, test_address(q)?)
        }
        Some("device.about") => {
            let guide = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.join("docs/windows.md")))
                .filter(|p| p.is_file())
                .and_then(|p| Url::from_file_path(p).ok())
                .map(|u| u.to_string())
                .unwrap_or_else(|| {
                    "https://github.com/Hubujiu/ShipForge/blob/main/docs/windows.md".into()
                });
            Ok(
                json!({"name":"ShipForge","version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"license":"MIT","releases":RELEASES,"guide":guide}),
            )
        }
        Some("device.update") => {
            let mut reply = http(Some(directory), RELEASE_API, 12)?
                .get(RELEASE_API)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "ShipForge")
                .call()
                .map_err(failure)?;
            if reply.status() != 200 {
                return Err(format!(
                    "server: release service returned HTTP {}",
                    reply.status().as_u16()
                ));
            }
            release_info(
                reply
                    .body_mut()
                    .with_config()
                    .limit(512 * 1024)
                    .read_json()
                    .map_err(|_| "server: invalid release data")?,
            )
        }
        _ => Err("invalid: unknown device operation".into()),
    }
}
pub(crate) fn schema(schema: &mut Value) {
    schema["properties"]["op"]["enum"]
        .as_array_mut()
        .unwrap()
        .extend(DEVICE_OPS.iter().map(|s| json!(s)));
    let rules = schema["allOf"].as_array_mut().unwrap();
    rules.push(json!({"if":{"properties":{"op":{"const":"network.save"}}},"then":{"required":["mode","expected_rev"]}}));
    rules.push(json!({"if":{"properties":{"op":{"enum":["network.save","network.test"]}}},"then":{"properties":{"mode":{"enum":["system","direct","manual"]},"endpoint":{"type":"string"},"username":{"type":"string"},"password":{"type":"string","writeOnly":true},"clear_password":{"type":"boolean"},"test_url":{"type":"string","format":"uri"}}}}));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stable(version: &str, names: &[&str]) -> Value {
        json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":false,
            "assets":names.iter().map(|name|json!({"name":name})).collect::<Vec<_>>()})
    }

    #[test]
    fn updater_needs_both_stable_windows_assets_and_compares_versions() {
        let setup = "shipforge-99.0.0-windows-x64-setup.exe";
        let checksum = format!("{setup}.sha256");
        let complete = release_info(stable("99.0.0", &[setup, &checksum])).unwrap();
        assert_eq!(complete["available"], true);
        assert_eq!(complete["release_url"], format!("{RELEASES}/tag/v99.0.0"));
        for incomplete in [vec![setup], vec![checksum.as_str()]] {
            assert_eq!(
                release_info(stable("99.0.0", &incomplete)).unwrap()["assets_ready"],
                false
            );
        }
        let installed = env!("CARGO_PKG_VERSION");
        let name = format!("shipforge-{installed}-windows-x64-setup.exe");
        let current = release_info(stable(installed, &[&name, &format!("{name}.sha256")])).unwrap();
        assert_eq!(
            (
                current["assets_ready"].as_bool(),
                current["available"].as_bool()
            ),
            (Some(true), Some(false))
        );
        for flag in ["draft", "prerelease"] {
            let mut data = stable("99.0.0", &[setup, &checksum]);
            data[flag] = json!(true);
            assert!(release_info(data).is_err());
        }
        assert!(release_info(stable("99.0.0-beta.1", &[])).is_err());
    }
}
