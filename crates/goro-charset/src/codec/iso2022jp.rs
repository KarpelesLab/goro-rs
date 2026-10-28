//! The ISO-2022-JP family: JIS, ISO-2022-JP (RFC 1468), ISO-2022-JP-MS,
//! CP50220/CP50221/CP50222, ISO-2022-JP-2004 and ISO-2022-JP-MOBILE#KDDI.
//!
//! The escape sequences each variant accepts, how invalid and truncated ones
//! are consumed and which character sets back each designation were
//! observed on PHP 8.5 (fixture tools/charset/fixtures/mb_stateful.php) and
//! follow tests ext/mbstring/tests/iso2022jp*_encoding.phpt and
//! cp5022x_encoding.phpt.

use super::{Decode, ERR, Encode, Subst, fail};
use crate::Encoding;
use crate::codec::mobile;
use crate::tables::{jis as t, kana, rev, seq};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flavor {
    Jis,
    Iso,
    Cp50220,
    Cp50221,
    Cp50222,
    Ms,
    Kddi,
    Jp2004,
}

impl Flavor {
    fn of(enc: Encoding) -> Flavor {
        match enc {
            Encoding::Jis => Flavor::Jis,
            Encoding::Iso2022Jp => Flavor::Iso,
            Encoding::Cp50220 => Flavor::Cp50220,
            Encoding::Cp50221 => Flavor::Cp50221,
            Encoding::Cp50222 => Flavor::Cp50222,
            Encoding::Iso2022JpMs => Flavor::Ms,
            Encoding::Iso2022JpKddi => Flavor::Kddi,
            _ => Flavor::Jp2004,
        }
    }

    /// JIS, ISO-2022-JP and CP5022x share the JIS7 decoder: SO/SI shift
    /// kana, and a bad escape costs only its ESC.
    fn jis7(self) -> bool {
        matches!(
            self,
            Flavor::Jis | Flavor::Iso | Flavor::Cp50220 | Flavor::Cp50221 | Flavor::Cp50222
        )
    }

    fn cp5022x(self) -> bool {
        matches!(self, Flavor::Cp50220 | Flavor::Cp50221 | Flavor::Cp50222)
    }
}

/// What G0 holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum G0 {
    Ascii,
    /// JIS X 0201 Roman (`ESC ( J`).
    Roman,
    /// JIS X 0201 katakana (`ESC ( I`).
    Kana,
    X0208,
    X0212,
    /// ISO-2022-JP-MS user-defined characters (`ESC $ ( ?`).
    Udc,
    /// JIS X 0213 plane 1 (`ESC $ ( Q`).
    X0213p1,
    /// JIS X 0213 plane 2 (`ESC $ ( P`).
    X0213p2,
}

/// The escape sequences after ESC, with the set they designate.
fn escapes(f: Flavor) -> &'static [(&'static [u8], G0)] {
    use G0::*;
    match f {
        Flavor::Jis | Flavor::Iso | Flavor::Cp50220 | Flavor::Cp50221 | Flavor::Cp50222 => &[
            (b"$@", X0208),
            (b"$B", X0208),
            (b"$(@", X0208),
            (b"$(B", X0208),
            (b"$(D", X0212),
            (b"(B", Ascii),
            (b"(H", Ascii),
            (b"(J", Roman),
            (b"(I", Kana),
        ],
        Flavor::Ms => &[
            (b"$@", X0208),
            (b"$B", X0208),
            (b"$(?", Udc),
            (b"$(@", X0208),
            (b"$(B", X0208),
            (b"(B", Ascii),
            (b"(I", Kana),
            (b"(J", Roman),
        ],
        Flavor::Kddi => &[
            (b"$@", X0208),
            (b"$B", X0208),
            (b"$(@", X0208),
            (b"$(B", X0208),
            (b"(B", Ascii),
            (b"(I", Kana),
            (b"(J", Roman),
        ],
        Flavor::Jp2004 => &[
            (b"$B", X0208),
            (b"(B", Ascii),
            (b"$(P", X0213p2),
            (b"$(Q", X0213p1),
        ],
    }
}

/// The escapes ISO-2022-JP (RFC 1468) itself defines; the others decode but
/// fail `mb_check_encoding` (test ext/mbstring/tests/iso2022jp_encoding.phpt).
fn rfc1468_escape(e: &[u8]) -> bool {
    matches!(e, b"$@" | b"$B" | b"(B" | b"(J")
}

