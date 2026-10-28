//! ext/mbstring (PHP manual: Multibyte String) on the goro-charset crate:
//! every `mb_*` string function, the encoding settings (internal, detect
//! order, substitute character, HTTP input/output, language) and the
//! mbstring.* ini settings. The encodings and the string operations are
//! goro-charset's; this module resolves arguments and settings and reports
//! errors as PHP does (tests ext/mbstring/tests, observed on PHP 8.5).

mod funcs;
mod regex;

use goro_charset::ops::MbError;
use goro_charset::{Encoding, Subst, SubstMode};
use goro_runtime::{PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

use funcs::*;
use regex::*;

pub static MODULE: Module = Module {
    name: "mbstring",
    functions: &[
        (
            "function mb_language(?string $language = null): string|bool",
            mb_language,
        ),
        (
            "function mb_internal_encoding(?string $encoding = null): string|bool",
            mb_internal_encoding,
        ),
        (
            "function mb_http_input(?string $type = null): array|string|false",
            mb_http_input,
        ),
        (
            "function mb_http_output(?string $encoding = null): string|bool",
            mb_http_output,
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
            "function mb_preferred_mime_name(string $encoding): string|false",
            mb_preferred_mime_name,
        ),
        (
            "function mb_parse_str(string $string, &$result): bool",
            mb_parse_str,
        ),
        (
            "function mb_output_handler(string $string, int $status): string",
            mb_output_handler,
        ),
        (
            "function mb_str_split(string $string, int $length = 1, ?string $encoding = null): array",
            mb_str_split,
        ),
        (
            "function mb_strlen(string $string, ?string $encoding = null): int",
            mb_strlen,
        ),
        (
            "function mb_strpos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_strpos,
        ),
        (
            "function mb_strrpos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_strrpos,
        ),
        (
            "function mb_stripos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false",
            mb_stripos,
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
            "function mb_strrchr(string $haystack, string $needle, bool $before_needle = false, ?string $encoding = null): string|false",
            mb_strrchr,
        ),
        (
            "function mb_stristr(string $haystack, string $needle, bool $before_needle = false, ?string $encoding = null): string|false",
            mb_stristr,
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
            "function mb_substr(string $string, int $start, ?int $length = null, ?string $encoding = null): string",
            mb_substr,
        ),
        (
            "function mb_strcut(string $string, int $start, ?int $length = null, ?string $encoding = null): string",
            mb_strcut,
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
            "function mb_convert_case(string $string, int $mode, ?string $encoding = null): string",
            mb_convert_case,
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
            "function mb_ucfirst(string $string, ?string $encoding = null): string",
            mb_ucfirst,
        ),
        (
            "function mb_lcfirst(string $string, ?string $encoding = null): string",
            mb_lcfirst,
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
        (
            "function mb_detect_encoding(string $string, array|string|null $encodings = null, bool $strict = false): string|false",
            mb_detect_encoding,
        ),
        ("function mb_list_encodings(): array", mb_list_encodings),
        (
            "function mb_encoding_aliases(string $encoding): array",
            mb_encoding_aliases,
        ),
        (
            "function mb_encode_mimeheader(string $string, ?string $charset = null, ?string $transfer_encoding = null, string $newline = \"\\r\\n\", int $indent = 0): string",
            mb_encode_mimeheader,
        ),
        (
            "function mb_decode_mimeheader(string $string): string",
            mb_decode_mimeheader,
        ),
        (
            "function mb_convert_kana(string $string, string $mode = \"KV\", ?string $encoding = null): string",
            mb_convert_kana,
        ),
        (
            "function mb_convert_variables(string $to_encoding, array|string $from_encoding, mixed &$var, mixed &...$vars): string|false",
            mb_convert_variables,
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
            "function mb_get_info(string $type = \"all\"): array|string|int|false",
            mb_get_info,
        ),
        (
            "function mb_check_encoding(array|string|null $value = null, ?string $encoding = null): bool",
            mb_check_encoding,
        ),
        (
            "function mb_scrub(string $string, ?string $encoding = null): string",
            mb_scrub,
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
            "function mb_str_pad(string $string, int $length, string $pad_string = \" \", int $pad_type = STR_PAD_RIGHT, ?string $encoding = null): string",
            mb_str_pad,
        ),
        (
            "function mb_regex_encoding(?string $encoding = null): string|bool",
            mb_regex_encoding,
        ),
        (
            "function mb_ereg(string $pattern, string $string, &$matches = null): bool",
            mb_ereg,
        ),
        (
            "function mb_eregi(string $pattern, string $string, &$matches = null): bool",
            mb_eregi,
        ),
        (
            "function mb_ereg_replace(string $pattern, string $replacement, string $string, ?string $options = null): string|false|null",
            mb_ereg_replace,
        ),
        (
            "function mb_eregi_replace(string $pattern, string $replacement, string $string, ?string $options = null): string|false|null",
            mb_eregi_replace,
        ),
        (
            "function mb_ereg_replace_callback(string $pattern, callable $callback, string $string, ?string $options = null): string|false|null",
            mb_ereg_replace_callback,
        ),
        (
            "function mb_split(string $pattern, string $string, int $limit = -1): array|false",
            mb_split,
        ),
        (
            "function mb_ereg_match(string $pattern, string $string, ?string $options = null): bool",
            mb_ereg_match,
        ),
        (
            "function mb_ereg_search(?string $pattern = null, ?string $options = null): bool",
            mb_ereg_search,
        ),
        (
            "function mb_ereg_search_pos(?string $pattern = null, ?string $options = null): array|false",
            mb_ereg_search_pos,
        ),
        (
            "function mb_ereg_search_regs(?string $pattern = null, ?string $options = null): array|false",
            mb_ereg_search_regs,
        ),
        (
            "function mb_ereg_search_init(string $string, ?string $pattern = null, ?string $options = null): bool",
            mb_ereg_search_init,
        ),
        (
            "function mb_ereg_search_getregs(): array|false",
            mb_ereg_search_getregs,
        ),
        (
            "function mb_ereg_search_getpos(): int",
            mb_ereg_search_getpos,
        ),
        (
            "function mb_ereg_search_setpos(int $offset): bool",
            mb_ereg_search_setpos,
        ),
        (
            "function mb_regex_set_options(?string $options = null): string",
            mb_regex_set_options,
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
    // PHP manual: mbstring runtime configuration; defaults observed on
    // PHP 8.5 (ini_get_all('mbstring')).
    ini: &[
        ("mbstring.language", "neutral"),
        ("mbstring.detect_order", ""),
        ("mbstring.http_input", ""),
        ("mbstring.http_output", ""),
        ("mbstring.internal_encoding", ""),
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
    startup_ini: Some(startup_ini),
    on_ini_set: Some(on_ini_set),
    listed: true,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- languages

/// A language of `mb_language()` with what it implies (observed on PHP
/// 8.5: `mb_get_info()` and `mb_detect_order()` under each
/// `mbstring.language`).
pub(crate) struct Lang {
    pub name: &'static str,
    pub short: &'static str,
    pub mail_charset: Encoding,
    /// `B` (true) or `Q` header encoding.
    pub header_b64: bool,
    pub body: Encoding,
    pub detect: &'static [Encoding],
}

use Encoding as E;

pub(crate) static LANGS: &[Lang] = &[
    Lang {
        name: "neutral",
        short: "neutral",
        mail_charset: E::Utf8,
        header_b64: true,
        body: E::Base64,
        detect: &[E::Ascii, E::Utf8],
    },
    Lang {
        name: "uni",
        short: "uni",
        mail_charset: E::Utf8,
        header_b64: true,
        body: E::Base64,
        detect: &[E::Ascii, E::Utf8],
    },
    Lang {
        name: "English",
        short: "en",
        mail_charset: E::Iso8859_1,
        header_b64: false,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8],
    },
    Lang {
        name: "German",
        short: "de",
        mail_charset: E::Iso8859_15,
        header_b64: false,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8],
    },
    Lang {
        name: "Japanese",
        short: "ja",
        mail_charset: E::Iso2022Jp,
        header_b64: true,
        body: E::SevenBit,
        detect: &[E::Ascii, E::Jis, E::Utf8, E::EucJp, E::Sjis],
    },
    Lang {
        name: "Korean",
        short: "ko",
        mail_charset: E::Iso2022Kr,
        header_b64: true,
        body: E::SevenBit,
        detect: &[E::Ascii, E::Utf8, E::EucKr, E::Uhc],
    },
    Lang {
        name: "Russian",
        short: "ru",
        mail_charset: E::Koi8R,
        header_b64: false,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8, E::Koi8R, E::Cp1251, E::Cp866],
    },
    Lang {
        name: "Armenian",
        short: "hy",
        mail_charset: E::ArmScii8,
        header_b64: false,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8, E::ArmScii8],
    },
    Lang {
        name: "Turkish",
        short: "tr",
        mail_charset: E::Iso8859_9,
        header_b64: false,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8, E::Cp1254, E::Iso8859_9],
    },
    Lang {
        name: "Ukrainian",
        short: "ua",
        mail_charset: E::Koi8U,
        header_b64: false,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8, E::Koi8U],
    },
    Lang {
        name: "Simplified Chinese",
        short: "zh-cn",
        mail_charset: E::Hz,
        header_b64: true,
        body: E::SevenBit,
        detect: &[E::Ascii, E::Utf8, E::EucCn, E::Cp936],
    },
    Lang {
        name: "Traditional Chinese",
        short: "zh-tw",
        mail_charset: E::Big5,
        header_b64: true,
        body: E::EightBit,
        detect: &[E::Ascii, E::Utf8, E::EucTw, E::Big5],
    },
];

