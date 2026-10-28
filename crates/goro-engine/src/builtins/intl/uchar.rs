//! `IntlListFormatter` (PHP 8.5; CLDR list patterns, the intl crate) and
//! `IntlChar` (PHP manual: IntlChar; Unicode character properties from
//! the intl crate's UCD 17 tables).

use goro_ext_intl::intl::list::{ListFormatOptions, ListType, ListWidth};
use goro_ext_intl::intl::unicode as u;
use goro_runtime::Value;

use super::{store, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    classes: &[
        ("intllistformatter", LIST_CLASS),
        ("intlchar", CHAR_CLASS),
    ],
    methods: &[
        ("intllistformatter::__construct", l_construct),
        ("intllistformatter::format", l_format),
        ("intlchar::charname", c_char_name),
        ("intlchar::charfromname", c_char_from_name),
        ("intlchar::ord", c_ord),
        ("intlchar::chr", c_chr),
        ("intlchar::isalpha", c_isalpha),
        ("intlchar::isdigit", c_isdigit),
        ("intlchar::isupper", c_isupper),
        ("intlchar::islower", c_islower),
        ("intlchar::isspace", c_isspace),
        ("intlchar::isalnum", c_isalnum),
        ("intlchar::ispunct", c_ispunct),
        ("intlchar::iscntrl", c_iscntrl),
        ("intlchar::istitle", c_istitle),
        ("intlchar::isdefined", c_isdefined),
        ("intlchar::iswhitespace", c_iswhitespace),
        ("intlchar::isuwhitespace", c_isuwhitespace),
        ("intlchar::isualphabetic", c_isualphabetic),
        ("intlchar::isuuppercase", c_isupper),
        ("intlchar::isulowercase", c_islower),
        ("intlchar::isxdigit", c_isxdigit),
        ("intlchar::ismirrored", c_ismirrored),
        ("intlchar::toupper", c_toupper),
        ("intlchar::tolower", c_tolower),
        ("intlchar::totitle", c_totitle),
        ("intlchar::foldcase", c_foldcase),
        ("intlchar::chartype", c_chartype),
        ("intlchar::charmirror", c_charmirror),
        ("intlchar::digit", c_digit),
        ("intlchar::fordigit", c_fordigit),
        ("intlchar::getnumericvalue", c_numeric),
        ("intlchar::getcombiningclass", c_ccc),
        ("intlchar::getunicodeversion", c_unicode_version),
        ("intlchar::charage", c_age),
    ],
    hooks: &[("intllistformatter", list_hooks)],
    ..Module::EMPTY
};

const LIST_CLASS: &str = r#"<?php final class IntlListFormatter {
    public const int TYPE_AND = 0;
    public const int TYPE_OR = 1;
    public const int TYPE_UNITS = 2;
    public const int WIDTH_WIDE = 0;
    public const int WIDTH_SHORT = 1;
    public const int WIDTH_NARROW = 2;
    public function __construct(string $locale, int $type = IntlListFormatter::TYPE_AND, int $width = IntlListFormatter::WIDTH_WIDE) {}
    public function format(array $strings): string|false {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): string {}
}"#;

