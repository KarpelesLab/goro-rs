//! DN parsing and formatting (getdn.c): ldap_str2dn, ldap_explode_dn,
//! ldap_dn2ufn. Input is parsed in LDAP_DN_FORMAT_LDAP; output is produced
//! for LDAPV3 (explode with types) or UFN (explode without types, dn2ufn).

/// One attribute-value assertion.
#[derive(Debug, Clone)]
pub struct Ava {
    pub attr: Vec<u8>,
    pub value: Vec<u8>,
    /// '#'-encoded binary value (output re-emits as '#'<hex>).
    pub binary: bool,
    /// value held a non-printable byte (affects IA5/pretty paths; unused here)
    pub nonprintable: bool,
}

/// An RDN is one or more AVAs joined with '+'.
pub type Rdn = Vec<Ava>;
/// A DN is a list of RDNs.
pub type Dn = Vec<Rdn>;

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}
fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}
fn is_ldh(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
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
// NEEDESCAPE = ESCAPE('\\') | NE ; NE = RDN_SEP_V2(','|';') | AVA_SEP('+') | QUOTES('"') | '<' | '>'
fn needescape(c: u8) -> bool {
    matches!(c, b'\\' | b',' | b';' | b'+' | b'"' | b'<' | b'>')
}
// MAYESCAPE = ESCAPE | NE | '=' | space | '#'
fn mayescape(c: u8) -> bool {
    needescape(c) || c == b'=' || is_space(c) || c == b'#'
}

/// ldap_str2dn (LDAP_DN_FORMAT_LDAP). Returns None on a decoding error
/// (as libldap returns LDAP_DECODING_ERROR).
pub fn str2dn(s: &[u8]) -> Option<Dn> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    if s.contains(&0) {
        return None;
    }
    let mut dn: Dn = Vec::new();
    let mut p = 0usize;
    loop {
        let (rdn, next) = parse_rdn(s, p)?;
        dn.push(rdn);
        p = next;
        if p >= s.len() {
            break;
        }
        // expect RDN separator ',' or ';'
        match s[p] {
            b',' | b';' => p += 1,
            _ => return None,
        }
    }
    Some(dn)
}

/// Parse one RDN beginning at `start`; returns (rdn, index at separator/end).
fn parse_rdn(s: &[u8], start: usize) -> Option<(Rdn, usize)> {
    let mut p = start;
    let mut avas: Rdn = Vec::new();
    loop {
        // B4AVA: trim leading spaces
        while p < s.len() && is_space(s[p]) {
            p += 1;
        }
        if p >= s.len() {
            return None;
        }
        // attribute type
        let attr: Vec<u8> = if s[p].is_ascii_digit() {
            // numeric OID: digits and dots
            let st = p;
            while p < s.len() && (s[p].is_ascii_digit() || s[p] == b'.') {
                p += 1;
            }
            s[st..p].to_vec()
        } else if is_alpha(s[p]) {
            let st = p;
            p += 1;
            while p < s.len() && is_ldh(s[p]) {
                p += 1;
            }
            let end = p;
            // trim language tags ';...'
            if p < s.len() && s[p] == b';' {
                while p < s.len() && (is_ldh(s[p]) || s[p] == b';') {
                    p += 1;
                }
            }
            s[st..end].to_vec()
        } else {
            return None;
        };
        if attr.is_empty() {
            return None;
        }
        // B4AVAEQUALS
        while p < s.len() && is_space(s[p]) {
            p += 1;
        }
        if p >= s.len() || s[p] != b'=' {
            return None;
        }
        p += 1;
        while p < s.len() && is_space(s[p]) {
            p += 1;
        }
        // value
        let (value, binary, nonprintable, next) = if p < s.len() && s[p] == b'#' {
            let (v, n) = parse_binary(s, p + 1)?;
            (v, true, false, n)
        } else {
            let (v, np, n) = parse_strval(s, p)?;
            (v, false, np, n)
        };
        p = next;
        avas.push(Ava {
            attr,
            value,
            binary,
            nonprintable,
        });
        // GOTAVA: '+' → another AVA; else RDN done
        if p < s.len() && s[p] == b'+' {
            p += 1;
            continue;
        }
        return Some((avas, p));
    }
}

