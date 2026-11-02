//! Destination allowlist: IDNA-normalised hostnames with dot-guarded
//! subdomain matching, plus the public-IP policy for resolved addresses.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use thiserror::Error;

pub const MAX_DOMAIN_LENGTH: usize = 253;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("domain is empty")]
    Empty,
    #[error("domain contains whitespace")]
    Whitespace,
    #[error("'{0}' is an IP address literal, not a domain")]
    IpLiteral(String),
    #[error("'{0}' is not a valid hostname")]
    Invalid(String),
    #[error("domain exceeds {1} characters: '{0}'")]
    TooLong(String, usize),
}

/// Normalises a user- or protocol-supplied hostname: trims surrounding
/// whitespace and a trailing dot, applies IDNA (UTS #46) and validates label
/// structure. Returns the punycode ASCII form.
pub fn normalize_domain(input: &str) -> Result<String, DomainError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Empty);
    }
    if trimmed.chars().any(|c| c.is_whitespace()) {
        return Err(DomainError::Whitespace);
    }
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Err(DomainError::Empty);
    }
    if trimmed.parse::<IpAddr>().is_ok() {
        return Err(DomainError::IpLiteral(trimmed.to_string()));
    }
    if trimmed.len() > MAX_DOMAIN_LENGTH {
        return Err(DomainError::TooLong(trimmed.to_string(), MAX_DOMAIN_LENGTH));
    }
    let ascii = idna::domain_to_ascii(trimmed)
        .map_err(|_| DomainError::Invalid(input.to_string()))?;
    if !is_strict_hostname(&ascii) {
        return Err(DomainError::Invalid(input.to_string()));
    }
    if ascii.len() > MAX_DOMAIN_LENGTH {
        return Err(DomainError::TooLong(ascii, MAX_DOMAIN_LENGTH));
    }
    Ok(ascii)
}

