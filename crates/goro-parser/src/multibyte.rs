//! zend.multibyte: scripts in an encoding other than the internal one.
//!
//! A port of the script-encoding parts of Zend/zend_multibyte.c and
//! zend_language_scanner.l. With zend.multibyte on, the encoding of a script
//! comes from a byte order mark (zend.detect_unicode), zend.script_encoding
//! or a leading `declare(encoding=...)`. The script is then converted before
//! scanning (the input filter) when the lexer cannot read it as is, or its
//! string literals and inline HTML are converted afterwards (the output
//! filter), so that they end up in the internal encoding.
//!
//! The encodings themselves are mbstring's: it installs a `Provider`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::token::{Token, TokenKind};

/// An encoding, as identified by the provider
pub type EncodingId = usize;

/// The encoding functions zend.multibyte needs (zend_multibyte_functions).
pub trait Provider {
    /// zend_multibyte_fetch_encoding()
    fn fetch(&self, name: &[u8]) -> Option<EncodingId>;
    /// Built-in encodings by name (UTF-8, UTF-16BE, ...) for BOM detection
    fn builtin(&self, name: &str) -> Option<EncodingId> {
        self.fetch(name.as_bytes())
    }
    /// zend_multibyte_check_lexer_compatibility()
    fn lexer_compatible(&self, enc: EncodingId) -> bool;
    /// zend_multibyte_encoding_converter(): `data` from `from` to `to`
    fn convert(&self, data: &[u8], to: EncodingId, from: EncodingId) -> Vec<u8>;
    /// zend_multibyte_encoding_detector() over `list`
    fn detect(&self, data: &[u8], list: &[EncodingId]) -> Option<EncodingId>;
    /// zend_multibyte_get_internal_encoding()
    fn internal_encoding(&self) -> Option<EncodingId>;
}

/// The zend.multibyte settings in effect.
pub struct Settings {
    pub provider: Rc<dyn Provider>,
    /// zend.detect_unicode
    pub detect_unicode: bool,
    /// zend.script_encoding
    pub script_encodings: Vec<EncodingId>,
}

thread_local! {
    static MULTIBYTE: RefCell<Option<Rc<Settings>>> = const { RefCell::new(None) };
}

/// Turn zend.multibyte on (Some) or off (None) for the scripts compiled next.
pub fn set_multibyte(settings: Option<Settings>) {
    MULTIBYTE.with(|m| *m.borrow_mut() = settings.map(Rc::new));
}

/// CG(multibyte)
pub fn multibyte_enabled() -> bool {
    MULTIBYTE.with(|m| m.borrow().is_some())
}

/// zend_multibyte_fetch_encoding() succeeds (for `declare(encoding=...)`).
pub fn encoding_supported(name: &[u8]) -> bool {
    MULTIBYTE.with(|m| {
        m.borrow()
            .as_ref()
            .is_some_and(|s| s.provider.fetch(name).is_some())
    })
}

/// A conversion between two encodings
#[derive(Clone, Copy, PartialEq)]
struct Filter {
    from: EncodingId,
    to: EncodingId,
}

/// The scanner's input and output filters (zend_multibyte_set_filter()).
#[derive(Clone, Copy, PartialEq)]
struct Filters {
    input: Option<Filter>,
    output: Option<Filter>,
}

const BOMS: &[(&[u8], &str)] = &[
    (b"\x00\x00\xfe\xff", "UTF-32BE"),
    (b"\xff\xfe\x00\x00", "UTF-32LE"),
    (b"\xfe\xff", "UTF-16BE"),
    (b"\xff\xfe", "UTF-16LE"),
    (b"\xef\xbb\xbf", "UTF-8"),
];

/// zend_multibyte_detect_utf_encoding(): UTF-16/32 without a BOM.
fn detect_utf_encoding(script: &[u8]) -> &'static str {
    let mut wchar_size = 2;
    let mut p = 0;
    while p + 2 < script.len() {
        let Some(z) = script[p..script.len() - 2].iter().position(|&b| b == 0) else {
            break;
        };
        let z = p + z;
        if script[z + 1] == 0 && script[z + 2] == 0 {
            wchar_size = 4;
            break;
        }
        p = z + 4;
    }
    let mut le = false;
    let mut p = 0;
    while p + wchar_size <= script.len() {
        if script[p] == 0 && script[p + wchar_size - 1] != 0 {
            le = false;
            break;
        } else if script[p] != 0 && script[p + wchar_size - 1] == 0 {
            le = true;
            break;
        }
        p += wchar_size;
    }
    match (wchar_size, le) {
        (2, true) => "UTF-16LE",
        (2, false) => "UTF-16BE",
        (_, true) => "UTF-32LE",
        _ => "UTF-32BE",
    }
}

