//! ext/intl/idn: idn_to_ascii(), idn_to_utf8() — ICU's UTS #46
//! implementation (uts46.cpp) with its option flags (transitional
//! processing, STD3 rules, CheckBidi, CheckJoiners, CONTEXTO) and the
//! UIDNA_ERROR_* bits of idna_info. The mapping step is ICU4X's UTS #46
//! normalizer (intl's IDNA has no options / error details).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use icu_normalizer::uts46::Uts46MapperBorrowed;
use intl::unicode as un;
use intl::unicode::BidiClass as B;

use crate::error;
use crate::{Call, sv};

const USE_STD3_RULES: i64 = 2;
const CHECK_BIDI: i64 = 4;
const CHECK_CONTEXTJ: i64 = 8;
const NONTRANSITIONAL_TO_ASCII: i64 = 0x10;
const NONTRANSITIONAL_TO_UNICODE: i64 = 0x20;
const CHECK_CONTEXTO: i64 = 0x40;

const E_EMPTY_LABEL: u32 = 1;
const E_LABEL_TOO_LONG: u32 = 2;
const E_DOMAIN_NAME_TOO_LONG: u32 = 4;
const E_LEADING_HYPHEN: u32 = 8;
const E_TRAILING_HYPHEN: u32 = 0x10;
const E_HYPHEN_3_4: u32 = 0x20;
const E_LEADING_COMBINING_MARK: u32 = 0x40;
const E_DISALLOWED: u32 = 0x80;
const E_PUNYCODE: u32 = 0x100;
const E_LABEL_HAS_DOT: u32 = 0x200;
const E_INVALID_ACE_LABEL: u32 = 0x400;
const E_BIDI: u32 = 0x800;
const E_CONTEXTJ: u32 = 0x1000;
const E_CONTEXTO_PUNCTUATION: u32 = 0x2000;
const E_CONTEXTO_DIGITS: u32 = 0x4000;

crate::intl_functions! { pub FUNCTIONS:
    "idn_to_ascii" => idn_to_ascii,
    "idn_to_utf8" => idn_to_utf8,
}

pub const METHODS: &[(&str, crate::Imp)] = &[];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[];

// ---------------------------------------------------------------------------
// Punycode (RFC 3492)

const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;

fn adapt(mut delta: u32, num: u32, first: bool) -> u32 {
    delta = if first { delta / DAMP } else { delta / 2 };
    delta += delta / num;
    let mut k = 0;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (BASE - TMIN + 1) * delta / (delta + SKEW)
}

fn digit(d: u32) -> char {
    (if d < 26 { b'a' + d as u8 } else { b'0' + (d - 26) as u8 }) as char
}

