//! ldap_url_parse / ldap_is_ldap_url (url.c)

use crate::consts::*;

#[derive(Debug, Clone, Default)]
pub struct LdapUrl {
    pub scheme: String,
    pub host: Option<String>,
    pub port: i32,
    pub dn: Option<String>,
    pub attrs: Vec<String>,
    pub scope: i32,
    pub filter: Option<String>,
    pub exts: Vec<String>,
    pub crit_exts: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlErr {
    Mem,
    Param,
    BadScheme,
    BadEnclosure,
    BadUrl,
    BadHost,
    BadAttrs,
    BadScope,
    BadFilter,
    BadExts,
}

// parse flags (LDAP_PVT_URL_PARSE_*)
pub const PARSE_NOEMPTY_HOST: u32 = 0x01;
pub const PARSE_DEF_PORT: u32 = 0x02;
pub const PARSE_NOEMPTY_DN: u32 = 0x04;
pub const PARSE_NODEF_SCOPE: u32 = 0x08;
pub const PARSE_HISTORIC: u32 = PARSE_NODEF_SCOPE | PARSE_NOEMPTY_HOST | PARSE_DEF_PORT;

/// skip_url_prefix: returns (scheme, remainder, enclosed).
fn skip_url_prefix(url: &str) -> Option<(&'static str, &str, bool)> {
    let mut p = url;
    let mut enclosed = false;
    if let Some(r) = p.strip_prefix('<') {
        enclosed = true;
        p = r;
    }
    // "URL:" prefix (case-insensitive)
    if p.len() >= 4 && p[..4].eq_ignore_ascii_case("URL:") {
        p = &p[4..];
    }
    for (prefix, scheme) in [
        ("ldap://", "ldap"),
        ("pldap://", "pldap"),
        ("ldaps://", "ldaps"),
        ("pldaps://", "pldaps"),
        ("ldapi://", "ldapi"),
        ("cldap://", "cldap"),
    ] {
        if p.len() >= prefix.len() && p[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return Some((scheme, &p[prefix.len()..], enclosed));
        }
    }
    None
}

/// ldap_is_ldap_url()
pub fn is_ldap_url(url: &str) -> bool {
    skip_url_prefix(url).is_some()
}

/// ldap_pvt_hex_unescape(): URL %-decoding in place semantics.
pub fn hex_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 < b.len() && is_hex(b[i + 1]) && is_hex(b[i + 2]) {
                out.push((unhex(b[i + 1]) << 4) | unhex(b[i + 2]));
                i += 3;
                continue;
            } else {
                // invalid: per libldap, restore original string unchanged
                return s.to_string();
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}
fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'A'..=b'F' => c - b'A' + 10,
        _ => c - b'a' + 10,
    }
}

fn str2scope(p: &str) -> i32 {
    match p.to_ascii_lowercase().as_str() {
        "one" | "onelevel" => LDAP_SCOPE_ONELEVEL,
        "base" => LDAP_SCOPE_BASE,
        "sub" | "subtree" => LDAP_SCOPE_SUBTREE,
        "subord" | "subordinate" | "children" => LDAP_SCOPE_SUBORDINATE,
        _ => -1,
    }
}

