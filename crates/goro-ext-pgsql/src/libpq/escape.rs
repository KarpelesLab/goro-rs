//! String escaping (fe-exec.c): PQescapeStringConn, PQescapeLiteral,
//! PQescapeIdentifier, PQescapeByteaConn and PQunescapeBytea.

use super::conn::PgConn;
use super::encoding;

/// PQescapeStringInternal: (escaped, error)
pub fn escape_string_internal(
    from: &[u8],
    enc: i32,
    std_strings: bool,
    mut complain: Option<&mut PgConn>,
) -> (Vec<u8>, bool) {
    let n = from.iter().position(|&c| c == 0).unwrap_or(from.len());
    let src = &from[..n];
    let mut out = Vec::with_capacity(src.len() * 2);
    let mut error = false;
    let mut complained = false;
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c < 0x80 {
            if c == b'\'' || (c == b'\\' && !std_strings) {
                out.push(c);
            }
            out.push(c);
            i += 1;
            continue;
        }
        let remaining = src.len() - i;
        let charlen = encoding::mblen(enc, &src[i..]);
        let valid = remaining >= charlen && encoding::verify_mbchar(enc, &src[i..]).is_some();
        if !valid {
            error = true;
            if let Some(c) = complain.as_deref_mut() {
                if !complained {
                    if remaining < charlen {
                        c.append_error("incomplete multibyte character");
                    } else {
                        c.append_error("invalid multibyte character");
                    }
                    complained = true;
                }
            }
            // pg_encoding_set_invalid
            if encoding::max_length(enc) > 1 {
                out.push(if enc == encoding::PG_UTF8 { 0xc0 } else { 0x8d });
                out.push(b' ');
            }
            i += 1;
        } else {
            out.extend_from_slice(&src[i..i + charlen]);
            i += charlen;
        }
    }
    (out, error)
}

thread_local! {
    /// static_client_encoding / static_std_strings: the settings of the
    /// last connection that reported them
    pub static STATIC: std::cell::Cell<(i32, bool)> = const { std::cell::Cell::new((0, false)) };
}

/// PQescapeString (no connection: the static settings)
pub fn escape_string(from: &[u8]) -> Vec<u8> {
    let (enc, std) = STATIC.with(|s| s.get());
    escape_string_internal(from, enc, std, None).0
}

/// PQescapeByteaInternal
pub fn escape_bytea_internal(from: &[u8], std_strings: bool, use_hex: bool) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(from.len() * 2 + 3);
    if use_hex {
        if !std_strings {
            out.push(b'\\');
        }
        out.push(b'\\');
        out.push(b'x');
    }
    for &c in from {
        if use_hex {
            out.push(HEX[(c >> 4) as usize]);
            out.push(HEX[(c & 15) as usize]);
        } else if !(0x20..=0x7e).contains(&c) {
            if !std_strings {
                out.push(b'\\');
            }
            out.push(b'\\');
            out.push((c >> 6) + b'0');
            out.push(((c >> 3) & 7) + b'0');
            out.push((c & 7) + b'0');
        } else if c == b'\'' {
            out.push(b'\'');
            out.push(b'\'');
        } else if c == b'\\' {
            if !std_strings {
                out.push(b'\\');
                out.push(b'\\');
            }
            out.push(b'\\');
            out.push(b'\\');
        } else {
            out.push(c);
        }
    }
    out
}

/// PQescapeBytea (no connection)
pub fn escape_bytea(from: &[u8]) -> Vec<u8> {
    let (_, std) = STATIC.with(|s| s.get());
    escape_bytea_internal(from, std, false)
}

fn get_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// PQunescapeBytea
pub fn unescape_bytea(strtext: &[u8]) -> Vec<u8> {
    let n = strtext
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(strtext.len());
    let s = &strtext[..n];
    if s.len() >= 2 && s[0] == b'\\' && s[1] == b'x' {
        let mut out = Vec::with_capacity((s.len() - 2) / 2);
        let mut i = 2;
        while i < s.len() {
            let v1 = get_hex(s[i]);
            i += 1;
            if i >= s.len() || v1.is_none() {
                continue;
            }
            let v2 = get_hex(s[i]);
            i += 1;
            if let (Some(a), Some(b)) = (v1, v2) {
                out.push((a << 4) | b);
            }
        }
        return out;
    }
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    let oct = |c: u8| (b'0'..=b'7').contains(&c);
    while i < s.len() {
        if s[i] == b'\\' {
            i += 1;
            if i < s.len() && s[i] == b'\\' {
                out.push(b'\\');
                i += 1;
            } else if i + 2 < s.len()
                && (b'0'..=b'3').contains(&s[i])
                && oct(s[i + 1])
                && oct(s[i + 2])
            {
                let b = ((s[i] - b'0') << 6) | ((s[i + 1] - b'0') << 3) | (s[i + 2] - b'0');
                out.push(b);
                i += 3;
            }
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

impl PgConn {
    /// PQescapeStringConn: (escaped, error)
    pub fn escape_string_conn(&mut self, from: &[u8]) -> (Vec<u8>, bool) {
        self.clear_error_state();
        let enc = self.client_encoding;
        let std = self.std_strings;
        escape_string_internal(from, enc, std, Some(self))
    }

    /// PQescapeInternal: PQescapeLiteral / PQescapeIdentifier
    pub fn escape_internal(&mut self, s: &[u8], as_ident: bool) -> Option<Vec<u8>> {
        if !self.cmd_queued {
            self.clear_error_state();
        }
        let input_len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        let s = &s[..input_len];
        let quote = if as_ident { b'"' } else { b'\'' };
        let enc = self.client_encoding;
        let mut num_quotes = 0;
        let mut num_backslashes = 0;
        let mut validated = false;
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            if c == quote {
                num_quotes += 1;
            } else if c == b'\\' {
                num_backslashes += 1;
            } else if c >= 0x80 {
                let remaining = s.len() - i;
                let charlen = encoding::mblen(enc, &s[i..]);
                if charlen > remaining {
                    self.append_error("incomplete multibyte character");
                    return None;
                }
                if !validated {
                    if encoding::verify_mbstr(enc, &s[i..]) != remaining {
                        self.append_error("invalid multibyte character");
                        return None;
                    }
                    validated = true;
                }
                i += charlen;
                continue;
            }
            i += 1;
        }
        let _ = num_quotes;
        let mut out = Vec::with_capacity(s.len() + 6);
        if !as_ident && num_backslashes > 0 {
            out.push(b' ');
            out.push(b'E');
        }
        out.push(quote);
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            if c == quote || (!as_ident && c == b'\\') {
                out.push(c);
                out.push(c);
                i += 1;
            } else if c < 0x80 {
                out.push(c);
                i += 1;
            } else {
                let l = encoding::mblen(enc, &s[i..]).min(s.len() - i);
                out.extend_from_slice(&s[i..i + l]);
                i += l;
            }
        }
        out.push(quote);
        Some(out)
    }

    /// PQescapeLiteral
    pub fn escape_literal(&mut self, s: &[u8]) -> Option<Vec<u8>> {
        self.escape_internal(s, false)
    }

    /// PQescapeIdentifier
    pub fn escape_identifier(&mut self, s: &[u8]) -> Option<Vec<u8>> {
        self.escape_internal(s, true)
    }

    /// PQescapeByteaConn
    pub fn escape_bytea_conn(&mut self, from: &[u8]) -> Vec<u8> {
        let hex = self.sversion >= 90000;
        escape_bytea_internal(from, self.std_strings, hex)
    }
}
