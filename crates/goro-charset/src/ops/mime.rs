//! `mb_encode_mimeheader()` and `mb_decode_mimeheader()`: RFC 2047
//! encoded-words.
//!
//! Both follow the observable behaviour of PHP 8.5 (tests
//! ext/mbstring/tests/mb_encode_mimeheader_*.phpt and
//! mb_decode_mimeheader_*.phpt, plus the layout cases of
//! tools/charset/fixtures/mb_mime.php).

use super::MbError;
use crate::codec::transform;
use crate::{Encoding, Subst, convert};

/// The transfer encoding of encoded-words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// `B` (Base64)
    Base64,
    /// `Q` (Quoted-Printable)
    QuotedPrintable,
}

impl Transfer {
    /// From the `$transfer_encoding` argument (anything but `q`/`Q` is `B`).
    pub fn from_php(s: &[u8]) -> Transfer {
        match s.first() {
            Some(b'Q' | b'q') => Transfer::QuotedPrintable,
            _ => Transfer::Base64,
        }
    }
}

/// The longest line of plain words.
const LINE_MAX: usize = 74;

/// Whether a character can stay as it is in a plain header word.
fn plain_ok(c: u32) -> bool {
    (0x21..0x7F).contains(&c) && !matches!(c, 0x3D | 0x3F | 0x5F)
}

/// Q encoding: letters and `! * + - /` as they are, everything else
/// (digits too) escaped (observed on PHP 8.5).
fn q_encode(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        if b.is_ascii_alphabetic() || matches!(b, b'!' | b'*' | b'+' | b'-' | b'/') {
            out.push(b);
        } else {
            out.push(b'=');
            out.push(b"0123456789ABCDEF"[(b >> 4) as usize]);
            out.push(b"0123456789ABCDEF"[(b & 15) as usize]);
        }
    }
}

fn transfer_len(t: Transfer, bytes: &[u8]) -> usize {
    match t {
        Transfer::Base64 => bytes.len().div_ceil(3) * 4,
        Transfer::QuotedPrintable => bytes
            .iter()
            .map(|&b| {
                if b.is_ascii_alphabetic() || matches!(b, b'!' | b'*' | b'+' | b'-' | b'/') {
                    1
                } else {
                    3
                }
            })
            .sum(),
    }
}

fn transfer_encode(t: Transfer, bytes: &[u8], out: &mut Vec<u8>) {
    match t {
        Transfer::Base64 => out.extend_from_slice(&transform::base64_encode(bytes)),
        Transfer::QuotedPrintable => q_encode(bytes, out),
    }
}

