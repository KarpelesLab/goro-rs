//! String functions of the standard extension (PHP manual: String
//! functions, URL functions, and the html/encoding helpers), wired to the
//! pure algorithms of `goro_ext_standard`.
//!
//! Signatures follow the manual pages; argument checks the engine can't do
//! (by-reference parameters, union-dependent rules) and edge-case messages
//! come from the tests ext/standard/tests/strings/*.phpt and observations
//! of PHP 8.5.

use goro_ext_standard as std_ext;
use goro_ext_standard::error::{Diag, StdError};
use goro_ext_standard::printf::{ArgsKind, FormatArgs, Locale};
use goro_runtime::{Key, KeyRef, PhpArray, PhpStr, Value};

use crate::bytecode::CastTo;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        ("function strlen(string $string): int", strlen),
        (
            "function str_repeat(string $string, int $times): string",
            str_repeat,
        ),
        (
            "function substr(string $string, int $offset, ?int $length = null): string",
            substr,
        ),
        (
            "function strpos(string $haystack, string $needle, int $offset = 0): int|false",
            strpos,
        ),
        (
            "function stripos(string $haystack, string $needle, int $offset = 0): int|false",
            stripos,
        ),
        (
            "function strrpos(string $haystack, string $needle, int $offset = 0): int|false",
            strrpos,
        ),
        (
            "function strripos(string $haystack, string $needle, int $offset = 0): int|false",
            strripos,
        ),
        (
            "function str_contains(string $haystack, string $needle): bool",
            str_contains,
        ),
        (
            "function str_starts_with(string $haystack, string $needle): bool",
            str_starts_with,
        ),
        (
            "function str_ends_with(string $haystack, string $needle): bool",
            str_ends_with,
        ),
        (
            "function strstr(string $haystack, string $needle, bool $before_needle = false): string|false",
            strstr,
        ),
        (
            "function strchr(string $haystack, string $needle, bool $before_needle = false): string|false",
            strstr,
        ),
        (
            "function stristr(string $haystack, string $needle, bool $before_needle = false): string|false",
            stristr,
        ),
        (
            "function strrchr(string $haystack, string $needle, bool $before_needle = false): string|false",
            strrchr,
        ),
        (
            "function str_replace(array|string $search, array|string $replace, array|string $subject, &$count = null): array|string",
            str_replace,
        ),
        (
            "function str_ireplace(array|string $search, array|string $replace, array|string $subject, &$count = null): array|string",
            str_ireplace,
        ),
        (
            "function substr_replace(array|string $string, array|string $replace, array|int $offset, array|int|null $length = null): array|string",
            substr_replace,
        ),
        (
            "function substr_count(string $haystack, string $needle, int $offset = 0, ?int $length = null): int",
            substr_count,
        ),
        ("function ucfirst(string $string): string", ucfirst),
        ("function lcfirst(string $string): string", lcfirst),
        (
            "function ucwords(string $string, string $separators = \" \\t\\r\\n\\f\\v\"): string",
            ucwords,
        ),
        ("function strtolower(string $string): string", strtolower),
        ("function strtoupper(string $string): string", strtoupper),
        (
            "function trim(string $string, string $characters = \" \\n\\r\\t\\v\\x00\"): string",
            trim,
        ),
        (
            "function ltrim(string $string, string $characters = \" \\n\\r\\t\\v\\x00\"): string",
            ltrim,
        ),
        (
            "function rtrim(string $string, string $characters = \" \\n\\r\\t\\v\\x00\"): string",
            rtrim,
        ),
        (
            "function chop(string $string, string $characters = \" \\n\\r\\t\\v\\x00\"): string",
            rtrim,
        ),
        (
            "function str_pad(string $string, int $length, string $pad_string = \" \", int $pad_type = STR_PAD_RIGHT): string",
            str_pad,
        ),
        (
            "function str_split(string $string, int $length = 1): array",
            str_split,
        ),
        (
            "function explode(string $separator, string $string, int $limit = PHP_INT_MAX): array",
            explode,
        ),
        (
            "function implode(array|string $separator, ?array $array = null): string",
            implode,
        ),
        (
            "function join(array|string $separator, ?array $array = null): string",
            implode,
        ),
        (
            "function nl2br(string $string, bool $use_xhtml = true): string",
            nl2br,
        ),
        (
            "function wordwrap(string $string, int $width = 75, string $break = \"\\n\", bool $cut_long_words = false): string",
            wordwrap,
        ),
        (
            "function chunk_split(string $string, int $length = 76, string $separator = \"\\r\\n\"): string",
            chunk_split,
        ),
        ("function strrev(string $string): string", strrev),
        (
            "function str_word_count(string $string, int $format = 0, ?string $characters = null): array|int",
            str_word_count,
        ),
        (
            "function strtr(string $string, array|string|null $from, ?string $to = null): string",
            strtr,
        ),
        ("function addslashes(string $string): string", addslashes),
        (
            "function stripslashes(string $string): string",
            stripslashes,
        ),
        (
            "function addcslashes(string $string, string $characters): string",
            addcslashes,
        ),
        (
            "function stripcslashes(string $string): string",
            stripcslashes,
        ),
        ("function quotemeta(string $string): string", quotemeta),
        ("function chr(int $codepoint): string", chr),
        ("function ord(string $character): int", ord),
        (
            "function sprintf(string $format, mixed ...$values): string",
            sprintf,
        ),
        (
            "function printf(string $format, mixed ...$values): int",
            printf,
        ),
        (
            "function vsprintf(string $format, array $values): string",
            vsprintf,
        ),
        (
            "function vprintf(string $format, array $values): int",
            vprintf,
        ),
        (
            "function number_format(int|float $num, int $decimals = 0, ?string $decimal_separator = \".\", ?string $thousands_separator = \",\"): string",
            number_format,
        ),
        (
            "function strcmp(string $string1, string $string2): int",
            strcmp,
        ),
        (
            "function strcasecmp(string $string1, string $string2): int",
            strcasecmp,
        ),
        (
            "function strncmp(string $string1, string $string2, int $length): int",
            strncmp,
        ),
        (
            "function strncasecmp(string $string1, string $string2, int $length): int",
            strncasecmp,
        ),
        (
            "function strnatcmp(string $string1, string $string2): int",
            strnatcmp,
        ),
        (
            "function strnatcasecmp(string $string1, string $string2): int",
            strnatcasecmp,
        ),
        (
            "function strcoll(string $string1, string $string2): int",
            strcmp,
        ),
        (
            "function substr_compare(string $haystack, string $needle, int $offset, ?int $length = null, bool $case_insensitive = false): int",
            substr_compare,
        ),
        (
            "function similar_text(string $string1, string $string2, &$percent = null): int",
            similar_text,
        ),
        (
            "function levenshtein(string $string1, string $string2, int $insertion_cost = 1, int $replacement_cost = 1, int $deletion_cost = 1): int",
            levenshtein,
        ),
        ("function soundex(string $string): string", soundex),
        (
            "function metaphone(string $string, int $max_phonemes = 0): string",
            metaphone,
        ),
        (
            "function count_chars(string $string, int $mode = 0): array|string",
            count_chars,
        ),
        (
            "function strspn(string $string, string $characters, int $offset = 0, ?int $length = null): int",
            strspn,
        ),
        (
            "function strcspn(string $string, string $characters, int $offset = 0, ?int $length = null): int",
            strcspn,
        ),
        (
            "function strpbrk(string $string, string $characters): string|false",
            strpbrk,
        ),
        (
            "function strtok(string $string, ?string $token = null): string|false",
            strtok,
        ),
        (
            "function str_increment(string $string): string",
            str_increment,
        ),
        (
            "function str_decrement(string $string): string",
            str_decrement,
        ),
        ("function str_shuffle(string $string): string", str_shuffle),
        ("function utf8_encode(string $string): string", utf8_encode),
        ("function utf8_decode(string $string): string", utf8_decode),
        ("function localeconv(): array", localeconv),
        (
            "function htmlspecialchars(string $string, int $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401, ?string $encoding = null, bool $double_encode = true): string",
            htmlspecialchars,
        ),
        (
            "function htmlspecialchars_decode(string $string, int $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401): string",
            htmlspecialchars_decode,
        ),
        (
            "function htmlentities(string $string, int $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401, ?string $encoding = null, bool $double_encode = true): string",
            htmlentities,
        ),
        (
            "function html_entity_decode(string $string, int $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401, ?string $encoding = null): string",
            html_entity_decode,
        ),
        (
            "function get_html_translation_table(int $table = HTML_SPECIALCHARS, int $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401, string $encoding = \"UTF-8\"): array",
            get_html_translation_table,
        ),
        (
            "function strip_tags(string $string, array|string|null $allowed_tags = null): string",
            strip_tags,
        ),
        ("function urlencode(string $string): string", urlencode),
        (
            "function rawurlencode(string $string): string",
            rawurlencode,
        ),
        ("function urldecode(string $string): string", urldecode),
        (
            "function rawurldecode(string $string): string",
            rawurldecode,
        ),
        (
            "function http_build_query(object|array $data, string $numeric_prefix = \"\", ?string $arg_separator = null, int $encoding_type = PHP_QUERY_RFC1738): string",
            http_build_query,
        ),
        (
            "function parse_url(string $url, int $component = -1): array|string|int|false|null",
            parse_url,
        ),
        (
            "function parse_str(string $string, &$result): void",
            parse_str,
        ),
        (
            "function base64_encode(string $string): string",
            base64_encode,
        ),
        (
            "function base64_decode(string $string, bool $strict = false): string|false",
            base64_decode,
        ),
        ("function bin2hex(string $string): string", bin2hex),
        ("function hex2bin(string $string): string|false", hex2bin),
        (
            "function md5(string $string, bool $binary = false): string",
            md5,
        ),
        (
            "function sha1(string $string, bool $binary = false): string",
            sha1,
        ),
        (
            "function md5_file(string $filename, bool $binary = false): string|false",
            md5_file,
        ),
        (
            "function sha1_file(string $filename, bool $binary = false): string|false",
            sha1_file,
        ),
        ("function crc32(string $string): int", crc32),
        ("function str_rot13(string $string): string", str_rot13),
        (
            "function quoted_printable_encode(string $string): string",
            quoted_printable_encode,
        ),
        (
            "function quoted_printable_decode(string $string): string",
            quoted_printable_decode,
        ),
        (
            "function convert_uuencode(string $string): string",
            convert_uuencode,
        ),
        (
            "function convert_uudecode(string $string): string|false",
            convert_uudecode,
        ),
    ],
    constants: &[
        ("STR_PAD_LEFT", ConstVal::Int(0)),
        ("STR_PAD_RIGHT", ConstVal::Int(1)),
        ("STR_PAD_BOTH", ConstVal::Int(2)),
        ("ENT_HTML_QUOTE_NONE", ConstVal::Int(0)),
        ("ENT_HTML_QUOTE_SINGLE", ConstVal::Int(1)),
        ("ENT_HTML_QUOTE_DOUBLE", ConstVal::Int(2)),
        ("ENT_COMPAT", ConstVal::Int(2)),
        ("ENT_QUOTES", ConstVal::Int(3)),
        ("ENT_NOQUOTES", ConstVal::Int(0)),
        ("ENT_IGNORE", ConstVal::Int(4)),
        ("ENT_SUBSTITUTE", ConstVal::Int(8)),
        ("ENT_DISALLOWED", ConstVal::Int(128)),
        ("ENT_HTML401", ConstVal::Int(0)),
        ("ENT_XML1", ConstVal::Int(16)),
        ("ENT_XHTML", ConstVal::Int(32)),
        ("ENT_HTML5", ConstVal::Int(48)),
        ("HTML_SPECIALCHARS", ConstVal::Int(0)),
        ("HTML_ENTITIES", ConstVal::Int(1)),
        ("PHP_URL_SCHEME", ConstVal::Int(0)),
        ("PHP_URL_HOST", ConstVal::Int(1)),
        ("PHP_URL_PORT", ConstVal::Int(2)),
        ("PHP_URL_USER", ConstVal::Int(3)),
        ("PHP_URL_PASS", ConstVal::Int(4)),
        ("PHP_URL_PATH", ConstVal::Int(5)),
        ("PHP_URL_QUERY", ConstVal::Int(6)),
        ("PHP_URL_FRAGMENT", ConstVal::Int(7)),
        ("PHP_QUERY_RFC1738", ConstVal::Int(1)),
        ("PHP_QUERY_RFC3986", ConstVal::Int(2)),
        ("CRYPT_SALT_LENGTH", ConstVal::Int(123)),
        ("CRYPT_STD_DES", ConstVal::Int(1)),
        ("CRYPT_EXT_DES", ConstVal::Int(1)),
        ("CRYPT_MD5", ConstVal::Int(1)),
        ("CRYPT_BLOWFISH", ConstVal::Int(1)),
        ("CRYPT_SHA256", ConstVal::Int(1)),
        ("CRYPT_SHA512", ConstVal::Int(1)),
        ("CHAR_MAX", ConstVal::Int(127)),
        ("LC_CTYPE", ConstVal::Int(0)),
        ("LC_NUMERIC", ConstVal::Int(1)),
        ("LC_TIME", ConstVal::Int(2)),
        ("LC_COLLATE", ConstVal::Int(3)),
        ("LC_MONETARY", ConstVal::Int(4)),
        ("LC_MESSAGES", ConstVal::Int(5)),
        ("LC_ALL", ConstVal::Int(6)),
    ],
    listed: true,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- helpers