const CHAR_CLASS: &str = r#"<?php class IntlChar {
    public const string UNICODE_VERSION = "17.0";
    public const int CODEPOINT_MIN = 0;
    public const int CODEPOINT_MAX = 1114111;
    public const float NO_NUMERIC_VALUE = -123456789.0;
    public const int UNICODE_CHAR_NAME = 0;
    public const int UNICODE_10_CHAR_NAME = 1;
    public const int EXTENDED_CHAR_NAME = 2;
    public const int CHAR_NAME_ALIAS = 3;
    public const int CHAR_NAME_CHOICE_COUNT = 4;
    public const int CHAR_CATEGORY_UNASSIGNED = 0;
    public const int CHAR_CATEGORY_GENERAL_OTHER_TYPES = 0;
    public const int CHAR_CATEGORY_UPPERCASE_LETTER = 1;
    public const int CHAR_CATEGORY_LOWERCASE_LETTER = 2;
    public const int CHAR_CATEGORY_TITLECASE_LETTER = 3;
    public const int CHAR_CATEGORY_MODIFIER_LETTER = 4;
    public const int CHAR_CATEGORY_OTHER_LETTER = 5;
    public const int CHAR_CATEGORY_NON_SPACING_MARK = 6;
    public const int CHAR_CATEGORY_ENCLOSING_MARK = 7;
    public const int CHAR_CATEGORY_COMBINING_SPACING_MARK = 8;
    public const int CHAR_CATEGORY_DECIMAL_DIGIT_NUMBER = 9;
    public const int CHAR_CATEGORY_LETTER_NUMBER = 10;
    public const int CHAR_CATEGORY_OTHER_NUMBER = 11;
    public const int CHAR_CATEGORY_SPACE_SEPARATOR = 12;
    public const int CHAR_CATEGORY_LINE_SEPARATOR = 13;
    public const int CHAR_CATEGORY_PARAGRAPH_SEPARATOR = 14;
    public const int CHAR_CATEGORY_CONTROL_CHAR = 15;
    public const int CHAR_CATEGORY_FORMAT_CHAR = 16;
    public const int CHAR_CATEGORY_PRIVATE_USE_CHAR = 17;
    public const int CHAR_CATEGORY_SURROGATE = 18;
    public const int CHAR_CATEGORY_DASH_PUNCTUATION = 19;
    public const int CHAR_CATEGORY_START_PUNCTUATION = 20;
    public const int CHAR_CATEGORY_END_PUNCTUATION = 21;
    public const int CHAR_CATEGORY_CONNECTOR_PUNCTUATION = 22;
    public const int CHAR_CATEGORY_OTHER_PUNCTUATION = 23;
    public const int CHAR_CATEGORY_MATH_SYMBOL = 24;
    public const int CHAR_CATEGORY_CURRENCY_SYMBOL = 25;
    public const int CHAR_CATEGORY_MODIFIER_SYMBOL = 26;
    public const int CHAR_CATEGORY_OTHER_SYMBOL = 27;
    public const int CHAR_CATEGORY_INITIAL_PUNCTUATION = 28;
    public const int CHAR_CATEGORY_FINAL_PUNCTUATION = 29;
    public const int CHAR_CATEGORY_CHAR_CATEGORY_COUNT = 30;
    public static function charName(int|string $codepoint, int $type = IntlChar::UNICODE_CHAR_NAME): ?string {}
    public static function charFromName(string $name, int $type = IntlChar::UNICODE_CHAR_NAME): ?int {}
    public static function ord(int|string $character): ?int {}
    public static function chr(int|string $codepoint): ?string {}
    public static function isalpha(int|string $codepoint): ?bool {}
    public static function isdigit(int|string $codepoint): ?bool {}
    public static function isupper(int|string $codepoint): ?bool {}
    public static function islower(int|string $codepoint): ?bool {}
    public static function isspace(int|string $codepoint): ?bool {}
    public static function isalnum(int|string $codepoint): ?bool {}
    public static function ispunct(int|string $codepoint): ?bool {}
    public static function iscntrl(int|string $codepoint): ?bool {}
    public static function istitle(int|string $codepoint): ?bool {}
    public static function isdefined(int|string $codepoint): ?bool {}
    public static function isWhitespace(int|string $codepoint): ?bool {}
    public static function isUWhiteSpace(int|string $codepoint): ?bool {}
    public static function isUAlphabetic(int|string $codepoint): ?bool {}
    public static function isUUppercase(int|string $codepoint): ?bool {}
    public static function isULowercase(int|string $codepoint): ?bool {}
    public static function isxdigit(int|string $codepoint): ?bool {}
    public static function isMirrored(int|string $codepoint): ?bool {}
    public static function toupper(int|string $codepoint): int|string|null {}
    public static function tolower(int|string $codepoint): int|string|null {}
    public static function totitle(int|string $codepoint): int|string|null {}
    public static function foldCase(int|string $codepoint, int $options = 0): int|string|null {}
    public static function charType(int|string $codepoint): ?int {}
    public static function charMirror(int|string $codepoint): int|string|null {}
    public static function digit(int|string $codepoint, int $base = 10): int|false|null {}
    public static function forDigit(int $digit, int $base = 10): int {}
    public static function getNumericValue(int|string $codepoint): ?float {}
    public static function getCombiningClass(int|string $codepoint): ?int {}
    public static function getUnicodeVersion(): array {}
    public static function charAge(int|string $codepoint): ?array {}
}"#;

// ---------------------------------------------------------------- list

#[derive(Clone)]
struct ListFmt {
    locale: String,
    opts: ListFormatOptions,
}

fn list_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|vm, _, _| {
            super::timezone::clone_error(
                vm,
                "Trying to clone an uncloneable object of class IntlListFormatter",
            );
        }),
        ..Default::default()
    }
}

