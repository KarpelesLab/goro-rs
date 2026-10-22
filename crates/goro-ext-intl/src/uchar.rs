//! ext/intl/uchar: IntlChar, ICU's uchar.h API over intl's character
//! properties. The property and value names / numbers are ICU's
//! (uchar_tables.rs); intl's enumerations are mapped to ICU's values by
//! name.

use std::cell::RefCell;
use std::collections::HashMap;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;
use intl::unicode as un;
use intl::unicode::GeneralCategory as G;

use crate::error::{self, U_ILLEGAL_ARGUMENT_ERROR};
use crate::uset::{self, loose};
use crate::{Call, s, sv};

#[path = "uchar_tables.rs"]
mod tables;

const NO_NUMERIC_VALUE: f64 = -123456789.0;

crate::intl_functions! { pub FUNCTIONS: }

/// The methods: the boolean u_isXxx() ones generated, then the others.
macro_rules! methods_table {
    ($($name:literal => $f:expr),* $(,)?; $($other:expr),* $(,)?) => {
        pub const METHODS: &[(&str, crate::Imp)] = &[$(($name, {
            fn w(vm: &mut Vm, c: &mut Call) -> Value {
                match cp_arg(vm, c, 0) {
                    Some(cp) => crate::bool_v(($f)(cp)),
                    None => Value::Null,
                }
            }
            w
        })),*, $($other),*];
    };
}

methods_table! {
    "IntlChar::isUAlphabetic" => |c| binary(c, 0),
    "IntlChar::isULowercase" => |c| binary(c, 22),
    "IntlChar::isUUppercase" => |c| binary(c, 30),
    "IntlChar::isUWhiteSpace" => |c| binary(c, 31),
    "IntlChar::islower" => |c| gc(c) == G::LowercaseLetter,
    "IntlChar::isupper" => |c| gc(c) == G::UppercaseLetter,
    "IntlChar::istitle" => |c| gc(c) == G::TitlecaseLetter,
    "IntlChar::isdigit" => |c| gc(c) == G::DecimalNumber,
    "IntlChar::isalpha" => |c| gc(c).is_letter(),
    "IntlChar::isalnum" => |c| gc(c).is_letter() || gc(c) == G::DecimalNumber,
    "IntlChar::isxdigit" => is_xdigit,
    "IntlChar::ispunct" => |c| gc(c).is_punctuation(),
    "IntlChar::isgraph" => |c| is_graph(c) && gc(c) != G::Format,
    "IntlChar::isblank" => |c| c == 9 || gc(c) == G::SpaceSeparator,
    "IntlChar::isdefined" => |c| gc(c) != G::Unassigned,
    "IntlChar::isspace" => |c| gc(c).is_separator() || (9..=0xD).contains(&c) || (0x1C..=0x1F).contains(&c),
    "IntlChar::isJavaSpaceChar" => |c| gc(c).is_separator(),
    "IntlChar::isWhitespace" => is_java_whitespace,
    "IntlChar::iscntrl" => |c| matches!(gc(c), G::Control | G::Format | G::LineSeparator | G::ParagraphSeparator),
    "IntlChar::isISOControl" => |c| c <= 0x1F || (0x7F..=0x9F).contains(&c),
    "IntlChar::isprint" => |c| !gc(c).is_other(),
    "IntlChar::isbase" => |c| gc(c).is_letter() || gc(c).is_number() || matches!(gc(c), G::SpacingMark | G::EnclosingMark),
    "IntlChar::isMirrored" => |c| binary(c, 3),
    "IntlChar::isIDStart" => |c| binary(c, 16),
    "IntlChar::isIDPart" => |c| binary(c, 15),
    "IntlChar::isIDIgnorable" => is_id_ignorable,
    "IntlChar::isJavaIDStart" => |c| gc(c).is_letter() || matches!(gc(c), G::CurrencySymbol | G::ConnectorPunctuation),
    "IntlChar::isJavaIDPart" => is_java_id_part;
    ("IntlChar::chr", chr),
    ("IntlChar::ord", ord),
    ("IntlChar::hasBinaryProperty", has_binary_property),
    ("IntlChar::getIntPropertyValue", get_int_property_value),
    ("IntlChar::getIntPropertyMinValue", get_int_property_min_value),
    ("IntlChar::getIntPropertyMaxValue", get_int_property_max_value),
    ("IntlChar::getNumericValue", get_numeric_value),
    ("IntlChar::enumCharTypes", enum_char_types),
    ("IntlChar::getBlockCode", get_block_code),
    ("IntlChar::charName", char_name),
    ("IntlChar::charFromName", char_from_name),
    ("IntlChar::enumCharNames", enum_char_names),
    ("IntlChar::getPropertyName", get_property_name),
    ("IntlChar::getPropertyEnum", get_property_enum),
    ("IntlChar::getPropertyValueName", get_property_value_name),
    ("IntlChar::getPropertyValueEnum", get_property_value_enum),
    ("IntlChar::foldCase", fold_case),
    ("IntlChar::digit", digit),
    ("IntlChar::forDigit", for_digit),
    ("IntlChar::charAge", char_age),
    ("IntlChar::getUnicodeVersion", get_unicode_version),
    ("IntlChar::getFC_NFKC_Closure", get_fc_nfkc_closure),
    ("IntlChar::charDirection", char_direction),
    ("IntlChar::charType", char_type),
    ("IntlChar::getCombiningClass", get_combining_class),
    ("IntlChar::charDigitValue", char_digit_value),
    ("IntlChar::charMirror", char_mirror),
    ("IntlChar::tolower", tolower),
    ("IntlChar::toupper", toupper),
    ("IntlChar::totitle", totitle),
    ("IntlChar::getBidiPairedBracket", get_bidi_paired_bracket),
}

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("IntlChar", None)];

