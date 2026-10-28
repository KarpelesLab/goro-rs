//! ext/mbstring basics (PHP manual: Multibyte String functions): the
//! Unicode encodings (UTF-8/16/32, UCS-2/4), ASCII, 8bit/7bit and the
//! single-byte charsets whose tables goro_ext_standard has, with
//! string length/position/case/width functions and conversions.
//! Behaviours observed on PHP 8.5 and in tests ext/mbstring/tests.

use goro_ext_standard::html::tables;
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::string::sv;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "mbstring",
    functions: &[
        (
            "function mb_internal_encoding(?string $encoding = null): string|bool",
            mb_internal_encoding,
        ),
        (
            "function mb_strlen(string $string, ?string $encoding = null): int",
            mb_strlen,
        ),
        (
            "function mb_substr(string $string, int $start, ?int $length = null, ?string $encoding = null): string",
            mb_substr,
        ),
        (
            "function mb_strcut(string $string, int $start, ?int $length = null, ?string $encoding = null): string",
            mb_strcut,
        ),
        (
            "function mb_strpos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_strpos,
        ),
        (
            "function mb_stripos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_stripos,
        ),
        (
            "function mb_strrpos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_strrpos,
        ),
        (
            "function mb_strripos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_strripos,
        ),
        (
            "function mb_strstr(string $haystack, string $needle, bool $before_needle = false, ?string $encoding = null): string|false",
            mb_strstr,
        ),
        (
            "function mb_stristr(string $haystack, string $needle, bool $before_needle = false, ?string $encoding = null): string|false",
            mb_stristr,
        ),
        (
            "function mb_strrchr(string $haystack, string $needle, bool $before_needle = false, ?string $encoding = null): string|false",
            mb_strrchr,
        ),
        (
            "function mb_strrichr(string $haystack, string $needle, bool $before_needle = false, ?string $encoding = null): string|false",
            mb_strrichr,
        ),
        (
            "function mb_substr_count(string $haystack, string $needle, ?string $encoding = null): int",
            mb_substr_count,
        ),
        (
            "function mb_strtoupper(string $string, ?string $encoding = null): string",
            mb_strtoupper,
        ),
        (
            "function mb_strtolower(string $string, ?string $encoding = null): string",
            mb_strtolower,
        ),
        (
            "function mb_convert_case(string $string, int $mode, ?string $encoding = null): string",
            mb_convert_case,
        ),
        (
            "function mb_ucfirst(string $string, ?string $encoding = null): string",
            mb_ucfirst,
        ),
        (
            "function mb_lcfirst(string $string, ?string $encoding = null): string",
            mb_lcfirst,
        ),
        (
            "function mb_str_split(string $string, int $length = 1, ?string $encoding = null): array",
            mb_str_split,
        ),
        (
            "function mb_strwidth(string $string, ?string $encoding = null): int",
            mb_strwidth,
        ),
        (
            "function mb_strimwidth(string $string, int $start, int $width, string $trim_marker = \"\", ?string $encoding = null): string",
            mb_strimwidth,
        ),
        (
            "function mb_convert_encoding(array|string $string, string $to_encoding, array|string|null $from_encoding = null): array|string|false",
            mb_convert_encoding,
        ),
        (
            "function mb_check_encoding(array|string|null $value = null, ?string $encoding = null): bool",
            mb_check_encoding,
        ),
        (
            "function mb_detect_encoding(string $string, array|string|null $encodings = null, bool $strict = false): string|false",
            mb_detect_encoding,
        ),
        (
            "function mb_detect_order(array|string|null $encoding = null): array|bool",
            mb_detect_order,
        ),
        (
            "function mb_substitute_character(string|int|null $substitute_character = null): string|int|bool",
            mb_substitute_character,
        ),
        (
            "function mb_ord(string $string, ?string $encoding = null): int|false",
            mb_ord,
        ),
        (
            "function mb_chr(int $codepoint, ?string $encoding = null): string|false",
            mb_chr,
        ),
        (
            "function mb_scrub(string $string, ?string $encoding = null): string",
            mb_scrub,
        ),
        (
            "function mb_str_pad(string $string, int $length, string $pad_string = \" \", int $pad_type = STR_PAD_RIGHT, ?string $encoding = null): string",
            mb_str_pad,
        ),
        (
            "function mb_trim(string $string, ?string $characters = null, ?string $encoding = null): string",
            mb_trim,
        ),
        (
            "function mb_ltrim(string $string, ?string $characters = null, ?string $encoding = null): string",
            mb_ltrim,
        ),
        (
            "function mb_rtrim(string $string, ?string $characters = null, ?string $encoding = null): string",
            mb_rtrim,
        ),
        ("function mb_list_encodings(): array", mb_list_encodings),
        (
            "function mb_encoding_aliases(string $encoding): array",
            mb_encoding_aliases,
        ),
        (
            "function mb_preferred_mime_name(string $encoding): string|false",
            mb_preferred_mime_name,
        ),
        (
            "function mb_language(?string $language = null): string|bool",
            mb_language,
        ),
        (
            "function mb_encode_numericentity(string $string, array $map, ?string $encoding = null, bool $hex = false): string",
            mb_encode_numericentity,
        ),
        (
            "function mb_decode_numericentity(string $string, array $map, ?string $encoding = null): string",
            mb_decode_numericentity,
        ),
        (
            "function mb_parse_str(string $string, &$result): bool",
            mb_parse_str,
        ),
        (
            "function mb_output_handler(string $string, int $status): string",
            mb_output_handler,
        ),
    ],
    constants: &[
        ("MB_CASE_UPPER", ConstVal::Int(0)),
        ("MB_CASE_LOWER", ConstVal::Int(1)),
        ("MB_CASE_TITLE", ConstVal::Int(2)),
        ("MB_CASE_FOLD", ConstVal::Int(3)),
        ("MB_CASE_UPPER_SIMPLE", ConstVal::Int(4)),
        ("MB_CASE_LOWER_SIMPLE", ConstVal::Int(5)),
        ("MB_CASE_TITLE_SIMPLE", ConstVal::Int(6)),
        ("MB_CASE_FOLD_SIMPLE", ConstVal::Int(7)),
        ("MB_ONIGURUMA_VERSION", ConstVal::Str("6.9.10")),
    ],
    ini: &[
        ("mbstring.language", "neutral"),
        ("mbstring.internal_encoding", ""),
        ("mbstring.http_input", ""),
        ("mbstring.http_output", ""),
        ("mbstring.detect_order", ""),
        ("mbstring.substitute_character", ""),
        ("mbstring.encoding_translation", "0"),
        (
            "mbstring.http_output_conv_mimetypes",
            "^(text/|application/xhtml\\+xml)",
        ),
        ("mbstring.strict_detection", "0"),
        ("mbstring.regex_retry_limit", "1000000"),
        ("mbstring.regex_stack_limit", "100000"),
    ],
    listed: true,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- encodings

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Utf8,
    Ascii,
    SevenBit,
    EightBit,
    Latin1,
    Sbcs(&'static [u16; 128]),
    Utf16 { be: bool, bom: bool },
    Utf32 { be: bool, bom: bool },
    Ucs2 { be: bool, bom: bool },
}

