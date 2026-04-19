//! PHP 8.5 Uri\WhatWg\Url and Uri\Rfc3986\Uri class implementations.
//!
//! Uses the `url` crate for WHATWG URL parsing. RFC 3986 mode uses the same
//! parser; strict RFC 3986 edge cases are out of scope for the MVP.
//!
//! Parsed URLs are stored on the PhpObject as individual string properties
//! (`__uri_scheme`, `__uri_host`, `__uri_port`, `__uri_user`, `__uri_pass`,
//! `__uri_path`, `__uri_query`, `__uri_fragment`) plus a canonical
//! `__uri_serialized`. Each method reads/writes these properties.

use crate::array::PhpArray;
use crate::object::PhpObject;
use crate::string::PhpString;
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;

/// Return true if the lower-cased class name is one of the Uri built-in classes.
pub fn is_uri_class(class_lower: &[u8]) -> bool {
    matches!(
        class_lower,
        b"uri\\whatwg\\url"
            | b"uri\\rfc3986\\uri"
            | b"uri\\whatwg\\invalidurlexception"
            | b"uri\\invaliduriexception"
    )
}

/// Map lowercase class name to its canonical (PHP-style) casing.
pub fn uri_canonical_name(class_lower: &[u8]) -> Option<&'static [u8]> {
    match class_lower {
        b"uri\\whatwg\\url" => Some(b"Uri\\WhatWg\\Url"),
        b"uri\\rfc3986\\uri" => Some(b"Uri\\Rfc3986\\Uri"),
        b"uri\\whatwg\\invalidurlexception" => Some(b"Uri\\WhatWg\\InvalidUrlException"),
        b"uri\\invaliduriexception" => Some(b"Uri\\InvalidUriException"),
        _ => None,
    }
}

/// Populate an empty `PhpObject` from a parsed URL.
pub fn populate_from_url(obj: &mut PhpObject, url: &Url) {
    populate_from_url_with_source(obj, url, None)
}

/// Same as `populate_from_url` but with access to the source text so that
/// we can preserve an explicit port even when it matches the scheme default
/// (which `url` crate normalizes away). Assumes RFC 3986-style preservation.
pub fn populate_from_url_with_source(obj: &mut PhpObject, url: &Url, source: Option<&str>) {
    populate_from_url_with_source_flags(obj, url, source, true)
}

