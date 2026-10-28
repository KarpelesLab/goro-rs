//! UTF-7 (RFC 2152) and the IMAP variant (RFC 3501 section 5.1.3), with the
//! details of glibc's iconv observed on PHP 8.5 (glibc 2.43):
//!
//! - UTF-7 writes only RFC 2152's set D, space, tab, CR and LF directly; every
//!   other character (set O included) goes in base64, '+' alone becomes "+-".
//!   A base64 run is closed with '-' only before a base64 character, '-' or the
//!   end of the input.
//! - UTF-7-IMAP writes 0x20-0x7E directly ('&' as "&-"), uses ',' for '/',
//!   and always closes a run with '-'.
//! - Decoding accepts the direct characters above plus RFC 2152's set O
//!   except '\' and '~'; closing a run ('-' or a direct character) with more
//!   than five or non-zero leftover bits, or with a lone surrogate, is
//!   illegal. The end of the input closes a run without checks, except right
//!   after the shift character, which is incomplete.

use super::convert::DecodeStatus;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[inline]
fn b64_value(b: u8, imap: bool) -> Option<u32> {
    Some(match b {
        b'A'..=b'Z' => u32::from(b - b'A'),
        b'a'..=b'z' => u32::from(b - b'a') + 26,
        b'0'..=b'9' => u32::from(b - b'0') + 52,
        b'+' => 62,
        b'/' if !imap => 63,
        b',' if imap => 63,
        _ => return None,
    })
}

/// Bytes UTF-7 decodes as themselves outside base64.
#[inline]
fn direct_in(b: u8, imap: bool) -> bool {
    if imap {
        (0x20..=0x7E).contains(&b) && b != b'&'
    } else {
        matches!(b, 0x09 | 0x0A | 0x0D | 0x20..=0x2A | 0x2C..=0x5B | 0x5D..=0x7D)
    }
}

/// Characters UTF-7 writes as themselves.
#[inline]
fn direct_out(cp: u32, imap: bool) -> bool {
    if imap {
        (0x20..=0x7E).contains(&cp) && cp != u32::from(b'&')
    } else {
        matches!(cp, 0x09 | 0x0A | 0x0D | 0x20 | 0x27..=0x29 | 0x2C..=0x3A | 0x3F | 0x41..=0x5A | 0x61..=0x7A)
    }
}

pub(crate) struct Utf7Decoder {
    imap: bool,
    in_b64: bool,
    /// No base64 character since the shift character.
    fresh: bool,
    bits: u32,
    nbits: u32,
    high: Option<u32>,
}

impl Utf7Decoder {
    pub(crate) fn new(imap: bool) -> Self {
        Utf7Decoder {
            imap,
            in_b64: false,
            fresh: false,
            bits: 0,
            nbits: 0,
            high: None,
        }
    }

    fn close_ok(&self) -> bool {
        self.nbits < 6 && self.bits & ((1 << self.nbits) - 1) == 0 && self.high.is_none()
    }

    fn leave(&mut self) {
        self.in_b64 = false;
        self.bits = 0;
        self.nbits = 0;
        self.high = None;
    }