// ---------------------------------------------------------------------------
// Code point arguments

/// convert_cp(): an int code point or a one-character UTF-8 string.
fn cp_arg(vm: &mut Vm, c: &Call, i: usize) -> Option<u32> {
    let v = c.arg(i);
    let cp: i64 = match &v {
        Value::String(st) => {
            let b = st.as_bytes();
            let ch = crate::breakiter::decode_native(b);
            let ok = ch.len() == 1
                && (ch[0].1 != 0xFFFD || b == "\u{FFFD}".as_bytes());
            if !ok {
                error::set_global(
                    vm,
                    &c.name,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "Passing a UTF-8 character for codepoint requires a string which is exactly one UTF-8 codepoint long.",
                );
                return None;
            }
            ch[0].1 as i64
        }
        v => v.to_long(),
    };
    if !(0..=0x10FFFF).contains(&cp) {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "Codepoint out of range");
        return None;
    }
    Some(cp as u32)
}

fn is_string_arg(c: &Call, i: usize) -> bool {
    matches!(c.arg(i), Value::String(_))
}

fn ch(cp: u32) -> char {
    char::from_u32(cp).unwrap_or('\u{FFFF}')
}

fn gc(cp: u32) -> G {
    un::general_category_u32(cp)
}

fn utf8(cp: u32) -> Value {
    let mut b = Vec::new();
    crate::common::push_utf8(&mut b, cp);
    sv(b)
}

/// A code point result: a string when the argument was a string.
fn cp_result(c: &Call, cp: u32) -> Value {
    if is_string_arg(c, 0) {
        utf8(cp)
    } else {
        Value::Long(cp as i64)
    }
}

// ---------------------------------------------------------------------------
// Properties

/// The ICU value of an enumerated property given its value name.
fn value_by_name(prop: i64, name: &str) -> Option<i64> {
    thread_local! {
        static IDX: RefCell<HashMap<(i64, String), i64>> = RefCell::new(HashMap::new());
    }
    IDX.with(|m| {
        let mut m = m.borrow_mut();
        if m.is_empty() {
            for (p, v, names) in tables::VALUES {
                for n in *names {
                    if !n.is_empty() {
                        m.entry((*p, loose(n))).or_insert(*v);
                    }
                }
            }
        }
        m.get(&(prop, loose(name))).copied()
    })
}

fn icu_gc(cp: u32) -> i64 {
    value_by_name(0x1005, gc(cp).abbr()).unwrap_or(0)
}

fn icu_bidi(cp: u32) -> i64 {
    let n = format!("{:?}", un::bidi_class(ch(cp)));
    value_by_name(0x1000, &n).unwrap_or(0)
}

fn block_code(cp: u32) -> i64 {
    let n = un::block(ch(cp));
    if n.is_empty() || cp > 0x10FFFF {
        return 0;
    }
    value_by_name(0x1001, n)
        .or_else(|| {
            // ICU's names for a few blocks
            let alias = match n {
                "Greek and Coptic" => "Greek",
                "Cyrillic Supplement" => "Cyrillic_Supplementary",
                "Combining Diacritical Marks for Symbols" => "Combining_Marks_For_Symbols",
                _ => return None,
            };
            value_by_name(0x1001, alias)
        })
        .unwrap_or(0)
}

fn script_code(cp: u32) -> i64 {
    value_by_name(0x100A, un::script_u32(cp).long_name()).unwrap_or(0)
}

fn is_xdigit(c: u32) -> bool {
    matches!(c, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66 | 0xFF10..=0xFF19 | 0xFF21..=0xFF26 | 0xFF41..=0xFF46)
        || gc(c) == G::DecimalNumber
}

fn is_graph(c: u32) -> bool {
    !matches!(gc(c), G::Control | G::Surrogate | G::Unassigned) && !gc(c).is_separator()
}

fn is_java_whitespace(c: u32) -> bool {
    (gc(c).is_separator() && !matches!(c, 0xA0 | 0x2007 | 0x202F))
        || (9..=0xD).contains(&c)
        || (0x1C..=0x1F).contains(&c)
}

fn is_id_ignorable(c: u32) -> bool {
    c <= 8 || (0xE..=0x1B).contains(&c) || (0x7F..=0x9F).contains(&c) || gc(c) == G::Format
}

fn is_java_id_part(c: u32) -> bool {
    let g = gc(c);
    g.is_letter()
        || matches!(
            g,
            G::CurrencySymbol
                | G::ConnectorPunctuation
                | G::DecimalNumber
                | G::LetterNumber
                | G::SpacingMark
                | G::NonspacingMark
        )
        || is_id_ignorable(c)
}

fn in_ranges(c: u32, r: &[(u32, u32)]) -> bool {
    r.iter().any(|&(a, b)| a <= c && c <= b)
}

fn is_cased(c: char) -> bool {
    un::is_uppercase(c) || un::is_lowercase(c) || un::general_category(c) == G::TitlecaseLetter
}

