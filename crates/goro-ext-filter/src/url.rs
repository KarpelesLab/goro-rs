//! FILTER_VALIDATE_URL.
//!
//! PHP manual (Validate filters: "Validates value as URL (according to RFC
//! 2396) ... will only find ASCII URLs to be valid") and observed on PHP 8.5
//! (the fixture's URL section with every byte at every position; tests
//! 015.phpt, 062.phpt, bug51192.phpt, bug64441.phpt, bug77423.phpt,
//! bug81122.phpt, ghsa-w8qr-v226-r27w.phpt):
//!
//! 1. every byte is printable ASCII (0x21-0x7E);
//! 2. parse_url() accepts it and finds a scheme;
//! 3. a URL without host is only valid for the mailto, news and file
//!    schemes;
//! 4. for http and https the host is a hostname (FILTER_VALIDATE_DOMAIN
//!    with FILTER_FLAG_HOSTNAME) or a bracketed IPv6 address;
//! 5. the user and password are unreserved characters, sub-delims, ':'
//!    and valid %XX escapes (RFC 3986 userinfo), for every scheme;
//! 6. FILTER_FLAG_PATH_REQUIRED and FILTER_FLAG_QUERY_REQUIRED need the
//!    component to be present (an empty query after "?" counts).
//!
//! With the `uri_parser_class` option the ext/uri parser replaces steps 2,
//! 4 and 5 (observed on PHP 8.5: `http://a_b.com/` passes with both
//! parsers, `http://h:1a` fails with them).

use goro_ext_standard::url::parse_url;

use crate::consts::*;
use crate::{FilterHost, UriParser, UriParts, ip, validate};

fn userinfo_ok(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:".contains(&c) {
            i += 1;
        } else if c == b'%'
            && i + 2 < s.len()
            && s[i + 1].is_ascii_hexdigit()
            && s[i + 2].is_ascii_hexdigit()
        {
            i += 3;
        } else {
            return false;
        }
    }
    true
}

fn no_host_scheme(scheme: &[u8]) -> bool {
    scheme.eq_ignore_ascii_case(b"mailto")
        || scheme.eq_ignore_ascii_case(b"news")
        || scheme.eq_ignore_ascii_case(b"file")
}

fn required_ok(flags: i64, has_path: bool, has_query: bool) -> bool {
    (flags & FILTER_FLAG_PATH_REQUIRED == 0 || has_path)
        && (flags & FILTER_FLAG_QUERY_REQUIRED == 0 || has_query)
}

pub(crate) fn validate<H: FilterHost>(
    host: &mut H,
    s: &[u8],
    flags: i64,
    parser: Option<UriParser>,
) -> bool {
    if !s.iter().all(|&c| (0x21..=0x7E).contains(&c)) {
        return false;
    }
    if let Some(p) = parser
        && let Some(parsed) = host.parse_uri(p, s)
    {
        let Some(UriParts {
            scheme,
            host: h,
            has_path,
            has_query,
        }) = parsed
        else {
            return false;
        };
        let Some(scheme) = scheme else {
            return false;
        };
        if h.is_none() && !no_host_scheme(&scheme) {
            return false;
        }
        return required_ok(flags, has_path, has_query);
    }
    let Some(u) = parse_url(s) else {
        return false;
    };
    let Some(scheme) = &u.scheme else {
        return false;
    };
    match &u.host {
        None => {
            if !no_host_scheme(scheme) {
                return false;
            }
        }
        Some(h) => {
            if scheme.eq_ignore_ascii_case(b"http") || scheme.eq_ignore_ascii_case(b"https") {
                let ok = match h.strip_prefix(b"[") {
                    Some(inner) => inner
                        .strip_suffix(b"]")
                        .is_some_and(|v6| v6.contains(&b':') && ip::parse_v6(v6, 7, 5).is_some()),
                    None => validate::valid_domain(h, true),
                };
                if !ok {
                    return false;
                }
            }
        }
    }
    if u.user.as_deref().is_some_and(|x| !userinfo_ok(x))
        || u.pass.as_deref().is_some_and(|x| !userinfo_ok(x))
    {
        return false;
    }
    required_ok(flags, u.path.is_some(), u.query.is_some())
}