pub(crate) struct Iso2022JpDec {
    f: Flavor,
    g0: G0,
    /// Shifted out (SO) to kana, JIS7 style.
    so: bool,
    bad: bool,
}

impl Iso2022JpDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        Iso2022JpDec {
            f: Flavor::of(enc),
            g0: G0::Ascii,
            so: false,
            bad: false,
        }
    }

    /// The code point of a two-byte code in the current set, or [`ERR`]
    /// (pushes sequences directly).
    fn lookup(&self, lead: u8, trail: u8, out: &mut Vec<u32>) {
        let row = (lead - 0x21) as usize;
        let idx = row * 94 + (trail - 0x21) as usize;
        let cp: u32 = match (self.g0, self.f) {
            (G0::X0208, Flavor::Jis | Flavor::Iso) => t::JIS0208[idx] as u32,
            (G0::X0208, Flavor::Cp50220 | Flavor::Cp50221 | Flavor::Cp50222) => {
                t::CP5022X[idx] as u32
            }
            (G0::X0208, Flavor::Ms) => {
                if idx < 94 * 94 {
                    t::CP932[idx] as u32
                } else {
                    0
                }
            }
            (G0::X0208, Flavor::Kddi) => {
                if let Some(v) = mobile::kddi_jis_emoji(lead, trail, out) {
                    if v {
                        return;
                    }
                    0
                } else if (12 * 94..13 * 94).contains(&idx) || idx < 84 * 94 {
                    t::CP932[idx] as u32
                } else {
                    0
                }
            }
            (G0::X0208, Flavor::Jp2004) => t::JIS0208[idx] as u32,
            (G0::X0212, _) => t::JIS0212.get(idx).copied().unwrap_or(0) as u32,
            (G0::Udc, _) => {
                if row < 20 {
                    0xE000 + idx as u32
                } else {
                    0
                }
            }
            (G0::X0213p1 | G0::X0213p2, _) => {
                let i = if self.g0 == G0::X0213p2 {
                    94 * 94 + idx
                } else {
                    idx
                };
                let v = t::X0213[i];
                if v != 0 {
                    out.extend_from_slice(seq(&v, &t::X0213_SEQ, &t::X0213_SEQ_OFF));
                    return;
                }
                0
            }
            _ => 0,
        };
        out.push(if cp == 0 { ERR } else { cp });
    }

    /// The lead byte range of the current two-byte set.
    fn is_lead(&self, b: u8) -> bool {
        match self.f {
            // CP5022x takes the CP932 rows past 94 with leads up to 0x97;
            // in JIS X 0212 mode those bytes still take a trail byte and
            // make an error (observed on PHP 8.5).
            Flavor::Cp50220 | Flavor::Cp50221 | Flavor::Cp50222 => (0x21..=0x97).contains(&b),
            Flavor::Ms | Flavor::Kddi => (0x21..=0x7F).contains(&b),
            _ => (0x21..=0x7E).contains(&b),
        }
    }
}