/// A string value from bytes.
#[inline]
pub(crate) fn sv(b: Vec<u8>) -> Value {
    Value::Str(PhpStr::from(b))
}

/// Throws a `StdError` of the standard-library algorithms.
pub(crate) fn std_err(vm: &mut Vm, e: StdError) -> Unwind {
    match e {
        StdError::ValueError(m) => vm.throw_error_msg("ValueError", &m),
        StdError::ArgumentCountError(m) => vm.throw_error_msg("ArgumentCountError", &m),
        StdError::TypeError(m) => vm.throw_error_msg("TypeError", &m),
        StdError::DivisionByZeroError(m) => vm.throw_error_msg("DivisionByZeroError", &m),
        StdError::ArithmeticError(m) => vm.throw_error_msg("ArithmeticError", &m),
        StdError::Fatal(m) => vm.fatal(&m),
    }
}

/// Raises the diagnostics of a standard-library algorithm, prefixed with
/// the function name.
pub(crate) fn diags(vm: &mut Vm, a: &Args, d: Vec<Diag>) -> R<()> {
    for x in d {
        match x {
            Diag::Warning(m) => vm.fwarn(a, &m)?,
            Diag::Notice(m) => vm.fnotice(a, &m)?,
            Diag::Deprecated(m) => vm.fdeprecated(a, &m)?,
        }
    }
    Ok(())
}

/// `Result<T, StdError>` to `R<T>`.
pub(crate) fn sr<T>(vm: &mut Vm, r: Result<T, StdError>) -> R<T> {
    r.map_err(|e| std_err(vm, e))
}

/// Refuses an allocation beyond memory_limit with PHP's fatal error (the
/// engine would otherwise abort on a huge allocation).
pub(crate) fn alloc_check(vm: &mut Vm, bytes: u128) -> R<()> {
    let limit = vm.ini.memory_limit;
    let cap: u128 = if limit < 0 {
        isize::MAX as u128 / 2
    } else {
        limit as u128
    };
    if bytes > cap {
        let shown = if limit < 0 { cap } else { limit as u128 };
        return Err(vm.fatal(&format!(
            "Allowed memory size of {} bytes exhausted (tried to allocate {} bytes)",
            shown,
            bytes + 25
        )));
    }
    Ok(())
}