fn is_case_ignorable(c: char) -> bool {
    matches!(
        un::general_category(c),
        G::NonspacingMark | G::EnclosingMark | G::Format | G::ModifierLetter | G::ModifierSymbol
    ) || matches!(
        c,
        '\'' | '.' | ':' | '^' | '`' | '\u{B7}' | '\u{387}' | '\u{55F}' | '\u{5F4}' | '\u{2018}' | '\u{2019}' | '\u{2024}' | '\u{2027}' | '\u{FE13}' | '\u{FE52}' | '\u{FE55}' | '\u{FF07}' | '\u{FF0E}' | '\u{FF1A}' | '\u{AD}'
    )
}

fn has_canonical_decomposition(c: char) -> bool {
    let d: Vec<char> = un::nfd(std::iter::once(c)).collect();
    d.len() != 1 || d[0] != c
}

/// u_hasBinaryProperty()
fn binary(cp: u32, prop: i64) -> bool {
    let Some(c) = char::from_u32(cp) else {
        // surrogates: normalization-inert
        return matches!(prop, 37..=41);
    };
    let g = un::general_category(c);
    match prop {
        0 => un::is_alphabetic(c),
        1 => c.is_ascii_hexdigit(),
        2 => in_ranges(cp, &[(0x61C, 0x61C), (0x200E, 0x200F), (0x202A, 0x202E), (0x2066, 0x2069)]),
        3 => un::is_bidi_mirrored(c),
        4 => un::is_dash(c),
        5 => un::is_default_ignorable(c),
        6 => matches!(cp, 0x149 | 0x673 | 0xF77 | 0xF79 | 0x17A3 | 0x17A4 | 0x206A..=0x206F | 0x2329 | 0x232A | 0xE0001),
        7 => un::is_diacritic(c),
        8 => matches!(
            cp,
            0xB7 | 0x2D0 | 0x2D1 | 0x640 | 0x7FA | 0xB55 | 0xE46 | 0xEC6 | 0x180A | 0x1843 | 0x1AA7 | 0x1C36 | 0x1C7B | 0x3005 | 0x3031..=0x3035 | 0x309D | 0x309E | 0x30FC..=0x30FE | 0xA015 | 0xA60C | 0xA9CF | 0xA9E6 | 0xAA70 | 0xAADD | 0xAAF3 | 0xAAF4 | 0xFF70
        ),
        9 => has_canonical_decomposition(c) && un::nfc(std::iter::once(c)).ne(std::iter::once(c)),
        10 => !matches!(g, G::Control | G::Format | G::Surrogate | G::PrivateUse | G::Unassigned | G::LineSeparator | G::ParagraphSeparator) && !binary(cp, 11),
        11 => matches!(g, G::NonspacingMark | G::EnclosingMark) || matches!(cp, 0x200C | 0xFF9E | 0xFF9F | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F),
        12 => un::canonical_combining_class(c) == 9 && g != G::SpacingMark || matches!(cp, 0xA8C4 | 0xA953),
        13 => un::is_hex_digit(c),
        14 => matches!(cp, 0x2D | 0xAD | 0x58A | 0x1806 | 0x2010 | 0x2011 | 0x2E17 | 0x30FB | 0xFE63 | 0xFF0D | 0xFF65),
        15 => un::is_xid_continue(c) || matches!(cp, 0x37A | 0x309B | 0x309C | 0xFC5E..=0xFC63 | 0xFDFA | 0xFDFB | 0xFE70..=0xFE7E | 0xFF9E | 0xFF9F),
        16 => un::is_xid_start(c) || matches!(cp, 0x37A | 0x309B | 0x309C | 0xFC5E..=0xFC63 | 0xFDFA | 0xFDFB | 0xFE70..=0xFE7E | 0xFF9E | 0xFF9F),
        17 => uset::is_ideographic(c),
        18 => in_ranges(cp, &[(0x2FF0, 0x2FF1), (0x2FF4, 0x2FFD), (0x31EF, 0x31EF)]),
        19 => in_ranges(cp, &[(0x2FF2, 0x2FF3)]),
        20 => un::is_join_control(c),
        21 => in_ranges(cp, &[(0xE40, 0xE44), (0xEC0, 0xEC4), (0x19B5, 0x19B7), (0x19BA, 0x19BA), (0xAAB5, 0xAAB6), (0xAAB9, 0xAAB9), (0xAABB, 0xAABC)]),
        22 => un::is_lowercase(c),
        23 => un::is_math(c),
        24 => uset::is_noncharacter(c),
        25 => un::is_quotation_mark(c),
        26 => in_ranges(cp, &[(0x2E80, 0x2E99), (0x2E9B, 0x2EF3), (0x2F00, 0x2FD5)]),
        27 => crate::spoofchecker::is_soft_dotted(c),
        28 => matches!(c, '!' | ',' | '.' | ':' | ';' | '?') || matches!(cp, 0x37E | 0x387 | 0x589 | 0x5C3 | 0x60C | 0x61B | 0x61D..=0x61F | 0x6D4 | 0x3001 | 0x3002 | 0xFF01 | 0xFF0C | 0xFF0E | 0xFF1A | 0xFF1B | 0xFF1F | 0xFF61 | 0xFF64),
        29 => uset::is_ideographic(c) && !matches!(cp, 0x3006 | 0x3007 | 0x3021..=0x3029 | 0x3038..=0x303A) && !in_ranges(cp, &[(0xF900, 0xFA0D), (0xFA10, 0xFA10), (0xFA12, 0xFA12), (0xFA15, 0xFA1E), (0xFA20, 0xFA20), (0xFA22, 0xFA22), (0xFA25, 0xFA26), (0xFA2A, 0xFAD9), (0x2F800, 0x2FA1D), (0x17000, 0x18CFF), (0x1B170, 0x1B2FB)]),
        30 => un::is_uppercase(c),
        31 => un::is_whitespace(c),
        32 => un::is_xid_continue(c),
        33 => un::is_xid_start(c),
        34 => un::changes_when_casemapped(c) || is_cased(c) && (un::changes_when_lowercased(c) || un::changes_when_uppercased(c)),
        35 => matches!(c, '!' | '.' | '?') || matches!(cp, 0x589 | 0x61D..=0x61F | 0x6D4 | 0x3002 | 0xFF01 | 0xFF0E | 0xFF1F | 0xFF61),
        36 => in_ranges(cp, &[(0x180B, 0x180D), (0x180F, 0x180F), (0xFE00, 0xFE0F), (0xE0100, 0xE01EF)]),
        37 => !has_canonical_decomposition(c) && un::canonical_combining_class(c) == 0,
        38 => un::nfkd(std::iter::once(c)).eq(std::iter::once(c)) && un::canonical_combining_class(c) == 0,
        39 => un::nfc(std::iter::once(c)).eq(std::iter::once(c)) && un::canonical_combining_class(c) == 0 && !combines_forward(c),
        40 => un::nfkc(std::iter::once(c)).eq(std::iter::once(c)) && un::canonical_combining_class(c) == 0 && !combines_forward(c),
        41 => un::canonical_combining_class(c) == 0,
        42 => in_ranges(cp, &[(0x21, 0x2F), (0x3A, 0x40), (0x5B, 0x5E), (0x60, 0x60), (0x7B, 0x7E), (0xA1, 0xA7), (0xA9, 0xA9), (0xAB, 0xAC), (0xAE, 0xAE), (0xB0, 0xB1), (0xB6, 0xB6), (0xBB, 0xBB), (0xBF, 0xBF), (0xD7, 0xD7), (0xF7, 0xF7), (0x2010, 0x2027), (0x2030, 0x203E), (0x2041, 0x2053), (0x2055, 0x205E), (0x2190, 0x245F), (0x2500, 0x2775), (0x2794, 0x2BFF), (0x2E00, 0x2E7F), (0x3001, 0x3003), (0x3008, 0x3020), (0x3030, 0x3030), (0xFD3E, 0xFD3F), (0xFE45, 0xFE46)]),
        43 => matches!(cp, 0x09..=0x0D | 0x20 | 0x85 | 0x200E | 0x200F | 0x2028 | 0x2029),
        44 => g.is_letter() || binary(cp, 0) || g == G::DecimalNumber,
        45 => cp == 9 || g == G::SpaceSeparator,
        46 => is_graph(cp),
        47 => is_graph(cp) && g != G::Control || g == G::SpaceSeparator,
        48 => is_xdigit(cp),
        49 => is_cased(c),
        50 => is_case_ignorable(c),
        51 => un::changes_when_lowercased(c),
        52 => un::changes_when_uppercased(c),
        53 => un::changes_when_titlecased(c),
        54 => un::changes_when_casefolded(c),
        55 => un::changes_when_casemapped(c),
        56 => crate::normalizer::nfkc_casefold(&c.to_string()) != c.to_string(),
        57 => is_emoji(cp),
        58 | 65 => is_emoji(cp) && cp >= 0x1F000 && !matches!(cp, 0x1F170 | 0x1F171 | 0x1F17E | 0x1F17F | 0x1F202 | 0x1F237 | 0x1F321..=0x1F32C | 0x1F336 | 0x1F37D | 0x1F396..=0x1F39F | 0x1F3CB..=0x1F3CE | 0x1F3D4..=0x1F3DF | 0x1F3F1..=0x1F3F3 | 0x1F3F5..=0x1F3F7),
        59 => (0x1F3FB..=0x1F3FF).contains(&cp),
        61 => matches!(cp, 0x23 | 0x2A | 0x30..=0x39 | 0x200D | 0x20E3 | 0xFE0F | 0x1F1E6..=0x1F1FF | 0x1F3FB..=0x1F3FF | 0x1F9B0..=0x1F9B3 | 0xE0020..=0xE007F),
        62 => (0x1F1E6..=0x1F1FF).contains(&cp),
        64 => is_ext_pict(cp),
        63 => matches!(cp, 0x600..=0x605 | 0x6DD | 0x70F | 0x890 | 0x891 | 0x8E2 | 0x110BD | 0x110CD),
        _ => false,
    }
}

