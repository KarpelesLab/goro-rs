//! A one-pass parser for the common shape of URL: a special scheme other
//! than `file`, `//`, an optional `user[:password]@`, an ASCII domain that
//! does not end in a number, an optional port, and a path, query and
//! fragment of URL code points without `%`, `\` or dot segments. For these
//! the URL Standard's state machine (in [`super::parser`]) has a single
//! outcome, which this builds directly; anything else returns `None` and
//! goes to the full parser. The crate's tests run both on the same inputs.

use super::errors::{ErrorType, ValidationError};
use super::host::{HostKind, ends_in_number};
use super::parser::{FRAG, PATH, SETS, SQUERY, Scheme, USERINFO, is_urlcp as urlcp, push_encoded};
use super::{Parsed, Span, Url};
use std::cell::RefCell;
use std::rc::Rc;

fn span(start: usize, end: usize) -> Span {
    Span {
        start: start as u32,
        end: end as u32,
    }
}

pub(super) fn parse(input: &[u8]) -> Option<Parsed> {
    if input.len() > u32::MAX as usize / 2 {
        return None;
    }
    // Scheme.
    let colon = input
        .iter()
        .position(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.')))?;
    if colon == 0 || input[colon] != b':' || !input[0].is_ascii_alphabetic() || colon > 5 {
        return None;
    }
    let mut lower = [0u8; 5];
    for (d, s) in lower.iter_mut().zip(&input[..colon]) {
        *d = s.to_ascii_lowercase();
    }
    let kind = Scheme::of(&lower[..colon]);
    if !kind.special() || kind == Scheme::File {
        return None;
    }
    let rest = &input[colon + 1..];
    if !rest.starts_with(b"//") {
        return None;
    }
    let auth_start = colon + 3;
    // Authority: up to '/', '?', '#' or the end; every other byte must be a
    // URL code point.
    let mut auth_end = auth_start;
    let mut at: Option<usize> = None;
    while auth_end < input.len() {
        let c = input[auth_end];
        if matches!(c, b'/' | b'?' | b'#') {
            break;
        }
        if !urlcp(c) {
            return None;
        }
        if c == b'@' {
            if at.is_some() {
                return None;
            }
            at = Some(auth_end);
        }
        auth_end += 1;
    }
    let host_start = at.map_or(auth_start, |a| a + 1);
    let (host_end, port) = match input[host_start..auth_end].iter().position(|&c| c == b':') {
        Some(i) => (host_start + i, Some(&input[host_start + i + 1..auth_end])),
        None => (auth_end, None),
    };
    let host = &input[host_start..host_end];
    if host.is_empty() {
        return None;
    }
    // Port.
    let port = match port {
        None | Some(b"") => None,
        Some(digits) => {
            let mut v: u32 = 0;
            for &d in digits {
                if !d.is_ascii_digit() {
                    return None;
                }
                v = v * 10 + (d - b'0') as u32;
                if v > 65535 {
                    return None;
                }
            }
            let v = v as u16;
            (Some(v) != kind.default_port()).then_some(v)
        }
    };
    // Path, query, fragment.
    let path_end = input[auth_end..]
        .iter()
        .position(|&c| c == b'?' || c == b'#')
        .map_or(input.len(), |i| auth_end + i);
    let path = &input[auth_end..path_end];
    let mut seg_start = 0;
    for (i, &c) in path.iter().enumerate() {
        if c == b'/' {
            if matches!(&path[seg_start..i], b"." | b"..") {
                return None;
            }
            seg_start = i + 1;
        } else if !urlcp(c) || SETS[c as usize] & PATH != 0 {
            return None;
        }
    }
    if matches!(&path[seg_start..], b"." | b"..") {
        return None;
    }
    let (query, frag) = match input.get(path_end) {
        Some(b'?') => {
            let q_end = input[path_end..]
                .iter()
                .position(|&c| c == b'#')
                .map_or(input.len(), |i| path_end + i);
            (Some(&input[path_end + 1..q_end]), input.get(q_end + 1..))
        }
        Some(_) => (None, Some(&input[path_end + 1..])),
        None => (None, None),
    };
    if !query.unwrap_or(b"").iter().all(|&c| urlcp(c))
        || !frag.unwrap_or(b"").iter().all(|&c| urlcp(c))
    {
        return None;
    }
    // Userinfo: no byte that needs encoding but the first ':'.
    let (user, pass) = match at {
        None => (&b""[..], &b""[..]),
        Some(a) => {
            let ui = &input[auth_start..a];
            let (u, p) = match ui.iter().position(|&c| c == b':') {
                Some(i) => (&ui[..i], &ui[i + 1..]),
                None => (ui, &b""[..]),
            };
            if u.iter().chain(p).any(|&c| SETS[c as usize] & USERINFO != 0) {
                return None;
            }
            (u, p)
        }
    };

    let mut href = Vec::with_capacity(input.len() + 4);
    for &c in &input[..colon] {
        href.push(c.to_ascii_lowercase());
    }
    let scheme = span(0, colon);
    href.extend_from_slice(b"://");
    let mut username = span(href.len(), href.len());
    let mut password = username;
    if !user.is_empty() || !pass.is_empty() {
        let s = href.len();
        href.extend_from_slice(user);
        username = span(s, href.len());
        if !pass.is_empty() {
            href.push(b':');
            let s = href.len();
            href.extend_from_slice(pass);
            password = span(s, href.len());
        } else {
            password = span(href.len(), href.len());
        }
        href.push(b'@');
    }
    let hs = href.len();
    for &c in host {
        href.push(c.to_ascii_lowercase());
    }
    let host_span = span(hs, href.len());
    // An "xn--" label is decoded and checked by the full parser.
    let h = &href[hs..];
    if h.split(|&c| c == b'.').any(|l| l.starts_with(b"xn--")) || ends_in_number(h) {
        return None;
    }
    if let Some(p) = port {
        href.push(b':');
        let mut digits = [0u8; 5];
        let mut i = digits.len();
        let mut v = p;
        loop {
            i -= 1;
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        href.extend_from_slice(&digits[i..]);
    }
    let ps = href.len();
    if path.is_empty() {
        href.push(b'/');
    } else {
        href.extend_from_slice(path);
    }
    let path_span = span(ps, href.len());
    let query = query.map(|q| {
        href.push(b'?');
        let s = href.len();
        push_encoded(&mut href, q, SQUERY);
        span(s, href.len())
    });
    let fragment = frag.map(|f| {
        href.push(b'#');
        let s = href.len();
        push_encoded(&mut href, f, FRAG);
        span(s, href.len())
    });
    let errors: Vec<ValidationError> = match at {
        // Every '@' of the authority is reported (URL Standard,
        // "invalid-credentials"); the context runs to the end.
        Some(a) => vec![ValidationError {
            kind: ErrorType::InvalidCredentials,
            context: input[a..].to_vec(),
        }],
        None => Vec::new(),
    };
    let url = Url {
        href: href.into_boxed_slice(),
        scheme,
        username,
        password,
        host: Some(host_span),
        host_kind: HostKind::Domain,
        port,
        path: path_span,
        query,
        fragment,
        opaque: false,
        kind,
        errors: Rc::new(RefCell::new(errors.clone())),
    };
    Some((url, errors))
}