/// Same but lets the caller disable RFC-3986-specific preservation (e.g. for
/// WhatWg, which normalizes explicit default ports to None and expects
/// `url` crate's raw path).
pub fn populate_from_url_with_source_flags(
    obj: &mut PhpObject,
    url: &Url,
    source: Option<&str>,
    preserve_rfc3986: bool,
) {
    obj.set_property(
        b"__uri_scheme".to_vec(),
        Value::String(PhpString::from_string(url.scheme().to_string())),
    );
    // For RFC 3986, preserve the raw host text from the source (url crate
    // normalizes IPv6 and percent-decoding). For WhatWg, use the normalized
    // form from `url` crate.
    let host_value = if preserve_rfc3986 {
        match source {
            Some(src) if src.contains("://") => {
                extract_raw_host(src).or_else(|| url.host_str().map(|h| h.to_string()))
            }
            _ => url.host_str().map(|h| h.to_string()),
        }
    } else {
        url.host_str().map(|h| h.to_string())
    };
    // WhatWg: for "file" scheme, host is reported as an empty string (never
    // null), even for `file:///path` with no host component.
    let host_value = if !preserve_rfc3986 && url.scheme() == "file" && host_value.is_none() {
        Some(String::new())
    } else {
        host_value
    };
    obj.set_property(
        b"__uri_host".to_vec(),
        match host_value {
            Some(h) => Value::String(PhpString::from_string(h)),
            None => Value::Null,
        },
    );
    // PHP RFC 3986 preserves explicit ports (including default ports like
    // :443 for https). WhatWg strips them to None. `url` crate strips
    // defaults; for RFC 3986 we recover them from the source text.
    let explicit_port = if preserve_rfc3986 {
        source.and_then(|src| extract_explicit_port(src))
    } else {
        None
    };
    let port_value = url.port().or(explicit_port);
    obj.set_property(
        b"__uri_port".to_vec(),
        match port_value {
            Some(p) => Value::Long(p as i64),
            None => Value::Null,
        },
    );
    // WhatWg semantics: if the source contains userinfo (`...@...`), always
    // report username (and password when `:` is present) as a string, even
    // empty. Otherwise, report NULL.
    let source_has_userinfo = source
        .and_then(|s| s.find("://").map(|i| &s[i + 3..]))
        .and_then(|rest| {
            let end = rest
                .find(|c: char| matches!(c, '/' | '?' | '#'))
                .unwrap_or(rest.len());
            Some(&rest[..end])
        })
        .is_some_and(|auth| auth.contains('@'));
    // For WhatWg, if there's an `@` in the authority, both username and
    // password are always reported as strings (possibly empty), regardless
    // of whether a `:` separator was present.
    let source_has_password = source_has_userinfo;
    let user = url.username();
    obj.set_property(
        b"__uri_user".to_vec(),
        if source_has_userinfo {
            Value::String(PhpString::from_string(user.to_string()))
        } else if user.is_empty() {
            Value::Null
        } else {
            Value::String(PhpString::from_string(user.to_string()))
        },
    );
    obj.set_property(
        b"__uri_pass".to_vec(),
        match url.password() {
            Some(p) => Value::String(PhpString::from_string(p.to_string())),
            None => {
                if source_has_password {
                    Value::String(PhpString::empty())
                } else {
                    Value::Null
                }
            }
        },
    );
    // PHP's RFC 3986 parser preserves an empty path when the source had no
    // path (e.g. `https://host`). The `url` crate normalizes to `/`. WhatWg
    // keeps the normalized form.
    let path_value = {
        let normalized = url.path().to_string();
        if preserve_rfc3986 {
            if let Some(src) = source {
                if source_has_empty_path(src) {
                    String::new()
                } else {
                    normalized
                }
            } else {
                normalized
            }
        } else {
            normalized
        }
    };
    obj.set_property(
        b"__uri_path".to_vec(),
        Value::String(PhpString::from_string(path_value)),
    );
    obj.set_property(
        b"__uri_query".to_vec(),
        match url.query() {
            Some(q) => Value::String(PhpString::from_string(q.to_string())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_fragment".to_vec(),
        match url.fragment() {
            Some(f) => Value::String(PhpString::from_string(f.to_string())),
            None => Value::Null,
        },
    );
    // If the source had an explicit port that differs from what `url` crate
    // kept (or an empty path that the crate normalized to "/"), prefer the
    // source text verbatim. Only applies when RFC 3986 preservation is on.
    let serialized = match source {
        Some(src)
            if preserve_rfc3986
                && ((explicit_port.is_some() && url.port().is_none())
                    || source_has_empty_path(src)) =>
        {
            src.to_string()
        }
        _ => url.as_str().to_string(),
    };
    obj.set_property(
        b"__uri_serialized".to_vec(),
        Value::String(PhpString::from_string(serialized)),
    );
}

/// Check that `[` and `]` only appear in the host portion (as IP-literal
/// brackets), not in userinfo, path, query, or fragment. Returns false if
/// brackets appear outside the authority host position.
fn rfc3986_brackets_valid(uri: &str) -> bool {
    // Scheme:// authority / path ? query # fragment
    // Brackets are only allowed wrapping the host after "://" (and after any
    // userinfo@). Anywhere else they must be percent-encoded in RFC 3986.
    let after_scheme_opt = uri.find("://").map(|i| i + 3);
    let (pre_auth, rest) = match after_scheme_opt {
        Some(i) => (&uri[..i], &uri[i..]),
        None => ("", uri),
    };
    // Brackets in the pre-authority (scheme) portion are not valid.
    if pre_auth.contains('[') || pre_auth.contains(']') {
        return false;
    }
    // If no scheme://, brackets aren't valid anywhere (no IP-literal).
    if after_scheme_opt.is_none() {
        return !(rest.contains('[') || rest.contains(']'));
    }
    // Find end of authority (first '/', '?', '#').
    let auth_end = rest
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let post_auth = &rest[auth_end..];
    // Authority: userinfo@host[:port]. userinfo cannot contain '[' or ']'.
    let host_port = match authority.rfind('@') {
        Some(i) => {
            let userinfo = &authority[..i];
            if userinfo.contains('[') || userinfo.contains(']') {
                return false;
            }
            &authority[i + 1..]
        }
        None => authority,
    };
    // Host may be IP-literal `[...]` - in which case brackets are allowed only
    // as the wrapping delimiters at positions 0 and rb. Otherwise, no brackets.
    if host_port.starts_with('[') {
        let rb = match host_port.find(']') {
            Some(i) => i,
            None => return false,
        };
        let inside = &host_port[1..rb];
        let tail = &host_port[rb + 1..];
        if inside.contains('[') || inside.contains(']') {
            return false;
        }
        // Tail after ']' should be empty or `:port` - no brackets allowed.
        if tail.contains('[') || tail.contains(']') {
            return false;
        }
    } else if host_port.contains('[') || host_port.contains(']') {
        return false;
    }
    // Path/query/fragment: no brackets.
    if post_auth.contains('[') || post_auth.contains(']') {
        return false;
    }
    true
}

/// Extract the raw host component from the source URI (including brackets
/// for IPv6 addresses). Returns `Some("")` if the authority is present but
/// empty (e.g. `file:///...`), or None if there's no authority at all.
fn extract_raw_host(src: &str) -> Option<String> {
    let after_scheme = match src.find("://") {
        Some(i) => &src[i + 3..],
        None => return None,
    };
    let end = after_scheme
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..end];
    // Strip userinfo if present
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    // IPv6: host is [...]
    if let Some(rb) = host_port.find(']') {
        return Some(host_port[..=rb].to_string());
    }
    // Regular host: strip any port
    let host = match host_port.rfind(':') {
        Some(i) => &host_port[..i],
        None => host_port,
    };
    // Preserve empty host (e.g. `file:///...`)
    Some(host.to_string())
}

/// True if the source URI has no path component (e.g. `https://host` or
/// `https://host?query`). The `url` crate normalizes these to include a
/// trailing `/`, but RFC 3986 preserves the empty path.
fn source_has_empty_path(src: &str) -> bool {
    let after_scheme = match src.find("://") {
        Some(i) => &src[i + 3..],
        None => return false,
    };
    // Authority ends at first '/', '?', '#', or end of string
    let end = after_scheme
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(after_scheme.len());
    // If authority consumed everything, path is empty.
    if end == after_scheme.len() {
        return true;
    }
    let ch = after_scheme.as_bytes()[end];
    // `?query` or `#fragment` with no path
    ch == b'?' || ch == b'#'
}

/// Look at the raw URI text and extract an explicit `:port` component,
/// returning the parsed port number. Returns None if no port was specified
/// in the authority.
fn extract_explicit_port(src: &str) -> Option<u16> {
    // Find "//" (authority delimiter)
    let after_scheme = match src.find("://") {
        Some(i) => &src[i + 3..],
        None => return None,
    };
    // Authority ends at first '/', '?', '#', or end of string
    let end = after_scheme
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..end];
    // Strip userinfo if present
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    // For IPv6 ([::1]:443), port comes after the closing ']'
    let search_from = host_port.rfind(']').map(|i| i + 1).unwrap_or(0);
    let tail = &host_port[search_from..];
    let port_str = tail.rfind(':').map(|i| &tail[i + 1..])?;
    port_str.parse::<u16>().ok()
}

