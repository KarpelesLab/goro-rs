//! Minimal RFC 3986 URI parsing (validity checks as done by libxml2's
//! xmlParseURISafe) and reference resolution (xmlBuildURI).

#[derive(Clone, Debug, Default)]
pub struct Uri {
    pub scheme: Option<String>,
    pub authority: Option<String>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

fn is_unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~')
}

fn is_sub_delim(c: u8) -> bool {
    matches!(
        c,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

fn is_pchar_byte(c: u8) -> bool {
    is_unreserved(c) || is_sub_delim(c) || c == b':' || c == b'@' || c >= 0x80
}

/// Check that `s` consists of allowed characters (with pct-encoding).
fn check_chars(s: &[u8], extra: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'%' {
            if i + 2 >= s.len() {
                return false;
            }
            if !(s.get(i + 1).map(|b| b.is_ascii_hexdigit()).unwrap_or(false)
                && s.get(i + 2).map(|b| b.is_ascii_hexdigit()).unwrap_or(false))
            {
                return false;
            }
            i += 3;
            continue;
        }
        if !(is_pchar_byte(c) || extra.contains(&c)) {
            return false;
        }
        i += 1;
    }
    true
}

/// Parse a URI reference. Returns None when invalid.
pub fn parse_uri(s: &str) -> Option<Uri> {
    let b = s.as_bytes();
    let mut u = Uri::default();
    let mut rest = b;
    // scheme
    if let Some(colon) = b.iter().position(|&c| c == b':') {
        let sch = &b[..colon];
        if !sch.is_empty()
            && sch[0].is_ascii_alphabetic()
            && sch
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        {
            u.scheme = Some(String::from_utf8_lossy(sch).into_owned());
            rest = &b[colon + 1..];
        }
    }
    // fragment
    let (before_frag, frag) = match rest.iter().position(|&c| c == b'#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    if let Some(f) = frag {
        if !check_chars(f, b"/?") {
            return None;
        }
        u.fragment = Some(String::from_utf8_lossy(f).into_owned());
    }
    let (before_q, q) = match before_frag.iter().position(|&c| c == b'?') {
        Some(i) => (&before_frag[..i], Some(&before_frag[i + 1..])),
        None => (before_frag, None),
    };
    if let Some(q) = q {
        if !check_chars(q, b"/?") {
            return None;
        }
        u.query = Some(String::from_utf8_lossy(q).into_owned());
    }
    let mut path = before_q;
    if path.starts_with(b"//") {
        let after = &path[2..];
        let end = after.iter().position(|&c| c == b'/').unwrap_or(after.len());
        let auth = &after[..end];
        if !check_chars(auth, b"[]") {
            return None;
        }
        u.authority = Some(String::from_utf8_lossy(auth).into_owned());
        path = &after[end..];
    }
    if !check_chars(path, b"/") {
        return None;
    }
    u.path = String::from_utf8_lossy(path).into_owned();
    Some(u)
}

fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let abs = path.starts_with('/');
    let segs: Vec<&str> = path.split('/').collect();
    let n = segs.len();
    for (i, seg) in segs.iter().enumerate() {
        if i == 0 && abs {
            continue;
        }
        match *seg {
            "." => {
                if i == n - 1 {
                    out.push("");
                }
            }
            ".." => {
                if !out.is_empty() {
                    out.pop();
                }
                if i == n - 1 {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let j = out.join("/");
    if abs { format!("/{}", j) } else { j }
}

/// xmlBuildURI(uri, base): resolve `reference` against `base`.
pub fn build_uri(reference: &str, base: Option<&str>) -> Option<String> {
    let base = match base {
        None => return Some(reference.to_string()),
        Some(b) => b,
    };
    if reference.is_empty() {
        return Some(base.to_string());
    }
    let r = parse_uri(reference)?;
    if r.scheme.is_some() {
        return Some(reference.to_string());
    }
    let b = match parse_uri(base) {
        Some(b) => b,
        None => return Some(reference.to_string()),
    };
    let mut out = String::new();
    if let Some(s) = &b.scheme {
        out.push_str(s);
        out.push(':');
    }
    if let Some(a) = &r.authority {
        out.push_str("//");
        out.push_str(a);
        out.push_str(&remove_dot_segments(&r.path));
    } else {
        if let Some(a) = &b.authority {
            out.push_str("//");
            out.push_str(a);
        }
        if r.path.is_empty() {
            out.push_str(&b.path);
            if r.query.is_none() {
                if let Some(q) = &b.query {
                    out.push('?');
                    out.push_str(q);
                }
            }
        } else if r.path.starts_with('/') {
            out.push_str(&remove_dot_segments(&r.path));
        } else {
            let merged = if b.authority.is_some() && b.path.is_empty() {
                format!("/{}", r.path)
            } else {
                match b.path.rfind('/') {
                    Some(i) => format!("{}{}", &b.path[..=i], r.path),
                    None => r.path.clone(),
                }
            };
            out.push_str(&remove_dot_segments(&merged));
        }
    }
    if let Some(q) = &r.query {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = &r.fragment {
        out.push('#');
        out.push_str(f);
    }
    Some(out)
}
