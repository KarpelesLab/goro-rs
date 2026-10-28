//! `iconv_mime_encode()`, `iconv_mime_decode()` and
//! `iconv_mime_decode_headers()` (RFC 2047 encoded words; PHP manual:
//! iconv_mime_encode, iconv_mime_decode, iconv_mime_decode_headers; tests
//! ext/iconv/tests/iconv_mime_*.phpt and bug*.phpt). The details below were
//! observed on PHP 8.5 with glibc 2.43.
//!
//! Encoding writes `name: ` and then encoded words of at most `line-length`
//! characters, joined by `line-break-chars` and a space. A word holds as
//! many whole characters as fit (for B, the room is
//! `(room - prefix - 2) / 4 * 3 - 4` bytes, the last four kept for the shift
//! sequence a stateful charset ends with); when the first word doesn't fit
//! after the field name, the value starts on the next line.
//!
//! Decoding (see [`decode_field`]) recognises `=?charset[*lang]?B|Q?text?=`
//! anywhere (with ICONV_MIME_DECODE_STRICT only after whitespace, and only
//! when whitespace follows), drops the whitespace between adjacent words,
//! unfolds lines and stops at the end of the header. Text outside words is
//! converted from ASCII. ICONV_MIME_DECODE_CONTINUE_ON_ERROR keeps a
//! malformed or undecodable word as it is.

use super::charset::{Charset, Encoder};
use super::convert::{self, ConvertErrorKind};
use super::names::{self, Flags};
use super::php::{MSG_ILLEGAL, MSG_INCOMPLETE, MSG_TOO_LONG};
use super::{Diag, MAX_CHARSET_LEN, MIME_DECODE_CONTINUE_ON_ERROR, MIME_DECODE_STRICT};

/// The encoding scheme of `iconv_mime_encode()`'s preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MimeScheme {
    /// Base64.
    B,
    /// Quoted-printable.
    Q,
}

/// `iconv_mime_encode()`'s preferences, with PHP's defaults applied by the
/// caller (scheme "B", charsets `iconv.internal_encoding`, line length 76,
/// line break "\r\n"; the scheme is "B" unless the preference starts with
/// "Q" or "q", observed).
#[derive(Clone, Debug)]
pub struct MimeEncodeOptions {
    pub scheme: MimeScheme,
    pub input_charset: Vec<u8>,
    pub output_charset: Vec<u8>,
    pub line_length: i64,
    pub line_break_chars: Vec<u8>,
}

const MSG_BUFFER: &str = "Buffer length exceeded";
const MSG_MALFORMED: &str = "Malformed string";

fn wrong_encoding(to: &[u8]) -> Diag {
    super::php::wrong_encoding(b"???", to)
}

/// Q encoding: printable ASCII but '=', '?' and '_' as is (space too is
/// encoded), the rest as =XX (observed).
fn q_encode(bytes: &[u8], out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in bytes {
        if (0x21..0x7F).contains(&b) && b != b'=' && b != b'?' && b != b'_' {
            out.push(b);
        } else {
            out.extend_from_slice(&[b'=', HEX[usize::from(b >> 4)], HEX[usize::from(b & 15)]]);
        }
    }
}