/// zend_multibyte_detect_unicode(): the encoding a BOM (stripped from
/// `script`) or NUL bytes reveal.
fn detect_unicode(s: &Settings, script: &mut &[u8]) -> Option<EncodingId> {
    if script.len() < 4 {
        return None;
    }
    for (bom, name) in BOMS {
        if script.starts_with(bom) {
            *script = &script[bom.len()..];
            return s.provider.builtin(name);
        }
    }
    // NUL bytes: UTF-16/32, unless they come after __HALT_COMPILER();
    let pos1 = script.iter().position(|&b| b == 0)?;
    let mut pos2 = 0;
    let at = |i: usize| script.get(i).copied().unwrap_or(0);
    let skip_ws = |mut i: usize| {
        while matches!(at(i), b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }
        i
    };
    while pos1 - pos2 >= b"__HALT_COMPILER();".len() {
        let Some(u) = script[pos2..pos1].iter().position(|&b| b == b'_') else {
            break;
        };
        pos2 += u + 1;
        if script[pos2..]
            .get(..b"_HALT_COMPILER".len())
            .is_some_and(|w| w.eq_ignore_ascii_case(b"_HALT_COMPILER"))
        {
            let mut q = skip_ws(pos2 + b"_HALT_COMPILER".len());
            if at(q) == b'(' {
                q = skip_ws(q + 1);
                if at(q) == b')' {
                    q = skip_ws(q + 1);
                    if at(q) == b';' {
                        return None;
                    }
                }
            }
        }
    }
    s.provider.builtin(detect_utf_encoding(script))
}

/// zend_multibyte_find_script_encoding()
fn find_script_encoding(s: &Settings, script: &mut &[u8]) -> Option<EncodingId> {
    if s.detect_unicode {
        if let Some(e) = detect_unicode(s, script) {
            return Some(e);
        }
    }
    match s.script_encodings.len() {
        0 => None,
        1 => Some(s.script_encodings[0]),
        _ => s.provider.detect(script, &s.script_encodings),
    }
}

/// zend_multibyte_set_filter() for a script in `script_encoding`.
fn set_filter(s: &Settings, script_encoding: EncodingId) -> Filters {
    let p = &s.provider;
    let internal = p.internal_encoding();
    // The intermediate encoding is UTF-8.
    let utf8 = p.builtin("UTF-8");
    let none = Filters {
        input: None,
        output: None,
    };
    let via_utf8 = |to: EncodingId| match utf8 {
        Some(u) => Filters {
            input: Some(Filter {
                from: script_encoding,
                to: u,
            }),
            output: Some(Filter { from: u, to }),
        },
        None => none,
    };
    match internal {
        None => {
            if p.lexer_compatible(script_encoding) {
                none
            } else {
                via_utf8(script_encoding)
            }
        }
        Some(i) if i == script_encoding => {
            if p.lexer_compatible(script_encoding) {
                none
            } else {
                via_utf8(script_encoding)
            }
        }
        Some(i) => {
            let to_internal = Some(Filter {
                from: script_encoding,
                to: i,
            });
            if p.lexer_compatible(i) {
                Filters {
                    input: to_internal,
                    output: None,
                }
            } else if p.lexer_compatible(script_encoding) {
                Filters {
                    input: None,
                    output: to_internal,
                }
            } else {
                via_utf8(i)
            }
        }
    }
}