/// The value of an int argument that may also be absent / null.
fn opt_len(a: &Args, i: usize) -> Option<i64> {
    a.opt_int(i)
}

fn str_or_false(v: Option<Vec<u8>>) -> Value {
    match v {
        Some(b) => sv(b),
        None => Value::Bool(false),
    }
}

fn int_or_false(v: Option<i64>) -> Value {
    match v {
        Some(i) => Value::Int(i),
        None => Value::Bool(false),
    }
}

/// A list of strings as a PHP array.
fn str_list(v: Vec<Vec<u8>>) -> Value {
    Value::Array(PhpArray::from_values(v.into_iter().map(sv)))
}

// ---------------------------------------------------------------- basics

fn strlen(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(a.bytes(0).len() as i64))
}

fn str_repeat(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let times = a.int(1);
    let s = a.str(0);
    if times == 1 {
        return Ok(Value::Str(s));
    }
    if times > 0 {
        alloc_check(vm, s.len() as u128 * times as u128)?;
        return Ok(Value::Str(PhpStr::repeat(&s, times as usize)));
    }
    let r = std_ext::strings::str_repeat(&s, times);
    Ok(sv(sr(vm, r)?))
}

fn substr(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let r = std_ext::strings::substr_range(s.len(), a.int(1), opt_len(a, 2));
    if r.start == 0 && r.end == s.len() {
        return Ok(Value::Str(s));
    }
    Ok(Value::from(&s[r]))
}

fn pos_common(vm: &mut Vm, a: &mut Args, kind: std_ext::strings::PosKind) -> R<Value> {
    let r = std_ext::strings::strpos(a.bytes(0), a.bytes(1), a.int_or(2, 0), kind);
    Ok(int_or_false(sr(vm, r)?))
}

fn strpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, std_ext::strings::PosKind::Pos)
}
fn stripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, std_ext::strings::PosKind::IPos)
}
fn strrpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, std_ext::strings::PosKind::RPos)
}
fn strripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos_common(vm, a, std_ext::strings::PosKind::RIPos)
}

fn str_contains(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(std_ext::strings::str_contains(
        a.bytes(0),
        a.bytes(1),
    )))
}
fn str_starts_with(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.bytes(0).starts_with(a.bytes(1))))
}
fn str_ends_with(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.bytes(0).ends_with(a.bytes(1))))
}

fn strstr(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(str_or_false(std_ext::strings::strstr(
        a.bytes(0),
        a.bytes(1),
        a.bool_or(2, false),
        false,
    )))
}
fn stristr(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(str_or_false(std_ext::strings::strstr(
        a.bytes(0),
        a.bytes(1),
        a.bool_or(2, false),
        true,
    )))
}
fn strrchr(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(str_or_false(std_ext::strings::strrchr(
        a.bytes(0),
        a.bytes(1),
        a.bool_or(2, false),
    )))
}

// ---------------------------------------------------------------- str_replace

/// The string values of an array, in order (converted as `(string)`).
fn array_strings(vm: &mut Vm, arr: &PhpArray) -> R<Vec<Vec<u8>>> {
    let mut out = Vec::with_capacity(arr.len());
    for (_, v) in arr.iter() {
        let v = v.deref();
        let mut b = Vec::new();
        vm.append_to_string(&mut b, &v)?;
        out.push(b);
    }
    Ok(out)
}

fn replace_common(vm: &mut Vm, a: &mut Args, ci: bool) -> R<Value> {
    let search = a.val(0);
    let replace = a.val(1);
    let subject = a.val(2);
    if let (Value::Str(_), Value::Array(_)) = (&search, &replace) {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #2 ($replace) must be of type string when argument #1 ($search) is a string",
        );
    }
    let searches: Vec<Vec<u8>>;
    let rep_list: Vec<Vec<u8>>;
    let rep_str: Vec<u8>;
    let single_search: Option<PhpStr> = match &search {
        Value::Array(arr) => {
            searches = array_strings(vm, arr)?;
            None
        }
        Value::Str(s) => {
            searches = Vec::new();
            Some(s.clone())
        }
        _ => unreachable!(),
    };
    let replacement = match &replace {
        Value::Array(arr) => {
            rep_list = array_strings(vm, arr)?;
            rep_str = Vec::new();
            std_ext::strings::Replacement::List(&rep_list)
        }
        Value::Str(s) => {
            rep_list = Vec::new();
            rep_str = s.to_vec();
            std_ext::strings::Replacement::Str(&rep_str)
        }
        _ => unreachable!(),
    };
    let _ = &rep_list;
    let mut total = 0usize;
    let one = |s: &PhpStr, total: &mut usize| -> Value {
        match &single_search {
            Some(needle) => {
                let (out, n) = std_ext::strings::replace(s, needle, &rep_str, ci);
                *total += n;
                if n == 0 {
                    Value::Str(s.clone())
                } else {
                    sv(out)
                }
            }
            None => {
                let (out, n) = std_ext::strings::str_replace_one(s, &searches, replacement, ci);
                *total += n;
                if n == 0 {
                    Value::Str(s.clone())
                } else {
                    sv(out)
                }
            }
        }
    };
    let result = match &subject {
        Value::Array(arr) => {
            let mut out = PhpArray::with_capacity(arr.len());
            for (k, v) in arr.iter() {
                let v = v.deref();
                let nv = match &v {
                    Value::Str(s) => one(s, &mut total),
                    other => {
                        let s = vm.to_php_str(other)?;
                        one(&s, &mut total)
                    }
                };
                out.insert(k.to_owned(), nv);
            }
            Value::Array(out)
        }
        Value::Str(s) => one(s, &mut total),
        _ => unreachable!(),
    };
    if a.has(3) {
        a.set_ref(3, Value::Int(total as i64));
    }
    Ok(result)
}

fn str_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    replace_common(vm, a, false)
}
fn str_ireplace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    replace_common(vm, a, true)
}

fn substr_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let string = a.val(0);
    let replace = a.val(1);
    let offset = a.val(2);
    let length = if a.has(3) { a.val(3) } else { Value::Null };
    match &string {
        Value::Str(s) => {
            if matches!(offset, Value::Array(_)) {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #3 ($offset) cannot be an array when working on a single string",
                );
            }
            if matches!(length, Value::Array(_)) {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #4 ($length) cannot be an array when working on a single string",
                );
            }
            let rep = match &replace {
                Value::Array(arr) => match arr.iter().next() {
                    Some((_, v)) => vm.to_php_str(&v.deref())?,
                    None => PhpStr::empty(),
                },
                v => vm.to_php_str(v)?,
            };
            let len = match length {
                Value::Null => None,
                v => Some(vm.to_int(&v)?),
            };
            let off = vm.to_int(&offset)?;
            Ok(sv(std_ext::strings::substr_replace(s, &rep, off, len)))
        }
        Value::Array(arr) => {
            let mut rep_it: Option<Vec<Value>> = match &replace {
                Value::Array(r) => Some(r.iter().map(|(_, v)| v.deref()).collect()),
                _ => None,
            };
            let rep_one = match &replace {
                Value::Array(_) => PhpStr::empty(),
                v => vm.to_php_str(v)?,
            };
            let off_list: Option<Vec<Value>> = match &offset {
                Value::Array(o) => Some(o.iter().map(|(_, v)| v.deref()).collect()),
                _ => None,
            };
            let off_one = match &offset {
                Value::Array(_) => 0,
                v => vm.to_int(v)?,
            };
            let len_list: Option<Vec<Value>> = match &length {
                Value::Array(l) => Some(l.iter().map(|(_, v)| v.deref()).collect()),
                _ => None,
            };
            let len_one: Option<i64> = match &length {
                Value::Array(_) | Value::Null => None,
                v => Some(vm.to_int(v)?),
            };
            let mut out = PhpArray::with_capacity(arr.len());
            for (idx, (k, v)) in arr.iter().enumerate() {
                let s = vm.to_php_str(&v.deref())?;
                let off = match &off_list {
                    Some(l) => match l.get(idx) {
                        Some(x) => vm.to_int(x)?,
                        None => 0,
                    },
                    None => off_one,
                };
                let len = match &len_list {
                    Some(l) => match l.get(idx) {
                        Some(x) => Some(vm.to_int(x)?),
                        None => Some(s.len() as i64),
                    },
                    None => len_one,
                };
                let rep = match &mut rep_it {
                    Some(l) => match l.get(idx) {
                        Some(x) => vm.to_php_str(x)?,
                        None => PhpStr::empty(),
                    },
                    None => rep_one.clone(),
                };
                out.insert(
                    k.to_owned(),
                    sv(std_ext::strings::substr_replace(&s, &rep, off, len)),
                );
            }
            Ok(Value::Array(out))
        }
        _ => unreachable!(),
    }
}