impl Decode for Iso2022JpDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        let f = self.f;
        while i < n && out.len() < limit {
            let b = s[i];
            if b == 0x1B {
                // Match the bytes after ESC against the valid escapes.
                let rest = &s[i + 1..];
                let mut done = false;
                let mut longest = 0;
                for &(e, g) in escapes(f) {
                    if rest.starts_with(e) {
                        self.g0 = g;
                        if f == Flavor::Iso && !rfc1468_escape(e) {
                            self.bad = true;
                        }
                        i += 1 + e.len();
                        done = true;
                        break;
                    }
                    let common = e.iter().zip(rest).take_while(|(a, b)| a == b).count();
                    longest = longest.max(common);
                }
                if done {
                    continue;
                }
                out.push(ERR);
                if 1 + longest >= n - i {
                    // The string ends inside a valid escape.
                    i = n;
                } else if f.jis7() {
                    i += 1;
                } else {
                    i += 2 + longest;
                }
                continue;
            }
            if f.jis7() && (b == 0x0E || b == 0x0F) {
                // A SI that shifts nothing back fails the check in JIS
                // (observed on PHP 8.5); ISO-2022-JP has no shifts at all.
                if f == Flavor::Iso || (f == Flavor::Jis && (b == 0x0F) != self.so) {
                    self.bad = true;
                }
                // SI after SO returns to the designated G0; SI otherwise
                // selects ASCII (observed on PHP 8.5).
                if b == 0x0F && !self.so {
                    self.g0 = G0::Ascii;
                }
                self.so = b == 0x0E;
                i += 1;
                continue;
            }
            if b >= 0x80 {
                if f.cp5022x() && matches!(self.g0, G0::X0208 | G0::X0212) && b <= 0x97 && !self.so {
                    // Rows 95..119 of CP932 (user-defined, IBM).
                } else {
                    i += 1;
                    if (0xA1..=0xDF).contains(&b) && f != Flavor::Jp2004 {
                        if f == Flavor::Iso {
                            self.bad = true;
                        }
                        out.push(0xFF61 + (b - 0xA1) as u32);
                    } else {
                        out.push(ERR);
                    }
                    continue;
                }
            }
            if self.so {
                i += 1;
                out.push(if (0x21..=0x5F).contains(&b) {
                    0xFF61 + (b - 0x21) as u32
                } else {
                    b as u32
                });
                continue;
            }
            match self.g0 {
                G0::Ascii => {
                    out.push(b as u32);
                    i += 1;
                }
                G0::Roman => {
                    out.push(match (b, f.jis7()) {
                        (0x5C, true) => 0xA5,
                        (0x7E, true) => 0x203E,
                        _ => b as u32,
                    });
                    i += 1;
                }
                G0::Kana => {
                    out.push(if (0x21..=0x5F).contains(&b) {
                        0xFF61 + (b - 0x21) as u32
                    } else {
                        b as u32
                    });
                    i += 1;
                }
                _ => {
                    if !self.is_lead(b) {
                        // CP5022x rejects DEL in JIS X 0212 mode (observed
                        // on PHP 8.5).
                        out.push(b as u32);
                        i += 1;
                        continue;
                    }
                    if i + 1 >= n {
                        out.push(ERR);
                        i += 1;
                        continue;
                    }
                    let c = s[i + 1];
                    i += 2;
                    if (0x21..=0x7E).contains(&c) && b != 0x7F || (b == 0x7F && f.cp5022x() && (0x21..=0x7E).contains(&c)) {
                        self.lookup(b, c, out);
                    } else {
                        out.push(ERR);
                    }
                }
            }
        }
        *pos = i;
    }

    fn check_failed(&self) -> bool {
        matches!(self.f, Flavor::Jis | Flavor::Iso)
            && (self.bad || self.so || self.g0 != G0::Ascii)
    }
}

/// What a code point encodes to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Out {
    /// A byte in this set (ASCII, Roman, kana as GL bytes).
    Single(G0, u8),
    /// A two-byte code in this set.
    Double(G0, u8, u8),
    /// A kana in shift-out mode (CP50222).
    Shifted(u8),
}

pub(crate) struct Iso2022JpEnc {
    f: Flavor,
    g0: G0,
    so: bool,
    /// A JIS X 0213 base character, or (CP50220) a halfwidth katakana
    /// converted to fullwidth, held to see whether the next character
    /// combines with it.
    pending: Option<(u32, Out)>,
}