/// The encodings of the `declare(encoding=...)` statements a script starts
/// with (zend_handle_encoding_declaration() switches the filters for each).
fn leading_encoding_declares(src: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    let at = |i: usize| src.get(i).copied().unwrap_or(0);
    // A shebang line, then the open tag
    if src.starts_with(b"#!") {
        while i < src.len() && src[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    if !src
        .get(i..i + 5)
        .is_some_and(|t| t.eq_ignore_ascii_case(b"<?php"))
    {
        return out;
    }
    i += 5;
    let skip_trivia = |mut i: usize| loop {
        match at(i) {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'#' if at(i + 1) != b'[' => {
                while i < src.len() && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if at(i + 1) == b'/' => {
                while i < src.len() && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if at(i + 1) == b'*' => match src[i + 2..].windows(2).position(|w| w == b"*/") {
                Some(e) => i += 2 + e + 2,
                None => return src.len(),
            },
            _ => return i,
        }
    };
    let ident = |i: usize| {
        let mut j = i;
        while at(j).is_ascii_alphanumeric() || at(j) == b'_' || at(j) >= 0x80 {
            j += 1;
        }
        j
    };
    loop {
        i = skip_trivia(i);
        let e = ident(i);
        if !src[i..e].eq_ignore_ascii_case(b"declare") {
            return out;
        }
        i = skip_trivia(e);
        if at(i) != b'(' {
            return out;
        }
        i += 1;
        loop {
            i = skip_trivia(i);
            let e = ident(i);
            let name = &src[i..e];
            i = skip_trivia(e);
            if name.is_empty() || at(i) != b'=' {
                return out;
            }
            i = skip_trivia(i + 1);
            // The value: a quoted string, or anything up to `,` / `)`
            let q = at(i);
            if q == b'\'' || q == b'"' {
                let Some(end) = src[i + 1..].iter().position(|&b| b == q) else {
                    return out;
                };
                if name.eq_ignore_ascii_case(b"encoding") {
                    out.push(src[i + 1..i + 1 + end].to_vec());
                }
                i += end + 2;
            } else {
                while i < src.len() && !matches!(src[i], b',' | b')') {
                    i += 1;
                }
            }
            i = skip_trivia(i);
            match at(i) {
                b',' => i += 1,
                b')' => {
                    i += 1;
                    break;
                }
                _ => return out,
            }
        }
        i = skip_trivia(i);
        if at(i) != b';' {
            // A block declare: nothing can follow it at the start
            return out;
        }
        i += 1;
    }
}

/// A script ready for the parser.
pub struct Script {
    pub tokens: Vec<Token>,
    /// The scanned bytes (the script after the input filter)
    pub source: Vec<u8>,
}

/// Lex a script file: with zend.multibyte on, through the encoding filters.
pub fn lex_script(source: &[u8], skip_shebang: bool) -> Script {
    // (with whether a `declare` was lexed: scan_encoding_declares() finds
    // nothing else)
    let lex = |src: &[u8]| {
        let mut lexer = crate::Lexer::new(src);
        if skip_shebang {
            lexer.skip_shebang();
        }
        let tokens = lexer.tokenize();
        (tokens, lexer.saw_declare())
    };
    let scan_declares = |tokens: &[Token], declare: bool, settings: Option<&Settings>| {
        if declare {
            scan_encoding_declares(tokens, settings);
        } else {
            DECLARE_WARNINGS.with(|w| w.borrow_mut().clear());
        }
    };
    let settings = MULTIBYTE.with(|m| m.borrow().clone());
    let Some(s) = settings else {
        set_halt_offset_map(None);
        let (tokens, declare) = lex(source);
        scan_declares(&tokens, declare, None);
        return Script {
            tokens,
            source: source.to_vec(),
        };
    };
    // open_file_for_scanning()
    let mut org: &[u8] = source;
    let mut filters = find_script_encoding(&s, &mut org).map(|e| set_filter(&s, e));
    let apply = |f: &Filters, org: &[u8]| match f.input {
        // (the original buffer is scanned as is, BOM included)
        None => source.to_vec(),
        Some(c) => s.provider.convert(org, c.to, c.from),
    };
    let mut scanned = match &filters {
        Some(f) => apply(f, org),
        None => source.to_vec(),
    };
    // declare(encoding=...) re-scans with the declared encoding.
    for name in leading_encoding_declares(&scanned) {
        let Some(e) = s.provider.fetch(&name) else {
            continue;
        };
        let new = set_filter(&s, e);
        let old = filters.unwrap_or(Filters {
            input: None,
            output: None,
        });
        if old.input != new.input || (old.input.is_some() && old != new) {
            scanned = apply(&new, org);
        }
        filters = Some(new);
    }
    let (mut tokens, declare) = lex(&scanned);
    scan_declares(&tokens, declare, Some(&s));
    let Some(f) = filters else {
        set_halt_offset_map(None);
        return Script {
            tokens,
            source: scanned,
        };
    };
    if let Some(c) = f.output {
        let conv = |v: &mut Vec<u8>| *v = s.provider.convert(v, c.to, c.from);
        for t in &mut tokens {
            match &mut t.kind {
                TokenKind::ConstantString(v)
                | TokenKind::InterpolatedStringPart(v)
                | TokenKind::InterpolatedStringEnd(v)
                | TokenKind::InlineHtml(v)
                | TokenKind::Identifier(v)
                | TokenKind::Variable(v)
                | TokenKind::VariableVariable(v) => conv(v),
                _ => {}
            }
        }
    }
    // zend_get_scanned_file_offset(): __COMPILER_HALT_OFFSET__ is an offset
    // into the original script.
    set_halt_offset_map(f.input.map(|c| {
        let org = org.to_vec();
        let provider = s.provider.clone();
        Rc::new(move |off: u32| -> u32 {
            let len = |n: usize| provider.convert(&org[..n], c.to, c.from).len();
            let (mut lo, mut hi) = (0usize, org.len());
            while lo < hi {
                let mid = (lo + hi) / 2;
                if len(mid) < off as usize {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo as u32
        }) as Rc<dyn Fn(u32) -> u32>
    }));
    Script {
        tokens,
        source: scanned,
    }
}

thread_local! {
    static HALT_OFFSET_MAP: RefCell<Option<Rc<dyn Fn(u32) -> u32>>> = const { RefCell::new(None) };
}

fn set_halt_offset_map(m: Option<Rc<dyn Fn(u32) -> u32>>) {
    HALT_OFFSET_MAP.with(|h| *h.borrow_mut() = m);
}

/// The map from offsets in the script last lexed by `lex_script()` to
/// offsets in the original script, if they differ (taken by the parser).
pub(crate) fn take_halt_offset_map() -> Option<Rc<dyn Fn(u32) -> u32>> {
    HALT_OFFSET_MAP.with(|h| h.borrow_mut().take())
}

thread_local! {
    static DECLARE_WARNINGS: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
}

/// zend_handle_encoding_declaration(), run by the parser on every
/// `declare(...)` with an encoding: its compile warnings.
fn scan_encoding_declares(tokens: &[Token], settings: Option<&Settings>) {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = tokens[i..]
        .iter()
        .position(|t| matches!(t.kind, TokenKind::Declare))
    {
        i += at;
        if !matches!(
            tokens.get(i + 1).map(|t| &t.kind),
            Some(TokenKind::OpenParen)
        ) {
            i += 1;
            continue;
        }
        // (name = value, ...): the values of the encoding directives,
        // when they are a single token
        let mut j = i + 2;
        let mut found: Vec<Option<&Token>> = Vec::new();
        let mut depth = 0;
        let mut start = j;
        let mut is_encoding = false;
        while j < tokens.len() {
            match &tokens[j].kind {
                TokenKind::OpenParen | TokenKind::OpenBracket => depth += 1,
                TokenKind::CloseParen | TokenKind::CloseBracket if depth > 0 => depth -= 1,
                TokenKind::Comma | TokenKind::CloseParen if depth == 0 => {
                    if is_encoding {
                        found.push((j == start + 3).then(|| &tokens[start + 2]));
                    }
                    if tokens[j].kind == TokenKind::CloseParen {
                        break;
                    }
                    start = j + 1;
                }
                TokenKind::Identifier(n) if j == start => {
                    is_encoding = n.eq_ignore_ascii_case(b"encoding");
                }
                _ if j == start => is_encoding = false,
                _ => {}
            }
            j += 1;
        }
        let line = tokens.get(j).map_or(0, |t| t.span.line);
        for v in found {
            // A non-literal value is a compile error (the compiler's)
            let literal = v.and_then(|v| match &v.kind {
                TokenKind::ConstantString(s) => Some(Some(s)),
                TokenKind::LongNumber(_) | TokenKind::DoubleNumber(_) => Some(None),
                _ => None,
            });
            let Some(name) = literal else { break };
            match settings {
                None => out.push((
                    "declare(encoding=...) ignored because Zend multibyte feature is turned off by settings".to_string(),
                    line,
                )),
                Some(s) => {
                    if let Some(name) = name {
                        if s.provider.fetch(name).is_none() {
                            out.push((
                                format!("Unsupported encoding [{}]", String::from_utf8_lossy(name)),
                                line,
                            ));
                        }
                    }
                }
            }
        }
        i = j;
    }
    DECLARE_WARNINGS.with(|w| *w.borrow_mut() = out);
}

/// The compile warnings `declare(encoding=...)` raised while the script
/// last lexed by `lex_script()` was parsed.
pub fn take_declare_warnings() -> Vec<(String, u32)> {
    DECLARE_WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}
