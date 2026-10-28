//! quoted_printable_encode()/decode(), convert_uuencode()/uudecode(),
//! bin2hex()/hex2bin(), crc32(), md5(), sha1() and str_rot13().
//!
//! Behaviour sources: the PHP manual pages of these functions, RFC 2045
//! section 6.7 (quoted-printable), the POSIX uuencode format, the CRC-32
//! of ISO 3309 / ITU-T V.42 (as used by zlib and PNG), RFC 1321 (MD5) and
//! FIPS 180 (SHA-1, through the purecrypto crate), the tests
//! ext/standard/tests/strings/*.phpt for these functions, and observation
//! of PHP 8.5 (tests/fixtures/encoding.txt and its driver encoding.php).

use crate::error::Diag;

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// bin2hex(): lowercase hex digits.
pub fn bin2hex(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for &c in s {
        out.push(HEX_LOWER[(c >> 4) as usize]);
        out.push(HEX_LOWER[(c & 15) as usize]);
    }
    out
}

/// hex2bin(): `None` (false) with a warning for an odd length or a
/// non-hex digit (test hex2bin_error.phpt).
pub fn hex2bin(s: &[u8], diags: &mut Vec<Diag>) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        diags.push(Diag::Warning(
            "Hexadecimal input string must have an even length".into(),
        ));
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for p in s.chunks(2) {
        match (hex_val(p[0]), hex_val(p[1])) {
            (Some(h), Some(l)) => out.push(h << 4 | l),
            _ => {
                diags.push(Diag::Warning(
                    "Input string must be hexadecimal string".into(),
                ));
                return None;
            }
        }
    }
    Some(out)
}

/// str_rot13().
pub fn str_rot13(s: &[u8]) -> Vec<u8> {
    s.iter()
        .map(|&c| match c {
            b'a'..=b'z' => (c - b'a' + 13) % 26 + b'a',
            b'A'..=b'Z' => (c - b'A' + 13) % 26 + b'A',
            _ => c,
        })
        .collect()
}

/// The CRC-32 table of the reflected polynomial 0xEDB88320.
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// Continues a CRC-32 over more data (start with 0).
pub fn crc32_update(crc: u32, s: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in s {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// crc32(): the unsigned checksum (PHP returns it as a positive int on
/// 64-bit platforms).
pub fn crc32(s: &[u8]) -> u32 {
    crc32_update(0, s)
}

fn digest_out(d: &[u8], binary: bool) -> Vec<u8> {
    if binary { d.to_vec() } else { bin2hex(d) }
}

/// md5(): hex digits, or the raw 16 bytes with `$binary`.
pub fn md5(s: &[u8], binary: bool) -> Vec<u8> {
    digest_out(&purecrypto::hash::md5(s), binary)
}

/// sha1(): hex digits, or the raw 20 bytes with `$binary`.
pub fn sha1(s: &[u8], binary: bool) -> Vec<u8> {
    digest_out(&purecrypto::hash::sha1(s), binary)
}

/// quoted_printable_encode(): RFC 2045 section 6.7, with soft line breaks
/// keeping lines at most 76 characters (PHP manual).
pub fn quoted_printable_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 2);
    let mut line = 0usize;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'\r' && s.get(i + 1) == Some(&b'\n') {
            out.extend_from_slice(b"\r\n");
            i += 2;
            line = 0;
            continue;
        }
        // Controls (tabs included), "=", bytes from 0x7F, and a space
        // before a CR are escaped (observed on PHP 8.5).
        let escape =
            c < 0x20 || c == b'=' || c >= 0x7F || (c == b' ' && s.get(i + 1) == Some(&b'\r'));
        let width = if escape { 3 } else { 1 };
        // Lines hold at most 75 characters before the soft break; an
        // escaped byte that may start a UTF-8 sequence (0x80..0xF4) needs
        // room for the rest of the sequence too, and bytes from 0xF5 never
        // cause a break (observed on PHP 8.5).
        let reserve = match c {
            0x80..=0xDF => 3,
            0xE0..=0xEF => 6,
            0xF0..=0xF4 => 9,
            _ => 0,
        };
        if c < 0xF5 && line + width + if escape { reserve } else { 0 } > 75 {
            out.extend_from_slice(b"=\r\n");
            line = 0;
        }
        if escape {
            out.extend_from_slice(&[
                b'=',
                HEX_UPPER[(c >> 4) as usize],
                HEX_UPPER[(c & 15) as usize],
            ]);
        } else {
            out.push(c);
        }
        line += width;
        i += 1;
    }
    out
}

