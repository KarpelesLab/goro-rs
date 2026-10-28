//! Fast paths for the common conversions. Each one either converts the whole
//! input or gives up (`None`) at the first thing it doesn't handle (illegal
//! input, an unencodable character), and the general loop then starts over,
//! so the results and errors are the same.

use super::charset::{Builtin, Charset, Kind};
use super::table::{CHAR_BASE, MULTI_FLAG, NODE_FLAG, Table, TableEncoder, push_packed};
use super::unicode::ascii_prefix;

/// Tries a fast path for `from` -> `to`.
pub(crate) fn convert(from: Charset, to: Charset, input: &[u8]) -> Option<Vec<u8>> {
    match from.0.kind {
        Kind::Builtin(Builtin::Utf8) => {
            let s = std::str::from_utf8(input).ok()?;
            from_utf8(s, to)
        }
        Kind::Table(t) if t.ascii && t.compose.is_empty() => {
            if matches!(to.0.kind, Kind::Builtin(Builtin::Utf8)) {
                table_to_utf8(t, input)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn from_utf8(s: &str, to: Charset) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    match to.0.kind {
        Kind::Builtin(Builtin::Utf8) => Some(b.to_vec()),
        Kind::Builtin(Builtin::Utf16Le) => Some(utf8_to_wide(b, 2, false)),
        Kind::Builtin(Builtin::Utf16Be) => Some(utf8_to_wide(b, 2, true)),
        Kind::Builtin(Builtin::Ucs2) if no_astral(b) => Some(utf8_to_wide(b, 2, false)),
        Kind::Builtin(Builtin::Ucs2Be) if no_astral(b) => Some(utf8_to_wide(b, 2, true)),
        Kind::Builtin(Builtin::Utf32Le) | Kind::Builtin(Builtin::Ucs4Le) => {
            Some(utf8_to_wide(b, 4, false))
        }
        Kind::Builtin(Builtin::Utf32Be) | Kind::Builtin(Builtin::Ucs4) => {
            Some(utf8_to_wide(b, 4, true))
        }
        Kind::Builtin(Builtin::Ascii) => b.is_ascii().then(|| b.to_vec()),
        Kind::Table(t) if t.ident_upto == 256 => utf8_to_latin1(b),
        Kind::Table(t) if t.pairs.is_empty() => utf8_to_table(b, t),
        Kind::Builtin(Builtin::Gb18030) => None,
        _ => None,
    }
}

fn no_astral(b: &[u8]) -> bool {
    !b.iter().any(|&c| c >= 0xF0)
}

/// Decodes the UTF-8 sequence at `b[*i]` (known valid).
#[inline(always)]
fn next_char(b: &[u8], i: &mut usize) -> u32 {
    let c = b[*i];
    if c < 0xE0 {
        let v = (u32::from(c & 0x1F) << 6) | u32::from(b[*i + 1] & 0x3F);
        *i += 2;
        v
    } else if c < 0xF0 {
        let v = (u32::from(c & 0x0F) << 12)
            | (u32::from(b[*i + 1] & 0x3F) << 6)
            | u32::from(b[*i + 2] & 0x3F);
        *i += 3;
        v
    } else {
        let v = (u32::from(c & 0x07) << 18)
            | (u32::from(b[*i + 1] & 0x3F) << 12)
            | (u32::from(b[*i + 2] & 0x3F) << 6)
            | u32::from(b[*i + 3] & 0x3F);
        *i += 4;
        v
    }
}

/// Length of the ASCII run at `b[i..]`: byte by byte for the first few, a
/// word at a time after that.
#[inline(always)]
fn ascii_run(b: &[u8], i: usize) -> usize {
    let mut n = 0;
    while n < 8 && i + n < b.len() && b[i + n] < 0x80 {
        n += 1;
    }
    if n == 8 {
        8 + ascii_prefix(&b[i + 8..])
    } else {
        n
    }
}

fn utf8_to_wide(b: &[u8], width: usize, be: bool) -> Vec<u8> {
    // At most `width` bytes per input byte (four-byte UTF-8 sequences give a
    // surrogate pair).
    let mut out = vec![0u8; b.len() * width];
    let mut o = 0;
    let mut i = 0;
    let put16 = |out: &mut [u8], o: &mut usize, v: u32| {
        let (hi, lo) = ((v >> 8) as u8, v as u8);
        if be {
            out[*o] = hi;
            out[*o + 1] = lo;
        } else {
            out[*o] = lo;
            out[*o + 1] = hi;
        }
        *o += 2;
    };
    while i < b.len() {
        let c = b[i];
        if c < 0x80 {
            let n = ascii_run(b, i);
            let dst = &mut out[o..o + n * width];
            let at = if be { width - 1 } else { 0 };
            for (d, &x) in dst.chunks_exact_mut(width).zip(&b[i..i + n]) {
                d[at] = x;
            }
            o += n * width;
            i += n;
            continue;
        }
        if width == 2 && (0xE0..0xF0).contains(&c) {
            // The common case for CJK text: three bytes, one unit.
            let v = (u32::from(c & 0x0F) << 12)
                | (u32::from(b[i + 1] & 0x3F) << 6)
                | u32::from(b[i + 2] & 0x3F);
            put16(&mut out, &mut o, v);
            i += 3;
            continue;
        }
        let cp = next_char(b, &mut i);
        if width == 4 {
            out[o..o + 4].copy_from_slice(&if be {
                cp.to_be_bytes()
            } else {
                cp.to_le_bytes()
            });
            o += 4;
        } else if cp >= 0x1_0000 {
            let c = cp - 0x1_0000;
            put16(&mut out, &mut o, 0xD800 | (c >> 10));
            put16(&mut out, &mut o, 0xDC00 | (c & 0x3FF));
        } else {
            put16(&mut out, &mut o, cp);
        }
    }
    finish(out, o)
}

/// UTF-8 to ISO-8859-1 (the table that maps every byte to itself).
fn utf8_to_latin1(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![0u8; b.len()];
    let mut o = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < 0x80 {
            if i + 8 <= b.len() {
                let w = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
                if w & 0x8080_8080_8080_8080 == 0 {
                    out[o..o + 8].copy_from_slice(&b[i..i + 8]);
                    o += 8;
                    i += 8;
                    continue;
                }
            }
            out[o] = c;
            o += 1;
            i += 1;
        } else if c == 0xC2 || c == 0xC3 {
            out[o] = ((c & 0x03) << 6) | (b[i + 1] & 0x3F);
            o += 1;
            i += 2;
        } else {
            return None;
        }
    }
    Some(finish(out, o))
}

fn utf8_to_table(b: &[u8], t: &'static Table) -> Option<Vec<u8>> {
    let enc = TableEncoder::new(t);
    // Table sequences are rarely longer than the UTF-8 they come from; the
    // buffer grows when they are.
    let mut out = vec![0u8; b.len() + 16];
    let mut o = 0;
    let mut i = 0;
    let ascii = t.ascii;
    let latin1 = t.ident_upto == 256;
    while i < b.len() {
        if o + 4 > out.len() {
            out.resize(out.len() * 2, 0);
        }
        let c = b[i];
        if ascii && c < 0x80 {
            let n = ascii_run(b, i);
            out[o..o + n].copy_from_slice(&b[i..i + n]);
            o += n;
            i += n;
            continue;
        }
        // ISO-8859-1: U+0080-U+00FF are the byte itself.
        if latin1 && (c == 0xC2 || c == 0xC3) {
            out[o] = ((c & 0x03) << 6) | (b[i + 1] & 0x3F);
            o += 1;
            i += 2;
            continue;
        }
        let cp = if c < 0x80 {
            i += 1;
            u32::from(c)
        } else {
            next_char(b, &mut i)
        };
        let v = enc.lookup_fast(cp)?;
        if v < 0x100 {
            out[o] = v as u8;
            o += 1;
        } else if v < 0x1_0000 {
            out[o] = (v >> 8) as u8;
            out[o + 1] = v as u8;
            o += 2;
        } else {
            let mut tmp = Vec::with_capacity(4);
            push_packed(&mut tmp, v);
            out[o..o + tmp.len()].copy_from_slice(&tmp);
            o += tmp.len();
        }
    }
    Some(finish(out, o))
}

/// Writes the UTF-8 form of `cp` (below U+110000) at `out[o..]`.
#[inline(always)]
fn put_utf8(out: &mut [u8], o: &mut usize, cp: u32) {
    if cp < 0x80 {
        out[*o] = cp as u8;
        *o += 1;
    } else if cp < 0x800 {
        out[*o] = 0xC0 | (cp >> 6) as u8;
        out[*o + 1] = 0x80 | (cp & 0x3F) as u8;
        *o += 2;
    } else if cp < 0x1_0000 {
        out[*o] = 0xE0 | (cp >> 12) as u8;
        out[*o + 1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[*o + 2] = 0x80 | (cp & 0x3F) as u8;
        *o += 3;
    } else {
        out[*o] = 0xF0 | (cp >> 18) as u8;
        out[*o + 1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
        out[*o + 2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[*o + 3] = 0x80 | (cp & 0x3F) as u8;
        *o += 4;
    }
}

/// An ASCII-transparent table charset (EUC-*, Shift_JIS-like tables without
/// pairs or compositions) to UTF-8.
fn table_to_utf8(t: &'static Table, b: &[u8]) -> Option<Vec<u8>> {
    // At most four output bytes per input byte (a one-byte sequence to a
    // supplementary character).
    let mut out = vec![0u8; b.len() * 3 + 4];
    let mut o = 0;
    let mut i = 0;
    let root = t.root();
    while i < b.len() {
        if o + 4 > out.len() {
            out.resize(out.len() * 2, 0);
        }
        let c = b[i];
        if c < 0x80 {
            let n = ascii_run(b, i);
            out[o..o + n].copy_from_slice(&b[i..i + n]);
            o += n;
            i += n;
            continue;
        }
        let mut e = root[usize::from(c)];
        let mut k = 1;
        while e & NODE_FLAG != 0 {
            let &nb = b.get(i + k)?;
            e = t.entry_at((e & !NODE_FLAG) as usize, nb);
            k += 1;
        }
        if e < CHAR_BASE || e & MULTI_FLAG != 0 {
            return None;
        }
        put_utf8(&mut out, &mut o, e - CHAR_BASE);
        i += k;
    }
    Some(finish(out, o))
}

/// Cuts the output buffer to its length, giving memory back when much of
/// it is unused.
fn finish(mut out: Vec<u8>, len: usize) -> Vec<u8> {
    out.truncate(len);
    if out.capacity() > len + len / 2 + 64 {
        out.shrink_to_fit();
    }
    out
}