/// Whether `c` composes with a following character (NFC).
fn combines_forward(c: char) -> bool {
    const MARKS: &[u32] = &[
        0x300, 0x301, 0x302, 0x303, 0x304, 0x306, 0x307, 0x308, 0x309, 0x30A, 0x30B, 0x30C, 0x30F,
        0x311, 0x313, 0x314, 0x31B, 0x323, 0x324, 0x325, 0x326, 0x327, 0x328, 0x32D, 0x32E, 0x330,
        0x331, 0x338, 0x342, 0x345, 0x653, 0x654, 0x655, 0x93C, 0x9BE, 0x9D7, 0xB3E, 0xB56, 0xB57,
        0xBBE, 0xBD7, 0xC56, 0xCC2, 0xCD5, 0xCD6, 0xD3E, 0xD57, 0xDCA, 0xDCF, 0xDDF, 0x102E, 0x1B35,
        0x3099, 0x309A, 0x1161, 0x11A8, 0x110BA, 0x11127, 0x1133E, 0x11357, 0x114B0, 0x114BA,
        0x114BD, 0x115AF, 0x11930,
    ];
    if (0xAC00..=0xD7A3).contains(&(c as u32)) {
        return (c as u32 - 0xAC00).is_multiple_of(28);
    }
    // a precomposed character can take more marks
    if has_canonical_decomposition(c) {
        return true;
    }
    MARKS.iter().any(|&m| {
        let m = char::from_u32(m).unwrap_or(' ');
        un::nfc([c, m].into_iter()).count() == 1
    })
}