    pub(crate) fn decode(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        limit: usize,
    ) -> DecodeStatus {
        let shift = if self.imap { b'&' } else { b'+' };
        while *pos < input.len() && out.len() < limit {
            let b = input[*pos];
            if self.in_b64 {
                if let Some(v) = b64_value(b, self.imap) {
                    self.fresh = false;
                    self.bits = (self.bits << 6) | v;
                    self.nbits += 6;
                    if self.nbits >= 16 {
                        self.nbits -= 16;
                        let u = (self.bits >> self.nbits) & 0xFFFF;
                        self.bits &= (1 << self.nbits) - 1;
                        if (0xD800..0xDC00).contains(&u) {
                            if self.high.is_some() {
                                self.leave();
                                return DecodeStatus::Illegal(1);
                            }
                            self.high = Some(u);
                        } else if (0xDC00..0xE000).contains(&u) {
                            match self.high.take() {
                                Some(h) => out.push(0x1_0000 + ((h - 0xD800) << 10) + (u - 0xDC00)),
                                None => {
                                    self.leave();
                                    return DecodeStatus::Illegal(1);
                                }
                            }
                        } else {
                            if self.high.is_some() {
                                self.leave();
                                return DecodeStatus::Illegal(1);
                            }
                            out.push(u);
                        }
                    }
                    *pos += 1;
                    continue;
                }
                if b == b'-' {
                    if self.fresh {
                        out.push(u32::from(shift));
                    } else if !self.close_ok() {
                        self.leave();
                        return DecodeStatus::Illegal(1);
                    }
                    self.leave();
                    *pos += 1;
                    continue;
                }
                // Any other byte ends the run; in UTF-7-IMAP it is illegal.
                let ok = !self.imap && self.close_ok() && direct_in(b, false);
                self.leave();
                if !ok {
                    return DecodeStatus::Illegal(1);
                }
                out.push(u32::from(b));
                *pos += 1;
                continue;
            }
            if b == shift {
                self.in_b64 = true;
                self.fresh = true;
                *pos += 1;
                continue;
            }
            if !direct_in(b, self.imap) {
                return DecodeStatus::Illegal(1);
            }
            out.push(u32::from(b));
            *pos += 1;
        }
        DecodeStatus::Ok
    }

    /// End of input: right after the shift character is incomplete.
    pub(crate) fn finish(&mut self) -> DecodeStatus {
        if self.in_b64 && self.fresh {
            return DecodeStatus::Incomplete;
        }
        DecodeStatus::Ok
    }
}

pub(crate) struct Utf7Encoder {
    imap: bool,
    in_b64: bool,
    bits: u32,
    nbits: u32,
}

impl Utf7Encoder {
    pub(crate) fn new(imap: bool) -> Self {
        Utf7Encoder {
            imap,
            in_b64: false,
            bits: 0,
            nbits: 0,
        }
    }

    pub(crate) fn can_encode(cp: u32) -> bool {
        cp <= 0x10_FFFF
    }

    fn put(&mut self, unit: u32, out: &mut Vec<u8>) {
        self.bits = (self.bits << 16) | unit;
        self.nbits += 16;
        while self.nbits >= 6 {
            self.nbits -= 6;
            out.push(self.sym((self.bits >> self.nbits) & 0x3F));
        }
        self.bits &= (1 << self.nbits) - 1;
    }

    fn sym(&self, v: u32) -> u8 {
        if self.imap && v == 63 {
            b','
        } else {
            B64[v as usize]
        }
    }

    fn flush_bits(&mut self, out: &mut Vec<u8>) {
        if self.nbits > 0 {
            out.push(self.sym((self.bits << (6 - self.nbits)) & 0x3F));
        }
        self.bits = 0;
        self.nbits = 0;
    }

    pub(crate) fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>) -> Result<(), usize> {
        let shift = if self.imap { b'&' } else { b'+' };
        for (i, &cp) in cps.iter().enumerate() {
            if cp > 0x10_FFFF {
                return Err(i);
            }
            if direct_out(cp, self.imap) {
                if self.in_b64 {
                    self.flush_bits(out);
                    let b = cp as u8;
                    if self.imap || b == b'-' || b64_value(b, false).is_some() {
                        out.push(b'-');
                    }
                    self.in_b64 = false;
                }
                out.push(cp as u8);
                continue;
            }
            if !self.in_b64 && cp == u32::from(shift) {
                out.extend_from_slice(&[shift, b'-']);
                continue;
            }
            if !self.in_b64 {
                out.push(shift);
                self.in_b64 = true;
            }
            if cp >= 0x1_0000 {
                let c = cp - 0x1_0000;
                self.put(0xD800 | (c >> 10), out);
                self.put(0xDC00 | (c & 0x3FF), out);
            } else {
                self.put(cp, out);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) {
        if self.in_b64 {
            self.flush_bits(out);
            out.push(b'-');
            self.in_b64 = false;
        }
    }
}
