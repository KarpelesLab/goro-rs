//! `Normalizer`, the `grapheme_*()` functions and `idn_to_ascii()` /
//! `idn_to_utf8()` (PHP manual: Normalizer, Grapheme Functions, IDN
//! Functions).

use goro_ext_intl::error as ue;
use goro_ext_intl::{grapheme as gr, idna};
use goro_runtime::{Key, PhpArray, Value};

use super::{fail, reset};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function normalizer_normalize(string $string, int $form = Normalizer::FORM_C): string|false",
            n_normalize,
        ),
        (
            "function normalizer_is_normalized(string $string, int $form = Normalizer::FORM_C): bool",
            n_is_normalized,
        ),
        (
            "function normalizer_get_raw_decomposition(string $string, int $form = Normalizer::FORM_C): ?string",
            n_raw_decomposition,
        ),
        ("function grapheme_strlen(string $string): int|false|null", g_strlen),
        (
            "function grapheme_strpos(string $haystack, string $needle, int $offset = 0, string $locale = \"\"): int|false",
            g_strpos,
        ),
        (
            "function grapheme_stripos(string $haystack, string $needle, int $offset = 0, string $locale = \"\"): int|false",
            g_stripos,
        ),
        (
            "function grapheme_strrpos(string $haystack, string $needle, int $offset = 0, string $locale = \"\"): int|false",
            g_strrpos,
        ),
        (
            "function grapheme_strripos(string $haystack, string $needle, int $offset = 0, string $locale = \"\"): int|false",
            g_strripos,
        ),
        (
            "function grapheme_substr(string $string, int $offset, ?int $length = null, string $locale = \"\"): string|false",
            g_substr,
        ),
        (
            "function grapheme_strstr(string $haystack, string $needle, bool $beforeNeedle = false, string $locale = \"\"): string|false",
            g_strstr,
        ),
        (
            "function grapheme_stristr(string $haystack, string $needle, bool $beforeNeedle = false, string $locale = \"\"): string|false",
            g_stristr,
        ),
        (
            "function grapheme_extract(string $haystack, int $size, int $type = GRAPHEME_EXTR_COUNT, int $offset = 0, &$next = null): string|false",
            g_extract,
        ),
        (
            "function grapheme_str_split(string $string, int $length = 1): array|false",
            g_str_split,
        ),
        (
            "function grapheme_levenshtein(string $string1, string $string2, int $insertion_cost = 1, int $replacement_cost = 1, int $deletion_cost = 1, string $locale = \"\"): int|false",
            g_levenshtein,
        ),
        (
            "function idn_to_ascii(string $domain, int $flags = IDNA_DEFAULT, int $variant = INTL_IDNA_VARIANT_UTS46, &$idna_info = null): string|false",
            idn_to_ascii,
        ),
        (
            "function idn_to_utf8(string $domain, int $flags = IDNA_DEFAULT, int $variant = INTL_IDNA_VARIANT_UTS46, &$idna_info = null): string|false",
            idn_to_utf8,
        ),
    ],
    classes: &[("normalizer", NORMALIZER_CLASS)],
    methods: &[
        ("normalizer::normalize", n_normalize),
        ("normalizer::isnormalized", n_is_normalized),
        ("normalizer::getrawdecomposition", n_raw_decomposition),
    ],
    constants: &[
        ("GRAPHEME_EXTR_COUNT", ConstVal::Int(0)),
        ("GRAPHEME_EXTR_MAXBYTES", ConstVal::Int(1)),
        ("GRAPHEME_EXTR_MAXCHARS", ConstVal::Int(2)),
        ("IDNA_DEFAULT", ConstVal::Int(0)),
        ("IDNA_ALLOW_UNASSIGNED", ConstVal::Int(1)),
        ("IDNA_USE_STD3_RULES", ConstVal::Int(2)),
        ("IDNA_CHECK_BIDI", ConstVal::Int(4)),
        ("IDNA_CHECK_CONTEXTJ", ConstVal::Int(8)),
        ("IDNA_NONTRANSITIONAL_TO_ASCII", ConstVal::Int(16)),
        ("IDNA_NONTRANSITIONAL_TO_UNICODE", ConstVal::Int(32)),
        ("INTL_IDNA_VARIANT_UTS46", ConstVal::Int(1)),
        ("IDNA_ERROR_EMPTY_LABEL", ConstVal::Int(1)),
        ("IDNA_ERROR_LABEL_TOO_LONG", ConstVal::Int(2)),
        ("IDNA_ERROR_DOMAIN_NAME_TOO_LONG", ConstVal::Int(4)),
        ("IDNA_ERROR_LEADING_HYPHEN", ConstVal::Int(8)),
        ("IDNA_ERROR_TRAILING_HYPHEN", ConstVal::Int(0x10)),
        ("IDNA_ERROR_HYPHEN_3_4", ConstVal::Int(0x20)),
        ("IDNA_ERROR_LEADING_COMBINING_MARK", ConstVal::Int(0x40)),
        ("IDNA_ERROR_DISALLOWED", ConstVal::Int(0x80)),
        ("IDNA_ERROR_PUNYCODE", ConstVal::Int(0x100)),
        ("IDNA_ERROR_LABEL_HAS_DOT", ConstVal::Int(0x200)),
        ("IDNA_ERROR_INVALID_ACE_LABEL", ConstVal::Int(0x400)),
        ("IDNA_ERROR_BIDI", ConstVal::Int(0x800)),
        ("IDNA_ERROR_CONTEXTJ", ConstVal::Int(0x1000)),
    ],
    ..Module::EMPTY
};