/// str2strval for LDAP format: stop at ',' ';' '+'; handle escapes.
fn parse_strval(s: &[u8], start: usize) -> Option<(Vec<u8>, bool, usize)> {
    let mut out: Vec<u8> = Vec::new();
    let mut p = start;
    let mut nonprintable = false;
    while p < s.len() {
        let c = s[p];
        if c == b'\\' {
            p += 1;
            if p >= s.len() {
                return None;
            }
            let e = s[p];
            if mayescape(e) {
                out.push(e);
                p += 1;
                continue;
            }
            if p + 1 < s.len() && is_hex(s[p]) && is_hex(s[p + 1]) {
                let b = (unhex(s[p]) << 4) | unhex(s[p + 1]);
                if !(b' '..=b'~').contains(&b) {
                    nonprintable = true;
                }
                out.push(b);
                p += 2;
                continue;
            }
            // illegal escape
            return None;
        }
        // VALUE_END_V2 for LDAP format: ',' ';' '+'
        if c == b',' || c == b';' || c == b'+' {
            break;
        }
        if !(b' '..=b'~').contains(&c) {
            nonprintable = true;
            out.push(c);
            p += 1;
            continue;
        }
        if needescape(c) {
            // unescaped special char: decoding error
            return None;
        }
        out.push(c);
        p += 1;
    }
    // strip trailing unescaped spaces
    // (count how many trailing bytes came from unescaped spaces)
    // Re-scan source to decide: simplest faithful approach—strip trailing
    // spaces that were not escaped. Since escaped spaces are pushed from a
    // '\' path, they are indistinguishable here; but a trailing run of
    // spaces at the raw tail is trimmed only when unescaped.
    // libldap strips only when p[-1] is space and p[-2] not escape.
    if p > start + 1 && is_space(s[p - 1]) && s[p - 2] != b'\\' {
        while out.last() == Some(&b' ') {
            out.pop();
        }
    }
    Some((out, nonprintable, p))
}

/// Binary '#'<hexpairs> value.
fn parse_binary(s: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    let mut p = start;
    let mut out = Vec::new();
    while p + 1 < s.len() && is_hex(s[p]) && is_hex(s[p + 1]) {
        out.push((unhex(s[p]) << 4) | unhex(s[p + 1]));
        p += 2;
    }
    if out.is_empty() {
        return None;
    }
    Some((out, p))
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// strval2str for LDAPV3: escape NEEDESCAPE | '=' | leading space/# |
/// trailing space | NUL | non-ASCII as hex.
fn strval2str(val: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(val.len());
    let n = val.len();
    for (i, &c) in val.iter().enumerate() {
        if c == 0 {
            out.extend_from_slice(b"\\00");
            continue;
        }
        let lead = i == 0;
        let trail = i == n - 1;
        // WILLESCAPE_CHAR = RDN_SEP(',') | AVA_SEP('+') | ESCAPE('\\')
        let willescape_hex = matches!(c, b',' | b'+' | b'\\');
        let needs = needescape(c)
            || c == b'='
            || (lead && mayescape(c))
            || (trail && (is_space(c) || needescape(c)));
        if c >= 0x80 {
            // non-ASCII byte: hex escape
            out.push(b'\\');
            out.push(HEX[(c >> 4) as usize]);
            out.push(HEX[(c & 0xf) as usize]);
        } else if needs {
            out.push(b'\\');
            if willescape_hex {
                out.push(HEX[(c >> 4) as usize]);
                out.push(HEX[(c & 0xf) as usize]);
            } else {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn binval2hex(val: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(val.len() * 2);
    for &b in val {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0xf) as usize]);
    }
    out
}

/// rdn2str (LDAPV3): "attr=value[+attr=value]..."
fn rdn2str(rdn: &Rdn) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, ava) in rdn.iter().enumerate() {
        if i > 0 {
            out.push(b'+');
        }
        out.extend_from_slice(&ava.attr);
        out.push(b'=');
        if ava.binary {
            out.push(b'#');
            out.extend_from_slice(&binval2hex(&ava.value));
        } else {
            out.extend_from_slice(&strval2str(&ava.value));
        }
    }
    out
}

