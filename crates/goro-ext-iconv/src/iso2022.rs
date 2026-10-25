//! glibc's ISO-2022-JP-2 (RFC 1554), ISO-2022-JP-3 (JIS X 0213) and
//! ISO-2022-CN / ISO-2022-CN-EXT (RFC 1922) decoders and encoders, with
//! glibc's choices of character sets, its escape sequences and shift
//! states (observed from glibc's conversions).

use crate::charset::{
    self, Dec, Decoder, Encoder, mbfl_decode_char, mbfl_decode_char_glibc, mbfl_encode_char, no_pua,
};
use crate::cjk;
use goro_ext_mbstring::mbfl::EncNo;

/// The 94 (or 94x94) character sets.
pub mod set {
    pub const ASCII: u8 = 0;
    /// JIS X 0201 Roman
    pub const ROMAN: u8 = 1;
    /// JIS X 0201 Katakana
    pub const KANA: u8 = 2;
    pub const JIS0208: u8 = 3;
    pub const JIS0212: u8 = 4;
    pub const GB2312: u8 = 5;
    pub const KSC5601: u8 = 6;
    /// ISO-8859-1 / ISO-8859-7 upper halves (ISO-2022-JP-2's G2).
    pub const LATIN1: u8 = 7;
    pub const GREEK: u8 = 8;
    /// JIS X 0213 plane 1 (ESC $ ( O: without the characters JIS X
    /// 0213:2004 added; ESC $ ( Q: with them), plane 2.
    pub const JIS0213_O: u8 = 9;
    pub const JIS0213_Q: u8 = 10;
    pub const JIS0213_P: u8 = 11;
    pub const IR165: u8 = 12;
    /// CNS 11643 planes 1 to 7: CNS1 + plane - 1.
    pub const CNS1: u8 = 13;
    pub const CNS2: u8 = 14;
}
use set::*;

/// The JIS X 0213:2004 additions to plane 1 (ESC $ ( Q only).
const JIS2004: [u16; 10] = [
    0x2E21, 0x2F7E, 0x4F54, 0x4F7E, 0x7427, 0x7E7A, 0x7E7B, 0x7E7C, 0x7E7D, 0x7E7E,
];

fn greek_high() -> &'static [u16] {
    charset::sbcs_high(b"ISO-8859-7")
}

/// The character(s) of a two-byte (0x21-0x7E) code in a set.
fn dec94(s: u8, b1: u8, b2: u8) -> (u32, u32) {
    let g = |v: Option<u32>| (v.unwrap_or(0), 0);
    match s {
        JIS0208 => g(mbfl_decode_char(EncNo::EucJp, &[b1 | 0x80, b2 | 0x80])),
        JIS0212 => g(mbfl_decode_char_glibc(
            EncNo::EucJp,
            &[0x8F, b1 | 0x80, b2 | 0x80],
        )),
        GB2312 => g(
            mbfl_decode_char_glibc(EncNo::EucCn, &[b1 | 0x80, b2 | 0x80])
                .filter(|&c| !no_pua(EncNo::EucCn, c)),
        ),
        KSC5601 => g(
            mbfl_decode_char_glibc(EncNo::EucKr, &[b1 | 0x80, b2 | 0x80])
                .filter(|&c| !no_pua(EncNo::EucKr, c)),
        ),
        JIS0213_O | JIS0213_Q => cjk::jis_plane1(b1, b2),
        JIS0213_P => (cjk::tables().jis2.get(b1, b2), 0),
        IR165 => (cjk::tables().ir165.get(b1, b2), 0),
        CNS1 => g(mbfl_decode_char_glibc(
            EncNo::EucTw,
            &[b1 | 0x80, b2 | 0x80],
        )),
        CNS2 => g(mbfl_decode_char_glibc(
            EncNo::EucTw,
            &[0x8E, 0xA2, b1 | 0x80, b2 | 0x80],
        )),
        _ if s > CNS2 => (cjk::cns_high(s - CNS1 + 1, b1, b2), 0),
        _ => (0, 0),
    }
}