const NORMALIZER_CLASS: &str = r#"<?php class Normalizer {
    public const int FORM_D = 4;
    public const int NFD = 4;
    public const int FORM_KD = 8;
    public const int NFKD = 8;
    public const int FORM_C = 16;
    public const int NFC = 16;
    public const int FORM_KC = 32;
    public const int NFKC = 32;
    public const int FORM_KC_CF = 48;
    public const int NFKC_CF = 48;
    public static function normalize(string $string, int $form = Normalizer::FORM_C): string|false {}
    public static function isNormalized(string $string, int $form = Normalizer::FORM_C): bool {}
    public static function getRawDecomposition(string $string, int $form = Normalizer::FORM_C): ?string {}
}"#;

// ---------------------------------------------------------------- Normalizer

fn normalize_str(s: &str, form: i64) -> String {
    use goro_ext_intl::intl::unicode::{nfc, nfd, nfkc, nfkd};
    match form {
        4 => nfd(s.chars()).collect(),
        8 => nfkd(s.chars()).collect(),
        32 => nfkc(s.chars()).collect(),
        48 => {
            // NFKC_Casefold (UAX #15 / UCD DerivedNormalizationProps):
            // NFKC of the case fold of NFKD, without default ignorables
            let d: String = nfkd(s.chars()).collect();
            let f: String = goro_ext_intl::intl::unicode::fold(d.chars())
                .filter(|c| !goro_ext_intl::intl::unicode::is_default_ignorable(*c))
                .collect();
            let k: String = nfkc(f.chars()).collect();
            let f2: String = goro_ext_intl::intl::unicode::fold(k.chars()).collect();
            nfkc(f2.chars()).collect()
        }
        _ => nfc(s.chars()).collect(),
    }
}

fn form_arg(vm: &mut Vm, a: &Args) -> R<Option<i64>> {
    let f = a.int_or(1, 16);
    if !matches!(f, 4 | 8 | 16 | 32 | 48) {
        return vm
            .fthrow(
                a,
                "ValueError",
                "Argument #2 ($form) must be a a valid normalization form",
            )
            .map(|()| None);
    }
    Ok(Some(f))
}

