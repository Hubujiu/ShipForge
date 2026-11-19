use crate::{Result, invalid};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

#[derive(Clone, Copy)]
struct Network {
    network: IpAddr,
    prefix: u8,
}
pub(super) fn canonical(ip: IpAddr) -> IpAddr {
    ip.to_canonical()
}
impl Network {
    fn parse(raw: &str) -> Result<(Self, bool)> {
        let (addr, cidr) = raw
            .trim()
            .split_once('/')
            .map_or((raw.trim(), None), |(a, b)| (a, Some(b)));
        let address = IpAddr::from_str(addr)
            .map_err(|_| invalid(&format!("invalid IP allowlist entry: {raw}")))?;
        let original_bits = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match cidr {
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|n| *n > 0 && *n <= original_bits)
                .ok_or_else(|| invalid(&format!("invalid or unrestricted CIDR: {raw}")))?,
            None => original_bits,
        };
        let ip = canonical(address);
        let prefix = if address.is_ipv6() && ip.is_ipv4() {
            prefix
                .checked_sub(96)
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid(&format!("invalid mapped IPv4 prefix: {raw}")))?
        } else {
            prefix
        };
        let network = match ip {
            IpAddr::V4(v) => IpAddr::V4(Ipv4Addr::from(u32::from(v) & (u32::MAX << (32 - prefix)))),
            IpAddr::V6(v) => IpAddr::V6(Ipv6Addr::from(
                u128::from(v) & (u128::MAX << (128 - prefix)),
            )),
        };
        Ok((Self { network, prefix }, cidr.is_some()))
    }
    fn contains(self, ip: IpAddr) -> bool {
        match (self.network, canonical(ip)) {
            (IpAddr::V4(net), IpAddr::V4(v)) => {
                (u32::from(v) >> (32 - self.prefix)) == (u32::from(net) >> (32 - self.prefix))
            }
            (IpAddr::V6(net), IpAddr::V6(v)) => {
                (u128::from(v) >> (128 - self.prefix)) == (u128::from(net) >> (128 - self.prefix))
            }
            _ => false,
        }
    }
}
pub(super) fn list(values: &serde_json::Value) -> Result<Vec<String>> {
    let values = values
        .as_array()
        .ok_or_else(|| invalid("allowlist must be an array of IP addresses or CIDRs"))?;
    if values.len() > 20 {
        return Err(invalid("at most 20 allowlist entries are supported"));
    }
    let mut out = Vec::new();
    for (index, v) in values.iter().enumerate() {
        let raw = v
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() < 125)
            .ok_or_else(|| invalid(&format!("invalid IP allowlist entry {}", index + 1)))?;
        let (net, _) = Network::parse(raw)?;
        let full = if net.network.is_ipv4() { 32 } else { 128 };
        let entry = if net.prefix == full {
            net.network.to_string()
        } else {
            format!("{}/{}", net.network, net.prefix)
        };
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    Ok(out)
}
pub(super) fn permits(entries: &[String], ip: IpAddr) -> bool {
    entries
        .iter()
        .any(|e| Network::parse(e).is_ok_and(|(net, _)| net.contains(ip)))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn mixed_families_are_canonical_and_restricted() {
        let list = list(&json!([
            "203.0.113.9/24",
            "203.0.113.44/24",
            "2001:db8:1::7/64",
            "::ffff:10.0.0.8/120",
            "127.0.0.1/32"
        ]))
        .unwrap();
        assert_eq!(
            list,
            [
                "203.0.113.0/24",
                "2001:db8:1::/64",
                "10.0.0.0/24",
                "127.0.0.1"
            ]
        );
        for value in [
            "203.0.113.8",
            "::ffff:203.0.113.8",
            "2001:db8:1::88",
            "10.0.0.23",
        ] {
            assert!(permits(&list, value.parse().unwrap()));
        }
        for value in ["203.0.112.8", "2001:db8:2::8", "10.0.1.23"] {
            assert!(!permits(&list, value.parse().unwrap()));
        }
        for value in [
            "0.0.0.0/0",
            "::/0",
            "::ffff:0.0.0.0/96",
            "a.b.c.d",
            "127.0.0.1/33",
        ] {
            assert!(Network::parse(value).is_err());
        }
    }
}