struct Enc {
    name: &'static str,
    aliases: &'static [&'static str],
    mime: &'static str,
    kind: Kind,
}

static ENCODINGS: &[Enc] = &[
    Enc {
        name: "7bit",
        aliases: &[],
        mime: "7bit",
        kind: Kind::SevenBit,
    },
    Enc {
        name: "8bit",
        aliases: &["binary"],
        mime: "8bit",
        kind: Kind::EightBit,
    },
    Enc {
        name: "UCS-4",
        aliases: &["ISO-10646-UCS-4", "UCS4"],
        mime: "UCS-4",
        kind: Kind::Utf32 {
            be: true,
            bom: true,
        },
    },
    Enc {
        name: "UCS-4BE",
        aliases: &["byte4be"],
        mime: "UCS-4BE",
        kind: Kind::Utf32 {
            be: true,
            bom: false,
        },
    },
    Enc {
        name: "UCS-4LE",
        aliases: &["byte4le"],
        mime: "UCS-4LE",
        kind: Kind::Utf32 {
            be: false,
            bom: false,
        },
    },
    Enc {
        name: "UCS-2",
        aliases: &["ISO-10646-UCS-2", "UCS2", "UNICODE"],
        mime: "UCS-2",
        kind: Kind::Ucs2 {
            be: true,
            bom: true,
        },
    },
    Enc {
        name: "UCS-2BE",
        aliases: &["byte2be"],
        mime: "UCS-2BE",
        kind: Kind::Ucs2 {
            be: true,
            bom: false,
        },
    },
    Enc {
        name: "UCS-2LE",
        aliases: &["byte2le"],
        mime: "UCS-2LE",
        kind: Kind::Ucs2 {
            be: false,
            bom: false,
        },
    },
    Enc {
        name: "UTF-32",
        aliases: &["utf32"],
        mime: "UTF-32",
        kind: Kind::Utf32 {
            be: true,
            bom: true,
        },
    },
    Enc {
        name: "UTF-32BE",
        aliases: &[],
        mime: "UTF-32BE",
        kind: Kind::Utf32 {
            be: true,
            bom: false,
        },
    },
    Enc {
        name: "UTF-32LE",
        aliases: &[],
        mime: "UTF-32LE",
        kind: Kind::Utf32 {
            be: false,
            bom: false,
        },
    },
    Enc {
        name: "UTF-16",
        aliases: &["utf16"],
        mime: "UTF-16",
        kind: Kind::Utf16 {
            be: true,
            bom: true,
        },
    },
    Enc {
        name: "UTF-16BE",
        aliases: &[],
        mime: "UTF-16BE",
        kind: Kind::Utf16 {
            be: true,
            bom: false,
        },
    },
    Enc {
        name: "UTF-16LE",
        aliases: &[],
        mime: "UTF-16LE",
        kind: Kind::Utf16 {
            be: false,
            bom: false,
        },
    },
    Enc {
        name: "UTF-8",
        aliases: &["utf8"],
        mime: "UTF-8",
        kind: Kind::Utf8,
    },
    Enc {
        name: "ASCII",
        aliases: &[
            "ANSI_X3.4-1968",
            "iso-ir-6",
            "ANSI_X3.4-1986",
            "ISO_646.irv:1991",
            "US-ASCII",
            "ISO646-US",
            "us",
            "IBM367",
            "IBM-367",
            "cp367",
            "csASCII",
        ],
        mime: "US-ASCII",
        kind: Kind::Ascii,
    },
    Enc {
        name: "Windows-1252",
        aliases: &["cp1252"],
        mime: "Windows-1252",
        kind: Kind::Sbcs(&tables::CP1252),
    },
    Enc {
        name: "ISO-8859-1",
        aliases: &["ISO8859-1", "latin1"],
        mime: "ISO-8859-1",
        kind: Kind::Latin1,
    },
    Enc {
        name: "ISO-8859-5",
        aliases: &["ISO8859-5", "cyrillic"],
        mime: "ISO-8859-5",
        kind: Kind::Sbcs(&tables::ISO_8859_5),
    },
    Enc {
        name: "ISO-8859-15",
        aliases: &["ISO8859-15"],
        mime: "ISO-8859-15",
        kind: Kind::Sbcs(&tables::ISO_8859_15),
    },
    Enc {
        name: "Windows-1251",
        aliases: &["CP1251", "CP-1251", "WINDOWS-1251"],
        mime: "Windows-1251",
        kind: Kind::Sbcs(&tables::CP1251),
    },
    Enc {
        name: "CP866",
        aliases: &["CP-866", "IBM866", "IBM-866"],
        mime: "CP866",
        kind: Kind::Sbcs(&tables::CP866),
    },
    Enc {
        name: "KOI8-R",
        aliases: &["KOI8R"],
        mime: "KOI8-R",
        kind: Kind::Sbcs(&tables::KOI8_R),
    },
];

fn lookup(name: &[u8]) -> Option<&'static Enc> {
    ENCODINGS.iter().find(|e| {
        e.name.as_bytes().eq_ignore_ascii_case(name)
            || e.aliases
                .iter()
                .any(|a| a.as_bytes().eq_ignore_ascii_case(name))
    })
}

/// A decoded character: its code point (None for an invalid sequence) and
/// its byte span.
#[derive(Clone, Copy)]
struct Ch {
    cp: Option<u32>,
    start: usize,
    len: usize,
}

