//! The conversion loop: decode to code points, encode, and handle illegal
//! input and unencodable characters the way glibc's iconv does under PHP.

use super::charset::{Charset, Decoder, Encoder};
use super::names::Flags;
use super::table::CHAR_BASE;
use super::translit;
use super::unicode::ascii_prefix;

/// Result of a decoding step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecodeStatus {
    /// The input (or the output limit) was reached.
    Ok,
    /// An illegal sequence starts at the current position; `//IGNORE` skips
    /// this many bytes.
    Illegal(usize),
    /// Illegal, but `//IGNORE` replaces these bytes with U+FFFD.
    IllegalReplace(usize),
    /// The input ends inside a sequence.
    Incomplete,
}

/// Why a conversion failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertErrorKind {
    /// An illegal input sequence, or a character the output charset can't
    /// represent ("Detected an illegal character in input string").
    IllegalSequence,
    /// The input ends in the middle of a character ("Detected an incomplete
    /// multibyte character in input string").
    IncompleteSequence,
}

/// A failed conversion, with what was converted before the failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvertError {
    pub kind: ConvertErrorKind,
    /// Offset in the input of the faulty sequence (for an unencodable
    /// character, of the block it was decoded in).
    pub offset: usize,
    /// The output produced before the failure.
    pub output: Vec<u8>,
}

const CHUNK: usize = 2048;

/// Converts `input` from one charset to another. `flags` are the output
/// charset's `//TRANSLIT` / `//IGNORE` suffixes.
pub fn convert(
    from: Charset,
    to: Charset,
    flags: Flags,
    input: &[u8],
) -> Result<Vec<u8>, ConvertError> {
    if let Some(out) = super::fast::convert(from, to, input) {
        return Ok(out);
    }
    if let (Some(f), Some(t)) = (from.sbcs_table(), to.sbcs_table())
        && f.ident_upto == 256
        && t.ident_upto == 256
    {
        return Ok(input.to_vec());
    }
    if let Some(t) = from.sbcs_table()
        && to.is_utf8()
    {
        return sbcs_to_utf8(t, input, flags.ignore);
    }
    let mut c = Converter::new(from.decoder(), to.encoder(), flags);
    let mut out = Vec::with_capacity(input.len() + input.len() / 2 + 16);
    c.run(input, &mut out)?;
    Ok(out)
}

/// Single-byte charset to UTF-8 with a 256-entry table of UTF-8 sequences.
fn sbcs_to_utf8(
    t: &'static super::table::Table,
    input: &[u8],
    ignore: bool,
) -> Result<Vec<u8>, ConvertError> {
    let mut map = [(0u8, [0u8; 3]); 256];
    for (b, slot) in map.iter_mut().enumerate() {
        let e = if (b as u16) < t.ident_upto {
            b as u32 + CHAR_BASE
        } else {
            let n = &t.nodes[0];
            if (b as u8) >= n.lo && (b as u8) <= n.hi {
                t.values[n.start as usize + b - n.lo as usize]
            } else {
                n.default
            }
        };
        if e == super::table::EMPTY {
            slot.0 = u8::MAX;
        } else if e >= CHAR_BASE && e & 0xC000_0000 == 0 {
            let cp = e - CHAR_BASE;
            let mut v = Vec::with_capacity(3);
            super::unicode::encode_utf8(&[cp], &mut v).ok();
            if v.len() <= 3 && !v.is_empty() {
                slot.0 = v.len() as u8;
                slot.1[..v.len()].copy_from_slice(&v);
            }
        }
    }
    let mut out = Vec::with_capacity(input.len() * 2);
    let ascii_ok = t.ident_upto >= 0x80;
    let mut i = 0;
    while i < input.len() {
        if ascii_ok {
            let n = ascii_prefix(&input[i..]);
            out.extend_from_slice(&input[i..i + n]);
            i += n;
            if i >= input.len() {
                break;
            }
        }
        let (n, bytes) = map[usize::from(input[i])];
        if n == 0 {
            if !ignore {
                return Err(ConvertError {
                    kind: ConvertErrorKind::IllegalSequence,
                    offset: i,
                    output: out,
                });
            }
        } else if n != u8::MAX {
            out.extend_from_slice(&bytes[..usize::from(n)]);
        }
        i += 1;
    }
    Ok(out)
}

/// A decoder/encoder pair with the error handling flags.
pub(crate) struct Converter {
    pub dec: Decoder,
    pub enc: Encoder,
    pub flags: Flags,
    buf: Vec<u32>,
    started: bool,
    /// //TRANSLIT results of stateless encoders, by code point's low byte.
    memo: Vec<Option<(u32, Vec<u8>)>>,
}

impl Converter {
    pub(crate) fn new(dec: Decoder, enc: Encoder, flags: Flags) -> Self {
        Converter {
            dec,
            enc,
            flags,
            buf: Vec::with_capacity(CHUNK),
            started: false,
            memo: if flags.translit {
                vec![None; 256]
            } else {
                Vec::new()
            },
        }
    }

