//! Host matching: `NO_PROXY`-style bypass lists and client-certificate host
//! patterns.

use std::net::IpAddr;

/// A `NO_PROXY` list: `localhost,.internal,*.corp.example,10.0.0.0/8,host:8080,*`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bypass {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
enum Rule {
    All,
    /// Matches the domain itself and every subdomain.
    Domain {
        name: String,
        port: Option<u16>,
    },
    Cidr {
        net: IpAddr,
        prefix: u8,
    },
}

fn split_port(s: &str) -> (&str, Option<u16>) {
    // IPv6 literals: "[::1]:8080" or bare "::1".
    if let Some(rest) = s.strip_prefix('[')
        && let Some((host, after)) = rest.split_once(']')
    {
        return (host, after.strip_prefix(':').and_then(|p| p.parse().ok()));
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => match p.parse() {
            Ok(port) => (h, Some(port)),
            Err(_) => (s, None),
        },
        _ => (s, None),
    }
}

fn parse_cidr(s: &str) -> Option<(IpAddr, u8)> {
    let (ip, prefix) = match s.split_once('/') {
        Some((ip, p)) => (ip, p.parse::<u8>().ok()?),
        None => {
            let ip: IpAddr = s.parse().ok()?;
            return Some((ip, if ip.is_ipv4() { 32 } else { 128 }));
        }
    };
    let ip: IpAddr = ip.parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    (prefix <= max).then_some((ip, prefix))
}

fn in_cidr(ip: IpAddr, net: IpAddr, prefix: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

impl Bypass {
    /// Parse entries (each may itself be comma/space separated).
    pub fn parse<S: AsRef<str>>(entries: &[S]) -> Self {
        let mut rules = Vec::new();
        for entry in entries {
            for raw in entry
                .as_ref()
                .split([',', ' ', '\n', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if raw == "*" {
                    rules.push(Rule::All);
                } else if let Some((net, prefix)) = parse_cidr(raw) {
                    rules.push(Rule::Cidr { net, prefix });
                } else {
                    let (host, port) = split_port(raw);
                    let name = host
                        .trim_start_matches("*.")
                        .trim_start_matches('.')
                        .to_ascii_lowercase();
                    if !name.is_empty() {
                        rules.push(Rule::Domain { name, port });
                    }
                }
            }
        }
        Self { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether `host:port` bypasses the proxy.
    pub fn matches(&self, host: &str, port: Option<u16>) -> bool {
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        let ip: Option<IpAddr> = host.parse().ok();
        self.rules.iter().any(|r| match r {
            Rule::All => true,
            Rule::Cidr { net, prefix } => ip.is_some_and(|ip| in_cidr(ip, *net, *prefix)),
            Rule::Domain { name, port: p } => {
                (host == *name || host.ends_with(&format!(".{name}")))
                    && p.is_none_or(|p| Some(p) == port)
            }
        })
    }
}

/// A client-certificate host pattern: `api.example.com`, `*.example.com`
/// (subdomains only), `*` (any host), each optionally with `:port`.
#[derive(Debug, Clone, PartialEq)]
pub struct HostPattern {
    host: String,
    wildcard: bool,
    port: Option<u16>,
}

impl HostPattern {
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        // Tolerate a URL ("https://api.example.com/*", Newman's `matches`).
        let s = s.split_once("://").map_or(s, |(_, rest)| rest);
        let s = s.split('/').next().unwrap_or_default();
        if s.is_empty() {
            return Err("empty host pattern".into());
        }
        let (host, port) = split_port(s);
        if host == "*" {
            return Ok(Self {
                host: String::new(),
                wildcard: true,
                port,
            });
        }
        let (host, wildcard) = match host.strip_prefix("*.") {
            Some(h) => (h, true),
            None => (host, false),
        };
        if host.is_empty()
            || host.contains('*')
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
        {
            return Err(format!("invalid host pattern '{s}'"));
        }
        Ok(Self {
            host: host.to_ascii_lowercase(),
            wildcard,
            port,
        })
    }

    pub fn matches(&self, host: &str, port: Option<u16>) -> bool {
        if self.port.is_some_and(|p| Some(p) != port) {
            return false;
        }
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        if self.wildcard {
            self.host.is_empty() || host.ends_with(&format!(".{}", self.host))
        } else {
            host == self.host
        }
    }
}

/// `scheme://host:port` of a proxy URL, without credentials or path.
pub fn strip_credentials(url: &reqwest::Url) -> String {
    let mut u = url.clone();
    let _ = u.set_username("");
    let _ = u.set_password(None);
    u.set_path("");
    u.set_query(None);
    u.set_fragment(None);
    u.to_string().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_rules() {
        let b = Bypass::parse(&[
            "localhost, .internal",
            "*.corp.example",
            "10.0.0.0/8",
            "h:8080",
            "::1",
        ]);
        assert!(b.matches("localhost", Some(80)));
        assert!(b.matches("a.b.internal", None));
        assert!(b.matches("internal", None));
        assert!(b.matches("x.corp.example", None));
        assert!(b.matches("10.2.3.4", Some(443)));
        assert!(!b.matches("11.2.3.4", Some(443)));
        assert!(b.matches("h", Some(8080)));
        assert!(!b.matches("h", Some(80)));
        assert!(b.matches("[::1]", None));
        assert!(!b.matches("example.com", None));
        assert!(!b.matches("notinternal", None));
        assert!(Bypass::parse(&["*"]).matches("anything", None));
        assert!(Bypass::parse::<&str>(&[]).is_empty());
    }

    #[test]
    fn host_patterns() {
        let p = HostPattern::parse("*.example.com").unwrap();
        assert!(p.matches("api.example.com", Some(443)));
        assert!(!p.matches("example.com", Some(443)));
        let p = HostPattern::parse("api.example.com:8443").unwrap();
        assert!(p.matches("API.example.com", Some(8443)));
        assert!(!p.matches("api.example.com", Some(443)));
        assert!(HostPattern::parse("*").unwrap().matches("x", None));
        assert!(
            HostPattern::parse("https://api.example.com/*")
                .unwrap()
                .matches("api.example.com", Some(443))
        );
        assert!(HostPattern::parse("").is_err());
        assert!(HostPattern::parse("a*b.com").is_err());
    }

    #[test]
    fn strips_credentials() {
        let u = reqwest::Url::parse("http://user:pw@proxy:8080/x").unwrap();
        assert_eq!(strip_credentials(&u), "http://proxy:8080");
    }
}
