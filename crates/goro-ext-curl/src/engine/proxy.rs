//! Proxy selection (lib/url.c detect_proxy/parse_proxy, lib/noproxy.c)

use std::net::{Ipv4Addr, Ipv6Addr};

/// getenv() that treats empty and missing alike like curl_getenv
fn getenv(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// detect_proxy(): (env var name used, value)
pub(crate) fn detect_proxy(scheme: &str) -> Option<(String, String)> {
    let mut env = format!("{}_proxy", scheme.to_ascii_lowercase());
    let mut proxy = getenv(&env);
    if proxy.is_none() && !env.eq_ignore_ascii_case("http_proxy") {
        env = env.to_ascii_uppercase();
        proxy = getenv(&env);
    }
    if proxy.is_none() {
        if env.eq_ignore_ascii_case("ws_proxy") {
            proxy = getenv("http_proxy");
        } else if env.eq_ignore_ascii_case("wss_proxy") {
            proxy = getenv("https_proxy").or_else(|| getenv("HTTPS_PROXY"));
        }
        if proxy.is_none() {
            env = "all_proxy".into();
            proxy = getenv(&env);
            if proxy.is_none() {
                env = "ALL_PROXY".into();
                proxy = getenv(&env);
            }
        }
    }
    proxy.map(|p| (env, p))
}

/// The no_proxy environment variable: (name, value)
pub(crate) fn env_noproxy() -> Option<(String, String)> {
    if let Some(v) = getenv("no_proxy") {
        return Some(("no_proxy".into(), v));
    }
    getenv("NO_PROXY").map(|v| ("NO_PROXY".into(), v))
}

fn cidr4_match(ipv4: &str, network: &str, bits: u32) -> bool {
    let (Ok(a), Ok(n)) = (ipv4.parse::<Ipv4Addr>(), network.parse::<Ipv4Addr>()) else {
        return false;
    };
    if bits > 32 {
        return false;
    }
    let bits = if bits == 0 { 32 } else { bits };
    let a = u32::from(a);
    let n = u32::from(n);
    let mask = if bits == 32 { !0 } else { !(!0u32 >> bits) };
    (a & mask) == (n & mask)
}

fn cidr6_match(ipv6: &str, network: &str, bits: u32) -> bool {
    let bits = if bits == 0 { 128 } else { bits };
    if bits > 128 {
        return false;
    }
    let (Ok(a), Ok(n)) = (ipv6.parse::<Ipv6Addr>(), network.parse::<Ipv6Addr>()) else {
        return false;
    };
    let a = u128::from(a);
    let n = u128::from(n);
    let mask = if bits == 128 { !0 } else { !(!0u128 >> bits) };
    (a & mask) == (n & mask)
}

/// Curl_check_noproxy(): true when `name` must not use a proxy
pub(crate) fn check_noproxy(name: &str, no_proxy: Option<&str>) -> bool {
    if name.is_empty() {
        return false;
    }
    let Some(np) = no_proxy else { return false };
    if np.is_empty() {
        return false;
    }
    if np == "*" {
        return true;
    }
    #[derive(PartialEq)]
    enum T {
        Host,
        V4,
        V6,
    }
    let (name, ty): (String, T) = if let Some(rest) = name.strip_prefix('[') {
        match rest.find(']') {
            Some(e) => (rest[..e].to_string(), T::V6),
            None => return false,
        }
    } else if name.parse::<Ipv4Addr>().is_ok() {
        (name.to_string(), T::V4)
    } else if name.contains(':') && name.parse::<Ipv6Addr>().is_ok() {
        (name.to_string(), T::V6)
    } else {
        (name.strip_suffix('.').unwrap_or(name).to_string(), T::Host)
    };
    let nb = name.as_bytes();
    let p = np.as_bytes();
    let mut i = 0;
    while i < p.len() {
        while i < p.len() && (p[i] == b' ' || p[i] == b'\t') {
            i += 1;
        }
        let start = i;
        while i < p.len() && p[i] != b' ' && p[i] != b'\t' && p[i] != b',' {
            i += 1;
        }
        let mut token = &p[start..i];
        if !token.is_empty() {
            let m = match ty {
                T::Host => {
                    if token.last() == Some(&b'.') {
                        token = &token[..token.len() - 1];
                    }
                    if token.first() == Some(&b'.') {
                        token = &token[1..];
                    }
                    if token.len() == nb.len() {
                        token.eq_ignore_ascii_case(nb)
                    } else if token.len() < nb.len() {
                        nb[nb.len() - token.len() - 1] == b'.'
                            && nb[nb.len() - token.len()..].eq_ignore_ascii_case(token)
                    } else {
                        false
                    }
                }
                T::V4 | T::V6 => {
                    let t = String::from_utf8_lossy(token).into_owned();
                    let (check, bits) = match t.find('/') {
                        Some(s) => (t[..s].to_string(), t[s + 1..].parse::<u32>().unwrap_or(0)),
                        None => (t.clone(), 0),
                    };
                    if ty == T::V6 {
                        cidr6_match(&name, &check, bits)
                    } else {
                        cidr4_match(&name, &check, bits)
                    }
                }
            };
            if m {
                return true;
            }
        }
        while i < p.len() && (p[i] == b' ' || p[i] == b'\t') {
            i += 1;
        }
        if i >= p.len() || p[i] != b',' {
            break;
        }
        while i < p.len() && p[i] == b',' {
            i += 1;
        }
    }
    false
}

/// A parsed proxy (struct proxy_info)
#[derive(Clone, Debug)]
pub(crate) struct ProxyInfo {
    pub host: String,
    pub port: u16,
    pub proxytype: i64,
    pub user: Option<Vec<u8>>,
    pub passwd: Option<Vec<u8>>,
    pub ipv6: bool,
}