pub(crate) fn find_lang(name: &[u8]) -> Option<usize> {
    LANGS.iter().position(|l| {
        name.eq_ignore_ascii_case(l.name.as_bytes())
            || name.eq_ignore_ascii_case(l.short.as_bytes())
    })
}

// ---------------------------------------------------------------- state

/// `mb_http_output()`: an encoding or "pass".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Output {
    Pass,
    Enc(Encoding),
}

pub(crate) struct MbState {
    loaded: bool,
    pub internal: Encoding,
    /// Set by `mb_internal_encoding()`: the internal_encoding and
    /// default_charset settings no longer apply (test internal_encoding.phpt).
    pub internal_func: bool,
    pub sub: Subst,
    pub detect: Vec<Encoding>,
    pub lang: usize,
    /// The language of the startup settings.
    pub lang0: usize,
    pub http_input: Vec<Encoding>,
    pub http_output: Output,
    pub http_output_func: bool,
    pub strict: bool,
    pub illegal: usize,
    pub regex_enc: Encoding,
    /// The encoding mb_parse_str() last detected (mb_http_input()).
    pub last_input: Option<Encoding>,
    /// The last encoding an argument named (see [`named_enc`]).
    pub last_named: Option<Encoding>,
}

impl Default for MbState {
    fn default() -> Self {
        MbState {
            loaded: false,
            internal: E::Utf8,
            internal_func: false,
            sub: Subst::default(),
            detect: vec![E::Ascii, E::Utf8],
            lang: 0,
            lang0: 0,
            http_input: Vec::new(),
            http_output: Output::Enc(E::Utf8),
            http_output_func: false,
            strict: false,
            illegal: 0,
            regex_enc: E::Utf8,
            last_input: None,
            last_named: None,
        }
    }
}