/// Strict hostname validation on the ASCII (IDNA-normalised) form: 1–253
/// chars, dot-separated labels of 1–63 chars, each limited to
/// `[a-z0-9-]` with no leading/trailing hyphen. IDNA punycode labels
/// (`xn--...`) fit these rules by construction.
fn is_strict_hostname(host: &str) -> bool {
    if host.is_empty() || host.len() > MAX_DOMAIN_LENGTH || !host.is_ascii() {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

#[derive(Debug, Clone, Default)]
pub struct DomainList {
    entries: BTreeSet<String>,
}

impl DomainList {
    /// Validates and normalises every entry before constructing the list.
    pub fn new<I, S>(domains: I) -> Result<Self, DomainError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut entries = BTreeSet::new();
        for d in domains {
            entries.insert(normalize_domain(d.as_ref())?);
        }
        Ok(Self { entries })
    }

    /// Unchecked construction from domains that were already normalised.
    pub fn from_normalized<I, S>(domains: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            entries: domains.into_iter().map(Into::into).collect(),
        }
    }

    /// Sorted, normalised entries.
    pub fn sorted(&self) -> Vec<String> {
        self.entries.iter().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Exact-or-subdomain match. The host is normalised first; a host that
    /// fails normalisation is never allowed.
    pub fn allows(&self, host: &str) -> bool {
        match normalize_domain(host) {
            Ok(h) => self.allows_normalized(&h),
            Err(_) => false,
        }
    }

    /// Dot-guarded suffix match on an already-normalised host:
    /// `sub.google.com` matches `google.com`; `evilgoogle.com` and
    /// `google.com.evil.net` do not.
    pub fn allows_normalized(&self, host: &str) -> bool {
        if self.entries.contains(host) {
            return true;
        }
        let mut suffix = host;
        while let Some(dot) = suffix.find('.') {
            suffix = &suffix[dot + 1..];
            if self.entries.contains(suffix) {
                return true;
            }
        }
        false
    }
}

/// Effective destination-hostname policy. By default every structurally
/// valid hostname is allowed (matching the original `allow *` 3proxy setup);
/// enabling `restrict` limits access to the allowlist.
#[derive(Debug, Clone)]
pub struct DomainPolicy {
    /// When `true`, only allowlisted hostnames pass. When `false` (default),
    /// every valid hostname is allowed.
    pub restrict: bool,
    pub list: DomainList,
}

impl DomainPolicy {
    pub fn new(restrict: bool, list: DomainList) -> Self {
        Self { restrict, list }
    }

    pub fn unrestricted() -> Self {
        Self {
            restrict: false,
            list: DomainList::default(),
        }
    }

    pub fn restricted(list: DomainList) -> Self {
        Self {
            restrict: true,
            list,
        }
    }

    /// True if the requested host passes the policy. In unrestricted mode a
    /// hostname still has to normalise successfully; garbage and
    /// IP-literal-shaped hosts never pass.
    pub fn allows(&self, host: &str) -> bool {
        if !self.restrict {
            normalize_domain(host).is_ok()
        } else {
            self.list.allows(host)
        }
    }
}

/// True if `ip` is a globally routable public address. Loopback, private,
/// link-local, CGNAT, multicast, unique-local and reserved ranges are all
/// rejected.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_documentation()
    {
        return false;
    }
    if o[0] == 0 {
        return false; // 0.0.0.0/8 "this network"
    }
    if o[0] == 100 && (o[1] & 0xc0) == 64 {
        return false; // 100.64.0.0/10 CGNAT
    }
    if o[0] == 192 && o[1] == 0 && o[2] == 0 {
        return false; // 192.0.0.0/24
    }
    if o[0] == 198 && (o[1] == 18 || o[1] == 19) {
        return false; // 198.18.0.0/15 benchmarking
    }
    if o[0] >= 240 {
        return false; // 240.0.0.0/4 reserved (incl. 255.255.255.255)
    }
    true
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    let s = ip.segments();
    // IPv4-mapped addresses are judged as the embedded IPv4 address.
    if s[..5] == [0, 0, 0, 0, 0] && s[5] == 0xffff {
        let v4 = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
        return is_public_v4(v4);
    }
    if (s[0] & 0xfe00) == 0xfc00 {
        return false; // fc00::/7 unique local
    }
    if (s[0] & 0xffc0) == 0xfec0 {
        return false; // fec0::/10 site-local (deprecated)
    }
    if (s[0] & 0xffc0) == 0xfe80 {
        return false; // fe80::/10 link-local
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_case_and_idna() {
        assert_eq!(normalize_domain("  Example.COM ").unwrap(), "example.com");
        assert_eq!(normalize_domain("Bücher.de").unwrap(), "xn--bcher-kva.de");
        assert_eq!(normalize_domain("example.com.").unwrap(), "example.com");
    }

    #[test]
    fn rejects_bad_domains() {
        assert_eq!(normalize_domain(""), Err(DomainError::Empty));
        assert_eq!(normalize_domain("."), Err(DomainError::Empty));
        assert!(matches!(normalize_domain("a b.com"), Err(DomainError::Whitespace)));
        assert!(matches!(normalize_domain("192.168.1.1"), Err(DomainError::IpLiteral(_))));
        assert!(matches!(normalize_domain("a..b"), Err(DomainError::Invalid(_))));
        assert!(matches!(normalize_domain("bad!name.example"), Err(DomainError::Invalid(_))));
        let long = "a".repeat(300);
        assert!(matches!(normalize_domain(&long), Err(DomainError::TooLong(_, _))));
    }

    #[test]
    fn dot_guarded_matching() {
        let list = DomainList::new(["google.com", "example.com"]).unwrap();
        assert!(list.allows("google.com"));
        assert!(list.allows("mail.google.com"));
        assert!(list.allows("a.b.example.com"));
        assert!(!list.allows("evilgoogle.com"));
        assert!(!list.allows("google.com.evil.net"));
        assert!(!list.allows("notgoogle.com"));
        assert!(!list.allows(""));
    }

    #[test]
    fn domain_policy_modes() {
        let list = DomainList::new(["google.com"]).unwrap();
        let unrestricted = DomainPolicy::unrestricted();
        assert!(unrestricted.allows("anything.example"));
        assert!(!unrestricted.allows("not a host"));

        let restricted = DomainPolicy::restricted(list);
        assert!(restricted.allows("google.com"));
        assert!(!restricted.allows("other.example"));
        // Toggling the same list changes the outcome.
        let lenient = DomainPolicy::new(false, DomainList::new(["google.com"]).unwrap());
        assert!(lenient.allows("other.example"));
    }

    #[test]
    fn public_ip_policy() {
        assert!(is_public_ip("93.184.216.34".parse().unwrap()));
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.1.2.3".parse().unwrap()));
        assert!(!is_public_ip("192.168.1.1".parse().unwrap()));
        assert!(!is_public_ip("169.254.1.1".parse().unwrap()));
        assert!(!is_public_ip("100.64.0.1".parse().unwrap()));
        assert!(!is_public_ip("224.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("::1".parse().unwrap()));
        assert!(!is_public_ip("fe80::1".parse().unwrap()));
        assert!(!is_public_ip("fd00::1".parse().unwrap()));
        assert!(!is_public_ip("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_public_ip("2606:2800:220:1::1".parse().unwrap()));
    }
}
