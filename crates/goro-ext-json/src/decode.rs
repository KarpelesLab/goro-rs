//! `json_decode` and `json_validate`: an iterative RFC 8259 parser with
//! PHP's error reporting.
//!
//! Error reporting (observed on PHP 8.5, see the fixture): the text is read
//! token by token, left to right, and the first problem wins.
//!
//! - A token that cannot be read is a lexical error: a control character
//!   (below U+0020, NUL included) outside or inside a string, or the end
//!   of the text inside a string, is `JSON_ERROR_CTRL_CHAR`; malformed
//!   UTF-8 is `JSON_ERROR_UTF8` (also outside strings, where well-formed
//!   non-ASCII text is a syntax error); a bad escape is `JSON_ERROR_SYNTAX`;
//!   a lone or mismatched UTF-16 surrogate escape is `JSON_ERROR_UTF16`.
//! - A token in the wrong place is `JSON_ERROR_SYNTAX`, except the wrong
//!   closing bracket where the right one would be accepted (`[}`, `{]`,
//!   `[1}`, `{"a":1]`), which is `JSON_ERROR_STATE_MISMATCH`. A string
//!   token in the wrong place is still read in full first, so its own
//!   lexical error wins.
//! - Opening an array or object at nesting level `depth` (the top-level
//!   value is level 0) is `JSON_ERROR_DEPTH`, raised at the bracket.
//! - In object mode, a member whose name starts with NUL is
//!   `JSON_ERROR_INVALID_PROPERTY_NAME`, raised as soon as the member's
//!   value has been read (tests ext/json/tests/bug68546.phpt).
//!
//! Numbers follow the JSON grammar; a token ends where the grammar stops
//! matching (`01` is `0` then `1`, `1.` is `1` then `.`). Integers that do
//! not fit in an int become floats, or strings with
//! `JSON_BIGINT_AS_STRING` (tests ext/json/tests/008.phpt, bug63737.phpt);
//! `-0` is the int 0 and `-0.0` the float -0.

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::{JsonError, JsonHost, Utf8Mode};

/// Options of [`decode`].
#[derive(Clone, Debug)]
pub struct DecodeOptions {
    /// Objects become arrays (`$associative` / `JSON_OBJECT_AS_ARRAY`).
    pub assoc: bool,
    /// The `$depth` argument (at least 1).
    pub depth: u32,
    /// `JSON_BIGINT_AS_STRING`.
    pub bigint_as_string: bool,
    /// `JSON_INVALID_UTF8_IGNORE` / `JSON_INVALID_UTF8_SUBSTITUTE`.
    pub invalid_utf8: Utf8Mode,
}

/// Decodes `json`. Objects are created with [`JsonHost::new_object`]
/// unless `opts.assoc`.
pub fn decode<H: JsonHost + ?Sized>(
    host: &mut H,
    json: &[u8],
    opts: &DecodeOptions,
) -> Result<Value, JsonError> {
    let mut p = Parser::new(json, opts.depth, opts.invalid_utf8);
    p.bigint_as_string = opts.bigint_as_string;
    p.assoc = opts.assoc;
    p.run::<true, H>(Some(host))
}

/// Checks `json` without building values (`json_validate`).
pub fn validate(json: &[u8], depth: u32, mode: Utf8Mode) -> JsonError {
    let mut p = Parser::new(json, depth, mode);
    // Observed on PHP 8.5: json_validate('{"\u0000":1}') is true; member
    // names only matter when objects are built.
    p.assoc = true;
    match p.run::<false, NoHost>(None) {
        Ok(_) => JsonError::None,
        Err(e) => e,
    }
}

/// Placeholder host for validation (never called).
struct NoHost;

impl JsonHost for NoHost {
    type Exception = ();
    fn object_kind(&mut self, _: &goro_runtime::ObjRef) -> crate::ObjectKind {
        crate::ObjectKind::Plain
    }
    fn json_serialize(&mut self, _: &goro_runtime::ObjRef) -> Result<Value, ()> {
        Ok(Value::Null)
    }
    fn object_properties(&mut self, _: &goro_runtime::ObjRef) -> Result<PhpArray, ()> {
        Ok(PhpArray::new())
    }
    fn new_object(&mut self, _: PhpArray) -> Value {
        Value::Null
    }
    fn json_exception(&mut self, _: JsonError) {}
    fn throw_error(&mut self, _: goro_runtime::PhpError) {}
}