    /// Converts all of `input` and closes the output.
    pub(crate) fn run(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), ConvertError> {
        let used = self.feed(input, out)?;
        if used < input.len() {
            return Err(self.error(ConvertErrorKind::IncompleteSequence, used, out));
        }
        self.close(out, input.len())
    }

    /// Converts the whole characters at the start of `input`; returns how
    /// many bytes it used (the rest starts an incomplete sequence).
    pub(crate) fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<usize, ConvertError> {
        if !self.started {
            self.enc.start(out);
            self.started = true;
        }
        let ascii_fast = self.dec.ascii_transparent() && self.enc.ascii_transparent();
        let mut pos = 0;
        while pos < input.len() {
            if ascii_fast && self.enc.ascii_transparent() {
                let n = ascii_prefix(&input[pos..]);
                out.extend_from_slice(&input[pos..pos + n]);
                pos += n;
                if pos >= input.len() {
                    break;
                }
            }
            let start = pos;
            self.buf.clear();
            let st = self.dec.decode(input, &mut pos, &mut self.buf, CHUNK);
            self.encode_buf(out, start)?;
            match st {
                DecodeStatus::Ok => {}
                DecodeStatus::Illegal(skip) => {
                    if !self.flags.ignore {
                        return Err(self.error(ConvertErrorKind::IllegalSequence, pos, out));
                    }
                    pos = (pos + skip).min(input.len());
                }
                DecodeStatus::IllegalReplace(skip) => {
                    if !self.flags.ignore {
                        return Err(self.error(ConvertErrorKind::IllegalSequence, pos, out));
                    }
                    pos = (pos + skip).min(input.len());
                    self.buf.clear();
                    self.buf.push(0xFFFD);
                    self.encode_buf(out, pos)?;
                }
                DecodeStatus::Incomplete => return Ok(pos),
            }
        }
        Ok(input.len())
    }

    /// The end of the input: flushes pending characters and shift states.
    pub(crate) fn close(&mut self, out: &mut Vec<u8>, offset: usize) -> Result<(), ConvertError> {
        if !self.started {
            self.enc.start(out);
            self.started = true;
        }
        self.buf.clear();
        let st = self.dec.finish(&mut self.buf);
        self.encode_buf(out, offset)?;
        if st == DecodeStatus::Incomplete {
            return Err(self.error(ConvertErrorKind::IncompleteSequence, offset, out));
        }
        self.enc.finish(out);
        Ok(())
    }

    fn error(&self, kind: ConvertErrorKind, offset: usize, out: &mut Vec<u8>) -> ConvertError {
        ConvertError {
            kind,
            offset,
            output: std::mem::take(out),
        }
    }

