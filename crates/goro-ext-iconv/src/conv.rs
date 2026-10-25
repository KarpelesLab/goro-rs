//! An emulation of glibc's iconv(3): conversion descriptors converting
//! character by character, with the errno values php-src's iconv code
//! inspects (EILSEQ, EINVAL, E2BIG) and the //IGNORE and //TRANSLIT
//! suffixes.

use crate::charset::{self, Dec, Decoder, Encoder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    /// Illegal input sequence or unconvertible character.
    Ilseq,
    /// Incomplete input sequence at the end of the input.
    Inval,
    /// No room left in the output buffer.
    TooBig,
}

/// iconv_t
#[derive(Clone, Debug)]
pub struct Iconv {
    dec: Decoder,
    enc: Encoder,
    ignore: bool,
    translit: bool,
    /// //IGNORE skipped something since the last call.
    skipped: bool,
}

/// The charset name of a "CHARSET//SUFFIX,SUFFIX" specification and its
/// suffixes (upper-cased).
fn split_spec(spec: &[u8]) -> (&[u8], Vec<Vec<u8>>) {
    let mut name_end = spec.len();
    if let Some(p) = spec.windows(2).position(|w| w == b"//") {
        name_end = p;
    }
    let name = &spec[..name_end];
    let mut sfx = Vec::new();
    if name_end < spec.len() {
        for part in spec[name_end..].split(|&c| c == b'/' || c == b',') {
            if !part.is_empty() {
                sfx.push(part.to_ascii_uppercase());
            }
        }
    }
    (name, sfx)
}

impl Iconv {
    /// iconv_open(tocode, fromcode): None for an unsupported conversion
    /// (EINVAL).
    pub fn open(to: &[u8], from: &[u8]) -> Option<Iconv> {
        let (to_name, to_sfx) = split_spec(to);
        let (from_name, _) = split_spec(from);
        let to_kind = charset::lookup(to_name)?;
        let from_kind = charset::lookup(from_name)?;
        Some(Iconv {
            dec: Decoder::new(from_kind),
            enc: Encoder::new(to_kind),
            ignore: to_sfx.iter().any(|s| s == b"IGNORE"),
            translit: to_sfx.iter().any(|s| s == b"TRANSLIT"),
            skipped: false,
        })
    }

    /// iconv(cd, &in, &inleft, &out, &outleft): converts `input[*pos..]`
    /// appending to `out`, at most `limit` bytes when given.
    pub fn conv(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u8>,
        limit: Option<usize>,
    ) -> Result<(), Errno> {
        let start_len = out.len();
        let mut tmp = Vec::with_capacity(8);
        while *pos < input.len() {
            match self.dec.decode(&input[*pos..]) {
                Dec::Shift(n) => *pos += n,
                Dec::Incomplete => return Err(Errno::Inval),
                Dec::Illegal(n) => {
                    if self.ignore {
                        *pos += n;
                        self.skipped = true;
                    } else {
                        return Err(Errno::Ilseq);
                    }
                }
                Dec::Char(cp, n) => {
                    tmp.clear();
                    let saved = self.enc;
                    if !self.enc.encode(cp, &mut tmp) {
                        self.enc = saved;
                        tmp.clear();
                        if self.translit && self.transliterate(cp, &mut tmp) {
                            // transliterated
                        } else if self.ignore {
                            self.enc = saved;
                            *pos += n;
                            self.skipped = true;
                            continue;
                        } else {
                            self.enc = saved;
                            return Err(Errno::Ilseq);
                        }
                    }
                    if limit.is_some_and(|l| out.len() - start_len + tmp.len() > l) {
                        self.enc = saved;
                        return Err(Errno::TooBig);
                    }
                    out.extend_from_slice(&tmp);
                    *pos += n;
                }
            }
        }
        if self.skipped {
            self.skipped = false;
            return Err(Errno::Ilseq);
        }
        Ok(())
    }

    /// //TRANSLIT: the first alternative of `cp`'s transliteration that
    /// the target charset can represent, else "?".
    fn transliterate(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        let saved = self.enc;
        if let Some(t) = translit(cp) {
            for alt in t.split('\0') {
                if alt.chars().all(|c| self.enc.encode(c as u32, out)) {
                    return true;
                }
                self.enc = saved;
                out.clear();
            }
        }
        if self.enc.encode('?' as u32, out) {
            return true;
        }
        self.enc = saved;
        out.clear();
        false
    }

    /// iconv(cd, NULL, NULL, &out, &outleft): back to the initial shift
    /// state, writing the sequence that does it.
    pub fn flush(&mut self, out: &mut Vec<u8>, limit: Option<usize>) -> Result<(), Errno> {
        let b = self.enc.reset_bytes();
        if limit.is_some_and(|l| b.len() > l) {
            return Err(Errno::TooBig);
        }
        out.extend_from_slice(&b);
        self.enc.reset();
        self.dec.reset();
        Ok(())
    }

    /// iconv(cd, NULL, NULL, NULL, NULL)
    pub fn reset(&mut self) {
        self.enc.reset();
        self.dec.reset();
    }
}

/// Decode a whole string to code points: the characters before the first
/// error, and the error.
pub fn decode_all(input: &[u8], enc: &[u8]) -> Option<(Vec<u32>, Option<Errno>)> {
    let (name, _) = split_spec(enc);
    let mut dec = Decoder::new(charset::lookup(name)?);
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0;
    while pos < input.len() {
        match dec.decode(&input[pos..]) {
            Dec::Char(cp, n) => {
                out.push(cp);
                pos += n;
            }
            Dec::Shift(n) => pos += n,
            Dec::Illegal(_) => return Some((out, Some(Errno::Ilseq))),
            Dec::Incomplete => return Some((out, Some(Errno::Inval))),
        }
    }
    Some((out, None))
}

/// An encoder for `enc` (UCS-4 → enc conversions of the string functions).
pub fn encoder(enc: &[u8]) -> Option<Encoder> {
    let (name, _) = split_spec(enc);
    Some(Encoder::new(charset::lookup(name)?))
}

/// glibc's transliteration alternatives of a character in the C.UTF-8
/// locale, separated by NUL.
fn translit(cp: u32) -> Option<&'static str> {
    use crate::translit_table::TRANSLIT;
    TRANSLIT
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map(|i| TRANSLIT[i].1)
}