fn ini(vm: &Vm, k: &[u8]) -> Vec<u8> {
    vm.get_ini(k)
        .map(|v| v.as_bytes().to_vec())
        .unwrap_or_default()
}

pub(crate) fn ini_truthy(v: &[u8]) -> bool {
    let l = v.trim_ascii().to_ascii_lowercase();
    match &l[..] {
        b"on" | b"yes" | b"true" => true,
        b"off" | b"no" | b"false" | b"none" | b"" => false,
        _ => goro_runtime::convert::str_to_int(&l) != 0,
    }
}

/// The state, read from the settings the first time.
pub(crate) fn st(vm: &mut Vm) -> &mut MbState {
    if !vm.ext::<MbState>().loaded {
        load(vm);
    }
    vm.ext::<MbState>()
}

/// The encoding of a charset setting (internal_encoding, default_charset),
/// if valid.
fn enc_setting(v: &[u8]) -> Option<Encoding> {
    let v = v.trim_ascii();
    if v.is_empty() {
        None
    } else {
        Encoding::from_name(v)
    }
}

/// The internal encoding the settings give: mbstring.internal_encoding,
/// else internal_encoding, else default_charset (PHP manual: mbstring
/// runtime configuration; test internal_encoding.phpt).
fn internal_from(mb: &[u8], internal: &[u8], charset: &[u8]) -> Encoding {
    enc_setting(mb)
        .or_else(|| enc_setting(internal))
        .or_else(|| enc_setting(charset))
        .unwrap_or(E::Utf8)
}

