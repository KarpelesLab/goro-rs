//! ext/intl/normalizer: Normalizer, normalizer_normalize(),
//! normalizer_is_normalized(), normalizer_get_raw_decomposition().

use goro_core::value::Value;
use goro_core::vm::Vm;
use icu_normalizer::properties::{CanonicalDecompositionBorrowed, Decomposed};
use intl::unicode as un;

use crate::error::{self, U_ILLEGAL_ARGUMENT_ERROR};
use crate::{Call, s, sv};

pub const FORM_D: i64 = 4;
pub const FORM_KD: i64 = 8;
pub const FORM_C: i64 = 16;
pub const FORM_KC: i64 = 32;
pub const FORM_KC_CF: i64 = 48;

crate::intl_functions! { pub FUNCTIONS:
    "normalizer_normalize" => normalize,
    "normalizer_is_normalized" => is_normalized,
    "normalizer_get_raw_decomposition" => get_raw_decomposition,
}

pub const METHODS: &[(&str, crate::Imp)] = &[];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("Normalizer", None)];

/// Normalize valid UTF-8 text to `form`.
pub fn normalize_str(form: i64, t: &str) -> String {
    match form {
        FORM_D => un::nfd(t.chars()).collect(),
        FORM_KD => un::nfkd(t.chars()).collect(),
        FORM_KC => un::nfkc(t.chars()).collect(),
        FORM_KC_CF => nfkc_casefold(t),
        _ => un::nfc(t.chars()).collect(),
    }
}

/// toNFKC_Casefold(): NFKC, case folding and the removal of
/// Default_Ignorable_Code_Point characters, to a fixed point.
pub fn nfkc_casefold(t: &str) -> String {
    let strip = |s: String| -> String { s.chars().filter(|&c| !un::is_default_ignorable(c)).collect() };
    let mut cur = strip(un::nfd(t.chars()).collect());
    for _ in 0..4 {
        let k: String = un::nfkc(cur.chars()).collect();
        let folded: String = un::fold(k.chars()).collect();
        let next = strip(un::nfkc(folded.chars()).collect());
        if next == cur {
            break;
        }
        cur = next;
    }
    un::nfc(cur.chars()).collect()
}

fn valid_form(form: i64) -> bool {
    matches!(form, FORM_D | FORM_KD | FORM_C | FORM_KC | FORM_KC_CF)
}

/// normalizer_normalize()
fn normalize(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let input = c.str(0);
    let form = c.long(1);
    if !valid_form(form) {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            "form",
            "must be a a valid normalization form",
        );
        return Value::Null;
    }
    let Ok(t) = std::str::from_utf8(&input) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting input string to UTF-16",
        );
        return Value::False;
    };
    s(normalize_str(form, t))
}

/// normalizer_is_normalized()
fn is_normalized(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let input = c.str(0);
    let form = c.long(1);
    if !valid_form(form) {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            "form",
            "must be a a valid normalization form",
        );
        return Value::Null;
    }
    let Ok(t) = std::str::from_utf8(&input) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting string to UTF-16.",
        );
        return Value::False;
    };
    let r = match form {
        FORM_D => un::is_nfd(t.chars()),
        FORM_KD => un::is_nfkd(t.chars()),
        FORM_KC => un::is_nfkc(t.chars()),
        FORM_KC_CF => nfkc_casefold(t) == t,
        _ => un::is_nfc(t.chars()),
    };
    crate::bool_v(r)
}

/// U8_NEXT(): the code point at the start of `s` (negative when
/// ill-formed) and the number of bytes consumed.
fn u8_next(s: &[u8]) -> (i32, usize) {
    if s.is_empty() {
        return (-1, 1);
    }
    let b = s[0];
    if b < 0x80 {
        return (b as i32, 1);
    }
    let (n, min, init) = match b {
        0xC2..=0xDF => (2, 0x80, (b & 0x1F) as u32),
        0xE0..=0xEF => (3, 0x800, (b & 0x0F) as u32),
        0xF0..=0xF4 => (4, 0x10000, (b & 0x07) as u32),
        _ => return (-1, 1),
    };
    let mut c = init;
    let mut i = 1;
    while i < n {
        match s.get(i) {
            Some(&t) if t & 0xC0 == 0x80 => {
                c = (c << 6) | (t & 0x3F) as u32;
                i += 1;
            }
            _ => return (-1, i),
        }
    }
    if c < min || (0xD800..=0xDFFF).contains(&c) || c > 0x10FFFF {
        return (-1, i);
    }
    (c as i32, n)
}

/// The Decomposition_Mapping of `ch` (one level), for the canonical or the
/// compatibility forms.
fn raw_decomposition(ch: char, compat: bool) -> Option<String> {
    let cd = CanonicalDecompositionBorrowed::new();
    match cd.decompose(ch) {
        Decomposed::Default => {}
        Decomposed::Singleton(a) => return Some(a.to_string()),
        Decomposed::Expansion(a, b) => return Some(format!("{}{}", a, b)),
    }
    if !compat {
        return None;
    }
    let one = ch.to_string();
    let k: String = un::nfkd(one.chars()).collect();
    if k == one {
        return None;
    }
    Some(un::nfc(k.chars()).collect())
}

/// normalizer_get_raw_decomposition()
fn get_raw_decomposition(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let input = c.str(0);
    let form = c.long(1);
    let (cp, off) = u8_next(&input);
    if off != input.len() {
        error::set_global(
            vm,
            &c.name,
            U_ILLEGAL_ARGUMENT_ERROR,
            "Input string must be exactly one UTF-8 encoded code point long.",
        );
        return Value::Null;
    }
    if !(0..=0x10FFFF).contains(&cp) {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "Code point out of range");
        return Value::Null;
    }
    let Some(ch) = char::from_u32(cp as u32) else {
        return Value::Null;
    };
    match raw_decomposition(ch, matches!(form, FORM_KC | FORM_KD | FORM_KC_CF)) {
        Some(d) => sv(d.into_bytes()),
        None => Value::Null,
    }
}
