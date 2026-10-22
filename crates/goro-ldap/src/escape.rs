//! ldap_escape() (php_ldap_do_escape in ldap.c), kept here so the client
//! crate owns the LDAP string escaping rules.

pub const ESCAPE_FILTER: i64 = 0x01;
pub const ESCAPE_DN: i64 = 0x02;

/// php_ldap_do_escape. `map[c]` tells whether byte c must be escaped.
/// Returns None if the result would exceed `max_len` (ZSTR_MAX_LEN).
pub fn do_escape(map: &[bool; 256], value: &[u8], flags: i64, max_len: usize) -> Option<Vec<u8>> {
    let mut len: usize = 0;
    for &b in value {
        let addend = if map[b as usize] { 3 } else { 1 };
        if len > max_len - addend {
            return None;
        }
        len += addend;
    }
    if flags & ESCAPE_DN != 0 && value.first() == Some(&b' ') {
        if len > max_len - 2 {
            return None;
        }
        len += 2;
    }
    if flags & ESCAPE_DN != 0 && value.len() > 1 && value.last() == Some(&b' ') {
        if len > max_len - 2 {
            return None;
        }
        len += 2;
    }
    let hex = b"0123456789abcdef";
    let mut out = Vec::with_capacity(len);
    let last = value.len().wrapping_sub(1);
    for (i, &v) in value.iter().enumerate() {
        let edge_space = flags & ESCAPE_DN != 0 && (i == 0 || i == last) && v == b' ';
        if map[v as usize] || edge_space {
            out.push(b'\\');
            out.push(hex[(v >> 4) as usize]);
            out.push(hex[(v & 0x0f) as usize]);
        } else {
            out.push(v);
        }
    }
    Some(out)
}

/// Build the escape map from the flags and the ignore list.
pub fn build_map(flags: i64, ignore: &[u8]) -> [bool; 256] {
    let mut map = [false; 256];
    let mut havecharlist = false;
    if flags & ESCAPE_FILTER != 0 {
        havecharlist = true;
        for &c in b"\\*()\0" {
            map[c as usize] = true;
        }
    }
    if flags & ESCAPE_DN != 0 {
        havecharlist = true;
        for &c in b"\\,=+<>;\"#\r" {
            map[c as usize] = true;
        }
    }
    if !havecharlist {
        for m in map.iter_mut() {
            *m = true;
        }
    }
    for &c in ignore {
        map[c as usize] = false;
    }
    map
}