/// An encoding list setting: the valid encodings and the invalid names.
fn parse_list_setting(v: &[u8], lang: usize) -> (Vec<Encoding>, Vec<Vec<u8>>) {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    for item in goro_charset::encoding::split_list(v) {
        if item.is_empty() {
            continue;
        }
        if item.eq_ignore_ascii_case(b"auto") {
            out.extend_from_slice(LANGS[lang].detect);
        } else if let Some(e) = Encoding::from_name(item) {
            out.push(e);
        } else {
            bad.push(item.to_vec());
        }
    }
    (out, bad)
}

/// The substitute character setting (`none`, `long`, `entity` or a code
/// point, decimal or `0x` hexadecimal; observed on PHP 8.5).
fn parse_sub_setting(v: &[u8], cur: Subst) -> Option<Subst> {
    let l = v.trim_ascii().to_ascii_lowercase();
    let mode = |m| Subst {
        mode: m,
        ch: cur.ch,
    };
    Some(match &l[..] {
        b"" => Subst::default(),
        b"none" => mode(SubstMode::None),
        b"long" => mode(SubstMode::Long),
        b"entity" => mode(SubstMode::Entity),
        _ => {
            let n = if let Some(h) = l.strip_prefix(b"0x") {
                i64::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?
            } else if l.iter().all(|c| c.is_ascii_digit()) {
                std::str::from_utf8(&l).ok()?.parse::<i64>().ok()?
            } else {
                return None;
            };
            if !(0..=0x10FFFF).contains(&n) || (0xD800..0xE000).contains(&n) {
                return None;
            }
            Subst::char(n as u32)
        }
    })
}

fn http_output_from(mb: &[u8], output: &[u8], charset: &[u8]) -> Output {
    let pick = |v: &[u8]| -> Option<Output> {
        let v = v.trim_ascii();
        if v.eq_ignore_ascii_case(b"pass") {
            Some(Output::Pass)
        } else {
            enc_setting(v).map(Output::Enc)
        }
    };
    pick(mb)
        .or_else(|| pick(output))
        .or_else(|| pick(charset))
        .unwrap_or(Output::Enc(E::Utf8))
}