/// `mb_encode_mimeheader()`. `linefeed` is used up to its first NUL and at
/// most 8 bytes; an indent of 74 or more counts as none.
///
/// The input splits into units: a separating space (except for the first
/// unit), more spaces, and a word. Units of plain words are written as they
/// are, a unit that does not fit on the line (74 columns, its separator not
/// counted) going to a new line, which starts with a space, unless the
/// input is a single unit; a final lone space is dropped. From the first
/// unit with a character that needs encoding, or a word other than the last
/// too long for any line, everything left becomes encoded-words: the
/// separator before it stays plain, the first line takes encoded-words up
/// to 75 columns and the following lines up to 76, and the first one goes
/// to a new line when fewer than 12 columns would be left for its text.
pub fn encode_mimeheader(
    s: &[u8],
    charset: Encoding,
    transfer: Transfer,
    linefeed: &[u8],
    indent: usize,
    internal: Encoding,
) -> Result<Vec<u8>, MbError> {
    if matches!(
        charset,
        Encoding::Base64
            | Encoding::Uuencode
            | Encoding::QuotedPrintable
            | Encoding::HtmlEntities
            | Encoding::SevenBit
            | Encoding::EightBit
    ) || charset.mime_name().is_none()
    {
        return Err(MbError::ValueError(format!(
            "Argument #2 ($charset) \"{}\" cannot be used for MIME header encoding",
            charset.name()
        )));
    }
    let lf: &[u8] = {
        let end = linefeed
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(linefeed.len());
        &linefeed[..end.min(8)]
    };
    let mime = charset.mime_name().unwrap_or(charset.name());
    let prefix = format!(
        "=?{}?{}?",
        mime,
        if transfer == Transfer::Base64 {
            'B'
        } else {
            'Q'
        }
    );
    let cps = convert::decode(s, internal);
    let n = cps.len();
    let mut out = Vec::with_capacity(s.len() + 16);
    let mut col = if indent >= LINE_MAX { 0 } else { indent };
    let mut folded = false;
    // Units: (separator, content start, content end).
    let mut units: Vec<(bool, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        let sep = !units.is_empty() && cps[i] == 0x20;
        if sep {
            i += 1;
        }
        let start = i;
        while i < n && cps[i] == 0x20 {
            i += 1;
        }
        while i < n && cps[i] != 0x20 {
            i += 1;
        }
        units.push((sep, start, i));
    }
    let single = units.len() == 1;
    let mut encode_from = None;
    for (k, &(sep, start, end)) in units.iter().enumerate() {
        let last = k + 1 == units.len();
        if start == end && last {
            break;
        }
        let content = &cps[start..end];
        let w = end - start;
        // A word too long for any line is encoded; for the last one the
        // bound is longer (fitted to observations on PHP 8.5).
        let too_long = if last {
            if out.len() <= 3 {
                out.len() + w >= 90
            } else {
                w >= 91
            }
        } else {
            w >= LINE_MAX
        };
        if !content.iter().all(|&c| c == 0x20 || plain_ok(c)) || (too_long && !single) {
            encode_from = Some((sep, start));
            break;
        }
        if single || col + w.max(1) <= LINE_MAX {
            if sep {
                out.push(b' ');
                col += 1;
            }
            col += w;
        } else {
            out.extend_from_slice(lf);
            out.push(b' ');
            col = 1 + w;
            folded = true;
        }
        out.extend(content.iter().map(|&c| c as u8));
    }
    let Some((sep, start)) = encode_from else {
        return Ok(out);
    };
    let rest = &cps[start..];
    let sub = Subst::default();
    let overhead = prefix.len() + 2;
    let mut limit = if folded { LINE_MAX + 2 } else { LINE_MAX + 1 };
    if col + overhead + 12 > LINE_MAX {
        out.extend_from_slice(lf);
        out.push(b' ');
        col = 1;
        limit = LINE_MAX + 2;
    } else if sep {
        out.push(b' ');
        col += 1;
    }
    let mut k = 0;
    while k < rest.len() {
        if k > 0 {
            out.extend_from_slice(lf);
            out.push(b' ');
            col = 1;
            limit = LINE_MAX + 2;
        }
        // Grow the chunk while the encoded-word fits; it holds at least one
        // character.
        let mut best = k + 1;
        let mut j = k + 2;
        while j <= rest.len() {
            let bytes = convert::encode(&rest[k..j], charset, &sub).bytes;
            if col + overhead + transfer_len(transfer, &bytes) > limit {
                break;
            }
            best = j;
            j += 1;
        }
        let bytes = convert::encode(&rest[k..best], charset, &sub).bytes;
        out.extend_from_slice(prefix.as_bytes());
        transfer_encode(transfer, &bytes, &mut out);
        out.extend_from_slice(b"?=");
        col += overhead + transfer_len(transfer, &bytes);
        k = best;
    }
    Ok(out)
}

/// `mb_decode_mimeheader()`: decodes into `internal`.
///
/// Observed on PHP 8.5 (and tests ext/mbstring/tests/mb_decode_mimeheader_*):
/// text outside encoded-words is read as ASCII (other bytes are errors);
/// a run of line breaks and the whitespace after it becomes one space, or
/// nothing at the end; whitespace between two encoded-words disappears and
/// whitespace after one, before other text, becomes one space; a malformed
/// `=?` is kept as text; an unterminated encoded-word runs to the end. Each
/// encoded-word is decoded as a whole, but a stateful charset keeps its
/// state into the next encoded-word of the same charset.
pub fn decode_mimeheader(s: &[u8], internal: Encoding, sub: &Subst) -> Vec<u8> {
    let mut out: Vec<u32> = Vec::with_capacity(s.len());
    let n = s.len();
    let mut i = 0;
    let mut after_word = false;
    let mut pending = false;
    let mut cur: Option<(Encoding, Box<dyn crate::codec::Decode>)> = None;
    let finish = |cur: &mut Option<(Encoding, Box<dyn crate::codec::Decode>)>,
                  out: &mut Vec<u32>| {
        if let Some((_, mut d)) = cur.take() {
            d.finish(out);
        }
    };
    while i < n {
        let b = s[i];
        if b == b'=' && s.get(i + 1) == Some(&b'?') {
            match parse_word(&s[i..]) {
                Some(w) => {
                    pending = false;
                    if !(after_word && cur.as_ref().is_some_and(|c| c.0 == w.enc)) {
                        finish(&mut cur, &mut out);
                        cur = Some((w.enc, crate::codec::decoder(w.enc)));
                    }
                    let d = &mut cur.as_mut().unwrap().1;
                    decode_body(&w, d.as_mut(), &mut out);
                    after_word = true;
                    i += w.len;
                }
                None => {
                    if after_word && pending {
                        out.push(0x20);
                    }
                    finish(&mut cur, &mut out);
                    after_word = false;
                    pending = false;
                    out.extend([0x3D, 0x3F]);
                    i += 2;
                }
            }
            continue;
        }
        if b == b'\r' || b == b'\n' {
            let mut j = i;
            while j < n && matches!(s[j], b'\r' | b'\n' | b' ' | b'\t') {
                j += 1;
            }
            if j < n {
                if after_word {
                    pending = true;
                } else {
                    out.push(0x20);
                }
            }
            i = j;
            continue;
        }
        if after_word && (b == b' ' || b == b'\t') {
            pending = true;
            i += 1;
            continue;
        }
        if after_word {
            if pending {
                out.push(0x20);
            }
            finish(&mut cur, &mut out);
            after_word = false;
            pending = false;
        }
        out.push(if b < 0x80 { b as u32 } else { crate::ERR });
        i += 1;
    }
    finish(&mut cur, &mut out);
    convert::encode(&out, internal, sub).bytes
}

