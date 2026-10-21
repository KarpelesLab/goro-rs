//! HTTP helpers (lib/http.c): header matching, auth state

use super::consts::auth;

/// struct auth
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct AuthState {
    pub want: u64,
    pub picked: u64,
    pub avail: u64,
    pub done: bool,
    pub multipass: bool,
    pub iestyle: bool,
}

pub(crate) use super::httpauth::DigestData;

/// pickoneauth()
pub(crate) fn pickoneauth(pick: &mut AuthState, mask: u64) -> bool {
    let avail = pick.avail & pick.want & mask;
    let mut picked = true;
    if avail & auth::NEGOTIATE != 0 {
        pick.picked = auth::NEGOTIATE;
    } else if avail & auth::BEARER != 0 {
        pick.picked = auth::BEARER;
    } else if avail & auth::DIGEST != 0 {
        pick.picked = auth::DIGEST;
    } else if avail & auth::NTLM != 0 {
        pick.picked = auth::NTLM;
    } else if avail & auth::BASIC != 0 {
        pick.picked = auth::BASIC;
    } else if avail & auth::AWS_SIGV4 != 0 {
        pick.picked = auth::AWS_SIGV4;
    } else {
        pick.picked = 1u64 << 30; // CURLAUTH_PICKNONE
        picked = false;
    }
    pick.avail = auth::NONE;
    picked
}

/// Does a header line (e.g. "Content-Type: x") have the given name
/// (`name` without the colon)?
pub(crate) fn header_is(line: &[u8], name: &[u8]) -> bool {
    line.len() > name.len()
        && line[..name.len()].eq_ignore_ascii_case(name)
        && line[name.len()] == b':'
}

/// HD_VAL(): the value of the header after the colon and blanks
pub(crate) fn header_val<'a>(line: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    if !header_is(line, name) {
        return None;
    }
    let mut v = &line[name.len() + 1..];
    while let Some((&c, rest)) = v.split_first() {
        if c == b' ' || c == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    Some(v)
}

/// Curl_copy_header_value(): value without leading/trailing whitespace
pub(crate) fn copy_header_value(line: &[u8]) -> Vec<u8> {
    let start = match line.iter().position(|&c| c == b':') {
        Some(i) => i + 1,
        None => return Vec::new(),
    };
    let mut v = &line[start..];
    while let Some((&c, rest)) = v.split_first() {
        if c == b' ' || c == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    let end = v
        .iter()
        .position(|&c| c == b'\r' || c == b'\n')
        .unwrap_or(v.len());
    let mut v = &v[..end];
    while let Some((&c, rest)) = v.split_last() {
        if c == b' ' || c == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    v.to_vec()
}

/// Curl_compareheader(): does `line` (a header with name `name`, given
/// without colon) contain the token `content`?
pub(crate) fn compareheader(line: &[u8], name: &[u8], content: &[u8]) -> bool {
    if !header_is(line, name) {
        return false;
    }
    let mut v = &line[name.len() + 1..];
    while let Some((&c, rest)) = v.split_first() {
        if c == b' ' || c == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    let end = v
        .iter()
        .position(|&c| c == b'\r' || c == b'\n')
        .unwrap_or(v.len());
    // libcurl's search loop only ever checks the start of the value
    let v = &v[..end];
    v.len() >= content.len() && v[..content.len()].eq_ignore_ascii_case(content)
}

/// HD_IS_AND_SAYS(): header with that name whose value starts with `val`
pub(crate) fn header_is_and_says(line: &[u8], name: &[u8], val: &[u8]) -> bool {
    compareheader(line, name, val)
}

/// Curl_checkheaders(): the first custom header (from CURLOPT_HTTPHEADER)
/// with this name (and a colon or semicolon after it)
pub(crate) fn checkheaders<'a>(headers: Option<&'a Vec<Vec<u8>>>, name: &[u8]) -> Option<&'a [u8]> {
    let headers = headers?;
    for h in headers {
        if h.len() > name.len()
            && h[..name.len()].eq_ignore_ascii_case(name)
            && (h[name.len()] == b':' || h[name.len()] == b';')
        {
            return Some(h);
        }
    }
    None
}

/// The weekday and month names of Curl_wkday / Curl_month
pub(crate) const WKDAY: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub(crate) const MONTH: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// gmtime(): (year, mon0, mday, hour, min, sec, wday(0=Sunday))
pub(crate) fn gmtime(t: i64) -> (i64, usize, i64, i64, i64, i64, usize) {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let (hour, min, sec) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let wday = (days + 4).rem_euclid(7) as usize;
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, (m - 1) as usize, d, hour, min, sec, wday)
}
