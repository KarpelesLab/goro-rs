//! UTF-7 (RFC 2152) and UTF7-IMAP (RFC 3501's modified UTF-7).
//!
//! Error handling inside Base64 sections (leftover bits, lone surrogates,
//! unterminated sections) and what `mb_check_encoding` rejects follow test
//! ext/mbstring/tests/utf_encodings.phpt, utf7imap_encoding.phpt and the
//! observations of tools/charset/fixtures/mb_stateful.php.

use super::{Decode, ERR, Encode, Subst, fail};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[inline]
fn b64_value(b: u8, imap: bool) -> Option<u32> {
    Some(match b {
        b'A'..=b'Z' => (b - b'A') as u32,
        b'a'..=b'z' => (b - b'a') as u32 + 26,
        b'0'..=b'9' => (b - b'0') as u32 + 52,
        b'+' => 62,
        b'/' if !imap => 63,
        b',' if imap => 63,
        _ => return None,
    })
}

/// Characters UTF-7 writes directly (RFC 2152 set D, and space, tab, CR,
/// LF and NUL); everything else goes into Base64 (test
/// ext/mbstring/tests/utf_encodings.phpt).
#[inline]
fn utf7_direct(c: u32) -> bool {
    matches!(c, 0x41..=0x5A | 0x61..=0x7A | 0x30..=0x39)
        || matches!(
            c,
            0x27 | 0x28
                | 0x29
                | 0x2C
                | 0x2D
                | 0x2E
                | 0x2F
                | 0x3A
                | 0x3F
                | 0x20
                | 0x09
                | 0x0D
                | 0x0A
                | 0x00
        )
}

/// Bytes `mb_check_encoding` accepts outside Base64: the direct and the
/// optional direct characters of RFC 2152 (observed on PHP 8.5).
#[inline]
fn utf7_check_ok(b: u8) -> bool {
    matches!(b, 0x20..=0x7D) && b != b'\\' || matches!(b, 0x00 | 0x09 | 0x0A | 0x0D)
}

pub(crate) struct Utf7Dec {
    imap: bool,
    b64: bool,
    bits: u32,
    nbits: u32,
    /// A high surrogate waiting for its low half.
    high: Option<u32>,
    /// Base64 characters read in the current section.
    nchars: u32,
    bad: bool,
}

impl Utf7Dec {
    pub(crate) fn new(imap: bool) -> Self {
        Utf7Dec {
            imap,
            b64: false,
            bits: 0,
            nbits: 0,
            high: None,
            nchars: 0,
            bad: false,
        }
    }

    fn unit(&mut self, u: u32, out: &mut Vec<u32>) {
        if let Some(h) = self.high.take() {
            if (0xDC00..=0xDFFF).contains(&u) {
                out.push(0x10000 + ((h - 0xD800) << 10) + (u - 0xDC00));
                return;
            }
            out.push(ERR);
        }
        if (0xD800..=0xDBFF).contains(&u) {
            self.high = Some(u);
        } else if (0xDC00..=0xDFFF).contains(&u) {
            out.push(ERR);
        } else if self.imap && (0x20..=0x7E).contains(&u) && u != 0x26 {
            // UTF7-IMAP must not Base64-encode printable ASCII other than
            // '&' (test ext/mbstring/tests/utf7imap_encoding.phpt).
            out.push(ERR);
        } else {
            out.push(u);
        }
    }

    /// Ends a Base64 section: a pending high surrogate, a useless sextet or
    /// non-zero padding bits are errors.
    /// `clean` is false when the section ends some other way than with
    /// '-' (end of string, another character).
    fn end_section(&mut self, out: &mut Vec<u32>, eof: bool, clean: bool) {
        let leftover = self.nbits >= 6 || self.bits != 0;
        // A pending high surrogate and bad leftover bits make one error. In
        // UTF7-IMAP a badly ended section is one error by itself, plus a
        // pending high surrogate unless the string ended on a unit boundary
        // (test utf7imap_encoding.phpt, observed on PHP 8.5).
        if self.imap && !clean {
            // At the end of the string, with a whole sextet or more pending
            // the unterminated section is the only error; otherwise a high
            // surrogate (unless on a unit boundary) and non-zero padding are
            // errors too (observed on PHP 8.5).
            if eof && self.nbits < 6 {
                if self.high.is_some() && self.nbits != 0 {
                    out.push(ERR);
                }
                if self.bits != 0 {
                    out.push(ERR);
                }
            }
            // (the caller adds the error for the bad end itself)
        } else if self.high.is_some() || leftover {
            out.push(ERR);
        }
        self.high = None;
        if self.nchars == 0 && !self.imap && !eof {
            self.bad = true;
        }
        self.b64 = false;
        self.bits = 0;
        self.nbits = 0;
        self.nchars = 0;
    }
}