/// A parsed `=?charset?X?text?=`.
struct Word<'a> {
    enc: Encoding,
    base64: bool,
    body: &'a [u8],
    /// Bytes consumed from the `=?`.
    len: usize,
}

fn parse_word(s: &[u8]) -> Option<Word<'_>> {
    let q1 = 2 + s[2..].iter().position(|&b| b == b'?')?;
    let enc = Encoding::from_name(&s[2..q1])?;
    let base64 = match s.get(q1 + 1)? {
        b'B' | b'b' => true,
        b'Q' | b'q' => false,
        _ => return None,
    };
    if s.get(q1 + 2) != Some(&b'?') {
        return None;
    }
    let start = q1 + 3;
    let (body, len) = match s[start..].windows(2).position(|w| w == b"?=") {
        Some(rel) => (&s[start..start + rel], start + rel + 2),
        // Unterminated: the text runs to the end, less a final '?'.
        None => (
            s[start..].strip_suffix(b"?").unwrap_or(&s[start..]),
            s.len(),
        ),
    };
    Some(Word {
        enc,
        base64,
        body,
        len,
    })
}

/// Decodes an encoded-word's text: the transfer encoding first, then the
/// charset with the run's decoder. An invalid Base64 character is an error
/// where it stands, before the bytes of the group it interrupts.
fn decode_body(w: &Word, d: &mut dyn crate::codec::Decode, out: &mut Vec<u32>) {
    let mut seg: Vec<u8> = Vec::with_capacity(w.body.len());
    let flush = |seg: &mut Vec<u8>, d: &mut dyn crate::codec::Decode, out: &mut Vec<u32>| {
        if matches!(
            w.enc,
            Encoding::Base64 | Encoding::QuotedPrintable | Encoding::Uuencode
        ) {
            out.extend(convert::decode(seg, w.enc));
        } else {
            let mut pos = 0;
            while pos < seg.len() {
                d.decode(seg, &mut pos, out, usize::MAX);
            }
        }
        seg.clear();
    };
    if w.base64 {
        let mut acc = 0u32;
        let mut nb = 0;
        for &c in w.body {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' | b' ' | b'\t' | b'\r' | b'\n' => continue,
                _ => {
                    flush(&mut seg, d, out);
                    out.push(crate::ERR);
                    continue;
                }
            };
            acc = (acc << 6) | v as u32;
            nb += 1;
            if nb == 4 {
                seg.extend([(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
                acc = 0;
                nb = 0;
            }
        }
        match nb {
            2 => seg.push((acc >> 4) as u8),
            3 => seg.extend([(acc >> 10) as u8, (acc >> 2) as u8]),
            _ => {}
        }
    } else {
        let b = w.body;
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c == b'_' {
                seg.push(b' ');
            } else if c == b'=' && i + 2 < b.len() {
                let h = |x: u8| (x as char).to_digit(16);
                match (h(b[i + 1]), h(b[i + 2])) {
                    (Some(a), Some(l)) => seg.push((a * 16 + l) as u8),
                    // An invalid escape leaves the '=' and drops the two
                    // characters after it.
                    _ => seg.push(b'='),
                }
                i += 3;
                continue;
            } else {
                seg.push(c);
            }
            i += 1;
        }
    }
    flush(&mut seg, d, out);
}