fn substr_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::substr_count(a.bytes(0), a.bytes(1), a.int_or(2, 0), opt_len(a, 3));
    Ok(Value::Int(sr(vm, r)?))
}

// ---------------------------------------------------------------- case, trim, pad

fn ucfirst(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    if s.first().is_none_or(|c| !c.is_ascii_lowercase()) {
        return Ok(Value::Str(s));
    }
    Ok(sv(std_ext::strings::ucfirst(&s)))
}
fn lcfirst(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    if s.first().is_none_or(|c| !c.is_ascii_uppercase()) {
        return Ok(Value::Str(s));
    }
    Ok(sv(std_ext::strings::lcfirst(&s)))
}
fn ucwords(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let delims: &[u8] = if a.has(1) {
        a.bytes(1)
    } else {
        std_ext::strings::UCWORDS_DELIMITERS
    };
    Ok(sv(std_ext::strings::ucwords(a.bytes(0), delims)))
}
fn strtolower(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    if !s.iter().any(|c| c.is_ascii_uppercase()) {
        return Ok(Value::Str(s));
    }
    Ok(sv(std_ext::strings::strtolower(&s)))
}
fn strtoupper(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    if !s.iter().any(|c| c.is_ascii_lowercase()) {
        return Ok(Value::Str(s));
    }
    Ok(sv(std_ext::strings::strtoupper(&s)))
}

fn trim_common(vm: &mut Vm, a: &mut Args, mode: std_ext::strings::TrimMode) -> R<Value> {
    let s = a.str(0);
    let chars = if a.has(1) { Some(a.bytes(1)) } else { None };
    let mut d = Vec::new();
    let out = std_ext::strings::trim(&s, chars, mode, &mut d);
    diags(vm, a, d)?;
    if out.len() == s.len() {
        return Ok(Value::Str(s));
    }
    Ok(sv(out))
}
fn trim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, std_ext::strings::TrimMode::Both)
}
fn ltrim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, std_ext::strings::TrimMode::Left)
}
fn rtrim(vm: &mut Vm, a: &mut Args) -> R<Value> {
    trim_common(vm, a, std_ext::strings::TrimMode::Right)
}

fn str_pad(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pad: &[u8] = if a.has(2) { a.bytes(2) } else { b" " };
    if !pad.is_empty() {
        alloc_check(vm, a.int(1).max(0) as u128)?;
    }
    let r = std_ext::strings::str_pad(a.bytes(0), a.int(1), pad, a.int_or(3, 1));
    Ok(sv(sr(vm, r)?))
}

fn str_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::str_split(a.bytes(0), a.int_or(1, 1));
    let parts = sr(vm, r)?;
    // Test str_split_basic.phpt: an empty string gives an empty array.
    Ok(str_list(parts))
}

fn explode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::explode(a.bytes(0), a.bytes(1), a.int_or(2, i64::MAX));
    Ok(str_list(sr(vm, r)?))
}

/// Joins array elements converted to strings (implode()).
pub(crate) fn join_values(vm: &mut Vm, sep: &[u8], arr: &PhpArray) -> R<Vec<u8>> {
    let mut out = Vec::new();
    for (i, (_, v)) in arr.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(sep);
        }
        let v = v.deref();
        vm.append_to_string(&mut out, &v)?;
    }
    Ok(out)
}

fn implode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let sep = a.val(0);
    let arr = if a.has(1) { a.val(1) } else { Value::Null };
    match (&sep, &arr) {
        (Value::Array(pieces), Value::Null) => Ok(sv(join_values(vm, b"", pieces)?)),
        (Value::Str(s), Value::Array(pieces)) => Ok(sv(join_values(vm, s, pieces)?)),
        (Value::Str(_), Value::Str(_)) => vm.fthrow(
            a,
            "TypeError",
            "Argument #2 ($array) must be of type ?array, string given",
        ),
        (Value::Str(_), _) => vm.fthrow(
            a,
            "TypeError",
            "If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given",
        ),
        (Value::Array(_), _) => vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($separator) must be of type string, array given",
        ),
        _ => unreachable!(),
    }
}

fn nl2br(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::strings::nl2br(a.bytes(0), a.bool_or(1, true))))
}

fn wordwrap(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let brk: &[u8] = if a.has(2) { a.bytes(2) } else { b"\n" };
    let width = a.int_or(1, 75);
    if a.bool_or(3, false) && width >= 0 {
        // Worst case: a break after every `width` bytes.
        let n = a.bytes(0).len() as u128;
        let w = width.max(1) as u128;
        alloc_check(vm, n + n.div_ceil(w) * brk.len() as u128)?;
    } else if width >= 0 && !brk.is_empty() {
        let n = a.bytes(0).len() as u128;
        let spaces = a.bytes(0).iter().filter(|&&c| c == b' ').count() as u128;
        alloc_check(vm, n + spaces * brk.len() as u128)?;
    }
    let r = std_ext::strings::wordwrap(a.bytes(0), a.int_or(1, 75), brk, a.bool_or(3, false));
    Ok(sv(sr(vm, r)?))
}

fn chunk_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let end: &[u8] = if a.has(2) { a.bytes(2) } else { b"\r\n" };
    let chunk = a.int_or(1, 76);
    if chunk > 0 {
        let n = a.bytes(0).len() as u128;
        alloc_check(vm, n + n.div_ceil(chunk as u128) * end.len() as u128)?;
    }
    let r = std_ext::strings::chunk_split(a.bytes(0), a.int_or(1, 76), end);
    Ok(sv(sr(vm, r)?))
}

fn strrev(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::strings::strrev(a.bytes(0))))
}

fn str_word_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let chars = a.opt_str(2);
    let mut d = Vec::new();
    let r = std_ext::strings::str_word_count(
        a.bytes(0),
        a.int_or(1, 0),
        chars.as_ref().map(|c| c.as_bytes()),
        &mut d,
    );
    let r = sr(vm, r)?;
    diags(vm, a, d)?;
    use std_ext::strings::WordCount;
    Ok(match r {
        WordCount::Count(n) => Value::Int(n),
        WordCount::Words(w) => str_list(w),
        WordCount::Positions(p) => Value::Array(PhpArray::from_pairs(
            p.into_iter().map(|(i, w)| (Key::Int(i), sv(w))),
        )),
    })
}

