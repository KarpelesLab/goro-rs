//! The sanitizing filters, and the SAPI's `filter.default`.
//!
//! Character sets observed on PHP 8.5 for every byte and flag combination
//! (the fixture's sanitizer section; tests 021.phpt-028.phpt, 033.phpt,
//! 043.phpt, 059.phpt, bug69202.phpt, bug69203.phpt).

use goro_ext_standard::html;
use goro_runtime::{Level, PhpStr, Value};

use crate::apply::Outcome;
use crate::consts::*;
use crate::{Filter, FilterHost};

const STRIP_ENCODE: i64 = FILTER_FLAG_STRIP_LOW
    | FILTER_FLAG_STRIP_HIGH
    | FILTER_FLAG_STRIP_BACKTICK
    | FILTER_FLAG_ENCODE_LOW
    | FILTER_FLAG_ENCODE_HIGH
    | FILTER_FLAG_ENCODE_AMP;

thread_local! {
    static BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Builds a string in a reused buffer (one allocation for the result).
fn build(f: impl FnOnce(&mut Vec<u8>)) -> PhpStr {
    BUF.with(|b| match b.try_borrow_mut() {
        Ok(mut buf) => {
            buf.clear();
            f(&mut buf);
            PhpStr::new(&buf)
        }
        Err(_) => {
            let mut v = Vec::new();
            f(&mut v);
            PhpStr::new(&v)
        }
    })
}

#[inline]
fn push_entity(out: &mut Vec<u8>, c: u8) {
    out.extend_from_slice(b"&#");
    if c >= 100 {
        out.push(b'0' + c / 100);
    }
    if c >= 10 {
        out.push(b'0' + c / 10 % 10);
    }
    out.extend_from_slice(&[b'0' + c % 10, b';']);
}

/// Whether the strip flags remove `c`: FILTER_FLAG_STRIP_LOW for bytes
/// below 32, FILTER_FLAG_STRIP_HIGH for 127 and above, and
/// FILTER_FLAG_STRIP_BACKTICK for '`'.
#[inline]
fn stripped(c: u8, flags: i64) -> bool {
    (flags & FILTER_FLAG_STRIP_LOW != 0 && c < 32)
        || (flags & FILTER_FLAG_STRIP_HIGH != 0 && c >= 127)
        || (flags & FILTER_FLAG_STRIP_BACKTICK != 0 && c == b'`')
}

/// The strip flags, then FILTER_FLAG_ENCODE_LOW / _HIGH / _AMP as
/// `&#NN;` (the strip flags win over the encode flags).
fn strip_encode(s: &[u8], flags: i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if stripped(c, flags) {
            continue;
        }
        if (flags & FILTER_FLAG_ENCODE_LOW != 0 && c < 32)
            || (flags & FILTER_FLAG_ENCODE_HIGH != 0 && c >= 127)
            || (flags & FILTER_FLAG_ENCODE_AMP != 0 && c == b'&')
        {
            push_entity(&mut out, c);
        } else {
            out.push(c);
        }
    }
    out
}

/// FILTER_SANITIZE_STRING's tag removal: everything from a '<' to the
/// matching '>' (nested '<' count; inside a tag a quote runs to the same
/// quote, '>' included; "<?" runs to "?>"), and NUL bytes; a '>' outside a
/// tag stays.
fn strip_tags(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut depth = 0usize;
    let mut quote = 0u8;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        i += 1;
        if depth == 0 && c == b'<' && s.get(i) == Some(&b'?') {
            // "<?>" is complete: the '?' after '<' may be the one of "?>".
            match s[i..].windows(2).position(|w| w == b"?>") {
                Some(p) => i += p + 2,
                None => i = s.len(),
            }
            continue;
        }
        if depth > 0 {
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
                continue;
            }
            match c {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                b'"' | b'\'' => quote = c,
                _ => {}
            }
            continue;
        }
        match c {
            b'<' => depth = 1,
            0 => {}
            _ => out.push(c),
        }
    }
    out
}

fn string_value(s: &PhpStr, out: Vec<u8>) -> Value {
    if out[..] == s[..] {
        Value::Str(s.clone())
    } else {
        Value::Str(PhpStr::new(&out))
    }
}

fn empty_null(v: Value, flags: i64) -> Value {
    match &v {
        Value::Str(s) if s.is_empty() && flags & FILTER_FLAG_EMPTY_STRING_NULL != 0 => Value::Null,
        _ => v,
    }
}