/// ldap_url_parse_ext()
pub fn parse_ext(url_in: &str, flags: u32) -> Result<LdapUrl, UrlErr> {
    let (scheme, rest, enclosed) = skip_url_prefix(url_in).ok_or(UrlErr::BadScheme)?;
    let proto_ipc = scheme == "ldapi";

    let mut url = rest.to_string();
    if enclosed {
        if url.is_empty() || !url.ends_with('>') {
            return Err(UrlErr::BadEnclosure);
        }
        url.pop();
    }

    let mut lud = LdapUrl {
        scheme: scheme.to_string(),
        scope: if flags & PARSE_NODEF_SCOPE != 0 {
            LDAP_SCOPE_BASE
        } else {
            LDAP_SCOPE_DEFAULT
        },
        ..LdapUrl::default()
    };

    // split hostport / dn at first '/'
    // p: start of dn (after '/'); q used for the Novell kludge.
    let bytes = url.clone();
    let (hostport, mut p, mut novell_q): (String, Option<String>, Option<String>) =
        match bytes.find('/') {
            Some(i) => (bytes[..i].to_string(), Some(bytes[i + 1..].to_string()), None),
            None => match bytes.find('?') {
                Some(i) => (bytes[..i].to_string(), None, Some(bytes[i + 1..].to_string())),
                None => (bytes.clone(), None, None),
            },
        };

    let mut host_src = hostport;
    let mut is_v6 = false;
    if !proto_ipc {
        if let Some(stripped) = host_src.strip_prefix('[') {
            // IPv6 [addr]:port
            let r = stripped.find(']').ok_or(UrlErr::BadUrl)?;
            let addr = stripped[..r].to_string();
            let after = &stripped[r + 1..];
            // after must be "" or ":port"
            let portpart = if let Some(pp) = after.strip_prefix(':') {
                Some(pp.to_string())
            } else if after.is_empty() {
                None
            } else {
                return Err(UrlErr::BadUrl);
            };
            host_src = addr;
            is_v6 = true;
            if let Some(pp) = portpart {
                set_port(&mut lud, &pp)?;
            }
        } else if let Some(i) = host_src.find(':') {
            let pp = hex_unescape(&host_src[i + 1..]);
            let h = host_src[..i].to_string();
            host_src = h;
            if pp.is_empty() {
                return Err(UrlErr::BadUrl);
            }
            // Novell kludge: a port can be followed by the dn when no '/'
            // was seen. strtol stops at first non-digit.
            let digits: String = pp.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
            let port: i64 = digits.parse().map_err(|_| UrlErr::BadUrl)?;
            if digits.len() != pp.len() {
                // trailing chars; for Novell kludge only when no '/' part
                if p.is_none() {
                    novell_q = Some(pp[digits.len()..].to_string());
                } else {
                    return Err(UrlErr::BadUrl);
                }
            }
            lud.port = port as i32;
        }
        let _ = is_v6;
        if flags & PARSE_DEF_PORT != 0 && lud.port == 0 {
            lud.port = if lud.scheme == "ldaps" {
                LDAPS_PORT
            } else {
                LDAP_PORT
            };
        }
    }

    let host = hex_unescape(&host_src);
    if flags & PARSE_NOEMPTY_HOST != 0 && host.is_empty() {
        lud.host = None;
    } else {
        lud.host = Some(host);
    }

    // Novell kludge: p is None, novell_q starts with '?'
    if p.is_none() {
        if let Some(q) = &novell_q {
            if let Some(rest) = q.strip_prefix('?') {
                if !rest.is_empty() {
                    lud.dn = Some(hex_unescape(rest));
                } else if flags & PARSE_NOEMPTY_DN == 0 {
                    lud.dn = Some(String::new());
                }
            }
        }
        return Ok(lud);
    }

    // parse dn ? attrs ? scope ? filter ? exts
    let dn_part = p.take().unwrap();
    let mut it = dn_part.splitn(2, '?');
    let dnp = it.next().unwrap().to_string();
    let mut tail = it.next();

    if !dnp.is_empty() {
        lud.dn = Some(hex_unescape(&dnp));
    } else if flags & PARSE_NOEMPTY_DN == 0 {
        lud.dn = Some(String::new());
    }
    if tail.is_none() {
        return Ok(lud);
    }

    // attrs
    let t = tail.take().unwrap().to_string();
    let mut it = t.splitn(2, '?');
    let attrp = it.next().unwrap();
    tail = it.next();
    if !attrp.is_empty() {
        lud.attrs = hex_unescape(attrp)
            .split(',')
            .map(|s| s.to_string())
            .collect();
    }
    if tail.is_none() {
        return Ok(lud);
    }

    // scope
    let t = tail.take().unwrap().to_string();
    let mut it = t.splitn(2, '?');
    let scopep = it.next().unwrap();
    tail = it.next();
    if !scopep.is_empty() {
        let s = str2scope(&hex_unescape(scopep));
        if s == -1 {
            return Err(UrlErr::BadScope);
        }
        lud.scope = s;
    }
    if tail.is_none() {
        return Ok(lud);
    }

    // filter
    let t = tail.take().unwrap().to_string();
    let mut it = t.splitn(2, '?');
    let filterp = it.next().unwrap();
    tail = it.next();
    if !filterp.is_empty() {
        let f = hex_unescape(filterp);
        if f.is_empty() {
            return Err(UrlErr::BadFilter);
        }
        lud.filter = Some(f);
    }
    if tail.is_none() {
        return Ok(lud);
    }

    // exts (no further '?')
    let t = tail.take().unwrap();
    if t.contains('?') {
        return Err(UrlErr::BadUrl);
    }
    let exts: Vec<String> = t.split(',').map(|s| hex_unescape(s)).collect();
    if exts.is_empty() || (exts.len() == 1 && exts[0].is_empty()) {
        return Err(UrlErr::BadExts);
    }
    for e in &exts {
        if e.starts_with('!') {
            lud.crit_exts += 1;
        }
    }
    lud.exts = exts;
    Ok(lud)
}

fn set_port(lud: &mut LdapUrl, pp: &str) -> Result<(), UrlErr> {
    let pp = hex_unescape(pp);
    if pp.is_empty() {
        return Err(UrlErr::BadUrl);
    }
    lud.port = pp.parse::<i64>().map_err(|_| UrlErr::BadUrl)? as i32;
    Ok(())
}

pub fn parse(url: &str) -> Result<LdapUrl, UrlErr> {
    parse_ext(url, PARSE_HISTORIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        assert!(is_ldap_url("ldap://localhost"));
        assert!(is_ldap_url("ldaps://localhost:636"));
        assert!(is_ldap_url("ldapi://"));
        assert!(!is_ldap_url("http://localhost"));
        let u = parse("ldap://localhost:389").unwrap();
        assert_eq!(u.host.as_deref(), Some("localhost"));
        assert_eq!(u.port, 389);
        let u = parse("ldaps://example.com").unwrap();
        assert_eq!(u.port, 636);
        let u = parse("ldap://host/dc=example,dc=com?cn,sn?sub?(cn=*)").unwrap();
        assert_eq!(u.dn.as_deref(), Some("dc=example,dc=com"));
        assert_eq!(u.attrs, vec!["cn", "sn"]);
        assert_eq!(u.scope, LDAP_SCOPE_SUBTREE);
        assert_eq!(u.filter.as_deref(), Some("(cn=*)"));
    }
}