fn strtr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let from = a.val(1);
    if a.len() < 3 {
        let Value::Array(pairs) = from else {
            // null is refused without the deprecation of a null string
            // (test strings/strtr_variation8.phpt)
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #2 ($from) must be of type array, {} given",
                    if matches!(from, Value::Null) {
                        "null"
                    } else {
                        "string"
                    }
                ),
            );
        };
        let mut list = Vec::with_capacity(pairs.len());
        for (k, v) in pairs.iter() {
            let key = match k {
                KeyRef::Int(i) => i.to_string().into_bytes(),
                KeyRef::Str(s) => s.to_vec(),
            };
            let mut val = Vec::new();
            vm.append_to_string(&mut val, &v.deref())?;
            list.push((key, val));
        }
        let mut d = Vec::new();
        let out = std_ext::strings::strtr_pairs(&s, &list, &mut d);
        diags(vm, a, d)?;
        return Ok(sv(out));
    }
    let to = a.val(2);
    let from = match from {
        Value::Str(f) => f,
        Value::Array(_) => {
            return vm.fthrow(
                a,
                "TypeError",
                "Argument #2 ($from) must be of type string, array given",
            );
        }
        _ => {
            vm.fdeprecated(
                a,
                "Passing null to parameter #2 ($from) of type array|string is deprecated",
            )?;
            PhpStr::empty()
        }
    };
    let to = match to {
        Value::Str(t) => t,
        _ => PhpStr::empty(),
    };
    Ok(sv(std_ext::strings::strtr_chars(&s, &from, &to)))
}

fn addslashes(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::strings::addslashes(a.bytes(0))))
}
fn stripslashes(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::strings::stripslashes(a.bytes(0))))
}
fn addcslashes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut d = Vec::new();
    let out = std_ext::strings::addcslashes(a.bytes(0), a.bytes(1), &mut d);
    diags(vm, a, d)?;
    Ok(sv(out))
}
fn stripcslashes(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::strings::stripcslashes(a.bytes(0))))
}
fn quotemeta(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::strings::quotemeta(a.bytes(0))))
}

fn chr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = a.int(0);
    if !(0..=255).contains(&c) {
        vm.fdeprecated(
            a,
            "Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256",
        )?;
    }
    Ok(Value::Str(PhpStr::from_byte(c.rem_euclid(256) as u8)))
}

fn ord(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0);
    match s.len() {
        0 => {
            vm.fdeprecated(a, "Providing an empty string is deprecated")?;
            Ok(Value::Int(0))
        }
        1 => Ok(Value::Int(s[0] as i64)),
        _ => {
            let c = s[0];
            vm.fdeprecated(
                a,
                "Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead",
            )?;
            Ok(Value::Int(c as i64))
        }
    }
}

// ---------------------------------------------------------------- printf

/// printf arguments taken from PHP values, converted on demand.
struct ValueArgs<'a> {
    vm: &'a mut Vm,
    vals: &'a [Value],
    err: Option<Unwind>,
}

impl ValueArgs<'_> {
    fn conv(&mut self, i: usize, to: CastTo) -> Value {
        if self.err.is_some() {
            return Value::Int(0);
        }
        let v = self.vals[i].deref();
        match self.vm.cast(to, v) {
            Ok(v) => v,
            Err(e) => {
                self.err = Some(e);
                Value::Int(0)
            }
        }
    }
}

impl FormatArgs for ValueArgs<'_> {
    fn count(&self) -> usize {
        self.vals.len()
    }
    fn int(&mut self, i: usize) -> i64 {
        match self.conv(i, CastTo::Int) {
            Value::Int(n) => n,
            _ => 0,
        }
    }
    fn float(&mut self, i: usize) -> f64 {
        match self.conv(i, CastTo::Float) {
            Value::Float(f) => f,
            Value::Int(n) => n as f64,
            _ => 0.0,
        }
    }
    fn string(&mut self, i: usize, out: &mut Vec<u8>) {
        if self.err.is_some() {
            return;
        }
        let v = self.vals[i].deref();
        if let Err(e) = self.vm.append_to_string(out, &v) {
            self.err = Some(e);
        }
    }
    fn exact_int(&mut self, i: usize) -> Option<i64> {
        match self.vals[i].deref() {
            Value::Int(n) => Some(n),
            _ => None,
        }
    }
}

/// Formats `fmt` with PHP values (the printf family).
pub(crate) fn format_values(
    vm: &mut Vm,
    a: &Args,
    fmt: &[u8],
    vals: &[Value],
    kind: ArgsKind,
) -> R<Vec<u8>> {
    let mut out = Vec::new();
    let mut d = Vec::new();
    let mut args = ValueArgs {
        vm,
        vals,
        err: None,
    };
    let r = std_ext::printf::format(fmt, &mut args, kind, Locale::default(), &mut out, &mut d);
    if let Some(e) = args.err.take() {
        return Err(e);
    }
    let r = sr(vm, r);
    // Diagnostics come before a thrown error only when they were raised
    // first; the engine reports them in order either way.
    diags(vm, a, d)?;
    r?;
    Ok(out)
}

fn sprintf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let vals: Vec<Value> = a.vals[1..].to_vec();
    Ok(sv(format_values(vm, a, &fmt, &vals, ArgsKind::Variadic)?))
}

fn printf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let vals: Vec<Value> = a.vals[1..].to_vec();
    let out = format_values(vm, a, &fmt, &vals, ArgsKind::Variadic)?;
    vm.echo(&out);
    Ok(Value::Int(out.len() as i64))
}

fn vsprintf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let vals: Vec<Value> = a.array(1).iter().map(|(_, v)| v.deref()).collect();
    Ok(sv(format_values(vm, a, &fmt, &vals, ArgsKind::Array)?))
}

fn vprintf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let vals: Vec<Value> = a.array(1).iter().map(|(_, v)| v.deref()).collect();
    let out = format_values(vm, a, &fmt, &vals, ArgsKind::Array)?;
    vm.echo(&out);
    Ok(Value::Int(out.len() as i64))
}

/// 2^63 as a float.
const I64_LIMIT: f64 = 9_223_372_036_854_775_808.0;

fn number_format(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let dec = a.int_or(1, 0);
    let dp = if a.has(2) {
        a.opt_str(2).unwrap_or_else(|| PhpStr::from("."))
    } else {
        PhpStr::from(".")
    };
    let ts = if a.has(3) {
        a.opt_str(3).unwrap_or_else(|| PhpStr::from(","))
    } else {
        PhpStr::from(",")
    };
    Ok(sv(match a.val(0) {
        Value::Int(i) => std_ext::math::number_format_int(i, dec, &dp, &ts),
        v => {
            let f = match v {
                Value::Float(f) => f,
                _ => 0.0,
            };
            // Observed on PHP 8.5: an integral float in the int range is
            // rounded as an int ("-9,223,372,036,854,775,810" for
            // -2^63 to tens, test math/number_format_basiclong_64bit.phpt),
            // one beyond it as a float (2^63 stays ...808).
            if dec < 0 && f.fract() == 0.0 && (-(I64_LIMIT)..I64_LIMIT).contains(&f) {
                std_ext::math::number_format_int(f as i64, dec, &dp, &ts)
            } else {
                std_ext::math::number_format(f, dec, &dp, &ts)
            }
        }
    }))
}

// ---------------------------------------------------------------- comparison

fn strcmp(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::strings::strcmp(a.bytes(0), a.bytes(1))))
}
fn strcasecmp(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::strings::strcasecmp(
        a.bytes(0),
        a.bytes(1),
    )))
}
fn strncmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::strncmp(a.bytes(0), a.bytes(1), a.int(2), false);
    Ok(Value::Int(sr(vm, r)?))
}
fn strncasecmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::strncmp(a.bytes(0), a.bytes(1), a.int(2), true);
    Ok(Value::Int(sr(vm, r)?))
}
fn strnatcmp(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::natcmp::strnatcmp(
        a.bytes(0),
        a.bytes(1),
        false,
    )))
}
fn strnatcasecmp(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::natcmp::strnatcmp(
        a.bytes(0),
        a.bytes(1),
        true,
    )))
}
fn substr_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::substr_compare(
        a.bytes(0),
        a.bytes(1),
        a.int(2),
        opt_len(a, 3),
        a.bool_or(4, false),
    );
    Ok(Value::Int(sr(vm, r)?))
}