impl Iso2022JpEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        Iso2022JpEnc {
            f: Flavor::of(enc),
            g0: G0::Ascii,
            so: false,
            pending: None,
        }
    }

    fn designate(&mut self, g: G0, out: &mut Vec<u8>) {
        if self.so {
            out.push(0x0F);
            self.so = false;
        }
        if self.g0 == g {
            return;
        }
        self.g0 = g;
        let esc: &[u8] = match g {
            G0::Ascii => b"\x1B(B",
            G0::Roman => b"\x1B(J",
            G0::Kana => b"\x1B(I",
            G0::X0208 => b"\x1B$B",
            G0::X0212 => b"\x1B$(D",
            G0::Udc => b"\x1B$(?",
            G0::X0213p1 => b"\x1B$(Q",
            G0::X0213p2 => b"\x1B$(P",
        };
        out.extend_from_slice(esc);
    }

    fn emit(&mut self, o: Out, out: &mut Vec<u8>) {
        match o {
            Out::Single(g, b) => {
                self.designate(g, out);
                out.push(b);
            }
            Out::Double(g, a, b) => {
                self.designate(g, out);
                out.extend_from_slice(&[a, b]);
            }
            Out::Shifted(b) => {
                if !self.so {
                    // PHP 8.5 counts the SI ending the kana run as a return
                    // to ASCII (observed on PHP 8.5).
                    out.push(0x0E);
                    self.so = true;
                    self.g0 = G0::Ascii;
                }
                out.push(b);
            }
        }
    }

    fn flush(&mut self, out: &mut Vec<u8>) {
        if let Some((_, o)) = self.pending.take() {
            self.emit(o, out);
        }
    }

    fn jis_double(g: G0, idx: usize) -> Out {
        Out::Double(g, (idx / 94) as u8 + 0x21, (idx % 94) as u8 + 0x21)
    }

    fn lookup(&self, cp: u32) -> Option<Out> {
        let f = self.f;
        if cp < 0x80 {
            // ASCII control characters and letters; in JIS X 0201 Roman mode
            // the encoder still switches back to ASCII.
            return Some(Out::Single(G0::Ascii, cp as u8));
        }
        if (0xFF61..=0xFF9F).contains(&cp) {
            let b = (cp - 0xFF61) as u8 + 0x21;
            return match f {
                Flavor::Jis | Flavor::Cp50221 | Flavor::Ms | Flavor::Kddi => {
                    Some(Out::Single(G0::Kana, b))
                }
                Flavor::Cp50222 => Some(Out::Shifted(b)),
                Flavor::Cp50220 => {
                    // Written as the fullwidth katakana; the sound marks as
                    // the spacing U+309B/U+309C (observed on PHP 8.5).
                    let fw = match kana::HW_TO_FW[(cp - 0xFF61) as usize] as u32 {
                        0x3099 => 0x309B,
                        0x309A => 0x309C,
                        c => c,
                    };
                    let i = rev(&t::CP5022X_REV_IDX, &t::CP5022X_REV_DATA, fw);
                    (i != 0).then(|| Self::jis_double(G0::X0208, i as usize - 1))
                }
                Flavor::Iso | Flavor::Jp2004 => None,
            };
        }
        match f {
            Flavor::Jis | Flavor::Iso => {
                match cp {
                    0xA5 => return Some(Out::Single(G0::Roman, 0x5C)),
                    0x203E if f == Flavor::Jis => return Some(Out::Single(G0::Roman, 0x7E)),
                    _ => {}
                }
                let c = match cp {
                    0xFF5E => 0x301C,
                    0x2225 => 0x2016,
                    0xFF0D => 0x2212,
                    0xFFE0 => 0xA2,
                    0xFFE1 => 0xA3,
                    0xFFE2 => 0xAC,
                    0xAF | 0x203E => 0xFFE3,
                    _ => cp,
                };
                if c <= 0xFFFF {
                    let i = rev(&t::JIS0208_REV_IDX, &t::JIS0208_REV_DATA, c);
                    if i != 0 {
                        return Some(Self::jis_double(G0::X0208, i as usize - 1));
                    }
                    if f == Flavor::Jis {
                        let i = rev(&t::JIS0212_REV_IDX, &t::JIS0212_REV_DATA, c);
                        if i != 0 {
                            return Some(Self::jis_double(G0::X0212, i as usize - 1));
                        }
                    }
                }
                None
            }
            Flavor::Cp50220 | Flavor::Cp50221 | Flavor::Cp50222 => {
                match cp {
                    0xA5 => return Some(Out::Single(G0::Roman, 0x5C)),
                    0x203E => return Some(Out::Single(G0::Roman, 0x7E)),
                    _ => {}
                }
                let c = if cp == 0xAF { 0xFFE3 } else { cp };
                if c > 0xFFFF {
                    return None;
                }
                let i = rev(&t::CP5022X_REV_IDX, &t::CP5022X_REV_DATA, c);
                if i == 0 {
                    return None;
                }
                let i = i as usize - 1;
                Some(Out::Double(
                    G0::X0208,
                    (i / 94) as u8 + 0x21,
                    (i % 94) as u8 + 0x21,
                ))
            }
            Flavor::Ms | Flavor::Kddi => {
                if f == Flavor::Kddi
                    && let Some((a, b)) = mobile::kddi_jis_emoji_rev(cp)
                {
                    return Some(Out::Double(G0::X0208, a, b));
                }
                if f == Flavor::Ms && (0xE000..0xE000 + 20 * 94).contains(&cp) {
                    return Some(Self::jis_double(G0::Udc, (cp - 0xE000) as usize));
                }
                let c = match cp {
                    0xA2 => 0xFFE0,
                    0xA3 => 0xFFE1,
                    0xAC => 0xFFE2,
                    0xAF | 0x203E => 0xFFE3,
                    0x2016 => 0x2225,
                    0x2212 => 0xFF0D,
                    0x301C => 0xFF5E,
                    0xA5 => 0xFFE5,
                    _ => cp,
                };
                if c > 0xFFFF {
                    return None;
                }
                let i = rev(&t::CP51932_REV_IDX, &t::CP51932_REV_DATA, c);
                if i == 0 {
                    return None;
                }
                let i = i as usize - 1;
                if f == Flavor::Kddi && !(i < 84 * 94) {
                    return None;
                }
                Some(Self::jis_double(G0::X0208, i))
            }
            Flavor::Jp2004 => {
                match cp {
                    0xA5 => return Some(Out::Single(G0::Ascii, 0x5C)),
                    0x203E => return Some(Out::Single(G0::Ascii, 0x7E)),
                    _ => {}
                }
                let i = rev(&t::X0213_REV_IDX, &t::X0213_REV_DATA, cp);
                if i == 0 {
                    return None;
                }
                let i = i as usize - 1;
                Some(if i >= 94 * 94 {
                    Self::jis_double(G0::X0213p2, i - 94 * 94)
                } else {
                    Self::jis_double(G0::X0213p1, i)
                })
            }
        }
    }
}