fn decode(k: Kind, s: &[u8]) -> Vec<Ch> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    let push = |out: &mut Vec<Ch>, cp, start, len| out.push(Ch { cp, start, len });
    match k {
        Kind::Utf8 => {
            while i < s.len() {
                let (cp, n) = utf8_char(&s[i..]);
                push(&mut out, cp, i, n);
                i += n;
            }
        }
        Kind::Ascii | Kind::SevenBit => {
            for (i, &b) in s.iter().enumerate() {
                push(&mut out, (b < 0x80).then_some(b as u32), i, 1);
            }
        }
        Kind::EightBit | Kind::Latin1 => {
            for (i, &b) in s.iter().enumerate() {
                push(&mut out, Some(b as u32), i, 1);
            }
        }
        Kind::Sbcs(t) => {
            for (i, &b) in s.iter().enumerate() {
                let cp = if b < 0x80 {
                    Some(b as u32)
                } else {
                    let u = t[(b - 0x80) as usize];
                    (u != 0xFFFF).then_some(u as u32)
                };
                push(&mut out, cp, i, 1);
            }
        }
        Kind::Utf16 { be, bom } | Kind::Ucs2 { be, bom } => {
            let ucs2 = matches!(k, Kind::Ucs2 { .. });
            let mut be = be;
            if bom && s.len() >= 2 {
                if s[0] == 0xFE && s[1] == 0xFF {
                    be = true;
                    i = 2;
                } else if s[0] == 0xFF && s[1] == 0xFE {
                    be = false;
                    i = 2;
                }
            }
            let rd = |p: usize| -> u32 {
                if be {
                    ((s[p] as u32) << 8) | s[p + 1] as u32
                } else {
                    ((s[p + 1] as u32) << 8) | s[p] as u32
                }
            };
            while i < s.len() {
                if i + 2 > s.len() {
                    push(&mut out, None, i, s.len() - i);
                    break;
                }
                let u = rd(i);
                if !ucs2 && (0xD800..0xDC00).contains(&u) {
                    if i + 4 <= s.len() {
                        let l = rd(i + 2);
                        if (0xDC00..0xE000).contains(&l) {
                            push(
                                &mut out,
                                Some(0x10000 + ((u - 0xD800) << 10) + (l - 0xDC00)),
                                i,
                                4,
                            );
                            i += 4;
                            continue;
                        }
                    }
                    push(&mut out, None, i, 2);
                    i += 2;
                    continue;
                }
                if !ucs2 && (0xDC00..0xE000).contains(&u) {
                    push(&mut out, None, i, 2);
                } else {
                    push(&mut out, Some(u), i, 2);
                }
                i += 2;
            }
        }
        Kind::Utf32 { be, bom } => {
            let mut be = be;
            if bom && s.len() >= 4 {
                if s[..4] == [0, 0, 0xFE, 0xFF] {
                    be = true;
                    i = 4;
                } else if s[..4] == [0xFF, 0xFE, 0, 0] {
                    be = false;
                    i = 4;
                }
            }
            while i < s.len() {
                if i + 4 > s.len() {
                    push(&mut out, None, i, s.len() - i);
                    break;
                }
                let b: [u8; 4] = s[i..i + 4].try_into().unwrap();
                let u = if be {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                };
                let ok = u <= 0x10FFFF && !(0xD800..0xE000).contains(&u);
                push(&mut out, ok.then_some(u), i, 4);
                i += 4;
            }
        }
    }
    out
}

/// One UTF-8 character; an invalid sequence is its maximal valid prefix
/// (Unicode "U+FFFD substitution of maximal subparts"; test
/// illformed_utf_sequences.phpt).
fn utf8_char(s: &[u8]) -> (Option<u32>, usize) {
    let b = s[0];
    let (n, lo, hi, init) = match b {
        0x00..=0x7F => return (Some(b as u32), 1),
        0xC2..=0xDF => (2, 0x80, 0xBF, (b & 0x1F) as u32),
        0xE0 => (3, 0xA0, 0xBF, 0),
        0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80, 0xBF, (b & 0x0F) as u32),
        0xED => (3, 0x80, 0x9F, 0x0D),
        0xF0 => (4, 0x90, 0xBF, 0),
        0xF1..=0xF3 => (4, 0x80, 0xBF, (b & 0x07) as u32),
        0xF4 => (4, 0x80, 0x8F, 4),
        _ => return (None, 1),
    };
    let mut cp = init;
    for k in 1..n {
        let (l, h) = if k == 1 { (lo, hi) } else { (0x80, 0xBF) };
        match s.get(k) {
            Some(&c) if (l..=h).contains(&c) => cp = (cp << 6) | (c & 0x3F) as u32,
            _ => return (None, k),
        }
    }
    (Some(cp), n)
}

/// Encodes one code point; false when the encoding can't represent it.
fn encode(k: Kind, cp: u32, out: &mut Vec<u8>) -> bool {
    match k {
        Kind::Utf8 => {
            let Some(c) = char::from_u32(cp) else {
                return false;
            };
            let mut b = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            true
        }
        Kind::Ascii | Kind::SevenBit => {
            if cp < 0x80 {
                out.push(cp as u8);
                true
            } else {
                false
            }
        }
        Kind::EightBit | Kind::Latin1 => {
            if cp < 0x100 {
                out.push(cp as u8);
                true
            } else {
                false
            }
        }
        Kind::Sbcs(t) => {
            if cp < 0x80 {
                out.push(cp as u8);
                return true;
            }
            match t.iter().position(|&u| u as u32 == cp && u != 0xFFFF) {
                Some(p) => {
                    out.push(0x80 + p as u8);
                    true
                }
                None => false,
            }
        }
        Kind::Utf16 { be, .. } | Kind::Ucs2 { be, .. } => {
            let mut put =
                |u: u16| out.extend_from_slice(&if be { u.to_be_bytes() } else { u.to_le_bytes() });
            if cp < 0x10000 {
                if (0xD800..0xE000).contains(&cp) {
                    return false;
                }
                put(cp as u16);
                true
            } else if cp <= 0x10FFFF && matches!(k, Kind::Utf16 { .. }) {
                let v = cp - 0x10000;
                put(0xD800 + (v >> 10) as u16);
                put(0xDC00 + (v & 0x3FF) as u16);
                true
            } else {
                false
            }
        }
        Kind::Utf32 { be, .. } => {
            if cp > 0x10FFFF || (0xD800..0xE000).contains(&cp) {
                return false;
            }
            out.extend_from_slice(&if be {
                cp.to_be_bytes()
            } else {
                cp.to_le_bytes()
            });
            true
        }
    }
}

// ---------------------------------------------------------------- state

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sub {
    None,
    Long,
    Entity,
    Char(u32),
}

struct MbState {
    internal: &'static Enc,
    set_internal: Option<&'static Enc>,
    sub: Sub,
    detect: Vec<&'static Enc>,
    language: &'static str,
}

impl Default for MbState {
    fn default() -> MbState {
        MbState {
            internal: lookup(b"UTF-8").unwrap(),
            set_internal: None,
            sub: Sub::Char(0x3F),
            detect: vec![lookup(b"ASCII").unwrap(), lookup(b"UTF-8").unwrap()],
            language: "neutral",
        }
    }
}

/// The internal encoding: set by mb_internal_encoding(), else the
/// mbstring.internal_encoding setting, else default_charset (PHP manual:
/// mbstring runtime configuration).
fn internal(vm: &mut Vm) -> &'static Enc {
    if let Some(e) = vm.ext::<MbState>().set_internal {
        return e;
    }
    for k in [
        &b"mbstring.internal_encoding"[..],
        b"internal_encoding",
        b"default_charset",
    ] {
        if let Some(v) = vm.get_ini(k)
            && !v.is_empty()
            && let Some(e) = lookup(&v)
        {
            return e;
        }
    }
    vm.ext::<MbState>().internal
}

/// The `$encoding` argument `i` (the internal encoding when null).
fn enc_arg(vm: &mut Vm, a: &Args, i: usize) -> R<&'static Enc> {
    match a.opt_str(i) {
        None => Ok(internal(vm)),
        Some(n) => match lookup(&n) {
            Some(e) => Ok(e),
            None => vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #{} ($encoding) must be a valid encoding, \"{}\" given",
                    i + 1,
                    n.to_string_lossy()
                ),
            ),
        },
    }
}