fn punycode_encode(input: &[char]) -> Option<String> {
    let mut out: String = input.iter().filter(|c| c.is_ascii()).collect();
    let b = out.chars().count() as u32;
    let mut h = b;
    if b > 0 {
        out.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let total = input.len() as u32;
    while h < total {
        let m = input.iter().map(|&c| c as u32).filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for &c in input {
            let c = c as u32;
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

fn punycode_decode(input: &str) -> Option<Vec<char>> {
    let (basic, rest) = match input.rfind('-') {
        Some(p) => (&input[..p], &input[p + 1..]),
        None => ("", input),
    };
    if !basic.is_ascii() {
        return None;
    }
    let mut out: Vec<char> = basic.chars().collect();
    let mut n = INITIAL_N;
    let mut i: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let mut it = rest.chars().peekable();
    while it.peek().is_some() {
        let old_i = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            let c = it.next()?;
            let d = match c {
                'a'..='z' => c as u32 - 'a' as u32,
                'A'..='Z' => c as u32 - 'A' as u32,
                '0'..='9' => c as u32 - '0' as u32 + 26,
                _ => return None,
            };
            i = i.checked_add(d.checked_mul(w)?)?;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if d < t {
                break;
            }
            w = w.checked_mul(BASE - t)?;
            k += BASE;
        }
        let len = out.len() as u32 + 1;
        bias = adapt(i - old_i, len, old_i == 0);
        n = n.checked_add(i / len)?;
        i %= len;
        let ch = char::from_u32(n)?;
        if out.len() > 1000 {
            return None;
        }
        out.insert(i as usize, ch);
        i += 1;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// UTS #46

fn is_deviation(c: char) -> bool {
    matches!(c, 'ß' | 'ς' | '\u{200C}' | '\u{200D}')
}

/// The Map + Normalize steps; transitional processing maps the deviation
/// characters.
fn map(s: &[char], transitional: bool) -> Vec<char> {
    let m = Uts46MapperBorrowed::new();
    let pre: Vec<char> = if transitional {
        let mut v = Vec::with_capacity(s.len());
        for &c in s {
            match c {
                'ß' => v.extend(['s', 's']),
                'ẞ' => v.extend(['s', 's']),
                'ς' => v.push('σ'),
                '\u{200C}' | '\u{200D}' => {}
                c => v.push(c),
            }
        }
        v
    } else {
        s.to_vec()
    };
    let out: Vec<char> = m.map_normalize(pre.into_iter()).collect();
    if transitional {
        // mappings producing deviation characters (ẞ -> ß...)
        let mut v = Vec::with_capacity(out.len());
        for c in out {
            match c {
                'ß' => v.extend(['s', 's']),
                'ς' => v.push('σ'),
                '\u{200C}' | '\u{200D}' => {}
                c => v.push(c),
            }
        }
        return v;
    }
    out
}

/// Whether a (mapped) label is valid per the UTS #46 validity criteria
/// (NFC, no mapped / disallowed characters).
fn valid_ace_label(label: &[char]) -> bool {
    let m = Uts46MapperBorrowed::new();
    let v: Vec<char> = m.normalize_validate(label.iter().copied()).collect();
    v == label
}

fn is_bidi_rtl(c: char) -> bool {
    matches!(un::bidi_class(c), B::R | B::AL | B::AN)
}

/// RFC 5893 rules on one label.
fn bidi_ok(label: &[char]) -> bool {
    let Some(&first) = label.first() else {
        return true;
    };
    let fc = un::bidi_class(first);
    let classes: Vec<B> = label.iter().map(|&c| un::bidi_class(c)).collect();
    let last = classes
        .iter()
        .rev()
        .find(|c| **c != B::NSM)
        .copied()
        .unwrap_or(fc);
    match fc {
        B::L => {
            classes
                .iter()
                .all(|c| matches!(c, B::L | B::EN | B::ES | B::CS | B::ET | B::ON | B::BN | B::NSM))
                && matches!(last, B::L | B::EN)
        }
        B::R | B::AL => {
            let ok = classes.iter().all(|c| {
                matches!(
                    c,
                    B::R | B::AL | B::AN | B::EN | B::ES | B::CS | B::ET | B::ON | B::BN | B::NSM
                )
            });
            let end = matches!(last, B::R | B::AL | B::EN | B::AN);
            let en = classes.contains(&B::EN);
            let an = classes.contains(&B::AN);
            ok && end && !(en && an)
        }
        _ => false,
    }
}

fn is_virama(c: char) -> bool {
    un::canonical_combining_class(c) == 9
}

/// CheckJoiners (RFC 5892 Appendix A.1, A.2).
fn contextj_ok(label: &[char]) -> bool {
    use un::JoiningType as J;
    for (i, &c) in label.iter().enumerate() {
        match c {
            '\u{200D}' => {
                if i == 0 || !is_virama(label[i - 1]) {
                    return false;
                }
            }
            '\u{200C}' => {
                if i > 0 && is_virama(label[i - 1]) {
                    continue;
                }
                // (L|D) T* ZWNJ T* (R|D)
                let mut j = i;
                let mut left = false;
                while j > 0 {
                    j -= 1;
                    match un::joining_type(label[j]) {
                        J::Transparent => continue,
                        J::LeftJoining | J::DualJoining => left = true,
                        _ => {}
                    }
                    break;
                }
                let mut right = false;
                for &x in &label[i + 1..] {
                    match un::joining_type(x) {
                        J::Transparent => continue,
                        J::RightJoining | J::DualJoining => right = true,
                        _ => {}
                    }
                    break;
                }
                if !(left && right) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// CONTEXTO rules (RFC 5892 Appendix A.3-A.9): the error bits.
fn contexto_errors(label: &[char]) -> u32 {
    let mut e = 0;
    let mut arabic = false;
    let mut ext = false;
    for (i, &c) in label.iter().enumerate() {
        match c {
            '\u{B7}' => {
                if !(i > 0 && label[i - 1] == 'l' && label.get(i + 1) == Some(&'l')) {
                    e |= E_CONTEXTO_PUNCTUATION;
                }
            }
            '\u{375}' => {
                if !label
                    .get(i + 1)
                    .is_some_and(|&n| un::script(n) == un::Script::Greek)
                {
                    e |= E_CONTEXTO_PUNCTUATION;
                }
            }
            '\u{5F3}' | '\u{5F4}' => {
                if !(i > 0 && un::script(label[i - 1]) == un::Script::Hebrew) {
                    e |= E_CONTEXTO_PUNCTUATION;
                }
            }
            '\u{30FB}' => {
                if !label.iter().any(|&x| {
                    matches!(
                        un::script(x),
                        un::Script::Hiragana | un::Script::Katakana | un::Script::Han
                    )
                }) {
                    e |= E_CONTEXTO_PUNCTUATION;
                }
            }
            '\u{660}'..='\u{669}' => arabic = true,
            '\u{6F0}'..='\u{6F9}' => ext = true,
            _ => {}
        }
    }
    if arabic && ext {
        e |= E_CONTEXTO_DIGITS;
    }
    e
}

/// UTS46::process(): (result, isTransitionalDifferent, errors).
fn process(domain: &str, options: i64, to_ascii: bool) -> (String, bool, u32) {
    let input: Vec<char> = domain.chars().collect();
    let transitional = if to_ascii {
        options & NONTRANSITIONAL_TO_ASCII == 0
    } else {
        options & NONTRANSITIONAL_TO_UNICODE == 0
    };
    let mapped_nt = map(&input, false);
    let trans_diff = mapped_nt.iter().any(|&c| is_deviation(c));
    let mapped = if transitional {
        map(&input, true)
    } else {
        mapped_nt
    };
    // labels (U+3002 & co are mapped to '.')
    let mut labels: Vec<Vec<char>> = mapped.split(|&c| c == '.').map(|l| l.to_vec()).collect();
    let mut errors: u32 = 0;
    let n = labels.len();
    let mut bidi_domain = false;
    let mut out_labels: Vec<String> = Vec::with_capacity(n);
    let mut checked: Vec<Vec<char>> = Vec::with_capacity(n);
    for (li, label) in labels.iter_mut().enumerate() {
        let mut lerr: u32 = 0;
        let is_last = li == n - 1;
        if label.is_empty() {
            // the root label (a trailing dot) is fine
            if !(is_last && n > 1) {
                lerr |= E_EMPTY_LABEL;
            }
            out_labels.push(String::new());
            checked.push(Vec::new());
            errors |= lerr;
            continue;
        }
        let mut uni: Vec<char> = label.clone();
        let mut was_ace = false;
        if label.len() >= 4
            && label[..4]
                .iter()
                .zip("xn--".chars())
                .all(|(a, b)| a.to_ascii_lowercase() == b)
        {
            was_ace = true;
            let rest: String = label[4..].iter().collect();
            match punycode_decode(&rest) {
                Some(d) if !rest.is_empty() => {
                    if !valid_ace_label(&d) {
                        lerr |= E_INVALID_ACE_LABEL;
                    }
                    if d.iter().all(|c| c.is_ascii()) {
                        lerr |= E_INVALID_ACE_LABEL;
                    }
                    uni = d;
                }
                _ => {
                    lerr |= E_PUNYCODE;
                }
            }
        }
        if lerr & E_PUNYCODE == 0 {
            if uni.len() >= 4 && uni[2] == '-' && uni[3] == '-' {
                lerr |= E_HYPHEN_3_4;
            }
            if uni.first() == Some(&'-') {
                lerr |= E_LEADING_HYPHEN;
            }
            if uni.last() == Some(&'-') {
                lerr |= E_TRAILING_HYPHEN;
            }
            for c in uni.iter_mut() {
                if *c == '\u{FFFD}' {
                    lerr |= E_DISALLOWED;
                } else if was_ace && *c == '.' {
                    lerr |= E_LABEL_HAS_DOT;
                } else if options & USE_STD3_RULES != 0
                    && c.is_ascii()
                    && !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                {
                    lerr |= E_DISALLOWED;
                    *c = '\u{FFFD}';
                }
            }
            if uni.first().is_some_and(|&c| un::is_mark(c)) {
                lerr |= E_LEADING_COMBINING_MARK;
                uni[0] = '\u{FFFD}';
            }
            if uni.iter().any(|&c| is_bidi_rtl(c)) {
                bidi_domain = true;
            }
            if options & CHECK_CONTEXTJ != 0 && !contextj_ok(&uni) {
                lerr |= E_CONTEXTJ;
            }
            if options & CHECK_CONTEXTO != 0 {
                lerr |= contexto_errors(&uni);
            }
        }
        let severe = E_LEADING_COMBINING_MARK | E_DISALLOWED | E_PUNYCODE | E_LABEL_HAS_DOT;
        let text: String = if lerr & E_INVALID_ACE_LABEL != 0 {
            // the label kept, marked with U+FFFD
            let mut t: String = label.iter().collect();
            t.push('\u{FFFD}');
            t
        } else if to_ascii {
            if was_ace {
                label.iter().collect::<String>().to_ascii_lowercase()
            } else if uni.iter().all(|c| c.is_ascii()) || lerr & severe != 0 {
                uni.iter().collect()
            } else {
                match punycode_encode(&uni) {
                    Some(p) => format!("xn--{}", p),
                    None => {
                        lerr |= E_PUNYCODE;
                        uni.iter().collect()
                    }
                }
            }
        } else if lerr & E_PUNYCODE != 0 {
            label.iter().collect()
        } else {
            uni.iter().collect()
        };
        if to_ascii && text.len() > 63 {
            lerr |= E_LABEL_TOO_LONG;
        }
        errors |= lerr;
        out_labels.push(text);
        checked.push(uni);
    }
    if options & CHECK_BIDI != 0 && bidi_domain {
        for l in &checked {
            if !l.is_empty() && !bidi_ok(l) {
                errors |= E_BIDI;
            }
        }
    }
    let result = out_labels.join(".");
    if to_ascii {
        let len = result.len() - usize::from(result.ends_with('.'));
        if len > 253 {
            errors |= E_DOMAIN_NAME_TOO_LONG;
        }
    }
    (result, trans_diff, errors)
}

fn handoff(vm: &mut Vm, c: &mut Call, to_ascii: bool) -> Value {
    error::reset_global();
    let domain = c.str(0);
    let options = c.long(1);
    let variant = c.long(2);
    if domain.is_empty() {
        crate::zpp::value_error(vm, &c.name, 1, "domain", "must not be empty");
        return Value::Null;
    }
    if variant != 1 {
        crate::zpp::value_error(vm, &c.name, 2, "flags", "must be INTL_IDNA_VARIANT_UTS46");
        return Value::Null;
    }
    let want_info = c.passed(3);
    if want_info && !c.set_ref(vm, 3, crate::arr(PhpArray::new())) {
        return Value::Null;
    }
    let text = String::from_utf8_lossy(&domain).into_owned();
    let (mut res, diff, mut errs) = process(&text, options, to_ascii);
    if std::str::from_utf8(&domain).is_err() {
        errs |= E_DISALLOWED;
        res = String::from_utf8_lossy(&domain).into_owned();
    }
    let cap = if to_ascii { 255 } else { 252 * 4 };
    if res.len() >= cap {
        return Value::False;
    }
    if want_info {
        let mut a = PhpArray::new();
        let k = |s: &str| ArrayKey::String(PhpString::from_bytes(s.as_bytes()));
        a.set(k("result"), sv(res.clone().into_bytes()));
        a.set(k("isTransitionalDifferent"), crate::bool_v(diff));
        a.set(k("errors"), Value::Long(errs as i64));
        c.set_ref(vm, 3, crate::arr(a));
    }
    if errs == 0 {
        sv(res.into_bytes())
    } else {
        Value::False
    }
}

fn idn_to_ascii(vm: &mut Vm, c: &mut Call) -> Value {
    handoff(vm, c, true)
}

fn idn_to_utf8(vm: &mut Vm, c: &mut Call) -> Value {
    handoff(vm, c, false)
}