#[derive(Clone, Copy)]
struct Frame {
    obj: bool,
    /// Start of this container's values in `Parser::vals`.
    vals: u32,
    /// Start of this container's keys in `Parser::keys` (objects).
    keys: u32,
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    max_depth: u32,
    utf8: Utf8Mode,
    bigint_as_string: bool,
    assoc: bool,
    stack: Vec<Frame>,
    vals: Vec<Value>,
    keys: Vec<Key>,
    /// Scratch for strings with escapes.
    buf: Vec<u8>,
    /// Whether the last key read starts with NUL (object mode check).
    key_nul: Vec<bool>,
    cache: KeyCache,
}

/// Where a decoded string's bytes are.
enum Str {
    /// `s[a..b]`, no escapes.
    Raw(usize, usize),
    /// In `Parser::buf`.
    Buf,
}

#[inline]
fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

impl<'a> Parser<'a> {
    fn new(s: &'a [u8], max_depth: u32, utf8: Utf8Mode) -> Parser<'a> {
        Parser {
            s,
            pos: 0,
            max_depth,
            utf8,
            bigint_as_string: false,
            assoc: false,
            stack: Vec::new(),
            vals: Vec::new(),
            keys: Vec::new(),
            buf: Vec::new(),
            key_nul: Vec::new(),
            cache: KeyCache::new(),
        }
    }

    #[inline]
    fn skip_ws(&mut self) {
        while self.pos < self.s.len() && is_ws(self.s[self.pos]) {
            self.pos += 1;
        }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    /// The error for the token at `pos`, which the grammar does not accept
    /// here: its own lexical error if it has one, else a syntax error.
    #[cold]
    fn unexpected(&mut self) -> JsonError {
        let Some(c) = self.peek() else {
            return JsonError::Syntax;
        };
        match c {
            b'"' => match self.string::<false>() {
                Err(e) => e,
                Ok(_) => JsonError::Syntax,
            },
            0..=0x1f => JsonError::CtrlChar,
            0x80.. => {
                if utf8_len(self.s, self.pos).is_some() {
                    JsonError::Syntax
                } else {
                    JsonError::Utf8
                }
            }
            _ => JsonError::Syntax,
        }
    }

    fn run<const BUILD: bool, H: JsonHost + ?Sized>(
        &mut self,
        mut host: Option<&mut H>,
    ) -> Result<Value, JsonError> {
        'value: loop {
            // A value is expected at `pos`.
            self.skip_ws();
            let Some(c) = self.peek() else {
                return Err(JsonError::Syntax);
            };
            match c {
                b'[' | b'{' => {
                    if self.stack.len() as u64 + 1 >= self.max_depth as u64 {
                        return Err(JsonError::Depth);
                    }
                    self.pos += 1;
                    let obj = c == b'{';
                    self.stack.push(Frame {
                        obj,
                        vals: self.vals.len() as u32,
                        keys: self.keys.len() as u32,
                    });
                    self.skip_ws();
                    match self.peek() {
                        Some(b']') if !obj => {
                            self.pos += 1;
                            self.close::<BUILD, H>(&mut host);
                        }
                        Some(b'}') if obj => {
                            self.pos += 1;
                            self.close::<BUILD, H>(&mut host);
                        }
                        Some(b'}') | Some(b']') => return Err(JsonError::StateMismatch),
                        Some(b'"') if obj => {
                            self.key::<BUILD>()?;
                            continue 'value;
                        }
                        _ if obj => return Err(self.unexpected()),
                        _ => continue 'value,
                    }
                }
                b'"' => {
                    let t = self.string::<BUILD>()?;
                    if BUILD {
                        let v = Value::Str(self.str_value(t));
                        self.vals.push(v);
                    }
                }
                b'-' | b'0'..=b'9' => {
                    let v = self.number::<BUILD>()?;
                    if BUILD {
                        self.vals.push(v);
                    }
                }
                b't' | b'f' | b'n' => {
                    let (lit, v): (&[u8], Value) = match c {
                        b't' => (b"true", Value::Bool(true)),
                        b'f' => (b"false", Value::Bool(false)),
                        _ => (b"null", Value::Null),
                    };
                    if !self.s[self.pos..].starts_with(lit) {
                        return Err(JsonError::Syntax);
                    }
                    self.pos += lit.len();
                    if BUILD {
                        self.vals.push(v);
                    }
                }
                _ => return Err(self.unexpected()),
            }
            // A value is complete: what may follow depends on the container.
            loop {
                let Some(top) = self.stack.last().copied() else {
                    self.skip_ws();
                    if self.pos == self.s.len() {
                        return Ok(if BUILD {
                            self.vals.pop().unwrap_or_default()
                        } else {
                            Value::Null
                        });
                    }
                    return Err(self.unexpected());
                };
                if top.obj && !self.assoc && self.key_nul.pop() == Some(true) {
                    return Err(JsonError::InvalidPropertyName);
                }
                self.skip_ws();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                        if top.obj {
                            self.skip_ws();
                            if self.peek() != Some(b'"') {
                                return Err(self.unexpected());
                            }
                            self.key::<BUILD>()?;
                        }
                        continue 'value;
                    }
                    Some(b']') if !top.obj => {
                        self.pos += 1;
                        self.close::<BUILD, H>(&mut host);
                    }
                    Some(b'}') if top.obj => {
                        self.pos += 1;
                        self.close::<BUILD, H>(&mut host);
                    }
                    Some(b']') | Some(b'}') => return Err(JsonError::StateMismatch),
                    _ => return Err(self.unexpected()),
                }
            }
        }
    }

    /// Reads a member name and the colon after it.
    fn key<const BUILD: bool>(&mut self) -> Result<(), JsonError> {
        let t = self.string::<BUILD>()?;
        if BUILD {
            let bytes = match t {
                Str::Raw(a, b) => &self.s[a..b],
                Str::Buf => &self.buf[..],
            };
            if !self.assoc {
                self.key_nul.push(bytes.first() == Some(&0));
            }
            let k = self.cache.key(bytes);
            self.keys.push(k);
        }
        self.skip_ws();
        if self.peek() != Some(b':') {
            return Err(self.unexpected());
        }
        self.pos += 1;
        Ok(())
    }

    /// Ends the innermost container, leaving its value in `vals`.
    fn close<const BUILD: bool, H: JsonHost + ?Sized>(&mut self, host: &mut Option<&mut H>) {
        let f = self.stack.pop().expect("open container");
        if !BUILD {
            return;
        }
        let vstart = f.vals as usize;
        let n = self.vals.len() - vstart;
        let v = if !f.obj {
            let mut a = PhpArray::with_capacity(n);
            if n > 0 {
                let d = a.make_mut();
                for v in self.vals.drain(vstart..) {
                    // Cannot overflow: keys are 0..n.
                    let _ = d.push(v);
                }
            }
            Value::Array(a)
        } else {
            let kstart = f.keys as usize;
            let mut a = PhpArray::with_capacity(n);
            if n > 0 {
                let d = a.make_mut();
                for (k, v) in self.keys.drain(kstart..).zip(self.vals.drain(vstart..)) {
                    d.insert(k, v);
                }
            }
            if self.assoc {
                Value::Array(a)
            } else {
                match host {
                    Some(h) => h.new_object(a),
                    None => Value::Null,
                }
            }
        };
        self.vals.push(v);
    }

    fn str_value(&self, t: Str) -> PhpStr {
        match t {
            Str::Raw(a, b) => PhpStr::new(&self.s[a..b]),
            Str::Buf => PhpStr::new(&self.buf),
        }
    }

    /// Reads a number token (`pos` at `-` or a digit).
    fn number<const BUILD: bool>(&mut self) -> Result<Value, JsonError> {
        let s = self.s;
        let start = self.pos;
        let mut i = start;
        let neg = s[i] == b'-';
        if neg {
            i += 1;
        }
        let digits_start = i;
        match s.get(i) {
            Some(b'0') => i += 1,
            Some(b'1'..=b'9') => {
                i += 1;
                while i < s.len() && s[i].is_ascii_digit() {
                    i += 1;
                }
            }
            _ => return Err(JsonError::Syntax),
        }
        let int_end = i;
        let mut float = false;
        if i + 1 < s.len() && s[i] == b'.' && s[i + 1].is_ascii_digit() {
            i += 2;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
            }
            float = true;
        }
        if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
            let mut j = i + 1;
            if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
                j += 1;
            }
            if j < s.len() && s[j].is_ascii_digit() {
                j += 1;
                while j < s.len() && s[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
                float = true;
            }
        }
        self.pos = i;
        if !BUILD {
            return Ok(Value::Null);
        }
        let text = &s[start..i];
        if !float {
            let digits = &s[digits_start..int_end];
            if digits.len() <= 18 {
                let mut v: i64 = 0;
                for &c in digits {
                    v = v * 10 + (c - b'0') as i64;
                }
                return Ok(Value::Int(if neg { -v } else { v }));
            }
            let mut v: u64 = 0;
            let mut ok = true;
            for &c in digits {
                match v
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((c - b'0') as u64))
                {
                    Some(x) => v = x,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                if !neg && v <= i64::MAX as u64 {
                    return Ok(Value::Int(v as i64));
                }
                if neg && v <= i64::MAX as u64 + 1 {
                    return Ok(Value::Int((v as i64).wrapping_neg()));
                }
            }
            if self.bigint_as_string {
                return Ok(Value::Str(PhpStr::new(text)));
            }
        }
        Ok(Value::Float(parse_f64(text)))
    }

    /// Reads a string token (`pos` at the opening quote). Without `BUILD`
    /// the contents are only checked.
    fn string<const BUILD: bool>(&mut self) -> Result<Str, JsonError> {
        let s = self.s;
        let start = self.pos + 1;
        let mut i = start;
        // Fast path: no escapes and no replaced bytes.
        loop {
            i = scan_plain(s, i);
            let Some(&c) = s.get(i) else {
                return Err(JsonError::CtrlChar);
            };
            match c {
                b'"' => {
                    self.pos = i + 1;
                    return Ok(Str::Raw(start, i));
                }
                b'\\' => break,
                0..=0x1f => return Err(JsonError::CtrlChar),
                _ => match utf8_len(s, i) {
                    Some(n) => i += n,
                    None if self.utf8 == Utf8Mode::Error => return Err(JsonError::Utf8),
                    None => break,
                },
            }
        }
        // Slow path: build in `buf`.
        self.buf.clear();
        if BUILD {
            self.buf.extend_from_slice(&s[start..i]);
        }
        let mut run = i;
        loop {
            i = scan_plain(s, i);
            let Some(&c) = s.get(i) else {
                return Err(JsonError::CtrlChar);
            };
            match c {
                b'"' => {
                    if BUILD {
                        self.buf.extend_from_slice(&s[run..i]);
                    }
                    self.pos = i + 1;
                    return Ok(Str::Buf);
                }
                b'\\' => {
                    if BUILD {
                        self.buf.extend_from_slice(&s[run..i]);
                    }
                    i = self.escape::<BUILD>(i)?;
                    run = i;
                }
                0..=0x1f => return Err(JsonError::CtrlChar),
                _ => match utf8_len(s, i) {
                    Some(n) => i += n,
                    None => {
                        if BUILD {
                            self.buf.extend_from_slice(&s[run..i]);
                        }
                        match self.utf8 {
                            Utf8Mode::Error => return Err(JsonError::Utf8),
                            Utf8Mode::Ignore => {}
                            Utf8Mode::Substitute => {
                                if BUILD {
                                    self.buf.extend_from_slice("\u{fffd}".as_bytes());
                                }
                            }
                        }
                        // Observed on PHP 8.5: each byte of a malformed
                        // sequence is dropped or replaced on its own
                        // ("\xe2\x82" gives two U+FFFD).
                        i += 1;
                        run = i;
                    }
                },
            }
        }
    }

    /// Reads the escape at `s[i] == '\\'`, appending its character to
    /// `buf`; returns the position after it.
    fn escape<const BUILD: bool>(&mut self, i: usize) -> Result<usize, JsonError> {
        let s = self.s;
        let c = match s.get(i + 1) {
            Some(&c) => c,
            None => return Err(JsonError::Syntax),
        };
        let ch = match c {
            b'"' => b'"',
            b'\\' => b'\\',
            b'/' => b'/',
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'u' => {
                let Some(u) = hex4(s, i + 2) else {
                    return Err(JsonError::Syntax);
                };
                let mut next = i + 6;
                let cp = match u {
                    0xD800..=0xDBFF => {
                        // Observed on PHP 8.5: a high surrogate must be
                        // followed by a complete low surrogate escape, or
                        // it is JSON_ERROR_UTF16 (even "\uD800\uDC0").
                        match (s.get(next), s.get(next + 1), hex4(s, next + 2)) {
                            (Some(b'\\'), Some(b'u'), Some(lo @ 0xDC00..=0xDFFF)) => {
                                next += 6;
                                0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00)
                            }
                            _ => return Err(JsonError::Utf16),
                        }
                    }
                    0xDC00..=0xDFFF => return Err(JsonError::Utf16),
                    u => u,
                };
                if BUILD {
                    push_utf8(&mut self.buf, cp);
                }
                return Ok(next);
            }
            _ => return Err(JsonError::Syntax),
        };
        if BUILD {
            self.buf.push(ch);
        }
        Ok(i + 2)
    }
}