/// Encodes characters; invalid input and unrepresentable characters use
/// the substitute character (observed on PHP 8.5: "long" prints U+XXXX
/// for unrepresentable characters and "?" for invalid input).
fn encode_all(vm: &mut Vm, k: Kind, cps: impl Iterator<Item = Option<u32>>) -> Vec<u8> {
    let sub = vm.ext::<MbState>().sub;
    let mut out = Vec::new();
    let put_sub = |out: &mut Vec<u8>, cp: Option<u32>| match sub {
        Sub::None => {}
        Sub::Char(c) => {
            if !encode(k, c, out) {
                encode(k, 0x3F, out);
            }
        }
        Sub::Long => match cp {
            Some(c) => {
                for b in format!("U+{:X}", c).bytes() {
                    encode(k, b as u32, out);
                }
            }
            None => {
                encode(k, 0x3F, out);
            }
        },
        Sub::Entity => match cp {
            Some(c) => {
                for b in format!("&#x{:X};", c).bytes() {
                    encode(k, b as u32, out);
                }
            }
            None => {
                encode(k, 0x3F, out);
            }
        },
    };
    for cp in cps {
        match cp {
            Some(c) => {
                if !encode(k, c, &mut out) {
                    put_sub(&mut out, Some(c));
                }
            }
            None => put_sub(&mut out, None),
        }
    }
    out
}

// ---------------------------------------------------------------- functions

fn mb_internal_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.opt_str(0) {
        None => Ok(Value::from(internal(vm).name)),
        Some(n) => match lookup(&n) {
            Some(e) => {
                vm.ext::<MbState>().set_internal = Some(e);
                Ok(Value::Bool(true))
            }
            None => vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                    n.to_string_lossy()
                ),
            ),
        },
    }
}

fn mb_strlen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let s = a.bytes(0);
    if e.kind == Kind::Utf8 && s.is_ascii() {
        return Ok(Value::Int(s.len() as i64));
    }
    Ok(Value::Int(decode(e.kind, s).len() as i64))
}

fn char_range(n: usize, start: i64, len: Option<i64>) -> (usize, usize) {
    let r = goro_ext_standard::strings::substr_range(n, start, len);
    (r.start, r.end)
}

fn mb_substr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    let s = a.str(0);
    let chars = decode(e.kind, &s);
    let (st, en) = char_range(chars.len(), a.int(1), a.opt_int(2));
    if chars[st..en].iter().all(|c| c.cp.is_some()) {
        if st >= en {
            return Ok(Value::Str(PhpStr::empty()));
        }
        let b0 = chars[st].start;
        let b1 = chars[en - 1].start + chars[en - 1].len;
        return Ok(Value::from(&s[b0..b1]));
    }
    let out = encode_all(vm, e.kind, chars[st..en].iter().map(|c| c.cp));
    Ok(sv(out))
}

fn mb_strcut(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    let s = a.str(0);
    let n = s.len() as i64;
    let mut start = a.int(1);
    if start < 0 {
        start = (n + start).max(0);
    }
    if start > n {
        return Ok(Value::Str(PhpStr::empty()));
    }
    let mut end = match a.opt_int(2) {
        None => n,
        Some(l) if l < 0 => (n + l).max(start),
        Some(l) => (start + l).min(n),
    };
    let chars = decode(e.kind, &s);
    // Both ends move back to the start of their character.
    let align = |p: i64| -> i64 {
        let mut best = 0;
        for c in &chars {
            if c.start as i64 <= p {
                best = c.start as i64;
            } else {
                break;
            }
        }
        if p >= n { n } else { best }
    };
    let st = align(start);
    if end < n {
        // The last character must fit entirely.
        let mut last = st;
        for c in &chars {
            if (c.start + c.len) as i64 <= end && c.start as i64 >= st {
                last = (c.start + c.len) as i64;
            }
        }
        end = last;
    }
    Ok(Value::from(&s[st as usize..end.max(st) as usize]))
}

/// Code points of a string (None for invalid sequences).
fn cps(k: Kind, s: &[u8]) -> Vec<Option<u32>> {
    decode(k, s).into_iter().map(|c| c.cp).collect()
}

fn fold_cp(c: Option<u32>) -> Option<u32> {
    let c = char::from_u32(c?)?;
    let mut l = c.to_lowercase();
    let f = l.next()?;
    Some(if l.next().is_some() {
        c as u32
    } else {
        f as u32
    })
}

fn find_seq(h: &[Option<u32>], n: &[Option<u32>], from: usize) -> Option<usize> {
    if n.is_empty() {
        return (from <= h.len()).then_some(from);
    }
    if n.len() > h.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()] == *n)
}

fn rfind_seq(h: &[Option<u32>], n: &[Option<u32>], upto: usize) -> Option<usize> {
    if n.len() > h.len() {
        return None;
    }
    let last = (h.len() - n.len()).min(upto);
    (0..=last).rev().find(|&i| h[i..i + n.len()] == *n)
}

fn pos_common(vm: &mut Vm, a: &mut Args, ci: bool, rev: bool) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    let mut h = cps(e.kind, a.bytes(0));
    let mut n = cps(e.kind, a.bytes(1));
    if ci {
        h = h.into_iter().map(fold_cp).collect();
        n = n.into_iter().map(fold_cp).collect();
    }
    let len = h.len() as i64;
    let off = a.int_or(2, 0);
    if off > len || off < -len {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        );
    }
    let r = if !rev {
        let from = if off < 0 { len + off } else { off } as usize;
        find_seq(&h, &n, from)
    } else if off >= 0 {
        rfind_seq(&h, &n, usize::MAX).filter(|&p| p as i64 >= off)
    } else {
        let upto = (len + off) as usize;
        rfind_seq(&h, &n, upto)
    };
    Ok(match r {
        Some(p) => Value::Int(p as i64),
        None => Value::Bool(false),
    })
}

fn mb_strpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, false, false)
}
fn mb_stripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, true, false)
}
fn mb_strrpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, false, true)
}
fn mb_strripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, true, true)
}

fn str_common(vm: &mut Vm, a: &mut Args, ci: bool, rev: bool) -> R<Value> {
    let e = enc_arg(vm, a, 3)?;
    let s = a.str(0);
    let chars = decode(e.kind, &s);
    let mut h: Vec<Option<u32>> = chars.iter().map(|c| c.cp).collect();
    let mut n = cps(e.kind, a.bytes(1));
    if ci {
        h = h.into_iter().map(fold_cp).collect();
        n = n.into_iter().map(fold_cp).collect();
    }
    let p = if rev {
        if n.is_empty() {
            return Ok(Value::Bool(false));
        }
        rfind_seq(&h, &n, usize::MAX)
    } else {
        find_seq(&h, &n, 0)
    };
    let Some(p) = p else {
        return Ok(Value::Bool(false));
    };
    let bp = chars.get(p).map_or(s.len(), |c| c.start);
    Ok(if a.bool_or(2, false) {
        Value::from(&s[..bp])
    } else {
        Value::from(&s[bp..])
    })
}