fn n_normalize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(form) = form_arg(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match std::str::from_utf8(a.bytes(0)) {
        Ok(s) => Ok(Value::from(normalize_str(s, form).as_str())),
        Err(_) => {
            fail(
                vm,
                a,
                ue::U_INVALID_CHAR_FOUND,
                "Error converting input string to UTF-16",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_is_normalized(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(form) = form_arg(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match std::str::from_utf8(a.bytes(0)) {
        Ok(s) => Ok(Value::Bool(normalize_str(s, form) == s)),
        Err(_) => {
            fail(
                vm,
                a,
                ue::U_INVALID_CHAR_FOUND,
                "Error converting input string to UTF-16",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_raw_decomposition(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use goro_ext_intl::intl::unicode::normalize::{NormalizationForm, raw_decomposition};
    reset(vm);
    let Some(form) = form_arg(vm, a)? else {
        return Ok(Value::Null);
    };
    let b = a.bytes(0);
    let s = match std::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "Code point out of range")?;
            return Ok(Value::Null);
        }
    };
    let mut it = s.chars();
    let (Some(c), None) = (it.next(), it.next()) else {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "Input string must be exactly one UTF-8 encoded code point long.",
        )?;
        return Ok(Value::Null);
    };
    let nf = match form {
        4 => NormalizationForm::Nfd,
        8 => NormalizationForm::Nfkd,
        32 | 48 => NormalizationForm::Nfkc,
        _ => NormalizationForm::Nfc,
    };
    match raw_decomposition(c, nf) {
        Some(d) => {
            let s: String = d.iter().collect();
            Ok(Value::from(s.as_str()))
        }
        None => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------- grapheme

fn text<'a>(vm: &mut Vm, a: &'a Args, i: usize, what: &str) -> R<Option<&'a str>> {
    match std::str::from_utf8(a.bytes(i)) {
        Ok(s) => Ok(Some(s)),
        Err(_) => {
            fail(
                vm,
                a,
                ue::U_INVALID_CHAR_FOUND,
                &format!("Error converting input string to UTF-16{what}"),
            )?;
            Ok(None)
        }
    }
}

fn g_strlen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    match std::str::from_utf8(a.bytes(0)) {
        Ok(s) => Ok(Value::Int(gr::strlen(s) as i64)),
        Err(_) => {
            fail(
                vm,
                a,
                ue::U_INVALID_CHAR_FOUND,
                "Error converting input string to UTF-16",
            )?;
            Ok(Value::Null)
        }
    }
}

fn offset_error<T>(vm: &mut Vm, a: &Args) -> R<T> {
    vm.fthrow(
        a,
        "ValueError",
        "Argument #3 ($offset) must be contained in argument #1 ($haystack)",
    )
}

/// The comparison of a search: exact bytes, or a collator's equality
/// (at secondary strength for the case-insensitive functions, ICU's
/// string search), None for an unusable locale.
fn search_eq(vm: &mut Vm, a: &Args, li: usize, ci: bool) -> R<Option<Box<dyn Fn(&str, &str) -> bool>>> {
    let loc = a.str(li).to_string_lossy();
    if !ci && loc.is_empty() {
        return Ok(Some(Box::new(|x: &str, y: &str| x == y)));
    }
    let lang = goro_ext_intl::locale::parse(&loc).language;
    if lang.len() > 8 {
        fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "Error creating search object")?;
        return Ok(None);
    }
    let id = if loc.is_empty() { super::default_locale(vm) } else { loc };
    let mut c = goro_ext_intl::collator::Collator::new(&id);
    if ci {
        c.set_attribute(goro_ext_intl::collator::STRENGTH, goro_ext_intl::collator::SECONDARY);
    }
    Ok(Some(Box::new(move |x: &str, y: &str| {
        x == y || c.compare(x, y) == std::cmp::Ordering::Equal
    })))
}

fn pos(vm: &mut Vm, a: &Args, reverse: bool, ci: bool) -> R<Value> {
    reset(vm);
    let Some(h) = text(vm, a, 0, "")? else {
        return Ok(Value::Bool(false));
    };
    let Some(n) = text(vm, a, 1, "")? else {
        return Ok(Value::Bool(false));
    };
    let off = a.int_or(2, 0);
    let Some(eq) = search_eq(vm, a, 3, ci)? else {
        return Ok(Value::Bool(false));
    };
    let r = if reverse {
        gr::strrpos_with(h, n, off, &*eq)
    } else {
        gr::strpos_with(h, n, off, &*eq)
    };
    match r {
        Ok(Some(i)) => Ok(Value::Int(i as i64)),
        Ok(None) => Ok(Value::Bool(false)),
        Err(_) => offset_error(vm, a),
    }
}

fn g_strpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos(vm, a, false, false)
}

fn g_stripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos(vm, a, false, true)
}

fn g_strrpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos(vm, a, true, false)
}

fn g_strripos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pos(vm, a, true, true)
}

fn g_substr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(s) = text(vm, a, 0, "")? else {
        return Ok(Value::Bool(false));
    };
    let off = a.int(1);
    let len = a.opt_int(2);
    match gr::substr(s, off, len) {
        Some(r) => Ok(Value::from(r.as_str())),
        None => Ok(Value::Bool(false)),
    }
}

fn strstr(vm: &mut Vm, a: &Args, ci: bool) -> R<Value> {
    reset(vm);
    let Some(h) = text(vm, a, 0, "")? else {
        return Ok(Value::Bool(false));
    };
    let Some(n) = text(vm, a, 1, "")? else {
        return Ok(Value::Bool(false));
    };
    let Some(eq) = search_eq(vm, a, 3, ci)? else {
        return Ok(Value::Bool(false));
    };
    match gr::strpos_with(h, n, 0, &*eq) {
        Ok(Some(i)) => {
            let at = gr::boundaries(h)[i];
            let r = if a.bool_or(2, false) { &h[..at] } else { &h[at..] };
            Ok(Value::from(r))
        }
        _ => Ok(Value::Bool(false)),
    }
}

fn g_strstr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    strstr(vm, a, false)
}

fn g_stristr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    strstr(vm, a, true)
}