/// quoted_printable_decode(): `=XX` (either case), soft line breaks `=`
/// followed by optional blanks and a line break; other `=` are kept.
pub fn quoted_printable_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != b'=' {
            out.push(c);
            i += 1;
            continue;
        }
        if let (Some(h), Some(l)) = (
            s.get(i + 1).and_then(|&x| hex_val(x)),
            s.get(i + 2).and_then(|&x| hex_val(x)),
        ) {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        let mut k = i + 1;
        while k < s.len() && (s[k] == b' ' || s[k] == b'\t') {
            k += 1;
        }
        if s.get(k) == Some(&b'\r') && s.get(k + 1) == Some(&b'\n') {
            i = k + 2;
        } else if s.get(k) == Some(&b'\n') || s.get(k) == Some(&b'\r') {
            i = k + 1;
        } else if k == s.len() {
            // A soft break at the end of the input (observed on PHP 8.5).
            i = k;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn uu_char(v: u8) -> u8 {
    if v == 0 { b'`' } else { v + 32 }
}

/// convert_uuencode(): lines of at most 45 bytes, `` ` `` for zero, ending
/// with a `` ` `` line.
pub fn convert_uuencode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 4 / 3 + s.len() / 45 * 2 + 4);
    for line in s.chunks(45) {
        out.push(uu_char(line.len() as u8));
        for g in line.chunks(3) {
            let b = [g[0], *g.get(1).unwrap_or(&0), *g.get(2).unwrap_or(&0)];
            out.push(uu_char(b[0] >> 2));
            out.push(uu_char((b[0] << 4 | b[1] >> 4) & 63));
            out.push(uu_char((b[1] << 2 | b[2] >> 6) & 63));
            out.push(uu_char(b[2] & 63));
        }
        out.push(b'\n');
    }
    out.extend_from_slice(b"`\n");
    out
}

/// convert_uudecode(): `None` (false) with a warning for malformed data.
///
/// Observed on PHP 8.5: each line starts with its byte count; its groups of
/// four characters are read even across line breaks; a line shorter than
/// 45 bytes (or a zero count) ends the data, whatever follows; after a
/// full line one character (the line break) is skipped. Empty input and a
/// line without enough characters are errors.
pub fn convert_uudecode(s: &[u8], diags: &mut Vec<Diag>) -> Option<Vec<u8>> {
    let bad = |diags: &mut Vec<Diag>| {
        diags.push(Diag::Warning(
            "Argument #1 ($data) is not a valid uuencoded string".into(),
        ));
        None
    };
    if s.is_empty() {
        return bad(diags);
    }
    let dec = |c: u8| (c.wrapping_sub(32)) & 63;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut i = 0;
    while i < s.len() {
        let n = dec(s[i]) as usize;
        i += 1;
        if n == 0 {
            break;
        }
        let need = n.div_ceil(3) * 4;
        if i + need > s.len() {
            return bad(diags);
        }
        let start = out.len();
        for g in s[i..i + need].chunks(4) {
            let v = [dec(g[0]), dec(g[1]), dec(g[2]), dec(g[3])];
            out.extend_from_slice(&[
                v[0] << 2 | v[1] >> 4,
                v[1] << 4 | v[2] >> 2,
                v[2] << 6 | v[3],
            ]);
        }
        out.truncate(start + n);
        i += need;
        if n < 45 {
            break;
        }
        i += 1;
    }
    Some(out)
}

#[cfg(test)]
mod tests;