fn mb_strstr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, false, false)
}
fn mb_stristr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, true, false)
}
fn mb_strrchr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, false, true)
}
fn mb_strrichr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    str_common(vm, a, true, true)
}

fn mb_substr_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    if a.bytes(1).is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #2 ($needle) must not be empty");
    }
    let h = cps(e.kind, a.bytes(0));
    let n = cps(e.kind, a.bytes(1));
    let mut c = 0;
    let mut p = 0;
    while let Some(i) = find_seq(&h, &n, p) {
        c += 1;
        p = i + n.len();
    }
    Ok(Value::Int(c))
}

// ---------------------------------------------------------------- case

fn is_cased(c: char) -> bool {
    c.is_lowercase() || c.is_uppercase() || matches!(c as u32, 0x1C5 | 0x1C8 | 0x1CB | 0x1F2)
}

/// Case-ignorable characters for title casing: apostrophes, periods,
/// colons, middle dots and combining marks (Unicode Word_Break MidLetter /
/// MidNumLet and Mn).
fn is_case_ignorable(c: char) -> bool {
    matches!(
        c,
        '\'' | '.'
            | ':'
            | '^'
            | '`'
            | '\u{B7}'
            | '\u{2019}'
            | '\u{2018}'
            | '\u{2024}'
            | '\u{FE52}'
            | '\u{FF07}'
            | '\u{FF0E}'
            | '\u{AD}'
    ) || ('\u{300}'..='\u{36F}').contains(&c)
}

fn titlecase(c: char, out: &mut String, simple: bool) {
    match c as u32 {
        0x1C4..=0x1C6 => out.push('\u{1C5}'),
        0x1C7..=0x1C9 => out.push('\u{1C8}'),
        0x1CA..=0x1CC => out.push('\u{1CB}'),
        0x1F1..=0x1F3 => out.push('\u{1F2}'),
        // Greek letters with ypogegrammeni title-case to their
        // prosgegrammeni forms (Unicode SpecialCasing; test bug69267.phpt).
        0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7 => {
            out.push(char::from_u32(c as u32 + 8).unwrap())
        }
        0x1F88..=0x1F8F | 0x1F98..=0x1F9F | 0x1FA8..=0x1FAF | 0x1FBC | 0x1FCC | 0x1FFC => {
            out.push(c)
        }
        0x1FB3 => out.push('\u{1FBC}'),
        0x1FC3 => out.push('\u{1FCC}'),
        0x1FF3 => out.push('\u{1FFC}'),
        0xDF if !simple => out.push_str("Ss"),
        _ => {
            if simple {
                let mut u = c.to_uppercase();
                match (u.next(), u.next()) {
                    (Some(x), None) => out.push(x),
                    _ => out.push(c),
                }
            } else {
                out.extend(c.to_uppercase());
            }
        }
    }
}

fn convert_case(s: &str, mode: i64) -> String {
    let simple = mode >= 4;
    match mode & 3 {
        0 => {
            if simple {
                s.chars()
                    .map(|c| {
                        let mut u = c.to_uppercase();
                        match (u.next(), u.next()) {
                            (Some(x), None) => x,
                            _ => c,
                        }
                    })
                    .collect()
            } else {
                s.to_uppercase()
            }
        }
        1 => {
            if simple {
                s.chars()
                    .map(|c| {
                        let mut l = c.to_lowercase();
                        match (l.next(), l.next()) {
                            (Some(x), None) => x,
                            _ => c,
                        }
                    })
                    .collect()
            } else {
                s.to_lowercase()
            }
        }
        3 => {
            let mut out = String::with_capacity(s.len());
            for c in s.chars() {
                match c {
                    'ß' | 'ẞ' if !simple => out.push_str("ss"),
                    _ => {
                        let mut l = c.to_lowercase();
                        match (l.next(), l.next()) {
                            (Some(x), None) => out.push(x),
                            _ => out.push(c),
                        }
                    }
                }
            }
            out
        }
        _ => {
            let mut out = String::with_capacity(s.len());
            let mut in_word = false;
            let chars: Vec<char> = s.chars().collect();
            for (idx, &c) in chars.iter().enumerate() {
                if is_cased(c) {
                    if in_word && c == '\u{3A3}' && !simple {
                        // Final sigma (Unicode SpecialCasing; test
                        // mb_convert_case_various_mode.phpt).
                        let next_cased = chars[idx + 1..]
                            .iter()
                            .find(|&&x| !is_case_ignorable(x))
                            .is_some_and(|&x| is_cased(x));
                        out.push(if next_cased { 'σ' } else { 'ς' });
                    } else if in_word {
                        if simple {
                            let mut l = c.to_lowercase();
                            match (l.next(), l.next()) {
                                (Some(x), None) => out.push(x),
                                _ => out.push(c),
                            }
                        } else {
                            out.extend(c.to_lowercase());
                        }
                    } else {
                        titlecase(c, &mut out, simple);
                        in_word = true;
                    }
                } else {
                    out.push(c);
                    if !is_case_ignorable(c) {
                        in_word = false;
                    }
                }
            }
            out
        }
    }
}

/// Applies a case mapping through UTF-8 text (invalid sequences become
/// the substitute character).
fn case_map(vm: &mut Vm, k: Kind, s: &[u8], f: impl Fn(&str) -> String) -> Vec<u8> {
    let sub = match vm.ext::<MbState>().sub {
        Sub::Char(c) => char::from_u32(c),
        _ => Some('?'),
    };
    let mut text = String::with_capacity(s.len());
    for cp in cps(k, s) {
        match cp.and_then(char::from_u32) {
            Some(c) => text.push(c),
            None => {
                if let Some(c) = sub {
                    text.push(c);
                }
            }
        }
    }
    let mapped = f(&text);
    encode_all(vm, k, mapped.chars().map(|c| Some(c as u32)))
}

fn mb_strtoupper(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    Ok(sv(case_map(vm, e.kind, a.bytes(0), |t| convert_case(t, 0))))
}
fn mb_strtolower(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    Ok(sv(case_map(vm, e.kind, a.bytes(0), |t| convert_case(t, 1))))
}
fn mb_convert_case(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    let mode = a.int(1);
    if !(0..=7).contains(&mode) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be one of the MB_CASE_* constants",
        );
    }
    Ok(sv(case_map(vm, e.kind, a.bytes(0), |t| {
        convert_case(t, mode)
    })))
}

fn first_case(vm: &mut Vm, a: &mut Args, upper: bool) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    Ok(sv(case_map(vm, e.kind, a.bytes(0), |t| {
        let mut it = t.chars();
        let mut out = String::with_capacity(t.len());
        if let Some(c) = it.next() {
            if upper {
                titlecase(c, &mut out, false);
            } else {
                out.extend(c.to_lowercase());
            }
        }
        out.extend(it);
        out
    })))
}
fn mb_ucfirst(vm: &mut Vm, a: &mut Args) -> R<Value> {
    first_case(vm, a, true)
}
fn mb_lcfirst(vm: &mut Vm, a: &mut Args) -> R<Value> {
    first_case(vm, a, false)
}