    /// Encodes the decoded characters in `self.buf`.
    fn encode_buf(&mut self, out: &mut Vec<u8>, offset: usize) -> Result<(), ConvertError> {
        let mut rest: &[u32] = &self.buf;
        while !rest.is_empty() {
            match self.enc.encode(rest, out) {
                Ok(()) => break,
                Err(i) => {
                    let cp = rest[i];
                    rest = &rest[i + 1..];
                    // Tag characters a charset can't encode vanish, even
                    // without //TRANSLIT or //IGNORE (observed, except for
                    // the ISO-2022 encoders).
                    if (0xE0000..=0xE007F).contains(&cp) && self.enc.drops_tags() {
                        continue;
                    }
                    if self.flags.translit {
                        // Stateless encoders give a code point the same
                        // replacement every time: remember it.
                        if self.enc.stateless() {
                            let slot = (cp & 0xFF) as usize;
                            if let Some((c, bytes)) = &self.memo[slot]
                                && *c == cp
                            {
                                out.extend_from_slice(bytes);
                                continue;
                            }
                            let before = out.len();
                            if translit_one(&mut self.enc, cp, out) {
                                self.memo[slot] = Some((cp, out[before..].to_vec()));
                                continue;
                            }
                        } else if translit_one(&mut self.enc, cp, out) {
                            continue;
                        }
                    }
                    if self.flags.ignore {
                        continue;
                    }
                    return Err(ConvertError {
                        kind: ConvertErrorKind::IllegalSequence,
                        offset,
                        output: std::mem::take(out),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Writes the first `//TRANSLIT` replacement of `cp` the encoder can encode,
/// or "?"; false when neither works.
fn translit_one(enc: &mut Encoder, cp: u32, out: &mut Vec<u8>) -> bool {
    for cand in translit::candidates(cp) {
        if cand.iter().all(|&c| enc.can_encode(c)) {
            return enc.encode(cand, out).is_ok();
        }
    }
    enc.can_encode(u32::from(b'?')) && enc.encode(&[u32::from(b'?')], out).is_ok()
}

/// Incremental conversion, for stream filters and output handlers: input
/// comes in pieces, and an incomplete character at the end of a piece waits
/// for the next one.
pub struct StreamConverter {
    conv: Converter,
    tail: Vec<u8>,
}

impl StreamConverter {
    /// A converter between two charsets; `flags` are the output charset's
    /// suffixes (see [`super::parse_charset`]).
    pub fn new(from: Charset, to: Charset, flags: Flags) -> Self {
        StreamConverter {
            conv: Converter::new(from.decoder(), to.encoder(), flags),
            tail: Vec::new(),
        }
    }

    /// Converts the next piece of input, appending to `out`. On an error,
    /// `out` holds what was converted before it.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), ConvertError> {
        let data: Vec<u8> = if self.tail.is_empty() {
            input.to_vec()
        } else {
            let mut d = std::mem::take(&mut self.tail);
            d.extend_from_slice(input);
            d
        };
        let mut tmp = Vec::new();
        match self.conv.feed(&data, &mut tmp) {
            Ok(used) => {
                out.extend_from_slice(&tmp);
                self.tail = data[used..].to_vec();
                Ok(())
            }
            Err(mut e) => {
                out.extend_from_slice(&e.output);
                e.output = std::mem::take(out);
                Err(e)
            }
        }
    }

    /// The end of the input: an incomplete character left is an error.
    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), ConvertError> {
        if !self.tail.is_empty() {
            return Err(ConvertError {
                kind: ConvertErrorKind::IncompleteSequence,
                offset: 0,
                output: std::mem::take(out),
            });
        }
        let mut tmp = Vec::new();
        match self.conv.close(&mut tmp, 0) {
            Ok(()) => {
                out.extend_from_slice(&tmp);
                Ok(())
            }
            Err(mut e) => {
                out.extend_from_slice(&e.output);
                e.output = std::mem::take(out);
                Err(e)
            }
        }
    }
}

/// Decodes `input` to code points.
pub(crate) fn decode_all(cs: Charset, input: &[u8]) -> Result<Vec<u32>, ConvertError> {
    let mut dec = cs.decoder();
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0;
    while pos < input.len() {
        match dec.decode(input, &mut pos, &mut out, usize::MAX) {
            DecodeStatus::Ok => {}
            DecodeStatus::Illegal(_) | DecodeStatus::IllegalReplace(_) => {
                return Err(ConvertError {
                    kind: ConvertErrorKind::IllegalSequence,
                    offset: pos,
                    output: Vec::new(),
                });
            }
            DecodeStatus::Incomplete => {
                return Err(ConvertError {
                    kind: ConvertErrorKind::IncompleteSequence,
                    offset: pos,
                    output: Vec::new(),
                });
            }
        }
    }
    if dec.finish(&mut out) == DecodeStatus::Incomplete {
        return Err(ConvertError {
            kind: ConvertErrorKind::IncompleteSequence,
            offset: pos,
            output: Vec::new(),
        });
    }
    Ok(out)
}

/// Counts the characters of `input` (`iconv_strlen()`), without keeping
/// them.
pub(crate) fn count_chars(cs: Charset, input: &[u8]) -> Result<usize, ConvertError> {
    if cs.is_utf8()
        && let Ok(s) = std::str::from_utf8(input)
    {
        return Ok(s.bytes().filter(|&b| b & 0xC0 != 0x80).count());
    }
    let mut dec = cs.decoder();
    let mut buf = Vec::with_capacity(CHUNK);
    let mut count = 0;
    let mut pos = 0;
    let err = |kind, offset| ConvertError {
        kind,
        offset,
        output: Vec::new(),
    };
    while pos < input.len() {
        buf.clear();
        match dec.decode(input, &mut pos, &mut buf, CHUNK) {
            DecodeStatus::Ok => count += buf.len(),
            DecodeStatus::Illegal(_) | DecodeStatus::IllegalReplace(_) => {
                return Err(err(ConvertErrorKind::IllegalSequence, pos));
            }
            DecodeStatus::Incomplete => return Err(err(ConvertErrorKind::IncompleteSequence, pos)),
        }
    }
    buf.clear();
    if dec.finish(&mut buf) == DecodeStatus::Incomplete {
        return Err(err(ConvertErrorKind::IncompleteSequence, pos));
    }
    Ok(count + buf.len())
}

/// Encodes code points with a fresh encoder (no translit, no ignore).
pub(crate) fn encode_all(cs: Charset, cps: &[u32]) -> Result<Vec<u8>, ConvertError> {
    let mut enc = cs.encoder();
    let mut out = Vec::with_capacity(cps.len() * 2);
    enc.start(&mut out);
    if enc.encode(cps, &mut out).is_err() {
        return Err(ConvertError {
            kind: ConvertErrorKind::IllegalSequence,
            offset: 0,
            output: out,
        });
    }
    enc.finish(&mut out);
    Ok(out)
}