/// rdn2UFNstr: values only, "value[ + value]...".
fn rdn2ufn(rdn: &Rdn) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, ava) in rdn.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b" + ");
        }
        if ava.binary {
            out.push(b'#');
            out.extend_from_slice(&binval2hex(&ava.value));
        } else {
            out.extend_from_slice(&strval2str(&ava.value));
        }
    }
    out
}

/// LDAP_DN_IS_RDN_DC: a single-AVA "dc" RDN with a string value.
fn is_rdn_dc(rdn: &Rdn) -> bool {
    rdn.len() == 1 && !rdn[0].binary && rdn[0].attr.eq_ignore_ascii_case(b"dc")
}

/// ldap_explode_dn: each RDN as a string, with or without attribute types.
pub fn explode_dn(dn: &[u8], notypes: bool) -> Option<Vec<Vec<u8>>> {
    let parsed = str2dn(dn)?;
    let mut out = Vec::new();
    for rdn in &parsed {
        out.push(if notypes { rdn2ufn(rdn) } else { rdn2str(rdn) });
    }
    Some(out)
}

/// ldap_dn2ufn (LDAP → UFN with DC_IN_UFN).
pub fn dn2ufn(dn: &[u8]) -> Option<Vec<u8>> {
    let parsed = str2dn(dn)?;
    if parsed.is_empty() {
        return Some(Vec::new());
    }
    // leftmost index of the final contiguous run of DC rdns
    let mut leftmost_dc: i64 = -1;
    for (i, rdn) in parsed.iter().enumerate() {
        if is_rdn_dc(rdn) {
            if leftmost_dc == -1 {
                leftmost_dc = i as i64;
            }
        } else {
            leftmost_dc = -1;
        }
    }
    let mut out = Vec::new();
    if leftmost_dc == -1 {
        for (i, rdn) in parsed.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(b", ");
            }
            out.extend_from_slice(&rdn2ufn(rdn));
        }
    } else {
        let lm = leftmost_dc as usize;
        for (i, rdn) in parsed.iter().enumerate().take(lm) {
            if i > 0 {
                out.extend_from_slice(b", ");
            }
            out.extend_from_slice(&rdn2ufn(rdn));
        }
        if lm > 0 {
            out.extend_from_slice(b", ");
        }
        // dn2domain: join the trailing dc values with '.'
        for (k, rdn) in parsed[lm..].iter().enumerate() {
            if k > 0 {
                out.push(b'.');
            }
            out.extend_from_slice(&rdn[0].value);
        }
    }
    Some(out)
}

/// ldap_dn2str in LDAPV3: canonical DN string.
pub fn dn2str_v3(dn: &Dn) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, rdn) in dn.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(&rdn2str(rdn));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(s: &str, nt: bool) -> Option<Vec<String>> {
        explode_dn(s.as_bytes(), nt).map(|v| {
            v.into_iter()
                .map(|x| String::from_utf8_lossy(&x).into_owned())
                .collect()
        })
    }
    fn ufn(s: &str) -> Option<String> {
        dn2ufn(s.as_bytes()).map(|v| String::from_utf8_lossy(&v).into_owned())
    }

    #[test]
    fn explode() {
        assert_eq!(
            ex("cn=bob,dc=example,dc=com", false).unwrap(),
            vec!["cn=bob", "dc=example", "dc=com"]
        );
        assert_eq!(
            ex("cn=bob,ou=users,dc=example,dc=com", true).unwrap(),
            vec!["bob", "users", "example", "com"]
        );
        assert!(ex("cn=<bob>,dc=example,dc=com", false).is_none());
        assert!(ex("bob,dc=example,dc=com", false).is_none());
    }

    #[test]
    fn ufn_tests() {
        assert_eq!(ufn("cn=bob,dc=example,dc=com").unwrap(), "bob, example.com");
        assert_eq!(
            ufn("cn=bob,ou=users,dc=example,dc=com").unwrap(),
            "bob, users, example.com"
        );
        assert!(ufn("cn=<bob>,dc=example,dc=com").is_none());
        assert!(ufn("bob,dc=example,dc=com").is_none());
    }
}