fn mb_str_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let len = a.int_or(1, 1);
    if len < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0",
        );
    }
    let e = enc_arg(vm, a, 2)?;
    let s = a.str(0);
    let chars = decode(e.kind, &s);
    let mut out = PhpArray::new();
    for chunk in chars.chunks(len as usize) {
        let b0 = chunk[0].start;
        let last = chunk[chunk.len() - 1];
        let _ = out.push(Value::from(&s[b0..last.start + last.len]));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- width

/// East Asian Wide and Fullwidth characters (Unicode UAX #11).
fn width(cp: u32) -> usize {
    const WIDE: &[(u32, u32)] = &[
        (0x1100, 0x115F),
        (0x231A, 0x231B),
        (0x2329, 0x232A),
        (0x23E9, 0x23EC),
        (0x23F0, 0x23F0),
        (0x23F3, 0x23F3),
        (0x25FD, 0x25FE),
        (0x2614, 0x2615),
        (0x2648, 0x2653),
        (0x267F, 0x267F),
        (0x2693, 0x2693),
        (0x26A1, 0x26A1),
        (0x26AA, 0x26AB),
        (0x26BD, 0x26BE),
        (0x26C4, 0x26C5),
        (0x26CE, 0x26CE),
        (0x26D4, 0x26D4),
        (0x26EA, 0x26EA),
        (0x26F2, 0x26F3),
        (0x26F5, 0x26F5),
        (0x26FA, 0x26FA),
        (0x26FD, 0x26FD),
        (0x2705, 0x2705),
        (0x270A, 0x270B),
        (0x2728, 0x2728),
        (0x274C, 0x274C),
        (0x274E, 0x274E),
        (0x2753, 0x2755),
        (0x2757, 0x2757),
        (0x2795, 0x2797),
        (0x27B0, 0x27B0),
        (0x27BF, 0x27BF),
        (0x2B1B, 0x2B1C),
        (0x2B50, 0x2B50),
        (0x2B55, 0x2B55),
        (0x2E80, 0x303E),
        (0x3041, 0x33FF),
        (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF),
        (0xA000, 0xA4CF),
        (0xA960, 0xA97F),
        (0xAC00, 0xD7A3),
        (0xF900, 0xFAFF),
        (0xFE10, 0xFE19),
        (0xFE30, 0xFE6F),
        (0xFF00, 0xFF60),
        (0xFFE0, 0xFFE6),
        (0x16FE0, 0x16FE4),
        (0x17000, 0x18CFF),
        (0x1B000, 0x1B2FF),
        (0x1F004, 0x1F004),
        (0x1F0CF, 0x1F0CF),
        (0x1F18E, 0x1F18E),
        (0x1F191, 0x1F19A),
        (0x1F200, 0x1F202),
        (0x1F210, 0x1F23B),
        (0x1F240, 0x1F248),
        (0x1F250, 0x1F251),
        (0x1F260, 0x1F265),
        (0x1F300, 0x1F64F),
        (0x1F680, 0x1F6FF),
        (0x1F900, 0x1F9FF),
        (0x1FA70, 0x1FAFF),
        (0x20000, 0x2FFFD),
        (0x30000, 0x3FFFD),
    ];
    if WIDE.iter().any(|&(a, b)| (a..=b).contains(&cp)) {
        2
    } else {
        1
    }
}

fn mb_strwidth(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let w: usize = cps(e.kind, a.bytes(0))
        .into_iter()
        .map(|c| c.map_or(1, width))
        .sum();
    Ok(Value::Int(w as i64))
}

fn mb_strimwidth(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 4)?;
    let s = a.str(0);
    let chars = decode(e.kind, &s);
    let n = chars.len() as i64;
    let mut start = a.int(1);
    if start < 0 {
        start += n;
    }
    if start < 0 || start > n {
        return vm.fthrow(a, "ValueError", "Argument #2 ($start) is out of range");
    }
    let w = a.int(2);
    if w < 0 {
        return vm.fthrow(a, "ValueError", "Argument #3 ($width) is out of range");
    }
    let marker = if a.has(3) { a.str(3) } else { PhpStr::empty() };
    let mw: usize = cps(e.kind, &marker)
        .into_iter()
        .map(|c| c.map_or(1, width))
        .sum();
    let rest = &chars[start as usize..];
    let total: usize = rest.iter().map(|c| c.cp.map_or(1, width)).sum();
    let bytes_of = |cs: &[Ch]| -> Vec<u8> {
        if cs.is_empty() {
            return Vec::new();
        }
        s[cs[0].start..cs[cs.len() - 1].start + cs[cs.len() - 1].len].to_vec()
    };
    if total as i64 <= w {
        return Ok(sv(bytes_of(rest)));
    }
    let budget = (w as usize).saturating_sub(mw);
    let mut acc = 0;
    let mut k = 0;
    while k < rest.len() {
        let cw = rest[k].cp.map_or(1, width);
        if acc + cw > budget {
            break;
        }
        acc += cw;
        k += 1;
    }
    let mut out = bytes_of(&rest[..k]);
    out.extend_from_slice(&marker);
    Ok(sv(out))
}

// ---------------------------------------------------------------- conversion

fn from_list(vm: &mut Vm, a: &Args, v: &Value, argn: usize) -> R<Vec<&'static Enc>> {
    let names: Vec<PhpStr> = match v {
        Value::Null => return Ok(vec![vm.ext::<MbState>().internal]),
        Value::Array(arr) => {
            let mut out = Vec::new();
            for (_, x) in arr.iter() {
                out.push(vm.to_php_str(&x.deref())?);
            }
            out
        }
        other => {
            let s = vm.to_php_str(other)?;
            s.split(|&c| c == b',')
                .map(|p| PhpStr::from(p.trim_ascii()))
                .collect()
        }
    };
    let mut out = Vec::new();
    for n in names {
        if n.as_bytes().eq_ignore_ascii_case(b"auto") {
            out.extend(vm.ext::<MbState>().detect.clone());
            continue;
        }
        match lookup(&n) {
            Some(e) => out.push(e),
            None => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #{} ($from_encoding) contains invalid encoding \"{}\"",
                        argn,
                        n.to_string_lossy()
                    ),
                );
            }
        }
    }
    if out.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($from_encoding) must specify at least one encoding",
                argn
            ),
        );
    }
    Ok(out)
}

fn is_valid(k: Kind, s: &[u8]) -> bool {
    decode(k, s).iter().all(|c| c.cp.is_some())
}