fn l_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let l = a.str(0).to_string_lossy();
    if l.len() > super::locale::MAX_LOCALE_LEN {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($locale) must be less than or equal to 156 characters",
        );
    }
    let lang = goro_ext_intl::locale::parse(&l).language;
    if !goro_ext_intl::locale::is_known_language(&lang) {
        return vm.fthrow(a, "ValueError", &format!("Argument #1 ($locale) \"{l}\" is invalid"));
    }
    let ty = match a.int_or(1, 0) {
        0 => ListType::Conjunction,
        1 => ListType::Disjunction,
        2 => ListType::Unit,
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($type) must be one of IntlListFormatter::TYPE_AND, IntlListFormatter::TYPE_OR, or IntlListFormatter::TYPE_UNITS",
            );
        }
    };
    let width = match a.int_or(2, 0) {
        0 => ListWidth::Long,
        1 => ListWidth::Short,
        2 => ListWidth::Narrow,
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #3 ($width) must be one of IntlListFormatter::WIDTH_WIDE, IntlListFormatter::WIDTH_SHORT, or IntlListFormatter::WIDTH_NARROW",
            );
        }
    };
    let mut opts = ListFormatOptions::default();
    opts.list_type = ty;
    opts.width = width;
    store(
        a.this_obj(),
        ListFmt {
            locale: goro_ext_intl::locale::to_bcp47(&l),
            opts,
        },
    );
    Ok(Value::Null)
}

fn l_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(f) = with_obj::<ListFmt, ListFmt>(a.this_obj(), |x| x.v.clone()) else {
        return vm.throw("Error", "IntlListFormatter object is not initialized");
    };
    let arr = a.array(0);
    let mut items = Vec::new();
    for (_, v) in arr.iter() {
        let s = vm.to_php_str(&v.deref())?;
        items.push(s.to_string_lossy());
    }
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let r = goro_ext_intl::intl::list::format_list(&f.locale, &refs, &f.opts);
    Ok(Value::from(r.as_str()))
}

// ---------------------------------------------------------------- IntlChar

/// A code point argument: an int, or a string of one UTF-8 character.
fn cp_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<char>> {
    match a.val(i) {
        Value::Int(n) => Ok(u32::try_from(n).ok().and_then(char::from_u32)),
        Value::Str(s) => {
            let t = String::from_utf8_lossy(s.as_bytes()).into_owned();
            let mut it = t.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Ok(Some(c)),
                _ => {
                    super::fail(
                        vm,
                        a,
                        goro_ext_intl::error::U_ILLEGAL_ARGUMENT_ERROR,
                        "Passing a UTF-8 character for codepoint requires a string which is exactly one UTF-8 codepoint long.",
                    )?;
                    Ok(None)
                }
            }
        }
        _ => Ok(None),
    }
}

/// Returns a code point in the argument's form (an int stays an int, a
/// string a string).
fn same_form(a: &Args, c: char) -> Value {
    match a.val(0) {
        Value::Str(_) => Value::from(c.to_string().as_str()),
        _ => Value::Int(c as i64),
    }
}

macro_rules! pred {
    ($($name:ident => $f:expr),* $(,)?) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            Ok(match cp_arg(vm, a, 0)? {
                Some(c) => Value::Bool(($f)(c)),
                None => Value::Null,
            })
        }
    )*};
}

pred!(
    c_isalpha => |c: char| u::is_letter(c),
    c_isdigit => |c: char| u::general_category(c) == u::GeneralCategory::DecimalNumber,
    c_isupper => |c: char| u::general_category(c) == u::GeneralCategory::UppercaseLetter,
    c_islower => |c: char| u::general_category(c) == u::GeneralCategory::LowercaseLetter,
    c_isspace => |c: char| u::is_separator(c) || matches!(c, '\t'..='\r' | '\u{1c}'..='\u{1f}'),
    c_isalnum => |c: char| u::is_letter(c) || u::general_category(c) == u::GeneralCategory::DecimalNumber,
    c_ispunct => |c: char| u::is_punctuation(c),
    c_iscntrl => |c: char| u::is_control(c) || matches!(u::general_category(c), u::GeneralCategory::LineSeparator | u::GeneralCategory::ParagraphSeparator),
    c_istitle => |c: char| u::general_category(c) == u::GeneralCategory::TitlecaseLetter,
    c_isdefined => |c: char| u::is_assigned(c),
    c_iswhitespace => |c: char| (u::is_separator(c) && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}')) || matches!(c, '\t'..='\r' | '\u{1c}'..='\u{1f}'),
    c_isuwhitespace => |c: char| u::is_whitespace(c),
    c_isualphabetic => |c: char| u::is_alphabetic(c),
    c_isxdigit => |c: char| u::general_category(c) == u::GeneralCategory::DecimalNumber || c.is_ascii_hexdigit() || matches!(c, '\u{ff21}'..='\u{ff26}' | '\u{ff41}'..='\u{ff46}'),
    c_ismirrored => |c: char| u::is_bidi_mirrored(c),
);