impl Decode for Utf7Dec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        let shift = if self.imap { b'&' } else { b'+' };
        while i < n && out.len() < limit {
            let b = s[i];
            if self.b64 {
                if let Some(v) = b64_value(b, self.imap) {
                    self.bits = (self.bits << 6) | v;
                    self.nbits += 6;
                    self.nchars += 1;
                    if self.nbits >= 16 {
                        self.nbits -= 16;
                        let u = (self.bits >> self.nbits) & 0xFFFF;
                        self.bits &= (1 << self.nbits) - 1;
                        self.unit(u, out);
                    }
                    i += 1;
                    continue;
                }
                self.end_section(out, false, b == b'-');
                if b == b'-' {
                    i += 1;
                } else if self.imap {
                    // A UTF7-IMAP section must end with '-'.
                    out.push(ERR);
                    i += 1;
                }
                continue;
            }
            if b == shift {
                if i + 1 < n && s[i + 1] == b'-' {
                    out.push(shift as u32);
                    i += 2;
                } else {
                    self.b64 = true;
                    i += 1;
                }
                continue;
            }
            i += 1;
            if self.imap {
                out.push(if (0x20..=0x7E).contains(&b) {
                    b as u32
                } else {
                    ERR
                });
            } else if b < 0x80 {
                if !utf7_check_ok(b) {
                    self.bad = true;
                }
                out.push(b as u32);
            } else {
                out.push(ERR);
            }
        }
        *pos = i;
    }

    fn finish(&mut self, out: &mut Vec<u32>) {
        if self.b64 {
            self.end_section(out, true, false);
            if self.imap {
                // Unterminated section.
                out.push(ERR);
            }
        }
    }

    fn check_failed(&self) -> bool {
        self.bad
    }
}

pub(crate) struct Utf7Enc {
    imap: bool,
    b64: bool,
    bits: u32,
    nbits: u32,
}

impl Utf7Enc {
    pub(crate) fn new(imap: bool) -> Self {
        Utf7Enc {
            imap,
            b64: false,
            bits: 0,
            nbits: 0,
        }
    }

    #[inline]
    fn sextet(&self, v: u32) -> u8 {
        let c = B64[v as usize];
        if self.imap && c == b'/' { b',' } else { c }
    }

    fn unit(&mut self, u: u32, out: &mut Vec<u8>) {
        self.bits = (self.bits << 16) | u;
        self.nbits += 16;
        while self.nbits >= 6 {
            self.nbits -= 6;
            out.push(self.sextet((self.bits >> self.nbits) & 0x3F));
        }
        self.bits &= (1 << self.nbits) - 1;
    }

    /// Writes the leftover bits of a Base64 section.
    fn flush_bits(&mut self, out: &mut Vec<u8>) {
        if self.nbits > 0 {
            out.push(self.sextet((self.bits << (6 - self.nbits)) & 0x3F));
        }
        self.bits = 0;
        self.nbits = 0;
    }

    fn direct(&mut self, c: u8, out: &mut Vec<u8>) {
        if self.b64 {
            self.flush_bits(out);
            // The '-' is written when the next character could be read as
            // Base64 or as the terminator, and before NUL; UTF7-IMAP always
            // writes it (observed on PHP 8.5).
            if self.imap || b64_value(c, false).is_some() || c == b'-' || c == 0 {
                out.push(b'-');
            }
            self.b64 = false;
        }
        out.push(c);
    }
}

impl Encode for Utf7Enc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp > 0x10FFFF {
            return false;
        }
        if self.imap {
            if cp == b'&' as u32 {
                self.direct(b'&', out);
                out.push(b'-');
                return true;
            }
            if (0x20..=0x7E).contains(&cp) {
                self.direct(cp as u8, out);
                return true;
            }
        } else if utf7_direct(cp) {
            self.direct(cp as u8, out);
            return true;
        }
        if !self.b64 {
            out.push(if self.imap { b'&' } else { b'+' });
            self.b64 = true;
        }
        if cp >= 0x10000 {
            let c = cp - 0x10000;
            self.unit(0xD800 | (c >> 10), out);
            self.unit(0xDC00 | (c & 0x3FF), out);
        } else {
            self.unit(cp, out);
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
        if self.b64 {
            self.flush_bits(out);
            out.push(b'-');
            self.b64 = false;
        }
    }
}