fn convert_str(
    vm: &mut Vm,
    a: &Args,
    s: &[u8],
    to: &Enc,
    from: &[&'static Enc],
) -> R<Option<Vec<u8>>> {
    let src = if from.len() == 1 {
        from[0]
    } else {
        match from.iter().copied().find(|e| is_valid(e.kind, s)) {
            Some(e) => e,
            None => {
                // Test gh10627.phpt.
                vm.fwarn(a, "Unable to detect character encoding")?;
                return Ok(None);
            }
        }
    };
    Ok(Some(encode_all(vm, to.kind, cps(src.kind, s).into_iter())))
}

fn mb_convert_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let to_name = a.str(1);
    let Some(to) = lookup(&to_name) else {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #2 ($to_encoding) must be a valid encoding, \"{}\" given",
                to_name.to_string_lossy()
            ),
        );
    };
    let fv = if a.has(2) { a.val(2) } else { Value::Null };
    let from = from_list(vm, a, &fv, 3)?;
    match a.val(0) {
        Value::Array(arr) => {
            let mut out = PhpArray::new();
            for (k, v) in arr.iter() {
                let v = v.deref();
                let key = match k {
                    goro_runtime::KeyRef::Str(s) => match convert_str(vm, a, s, to, &from)? {
                        Some(b) => Key::from_php_str(&PhpStr::from(b)),
                        None => continue,
                    },
                    goro_runtime::KeyRef::Int(i) => Key::Int(i),
                };
                let nv = match &v {
                    Value::Str(s) => match convert_str(vm, a, s, to, &from)? {
                        Some(b) => sv(b),
                        None => continue,
                    },
                    Value::Object(_) => {
                        vm.fwarn(a, "Object is not supported")?;
                        continue;
                    }
                    other => other.clone(),
                };
                out.insert(key, nv);
            }
            Ok(Value::Array(out))
        }
        v => {
            let s = vm.to_php_str(&v)?;
            Ok(match convert_str(vm, a, &s, to, &from)? {
                Some(b) => sv(b),
                None => Value::Bool(false),
            })
        }
    }
}

fn mb_check_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    fn check(vm: &mut Vm, k: Kind, v: &Value, depth: usize) -> R<bool> {
        Ok(match v {
            Value::Str(s) => is_valid(k, s),
            Value::Array(arr) => {
                for (key, x) in arr.iter() {
                    if let goro_runtime::KeyRef::Str(s) = key
                        && !is_valid(k, s)
                    {
                        return Ok(false);
                    }
                    if depth < 64 && !check(vm, k, &x.deref(), depth + 1)? {
                        return Ok(false);
                    }
                }
                true
            }
            Value::Null => true,
            other => {
                let s = vm.to_php_str(other)?;
                is_valid(k, &s)
            }
        })
    }
    if !a.has(0) || a.val(0).is_null() {
        vm.fdeprecated(
            a,
            "Calling mb_check_encoding() without argument is deprecated",
        )?;
        return Ok(Value::Bool(true));
    }
    let v = a.val(0);
    Ok(Value::Bool(check(vm, e.kind, &v, 0)?))
}

fn mb_detect_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let list = match a.get(1).map(|v| v.deref()) {
        None | Some(Value::Null) => vm.ext::<MbState>().detect.clone(),
        Some(v) => from_list(vm, a, &v, 2)?,
    };
    let strict = a.bool_or(2, false);
    for e in &list {
        if is_valid(e.kind, &s) {
            return Ok(Value::from(e.name));
        }
    }
    if strict {
        return Ok(Value::Bool(false));
    }
    // The one with the fewest invalid sequences.
    let best = list
        .iter()
        .min_by_key(|e| decode(e.kind, &s).iter().filter(|c| c.cp.is_none()).count());
    Ok(match best {
        Some(e) => Value::from(e.name),
        None => Value::Bool(false),
    })
}

fn mb_detect_order(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.get(0).map(|v| v.deref()) {
        None | Some(Value::Null) => {
            let names: Vec<Value> = vm
                .ext::<MbState>()
                .detect
                .iter()
                .map(|e| Value::from(e.name))
                .collect();
            Ok(Value::Array(PhpArray::from_values(names)))
        }
        Some(v) => {
            let list = from_list(vm, a, &v, 1)?;
            vm.ext::<MbState>().detect = list;
            Ok(Value::Bool(true))
        }
    }
}

fn mb_substitute_character(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = if a.has(0) { a.val(0) } else { Value::Null };
    let bad = |vm: &mut Vm, a: &Args| -> R<Value> {
        vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($substitute_character) must be \"none\", \"long\", \"entity\" or a valid codepoint",
        )
    };
    match v {
        Value::Null => Ok(match vm.ext::<MbState>().sub {
            Sub::None => Value::from("none"),
            Sub::Long => Value::from("long"),
            Sub::Entity => Value::from("entity"),
            Sub::Char(c) => Value::Int(c as i64),
        }),
        Value::Str(s) => {
            let sub = match s.to_ascii_lowercase().as_slice() {
                b"none" => Sub::None,
                b"long" => Sub::Long,
                b"entity" => Sub::Entity,
                _ => match goro_runtime::numeric::numeric_value(&s) {
                    Some(goro_runtime::Number::Int(i)) if (0..=0x10FFFF).contains(&i) => {
                        Sub::Char(i as u32)
                    }
                    _ => return bad(vm, a),
                },
            };
            vm.ext::<MbState>().sub = sub;
            Ok(Value::Bool(true))
        }
        Value::Int(i) => {
            if !(0..=0x10FFFF).contains(&i) || (0xD800..0xE000).contains(&i) {
                return bad(vm, a);
            }
            vm.ext::<MbState>().sub = Sub::Char(i as u32);
            Ok(Value::Bool(true))
        }
        _ => bad(vm, a),
    }
}

fn mb_ord(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    if a.bytes(0).is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($string) must not be empty");
    }
    Ok(
        match decode(e.kind, a.bytes(0)).first().and_then(|c| c.cp) {
            Some(c) => Value::Int(c as i64),
            None => Value::Bool(false),
        },
    )
}

fn mb_chr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    let c = a.int(0);
    if !(0..=0x10FFFF).contains(&c) || (0xD800..0xE000).contains(&c) {
        return Ok(Value::Bool(false));
    }
    let mut out = Vec::new();
    Ok(if encode(e.kind, c as u32, &mut out) {
        sv(out)
    } else {
        Value::Bool(false)
    })
}

fn mb_scrub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 1)?;
    Ok(sv(encode_all(
        vm,
        e.kind,
        cps(e.kind, a.bytes(0)).into_iter(),
    )))
}

fn mb_str_pad(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 4)?;
    let s = a.str(0);
    let pad: PhpStr = if a.has(2) {
        a.str(2)
    } else {
        PhpStr::from(" ")
    };
    if pad.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($pad_string) must not be empty",
        );
    }
    let ty = a.int_or(3, 1);
    if !(0..=2).contains(&ty) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH",
        );
    }
    let n = decode(e.kind, &s).len() as i64;
    let len = a.int(1);
    if len <= n {
        return Ok(Value::Str(s));
    }
    let pchars = decode(e.kind, &pad);
    let fill = |count: usize| -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..count {
            let c = pchars[i % pchars.len()];
            out.extend_from_slice(&pad[c.start..c.start + c.len]);
        }
        out
    };
    let total = (len - n) as usize;
    let (l, r) = match ty {
        0 => (total, 0),
        1 => (0, total),
        _ => (total / 2, total - total / 2),
    };
    let mut out = fill(l);
    out.extend_from_slice(&s);
    out.extend(fill(r));
    Ok(sv(out))
}