fn similar_text(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (n, pct) = std_ext::phonetic::similar_text(a.bytes(0), a.bytes(1));
    if a.has(2) {
        a.set_ref(2, Value::Float(pct));
    }
    Ok(Value::Int(n))
}

fn levenshtein(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::phonetic::levenshtein(
        a.bytes(0),
        a.bytes(1),
        a.int_or(2, 1),
        a.int_or(3, 1),
        a.int_or(4, 1),
    )))
}

fn soundex(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::phonetic::soundex(a.bytes(0))))
}

fn metaphone(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::phonetic::metaphone(a.bytes(0), a.int_or(1, 0));
    Ok(sv(sr(vm, r)?))
}

fn count_chars(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.int_or(1, 0);
    if !(0..=4).contains(&mode) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be between 0 and 4 (inclusive)",
        );
    }
    if mode >= 3 {
        let r = std_ext::strings::count_chars_string(a.bytes(0), mode);
        return Ok(sv(sr(vm, r)?));
    }
    let c = std_ext::strings::count_chars(a.bytes(0));
    let mut out = PhpArray::new();
    for (b, &n) in c.iter().enumerate() {
        let keep = match mode {
            0 => true,
            1 => n > 0,
            _ => n == 0,
        };
        if keep {
            out.insert(Key::Int(b as i64), Value::Int(n as i64));
        }
    }
    Ok(Value::Array(out))
}

fn strspn(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::strings::strspn(
        a.bytes(0),
        a.bytes(1),
        a.int_or(2, 0),
        opt_len(a, 3),
    )))
}
fn strcspn(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::strings::strcspn(
        a.bytes(0),
        a.bytes(1),
        a.int_or(2, 0),
        opt_len(a, 3),
    )))
}
fn strpbrk(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::strings::strpbrk(a.bytes(0), a.bytes(1));
    Ok(str_or_false(sr(vm, r)?))
}

/// strtok() state: the string being tokenized (None when not started or
/// started on an empty string) and the position.
#[derive(Default)]
struct StrtokState {
    s: Option<PhpStr>,
    pos: usize,
}

fn strtok(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let token = match a.opt_str(1) {
        Some(t) => {
            let s = a.str(0);
            let st = vm.ext::<StrtokState>();
            st.s = Some(s);
            st.pos = 0;
            t
        }
        None => a.str(0),
    };
    let st = vm.ext::<StrtokState>();
    let Some(s) = st.s.clone() else {
        if a.opt_str(1).is_none() {
            // Tests strtok_variation3.phpt and friends.
            vm.fwarn(
                a,
                "Both arguments must be provided when starting tokenization",
            )?;
        }
        return Ok(Value::Bool(false));
    };
    let mut mask = [false; 256];
    for &c in token.iter() {
        mask[c as usize] = true;
    }
    let mut p = st.pos;
    if p >= s.len() {
        return Ok(Value::Bool(false));
    }
    while p < s.len() && mask[s[p] as usize] {
        p += 1;
    }
    if p >= s.len() {
        // Only delimiters were left: the tokenization ends (observed on
        // PHP 8.5: a later strtok($token) warns).
        st.s = None;
        return Ok(Value::Bool(false));
    }
    let start = p;
    while p < s.len() && !mask[s[p] as usize] {
        p += 1;
    }
    st.pos = if p < s.len() { p + 1 } else { p };
    Ok(Value::from(&s[start..p]))
}

fn str_increment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0);
    if s.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($string) must not be empty");
    }
    if !s.iter().all(|c| c.is_ascii_alphanumeric()) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($string) must be composed only of alphanumeric ASCII characters",
        );
    }
    Ok(sv(goro_runtime::ops::str_increment_bytes(s)))
}

fn str_decrement(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0).to_vec();
    if s.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($string) must not be empty");
    }
    if !s.iter().all(|c| c.is_ascii_alphanumeric()) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($string) must be composed only of alphanumeric ASCII characters",
        );
    }
    // Test str_decrement_basic.phpt / str_decrement_underflow.phpt: a
    // borrow past a leading letter drops it ("Aa" → "z"), a leading "1"
    // going to "0" is dropped ("10" → "9"); a borrow past a leading "0",
    // or of a single character, is out of range.
    let mut out = s.clone();
    let mut i = out.len() - 1;
    loop {
        let c = out[i];
        if matches!(c, b'a' | b'A' | b'0') {
            if i == 0 {
                if c == b'0' || out.len() == 1 {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        &format!(
                            "Argument #1 ($string) \"{}\" is out of decrement range",
                            String::from_utf8_lossy(&s)
                        ),
                    );
                }
                out.remove(0);
                break;
            }
            out[i] = match c {
                b'a' => b'z',
                b'A' => b'Z',
                _ => b'9',
            };
            i -= 1;
            continue;
        }
        out[i] = c - 1;
        if i == 0 && out[0] == b'0' && out.len() > 1 {
            out.remove(0);
        }
        break;
    }
    Ok(sv(out))
}

fn str_shuffle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut s = a.bytes(0).to_vec();
    super::random::legacy_shuffle(vm, &mut s);
    Ok(sv(s))
}

fn utf8_encode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives",
    )?;
    let mut out = Vec::with_capacity(a.bytes(0).len());
    for &c in a.bytes(0) {
        if c < 0x80 {
            out.push(c);
        } else {
            out.push(0xC0 | (c >> 6));
            out.push(0x80 | (c & 0x3F));
        }
    }
    Ok(sv(out))
}

fn utf8_decode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives",
    )?;
    let s = a.bytes(0);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let (cp, len) = decode_utf8(&s[i..]);
        match cp {
            Some(cp) => {
                out.push(if cp < 0x100 { cp as u8 } else { b'?' });
                i += len;
            }
            None => {
                let _ = c;
                out.push(b'?');
                i += len.max(1);
            }
        }
    }
    Ok(sv(out))
}

/// Decodes one UTF-8 sequence: the code point (None when invalid) and the
/// number of bytes consumed.
pub(crate) fn decode_utf8(s: &[u8]) -> (Option<u32>, usize) {
    let c = s[0];
    let (len, init) = match c {
        0x00..=0x7F => return (Some(c as u32), 1),
        0xC2..=0xDF => (2, (c & 0x1F) as u32),
        0xE0..=0xEF => (3, (c & 0x0F) as u32),
        0xF0..=0xF4 => (4, (c & 0x07) as u32),
        _ => return (None, 1),
    };
    let mut cp = init;
    for k in 1..len {
        match s.get(k) {
            Some(&b) if b & 0xC0 == 0x80 => cp = (cp << 6) | (b & 0x3F) as u32,
            _ => return (None, k),
        }
    }
    let min = [0, 0, 0x80, 0x800, 0x10000][len];
    if cp < min || cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return (None, len);
    }
    (Some(cp), len)
}