/// Whether a first byte alone at the end of the input is an incomplete
/// character of a set (glibc checks the row first in some sets).
fn lead_ok(s: u8, b: u8) -> bool {
    match s {
        JIS0212 => (0x22..=0x6D).contains(&b),
        GB2312 => (0x21..=0x77).contains(&b),
        CNS1 => (0x21..=0x7D).contains(&b),
        _ => true,
    }
}

/// The two-byte code (0x2121-0x7E7E) of `cp` in a set.
fn enc94(s: u8, cp: u32) -> Option<u16> {
    if cp < 0x80 {
        return None;
    }
    let two = |b: Option<Vec<u8>>, lead: &[u8]| -> Option<u16> {
        let b = b?;
        if b.len() != lead.len() + 2 || &b[..lead.len()] != lead {
            return None;
        }
        let (h, l) = (b[lead.len()], b[lead.len() + 1]);
        ((0xA1..=0xFE).contains(&h) && (0xA1..=0xFE).contains(&l))
            .then_some(((h & 0x7F) as u16) << 8 | (l & 0x7F) as u16)
    };
    match s {
        JIS0208 => two(mbfl_encode_char(EncNo::EucJp, cp), &[]),
        JIS0212 => two(mbfl_encode_char(EncNo::EucJp, cp), &[0x8F]),
        GB2312 => two(mbfl_encode_char(EncNo::EucCn, cp), &[]),
        // (KS X 1001:2002's won sign is EUC-KR's only)
        KSC5601 if cp == 0x20A9 => None,
        KSC5601 => two(mbfl_encode_char(EncNo::EucKr, cp), &[]),
        JIS0213_O | JIS0213_Q | JIS0213_P => {
            let c = cjk::tables().jis_enc.code(cp, false)?;
            let plane2 = c >= 0x10000;
            let c = (c & 0xFFFF) as u16;
            if !(0x2121..=0x7E7E).contains(&c) || plane2 != (s == JIS0213_P) {
                return None;
            }
            (s != JIS0213_O || !JIS2004.contains(&c)).then_some(c)
        }
        IR165 => cjk::tables().ir165_enc.code(cp, false).map(|c| c as u16),
        CNS1 => two(mbfl_encode_char(EncNo::EucTw, cp), &[]),
        CNS2 => two(mbfl_encode_char(EncNo::EucTw, cp), &[0x8E, 0xA2]),
        _ if s > CNS2 => {
            let c = cjk::cns_high_code(cp)?;
            ((c >> 16) as u8 == s - CNS1 + 1).then_some(c as u16)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ISO-2022-JP-2 / ISO-2022-JP-3 decoders

/// The set an ISO-2022-JP escape sequence designates (G0, or G2 for
/// ESC . A / ESC . F), and its length; None: not an escape of the charset.
fn jp_escape(s: &[u8], jp3: bool) -> Option<(u8, bool, usize)> {
    Some(match (s.get(1)?, s.get(2)?) {
        (b'(', b'B') => (ASCII, false, 3),
        (b'(', b'J') => (ROMAN, false, 3),
        (b'(', b'I') => (KANA, false, 3),
        (b'$', b'@') | (b'$', b'B') => (JIS0208, false, 3),
        (b'$', b'A') if !jp3 => (GB2312, false, 3),
        (b'.', b'A') if !jp3 => (LATIN1, true, 3),
        (b'.', b'F') if !jp3 => (GREEK, true, 3),
        (b'$', b'(') => match (s.get(3)?, jp3) {
            (b'C', false) => (KSC5601, false, 4),
            (b'D', false) => (JIS0212, false, 4),
            (b'O', true) => (JIS0213_O, false, 4),
            (b'Q', true) => (JIS0213_Q, false, 4),
            (b'P', true) => (JIS0213_P, false, 4),
            _ => return None,
        },
        _ => return None,
    })
}

/// Bytes an escape sequence may still need: ESC ... at the end of the
/// input.
fn escape_incomplete(s: &[u8]) -> bool {
    s.len() < 3 || (s.len() < 4 && s[1] == b'$' && s[2] == b'(')
}

fn decode_jp(d: &mut Decoder, s: &[u8], jp3: bool) -> cjk::Decoded {
    let st = d.iso_state();
    let g0 = (*st & 0xFF) as u8;
    let g2 = (*st >> 8 & 0xFF) as u8;
    let c = s[0];
    if c == 0x1B {
        if escape_incomplete(s) {
            return (Dec::Incomplete, 0);
        }
        if !jp3 && s[1] == b'N' {
            // single shift: one G2 character
            let b = s[2];
            let cp = match g2 {
                // (any byte, high bit set or not)
                LATIN1 => (b | 0x80) as u32,
                GREEK if (0x20..=0x7E).contains(&b) => greek_high()[b as usize] as u32,
                _ => 0xFFFF,
            };
            if cp == 0xFFFF {
                return (Dec::Illegal(1), 0);
            }
            return (Dec::Char(cp, 3), 0);
        }
        return match jp_escape(s, jp3) {
            Some((set, true, n)) => {
                *st = (*st & !0xFF00) | (set as u32) << 8;
                (Dec::Shift(n), 0)
            }
            Some((set, false, n)) => {
                *st = (*st & !0xFF) | set as u32;
                (Dec::Shift(n), 0)
            }
            None => (Dec::Char(0x1B, 1), 0),
        };
    }
    if c >= 0x80 {
        return (Dec::Illegal(1), 0);
    }
    match g0 {
        ASCII => (Dec::Char(c as u32, 1), 0),
        ROMAN => (
            Dec::Char(
                match c {
                    0x5C => 0xA5,
                    0x7E => 0x203E,
                    _ => c as u32,
                },
                1,
            ),
            0,
        ),
        KANA => {
            if (0x21..=0x5F).contains(&c) {
                (Dec::Char(0xFF61 + (c - 0x21) as u32, 1), 0)
            } else if c < 0x21 || c == 0x7F {
                (Dec::Char(c as u32, 1), 0)
            } else {
                (Dec::Illegal(1), 0)
            }
        }
        _ => {
            if c < 0x21 || c == 0x7F {
                // control characters pass through
                return (Dec::Char(c as u32, 1), 0);
            }
            if s.len() < 2 {
                return (
                    if lead_ok(g0, c) {
                        Dec::Incomplete
                    } else {
                        Dec::Illegal(1)
                    },
                    0,
                );
            }
            if !(0x21..=0x7E).contains(&s[1]) {
                return (Dec::Illegal(1), 0);
            }
            match dec94(g0, c, s[1]) {
                (0, _) => (Dec::Illegal(1), 0),
                (a, b) => (Dec::Char(a, 2), b),
            }
        }
    }
}

pub fn decode_jp2(d: &mut Decoder, s: &[u8]) -> cjk::Decoded {
    decode_jp(d, s, false)
}

pub fn decode_jp3(d: &mut Decoder, s: &[u8]) -> cjk::Decoded {
    decode_jp(d, s, true)
}

// ---------------------------------------------------------------------------
// ISO-2022-JP-2 encoder

fn designation(set: u8) -> &'static [u8] {
    match set {
        ASCII => b"\x1b(B",
        ROMAN => b"\x1b(J",
        KANA => b"\x1b(I",
        JIS0208 => b"\x1b$B",
        JIS0212 => b"\x1b$(D",
        GB2312 => b"\x1b$A",
        KSC5601 => b"\x1b$(C",
        LATIN1 => b"\x1b.A",
        GREEK => b"\x1b.F",
        JIS0213_O => b"\x1b$(O",
        JIS0213_Q => b"\x1b$(Q",
        _ => b"\x1b$(P",
    }
}

/// The code of `cp` in a G0 set (one byte for the 94 sets).
fn g0_code(set: u8, cp: u32) -> Option<u16> {
    match set {
        ASCII => (cp < 0x80).then_some(cp as u16),
        ROMAN => match cp {
            0xA5 => Some(0x5C),
            0x203E => Some(0x7E),
            0x21..=0x7D if cp != 0x5C => Some(cp as u16),
            _ => None,
        },
        KANA => (0xFF61..=0xFF9F)
            .contains(&cp)
            .then(|| (cp - 0xFF61 + 0x21) as u16),
        _ => enc94(set, cp),
    }
}

/// The G2 byte (after ESC N) of `cp`.
fn g2_code(set: u8, cp: u32) -> Option<u8> {
    match set {
        LATIN1 => (0x80..=0xFF).contains(&cp).then(|| (cp - 0x80) as u8),
        GREEK => {
            let t = greek_high();
            (0xA0..0xFFFF)
                .contains(&cp)
                .then(|| t.iter().skip(0x20).position(|&c| c as u32 == cp))
                .flatten()
                .map(|i| (i + 0x20) as u8)
        }
        _ => None,
    }
}

/// ISO-2022-JP-2's language tag (Unicode tag characters) states, in the
/// encoder's `acc` bits 8-31: the language (0 none / ja, 1 ko, 2 zh), the
/// tag parse state and its first letter.
const LANG_KO: u32 = 1;
const LANG_ZH: u32 = 2;

pub fn encode_jp2(e: &mut Encoder, cp: u32, out: &mut Vec<u8>) -> bool {
    let (g0, acc, _) = e.iso_state();
    // acc: bits 0-7 G2, 8-15 language, 16-23 tag parse state (1: after
    // TAG LANGUAGE, 2: after its first letter), 24-31 that letter
    if (0xE0000..=0xE007F).contains(&cp) {
        let parse = *acc >> 16 & 0xFF;
        match cp {
            0xE0001 => *acc = (*acc & 0xFFFF) | 1 << 16,
            0xE007F => *acc &= 0xFF,
            _ if parse == 1 => *acc = (*acc & 0xFFFF) | 2 << 16 | (cp & 0x7F) << 24,
            _ if parse == 2 => {
                let a = (*acc >> 24) as u8;
                let lang = match [
                    a.to_ascii_lowercase(),
                    (cp as u8 & 0x7F).to_ascii_lowercase(),
                ] {
                    [b'k', b'o'] => LANG_KO,
                    [b'z', b'h'] => LANG_ZH,
                    _ => 0,
                };
                *acc = (*acc & 0xFF) | lang << 8;
            }
            _ => {}
        }
        return true;
    }
    if *acc >> 16 & 0xFF != 0 {
        // (a tag cut short sets no language)
        *acc &= 0xFF;
    }
    let g2 = (*acc & 0xFF) as u8;
    if cp == 0x7E && *g0 == JIS0212 {
        // (glibc's JIS X 0212 encodes the tilde, 0x2237)
        out.extend_from_slice(b"\x22\x37");
        return true;
    }
    if cp < 0x80 {
        if !(*g0 == ASCII || (*g0 == ROMAN && g0_code(ROMAN, cp).is_some())) {
            out.extend_from_slice(designation(ASCII));
            *g0 = ASCII;
        }
        out.push(cp as u8);
        if cp == b'\n' as u32 {
            *acc &= !0xFF;
        }
        return true;
    }
    // the sets in use first
    if *g0 != ASCII
        && let Some(c) = g0_code(*g0, cp)
    {
        push_g0(out, *g0, c);
        return true;
    }
    if g2 != 0
        && let Some(b) = g2_code(g2, cp)
    {
        out.extend_from_slice(&[0x1B, b'N', b]);
        return true;
    }
    let lang = *acc >> 8 & 0xFF;
    let order: &[u8] = match lang {
        LANG_KO => &[
            KSC5601, LATIN1, GREEK, ROMAN, JIS0208, JIS0212, GB2312, KANA,
        ],
        LANG_ZH => &[
            GB2312, LATIN1, GREEK, ROMAN, JIS0208, JIS0212, KSC5601, KANA,
        ],
        _ => &[
            ROMAN, JIS0208, JIS0212, LATIN1, GREEK, GB2312, KSC5601, KANA,
        ],
    };
    for &s in order {
        if s == LATIN1 || s == GREEK {
            if let Some(b) = g2_code(s, cp) {
                out.extend_from_slice(designation(s));
                out.extend_from_slice(&[0x1B, b'N', b]);
                *acc = (*acc & !0xFF) | s as u32;
                return true;
            }
        } else if let Some(c) = g0_code(s, cp) {
            out.extend_from_slice(designation(s));
            *g0 = s;
            push_g0(out, s, c);
            return true;
        }
    }
    false
}

fn push_g0(out: &mut Vec<u8>, set: u8, c: u16) {
    if matches!(set, ASCII | ROMAN | KANA) {
        out.push(c as u8);
    } else {
        out.extend_from_slice(&[(c >> 8) as u8, c as u8]);
    }
}

// ---------------------------------------------------------------------------
// ISO-2022-JP-3 encoder (combining: か + U+309A is one JIS X 0213 code)

/// The set and code of one character in ISO-2022-JP-3 from G0 `g0`.
fn jp3_code(g0: u8, cp: u32) -> Option<(u8, u16)> {
    if cp < 0x80 {
        if g0 == ROMAN && g0_code(ROMAN, cp).is_some() {
            return Some((ROMAN, cp as u16));
        }
        return Some((ASCII, cp as u16));
    }
    if g0 != ASCII
        && let Some(c) = g0_code(g0, cp)
    {
        return Some((g0, c));
    }
    for s in [ROMAN, JIS0208, KANA, JIS0213_O, JIS0213_Q, JIS0213_P] {
        if let Some(c) = g0_code(s, cp) {
            return Some((s, c));
        }
    }
    None
}

fn jp3_put(g0: &mut u8, out: &mut Vec<u8>, s: u8, c: u16) {
    if *g0 != s {
        out.extend_from_slice(designation(s));
        *g0 = s;
    }
    push_g0(out, s, c);
}

/// Writes the held back character unless `next` combines with it.
pub fn flush_jp3(e: &mut Encoder, next: Option<u32>, out: &mut Vec<u8>) {
    let enc = &cjk::tables().jis_enc;
    let (g0, _, last) = e.iso_state();
    if *last == 0 || next.is_some_and(|n| enc.compose(*last, n).is_some()) {
        return;
    }
    let base = std::mem::take(last);
    if let Some((s, c)) = jp3_code(*g0, base) {
        jp3_put(g0, out, s, c);
    }
}

pub fn encode_jp3(e: &mut Encoder, cp: u32, out: &mut Vec<u8>) -> bool {
    let enc = &cjk::tables().jis_enc;
    flush_jp3(e, Some(cp), out);
    let (g0, _, last) = e.iso_state();
    if *last != 0 {
        let base = std::mem::take(last);
        let code = enc.compose(base, cp).unwrap_or(0) as u16;
        // (the composed characters are plane 1's; the current set if it
        // is plane 1)
        let s = if *g0 == JIS0213_Q || *g0 == JIS0213_O {
            *g0
        } else {
            JIS0213_O
        };
        jp3_put(g0, out, s, code);
        return true;
    }
    let Some((s, c)) = jp3_code(*g0, cp) else {
        return false;
    };
    if enc.is_base(cp) {
        // (glibc writes the escape sequence of a JIS X 0213 character it
        // holds back at once, that of a JIS X 0208 one with the character)
        if *g0 != s && matches!(s, JIS0213_O | JIS0213_Q | JIS0213_P) {
            out.extend_from_slice(designation(s));
            *g0 = s;
        }
        *last = cp;
        return true;
    }
    jp3_put(g0, out, s, c);
    true
}

/// The bytes back to the initial state: a held back character, ESC ( B.
pub fn reset(e: &mut Encoder, out: &mut Vec<u8>) {
    match e.kind {
        charset::Kind::Iso2022Jp3 => flush_jp3(e, None, out),
        charset::Kind::Iso2022Cn(_) => {
            // (glibc writes SI unless back in the initial state: a
            // designation since the last new line counts too)
            let (cur, ann, _) = e.iso_state();
            if *cur != 0 || *ann != 0 {
                out.push(0x0F);
            }
            return;
        }
        _ => {}
    }
    let (g0, _, _) = e.iso_state();
    if *g0 != ASCII {
        out.extend_from_slice(designation(ASCII));
    }
}

// ---------------------------------------------------------------------------
// ISO-2022-CN / ISO-2022-CN-EXT

/// Decoder state (`acc`): bits 0-7 shifted out (1), 8-15 the SO set
/// (taken when shifting out), 16-23 / 24-27 / 28-31 the G1 / G2 / G3
/// designations (0: none; G2 / G3 hold set - CNS1).
pub fn decode_cn(d: &mut Decoder, s: &[u8], ext: bool) -> cjk::Decoded {
    let st = d.iso_state();
    let so = *st & 0xFF != 0;
    let so_set = (*st >> 8 & 0xFF) as u8;
    let g1 = (*st >> 16 & 0xFF) as u8;
    let g3 = (*st >> 28 & 0xF) as u8;
    let c = s[0];
    match c {
        0x1B => {
            // (glibc looks at each byte of an escape sequence as it comes)
            // (shifted out, ISO-2022-CN-EXT's //IGNORE skips two bytes of
            // a bad sequence)
            let plain = match (so, ext) {
                (false, _) => Dec::Char(0x1B, 1),
                (true, false) => Dec::Illegal(1),
                (true, true) => Dec::Illegal(2),
            };
            let ok = match s.get(1) {
                None | Some(b'N') => true,
                Some(b'O') => ext,
                Some(b'$') => match s.get(2) {
                    None => true,
                    Some(b')') => s
                        .get(3)
                        .is_none_or(|&c| c == b'A' || c == b'G' || (ext && c == b'E')),
                    Some(b'*') => s.get(3).is_none_or(|&c| c == b'H'),
                    Some(b'+') => ext && s.get(3).is_none_or(|c| (b'I'..=b'M').contains(c)),
                    _ => false,
                },
                _ => false,
            };
            if !ok {
                return (plain, 0);
            }
            if s.len() < 2 {
                return (Dec::Incomplete, 0);
            }
            // single shifts: SS2 (G2: CNS plane 2), SS3 (G3: CNS planes 3-7)
            if s[1] == b'N' || (ext && s[1] == b'O') {
                if s.len() < 4 {
                    return (Dec::Incomplete, 0);
                }
                // (SS2 is CNS plane 2 even before its designation; //IGNORE
                // skips the single shift, or the whole sequence in
                // ISO-2022-CN-EXT)
                let bad = (Dec::Illegal(if ext { 4 } else { 2 }), 0);
                let set = if s[1] == b'N' {
                    CNS2
                } else if g3 == 0 {
                    return bad;
                } else {
                    CNS1 + g3
                };
                if !(0x21..=0x7E).contains(&s[2]) || !(0x21..=0x7E).contains(&s[3]) {
                    return bad;
                }
                return match dec94(set, s[2], s[3]) {
                    (0, _) => bad,
                    (a, _) => (Dec::Char(a, 4), 0),
                };
            }
            if s.len() < 4 {
                return (Dec::Incomplete, 0);
            }
            let des = match (s[1], s[2], s[3]) {
                (b'$', b')', b'A') => Some((1, GB2312)),
                (b'$', b')', b'G') => Some((1, CNS1)),
                (b'$', b')', b'E') if ext => Some((1, IR165)),
                (b'$', b'*', b'H') => Some((2, CNS2)),
                (b'$', b'+', b'I'..=b'M') if ext => Some((3, CNS1 + 2 + s[3] - b'I')),
                _ => None,
            };
            match des {
                Some((1, set)) => *st = (*st & !0x00FF_0000) | (set as u32) << 16,
                Some((2, set)) => *st = (*st & !0x0F00_0000) | ((set - CNS1) as u32) << 24,
                Some((_, set)) => *st = (*st & !0xF000_0000) | ((set - CNS1) as u32) << 28,
                None => return (plain, 0),
            }
            (Dec::Shift(4), 0)
        }
        0x0E => {
            let set = if g1 != 0 {
                g1
            } else if ext {
                return (Dec::Illegal(2), 0);
            } else {
                GB2312
            };
            *st = (*st & !0xFFFF) | 1 | (set as u32) << 8;
            (Dec::Shift(1), 0)
        }
        0x0F => {
            *st &= !0xFF;
            (Dec::Shift(1), 0)
        }
        0x80..=0xFF => (Dec::Illegal(1), 0),
        // (ISO-2022-CN takes DEL for no character)
        0x7F if !ext && !so => (Dec::Illegal(1), 0),
        // (a new line keeps the designations)
        _ if !so => (Dec::Char(c as u32, 1), 0),
        _ => {
            // (ISO-2022-CN looks at the first byte before wanting the
            // second, ISO-2022-CN-EXT wants both)
            if s.len() < 2 {
                return (
                    if ext || ((0x21..=0x7E).contains(&c) && lead_ok(so_set, c)) {
                        Dec::Incomplete
                    } else {
                        Dec::Illegal(1)
                    },
                    0,
                );
            }
            let bad = (Dec::Illegal(if ext { 2 } else { 1 }), 0);
            if !(0x21..=0x7E).contains(&c) || !(0x21..=0x7E).contains(&s[1]) {
                return bad;
            }
            match dec94(so_set, c, s[1]) {
                (0, _) => bad,
                (a, _) => (Dec::Char(a, 2), 0),
            }
        }
    }
}

/// Encoder state: `state` the set of the last character (ASCII, a G1
/// set while shifted out, or the single-shifted CNS plane); `acc` bits
/// 0-7 / 8-15 / 16-23 the G1 / G2 / G3 designations.
pub fn encode_cn(e: &mut Encoder, cp: u32, out: &mut Vec<u8>, ext: bool) -> bool {
    let (cur, ann, _) = e.iso_state();
    if cp < 0x80 {
        if *cur != 0 {
            out.push(0x0F);
            *cur = 0;
        }
        out.push(cp as u8);
        if cp == b'\n' as u32 {
            *ann = 0;
        }
        return true;
    }
    // glibc's choice: the SO set while shifted out, then an order that
    // puts CNS 11643 plane 1 first once SO designates it (or ISO-IR-165,
    // when not shifted out)
    let so_set = (*ann & 0xFF) as u8;
    let cns_first = so_set == CNS1 || (so_set == IR165 && !matches!(*cur, GB2312 | IR165 | CNS1));
    let order: &[u8] = match (ext, cns_first) {
        (true, false) => &[
            GB2312,
            CNS2,
            IR165,
            CNS1,
            CNS1 + 2,
            CNS1 + 3,
            CNS1 + 4,
            CNS1 + 5,
            CNS1 + 6,
        ],
        (true, true) => &[
            CNS1,
            CNS2,
            GB2312,
            IR165,
            CNS1 + 2,
            CNS1 + 3,
            CNS1 + 4,
            CNS1 + 5,
            CNS1 + 6,
        ],
        (false, false) => &[GB2312, CNS1, CNS2],
        (false, true) => &[CNS1, CNS2, GB2312],
    };
    // (the set shifted out to comes first, as it is: without looking at
    // the designation, which a character without room may have changed)
    let current = matches!(*cur, GB2312 | IR165 | CNS1).then_some(*cur);
    if let Some(c) = current.and_then(|s| enc94(s, cp)) {
        out.extend_from_slice(&[(c >> 8) as u8, c as u8]);
        return true;
    }
    for &s in order {
        let Some(c) = enc94(s, cp) else { continue };
        let (g, des): (u32, &[u8]) = match s {
            GB2312 => (0, b"\x1b$)A"),
            IR165 => (0, b"\x1b$)E"),
            CNS1 => (0, b"\x1b$)G"),
            CNS2 => (8, b"\x1b$*H"),
            _ => (16, &[0x1B, b'$', b'+', b'I' + (s - CNS1 - 2)]),
        };
        if (*ann >> g & 0xFF) as u8 != s {
            out.extend_from_slice(des);
            *ann = (*ann & !(0xFF << g)) | (s as u32) << g;
        }
        // (glibc shifts out from ASCII only, and writes a single shift
        // when the set differs from the last character's)
        match g {
            0 if *cur == ASCII => out.push(0x0E),
            8 if *cur != s => out.extend_from_slice(b"\x1bN"),
            16 if *cur != s => out.extend_from_slice(b"\x1bO"),
            _ => {}
        }
        *cur = s;
        out.extend_from_slice(&[(c >> 8) as u8, c as u8]);
        return true;
    }
    false
}