fn load(vm: &mut Vm) {
    let lang = find_lang(ini(vm, b"mbstring.language").trim_ascii()).unwrap_or(0);
    let internal = internal_from(
        &ini(vm, b"mbstring.internal_encoding"),
        &ini(vm, b"internal_encoding"),
        &ini(vm, b"default_charset"),
    );
    let (mut detect, _) = parse_list_setting(&ini(vm, b"mbstring.detect_order"), lang);
    if detect.is_empty() {
        detect = LANGS[lang].detect.to_vec();
    }
    let mut http_input = parse_list_setting(&ini(vm, b"mbstring.http_input"), lang).0;
    if http_input.is_empty() && ini(vm, b"mbstring.http_input").trim_ascii().is_empty() {
        let ie = ini(vm, b"input_encoding");
        http_input = if ie.trim_ascii().is_empty() {
            enc_setting(&ini(vm, b"default_charset"))
                .into_iter()
                .collect()
        } else {
            parse_list_setting(&ie, lang).0
        };
    }
    let http_output = http_output_from(
        &ini(vm, b"mbstring.http_output"),
        &ini(vm, b"output_encoding"),
        &ini(vm, b"default_charset"),
    );
    let sub = parse_sub_setting(&ini(vm, b"mbstring.substitute_character"), Subst::default())
        .unwrap_or_default();
    let strict = ini_truthy(&ini(vm, b"mbstring.strict_detection"));
    let s = vm.ext::<MbState>();
    s.loaded = true;
    s.lang = lang;
    s.lang0 = lang;
    s.internal = internal;
    s.detect = detect;
    s.http_input = http_input;
    s.http_output = http_output;
    s.sub = sub;
    s.strict = strict;
    s.regex_enc = if regex::onig_name(internal).is_some() {
        internal
    } else {
        E::Utf8
    };
}

fn startup_warn(vm: &mut Vm, level: i64, msg: &str) {
    let _ = vm.raise_at(
        level,
        &format!("PHP Startup: {msg}"),
        PhpStr::from("Unknown"),
        0,
    );
}

/// The "PHP Startup:" diagnostics for the `-d` settings, in the order PHP
/// checks them (test ini_mbstring_invalid.phpt).
fn startup_ini(vm: &mut Vm, given: &[(Vec<u8>, Vec<u8>)]) {
    use crate::vm::errors::{E_DEPRECATED, E_WARNING};
    let has = |k: &[u8]| {
        given
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
    };
    let lang = find_lang(ini(vm, b"mbstring.language").trim_ascii()).unwrap_or(0);
    if let Some(v) = has(b"mbstring.detect_order") {
        for b in parse_list_setting(&v, lang).1 {
            startup_warn(
                vm,
                E_WARNING,
                &format!(
                    "INI setting contains invalid encoding \"{}\"",
                    String::from_utf8_lossy(&b)
                ),
            );
        }
    }
    if let Some(v) = has(b"mbstring.http_input")
        && !v.is_empty()
    {
        startup_warn(vm, E_DEPRECATED, "Use of mbstring.http_input is deprecated");
        for b in parse_list_setting(&v, lang).1 {
            startup_warn(
                vm,
                E_WARNING,
                &format!(
                    "INI setting contains invalid encoding \"{}\"",
                    String::from_utf8_lossy(&b)
                ),
            );
        }
    }
    if let Some(v) = has(b"mbstring.http_output")
        && !v.is_empty()
    {
        startup_warn(
            vm,
            E_DEPRECATED,
            "Use of mbstring.http_output is deprecated",
        );
    }
    if let Some(v) = has(b"mbstring.internal_encoding")
        && !v.is_empty()
    {
        startup_warn(
            vm,
            E_DEPRECATED,
            "Use of mbstring.internal_encoding is deprecated",
        );
        if enc_setting(&v).is_none() {
            startup_warn(
                vm,
                E_WARNING,
                &format!(
                    "Unknown encoding \"{}\" in ini setting",
                    String::from_utf8_lossy(v.trim_ascii())
                ),
            );
        }
    }
    // input_encoding is checked twice, for the HTTP input list and for the
    // request (test mb_http_input_001.phpt).
    if let Some(v) = has(b"input_encoding") {
        for b in parse_list_setting(&v, lang).1 {
            for _ in 0..2 {
                startup_warn(
                    vm,
                    E_WARNING,
                    &format!(
                        "INI setting contains invalid encoding \"{}\"",
                        String::from_utf8_lossy(&b)
                    ),
                );
            }
        }
    }
    load(vm);
}