/// Parse a URI, optionally relative to a base URL, and populate `obj`.
/// Returns true on success.
///
/// `allow_relative` controls fallback behavior for RFC 3986 which permits
/// relative references (empty string, path-only, etc.). WhatWG URL is stricter
/// and rejects these. It also controls port preservation: RFC 3986 keeps an
/// explicit port even when it matches the scheme default; WhatWG strips it.
pub fn parse_into(obj: &mut PhpObject, uri: &str, base: Option<&str>, allow_relative: bool) -> bool {
    // RFC 3986 is strictly ASCII: reject any URI containing non-ASCII or
    // whitespace/control characters up front. (WhatWg is more permissive.)
    if allow_relative {
        for b in uri.bytes() {
            if b <= 0x20 || b >= 0x7f {
                return false;
            }
        }
        // Reject reserved characters ('[', ']') in userinfo and path where they
        // are only valid as IP-literal brackets surrounding the host.
        if !rfc3986_brackets_valid(uri) {
            return false;
        }
    }

    // In RFC 3986 mode, `scheme:path` (no `//` after the scheme) is NOT an
    // authority URI — it's a scheme-prefixed path. url::Url::parse may
    // normalize this incorrectly (treating the first path segment as host),
    // so we preempt that path with our own parser.
    if allow_relative && base.is_none() {
        if !uri.contains("://") {
            if let Some(parsed) = try_parse_scheme_uri(uri) {
                populate_scheme_uri(obj, &parsed, uri);
                return true;
            }
        }
        // Network-path reference: `//authority/path` with no scheme.
        if uri.starts_with("//") {
            populate_network_path(obj, uri);
            return true;
        }
    }

    let parsed = match base {
        Some(b) => match Url::parse(b) {
            Ok(base_url) => base_url.join(uri).ok(),
            Err(_) => None,
        },
        None => Url::parse(uri).ok(),
    };
    match parsed {
        Some(u) => {
            // Always pass source text so we can recover components PHP
            // expects (explicit ports for RFC 3986, empty userinfo for
            // WhatWg). For WhatWg, the source must still be consulted for
            // authority-shape information; populate_from_url_with_source
            // handles WhatWg port normalization based on `allow_relative`.
            populate_from_url_with_source_flags(obj, &u, Some(uri), allow_relative);
            true
        }
        None => {
            if allow_relative {
                if let Some(parsed) = try_parse_scheme_uri(uri) {
                    populate_scheme_uri(obj, &parsed, uri);
                    return true;
                }
                if let Some(parsed) = try_parse_empty_authority_uri(uri) {
                    populate_empty_authority(obj, &parsed, uri);
                    return true;
                }
                // Generic RFC 3986 authority parser (for hosts like IPvFuture
                // `[v7.foo]` that the `url` crate rejects).
                if let Some(parsed) = try_parse_generic_authority_uri(uri) {
                    populate_generic_authority(obj, &parsed, uri);
                    return true;
                }
                if looks_like_relative_reference(uri) {
                    populate_relative(obj, uri);
                    return true;
                }
            }
            false
        }
    }
}

/// Generic RFC 3986 authority URI parser: handles `scheme://user:pass@host:port/path?query#fragment`.
/// Used as a fallback when `url::Url::parse` rejects non-standard hosts (e.g. IPvFuture).
/// Returns (scheme, user, pass, host, port, path, query, fragment).
#[allow(clippy::type_complexity)]
fn try_parse_generic_authority_uri(
    uri: &str,
) -> Option<(String, Option<String>, Option<String>, String, Option<u16>, String, Option<String>, Option<String>)> {
    let sep = uri.find("://")?;
    let scheme = &uri[..sep];
    if scheme.is_empty() {
        return None;
    }
    if !scheme.bytes().next()?.is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return None;
    }
    let rest = &uri[sep + 3..];
    let auth_end = rest
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let path_and_rest = &rest[auth_end..];
    let (user, pass, host_port) = if let Some(at) = authority.rfind('@') {
        let userinfo = &authority[..at];
        let hp = &authority[at + 1..];
        let (u, p) = match userinfo.find(':') {
            Some(c) => (userinfo[..c].to_string(), Some(userinfo[c + 1..].to_string())),
            None => (userinfo.to_string(), None),
        };
        (Some(u), p, hp)
    } else {
        (None, None, authority)
    };
    // Host can be an IP-literal `[...]` (IPv6 or IPvFuture), reg-name, or IPv4.
    let (host, port_str) = if host_port.starts_with('[') {
        let rb = host_port.find(']')?;
        let h = &host_port[..=rb];
        let tail = &host_port[rb + 1..];
        let p = if let Some(stripped) = tail.strip_prefix(':') {
            Some(stripped)
        } else if tail.is_empty() {
            None
        } else {
            return None;
        };
        (h.to_string(), p)
    } else {
        let (h_str, p_str) = match host_port.rfind(':') {
            Some(i) => (&host_port[..i], Some(&host_port[i + 1..])),
            None => (host_port, None),
        };
        // Reject brackets/reserved chars in bare host (not IP-literal).
        for b in h_str.bytes() {
            if matches!(b, b'[' | b']' | b' ' | b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}')
            {
                return None;
            }
        }
        (h_str.to_string(), p_str)
    };
    let port_value = match port_str {
        Some(p) if !p.is_empty() => Some(p.parse::<u16>().ok()?),
        _ => None,
    };
    let (before_frag, fragment) = match path_and_rest.find('#') {
        Some(i) => (&path_and_rest[..i], Some(path_and_rest[i + 1..].to_string())),
        None => (path_and_rest, None),
    };
    let (path, query) = match before_frag.find('?') {
        Some(i) => (&before_frag[..i], Some(before_frag[i + 1..].to_string())),
        None => (before_frag, None),
    };
    Some((
        scheme.to_string(),
        user,
        pass,
        host,
        port_value,
        path.to_string(),
        query,
        fragment,
    ))
}