fn is_ext_pict(cp: u32) -> bool {
    matches!(
        cp,
        0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x2199 | 0x21A9 | 0x21AA
            | 0x231A | 0x231B | 0x2328 | 0x2388 | 0x23CF | 0x23E9..=0x23F3 | 0x23F8..=0x23FA
            | 0x24C2 | 0x25AA | 0x25AB | 0x25B6 | 0x25C0 | 0x25FB..=0x25FE | 0x2600..=0x2605
            | 0x2607..=0x2612 | 0x2614..=0x2685 | 0x2690..=0x2705 | 0x2708..=0x2712 | 0x2714
            | 0x2716 | 0x271D | 0x2721 | 0x2728 | 0x2733 | 0x2734 | 0x2744 | 0x2747 | 0x274C
            | 0x274E | 0x2753..=0x2755 | 0x2757 | 0x2763..=0x2767 | 0x2795..=0x2797 | 0x27A1
            | 0x27B0 | 0x27BF | 0x2934 | 0x2935 | 0x2B05..=0x2B07 | 0x2B1B | 0x2B1C | 0x2B50
            | 0x2B55 | 0x3030 | 0x303D | 0x3297 | 0x3299 | 0x1F000..=0x1F0FF | 0x1F10D..=0x1F10F
            | 0x1F12F | 0x1F16C..=0x1F171 | 0x1F17E | 0x1F17F | 0x1F18E | 0x1F191..=0x1F19A
            | 0x1F1AD..=0x1F1E5 | 0x1F201..=0x1F20F | 0x1F21A | 0x1F22F | 0x1F232..=0x1F23A
            | 0x1F23C..=0x1F23F | 0x1F249..=0x1F3FA | 0x1F400..=0x1F53D | 0x1F546..=0x1F64F
            | 0x1F680..=0x1F6FF | 0x1F774..=0x1F77F | 0x1F7D5..=0x1F7FF | 0x1F80C..=0x1F80F
            | 0x1F848..=0x1F84F | 0x1F85A..=0x1F85F | 0x1F888..=0x1F88F | 0x1F8AE..=0x1F8FF
            | 0x1F90C..=0x1F93A | 0x1F93C..=0x1F945 | 0x1F947..=0x1FAFF | 0x1FC00..=0x1FFFD
    )
}

fn is_emoji(cp: u32) -> bool {
    matches!(cp, 0x23 | 0x2A | 0x30..=0x39 | 0xA9 | 0xAE | 0x1F1E6..=0x1F1FF | 0x1F3FB..=0x1F3FF)
        || (is_ext_pict(cp) && gc(cp) != G::Unassigned)
}

/// The Decomposition_Type (ICU value) from the compatibility mapping and
/// the character name (intl has no Decomposition_Type).
fn decomposition_type(c: char) -> i64 {
    if has_canonical_decomposition(c) {
        return 1;
    }
    if un::nfkd(std::iter::once(c)).eq(std::iter::once(c)) {
        return 0;
    }
    let cp = c as u32;
    let name = un::name(c).unwrap_or_default();
    let has = |s: &str| name.contains(s);
    if matches!(cp, 0xA0 | 0xF0C | 0x2007 | 0x2011 | 0x202F) {
        11
    } else if un::nfkd(std::iter::once(c)).any(|x| x == '\u{2044}') && gc(cp) == G::OtherNumber {
        6
    } else if matches!(cp, 0x3000 | 0xFF01..=0xFF60 | 0xFFE0..=0xFFE6) {
        17
    } else if matches!(cp, 0xFF61..=0xFFDC | 0xFFE8..=0xFFEE) {
        10
    } else if (0xFE10..=0xFE19).contains(&cp) || (0xFE30..=0xFE48).contains(&cp) || has("VERTICAL") && (0xFE00..=0xFE4F).contains(&cp) {
        16
    } else if (0xFE50..=0xFE6B).contains(&cp) {
        12
    } else if has("ISOLATED FORM") {
        8
    } else if has("FINAL FORM") {
        4
    } else if has("INITIAL FORM") {
        7
    } else if has("MEDIAL FORM") {
        9
    } else if has("SUPERSCRIPT") || has("MODIFIER LETTER SMALL") || has("MODIFIER LETTER CAPITAL") {
        15
    } else if has("SUBSCRIPT") {
        14
    } else if has("CIRCLED") || has("NEGATIVE CIRCLED") {
        3
    } else if has("SQUARE") || (0x3300..=0x33FF).contains(&cp) {
        13
    } else if (0x1D400..=0x1D7FF).contains(&cp)
        || (0x1EE00..=0x1EEFF).contains(&cp)
        || (0xFB20..=0xFB29).contains(&cp)
        || matches!(cp, 0x2102 | 0x210A..=0x2113 | 0x2115 | 0x2119..=0x211D | 0x2124 | 0x2128 | 0x212C | 0x212D | 0x212F..=0x2131 | 0x2133..=0x2134 | 0x2139 | 0x213C..=0x2140 | 0x2145..=0x2149)
    {
        5
    } else {
        2
    }
}