/// ini_set() of an mbstring setting or of one it follows (observed on PHP
/// 8.5).
fn on_ini_set(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    let warn = |vm: &mut Vm, m: String| vm.warn(&format!("ini_set(): {m}"));
    match k {
        b"mbstring.internal_encoding" => {
            vm.deprecated("ini_set(): Use of mbstring.internal_encoding is deprecated")?;
            if !v.trim_ascii().is_empty() && enc_setting(v).is_none() {
                warn(
                    vm,
                    format!(
                        "Unknown encoding \"{}\" in ini setting",
                        String::from_utf8_lossy(v)
                    ),
                )?;
            }
            let ie = ini(vm, b"internal_encoding");
            let dc = ini(vm, b"default_charset");
            let s = st(vm);
            s.internal_func = false;
            s.internal = internal_from(v, &ie, &dc);
        }
        b"internal_encoding" | b"default_charset" => {
            let mb = ini(vm, b"mbstring.internal_encoding");
            let (ie, dc) = if k == b"internal_encoding" {
                (v.to_vec(), ini(vm, b"default_charset"))
            } else {
                (ini(vm, b"internal_encoding"), v.to_vec())
            };
            let mo = ini(vm, b"mbstring.http_output");
            let oe = ini(vm, b"output_encoding");
            let s = st(vm);
            if !s.internal_func {
                s.internal = internal_from(&mb, &ie, &dc);
            }
            if k == b"default_charset" && !s.http_output_func {
                s.http_output = http_output_from(&mo, &oe, &dc);
            }
        }
        b"output_encoding" => {
            let mo = ini(vm, b"mbstring.http_output");
            let dc = ini(vm, b"default_charset");
            let s = st(vm);
            if !s.http_output_func {
                s.http_output = http_output_from(&mo, v, &dc);
            }
        }
        b"input_encoding" => {
            let lang = st(vm).lang;
            if ini(vm, b"mbstring.http_input").is_empty() {
                let l = parse_list_setting(v, lang).0;
                st(vm).http_input = l;
            }
        }
        b"mbstring.language" => {
            let Some(l) = find_lang(v.trim_ascii()) else {
                return Ok(false);
            };
            st(vm).lang = l;
        }
        b"mbstring.detect_order" => {
            let lang = st(vm).lang;
            let (l, bad) = parse_list_setting(v, lang);
            if let Some(b) = bad.first() {
                warn(
                    vm,
                    format!(
                        "INI setting contains invalid encoding \"{}\"",
                        String::from_utf8_lossy(b)
                    ),
                )?;
                return Ok(false);
            }
            if l.is_empty() {
                return Ok(false);
            }
            st(vm).detect = l;
        }
        b"mbstring.http_input" => {
            vm.deprecated("ini_set(): Use of mbstring.http_input is deprecated")?;
            let lang = st(vm).lang;
            let (l, bad) = parse_list_setting(v, lang);
            if let Some(b) = bad.first() {
                warn(
                    vm,
                    format!(
                        "INI setting contains invalid encoding \"{}\"",
                        String::from_utf8_lossy(b)
                    ),
                )?;
                return Ok(false);
            }
            st(vm).http_input = l;
        }
        b"mbstring.http_output" => {
            vm.deprecated("ini_set(): Use of mbstring.http_output is deprecated")?;
            let t = v.trim_ascii();
            let o = if t.eq_ignore_ascii_case(b"pass") {
                Output::Pass
            } else if t.is_empty() {
                let oe = ini(vm, b"output_encoding");
                let dc = ini(vm, b"default_charset");
                http_output_from(b"", &oe, &dc)
            } else {
                match enc_setting(t) {
                    Some(e) => Output::Enc(e),
                    None => return Ok(false),
                }
            };
            let s = st(vm);
            s.http_output = o;
            s.http_output_func = false;
        }
        b"mbstring.substitute_character" => {
            let cur = st(vm).sub;
            if let Some(s) = parse_sub_setting(v, cur) {
                st(vm).sub = s;
            }
        }
        b"mbstring.strict_detection" => st(vm).strict = ini_truthy(v),
        b"mbstring.encoding_translation" => return Ok(false),
        _ => {}
    }
    Ok(true)
}

// ---------------------------------------------------------------- helpers

