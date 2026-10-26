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

impl Errno {
    /// The errno value glibc sets.
    pub fn code(self) -> i32 {
        match self {
            Errno::Ilseq => 84,
            Errno::Inval => 22,
            Errno::TooBig => 7,
        }
    }
}

thread_local! {
    /// The errno the last failing iconv_open()/iconv() call left behind
    /// (php-src's "Unknown error (%d)" notices print errno).
    static LAST_ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// The errno left behind by the last failing conversion call.
pub fn last_errno() -> i32 {
    LAST_ERRNO.with(|c| c.get())
}

fn set_errno(code: i32) {
    LAST_ERRNO.with(|c| c.set(code));
}

/// iconv_t
#[derive(Clone, Debug)]
pub struct Iconv {
    dec: Decoder,
    enc: Encoder,
    ignore: bool,
    translit: bool,
    /// //IGNORE skipped something in this call.
    skipped: bool,
    /// ... and a character the target charset lacks among it.
    skipped_char: bool,
    /// iconv() calls made (glibc's invocation counter).
    calls: u32,
    /// glibc's direct module for the pair, if it has one.
    direct: Option<crate::direct::Direct>,
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
        let cd = Self::open_inner(to, from);
        if cd.is_none() {
            set_errno(22);
        }
        cd
    }

    fn open_inner(to: &[u8], from: &[u8]) -> Option<Iconv> {
        let (to_name, to_sfx) = split_spec(to);
        let (from_name, _) = split_spec(from);
        let to_kind = charset::lookup(to_name)?;
        let from_kind = charset::lookup(from_name)?;
        let direct = crate::direct::Direct::of(from_kind, to_kind);
        Some(Iconv {
            dec: Decoder::new(from_kind),
            enc: Encoder::new(to_kind),
            ignore: to_sfx.iter().any(|s| s == b"IGNORE"),
            translit: to_sfx.iter().any(|s| s == b"TRANSLIT"),
            skipped: false,
            skipped_char: false,
            calls: 0,
            direct,
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
        let r = self.conv_inner(input, pos, out, limit);
        if let Err(e) = r {
            set_errno(e.code());
        }
        r
    }

    fn conv_inner(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u8>,
        limit: Option<usize>,
    ) -> Result<(), Errno> {
        if let Some(d) = self.direct {
            self.calls = self.calls.saturating_add(1);
            // (//IGNORE having skipped something ends with EILSEQ)
            return match crate::direct::conv(d, self.ignore, input, pos, out, limit)? {
                true => Err(Errno::Ilseq),
                false => Ok(()),
            };
        }
        let start_len = out.len();
        let mut tmp = Vec::with_capacity(8);
        let first = self.calls == 0;
        if first {
            // (glibc's ISO-2022-KR starts its first call with the header,
            // or returns without counting the call when it has no room)
            let h = self.enc.pending_header();
            if limit.is_some_and(|l| h.len() > l) {
                return Err(Errno::TooBig);
            }
            out.extend_from_slice(h);
            self.enc.header_written();
        }
        self.calls = self.calls.saturating_add(1);
        // (what //IGNORE skips counts per call)
        self.skipped = false;
        self.skipped_char = false;
        // (glibc wants room for the smallest character before looking at
        // the next one, even one it then skips)
        let min_out = self.enc.min_len();
        // (glibc converts to UCS-4 first without a limit, then reruns that
        // step up to the characters the output took: the bytes a
        // combining-aware decoder held after the last of them go back)
        let rerun = matches!(self.dec.kind, charset::Kind::CompSbcs(_));
        let mut after_char = (*pos, self.dec);
        macro_rules! too_big {
            () => {{
                if rerun {
                    (*pos, self.dec) = after_char;
                }
                return Err(Errno::TooBig);
            }};
        }
        while *pos < input.len() || self.dec.has_pending() {
            let dsaved = self.dec;
            match self.dec.decode(&input[*pos..]) {
                Dec::Shift(n) => *pos += n,
                Dec::Incomplete => {
                    // (glibc reports an unconvertible character //IGNORE
                    // skipped first, not an illegal sequence)
                    self.skipped = false;
                    if std::mem::take(&mut self.skipped_char) {
                        return Err(Errno::Ilseq);
                    }
                    return Err(Errno::Inval);
                }
                Dec::Illegal(n) => {
                    if self.ignore {
                        if *pos + n > input.len() {
                            // (a skip past the end of the input wants more)
                            return Err(Errno::Inval);
                        }
                        *pos += n;
                        self.skipped = true;
                    } else {
                        return Err(Errno::Ilseq);
                    }
                }
                Dec::Char(cp, n) => {
                    if limit.is_some_and(|l| out.len() - start_len + min_out > l) {
                        // (the decoder takes the character again)
                        self.dec = dsaved;
                        too_big!();
                    }
                    tmp.clear();
                    // (a character held back for a composition goes out
                    // first unless this one combines with it)
                    let held = self.enc;
                    self.enc.flush_pending(cp, &mut tmp);
                    if !tmp.is_empty() {
                        if limit.is_some_and(|l| out.len() - start_len + tmp.len() > l) {
                            self.enc = held;
                            // (the decoder takes the character again)
                            self.dec = dsaved;
                            too_big!();
                        }
                        out.extend_from_slice(&tmp);
                        tmp.clear();
                    }
                    let saved = self.enc;
                    if !self.enc.encode(cp, &mut tmp) {
                        self.enc = saved;
                        tmp.clear();
                        if (cp >> 7) == (0xE0000 >> 7) && self.enc.kind != charset::Kind::Iso2022Jp
                        {
                            // glibc's encoders drop the Unicode tag
                            // characters they cannot write
                            *pos += n;
                            continue;
                        }
                        let room = limit.map(|l| l - (out.len() - start_len));
                        let t = if self.translit {
                            self.transliterate(cp, &mut tmp, first, room)
                        } else {
                            Ok(false)
                        };
                        if t.is_err() {
                            out.extend_from_slice(&tmp);
                            // (the decoder takes the character again)
                            self.dec = dsaved;
                            too_big!();
                        }
                        if t == Ok(true) {
                            // transliterated
                        } else if self.ignore {
                            *pos += n;
                            self.skipped = true;
                            self.skipped_char = true;
                            after_char = (*pos, self.dec);
                            continue;
                        } else {
                            return Err(Errno::Ilseq);
                        }
                    }
                    if let Some(l) = limit.filter(|&l| out.len() - start_len + tmp.len() > l) {
                        let room = l - (out.len() - start_len);
                        let n = no_room(&mut self.enc, &saved, cp, &tmp, room);
                        out.extend_from_slice(&tmp[..n]);
                        // (the decoder takes the character again)
                        self.dec = dsaved;
                        too_big!();
                    }
                    out.extend_from_slice(&tmp);
                    *pos += n;
                    after_char = (*pos, self.dec);
                }
            }
        }
        if self.skipped {
            self.skipped = false;
            self.skipped_char = false;
            return Err(Errno::Ilseq);
        }
        Ok(())
    }

    /// //TRANSLIT: the first alternative of `cp`'s transliteration that
    /// the target charset can represent, else "?".
    ///
    /// glibc converts each alternative with a nested call of the
    /// conversion: one that fails drops its output but keeps its shift
    /// state changes, and ISO-2022-KR writes its header again in each
    /// during the first call.
    ///
    /// Err: an alternative found no room (`room`): its output is dropped,
    /// its shift state changes are not.
    fn transliterate(
        &mut self,
        cp: u32,
        out: &mut Vec<u8>,
        first: bool,
        room: Option<usize>,
    ) -> Result<bool, ()> {
        let header: &[u8] = if first && self.enc.kind == charset::Kind::Iso2022Kr {
            b"\x1b$)C"
        } else {
            b""
        };
        let room = room.unwrap_or(usize::MAX);
        let mut one = Vec::with_capacity(8);
        // (an empty alternative drops the character)
        let alts = translit(cp).map(|t| t.split('\0').map(|a| (a, false)));
        'alts: for (alt, missing) in alts
            .into_iter()
            .flatten()
            .chain(std::iter::once(("?", true)))
        {
            out.clear();
            if header.len() > room {
                return Err(());
            }
            out.extend_from_slice(header);
            for c in alt.chars() {
                one.clear();
                let saved = self.enc;
                if !self.enc.encode(c as u32, &mut one) {
                    self.enc = saved;
                    continue 'alts;
                }
                if out.len() + one.len() > room {
                    let n = no_room(&mut self.enc, &saved, c as u32, &one, room - out.len());
                    // (what the default "?" wrote before running out stays)
                    if missing {
                        out.extend_from_slice(&one[..n]);
                    } else {
                        out.clear();
                    }
                    return Err(());
                }
                out.extend_from_slice(&one);
            }
            return Ok(true);
        }
        out.clear();
        Ok(false)
    }

    /// iconv(cd, NULL, NULL, &out, &outleft): back to the initial shift
    /// state, writing the sequence that does it.
    pub fn flush(&mut self, out: &mut Vec<u8>, limit: Option<usize>) -> Result<(), Errno> {
        // (EMIT_SHIFT_TO_INIT: a decoder's held character goes through the
        // rest of the conversion first)
        if let Some(c) = self.dec.take_held() {
            self.dec.set_pending(c);
        }
        if self.direct.is_none() && self.dec.has_pending() {
            let mut p = 0;
            let start = out.len();
            self.conv(&[], &mut p, out, limit)?;
            let limit = limit.map(|l| l - (out.len() - start));
            return self.flush_shift(out, limit);
        }
        self.flush_shift(out, limit)
    }

    fn flush_shift(&mut self, out: &mut Vec<u8>, limit: Option<usize>) -> Result<(), Errno> {
        let b = self.enc.reset_bytes();
        if limit.is_some_and(|l| b.len() > l) {
            set_errno(7);
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

/// A character (`bytes`, from `saved`'s state to `enc`'s) without room in
/// the output (`room` bytes left): how much glibc's stateful encoders
/// still write of it (their escape sequences and shifts), with the state
/// they leave.
fn no_room(enc: &mut Encoder, saved: &Encoder, cp: u32, bytes: &[u8], room: usize) -> usize {
    use charset::Kind;
    let char_len = |n: usize| bytes.len().saturating_sub(n);
    let prefix = match enc.kind {
        // the escape sequence (G0 designation), switched to
        Kind::Iso2022Jp | Kind::Iso2022Jp2 | Kind::Iso2022Jp3 if bytes.first() == Some(&0x1B) => {
            let esc_len = if bytes.get(1) == Some(&b'$') && bytes.get(2) == Some(&b'(') {
                4
            } else {
                3
            };
            if bytes.len() <= esc_len {
                // (the escape of a character ISO-2022-JP-3 holds back)
                *enc = *saved;
                return 0;
            }
            if bytes.len() >= 3 && bytes[bytes.len() - 3] == 0x1B && bytes[bytes.len() - 2] == b'N'
            {
                // (a G2 character: ESC N and its byte)
                char_len(3)
            } else if cp < 0x80 || (0xFF61..=0xFF9F).contains(&cp) || cp == 0xA5 || cp == 0x203E {
                char_len(1)
            } else {
                char_len(2)
            }
        }
        // the header and SO, switched to
        Kind::Iso2022Kr => char_len(if cp < 0x80 { 1 } else { 2 }),
        // SI, switched to
        Kind::Iso2022Cn(_) if cp < 0x80 => char_len(1),
        Kind::Iso2022Cn(_) => {
            // the designation and the shift, without taking the shift
            // state (the shift comes again)
            let p = char_len(2);
            let n = if p <= room {
                p
            } else if p >= 4 && bytes[0] == 0x1B && bytes[1] == b'$' && room >= 4 {
                4
            } else {
                0
            };
            if n == 0 {
                *enc = *saved;
            } else {
                enc.restore_shift(saved);
            }
            return n;
        }
        _ => 0,
    };
    if prefix > 0 && prefix <= room {
        prefix
    } else {
        *enc = *saved;
        0
    }
}

/// Decode a whole string to code points: the characters before the first
/// error, and the error.
pub fn decode_all(input: &[u8], enc: &[u8]) -> Option<(Vec<u32>, Option<Errno>)> {
    decode_all_marked(input, enc, &mut Vec::new())
}

/// decode_all(), also telling (in `second`) which characters are the
/// second of a sequence decoding to two.
pub fn decode_all_marked(
    input: &[u8],
    enc: &[u8],
    second: &mut Vec<bool>,
) -> Option<(Vec<u32>, Option<Errno>)> {
    let (name, _) = split_spec(enc);
    let Some(kind) = charset::lookup(name) else {
        set_errno(22);
        return None;
    };
    let mut dec = Decoder::new(kind);
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0;
    while pos < input.len() || dec.has_pending() {
        match dec.decode(&input[pos..]) {
            Dec::Char(cp, n) => {
                out.push(cp);
                second.push(n == 0);
                pos += n;
            }
            Dec::Shift(n) => pos += n,
            Dec::Illegal(_) => {
                set_errno(84);
                return Some((out, Some(Errno::Ilseq)));
            }
            Dec::Incomplete => {
                set_errno(22);
                return Some((out, Some(Errno::Inval)));
            }
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