/// u_getIntPropertyValue()
fn int_value(cp: u32, prop: i64) -> i64 {
    if prop < 0x1000 {
        return binary(cp, prop) as i64;
    }
    let c = ch(cp);
    let byname = |p: i64, n: &str| value_by_name(p, n).unwrap_or(0);
    match prop {
        0x1000 => icu_bidi(cp),
        0x1001 => block_code(cp),
        0x1002 => un::canonical_combining_class_u32(cp) as i64,
        0x1003 => decomposition_type(c),
        0x1004 => byname(0x1004, un::east_asian_width_u32(cp).abbr()),
        0x1005 => icu_gc(cp),
        0x1006 => byname(0x1006, &format!("{:?}", un::joining_group(c))),
        0x1007 => {
            let n = match un::joining_type(c) {
                un::JoiningType::NonJoining => "U",
                un::JoiningType::JoinCausing => "C",
                un::JoiningType::DualJoining => "D",
                un::JoiningType::LeftJoining => "L",
                un::JoiningType::RightJoining => "R",
                un::JoiningType::Transparent => "T",
            };
            byname(0x1007, n)
        }
        0x1009 => match un::numeric_type(c) {
            None => 0,
            Some(t) => byname(0x1009, &format!("{:?}", t)),
        },
        0x100A => script_code(cp),
        0x100B => {
            if (0xAC00..=0xD7A3).contains(&cp) {
                if (cp - 0xAC00).is_multiple_of(28) { 4 } else { 5 }
            } else if in_ranges(cp, &[(0x1100, 0x115F), (0xA960, 0xA97C)]) {
                1
            } else if in_ranges(cp, &[(0x1160, 0x11A7), (0xD7B0, 0xD7C6)]) {
                2
            } else if in_ranges(cp, &[(0x11A8, 0x11FF), (0xD7CB, 0xD7FB)]) {
                3
            } else {
                0
            }
        }
        0x100C..=0x100F => {
            let r = match prop {
                0x100C => un::quick_check_nfd(std::iter::once(c)),
                0x100D => un::quick_check_nfkd(std::iter::once(c)),
                0x100E => un::quick_check_nfc(std::iter::once(c)),
                _ => un::quick_check_nfkc(std::iter::once(c)),
            };
            match r {
                un::IsNormalized::No => 0,
                un::IsNormalized::Yes => 1,
                _ => 2,
            }
        }
        0x1010 => un::nfd(std::iter::once(c))
            .next()
            .map_or(0, |x| un::canonical_combining_class(x) as i64),
        0x1011 => un::nfd(std::iter::once(c))
            .last()
            .map_or(0, |x| un::canonical_combining_class(x) as i64),
        0x1015 => {
            if un::bidi_mirror(c).is_some() {
                match un::general_category(c) {
                    G::OpenPunctuation => 1,
                    G::ClosePunctuation => 2,
                    _ => 0,
                }
            } else {
                0
            }
        }
        0x1016 => byname(0x1016, &format!("{:?}", un::indic_positional_category(c))),
        0x1017 => byname(0x1017, &format!("{:?}", un::indic_syllabic_category(c))),
        0x2000 => 1 << icu_gc(cp),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Methods

fn chr(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => utf8(cp),
        None => Value::Null,
    }
}

fn ord(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(cp as i64),
        None => Value::Null,
    }
}

fn has_binary_property(vm: &mut Vm, c: &mut Call) -> Value {
    let prop = c.long(1);
    match cp_arg(vm, c, 0) {
        Some(cp) => crate::bool_v(binary(cp, prop)),
        None => Value::Null,
    }
}

fn get_int_property_value(vm: &mut Vm, c: &mut Call) -> Value {
    let prop = c.long(1);
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(int_value(cp, prop)),
        None => Value::Null,
    }
}

fn get_int_property_min_value(_vm: &mut Vm, _c: &mut Call) -> Value {
    Value::Long(0)
}

fn get_int_property_max_value(_vm: &mut Vm, c: &mut Call) -> Value {
    let p = c.long(0);
    let v = tables::PROPS
        .iter()
        .find(|e| e.0 == p)
        .map_or(-1, |e| e.2);
    Value::Long(v)
}

fn get_numeric_value(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Double(
            un::numeric_value_u32(cp).map_or(NO_NUMERIC_VALUE, |n| n.as_f64()),
        ),
        None => Value::Null,
    }
}