fn localeconv(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // The C locale (observed on PHP 8.5).
    let mut out = PhpArray::new();
    let s = |v: &str| Value::from(v);
    out.insert(Key::from("decimal_point"), s("."));
    for k in [
        "thousands_sep",
        "int_curr_symbol",
        "currency_symbol",
        "mon_decimal_point",
        "mon_thousands_sep",
        "positive_sign",
        "negative_sign",
    ] {
        out.insert(Key::from(k), s(""));
    }
    for k in [
        "int_frac_digits",
        "frac_digits",
        "p_cs_precedes",
        "p_sep_by_space",
        "n_cs_precedes",
        "n_sep_by_space",
        "p_sign_posn",
        "n_sign_posn",
    ] {
        out.insert(Key::from(k), Value::Int(127));
    }
    out.insert(Key::from("grouping"), Value::Array(PhpArray::new()));
    out.insert(Key::from("mon_grouping"), Value::Array(PhpArray::new()));
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- html

/// The charset used when none is given: internal_encoding, then
/// default_charset (PHP manual: htmlentities, "encoding";
/// mbstring.internal_encoding does not count: htmlentities25).
fn default_html_charset(vm: &Vm) -> PhpStr {
    {
        let k = &b"internal_encoding"[..];
        if let Some(v) = vm.get_ini(k)
            && !v.is_empty()
        {
            return v;
        }
    }
    vm.get_ini(b"default_charset").unwrap_or_default()
}

fn charset_arg(vm: &mut Vm, a: &Args, i: usize) -> R<std_ext::html::Charset> {
    let name = a.opt_str(i);
    let default = default_html_charset(vm);
    let mut d = Vec::new();
    let cs = std_ext::html::resolve_charset(name.as_ref().map(|n| n.as_bytes()), &default, &mut d);
    diags(vm, a, d)?;
    Ok(cs)
}

const HTML_DEFAULT: i64 = std_ext::html::DEFAULT_FLAGS;

fn htmlspecialchars(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cs = charset_arg(vm, a, 2)?;
    Ok(sv(std_ext::html::htmlspecialchars(
        a.bytes(0),
        a.int_or(1, HTML_DEFAULT),
        cs,
        a.bool_or(3, true),
    )))
}

fn htmlspecialchars_decode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::html::htmlspecialchars_decode(
        a.bytes(0),
        a.int_or(1, HTML_DEFAULT),
    )))
}

fn htmlentities(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cs = charset_arg(vm, a, 2)?;
    let mut d = Vec::new();
    let out = std_ext::html::htmlentities(
        a.bytes(0),
        a.int_or(1, HTML_DEFAULT),
        cs,
        a.bool_or(3, true),
        &mut d,
    );
    diags(vm, a, d)?;
    Ok(sv(out))
}

fn html_entity_decode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cs = charset_arg(vm, a, 2)?;
    Ok(sv(std_ext::html::html_entity_decode(
        a.bytes(0),
        a.int_or(1, HTML_DEFAULT),
        cs,
    )))
}

fn get_html_translation_table(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = if a.has(2) {
        a.str(2)
    } else {
        PhpStr::from("UTF-8")
    };
    let default = default_html_charset(vm);
    let mut d = Vec::new();
    let cs = std_ext::html::resolve_charset(Some(&name), &default, &mut d);
    diags(vm, a, d)?;
    let rows =
        std_ext::html::get_html_translation_table(a.int_or(0, 0), a.int_or(1, HTML_DEFAULT), cs);
    let mut out = PhpArray::with_capacity(rows.len());
    for (k, v) in rows {
        out.insert(Key::Str(PhpStr::from(k)), sv(v));
    }
    Ok(Value::Array(out))
}

fn strip_tags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std_ext::html::AllowedTags;
    let allowed = if a.has(1) { a.val(1) } else { Value::Null };
    let out = match &allowed {
        Value::Null => std_ext::html::strip_tags(a.bytes(0), AllowedTags::None),
        Value::Str(s) => std_ext::html::strip_tags(a.bytes(0), AllowedTags::Str(s)),
        Value::Array(arr) => {
            let list = array_strings(vm, arr)?;
            let refs: Vec<&[u8]> = list.iter().map(|v| v.as_slice()).collect();
            std_ext::html::strip_tags(a.bytes(0), AllowedTags::List(&refs))
        }
        _ => unreachable!(),
    };
    Ok(sv(out))
}

// ---------------------------------------------------------------- url

fn urlencode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::url::urlencode(a.bytes(0))))
}
fn rawurlencode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::url::rawurlencode(a.bytes(0))))
}
fn urldecode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::url::urldecode(a.bytes(0))))
}
fn rawurldecode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::url::rawurldecode(a.bytes(0))))
}