/// Whether `cp` starts a JIS X 0213 combining sequence.
fn x0213_base(cp: u32) -> bool {
    t::X0213_COMBOS.binary_search_by_key(&cp, |e| e.0).is_ok()
}

impl Encode for Iso2022JpEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if self.f == Flavor::Kddi {
            // Keycaps and flags: a held digit, '#' or regional indicator
            // (observed on PHP 8.5).
            if let Some((p, o)) = self.pending.take() {
                if let Some((a, b)) = mobile::kddi_jis_pair(p, cp) {
                    self.emit(Out::Double(G0::X0208, a, b), out);
                    return true;
                }
                self.emit(o, out);
            }
            let Some(o) = self.lookup(cp) else {
                return false;
            };
            if mobile::kddi_starts_pair(cp) {
                self.pending = Some((cp, o));
            } else {
                self.emit(o, out);
            }
            return true;
        }
        if let Some((base, _)) = self.pending {
            if self.f == Flavor::Jp2004
                && let Some(e) = t::X0213_COMBOS.iter().find(|e| e.0 == base && e.1 == cp)
            {
                self.pending = None;
                let i = e.2 as usize - 1;
                self.emit(Self::jis_double(G0::X0213p1, i), out);
                return true;
            }
            if self.f == Flavor::Cp50220
                && (cp == 0xFF9E || cp == 0xFF9F)
                && let Some(v) = kana::VOICED.iter().find(|v| {
                    v.0 as u32 == base && v.1 as u32 == if cp == 0xFF9E { 0x3099 } else { 0x309A }
                })
            {
                let i = rev(&t::CP5022X_REV_IDX, &t::CP5022X_REV_DATA, v.2 as u32);
                if i != 0 {
                    self.pending = None;
                    self.emit(Self::jis_double(G0::X0208, i as usize - 1), out);
                    return true;
                }
            }
        }
        let Some(o) = self.lookup(cp) else {
            return false;
        };
        self.flush(out);
        if self.f == Flavor::Jp2004 && x0213_base(cp) {
            self.pending = Some((cp, o));
        } else if self.f == Flavor::Cp50220 && (0xFF61..=0xFF9F).contains(&cp) {
            let fw = kana::HW_TO_FW[(cp - 0xFF61) as usize] as u32;
            if kana::VOICED.iter().any(|v| v.0 as u32 == fw) {
                self.pending = Some((fw, o));
            } else {
                self.emit(o, out);
            }
        } else {
            self.emit(o, out);
        }
        true
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        for &cp in cps {
            if !self.put(cp, out) {
                fail(self, cp, out, sub, errors);
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {
        self.flush(out);
        if self.so {
            out.push(0x0F);
            self.so = false;
        }
        if self.g0 != G0::Ascii {
            out.extend_from_slice(b"\x1B(B");
            self.g0 = G0::Ascii;
        }
    }
}