#[allow(clippy::type_complexity)]
fn populate_generic_authority(
    obj: &mut PhpObject,
    parsed: &(String, Option<String>, Option<String>, String, Option<u16>, String, Option<String>, Option<String>),
    original: &str,
) {
    let (scheme, user, pass, host, port, path, query, fragment) = parsed;
    obj.set_property(
        b"__uri_scheme".to_vec(),
        Value::String(PhpString::from_string(scheme.clone())),
    );
    obj.set_property(
        b"__uri_host".to_vec(),
        Value::String(PhpString::from_string(host.clone())),
    );
    obj.set_property(
        b"__uri_port".to_vec(),
        match port {
            Some(p) => Value::Long(*p as i64),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_user".to_vec(),
        match user {
            Some(u) => Value::String(PhpString::from_string(u.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_pass".to_vec(),
        match pass {
            Some(p) => Value::String(PhpString::from_string(p.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_path".to_vec(),
        Value::String(PhpString::from_string(path.clone())),
    );
    obj.set_property(
        b"__uri_query".to_vec(),
        match query {
            Some(q) => Value::String(PhpString::from_string(q.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_fragment".to_vec(),
        match fragment {
            Some(f) => Value::String(PhpString::from_string(f.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_serialized".to_vec(),
        Value::String(PhpString::from_string(original.to_string())),
    );
}

/// Classify a WhatWg URL parse failure into a short error code that matches
/// PHP's `InvalidUrlException` messages. This is a best-effort heuristic
/// against the input string.
pub fn whatwg_error_code(uri: &str) -> &'static str {
    if uri.is_empty() {
        return "MissingSchemeNonRelativeUrl";
    }
    // Null byte in scheme portion → MissingSchemeNonRelativeUrl
    if let Some(colon) = uri.find(':') {
        if uri[..colon].as_bytes().contains(&0) {
            return "MissingSchemeNonRelativeUrl";
        }
    }
    // Check scheme validity: must start with ASCII letter, rest alnum/+/-/.
    if let Some(colon) = uri.find(':') {
        let scheme = &uri[..colon];
        if scheme.is_empty() {
            return "MissingSchemeNonRelativeUrl";
        }
        let first_ok = scheme.bytes().next().is_some_and(|b| b.is_ascii_alphabetic());
        let rest_ok = scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
        if !first_ok || !rest_ok {
            return "MissingSchemeNonRelativeUrl";
        }
    }
    if let Some(sep) = uri.find("://") {
        // Null byte in authority → DomainInvalidCodePoint
        let rest_for_auth = &uri.as_bytes()[sep + 3..];
        let auth_end = rest_for_auth
            .iter()
            .position(|&b| matches!(b, b'/' | b'?' | b'#'))
            .unwrap_or(rest_for_auth.len());
        if rest_for_auth[..auth_end].contains(&0) {
            return "DomainInvalidCodePoint";
        }
    }
    if uri.as_bytes().contains(&0) {
        return "InvalidUrl";
    }
    // Special schemes (http, https, ftp, ws, wss, file) require a host.
    // For these, "scheme:" or "scheme:...without //" is HostMissing.
    if let Some(colon) = uri.find(':') {
        let scheme_lower: Vec<u8> = uri[..colon].bytes().map(|b| b.to_ascii_lowercase()).collect();
        let is_special = matches!(
            scheme_lower.as_slice(),
            b"http" | b"https" | b"ftp" | b"ws" | b"wss" | b"file"
        );
        if is_special {
            let rest = &uri[colon + 1..];
            if !rest.starts_with("//") {
                return "HostMissing";
            }
        }
    }
    if let Some(sep) = uri.find("://") {
        let rest = &uri[sep + 3..];
        let end = rest
            .find(|c: char| matches!(c, '/' | '?' | '#'))
            .unwrap_or(rest.len());
        let authority = &rest[..end];
        let (has_userinfo, host_port) = match authority.rfind('@') {
            Some(i) => (true, &authority[i + 1..]),
            None => (false, authority),
        };
        let (host, port_str) = if let Some(rb) = host_port.rfind(']') {
            let after = &host_port[rb + 1..];
            if let Some(colon) = after.find(':') {
                (&host_port[..=rb], Some(&after[colon + 1..]))
            } else {
                (&host_port[..=rb], None)
            }
        } else {
            match host_port.rfind(':') {
                Some(i) => (&host_port[..i], Some(&host_port[i + 1..])),
                None => (host_port, None),
            }
        };
        if let Some(p) = port_str {
            if p.is_empty() || p.parse::<u16>().is_err() {
                return "PortInvalid";
            }
        }
        if host.is_empty() {
            // `scheme://user:pass@` (authority with '@' but no host): PHP's
            // lexbor reports a generic (codeless) error. Signal with "".
            if has_userinfo {
                return "";
            }
            return "HostMissing";
        }
        // IP-literal host `[...]`. If it's not a valid IPv6 or the inner text
        // is invalid (e.g. `[v7.host]`), PHP emits Ipv6InvalidCodePoint.
        if host.starts_with('[') && host.ends_with(']') {
            let inside = &host[1..host.len() - 1];
            if inside.parse::<std::net::Ipv6Addr>().is_err() {
                return "Ipv6InvalidCodePoint";
            }
        }
        // Host contains invalid character (e.g. brackets without IPv6, spaces).
        // For special schemes, PHP reports DomainInvalidCodePoint.
        for ch in host.bytes() {
            if !ch.is_ascii_alphanumeric() && !matches!(ch, b'-' | b'.' | b'_' | b'~' | b'[' | b']' | b':' | b'%') {
                return "DomainInvalidCodePoint";
            }
        }
        // Specifically: brackets that aren't a wrapping IP-literal → invalid.
        if (host.contains('[') || host.contains(']')) && !(host.starts_with('[') && host.ends_with(']')) {
            return "DomainInvalidCodePoint";
        }
    } else if !uri.contains(':') {
        return "MissingSchemeNonRelativeUrl";
    }
    "InvalidUrl"
}

/// Check if an input looks like a valid RFC 3986 relative reference:
/// empty, or only contains characters valid in path/query/fragment.
/// RFC 3986 is ASCII-only; multi-byte characters must be percent-encoded.
fn looks_like_relative_reference(uri: &str) -> bool {
    if uri.is_empty() {
        return true;
    }
    // If the input contains "://" it was meant as an absolute URI; don't fall
    // back to relative-reference interpretation when that parse failed.
    if uri.contains("://") {
        return false;
    }
    // Reject anything that looks like a scheme (name followed by ':') so that
    // malformed absolute forms like `http:example` don't accidentally succeed.
    if let Some(colon_pos) = uri.find(':') {
        let scheme_candidate = &uri[..colon_pos];
        if !scheme_candidate.is_empty()
            && scheme_candidate.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
            && scheme_candidate.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        {
            return false;
        }
    }
    for b in uri.bytes() {
        // Reject whitespace, ASCII controls, DEL, and all non-ASCII bytes.
        if b <= 0x20 || b >= 0x7f {
            return false;
        }
    }
    true
}

/// Split an RFC 3986-style `scheme:path?query#fragment` with no `//`
/// authority. Returns (scheme, path, query, fragment) or None if the input
/// doesn't look like a scheme-prefixed URI.
fn try_parse_scheme_uri(uri: &str) -> Option<(String, String, Option<String>, Option<String>)> {
    let colon_pos = uri.find(':')?;
    let scheme = &uri[..colon_pos];
    // A scheme starts with a letter and continues with letter/digit/+/-/.
    if scheme.is_empty() {
        return None;
    }
    if !scheme.bytes().next()?.is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
    {
        return None;
    }
    let rest = &uri[colon_pos + 1..];
    // Authority present ("//..."): let the URL crate handle it (fail-through).
    if rest.starts_with("//") {
        return None;
    }
    // Reject control characters and non-ASCII in the remainder.
    for b in rest.bytes() {
        if b <= 0x20 || b >= 0x7f {
            return None;
        }
    }
    let (before_frag, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(rest[i + 1..].to_string())),
        None => (rest, None),
    };
    let (path, query) = match before_frag.find('?') {
        Some(i) => (&before_frag[..i], Some(before_frag[i + 1..].to_string())),
        None => (before_frag, None),
    };
    Some((scheme.to_string(), path.to_string(), query, fragment))
}

/// Parse `scheme://` style URIs where the host portion is empty (but user/pass
/// may be present). Returns (scheme, user, pass, path, query, fragment) or None
/// if the input doesn't match this shape.
#[allow(clippy::type_complexity)]
fn try_parse_empty_authority_uri(
    uri: &str,
) -> Option<(String, Option<String>, Option<String>, String, Option<String>, Option<String>)> {
    let sep = uri.find("://")?;
    let scheme = &uri[..sep];
    if scheme.is_empty() {
        return None;
    }
    if !scheme.bytes().next()?.is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
    {
        return None;
    }
    let rest = &uri[sep + 3..];
    for b in rest.bytes() {
        if b <= 0x20 || b >= 0x7f {
            return None;
        }
    }
    // Locate end of authority (first '/', '?', '#', or end).
    let auth_end = rest
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let path_and_rest = &rest[auth_end..];
    // Optional userinfo@
    let (user, pass, host_str) = if let Some(at) = authority.rfind('@') {
        let userinfo = &authority[..at];
        let hp = &authority[at + 1..];
        let (u, p) = match userinfo.find(':') {
            Some(c) => (userinfo[..c].to_string(), Some(userinfo[c + 1..].to_string())),
            None => (userinfo.to_string(), None),
        };
        (Some(u), p, hp)
    } else {
        (None, None, authority)
    };
    // Only accept when host is empty (otherwise url::Url::parse should have
    // succeeded; if it didn't, the authority was malformed).
    if !host_str.is_empty() {
        return None;
    }
    let (before_frag, fragment) = match path_and_rest.find('#') {
        Some(i) => (&path_and_rest[..i], Some(path_and_rest[i + 1..].to_string())),
        None => (path_and_rest, None),
    };
    let (path, query) = match before_frag.find('?') {
        Some(i) => (&before_frag[..i], Some(before_frag[i + 1..].to_string())),
        None => (before_frag, None),
    };
    Some((scheme.to_string(), user, pass, path.to_string(), query, fragment))
}

fn populate_empty_authority(
    obj: &mut PhpObject,
    parsed: &(String, Option<String>, Option<String>, String, Option<String>, Option<String>),
    original: &str,
) {
    let (scheme, user, pass, path, query, fragment) = parsed;
    obj.set_property(
        b"__uri_scheme".to_vec(),
        Value::String(PhpString::from_string(scheme.clone())),
    );
    obj.set_property(
        b"__uri_host".to_vec(),
        Value::String(PhpString::empty()),
    );
    obj.set_property(b"__uri_port".to_vec(), Value::Null);
    obj.set_property(
        b"__uri_user".to_vec(),
        match user {
            Some(u) => Value::String(PhpString::from_string(u.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_pass".to_vec(),
        match pass {
            Some(p) => Value::String(PhpString::from_string(p.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_path".to_vec(),
        Value::String(PhpString::from_string(path.clone())),
    );
    obj.set_property(
        b"__uri_query".to_vec(),
        match query {
            Some(q) => Value::String(PhpString::from_string(q.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_fragment".to_vec(),
        match fragment {
            Some(f) => Value::String(PhpString::from_string(f.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_serialized".to_vec(),
        Value::String(PhpString::from_string(original.to_string())),
    );
}

fn populate_scheme_uri(
    obj: &mut PhpObject,
    parsed: &(String, String, Option<String>, Option<String>),
    original: &str,
) {
    obj.set_property(
        b"__uri_scheme".to_vec(),
        Value::String(PhpString::from_string(parsed.0.clone())),
    );
    obj.set_property(b"__uri_host".to_vec(), Value::Null);
    obj.set_property(b"__uri_port".to_vec(), Value::Null);
    obj.set_property(b"__uri_user".to_vec(), Value::Null);
    obj.set_property(b"__uri_pass".to_vec(), Value::Null);
    obj.set_property(
        b"__uri_path".to_vec(),
        Value::String(PhpString::from_string(parsed.1.clone())),
    );
    obj.set_property(
        b"__uri_query".to_vec(),
        match &parsed.2 {
            Some(q) => Value::String(PhpString::from_string(q.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_fragment".to_vec(),
        match &parsed.3 {
            Some(f) => Value::String(PhpString::from_string(f.clone())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_serialized".to_vec(),
        Value::String(PhpString::from_string(original.to_string())),
    );
}

/// Populate URI components from a network-path reference (`//authority/path`
/// with no scheme) per RFC 3986.
fn populate_network_path(obj: &mut PhpObject, uri: &str) {
    // Strip the leading "//"
    let rest = &uri[2..];
    let auth_end = rest
        .find(|c: char| matches!(c, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let path_and_rest = &rest[auth_end..];
    // Parse userinfo@ if present
    let (user, pass, host_port) = if let Some(at) = authority.rfind('@') {
        let userinfo = &authority[..at];
        let hp = &authority[at + 1..];
        let (u, p) = match userinfo.find(':') {
            Some(c) => (userinfo[..c].to_string(), Some(userinfo[c + 1..].to_string())),
            None => (userinfo.to_string(), None),
        };
        (Some(u), p, hp)
    } else {
        (None, None, authority)
    };
    // Parse host[:port]
    let (host, port) = if host_port.starts_with('[') {
        if let Some(rb) = host_port.find(']') {
            let h = &host_port[..=rb];
            let tail = &host_port[rb + 1..];
            let port = tail.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
            (h.to_string(), port)
        } else {
            (host_port.to_string(), None)
        }
    } else {
        match host_port.rfind(':') {
            Some(i) => (host_port[..i].to_string(), host_port[i + 1..].parse::<u16>().ok()),
            None => (host_port.to_string(), None),
        }
    };
    let (before_frag, fragment) = match path_and_rest.find('#') {
        Some(i) => (&path_and_rest[..i], Some(path_and_rest[i + 1..].to_string())),
        None => (path_and_rest, None),
    };
    let (path, query) = match before_frag.find('?') {
        Some(i) => (&before_frag[..i], Some(before_frag[i + 1..].to_string())),
        None => (before_frag, None),
    };
    obj.set_property(b"__uri_scheme".to_vec(), Value::Null);
    obj.set_property(
        b"__uri_host".to_vec(),
        Value::String(PhpString::from_string(host)),
    );
    obj.set_property(
        b"__uri_port".to_vec(),
        match port {
            Some(p) => Value::Long(p as i64),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_user".to_vec(),
        match user {
            Some(u) => Value::String(PhpString::from_string(u)),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_pass".to_vec(),
        match pass {
            Some(p) => Value::String(PhpString::from_string(p)),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_path".to_vec(),
        Value::String(PhpString::from_string(path.to_string())),
    );
    obj.set_property(
        b"__uri_query".to_vec(),
        match query {
            Some(q) => Value::String(PhpString::from_string(q)),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_fragment".to_vec(),
        match fragment {
            Some(f) => Value::String(PhpString::from_string(f)),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_serialized".to_vec(),
        Value::String(PhpString::from_string(uri.to_string())),
    );
}

/// Populate URI components from a relative reference (no scheme/authority).
/// Splits on `?` for query and `#` for fragment, treats the rest as path.
fn populate_relative(obj: &mut PhpObject, uri: &str) {
    let (before_fragment, fragment) = match uri.find('#') {
        Some(i) => (&uri[..i], Some(&uri[i + 1..])),
        None => (uri, None),
    };
    let (path, query) = match before_fragment.find('?') {
        Some(i) => (&before_fragment[..i], Some(&before_fragment[i + 1..])),
        None => (before_fragment, None),
    };
    obj.set_property(b"__uri_scheme".to_vec(), Value::Null);
    obj.set_property(b"__uri_host".to_vec(), Value::Null);
    obj.set_property(b"__uri_port".to_vec(), Value::Null);
    obj.set_property(b"__uri_user".to_vec(), Value::Null);
    obj.set_property(b"__uri_pass".to_vec(), Value::Null);
    obj.set_property(
        b"__uri_path".to_vec(),
        Value::String(PhpString::from_string(path.to_string())),
    );
    obj.set_property(
        b"__uri_query".to_vec(),
        match query {
            Some(q) => Value::String(PhpString::from_string(q.to_string())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_fragment".to_vec(),
        match fragment {
            Some(f) => Value::String(PhpString::from_string(f.to_string())),
            None => Value::Null,
        },
    );
    obj.set_property(
        b"__uri_serialized".to_vec(),
        Value::String(PhpString::from_string(uri.to_string())),
    );
}

/// Re-serialize the object's URI from stored components (used for toString,
/// toAsciiString, etc.). Falls back to the stored serialized form if
/// components are missing.
pub fn serialize_uri(obj: &PhpObject) -> String {
    let s = obj.get_property(b"__uri_serialized");
    if let Value::String(s) = s {
        return s.to_string_lossy();
    }
    String::new()
}

fn component_to_value(obj: &PhpObject, prop: &[u8]) -> Value {
    obj.get_property(prop)
}

/// Dispatch a no-arg Uri method. Methods that take arguments are handled
/// separately (see uri_dispatch_with_args).
pub fn uri_dispatch_noarg(
    _class_lower: &[u8],
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    match method {
        b"getscheme" => Some(component_to_value(&ob, b"__uri_scheme")),
        b"gethost" => Some(component_to_value(&ob, b"__uri_host")),
        b"getport" => Some(component_to_value(&ob, b"__uri_port")),
        b"getusername" | b"getuserinfo" => Some(component_to_value(&ob, b"__uri_user")),
        b"getpassword" => Some(component_to_value(&ob, b"__uri_pass")),
        b"getpath" => Some(component_to_value(&ob, b"__uri_path")),
        b"getquery" => Some(component_to_value(&ob, b"__uri_query")),
        b"getfragment" => Some(component_to_value(&ob, b"__uri_fragment")),
        b"tostring" | b"toasciistring" | b"__tostring" | b"torawstring" => {
            Some(component_to_value(&ob, b"__uri_serialized"))
        }
        b"tounicodestring" => {
            // Decode the host (if Punycode) back to Unicode form. The rest
            // of the URI text is left as-is.
            let ascii = match ob.get_property(b"__uri_serialized") {
                Value::String(s) => s.to_string_lossy(),
                _ => return Some(Value::Null),
            };
            let host = match ob.get_property(b"__uri_host") {
                Value::String(s) => s.to_string_lossy(),
                _ => return Some(Value::String(PhpString::from_string(ascii))),
            };
            // Run idna::domain_to_unicode on the (already ASCII) host. It
            // returns the unicode form (empty errors on success).
            let (unicode_host, _errors) = idna::domain_to_unicode(&host);
            // Splice unicode_host back into the serialized form, replacing
            // the ASCII host occurrence verbatim.
            let out = if !host.is_empty() && ascii.contains(&host) {
                ascii.replacen(&host, &unicode_host, 1)
            } else {
                ascii
            };
            Some(Value::String(PhpString::from_string(out)))
        }
        b"tonormalizedstring" | b"tonormalizedasciistring" => {
            Some(component_to_value(&ob, b"__uri_serialized"))
        }
        b"equals" => None, // takes args
        b"resolve" => None, // takes args
        b"withscheme" | b"withhost" | b"withport" | b"withpath" | b"withquery"
        | b"withfragment" | b"withuserinfo" | b"withusername" | b"withpassword" => None,
        b"getrawuserinfo" | b"getrawhost" | b"getrawpath" | b"getrawquery"
        | b"getrawfragment" | b"getrawusername" | b"getrawpassword" => {
            // MVP: return same as non-raw getters
            let prop_name: &[u8] = match method {
                b"getrawuserinfo" | b"getrawusername" => b"__uri_user",
                b"getrawhost" => b"__uri_host",
                b"getrawpath" => b"__uri_path",
                b"getrawquery" => b"__uri_query",
                b"getrawfragment" => b"__uri_fragment",
                b"getrawpassword" => b"__uri_pass",
                _ => b"",
            };
            Some(component_to_value(&ob, prop_name))
        }
        _ => None,
    }
}

/// Build a new URL by applying a component change, returning the new serialized form.
/// Returns None if the change is invalid for the URL type.
fn with_component(current: &str, component: &str, value: Option<&str>) -> Option<String> {
    let mut url = Url::parse(current).ok()?;
    match component {
        "scheme" => {
            let v = value?;
            url.set_scheme(v).ok()?;
        }
        "host" => {
            url.set_host(value).ok()?;
        }
        "port" => {
            let p = value.and_then(|v| v.parse::<u16>().ok());
            url.set_port(p).ok()?;
        }
        "path" => {
            url.set_path(value.unwrap_or(""));
        }
        "query" => {
            url.set_query(value);
        }
        "fragment" => {
            url.set_fragment(value);
        }
        "username" => {
            url.set_username(value.unwrap_or("")).ok()?;
        }
        "password" => {
            url.set_password(value).ok()?;
        }
        _ => return None,
    }
    Some(url.as_str().to_string())
}

/// Dispatch Uri methods that take arguments. `args[0]` is `$this`, rest are user args.
/// Returns Some(value) if handled, None if unhandled.
pub fn uri_dispatch_with_args(
    class_lower: &[u8],
    method: &[u8],
    args: &[Value],
    next_object_id: &mut u64,
) -> Option<Value> {
    let this = args.first()?;
    let obj = match this {
        Value::Object(o) => o.clone(),
        _ => return None,
    };

    match method {
        b"equals" => {
            let other = args.get(1)?;
            let mode = args.get(2);
            // Determine exclude-fragment flag. The enum case is stored as a
            // plain string ("IncludeFragment" | "ExcludeFragment").
            let exclude_fragment = match mode {
                Some(Value::String(s)) => s.as_bytes() == b"ExcludeFragment",
                _ => false,
            };
            let other_obj = match other {
                Value::Object(o) => o,
                _ => return Some(Value::False),
            };
            let self_ob = obj.borrow();
            let other_ob = other_obj.borrow();
            fn part(ob: &PhpObject, key: &[u8]) -> Vec<u8> {
                match ob.get_property(key) {
                    Value::String(s) => s.as_bytes().to_vec(),
                    Value::Long(n) => n.to_string().into_bytes(),
                    Value::Null | Value::Undef => Vec::new(),
                    other => other.to_php_string().as_bytes().to_vec(),
                }
            }
            // Determine whether this is an RFC 3986 URI. RFC 3986 applies
            // syntax normalization (lowercase scheme/host, resolve .., decode
            // unreserved percent-encodings, remove default ports, normalize
            // IPv6) in equals(). WhatWg uses byte-exact comparison.
            let is_rfc3986 = class_lower == b"uri\\rfc3986\\uri";
            let scheme_self = part(&self_ob, b"__uri_scheme");
            let scheme_other = part(&other_ob, b"__uri_scheme");
            let compare_part = |key: &[u8]| -> bool {
                let a = part(&self_ob, key);
                let b = part(&other_ob, key);
                if !is_rfc3986 {
                    return a == b;
                }
                let na = normalize_rfc3986_component(key, &scheme_self, &a);
                let nb = normalize_rfc3986_component(key, &scheme_other, &b);
                na == nb
            };
            let eq = compare_part(b"__uri_scheme")
                && compare_part(b"__uri_host")
                && compare_part(b"__uri_port")
                && compare_part(b"__uri_user")
                && compare_part(b"__uri_pass")
                && compare_part(b"__uri_path")
                && compare_part(b"__uri_query")
                && (exclude_fragment || compare_part(b"__uri_fragment"));
            Some(if eq { Value::True } else { Value::False })
        }
        b"resolve" => {
            let arg = args.get(1).cloned().unwrap_or(Value::Null);
            let rel = arg.to_php_string().to_string_lossy();
            let ob = obj.borrow();
            let base_str = ob.get_property(b"__uri_serialized").to_php_string().to_string_lossy();
            drop(ob);
            let parsed = match Url::parse(&base_str) {
                Ok(base) => base.join(&rel).ok(),
                Err(_) => None,
            };
            parsed.map(|u| {
                let new_id = *next_object_id;
                *next_object_id += 1;
                let canonical = uri_canonical_name(class_lower).unwrap_or(b"Uri\\WhatWg\\Url");
                let mut new_obj = PhpObject::new(canonical.to_vec(), new_id);
                populate_from_url(&mut new_obj, &u);
                Value::Object(Rc::new(RefCell::new(new_obj)))
            })
        }
        m if m.starts_with(b"with") => {
            let component = match m {
                b"withscheme" => "scheme",
                b"withhost" => "host",
                b"withport" => "port",
                b"withpath" => "path",
                b"withquery" => "query",
                b"withfragment" => "fragment",
                b"withuserinfo" | b"withusername" => "username",
                b"withpassword" => "password",
                _ => return None,
            };
            let arg = args.get(1).cloned().unwrap_or(Value::Null);
            let value_str: Option<String> = match &arg {
                Value::Null => None,
                Value::Long(n) => Some(n.to_string()),
                Value::String(s) => Some(s.to_string_lossy()),
                _ => Some(arg.to_php_string().to_string_lossy()),
            };
            let ob = obj.borrow();
            let current = ob.get_property(b"__uri_serialized").to_php_string().to_string_lossy();
            drop(ob);
            let new_serialized = with_component(&current, component, value_str.as_deref())?;
            let parsed = Url::parse(&new_serialized).ok()?;
            let new_id = *next_object_id;
            *next_object_id += 1;
            let canonical = uri_canonical_name(class_lower).unwrap_or(b"Uri\\WhatWg\\Url");
            let mut new_obj = PhpObject::new(canonical.to_vec(), new_id);
            populate_from_url(&mut new_obj, &parsed);
            Some(Value::Object(Rc::new(RefCell::new(new_obj))))
        }
        _ => None,
    }
}

/// Return the list of "arg-taking" method names for Uri classes. Used by
/// `is_spl_args_method`-style gating.
pub fn uri_is_args_method(method: &[u8]) -> bool {
    matches!(
        method,
        b"equals"
            | b"resolve"
            | b"withscheme"
            | b"withhost"
            | b"withport"
            | b"withpath"
            | b"withquery"
            | b"withfragment"
            | b"withuserinfo"
            | b"withusername"
            | b"withpassword"
    )
}

/// Normalize an RFC 3986 component for equivalence comparison.
///
/// - scheme: lowercase
/// - host: lowercase, percent-decode unreserved chars, IPv6 normalize
/// - port: drop scheme default (http=80, https=443, ftp=21, ws=80, wss=443)
/// - user/pass/path/query/fragment: percent-decode unreserved, uppercase
///   hex digits, resolve `.`/`..` in path.
pub fn normalize_rfc3986_component(key: &[u8], scheme: &[u8], bytes: &[u8]) -> Vec<u8> {
    match key {
        b"__uri_scheme" => bytes.iter().map(|b| b.to_ascii_lowercase()).collect(),
        b"__uri_host" => {
            // Lowercase, percent-decode unreserved, normalize IPv6 literal.
            let lowered: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
            let decoded = percent_decode_unreserved(&lowered);
            if decoded.starts_with(b"[") && decoded.ends_with(b"]") {
                let inside = &decoded[1..decoded.len() - 1];
                if let Some(norm) = normalize_ipv6(std::str::from_utf8(inside).unwrap_or("")) {
                    let mut out = Vec::with_capacity(norm.len() + 2);
                    out.push(b'[');
                    out.extend_from_slice(norm.as_bytes());
                    out.push(b']');
                    return out;
                }
            }
            decoded
        }
        b"__uri_port" => {
            // Drop scheme's default port. Represent as empty bytes.
            if bytes.is_empty() {
                return Vec::new();
            }
            let s = match std::str::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => return bytes.to_vec(),
            };
            let port = match s.parse::<u32>() {
                Ok(p) => p,
                Err(_) => return bytes.to_vec(),
            };
            let default_port = match scheme {
                s if s.eq_ignore_ascii_case(b"http") => Some(80u32),
                s if s.eq_ignore_ascii_case(b"https") => Some(443),
                s if s.eq_ignore_ascii_case(b"ftp") => Some(21),
                s if s.eq_ignore_ascii_case(b"ws") => Some(80),
                s if s.eq_ignore_ascii_case(b"wss") => Some(443),
                _ => None,
            };
            if default_port == Some(port) {
                Vec::new()
            } else {
                port.to_string().into_bytes()
            }
        }
        b"__uri_path" => {
            // Percent-decode unreserved, resolve .. and . segments.
            let decoded = percent_decode_unreserved(bytes);
            resolve_dot_segments(&decoded)
        }
        b"__uri_user" | b"__uri_pass" | b"__uri_query" | b"__uri_fragment" => {
            percent_decode_unreserved(bytes)
        }
        _ => bytes.to_vec(),
    }
}

/// Percent-decode unreserved characters (RFC 3986 §6.2.2.2): alpha, digit, '-',
/// '.', '_', '~'. Normalize hex digits to uppercase for other percent-
/// encodings so that `%2f` and `%2F` compare equal.
fn percent_decode_unreserved(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            let h1 = input[i + 1];
            let h2 = input[i + 2];
            if let (Some(d1), Some(d2)) = (hex_digit(h1), hex_digit(h2)) {
                let byte = (d1 << 4) | d2;
                let is_unreserved = byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'.' | b'_' | b'~');
                if is_unreserved {
                    out.push(byte);
                } else {
                    // Preserve percent-encoding, uppercase hex.
                    out.push(b'%');
                    out.push(h1.to_ascii_uppercase());
                    out.push(h2.to_ascii_uppercase());
                }
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Resolve `.` and `..` segments in an RFC 3986 path. (RFC 3986 §5.2.4
/// "Remove Dot Segments".)
fn resolve_dot_segments(path: &[u8]) -> Vec<u8> {
    let mut input = path.to_vec();
    let mut output: Vec<u8> = Vec::with_capacity(path.len());
    while !input.is_empty() {
        if input.starts_with(b"../") {
            input.drain(..3);
        } else if input.starts_with(b"./") {
            input.drain(..2);
        } else if input.starts_with(b"/./") {
            input.drain(..2);
            input.insert(0, b'/');
        } else if input == b"/." {
            input = b"/".to_vec();
        } else if input.starts_with(b"/../") {
            input.drain(..3);
            input.insert(0, b'/');
            // Remove last segment from output.
            while let Some(&b) = output.last() {
                output.pop();
                if b == b'/' {
                    break;
                }
            }
        } else if input == b"/.." {
            input = b"/".to_vec();
            while let Some(&b) = output.last() {
                output.pop();
                if b == b'/' {
                    break;
                }
            }
        } else if input == b"." || input == b".." {
            break;
        } else {
            // Move first segment (up to next '/') to output.
            // At least the first character if it's '/', then until next '/'.
            let mut split = 0;
            if input[0] == b'/' {
                split = 1;
            }
            while split < input.len() && input[split] != b'/' {
                split += 1;
            }
            output.extend_from_slice(&input[..split]);
            input.drain(..split);
        }
    }
    output
}

/// Normalize an IPv6 address per RFC 5952: lowercase hex, leading-zero strip,
/// longest zero-run compressed as `::`. Returns None if the input isn't a
/// valid IPv6 literal.
fn normalize_ipv6(addr: &str) -> Option<String> {
    // Parse via Rust stdlib.
    let ip: std::net::Ipv6Addr = addr.parse().ok()?;
    // Use the stdlib Display for canonicalization (already RFC 5952-ish).
    Some(ip.to_string())
}

/// For var_dump/print_r: return a PhpArray view of the URI components that
/// matches PHP's expected property set. MVP: return all __uri_* props without
/// the __ prefix.
pub fn uri_component_array(obj: &PhpObject) -> PhpArray {
    let mut arr = PhpArray::new();
    for (k, v) in &obj.properties {
        if k.starts_with(b"__uri_") {
            let display = &k[b"__uri_".len()..];
            arr.set(
                crate::array::ArrayKey::String(PhpString::from_vec(display.to_vec())),
                v.clone(),
            );
        }
    }
    arr
}
