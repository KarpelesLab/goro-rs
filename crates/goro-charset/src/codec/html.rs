//! HTML-ENTITIES (deprecated since PHP 8.2): bytes are Latin-1 characters,
//! with HTML 4.01 named and numeric character references decoded; encoding
//! writes ASCII as is and everything else as a named or decimal reference
//! (observed on PHP 8.5; test ext/mbstring/tests/htmlent.phpt).

use super::{Decode, ERR, Encode, Subst};
use crate::tables::html as t;

#[derive(Default)]
pub(crate) struct HtmlDec;

/// Parses a character reference at `s[i]` (which is `&`). Returns the code
/// point and the length consumed, or `None` to leave the `&` literal.
fn reference(s: &[u8], i: usize) -> Option<(u32, usize)> {
    let rest = &s[i + 1..];
    if rest.first() == Some(&b'#') {
        let (hex, start) = match rest.get(1) {
            Some(b'x' | b'X') => (true, 2),
            _ => (false, 1),
        };
        let mut v: u64 = 0;
        let mut j = start;
        while j < rest.len() && j - start < 16 {
            let d = match rest[j] {
                c @ b'0'..=b'9' => (c - b'0') as u64,
                c @ b'a'..=b'f' if hex => (c - b'a' + 10) as u64,
                c @ b'A'..=b'F' if hex => (c - b'A' + 10) as u64,
                _ => break,
            };
            v = v * if hex { 16 } else { 10 } + d;
            j += 1;
        }
        if j == start || rest.get(j) != Some(&b';') || v > 0x10FFFF {
            return None;
        }
        return Some((v as u32, j + 2));
    }
    let mut j = 0;
    while j < rest.len() && rest[j].is_ascii_alphanumeric() {
        j += 1;
    }
    if j == 0 || rest.get(j) != Some(&b';') {
        return None;
    }
    let name = std::str::from_utf8(&rest[..j]).ok()?;
    let k = t::BY_NAME.binary_search_by_key(&name, |e| e.0).ok()?;
    Some((t::BY_NAME[k].1, j + 2))
}

impl Decode for HtmlDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        while i < s.len() && out.len() < limit {
            if s[i] == b'&'
                && let Some((cp, len)) = reference(s, i)
            {
                out.push(cp);
                i += len;
                continue;
            }
            out.push(s[i] as u32);
            i += 1;
        }
        *pos = i;
    }
}

pub(crate) struct HtmlEnc;

impl Encode for HtmlEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            return true;
        }
        out.push(b'&');
        match t::BY_CP.binary_search_by_key(&cp, |e| e.0) {
            Ok(k) => out.extend_from_slice(t::BY_CP[k].1.as_bytes()),
            Err(_) => {
                out.push(b'#');
                out.extend_from_slice(cp.to_string().as_bytes());
            }
        }
        out.push(b';');
        true
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {
        // Even invalid input is written, as its marker's decimal value
        // (observed on PHP 8.5: "&#4294967295;").
        for &cp in cps {
            self.put(if cp == ERR { 0xFFFF_FFFF } else { cp }, out);
        }
    }
}