fn c_char_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ty = a.int_or(1, 0);
    let Some(c) = cp_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    match ty {
        0 | 2 => Ok(Value::from(u::name(c).unwrap_or_default().as_str())),
        1 | 3 => Ok(Value::from("")),
        _ => Ok(Value::Null),
    }
}

fn c_char_from_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static NAMES: OnceLock<HashMap<String, u32>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let mut m = HashMap::new();
        for cp in 0..0x110000u32 {
            if let Some(c) = char::from_u32(cp)
                && let Some(n) = u::name(c)
            {
                m.insert(n, cp);
            }
        }
        m
    });
    let n = a.str(0).to_string_lossy().to_ascii_uppercase();
    Ok(match names.get(&n) {
        Some(cp) => Value::Int(*cp as i64),
        None => Value::Null,
    })
}

fn c_ord(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| Value::Int(c as i64)))
}

fn c_chr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| Value::from(c.to_string().as_str())))
}

fn single(it: impl Iterator<Item = char>, c: char) -> char {
    let v: Vec<char> = it.collect();
    if v.len() == 1 { v[0] } else { c }
}

fn c_toupper(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| same_form(a, single(u::to_uppercase(c), c))))
}

fn c_tolower(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| same_form(a, single(u::to_lowercase(c), c))))
}

fn c_totitle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| same_form(a, single(u::to_titlecase(c), c))))
}

fn c_foldcase(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| same_form(a, single(u::case_fold(c), c))))
}

fn c_charmirror(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| same_form(a, u::bidi_mirror(c).unwrap_or(c))))
}

/// ICU's UCharCategory numbering.
fn category_number(g: u::GeneralCategory) -> i64 {
    use u::GeneralCategory as G;
    match g {
        G::Unassigned => 0,
        G::UppercaseLetter => 1,
        G::LowercaseLetter => 2,
        G::TitlecaseLetter => 3,
        G::ModifierLetter => 4,
        G::OtherLetter => 5,
        G::NonspacingMark => 6,
        G::EnclosingMark => 7,
        G::SpacingMark => 8,
        G::DecimalNumber => 9,
        G::LetterNumber => 10,
        G::OtherNumber => 11,
        G::SpaceSeparator => 12,
        G::LineSeparator => 13,
        G::ParagraphSeparator => 14,
        G::Control => 15,
        G::Format => 16,
        G::PrivateUse => 17,
        G::Surrogate => 18,
        G::DashPunctuation => 19,
        G::OpenPunctuation => 20,
        G::ClosePunctuation => 21,
        G::ConnectorPunctuation => 22,
        G::OtherPunctuation => 23,
        G::MathSymbol => 24,
        G::CurrencySymbol => 25,
        G::ModifierSymbol => 26,
        G::OtherSymbol => 27,
        G::InitialPunctuation => 28,
        G::FinalPunctuation => 29,
    }
}

fn c_chartype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| Value::Int(category_number(u::general_category(c)))))
}

fn c_digit(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let base = a.int_or(1, 10);
    let Some(c) = cp_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    let d = if u::general_category(c) == u::GeneralCategory::DecimalNumber {
        u::numeric_value(c).and_then(|n| n.to_i64())
    } else {
        c.to_digit(36).map(|d| d as i64)
    };
    Ok(match d {
        Some(d) if d < base => Value::Int(d),
        _ => Value::Bool(false),
    })
}

fn c_fordigit(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.int(0);
    let base = a.int_or(1, 10);
    Ok(Value::Int(if (2..=36).contains(&base) && (0..base).contains(&d) {
        std::char::from_digit(d as u32, 36).map_or(0, |c| c as i64)
    } else {
        0
    }))
}

fn c_numeric(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| {
        Value::Float(u::numeric_value(c).map_or(-123456789.0, |n| n.as_f64()))
    }))
}

fn c_ccc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| Value::Int(u::canonical_combining_class(c) as i64)))
}

fn c_unicode_version(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(goro_runtime::PhpArray::from_values(
        [17, 0, 0, 0].into_iter().map(Value::Int),
    )))
}

fn c_age(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(cp_arg(vm, a, 0)?.map_or(Value::Null, |c| {
        let (x, y) = u::age(c).unwrap_or((0, 0));
        Value::Array(goro_runtime::PhpArray::from_values(
            [x as i64, y as i64, 0, 0].into_iter().map(Value::Int),
        ))
    }))
}