fn g_extract(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let b = a.bytes(0).to_vec();
    let size = a.int(1);
    let ty = a.int_or(2, 0);
    let mut off = a.int_or(3, 0);
    if !(0..=2).contains(&ty) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($type) must be one of GRAPHEME_EXTR_COUNT, GRAPHEME_EXTR_MAXBYTES, or GRAPHEME_EXTR_MAXCHARS",
        );
    }
    if off < 0 {
        off += b.len() as i64;
    }
    if off < 0 || off as usize >= b.len() {
        return Ok(Value::Bool(false));
    }
    if size < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($size) must be greater than or equal to 0",
        );
    }
    // a start inside a character moves to the next one
    let mut start = off as usize;
    while start < b.len() && (b[start] & 0xC0) == 0x80 {
        start += 1;
    }
    if start >= b.len() {
        return Ok(Value::Bool(false));
    }
    let rest = &b[start..];
    let valid = match std::str::from_utf8(rest) {
        Ok(s) => s.to_string(),
        Err(e) => String::from_utf8_lossy(&rest[..e.valid_up_to()]).into_owned(),
    };
    let (out, next) = gr::extract(&valid, size as usize, ty, 0);
    if a.has(4)
        && let Some(r) = a.ref_at(4)
    {
        let v = vm.typed_ref_value(&r, Value::Int((start + next) as i64))?;
        drop(r.set(v));
    }
    Ok(Value::from(out.as_str()))
}

fn g_str_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let len = a.int_or(1, 1);
    if !(1..=1073741823).contains(&len) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than 0 and less than or equal to 1073741823",
        );
    }
    let Some(s) = text(vm, a, 0, "")? else {
        return Ok(Value::Bool(false));
    };
    let parts = gr::str_split(s, len as usize);
    Ok(Value::Array(PhpArray::from_values(
        parts.iter().map(|p| Value::from(p.as_str())),
    )))
}

fn g_levenshtein(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let names = ["insertion_cost", "replacement_cost", "deletion_cost"];
    let mut costs = [1i64; 3];
    for (k, name) in names.iter().enumerate() {
        let c = a.int_or(2 + k, 1);
        if c <= 0 || c > i64::MAX / 4 {
            return vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #{} (${name}) must be greater than 0 and less than or equal to {}",
                    3 + k,
                    i64::MAX / 4
                ),
            );
        }
        costs[k] = c;
    }
    let Some(x) = text(vm, a, 0, "")? else {
        return Ok(Value::Bool(false));
    };
    let Some(y) = text(vm, a, 1, "")? else {
        return Ok(Value::Bool(false));
    };
    let loc = a.str(5).to_string_lossy();
    let lang = goro_ext_intl::locale::parse(&loc).language;
    if lang.len() > 8 {
        fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "Error on ucol_open")?;
        return Ok(Value::Bool(false));
    }
    let coll = goro_ext_intl::collator::Collator::new(if loc.is_empty() { "root" } else { &loc });
    let same = |p: &str, q: &str| p == q || coll.compare(p, q) == std::cmp::Ordering::Equal;
    Ok(Value::Int(gr::levenshtein(
        x, y, costs[0], costs[1], costs[2], &same,
    )))
}

// ---------------------------------------------------------------- IDN

fn idn(vm: &mut Vm, a: &Args, ascii: bool) -> R<Value> {
    reset(vm);
    let d = a.bytes(0);
    let flags = a.int_or(1, 0);
    let variant = a.int_or(2, 1);
    if variant != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($flags) must be INTL_IDNA_VARIANT_UTS46",
        );
    }
    if d.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($domain) must not be empty");
    }
    if d.len() > i32::MAX as usize {
        return vm.fthrow(a, "ValueError", "Argument #1 ($domain) is too long");
    }
    let Ok(s) = std::str::from_utf8(d) else {
        fail(
            vm,
            a,
            ue::U_INVALID_CHAR_FOUND,
            "Error converting input string to UTF-16",
        )?;
        return Ok(Value::Bool(false));
    };
    let r = if ascii {
        idna::to_ascii(s, flags)
    } else {
        idna::to_unicode(s, flags)
    };
    if a.has(3)
        && let Some(rf) = a.ref_at(3)
    {
        let mut info = PhpArray::new();
        info.insert(Key::from_bytes(b"result"), Value::from(r.result.as_str()));
        info.insert(
            Key::from_bytes(b"isTransitionalDifferent"),
            Value::Bool(r.is_transitional_different),
        );
        info.insert(Key::from_bytes(b"errors"), Value::Int(r.errors));
        let v = vm.typed_ref_value(&rf, Value::Array(info))?;
        drop(rf.set(v));
    }
    if r.errors != 0 {
        return Ok(Value::Bool(false));
    }
    Ok(Value::from(r.result.as_str()))
}

fn idn_to_ascii(vm: &mut Vm, a: &mut Args) -> R<Value> {
    idn(vm, a, true)
}

fn idn_to_utf8(vm: &mut Vm, a: &mut Args) -> R<Value> {
    idn(vm, a, false)
}