/// Throws an [`MbError`] as `ValueError`.
pub(crate) fn mb_err<T>(vm: &mut Vm, a: &Args, e: MbError) -> R<T> {
    vm.throw("ValueError", &e.message(&a.fname()))
}

pub(crate) fn internal(vm: &mut Vm) -> Encoding {
    st(vm).internal
}

pub(crate) fn subst(vm: &mut Vm) -> Subst {
    st(vm).sub
}

/// Resolves an encoding name given as argument `i` (`$name`), with the
/// deprecation for the non-text encodings (observed on PHP 8.5).
pub(crate) fn named_enc(vm: &mut Vm, a: &Args, i: usize, pname: &str, n: &[u8]) -> R<Encoding> {
    match Encoding::from_name(n) {
        Some(e) => {
            // mbstring remembers the last encoding named by an argument:
            // naming it again, in any function, does not repeat the
            // deprecation (observed on PHP 8.5).
            let s = st(vm);
            let again = s.last_named == Some(e);
            s.last_named = Some(e);
            if !again && let Some(m) = e.deprecation_message() {
                vm.fdeprecated(a, m)?;
            }
            Ok(e)
        }
        None => vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${}) must be a valid encoding, \"{}\" given",
                i + 1,
                pname,
                String::from_utf8_lossy(n)
            ),
        ),
    }
}

/// The `?string $encoding` argument `i`: the internal encoding when null.
pub(crate) fn enc_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Encoding> {
    match a.opt_str(i) {
        None => Ok(internal(vm)),
        Some(n) => named_enc(vm, a, i, "encoding", &n),
    }
}

/// An encoding list argument (array or comma-separated string); `auto`
/// expands to the language's detection order. Errors name argument `i`
/// (`$pname`).
pub(crate) fn enc_list(
    vm: &mut Vm,
    a: &Args,
    v: &Value,
    i: usize,
    pname: &str,
) -> R<Vec<Encoding>> {
    let lang = st(vm).lang;
    let mut out = Vec::new();
    let add = |vm: &mut Vm, n: &[u8], out: &mut Vec<Encoding>| -> R<()> {
        if n.eq_ignore_ascii_case(b"auto") {
            out.extend_from_slice(LANGS[lang].detect);
            return Ok(());
        }
        match Encoding::from_name(n) {
            Some(e) => {
                out.push(e);
                Ok(())
            }
            None => {
                // the name as a C string (test bug79149.phpt)
                let n = n.split(|&c| c == 0).next().unwrap_or(b"");
                vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #{} (${}) contains invalid encoding \"{}\"",
                        i + 1,
                        pname,
                        String::from_utf8_lossy(n)
                    ),
                )
            }
        }
    };
    match v {
        Value::Array(arr) => {
            for (_, x) in arr.iter() {
                let n = vm.to_php_str(&x.deref())?;
                add(vm, &n, &mut out)?;
            }
        }
        other => {
            let s = vm.to_php_str(other)?;
            if !s.is_empty() {
                // empty items are skipped ("CP50220, ASCII," in test
                // gh15824.phpt)
                for p in goro_charset::encoding::split_list(&s) {
                    if p.is_empty() {
                        continue;
                    }
                    add(vm, p, &mut out)?;
                }
            }
        }
    }
    if out.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${}) must specify at least one encoding",
                i + 1,
                pname
            ),
        );
    }
    Ok(out)
}

pub(crate) fn sv(b: Vec<u8>) -> Value {
    Value::Str(PhpStr::from(b))
}

pub(crate) fn names_array(list: &[Encoding]) -> Value {
    Value::Array(PhpArray::from_values(
        list.iter().map(|e| Value::from(e.name())),
    ))
}

/// Converts `s` to the internal encoding from the encoding detected among
/// `list` (mb_parse_str with mbstring.http_input).
pub(crate) fn convert_counted(vm: &mut Vm, s: &[u8], to: Encoding, from: Encoding) -> Vec<u8> {
    let sub = subst(vm);
    let c = goro_charset::convert(s, to, from, &sub);
    st(vm).illegal += c.illegal;
    c.bytes
}