fn enum_char_types(vm: &mut Vm, c: &mut Call) -> Value {
    let cb = c.arg(0);
    let mut start = 0u32;
    let mut cur = icu_gc(0);
    let mut cp = 1u32;
    while cp <= 0x110000 {
        let t = if cp < 0x110000 { icu_gc(cp) } else { -1 };
        if t != cur {
            let args = [Value::Long(start as i64), Value::Long(cp as i64), Value::Long(cur)];
            if vm.call_callback(&cb, &args).is_err() || vm.current_exception.is_some() {
                return Value::Null;
            }
            start = cp;
            cur = t;
        }
        cp += 1;
    }
    Value::Null
}

fn get_block_code(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(block_code(cp)),
        None => Value::Null,
    }
}

/// u_charName()
fn name_of(cp: u32, choice: i64) -> Option<String> {
    match choice {
        0 => Some(char::from_u32(cp).and_then(un::name).unwrap_or_default()),
        1 | 3 => Some(String::new()),
        2 => Some(match char::from_u32(cp).and_then(un::name) {
            Some(n) => n,
            None => {
                let g = gc(cp);
                let label = if uset::is_noncharacter(ch(cp)) && cp <= 0x10FFFF && char::from_u32(cp).is_some() {
                    "noncharacter"
                } else {
                    match g {
                        G::Control => "control",
                        G::PrivateUse => "private-use",
                        G::Surrogate => {
                            if cp < 0xDC00 {
                                "lead-surrogate"
                            } else {
                                "trail-surrogate"
                            }
                        }
                        _ => "unassigned",
                    }
                };
                format!("<{}-{:04X}>", label, cp)
            }
        }),
        _ => None,
    }
}

fn char_name(vm: &mut Vm, c: &mut Call) -> Value {
    let choice = c.long(1);
    let Some(cp) = cp_arg(vm, c, 0) else {
        return Value::Null;
    };
    match name_of(cp, choice) {
        Some(n) => s(n),
        None => {
            error::set_global(
                vm,
                &c.name,
                U_ILLEGAL_ARGUMENT_ERROR,
                "Failure getting character name",
            );
            Value::Null
        }
    }
}

fn char_from_name(_vm: &mut Vm, c: &mut Call) -> Value {
    let name = String::from_utf8_lossy(&c.str(0)).into_owned();
    let choice = c.long(1);
    error::reset_global();
    let up = name.to_ascii_uppercase();
    let r = match choice {
        0 => uset::char_from_name(&up),
        2 => uset::char_from_name(&up).or_else(|| {
            let inner = up.strip_prefix('<')?.strip_suffix('>')?;
            let (_, hex) = inner.rsplit_once('-')?;
            let cp = u32::from_str_radix(hex, 16).ok()?;
            (name_of(cp, 2)?.to_ascii_uppercase() == up).then_some(cp)
        }),
        _ => None,
    };
    match r {
        Some(cp) => Value::Long(cp as i64),
        None => {
            error::set_global_code(error::U_ILLEGAL_CHAR_FOUND);
            Value::Null
        }
    }
}

fn enum_char_names(vm: &mut Vm, c: &mut Call) -> Value {
    let cb = c.arg(2);
    let choice = c.long(3);
    let Some(start) = cp_arg(vm, c, 0) else {
        return Value::False;
    };
    let Some(limit) = cp_arg(vm, c, 1) else {
        return Value::False;
    };
    if !(0..=3).contains(&choice) {
        error::set_global_code(U_ILLEGAL_ARGUMENT_ERROR);
        return Value::False;
    }
    for cp in start..limit {
        let Some(n) = name_of(cp, choice) else { break };
        if n.is_empty() {
            continue;
        }
        let args = [Value::Long(cp as i64), Value::Long(choice), s(n)];
        if vm.call_callback(&cb, &args).is_err() || vm.current_exception.is_some() {
            break;
        }
    }
    Value::True
}

fn get_property_name(vm: &mut Vm, c: &mut Call) -> Value {
    let p = c.long(0);
    let choice = c.long(1);
    let n = tables::PROPS
        .iter()
        .find(|e| e.0 == p)
        .and_then(|e| usize::try_from(choice).ok().and_then(|i| e.1.get(i)))
        .filter(|n| !n.is_empty());
    match n {
        Some(n) => s(*n),
        None => {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "Failed to get property name");
            Value::False
        }
    }
}

fn get_property_enum(_vm: &mut Vm, c: &mut Call) -> Value {
    let a = loose(&String::from_utf8_lossy(&c.str(0)));
    let v = tables::PROPS
        .iter()
        .find(|e| e.1.iter().any(|n| loose(n) == a))
        .map_or(-1, |e| e.0);
    Value::Long(v)
}

fn get_property_value_name(vm: &mut Vm, c: &mut Call) -> Value {
    let p = c.long(0);
    let v = c.long(1);
    let choice = c.long(2);
    let n = tables::VALUES
        .iter()
        .find(|e| e.0 == p && e.1 == v)
        .and_then(|e| usize::try_from(choice).ok().and_then(|i| e.2.get(i)))
        .filter(|n| !n.is_empty());
    match n {
        Some(n) => s(*n),
        None => {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "Failed to get property name");
            Value::False
        }
    }
}

fn get_property_value_enum(_vm: &mut Vm, c: &mut Call) -> Value {
    let p = c.long(0);
    let n = String::from_utf8_lossy(&c.str(1)).into_owned();
    Value::Long(value_by_name(p, &n).unwrap_or(-1))
}

/// Simple (one code point) case mappings.
fn simple(cp: u32, it: impl Iterator<Item = char>) -> u32 {
    let v: Vec<char> = it.collect();
    if v.len() == 1 { v[0] as u32 } else { cp }
}