fn q_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .map(|&b| {
            if (0x21..0x7F).contains(&b) && b != b'=' && b != b'?' && b != b'_' {
                1
            } else {
                3
            }
        })
        .sum()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(bytes: &[u8], out: &mut Vec<u8>) {
    for chunk in bytes.chunks(3) {
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(v >> 18) as usize & 63]);
        out.push(B64[(v >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 {
            B64[(v >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            B64[v as usize & 63]
        } else {
            b'='
        });
    }
}

/// Encodes `cps` with a fresh encoder: (bytes before the closing shift
/// sequence, all bytes).
fn encode_chunk(cs: Charset, flags: Flags, cps: &[u32]) -> Option<(usize, Vec<u8>)> {
    let mut enc = cs.encoder();
    let mut out = Vec::new();
    enc.start(&mut out);
    let mut rest = cps;
    while !rest.is_empty() {
        match enc.encode(rest, &mut out) {
            Ok(()) => break,
            Err(i) => {
                let cp = rest[i];
                rest = &rest[i + 1..];
                if !(flags.translit && translit_into(&mut enc, cp, &mut out)) {
                    return None;
                }
            }
        }
    }
    let body = out.len();
    enc.finish(&mut out);
    Some((body, out))
}

fn translit_into(enc: &mut Encoder, cp: u32, out: &mut Vec<u8>) -> bool {
    for cand in super::translit::candidates(cp) {
        if cand.iter().all(|&c| enc.can_encode(c)) {
            return enc.encode(cand, out).is_ok();
        }
    }
    enc.can_encode(u32::from(b'?')) && enc.encode(&[u32::from(b'?')], out).is_ok()
}

/// `iconv_mime_encode(string $field_name, string $field_value, array $options = []): string|false`.
pub fn mime_encode(
    name: &[u8],
    value: &[u8],
    opts: &MimeEncodeOptions,
    diags: &mut Vec<Diag>,
) -> Option<Vec<u8>> {
    if opts.input_charset.len() >= MAX_CHARSET_LEN || opts.output_charset.len() >= MAX_CHARSET_LEN {
        diags.push(Diag::Warning(MSG_TOO_LONG.into()));
        return None;
    }
    let (Some((src, _)), Some((dst, flags))) = (
        names::parse_charset(&opts.input_charset),
        names::parse_charset(&opts.output_charset),
    ) else {
        diags.push(super::php::wrong_encoding(
            &opts.input_charset,
            &opts.output_charset,
        ));
        return None;
    };
    let chars = match convert::decode_all(src, value) {
        Ok(c) => c,
        Err(e) => {
            diags.push(super::php::conv_diag(&e));
            return None;
        }
    };
    let flags = Flags {
        translit: flags.translit,
        ignore: false,
    };
    let line_len = opts.line_length;
    let lf = &opts.line_break_chars;
    let mut out = Vec::with_capacity(value.len() * 2 + 32);
    out.extend_from_slice(name);
    out.extend_from_slice(b": ");
    let mut prefix = b"=?".to_vec();
    prefix.extend_from_slice(&opts.output_charset);
    prefix.extend_from_slice(match opts.scheme {
        MimeScheme::B => b"?B?",
        MimeScheme::Q => b"?Q?",
    });
    let plen = prefix.len() as i64;
    let buffer_err = |diags: &mut Vec<Diag>| {
        diags.push(Diag::Warning(MSG_BUFFER.into()));
        None
    };
    let mut c = line_len - out.len() as i64;
    if c <= 0 || (opts.scheme == MimeScheme::Q && line_len < plen + 8) {
        return buffer_err(diags);
    }
    let lf_len = lf.len() as i64;
    let mut first = true;
    let mut i = 0usize;
    loop {
        let wrap_at = plen + lf_len + if opts.scheme == MimeScheme::B { 7 } else { 6 };
        if first && c < wrap_at {
            out.extend_from_slice(lf);
            out.push(b' ');
            c = line_len - 1;
        }
        // The longest run of characters from i that fits.
        let mut n = 0usize;
        let mut best: Option<Vec<u8>> = None;
        match opts.scheme {
            MimeScheme::B => {
                let room = c - plen - 2;
                let avail = if room < 0 { -1 } else { room / 4 * 3 - 4 };
                if avail < 0 {
                    return buffer_err(diags);
                }
                while i + n < chars.len() {
                    match encode_chunk(dst, flags, &chars[i..i + n + 1]) {
                        Some((body, bytes)) if body as i64 <= avail => best = Some(bytes),
                        Some(_) => break,
                        None => {
                            diags.push(Diag::Notice(MSG_ILLEGAL.into()));
                            return None;
                        }
                    }
                    n += 1;
                }
                if n == 0 && i < chars.len() {
                    return buffer_err(diags);
                }
            }
            MimeScheme::Q => {
                let room = c - plen - 2;
                while i + n < chars.len() {
                    match encode_chunk(dst, flags, &chars[i..i + n + 1]) {
                        Some((_, bytes)) if q_len(&bytes) as i64 <= room => best = Some(bytes),
                        Some(_) => break,
                        None => {
                            diags.push(Diag::Notice(MSG_ILLEGAL.into()));
                            return None;
                        }
                    }
                    n += 1;
                }
                if n == 0 && i < chars.len() {
                    diags.push(Diag::Notice("Unknown error (7)".into()));
                    return None;
                }
            }
        }
        let bytes = match best {
            Some(b) => b,
            None => encode_chunk(dst, flags, &[])
                .map(|(_, b)| b)
                .unwrap_or_default(),
        };
        out.extend_from_slice(&prefix);
        match opts.scheme {
            MimeScheme::B => b64_encode(&bytes, &mut out),
            MimeScheme::Q => q_encode(&bytes, &mut out),
        }
        out.extend_from_slice(b"?=");
        first = false;
        i += n;
        if i >= chars.len() {
            return Some(out);
        }
        out.extend_from_slice(lf);
        out.push(b' ');
        c = line_len - 1;
    }
}

/// One decoding pass over a header field. Returns where it stopped (the
/// end of the header) or the diagnostic that makes the call fail.
struct FieldDecoder<'a> {
    s: &'a [u8],
    /// The output charset argument, for messages.
    cs_arg: &'a [u8],
    strict: bool,
    cont: bool,
    cs: Charset,
    flags: Flags,
    out: Vec<u8>,
    /// Converter for the text outside encoded words (from ASCII).
    plain: Encoder,
    /// Whitespace waiting for what comes next.
    pending: Vec<u8>,
    /// The last thing written was an encoded word.
    after_word: bool,
    /// The previous character was whitespace (or the field start).
    prev_ws: bool,
    /// Start of the current run of adjacent encoded words.
    seq_start: Option<usize>,
    converted: bool,
    /// An 8-bit byte was dropped from the text.
    dropped: bool,
    /// Strict mode drops 8-bit text bytes, but not after a lone CR or a word
    /// it refused (observed).
    strict8: bool,
    /// A conversion failure lost the start of the current run of words.
    lost: bool,
    /// The header ended inside a word or after an '='.
    ended: bool,
}

enum Stop {
    /// The header ended at this offset (the newline, or the input end).
    End(usize),
    Fail(Diag),
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

impl<'a> FieldDecoder<'a> {
    fn new(s: &'a [u8], cs_arg: &'a [u8], mode: i64, cs: Charset, flags: Flags) -> Self {
        FieldDecoder {
            s,
            cs_arg,
            strict: mode & MIME_DECODE_STRICT != 0,
            cont: mode & MIME_DECODE_CONTINUE_ON_ERROR != 0,
            cs,
            flags: Flags {
                translit: flags.translit,
                ignore: false,
            },
            out: Vec::new(),
            plain: cs.encoder(),
            pending: Vec::new(),
            after_word: false,
            prev_ws: true,
            seq_start: None,
            converted: false,
            dropped: false,
            strict8: false,
            lost: false,
            ended: false,
        }
    }

    /// Appends plain (ASCII) bytes; bytes above 0x7F are illegal, or dropped
    /// when `lenient` (observed: after whitespace or a lone CR) or in the
    /// strict and continue modes.
    fn plain(&mut self, bytes: &[u8], lenient: bool) -> Result<(), Diag> {
        for &b in bytes {
            if b >= 0x80 {
                if lenient || self.cont || (self.strict && !self.strict8) {
                    self.dropped = true;
                    continue;
                }
                return Err(Diag::Notice(MSG_ILLEGAL.into()));
            }
            if self.plain.encode(&[u32::from(b)], &mut self.out).is_err() {
                return Err(Diag::Notice(MSG_ILLEGAL.into()));
            }
            self.converted = true;
        }
        Ok(())
    }

    fn flush_pending(&mut self) -> Result<(), Diag> {
        let p = std::mem::take(&mut self.pending);
        self.plain(&p, false)
    }

    /// Writes the raw text of a failed word (CONTINUE_ON_ERROR), unless the
    /// run it belongs to lost its start (after a conversion failure).
    fn raw(&mut self, from: Option<usize>, to: usize) -> Result<(), Diag> {
        let mut chunk = std::mem::take(&mut self.pending);
        if let Some(from) = from {
            chunk.extend_from_slice(&self.s[from..to.min(self.s.len())]);
        }
        // The whitespace and the text go out together, or not at all when
        // they hold an 8-bit byte (observed).
        if chunk.iter().any(|&b| b >= 0x80) {
            self.dropped = true;
        } else {
            self.plain(&chunk, false)?;
        }
        self.after_word = false;
        self.seq_start = None;
        Ok(())
    }

    /// Where the raw text of a word starting at `start` begins: the start of
    /// the run of adjacent words it continues, if any.
    fn run_start(&self, start: usize) -> Option<usize> {
        if self.after_word && self.pending.is_empty() {
            self.seq_start.or(Some(start))
        } else if self.lost && self.pending.is_empty() {
            None
        } else {
            Some(start)
        }
    }

    /// Decodes the text of a complete word.
    fn word(
        &mut self,
        src: Charset,
        q: bool,
        data: &[u8],
        end: usize,
    ) -> Result<Vec<u8>, WordError> {
        let bytes = if q {
            match q_decode(data) {
                Some(b) => b,
                None => {
                    // PHP reports errno here (observed): EILSEQ after a
                    // dropped 8-bit byte or before one, a ':' or another
                    // word, EINVAL after earlier text, else 0.
                    let rest = &self.s[end.min(self.s.len())..];
                    let rest = &rest[..rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len())];
                    let later = rest.iter().any(|&b| b >= 0x80 || b == b':')
                        || rest.windows(2).any(|w| w == b"=?");
                    let converted =
                        self.converted || (!self.after_word && !self.pending.is_empty());
                    let errno = if self.dropped || later {
                        84
                    } else if converted {
                        22
                    } else {
                        0
                    };
                    return Err(WordError::Q(Diag::Notice(format!(
                        "Unknown error ({errno})"
                    ))));
                }
            }
        } else {
            b64_decode(data)
        };
        match convert::convert(src, self.cs, self.flags, &bytes) {
            Ok(v) => {
                self.converted = true;
                Ok(v)
            }
            Err(e) => Err(WordError::Convert(Diag::Notice(
                match e.kind {
                    ConvertErrorKind::IllegalSequence => MSG_ILLEGAL,
                    ConvertErrorKind::IncompleteSequence => MSG_INCOMPLETE,
                }
                .into(),
            ))),
        }
    }

    fn run(&mut self, mut i: usize) -> Stop {
        match self.scan(&mut i) {
            Ok(()) => {
                self.plain.finish(&mut self.out);
                Stop::End(i)
            }
            Err(d) => Stop::Fail(d),
        }
    }

    /// Handles a newline at `i` (CR LF or LF): unfolds, or ends the header.
    /// Returns false at the end of the header.
    fn newline(&mut self, i: &mut usize) -> bool {
        let s = self.s;
        let nl = if s[*i] == b'\r' { 2 } else { 1 };
        let next = *i + nl;
        if next < s.len() && is_ws(s[next]) {
            let mut j = next;
            while j < s.len() && is_ws(s[j]) {
                j += 1;
            }
            *i = j;
            self.pending.clear();
            // After a word the fold disappears; elsewhere it is a space.
            if !self.after_word {
                let _ = self.plain(b" ", false);
            }
            self.prev_ws = true;
            self.lost = false;
            return true;
        }
        *i += nl;
        false
    }

    /// Right after a word, an '=' that doesn't start another one brings the
    /// word's text out again (observed).
    fn reemit_run(&mut self, start: usize) -> Result<(), Diag> {
        if self.after_word
            && self.pending.is_empty()
            && let Some(st) = self.seq_start
        {
            let bytes = self.s[st..start].to_vec();
            self.plain(&bytes, true)?;
        }
        Ok(())
    }

    /// Ends the header, the next one starting at `next`.
    fn end_at(&mut self, next: usize, i: &mut usize) {
        self.ended = true;
        *i = next.min(self.s.len());
    }

    fn text_char(&mut self) {
        self.after_word = false;
        self.prev_ws = false;
        self.seq_start = None;
        self.lost = false;
    }

    fn scan(&mut self, i: &mut usize) -> Result<(), Diag> {
        let s = self.s;
        while *i < s.len() {
            let c = s[*i];
            match c {
                b' ' | b'\t' => {
                    self.pending.push(c);
                    self.prev_ws = true;
                    self.lost = false;
                    *i += 1;
                }
                b'\n' => {
                    if !self.newline(i) {
                        return Ok(());
                    }
                }
                b'\r' => {
                    if s.get(*i + 1) == Some(&b'\n') {
                        if !self.newline(i) {
                            return Ok(());
                        }
                        continue;
                    }
                    // A lone CR and the byte after it are text.
                    let Some(&n) = s.get(*i + 1) else {
                        if self.cont {
                            *i += 1;
                            return Ok(());
                        }
                        return Err(Diag::Warning(MSG_MALFORMED.into()));
                    };
                    self.pending.clear();
                    self.plain(b"\r", false)?;
                    self.plain(&[n], true)?;
                    self.text_char();
                    // Strict mode takes what follows as after whitespace.
                    self.prev_ws = true;
                    *i += 2;
                }
                b'=' if !self.strict || self.prev_ws => {
                    self.encoded_word(i)?;
                    if self.ended {
                        return Ok(());
                    }
                }
                _ if c >= 0x80 && self.after_word => {
                    // 8-bit bytes after a word are dropped (observed).
                    self.dropped = true;
                    *i += 1;
                }
                _ => {
                    let lenient = self.prev_ws;
                    // Text right after a word keeps its "after a word" state:
                    // whitespace before the next word still goes (observed).
                    let keep = self.after_word && self.pending.is_empty();
                    self.flush_pending()?;
                    self.plain(&[c], lenient && c >= 0x80)?;
                    self.text_char();
                    self.after_word = keep;
                    *i += 1;
                }
            }
        }
        Ok(())
    }

    /// At an '=' that may start an encoded word.
    fn encoded_word(&mut self, i: &mut usize) -> Result<(), Diag> {
        let s = self.s;
        let start = *i;
        let malformed = Diag::Warning(MSG_MALFORMED.into());
        // '=' then something other than '?': both are text.
        match s.get(start + 1) {
            None => {
                if self.cont {
                    // Whitespace after a word goes with the '=' (observed).
                    if self.after_word {
                        self.pending.clear();
                    }
                    self.flush_pending()?;
                    self.plain(b"=", false)?;
                    *i += 1;
                    return Ok(());
                }
                return Err(malformed);
            }
            Some(b'\n') => {
                // "=" LF ends the header (observed).
                self.reemit_run(start)?;
                self.flush_pending()?;
                self.plain(b"=", false)?;
                self.end_at(start + 2, i);
                return Ok(());
            }
            Some(&b) if b != b'?' => {
                if b == b'\r' {
                    // "=" CR LF ends the header, but is malformed at the end
                    // of the input; "=" CR drops the CR (observed).
                    if s.get(start + 2) == Some(&b'\n') {
                        if start + 3 >= s.len() && !self.cont {
                            return Err(malformed);
                        }
                        self.reemit_run(start)?;
                        self.flush_pending()?;
                        self.plain(b"=", false)?;
                        self.end_at(start + 3, i);
                        return Ok(());
                    }
                    self.flush_pending()?;
                    self.plain(b"=", false)?;
                    self.text_char();
                    *i += 2;
                    return Ok(());
                }
                self.reemit_run(start)?;
                self.flush_pending()?;
                self.plain(b"=", false)?;
                if b >= 0x80 {
                    return Err(Diag::Notice(MSG_ILLEGAL.into()));
                }
                self.plain(&[b], false)?;
                self.text_char();
                *i += 2;
                return Ok(());
            }
            _ => {}
        }
        let raw_start = self.run_start(start);
        // =? charset [*lang] ? B|Q ? text ?=
        let mut j = start + 2;
        let mut charset = Vec::new();
        let mut in_lang = false;
        loop {
            match s.get(j) {
                None => return self.unterminated(i),
                Some(b'\r') | Some(b'\n') => return self.word_newline(start, j, i),
                Some(b'?') => break,
                Some(b'*') => in_lang = true,
                Some(&b) => {
                    if !in_lang {
                        charset.push(b);
                    }
                }
            }
            j += 1;
        }
        // The charset is opened as soon as its name ends (observed); with
        // CONTINUE_ON_ERROR the word is kept as text once complete.
        let src = names::parse_charset(&charset).map(|c| c.0);
        if src.is_none() && !self.cont {
            return Err(wrong_encoding(self.cs_arg));
        }
        j += 1;
        // After an unknown charset, a word the input ends in is kept as text.
        let bad_cs = src.is_none();
        macro_rules! unterminated {
            () => {
                if bad_cs {
                    return self.bad_word(raw_start, s.len(), i);
                } else {
                    return self.unterminated(i);
                }
            };
        }
        let q = match s.get(j) {
            None => unterminated!(),
            Some(b'B') | Some(b'b') => false,
            Some(b'Q') | Some(b'q') => true,
            Some(_) => return self.bad_word(raw_start, j + 1, i),
        };
        j += 1;
        match s.get(j) {
            None => unterminated!(),
            Some(b'?') => {}
            Some(_) => return self.bad_word(raw_start, j + 1, i),
        }
        j += 1;
        let data_start = j;
        loop {
            match s.get(j) {
                None => unterminated!(),
                Some(b'?') => match s.get(j + 1) {
                    Some(b'=') => break,
                    None => unterminated!(),
                    Some(_) => return self.bad_word(raw_start, j + 2, i),
                },
                Some(_) => j += 1,
            }
        }
        let data = &s[data_start..j];
        let end = j + 2;
        // Strict mode: the word must be followed by whitespace or the end.
        if self.strict
            && let Some(&n) = s.get(end)
            && !matches!(n, b' ' | b'\t' | b'\r' | b'\n')
        {
            self.flush_pending()?;
            let bytes = s[start..end].to_vec();
            self.plain(&bytes, false)?;
            self.strict8 = true;
            self.text_char();
            self.after_word = true;
            *i = end;
            return Ok(());
        }
        let Some(src) = src else {
            // Unknown charset (CONTINUE_ON_ERROR): the whole word as text,
            // then as after a word.
            if self.after_word {
                self.pending.clear();
            }
            self.raw(raw_start, end)?;
            self.after_word = true;
            self.prev_ws = false;
            self.lost = false;
            *i = end;
            return Ok(());
        };
        match self.word(src, q, data, end) {
            Ok(text) => {
                if self.after_word {
                    self.pending.clear();
                } else {
                    self.flush_pending()?;
                }
                self.out.extend_from_slice(&text);
                self.after_word = true;
                self.prev_ws = false;
                self.seq_start = raw_start;
                self.lost = false;
                *i = end;
                Ok(())
            }
            Err(WordError::Q(diag)) => {
                if !self.cont {
                    return Err(diag);
                }
                // The text up to the final '=', which then takes the next
                // byte as text (observed).
                if self.after_word {
                    self.pending.clear();
                }
                if raw_start.is_none() {
                    self.raw(None, end)?;
                    self.text_char();
                    *i = end;
                    return Ok(());
                }
                self.raw(raw_start, end - 1)?;
                self.plain(b"=", false)?;
                if let Some(&n) = s.get(end) {
                    self.plain(&[n], true)?;
                    *i = end + 1;
                } else {
                    *i = end;
                }
                self.text_char();
                Ok(())
            }
            Err(WordError::Convert(diag)) => {
                if !self.cont {
                    return Err(diag);
                }
                // The text of the run, without the final '=' at the end of
                // the input; the next adjacent words lose theirs (observed).
                if self.after_word {
                    self.pending.clear();
                }
                let stop = if end < s.len() { end } else { end - 1 };
                self.raw(raw_start, stop)?;
                self.text_char();
                self.lost = true;
                *i = end;
                Ok(())
            }
        }
    }

    /// The input ends inside a word.
    fn unterminated(&mut self, i: &mut usize) -> Result<(), Diag> {
        if self.cont {
            if self.after_word {
                self.pending.clear();
            }
            self.flush_pending()?;
            *i = self.s.len();
            return Ok(());
        }
        Err(Diag::Warning(MSG_MALFORMED.into()))
    }

    /// A newline inside a word: the word so far is text and the header goes
    /// on with the newline (observed); CR LF there ends the header, but is
    /// malformed at the end of the input, and a lone CR is dropped.
    fn word_newline(&mut self, start: usize, j: usize, i: &mut usize) -> Result<(), Diag> {
        let s = self.s;
        if s[j] == b'\r' {
            if s.get(j + 1) == Some(&b'\n') {
                if j + 2 >= s.len() && !self.cont {
                    return Err(Diag::Warning(MSG_MALFORMED.into()));
                }
                self.flush_pending()?;
                let bytes = s[start..j].to_vec();
                self.plain(&bytes, false)?;
                self.end_at(j + 2, i);
                return Ok(());
            }
            self.flush_pending()?;
            let bytes = s[start..j].to_vec();
            self.plain(&bytes, false)?;
            self.text_char();
            *i = j + 1;
            return Ok(());
        }
        // LF: the header ends here, even before whitespace.
        self.flush_pending()?;
        let bytes = self.s[start..j].to_vec();
        self.plain(&bytes, false)?;
        self.end_at(j + 1, i);
        Ok(())
    }

    /// A malformed word: an error, or with CONTINUE_ON_ERROR its text up to
    /// and with the offending byte, the scan going on after it.
    fn bad_word(&mut self, raw_start: Option<usize>, to: usize, i: &mut usize) -> Result<(), Diag> {
        if !self.cont {
            return Err(Diag::Warning(MSG_MALFORMED.into()));
        }
        if self.after_word {
            self.pending.clear();
        }
        self.raw(raw_start, to)?;
        self.text_char();
        *i = to;
        Ok(())
    }
}

enum WordError {
    /// Bad quoted-printable text.
    Q(Diag),
    /// The decoded bytes don't convert.
    Convert(Diag),
}

/// Q decoding: '_' is a space, =XX a byte (either case), a final '=' is
/// ignored; anything else after '=' fails.
fn q_decode(d: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(d.len());
    let mut i = 0;
    while i < d.len() {
        match d[i] {
            b'_' => out.push(b' '),
            b'=' => {
                if i + 1 == d.len() {
                    break;
                }
                let h = d.get(i + 1..i + 3)?;
                let v = (hex_val(h[0])? << 4) | hex_val(h[1])?;
                out.push(v);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Base64 decoding that skips anything outside the alphabet.
pub(crate) fn b64_decode(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len() * 3 / 4);
    let mut bits = 0u32;
    let mut n = 0;
    for &b in d {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    out
}

fn output_charset(charset: &[u8], diags: &mut Vec<Diag>) -> Option<(Charset, Flags)> {
    if charset.len() >= MAX_CHARSET_LEN {
        diags.push(Diag::Warning(MSG_TOO_LONG.into()));
        return None;
    }
    match names::parse_charset(charset) {
        Some(v) => Some(v),
        None => {
            diags.push(wrong_encoding(charset));
            None
        }
    }
}

/// Decodes one header field starting at `start`; (value, end offset).
fn decode_field(
    s: &[u8],
    start: usize,
    mode: i64,
    cs_arg: &[u8],
    cs: Charset,
    flags: Flags,
) -> Result<(Vec<u8>, usize), Diag> {
    let mut d = FieldDecoder::new(s, cs_arg, mode, cs, flags);
    match d.run(start) {
        Stop::End(end) => Ok((d.out, end)),
        Stop::Fail(diag) => Err(diag),
    }
}

/// `iconv_mime_decode(string $string, int $mode = 0, ?string $encoding = null): string|false`.
pub fn mime_decode(s: &[u8], mode: i64, charset: &[u8], diags: &mut Vec<Diag>) -> Option<Vec<u8>> {
    let (cs, flags) = output_charset(charset, diags)?;
    match decode_field(s, 0, mode, charset, cs, flags) {
        Ok((v, _)) => Some(v),
        Err(d) => {
            diags.push(d);
            None
        }
    }
}

/// `iconv_mime_decode_headers(string $headers, int $mode = 0, ?string $encoding = null): array|false`.
///
/// The headers in first-seen order, each with its values (a header that
/// appears once has one value; PHP makes the repeated ones an array).
#[allow(clippy::type_complexity)]
pub fn mime_decode_headers(
    s: &[u8],
    mode: i64,
    charset: &[u8],
    diags: &mut Vec<Diag>,
) -> Option<Vec<(Vec<u8>, Vec<Vec<u8>>)>> {
    let (cs, flags) = output_charset(charset, diags)?;
    let mut headers: Vec<(Vec<u8>, Vec<Vec<u8>>)> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        // A blank line ends the headers.
        if s[i] == b'\n' || (s[i] == b'\r' && s.get(i + 1) == Some(&b'\n')) {
            break;
        }
        let line_end = s[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(s.len(), |p| i + p);
        let Some(colon) = s[i..line_end]
            .iter()
            .position(|&b| b == b':')
            .map(|p| i + p)
        else {
            // No name: skip the line.
            i = line_end + 1;
            continue;
        };
        let name = s[i..colon].to_vec();
        let mut v = colon + 1;
        while v < s.len() && is_ws(s[v]) {
            v += 1;
        }
        let (value, end) = match decode_field(s, v, mode, charset, cs, flags) {
            Ok(r) => r,
            Err(d) => {
                diags.push(d);
                return None;
            }
        };
        match headers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, vals)) => vals.push(value),
            None => headers.push((name, vec![value])),
        }
        i = end;
    }
    Some(headers)
}