fn trim_common(vm: &mut Vm, a: &mut Args, left: bool, right: bool) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    let s = a.str(0);
    let chars = decode(e.kind, &s);
    let set: Vec<Option<u32>> = match a.opt_str(1) {
        Some(c) => cps(e.kind, &c),
        // PHP 8.4 mb_trim default: whitespace and NUL (PHP manual: mb_trim).
        None => [
            0x20, 0x0C, 0x0A, 0x0D, 0x09, 0x0B, 0x00, 0xA0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003,
            0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200A, 0x2028, 0x2029, 0x202F, 0x205F,
            0x3000, 0x85, 0x180E,
        ]
        .iter()
        .map(|&c| Some(c))
        .collect(),
    };
    let mut st = 0;
    let mut en = chars.len();
    if left {
        while st < en && set.contains(&chars[st].cp) {
            st += 1;
        }
    }
    if right {
        while en > st && set.contains(&chars[en - 1].cp) {
            en -= 1;
        }
    }
    if st >= en {
        return Ok(Value::Str(PhpStr::empty()));
    }
    Ok(Value::from(
        &s[chars[st].start..chars[en - 1].start + chars[en - 1].len],
    ))
}
fn mb_trim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, true, true)
}
fn mb_ltrim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, true, false)
}
fn mb_rtrim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, false, true)
}

fn mb_list_encodings(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        ENCODINGS.iter().map(|e| Value::from(e.name)),
    )))
}

fn mb_encoding_aliases(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    match lookup(&n) {
        Some(e) => Ok(Value::Array(PhpArray::from_values(
            e.aliases.iter().map(|x| Value::from(*x)),
        ))),
        None => vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                n.to_string_lossy()
            ),
        ),
    }
}

fn mb_preferred_mime_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    match lookup(&n) {
        Some(e) => Ok(Value::from(e.mime)),
        None => {
            vm.fwarn(
                a,
                &format!(
                    "No MIME preferred name corresponding to \"{}\"",
                    n.to_string_lossy()
                ),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn mb_language(vm: &mut Vm, a: &mut Args) -> R<Value> {
    const LANGS: &[(&str, &str)] = &[
        ("neutral", "neutral"),
        ("uni", "uni"),
        ("english", "English"),
        ("en", "English"),
        ("japanese", "Japanese"),
        ("ja", "Japanese"),
        ("german", "German"),
        ("de", "German"),
        ("korean", "Korean"),
        ("ko", "Korean"),
        ("russian", "Russian"),
        ("ru", "Russian"),
    ];
    match a.opt_str(0) {
        None => {
            if let Some(v) = vm.get_ini(b"mbstring.language") {
                let low = String::from_utf8_lossy(&v).to_ascii_lowercase();
                if let Some((_, l)) = LANGS.iter().find(|(k, _)| *k == low) {
                    return Ok(Value::from(*l));
                }
            }
            Ok(Value::from(vm.ext::<MbState>().language))
        }
        Some(l) => {
            let low = String::from_utf8_lossy(&l).to_ascii_lowercase();
            match LANGS.iter().find(|(k, _)| *k == low) {
                Some((_, v)) => {
                    vm.ext::<MbState>().language = v;
                    vm.set_ini(b"mbstring.language", v.as_bytes());
                    Ok(Value::Bool(true))
                }
                None => vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #1 ($language) must be a valid language, \"{}\" given",
                        l.to_string_lossy()
                    ),
                ),
            }
        }
    }
}

fn entity_map(vm: &mut Vm, a: &Args) -> R<Vec<[i64; 4]>> {
    let arr = a.array(1);
    let vals: Vec<Value> = arr.iter().map(|(_, v)| v.deref()).collect();
    if !vals.len().is_multiple_of(4) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($map) must have a multiple of 4 elements",
        );
    }
    let mut out = Vec::new();
    for ch in vals.chunks(4) {
        let mut q = [0i64; 4];
        for (i, v) in ch.iter().enumerate() {
            q[i] = match v {
                Value::Int(n) => *n,
                _ => {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        "Argument #2 ($map) must only be composed of values of type int",
                    );
                }
            };
        }
        out.push(q);
    }
    Ok(out)
}

fn mb_encode_numericentity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    let map = entity_map(vm, a)?;
    let hex = a.bool_or(3, false);
    let mut out = Vec::new();
    for c in cps(e.kind, a.bytes(0)) {
        let Some(cp) = c else {
            encode(e.kind, 0x3F, &mut out);
            continue;
        };
        let hit = map.iter().find(|m| (m[0]..=m[1]).contains(&(cp as i64)));
        match hit {
            Some(m) => {
                let v = ((cp as i64 + m[2]) & m[3]) as u64;
                let t = if hex {
                    format!("&#x{:X};", v)
                } else {
                    format!("&#{};", v)
                };
                for b in t.bytes() {
                    encode(e.kind, b as u32, &mut out);
                }
            }
            None => {
                encode(e.kind, cp, &mut out);
            }
        }
    }
    Ok(sv(out))
}

fn mb_decode_numericentity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = enc_arg(vm, a, 2)?;
    let map = entity_map(vm, a)?;
    let chars = cps(e.kind, a.bytes(0));
    let mut res: Vec<Option<u32>> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == Some('&' as u32) && chars.get(i + 1) == Some(&Some('#' as u32)) {
            let mut j = i + 2;
            let hex = matches!(chars.get(j), Some(Some(0x78 | 0x58)));
            if hex {
                j += 1;
            }
            let st = j;
            let mut v: i64 = 0;
            while let Some(Some(c)) = chars.get(j) {
                let d = match char::from_u32(*c).and_then(|c| c.to_digit(if hex { 16 } else { 10 }))
                {
                    Some(d) => d,
                    None => break,
                };
                v = v
                    .saturating_mul(if hex { 16 } else { 10 })
                    .saturating_add(d as i64);
                j += 1;
            }
            if j > st {
                let semi = chars.get(j) == Some(&Some(';' as u32));
                let end = if semi { j + 1 } else { j };
                let hit = map.iter().find(|m| {
                    let c = v - m[2];
                    (m[0]..=m[1]).contains(&c)
                });
                if let Some(m) = hit {
                    res.push(Some((v - m[2]) as u32));
                    i = end;
                    continue;
                }
            }
        }
        res.push(chars[i]);
        i += 1;
    }
    Ok(sv(encode_all(vm, e.kind, res.into_iter())))
}

fn mb_parse_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let seps = vm
        .get_ini(b"arg_separator.input")
        .unwrap_or_else(|| PhpStr::from("&"));
    let arr = super::string::parse_query(a.bytes(0), &seps);
    a.set_ref(1, Value::Array(arr));
    Ok(Value::Bool(true))
}

/// With mbstring.http_output "pass" (the default) output is unchanged.
fn mb_output_handler(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(a.str(0)))
}