fn hex4(s: &[u8], i: usize) -> Option<u32> {
    let h = s.get(i..i + 4)?;
    let mut v = 0u32;
    for &c in h {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        v = v << 4 | d as u32;
    }
    Some(v)
}

pub(crate) fn push_utf8(out: &mut Vec<u8>, cp: u32) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.extend_from_slice(&[0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8]);
    } else if cp < 0x10000 {
        out.extend_from_slice(&[
            0xE0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]);
    } else {
        out.extend_from_slice(&[
            0xF0 | (cp >> 18) as u8,
            0x80 | ((cp >> 12) & 0x3F) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]);
    }
}

/// Length of the well-formed UTF-8 sequence at `s[i]` (Unicode 16.0, table
/// 3-7), or `None`.
#[inline]
pub(crate) fn utf8_len(s: &[u8], i: usize) -> Option<usize> {
    let c = s[i];
    let cont = |j: usize, lo: u8, hi: u8| s.get(i + j).is_some_and(|&b| (lo..=hi).contains(&b));
    match c {
        0x00..=0x7F => Some(1),
        0xC2..=0xDF => cont(1, 0x80, 0xBF).then_some(2),
        0xE0 => (cont(1, 0xA0, 0xBF) && cont(2, 0x80, 0xBF)).then_some(3),
        0xED => (cont(1, 0x80, 0x9F) && cont(2, 0x80, 0xBF)).then_some(3),
        0xE1..=0xEF => (cont(1, 0x80, 0xBF) && cont(2, 0x80, 0xBF)).then_some(3),
        0xF0 => (cont(1, 0x90, 0xBF) && cont(2, 0x80, 0xBF) && cont(3, 0x80, 0xBF)).then_some(4),
        0xF4 => (cont(1, 0x80, 0x8F) && cont(2, 0x80, 0xBF) && cont(3, 0x80, 0xBF)).then_some(4),
        0xF1..=0xF3 => {
            (cont(1, 0x80, 0xBF) && cont(2, 0x80, 0xBF) && cont(3, 0x80, 0xBF)).then_some(4)
        }
        _ => None,
    }
}

