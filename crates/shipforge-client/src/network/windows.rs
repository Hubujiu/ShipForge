use super::*;
use std::ptr::null;
use windows_sys::Win32::{Foundation::GlobalFree, Networking::WinHttp::*};

// WinHTTP allocates each returned string with GlobalAlloc.
fn take_text(text: *mut u16) -> Option<String> {
    if text.is_null() {
        return None;
    }
    unsafe {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        GlobalFree(text.cast());
        Some(value)
    }
}
fn matches_host(host: &str, rule: &str) -> bool {
    // WinINet bypass entries use '*' to match zero or more characters.
    let (mut pos, mut wildcard, mut backtrack, mut i) = (0, None, 0, 0);
    let pattern = rule.as_bytes();
    let value = host.as_bytes();
    while i < value.len() {
        if pattern.get(pos) == Some(&b'*') {
            wildcard = Some(pos);
            pos += 1;
            backtrack = i;
        } else if pattern.get(pos) == value.get(i) {
            pos += 1;
            i += 1;
        } else if let Some(star) = wildcard {
            backtrack += 1;
            i = backtrack;
            pos = star + 1;
        } else {
            return false;
        }
    }
    pattern[pos..].iter().all(|&b| b == b'*')
}
fn bypass(target: &Url, rules: &str) -> bool {
    let host = target.host_str().unwrap_or("").to_ascii_lowercase();
    let authority = target
        .port_or_known_default()
        .map(|port| format!("{host}:{port}"));
    rules
        .split([';', ',', ' '])
        .filter(|s| !s.is_empty())
        .any(|s| {
            let rule = s.to_ascii_lowercase();
            if rule == "<local>" {
                return !host.contains('.') && !host.contains(':');
            }
            let pattern = if let Some((scheme, pattern)) = rule.split_once("://") {
                if scheme != target.scheme() {
                    return false;
                }
                pattern
            } else {
                &rule
            };
            matches_host(&host, pattern)
                || authority
                    .as_deref()
                    .is_some_and(|value| matches_host(value, pattern))
        })
}
fn select(proxy_list: &str, bypass_list: &str, target: &Url) -> Result<Option<ureq::Proxy>> {
    if bypass(target, bypass_list) {
        return Ok(None);
    }
    let mut default = None;
    let mut matching = None;
    for entry in proxy_list.split([';', ' ', '\t']).filter(|s| !s.is_empty()) {
        if let Some((scheme, addr)) = entry.split_once('=') {
            if scheme.eq_ignore_ascii_case(target.scheme()) {
                matching = Some((addr, "http"));
                break;
            }
            if scheme.eq_ignore_ascii_case("socks") && default.is_none() {
                default = Some((addr, "socks5h"));
            }
        } else if default.is_none() {
            default = Some((entry, "http"));
        }
    }
    let Some((address, protocol)) = matching.or(default) else {
        return Ok(None);
    };
    let url = if address.contains("://") {
        address.to_string()
    } else {
        format!("{protocol}://{address}")
    };
    ureq::Proxy::new(&url)
        .map(Some)
        .map_err(|_| "offline: invalid Windows proxy configuration".into())
}
pub(super) fn proxy(target: &Url) -> Result<Option<ureq::Proxy>> {
    let mut ie = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut ie) } == 0 {
        // A service account without a pre-existing Internet Settings key has no per-user proxy.
        if std::io::Error::last_os_error().raw_os_error() == Some(2) {
            return Ok(None);
        }
        return Err("offline: Windows proxy settings are unavailable".into());
    }
    let pac = take_text(ie.lpszAutoConfigUrl);
    let fixed = take_text(ie.lpszProxy).unwrap_or_default();
    let except = take_text(ie.lpszProxyBypass).unwrap_or_default();
    if pac.is_none() && ie.fAutoDetect == 0 {
        return select(&fixed, &except, target);
    }
    let session = unsafe { WinHttpOpen(null(), WINHTTP_ACCESS_TYPE_NO_PROXY, null(), null(), 0) };
    if session.is_null() {
        return Err("offline: Windows proxy lookup failed".into());
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let config = pac.as_deref().map(wide);
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: if config.is_some() {
            WINHTTP_AUTOPROXY_CONFIG_URL
        } else {
            WINHTTP_AUTOPROXY_AUTO_DETECT
        },
        dwAutoDetectFlags: if config.is_none() {
            WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A
        } else {
            0
        },
        lpszAutoConfigUrl: config.as_ref().map_or(null(), |s| s.as_ptr()),
        ..Default::default()
    };
    let mut info = WINHTTP_PROXY_INFO::default();
    let address = wide(target.as_str());
    // Sessions and URL buffers remain valid for the synchronous lookup.
    let ok = unsafe {
        WinHttpSetTimeouts(session, 3000, 5000, 5000, 5000);
        WinHttpGetProxyForUrl(session, address.as_ptr(), &mut options, &mut info)
    };
    let error = std::io::Error::last_os_error().raw_os_error();
    unsafe {
        WinHttpCloseHandle(session);
    }
    let resolved = take_text(info.lpszProxy).unwrap_or_default();
    let resolved_bypass = take_text(info.lpszProxyBypass).unwrap_or_default();
    if ok != 0 {
        if info.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY {
            return Ok(None);
        }
        return select(&resolved, &resolved_bypass, target);
    }
    if !fixed.is_empty()
        || (pac.is_none() && error == Some(ERROR_WINHTTP_AUTODETECTION_FAILED as i32))
    {
        return select(&fixed, &except, target);
    }
    Err("offline: Windows could not resolve the automatic proxy configuration".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_protocol_and_destination_bypasses() {
        let site = Url::parse("https://team.example.test/").unwrap();
        let rules = "http=dev.test:8000;https=secure.test:8001;socks=socks.test:1080";
        assert_eq!(
            select(rules, "", &site).unwrap().unwrap().host(),
            "secure.test"
        );
        assert!(
            select(rules, "*.example.test;<local>", &site)
                .unwrap()
                .is_none()
        );
        assert!(bypass(&Url::parse("http://internal/").unwrap(), "<local>"));
        assert!(!bypass(
            &Url::parse("https://example.test.invalid/").unwrap(),
            "*.example.test"
        ));
        assert!(bypass(&site, "https://*.example.test:443"));
        assert!(!bypass(
            &site,
            "http://*.example.test:443;*.example.test:8080"
        ));
        assert!(bypass(
            &Url::parse("https://[2001:db8::1]:4567/").unwrap(),
            "[2001:db8::1]:4567"
        ));
    }
}