/// Runs a sanitizing filter.
pub(crate) fn run<H: FilterHost>(host: &mut H, filter: Filter, flags: i64, s: &PhpStr) -> Outcome {
    let keep = |f: &dyn Fn(u8) -> bool| -> Value {
        if s.iter().all(|&c| f(c)) {
            Value::Str(s.clone())
        } else {
            let out: Vec<u8> = s.iter().copied().filter(|&c| f(c)).collect();
            Value::Str(PhpStr::new(&out))
        }
    };
    Outcome::Value(match filter {
        Filter::UnsafeRaw => {
            // FILTER_FLAG_EMPTY_STRING_NULL looks at the input here ("\x01"
            // with FILTER_FLAG_STRIP_LOW stays "", test bug53037.phpt and
            // observed on PHP 8.5).
            if s.is_empty() && flags & FILTER_FLAG_EMPTY_STRING_NULL != 0 {
                Value::Null
            } else if flags & STRIP_ENCODE == 0 {
                Value::Str(s.clone())
            } else {
                string_value(s, strip_encode(s, flags))
            }
        }
        Filter::String => {
            // The strip/encode flags apply first, then quotes are encoded
            // (unless FILTER_FLAG_NO_ENCODE_QUOTES), then tags go; an empty
            // result is null with FILTER_FLAG_EMPTY_STRING_NULL.
            let mut out = strip_encode(s, flags);
            if flags & FILTER_FLAG_NO_ENCODE_QUOTES == 0
                && out.iter().any(|&c| c == b'"' || c == b'\'')
            {
                let mut q = Vec::with_capacity(out.len() + 8);
                for &c in &out {
                    match c {
                        b'"' | b'\'' => push_entity(&mut q, c),
                        _ => q.push(c),
                    }
                }
                out = q;
            }
            empty_null(string_value(s, strip_tags(&out)), flags)
        }
        Filter::Encoded => {
            let mut out = Vec::with_capacity(s.len());
            for &c in s.iter() {
                if stripped(c, flags) {
                    continue;
                }
                // 0xFF is left as is (observed on PHP 8.5 for every byte).
                if c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_' || c == 0xFF {
                    out.push(c);
                } else {
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    out.extend_from_slice(&[b'%', HEX[(c >> 4) as usize], HEX[(c & 15) as usize]]);
                }
            }
            string_value(s, out)
        }
        Filter::SpecialChars => {
            let high = flags & FILTER_FLAG_ENCODE_HIGH != 0;
            let encode = |c: u8| {
                c < 32 || matches!(c, b'"' | b'&' | b'\'' | b'<' | b'>') || (high && c >= 127)
            };
            if !s.iter().any(|&c| encode(c) || stripped(c, flags)) {
                Value::Str(s.clone())
            } else {
                Value::Str(build(|out| {
                    for &c in s.iter() {
                        if stripped(c, flags) {
                            continue;
                        }
                        if encode(c) {
                            push_entity(out, c);
                        } else {
                            out.push(c);
                        }
                    }
                }))
            }
        }
        Filter::FullSpecialChars => {
            // htmlentities() with ENT_QUOTES (ENT_NOQUOTES with
            // FILTER_FLAG_NO_ENCODE_QUOTES), ini default_charset, no double
            // encoding; invalid input gives "" (observed on PHP 8.5: "é" is
            // "&eacute;", "&amp;" stays, "\xff" is "").
            let mut diags = Vec::new();
            let cs = html::resolve_charset(None, &host.default_charset(), &mut diags);
            let q = if flags & FILTER_FLAG_NO_ENCODE_QUOTES != 0 {
                html::ENT_NOQUOTES
            } else {
                html::ENT_QUOTES
            };
            let out = html::htmlentities(s, q | html::ENT_HTML401, cs, false, &mut diags);
            for d in diags {
                if let goro_ext_standard::error::Diag::Warning(m) = d {
                    host.notice(Level::Warning, m);
                }
            }
            string_value(s, out)
        }
        Filter::SanitizeEmail => {
            keep(&|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-=?^_`{|}~@.[]".contains(&c))
        }
        Filter::SanitizeUrl => keep(&|c| (0x21..=0x7E).contains(&c)),
        Filter::NumberInt => keep(&|c| c.is_ascii_digit() || c == b'+' || c == b'-'),
        Filter::NumberFloat => keep(&|c| {
            c.is_ascii_digit()
                || c == b'+'
                || c == b'-'
                || (flags & FILTER_FLAG_ALLOW_FRACTION != 0 && c == b'.')
                || (flags & FILTER_FLAG_ALLOW_THOUSAND != 0 && c == b',')
                || (flags & FILTER_FLAG_ALLOW_SCIENTIFIC != 0 && (c == b'e' || c == b'E'))
        }),
        Filter::AddSlashes => string_value(s, goro_ext_standard::strings::addslashes(s)),
        _ => unreachable!("not a sanitizer"),
    })
}

/// ini `filter.default` / `filter.default_flags`: the filter the SAPI runs
/// on each incoming GET, POST and COOKIE value (and the server variables it
/// registers) before storing it in the superglobals (tests 004.phpt,
/// 005.phpt, 041.phpt). `filter_input()` still sees the raw values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputFilter {
    pub filter: Filter,
    pub flags: i64,
}

impl InputFilter {
    /// From the ini values: an unknown name is unsafe_raw; without
    /// `filter.default_flags` the flags are FILTER_FLAG_NO_ENCODE_QUOTES
    /// (PHP manual: Filter, runtime configuration).
    pub fn from_ini(default: &[u8], default_flags: Option<i64>) -> InputFilter {
        let filter = crate::filter_id(default)
            .and_then(Filter::from_id)
            .unwrap_or(Filter::UnsafeRaw);
        InputFilter {
            filter,
            flags: default_flags.unwrap_or(FILTER_FLAG_NO_ENCODE_QUOTES),
        }
    }

    /// Whether values pass through unchanged (FILTER_UNSAFE_RAW without
    /// strip/encode flags), so the SAPI can skip filtering.
    pub fn is_raw(&self) -> bool {
        self.filter == Filter::UnsafeRaw && self.flags & STRIP_ENCODE == 0
    }

    /// Whether setting `filter.default` to this value raises PHP 8.5's
    /// startup deprecation "The filter.default ini setting is deprecated"
    /// (test filter_default_deprecation.phpt): any filter but unsafe_raw.
    pub fn deprecated(default: &[u8]) -> bool {
        crate::filter_id(default).is_some_and(|id| id != FILTER_UNSAFE_RAW)
    }

    /// Filters one incoming value.
    pub fn apply<H: FilterHost>(&self, host: &mut H, raw: &PhpStr) -> Result<Value, H::Exception> {
        crate::filter_var(
            host,
            &Value::Str(raw.clone()),
            self.filter.id(),
            crate::Options::Int(self.flags),
        )
    }
}