/// Correctly rounded conversion of a JSON number (Rust's parser rounds to
/// nearest-even; overflow gives ±INF, observed on PHP 8.5 for `1e400`).
fn parse_f64(text: &[u8]) -> f64 {
    // JSON number syntax is a subset of what `f64::from_str` accepts.
    std::str::from_utf8(text)
        .ok()
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Index of the first byte at or after `i` that is `"`, `\`, below 0x20 or
/// above 0x7F, or `s.len()`.
#[inline]
fn scan_plain(s: &[u8], mut i: usize) -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline; loads are unaligned
        // and within bounds.
        unsafe {
            use std::arch::x86_64::*;
            let q = _mm_set1_epi8(b'"' as i8);
            let bs = _mm_set1_epi8(b'\\' as i8);
            let sp = _mm_set1_epi8(0x20);
            while i + 16 <= s.len() {
                let x = _mm_loadu_si128(s.as_ptr().add(i) as *const __m128i);
                // Signed compare: bytes >= 0x80 are negative, so `x < 0x20`
                // catches both control characters and non-ASCII.
                let m = _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(x, q), _mm_cmpeq_epi8(x, bs)),
                    _mm_cmplt_epi8(x, sp),
                );
                let mask = _mm_movemask_epi8(m) as u32;
                if mask != 0 {
                    return i + mask.trailing_zeros() as usize;
                }
                i += 16;
            }
        }
    }
    while i < s.len() {
        let c = s[i];
        if c == b'"' || c == b'\\' || !(0x20..0x80).contains(&c) {
            return i;
        }
        i += 1;
    }
    i
}

/// A small cache of member names, so the keys of repeated objects share one
/// string (and its cached hash).
struct KeyCache {
    slots: Vec<Option<PhpStr>>,
}

impl KeyCache {
    const SIZE: usize = 256;
    const MAX_LEN: usize = 24;

    fn new() -> KeyCache {
        KeyCache { slots: Vec::new() }
    }

    #[inline]
    fn key(&mut self, b: &[u8]) -> Key {
        if b.is_empty() || b.len() > Self::MAX_LEN {
            return Key::from_bytes(b);
        }
        if (b[0].is_ascii_digit() || b[0] == b'-')
            && let Some(i) = goro_runtime::array::canonical_int(b)
        {
            return Key::Int(i);
        }
        if self.slots.is_empty() {
            self.slots.resize(Self::SIZE, None);
        }
        // FNV-1a over the bytes picks the slot.
        let mut h: u32 = 0x811c_9dc5;
        for &c in b {
            h = (h ^ c as u32).wrapping_mul(0x0100_0193);
        }
        let slot = &mut self.slots[(h as usize) & (Self::SIZE - 1)];
        if let Some(s) = slot
            && s.as_bytes() == b
        {
            return Key::Str(s.clone());
        }
        let s = PhpStr::new(b);
        *slot = Some(s.clone());
        Key::Str(s)
    }
}