fn http_build_query(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std_ext::url::{QueryBuilder, QueryEncoding, QueryKey};
    let data = a.val(0);
    let prefix = if a.has(1) { a.str(1) } else { PhpStr::empty() };
    let sep = match a.opt_str(2) {
        Some(s) => s,
        None => vm
            .get_ini(b"arg_separator.output")
            .unwrap_or_else(|| PhpStr::from("&")),
    };
    let enc = QueryEncoding::from_php(a.int_or(3, 1));
    let mut qb = QueryBuilder::new(&sep, enc);

    fn pairs_of(vm: &mut Vm, v: &Value) -> Vec<(Key, Value)> {
        match v {
            Value::Array(arr) => arr.iter().map(|(k, v)| (k.to_owned(), v.deref())).collect(),
            Value::Object(o) => vm
                .visible_props(o)
                .into_iter()
                .map(|(k, v)| {
                    let k = match k {
                        Value::Int(i) => Key::Int(i),
                        Value::Str(s) => Key::Str(s),
                        _ => Key::Int(0),
                    };
                    (k, v.deref())
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn walk(
        vm: &mut Vm,
        qb: &mut QueryBuilder,
        v: &Value,
        parent: Option<&[u8]>,
        prefix: &[u8],
        depth: usize,
    ) -> R<()> {
        if depth > 64 {
            return Ok(());
        }
        for (k, v) in pairs_of(vm, v) {
            let qk = match &k {
                Key::Int(i) => QueryKey::Int(*i),
                Key::Str(s) => QueryKey::Str(s),
            };
            let name = match parent {
                None => qb.top_key(qk, prefix),
                Some(p) => qb.child_key(p, qk),
            };
            match &v {
                Value::Null => {}
                // enums give their value (test http/gh15650.phpt)
                Value::Object(o) if vm.class_of(o).is_enum() => {
                    let cls = vm.class_of(o);
                    if cls.enum_backing.is_none() {
                        return vm.throw(
                            "ValueError",
                            &format!(
                                "Unbacked enum {} cannot be converted to a string",
                                cls.name.to_string_lossy()
                            ),
                        );
                    }
                    let bv = vm.get_prop_raw(o, b"value");
                    let s = vm.to_php_str(&bv)?;
                    qb.push(&name, &s);
                }
                Value::Array(_) | Value::Object(_) => {
                    walk(vm, qb, &v, Some(&name), prefix, depth + 1)?;
                }
                Value::Bool(b) => qb.push(&name, if *b { b"1" } else { b"0" }),
                Value::Resource(_) => {}
                other => {
                    let s = vm.to_php_str(other)?;
                    qb.push(&name, &s);
                }
            }
        }
        Ok(())
    }
    if let Value::Object(o) = &data
        && vm.class_of(o).is_enum()
    {
        let n = vm.class_of(o).name.to_string_lossy();
        return vm.fthrow(
            a,
            "TypeError",
            &format!("Argument #1 ($data) must not be an enum, {n} given"),
        );
    }
    walk(vm, &mut qb, &data, None, &prefix, 0)?;
    Ok(sv(qb.finish()))
}

fn url_to_array(u: std_ext::url::Url) -> PhpArray {
    let mut out = PhpArray::new();
    let mut put = |k: &str, v: Option<Vec<u8>>| {
        if let Some(v) = v {
            out.insert(Key::from(k), sv(v));
        }
    };
    put("scheme", u.scheme);
    put("host", u.host);
    if let Some(p) = u.port {
        out.insert(Key::from("port"), Value::Int(p));
    }
    let mut put = |k: &str, v: Option<Vec<u8>>| {
        if let Some(v) = v {
            out.insert(Key::from(k), sv(v));
        }
    };
    put("user", u.user);
    put("pass", u.pass);
    put("path", u.path);
    put("query", u.query);
    put("fragment", u.fragment);
    out
}

fn parse_url(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std_ext::url::{UrlPart, UrlResult};
    let r = std_ext::url::parse_url_component(a.bytes(0), a.int_or(1, -1));
    Ok(match sr(vm, r)? {
        UrlResult::False => Value::Bool(false),
        UrlResult::Array(u) => Value::Array(url_to_array(u)),
        UrlResult::Part(None) => Value::Null,
        UrlResult::Part(Some(UrlPart::Str(s))) => sv(s),
        UrlResult::Part(Some(UrlPart::Int(i))) => Value::Int(i),
    })
}

/// Registers `name=value` into `arr` as PHP registers request variables
/// (PHP manual: Variables From External Sources): `a[b][]` nesting, `.`
/// and space in the base name become `_`, an unmatched `[` too.
pub(crate) fn register_variable(arr: &mut PhpArray, name: &[u8], value: Value) {
    // Leading spaces are dropped.
    let mut name = name;
    while let Some((&b' ', rest)) = name.split_first() {
        name = rest;
    }
    // The base name ends at the first '['.
    let bracket = name.iter().position(|&c| c == b'[');
    let base_end = bracket.unwrap_or(name.len());
    let mut base: Vec<u8> = name[..base_end].to_vec();
    if base.is_empty() {
        return;
    }
    let mut indexes: Vec<Option<Vec<u8>>> = Vec::new();
    let mut rest = &name[base_end..];
    let mut malformed_tail = false;
    while let Some(b'[') = rest.first() {
        let Some(close) = rest.iter().position(|&c| c == b']') else {
            // An unmatched '[': for the first one, the base name gets it as
            // '_' and the rest literally; later ones end the parsing.
            if indexes.is_empty() {
                base.push(b'_');
                base.extend_from_slice(&rest[1..]);
            }
            malformed_tail = true;
            break;
        };
        let idx = &rest[1..close];
        let mut start = 0;
        while start < idx.len() && matches!(idx[start], b' ' | b'\t' | b'\r' | b'\n') {
            start += 1;
        }
        let idx = &idx[start..];
        indexes.push(if idx.is_empty() {
            None
        } else {
            Some(idx.to_vec())
        });
        rest = &rest[close + 1..];
        if rest.first() != Some(&b'[') {
            break;
        }
    }
    let _ = malformed_tail;
    for c in base.iter_mut() {
        if *c == b' ' || *c == b'.' {
            *c = b'_';
        }
    }
    // Once the first '[' is converted, later '.' stay (handled above).
    let key = Key::from_bytes(&base);
    if indexes.is_empty() {
        arr.insert(key, value);
        return;
    }
    fn descend(cur: &mut PhpArray, key: Option<Key>, idx: &[Option<Vec<u8>>], value: Value) {
        if idx.is_empty() {
            match key {
                Some(k) => {
                    cur.insert(k, value);
                }
                None => {
                    let _ = cur.push(value);
                }
            }
            return;
        }
        let slot: &mut Value = match key {
            Some(k) => {
                let e = cur.entry(k);
                if !matches!(e, Value::Array(_)) {
                    *e = Value::Array(PhpArray::new());
                }
                e
            }
            None => match cur.make_mut().push_entry() {
                Ok(e) => {
                    *e = Value::Array(PhpArray::new());
                    e
                }
                Err(_) => return,
            },
        };
        let Value::Array(inner) = slot else {
            unreachable!()
        };
        let next = idx[0].as_ref().map(|b| Key::from_bytes(b));
        descend(inner, next, &idx[1..], value);
    }
    descend(arr, Some(key), &indexes, value);
}

/// Parses a query string into an array (parse_str()).
pub(crate) fn parse_query(s: &[u8], seps: &[u8]) -> PhpArray {
    let mut arr = PhpArray::new();
    for part in s.split(|c| seps.contains(c)) {
        if part.is_empty() {
            continue;
        }
        let (n, v) = match part.iter().position(|&c| c == b'=') {
            Some(p) => (&part[..p], &part[p + 1..]),
            None => (part, &b""[..]),
        };
        let name = std_ext::url::urldecode(n);
        let val = std_ext::url::urldecode(v);
        register_variable(&mut arr, &name, sv(val));
    }
    arr
}

fn parse_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let seps = vm
        .get_ini(b"arg_separator.input")
        .unwrap_or_else(|| PhpStr::from("&"));
    let arr = parse_query(a.bytes(0), &seps);
    a.set_ref(1, Value::Array(arr));
    Ok(Value::Null)
}

fn base64_encode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::url::base64_encode(a.bytes(0))))
}
fn base64_decode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(str_or_false(std_ext::url::base64_decode(
        a.bytes(0),
        a.bool_or(1, false),
    )))
}

// ---------------------------------------------------------------- encodings

fn bin2hex(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::bin2hex(a.bytes(0))))
}
fn hex2bin(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut d = Vec::new();
    let r = std_ext::encoding::hex2bin(a.bytes(0), &mut d);
    diags(vm, a, d)?;
    Ok(str_or_false(r))
}
fn md5(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::md5(a.bytes(0), a.bool_or(1, false))))
}
fn sha1(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::sha1(a.bytes(0), a.bool_or(1, false))))
}
/// The digest of a file for md5_file()/sha1_file(), read in chunks (the
/// file may be larger than memory_limit: test ext/standard/tests/file/
/// bug22414); PHP manual; the messages observed on PHP 8.5.
fn hash_file_digest(vm: &mut Vm, a: &Args, algo: &[u8]) -> R<Option<Vec<u8>>> {
    use std::io::Read;
    let f = a.str(0);
    if f.is_empty() {
        return vm.value_error("Path must not be empty");
    }
    let file = std::fs::File::open(vm.fs_path(&f)).and_then(|fh| {
        if fh.metadata()?.is_dir() {
            Err(std::io::Error::from_raw_os_error(libc::EISDIR))
        } else {
            Ok(fh)
        }
    });
    match file {
        Ok(mut fh) => {
            let alg = goro_ext_hash::lookup(algo).expect("built-in algorithm");
            let mut h = goro_ext_hash::Hasher::plain(alg);
            let mut buf = vec![0u8; 64 << 10];
            loop {
                match fh.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => h.update(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            Ok(Some(h.finish()))
        }
        Err(e) => {
            let msg = e.to_string();
            let msg = match msg.rfind(" (os error") {
                Some(p) => msg[..p].to_string(),
                None => msg,
            };
            vm.warn(&format!(
                "{}({}): Failed to open stream: {}",
                a.fname(),
                f.to_string_lossy(),
                msg
            ))?;
            Ok(None)
        }
    }
}

fn digest_value(d: Vec<u8>, raw: bool) -> Value {
    if raw {
        return sv(d);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(d.len() * 2);
    for b in d {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 15) as usize]);
    }
    sv(out)
}

fn md5_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match hash_file_digest(vm, a, b"md5")? {
        Some(d) => digest_value(d, a.bool_or(1, false)),
        None => Value::Bool(false),
    })
}
fn sha1_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match hash_file_digest(vm, a, b"sha1")? {
        Some(d) => digest_value(d, a.bool_or(1, false)),
        None => Value::Bool(false),
    })
}
fn crc32(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::encoding::crc32(a.bytes(0)) as i64))
}
fn str_rot13(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::str_rot13(a.bytes(0))))
}
fn quoted_printable_encode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::quoted_printable_encode(a.bytes(0))))
}
fn quoted_printable_decode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::quoted_printable_decode(a.bytes(0))))
}
fn convert_uuencode(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::encoding::convert_uuencode(a.bytes(0))))
}
fn convert_uudecode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut d = Vec::new();
    let r = std_ext::encoding::convert_uudecode(a.bytes(0), &mut d);
    diags(vm, a, d)?;
    Ok(str_or_false(r))
}