fn fold_case(vm: &mut Vm, c: &mut Call) -> Value {
    let opt = c.long(1);
    let Some(cp) = cp_arg(vm, c, 0) else {
        return Value::Null;
    };
    let r = if opt & 1 != 0 && cp == 0x49 {
        0x131
    } else if opt & 1 != 0 && cp == 0x130 {
        0x69
    } else {
        match char::from_u32(cp) {
            Some(x) => simple(cp, un::case_fold(x)),
            None => cp,
        }
    };
    cp_result(c, r)
}

/// u_charDigitValue()
fn digit_value(cp: u32) -> i64 {
    if gc(cp) == G::DecimalNumber {
        un::numeric_value_u32(cp)
            .and_then(|n| n.to_i64())
            .unwrap_or(-1)
    } else {
        -1
    }
}

fn digit(vm: &mut Vm, c: &mut Call) -> Value {
    let radix = c.long(1);
    let Some(cp) = cp_arg(vm, c, 0) else {
        return Value::Null;
    };
    let mut v = digit_value(cp);
    if v < 0 {
        v = match cp {
            0x41..=0x5A => cp as i64 - 0x41 + 10,
            0x61..=0x7A => cp as i64 - 0x61 + 10,
            0xFF21..=0xFF3A => cp as i64 - 0xFF21 + 10,
            0xFF41..=0xFF5A => cp as i64 - 0xFF41 + 10,
            _ => -1,
        };
    }
    if v < 0 || !(2..=36).contains(&radix) || v >= radix {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "Invalid digit");
        return Value::False;
    }
    Value::Long(v)
}

fn for_digit(_vm: &mut Vm, c: &mut Call) -> Value {
    let d = c.long(0);
    let radix = c.long(1);
    if !(2..=36).contains(&radix) || d < 0 || d >= radix {
        return Value::Long(0);
    }
    Value::Long(if d < 10 { 0x30 + d } else { 0x61 + d - 10 })
}

fn version_array(v: [i64; 4]) -> Value {
    let mut a = PhpArray::new();
    for x in v {
        a.push(Value::Long(x));
    }
    crate::arr(a)
}

fn char_age(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(cp) = cp_arg(vm, c, 0) else {
        return Value::Null;
    };
    let (ma, mi) = char::from_u32(cp)
        .and_then(un::age)
        .map_or((0, 0), |(a, b)| (a as i64, b as i64));
    version_array([ma, mi, 0, 0])
}

fn get_unicode_version(_vm: &mut Vm, _c: &mut Call) -> Value {
    let (a, b, d) = un::UNICODE_VERSION;
    version_array([a as i64, b as i64, d as i64, 0])
}

fn get_fc_nfkc_closure(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(cp) = cp_arg(vm, c, 0) else {
        return Value::Null;
    };
    let Some(x) = char::from_u32(cp) else {
        return s("");
    };
    let fold: String = un::fold(std::iter::once(x)).collect();
    let nk: String = un::nfkc(std::iter::once(x)).collect();
    let b: String = un::nfkc(un::fold(nk.chars())).collect();
    let a: String = un::nfkc(fold.chars()).collect();
    if a == b && b == fold {
        return s("");
    }
    if b == fold { s("") } else { s(b) }
}

fn char_direction(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(icu_bidi(cp)),
        None => Value::Null,
    }
}

fn char_type(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(icu_gc(cp)),
        None => Value::Null,
    }
}

fn get_combining_class(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(un::canonical_combining_class_u32(cp) as i64),
        None => Value::Null,
    }
}

fn char_digit_value(vm: &mut Vm, c: &mut Call) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => Value::Long(digit_value(cp)),
        None => Value::Null,
    }
}

fn map_cp(vm: &mut Vm, c: &mut Call, f: fn(u32) -> u32) -> Value {
    match cp_arg(vm, c, 0) {
        Some(cp) => cp_result(c, f(cp)),
        None => Value::Null,
    }
}

fn char_mirror(vm: &mut Vm, c: &mut Call) -> Value {
    map_cp(vm, c, |cp| {
        char::from_u32(cp)
            .and_then(un::bidi_mirror)
            .map_or(cp, |m| m as u32)
    })
}

fn get_bidi_paired_bracket(vm: &mut Vm, c: &mut Call) -> Value {
    map_cp(vm, c, |cp| {
        if int_value(cp, 0x1015) != 0 {
            char::from_u32(cp)
                .and_then(un::bidi_mirror)
                .map_or(cp, |m| m as u32)
        } else {
            cp
        }
    })
}

fn tolower(vm: &mut Vm, c: &mut Call) -> Value {
    map_cp(vm, c, |cp| {
        if cp == 0x130 {
            return 0x69;
        }
        char::from_u32(cp).map_or(cp, |x| simple(cp, un::to_lowercase(x)))
    })
}

fn toupper(vm: &mut Vm, c: &mut Call) -> Value {
    map_cp(vm, c, |cp| {
        char::from_u32(cp).map_or(cp, |x| simple(cp, un::to_uppercase(x)))
    })
}

fn totitle(vm: &mut Vm, c: &mut Call) -> Value {
    map_cp(vm, c, |cp| {
        char::from_u32(cp).map_or(cp, |x| simple(cp, un::to_titlecase(x)))
    })
}
