//! `NumberFormatter` and the `numfmt_*()` functions (PHP manual:
//! NumberFormatter).

use goro_ext_intl::error as ue;
use goro_ext_intl::numfmt::{self as nf, NumberFormat};
use goro_runtime::{ObjRef, Value};

use super::locale::MAX_LOCALE_LEN;
use super::{
    already_constructed, fail, fail_obj, has_obj, new_obj, obj_err, reset, reset_obj, store,
    this_or_first, with_obj,
};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function numfmt_create(string $locale, int $style, ?string $pattern = null): ?NumberFormatter",
            n_create,
        ),
        (
            "function numfmt_format(NumberFormatter $formatter, int|float $num, int $type = NumberFormatter::TYPE_DEFAULT): string|false",
            n_format,
        ),
        (
            "function numfmt_parse(NumberFormatter $formatter, string $string, int $type = NumberFormatter::TYPE_DOUBLE, &$offset = null): int|float|false",
            n_parse,
        ),
        (
            "function numfmt_format_currency(NumberFormatter $formatter, float $amount, string $currency): string|false",
            n_format_currency,
        ),
        (
            "function numfmt_parse_currency(NumberFormatter $formatter, string $string, &$currency, &$offset = null): float|false",
            n_parse_currency,
        ),
        (
            "function numfmt_set_attribute(NumberFormatter $formatter, int $attribute, int|float $value): bool",
            n_set_attribute,
        ),
        (
            "function numfmt_get_attribute(NumberFormatter $formatter, int $attribute): int|float|false",
            n_get_attribute,
        ),
        (
            "function numfmt_set_text_attribute(NumberFormatter $formatter, int $attribute, string $value): bool",
            n_set_text_attribute,
        ),
        (
            "function numfmt_get_text_attribute(NumberFormatter $formatter, int $attribute): string|false",
            n_get_text_attribute,
        ),
        (
            "function numfmt_set_symbol(NumberFormatter $formatter, int $symbol, string $value): bool",
            n_set_symbol,
        ),
        (
            "function numfmt_get_symbol(NumberFormatter $formatter, int $symbol): string|false",
            n_get_symbol,
        ),
        (
            "function numfmt_set_pattern(NumberFormatter $formatter, string $pattern): bool",
            n_set_pattern,
        ),
        (
            "function numfmt_get_pattern(NumberFormatter $formatter): string|false",
            n_get_pattern,
        ),
        (
            "function numfmt_get_locale(NumberFormatter $formatter, int $type = ULOC_ACTUAL_LOCALE): string|false",
            n_get_locale,
        ),
        (
            "function numfmt_get_error_code(NumberFormatter $formatter): int",
            n_get_error_code,
        ),
        (
            "function numfmt_get_error_message(NumberFormatter $formatter): string",
            n_get_error_message,
        ),
    ],
    classes: &[("numberformatter", NF_CLASS)],
    methods: &[
        ("numberformatter::__construct", n_construct),
        ("numberformatter::create", n_create),
        ("numberformatter::format", n_format),
        ("numberformatter::parse", n_parse),
        ("numberformatter::formatcurrency", n_format_currency),
        ("numberformatter::parsecurrency", n_parse_currency),
        ("numberformatter::setattribute", n_set_attribute),
        ("numberformatter::getattribute", n_get_attribute),
        ("numberformatter::settextattribute", n_set_text_attribute),
        ("numberformatter::gettextattribute", n_get_text_attribute),
        ("numberformatter::setsymbol", n_set_symbol),
        ("numberformatter::getsymbol", n_get_symbol),
        ("numberformatter::setpattern", n_set_pattern),
        ("numberformatter::getpattern", n_get_pattern),
        ("numberformatter::getlocale", n_get_locale),
        ("numberformatter::geterrorcode", n_get_error_code),
        ("numberformatter::geterrormessage", n_get_error_message),
    ],
    hooks: &[("numberformatter", hooks)],
    constants: &[
        ("ULOC_ACTUAL_LOCALE", crate::registry::ConstVal::Int(0)),
        ("ULOC_VALID_LOCALE", crate::registry::ConstVal::Int(1)),
    ],
    ..Module::EMPTY
};

const NF_CLASS: &str = r#"<?php class NumberFormatter {
    public const int PATTERN_DECIMAL = 0;
    public const int DECIMAL = 1;
    public const int CURRENCY = 2;
    public const int PERCENT = 3;
    public const int SCIENTIFIC = 4;
    public const int SPELLOUT = 5;
    public const int ORDINAL = 6;
    public const int DURATION = 7;
    public const int PATTERN_RULEBASED = 9;
    public const int IGNORE = 0;
    public const int CURRENCY_ACCOUNTING = 12;
    public const int DEFAULT_STYLE = 1;
    public const int ROUND_CEILING = 0;
    public const int ROUND_FLOOR = 1;
    public const int ROUND_DOWN = 2;
    public const int ROUND_UP = 3;
    public const int ROUND_TOWARD_ZERO = 2;
    public const int ROUND_AWAY_FROM_ZERO = 3;
    public const int ROUND_HALFEVEN = 4;
    public const int ROUND_HALFODD = 8;
    public const int ROUND_HALFDOWN = 5;
    public const int ROUND_HALFUP = 6;
    public const int PAD_BEFORE_PREFIX = 0;
    public const int PAD_AFTER_PREFIX = 1;
    public const int PAD_BEFORE_SUFFIX = 2;
    public const int PAD_AFTER_SUFFIX = 3;
    public const int PARSE_INT_ONLY = 0;
    public const int GROUPING_USED = 1;
    public const int DECIMAL_ALWAYS_SHOWN = 2;
    public const int MAX_INTEGER_DIGITS = 3;
    public const int MIN_INTEGER_DIGITS = 4;
    public const int INTEGER_DIGITS = 5;
    public const int MAX_FRACTION_DIGITS = 6;
    public const int MIN_FRACTION_DIGITS = 7;
    public const int FRACTION_DIGITS = 8;
    public const int MULTIPLIER = 9;
    public const int GROUPING_SIZE = 10;
    public const int ROUNDING_MODE = 11;
    public const int ROUNDING_INCREMENT = 12;
    public const int FORMAT_WIDTH = 13;
    public const int PADDING_POSITION = 14;
    public const int SECONDARY_GROUPING_SIZE = 15;
    public const int SIGNIFICANT_DIGITS_USED = 16;
    public const int MIN_SIGNIFICANT_DIGITS = 17;
    public const int MAX_SIGNIFICANT_DIGITS = 18;
    public const int LENIENT_PARSE = 19;
    public const int POSITIVE_PREFIX = 0;
    public const int POSITIVE_SUFFIX = 1;
    public const int NEGATIVE_PREFIX = 2;
    public const int NEGATIVE_SUFFIX = 3;
    public const int PADDING_CHARACTER = 4;
    public const int CURRENCY_CODE = 5;
    public const int DEFAULT_RULESET = 6;
    public const int PUBLIC_RULESETS = 7;
    public const int DECIMAL_SEPARATOR_SYMBOL = 0;
    public const int GROUPING_SEPARATOR_SYMBOL = 1;
    public const int PATTERN_SEPARATOR_SYMBOL = 2;
    public const int PERCENT_SYMBOL = 3;
    public const int ZERO_DIGIT_SYMBOL = 4;
    public const int DIGIT_SYMBOL = 5;
    public const int MINUS_SIGN_SYMBOL = 6;
    public const int PLUS_SIGN_SYMBOL = 7;
    public const int CURRENCY_SYMBOL = 8;
    public const int INTL_CURRENCY_SYMBOL = 9;
    public const int MONETARY_SEPARATOR_SYMBOL = 10;
    public const int EXPONENTIAL_SYMBOL = 11;
    public const int PERMILL_SYMBOL = 12;
    public const int PAD_ESCAPE_SYMBOL = 13;
    public const int INFINITY_SYMBOL = 14;
    public const int NAN_SYMBOL = 15;
    public const int SIGNIFICANT_DIGIT_SYMBOL = 16;
    public const int MONETARY_GROUPING_SEPARATOR_SYMBOL = 17;
    public const int TYPE_DEFAULT = 0;
    public const int TYPE_INT32 = 1;
    public const int TYPE_INT64 = 2;
    public const int TYPE_DOUBLE = 3;
    #[\Deprecated(since: '8.3')]
    public const int TYPE_CURRENCY = 4;
    public const int CURRENCY_ISO = 10;
    public const int CURRENCY_PLURAL = 11;
    public const int CASH_CURRENCY = 13;
    public const int CURRENCY_STANDARD = 16;
    public const int DECIMAL_COMPACT_SHORT = 14;
    public const int DECIMAL_COMPACT_LONG = 15;
    public function __construct(string $locale, int $style, ?string $pattern = null) {}
    public static function create(string $locale, int $style, ?string $pattern = null): ?NumberFormatter {}
    public function format(int|float $num, int $type = NumberFormatter::TYPE_DEFAULT): string|false {}
    public function parse(string $string, int $type = NumberFormatter::TYPE_DOUBLE, &$offset = null): int|float|false {}
    public function formatCurrency(float $amount, string $currency): string|false {}
    public function parseCurrency(string $string, &$currency, &$offset = null): float|false {}
    public function setAttribute(int $attribute, int|float $value): bool {}
    public function getAttribute(int $attribute): int|float|false {}
    public function setTextAttribute(int $attribute, string $value): bool {}
    public function getTextAttribute(int $attribute): string|false {}
    public function setSymbol(int $symbol, string $value): bool {}
    public function getSymbol(int $symbol): string|false {}
    public function setPattern(string $pattern): bool {}
    public function getPattern(): string|false {}
    public function getLocale(int $type = ULOC_ACTUAL_LOCALE): string|false {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): string {}
}"#;

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(on_create),
        clone: Some(|vm, from, to| {
            match with_obj::<NumberFormat, NumberFormat>(from, |x| x.v.clone()) {
                Some(f) => store(to, f),
                None => {
                    super::timezone::clone_error(vm, "Cannot clone uninitialized NumberFormatter")
                }
            }
        }),
        ..Default::default()
    }
}

/// Marks the class not serializable (PHP 8.1: intl objects can't be
/// serialized; ext/intl/tests/bug74063.phpt).
pub(crate) fn not_serializable(vm: &mut Vm, o: &ObjRef) {
    let cls = vm.class_of(o);
    cls.flags
        .set(cls.flags.get() | crate::class::cflags::NOT_SERIALIZABLE);
}

/// Object creation: not serializable, and the global error is reset
/// (ext/intl/tests/msgfmt_fail2.phpt: a TypeError of the constructor
/// leaves U_ZERO_ERROR).
pub(crate) fn on_create(vm: &mut Vm, o: &ObjRef) {
    not_serializable(vm, o);
    reset(vm);
}

/// Builds a formatter, or the error code and message.
fn make(vm: &mut Vm, a: &Args) -> R<Result<NumberFormat, (i64, String)>> {
    let l = a.str(0).to_string_lossy();
    if l.len() > MAX_LOCALE_LEN {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($locale) must be less than or equal to 156 characters",
        );
    }
    let style = a.int(1);
    let pattern = a.opt_str(2).map(|s| s.to_string_lossy());
    let l = if l.is_empty() {
        super::default_locale(vm)
    } else {
        l
    };
    let lang = goro_ext_intl::locale::parse(&l).language;
    if !lang.is_empty() && !goro_ext_intl::locale::is_known_language(&lang) {
        let shown = a.str(0).to_string_lossy();
        let shown = if shown.is_empty() { l.clone() } else { shown };
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 ($locale) \"{shown}\" is invalid"),
        );
    }
    match NumberFormat::new(&l, style, pattern.as_deref()) {
        Ok(f) => Ok(Ok(f)),
        Err(nf::CreateError::Pattern) => Ok(Err((
            ue::U_PATTERN_SYNTAX_ERROR,
            "number formatter creation failed".to_string(),
        ))),
        Err(nf::CreateError::BadStyle) => Ok(Err((
            ue::U_UNSUPPORTED_ERROR,
            "number formatter creation failed".to_string(),
        ))),
        Err(nf::CreateError::Unsupported) => Ok(Err((
            ue::U_MEMORY_ALLOCATION_ERROR,
            "number formatter creation failed".to_string(),
        ))),
    }
}

fn n_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<NumberFormat>(&o) {
        return already_constructed(vm, "NumberFormatter");
    }
    reset(vm);
    match make(vm, a)? {
        Ok(f) => {
            store(&o, f);
            Ok(Value::Null)
        }
        Err((code, m)) => {
            let msg = format!("{}(): {}", a.fname(), m);
            super::set_error_quiet(vm, code, msg.clone());
            vm.throw("IntlException", &msg)
        }
    }
}

fn n_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    match make(vm, a)? {
        Ok(f) => Ok(Value::Object(new_obj(vm, "NumberFormatter", f)?)),
        Err((code, m)) => {
            fail(vm, a, code, &m)?;
            Ok(Value::Null)
        }
    }
}

fn fmt_of(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize, NumberFormat)> {
    let (o, off) = this_or_first(a);
    match with_obj::<NumberFormat, NumberFormat>(&o, |x| x.v.clone()) {
        Some(f) => Ok((o, off, f)),
        None => vm.throw("Error", "NumberFormatter object is not initialized"),
    }
}

fn update(o: &ObjRef, f: impl FnOnce(&mut NumberFormat) -> bool) -> bool {
    with_obj::<NumberFormat, bool>(o, |x| f(&mut x.v)).unwrap_or(false)
}

fn type_arg(vm: &mut Vm, a: &Args, i: usize, parse: bool) -> R<i64> {
    let t = a.int_or(i, if parse { 3 } else { 0 });
    if t == 4 {
        let (fname, what) = match (a.this.is_some(), parse) {
            (true, false) => ("NumberFormatter::formatCurrency()", "method"),
            (true, true) => ("NumberFormatter::parseCurrency()", "method"),
            (false, false) => ("numfmt_format_currency()", "function"),
            (false, true) => ("numfmt_parse_currency()", "function"),
        };
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($type) cannot be NumberFormatter::TYPE_CURRENCY constant, use {fname} {what} instead",
                i + 1
            ),
        );
    }
    if !(0..=3).contains(&t) || (parse && t == 0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($type) must be a NumberFormatter::TYPE_* constant",
                i + 1
            ),
        );
    }
    Ok(t)
}

fn n_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    let num = a.val(off);
    let t = type_arg(vm, a, off + 1, false)?;
    reset_obj::<NumberFormat>(vm, &o);
    let r = match (t, &num) {
        (0, Value::Int(i)) => f.format_i64(*i),
        (0, Value::Float(x)) => f.format_f64(*x),
        (1, v) => {
            let i = to_i64(v);
            f.format_i64(i as i32 as i64)
        }
        (2, v) => f.format_i64(to_i64(v)),
        (_, v) => f.format_f64(match v {
            Value::Int(i) => *i as f64,
            Value::Float(x) => *x,
            _ => 0.0,
        }),
    };
    match r {
        Ok(s) => Ok(Value::from(s.as_str())),
        Err(_) => {
            fail_obj::<NumberFormat>(
                vm,
                a,
                &o,
                ue::U_FORMAT_INEXACT_ERROR,
                "Number formatting failed",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn to_i64(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Float(f) => {
            if f.is_nan() {
                0
            } else {
                f.clamp(i64::MIN as f64, i64::MAX as f64) as i64
            }
        }
        _ => 0,
    }
}

fn set_out(vm: &mut Vm, a: &Args, i: usize, v: Value) -> R<()> {
    if let Some(r) = a.ref_at(i) {
        let v = vm.typed_ref_value(&r, v)?;
        drop(r.set(v));
    }
    Ok(())
}

/// The UTF-16 offset of char index `c` in `s`.
fn u16_off(s: &str, c: usize) -> i64 {
    s.chars().take(c).map(char::len_utf16).sum::<usize>() as i64
}

/// The char index of UTF-16 offset `u` in `s`.
fn char_of_u16(s: &str, u: i64) -> usize {
    let mut n = 0i64;
    for (i, c) in s.chars().enumerate() {
        if n >= u {
            return i;
        }
        n += c.len_utf16() as i64;
    }
    s.chars().count()
}

fn n_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    let t = type_arg(vm, a, off + 1, true)?;
    reset_obj::<NumberFormat>(vm, &o);
    let Ok(text) = std::str::from_utf8(a.bytes(off)).map(str::to_string) else {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            ue::U_INVALID_CHAR_FOUND,
            "String conversion to UTF-16 failed",
        )?;
        return Ok(Value::Bool(false));
    };
    let start = match a.get(off + 2).map(|v| v.deref()) {
        Some(Value::Int(p)) => char_of_u16(&text, p),
        _ => 0,
    };
    match f.parse(&text, start) {
        Some((v, _, end)) => {
            if a.has(off + 2) {
                set_out(vm, a, off + 2, Value::Int(u16_off(&text, end)))?;
            }
            Ok(match t {
                1 => {
                    if v.abs() > i32::MAX as f64 {
                        fail_obj::<NumberFormat>(
                            vm,
                            a,
                            &o,
                            ue::U_INVALID_FORMAT_ERROR,
                            "Number parsing failed",
                        )?;
                        Value::Bool(false)
                    } else {
                        Value::Int(v as i64)
                    }
                }
                2 => Value::Int(v as i64),
                _ => Value::Float(v),
            })
        }
        None => {
            if a.has(off + 2) {
                set_out(vm, a, off + 2, Value::Int(u16_off(&text, start)))?;
            }
            fail_obj::<NumberFormat>(vm, a, &o, ue::U_PARSE_ERROR, "Number parsing failed")?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_format_currency(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, mut f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let amount = a.float(off);
    let cur = a.str(off + 1).to_string_lossy();
    f.set_text_attribute(nf::CURRENCY_CODE, &cur);
    match f.format_f64(amount) {
        Ok(s) => Ok(Value::from(s.as_str())),
        Err(_) => {
            fail_obj::<NumberFormat>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "Number formatting failed",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_parse_currency(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let text = String::from_utf8_lossy(a.bytes(off)).into_owned();
    let start = match a.get(off + 2).map(|v| v.deref()) {
        Some(Value::Int(p)) => char_of_u16(&text, p),
        _ => 0,
    };
    match f.parse_currency(&text, start) {
        Some((v, code, end)) => {
            set_out(vm, a, off + 1, Value::from(code.as_str()))?;
            if a.has(off + 2) {
                set_out(vm, a, off + 2, Value::Int(u16_off(&text, end)))?;
            }
            Ok(Value::Float(v))
        }
        None => {
            fail_obj::<NumberFormat>(vm, a, &o, ue::U_PARSE_ERROR, "Number parsing failed")?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_set_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let attr = a.int(off);
    let v = match a.val(off + 1) {
        Value::Int(i) => i as f64,
        Value::Float(x) => x,
        _ => 0.0,
    };
    let ok = update(&o, |f| f.set_attribute(attr, v));
    if !ok {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            ue::U_UNSUPPORTED_ERROR,
            "Error setting attribute value",
        )?;
    }
    Ok(Value::Bool(ok))
}

fn n_get_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let attr = a.int(off);
    match f.attribute(attr) {
        Some(v) => Ok(if attr == nf::ROUNDING_INCREMENT {
            Value::Float(v)
        } else {
            Value::Int(v as i64)
        }),
        None => {
            fail_obj::<NumberFormat>(
                vm,
                a,
                &o,
                ue::U_UNSUPPORTED_ERROR,
                "Error getting attribute value",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_set_text_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let attr = a.int(off);
    let v = a.str(off + 1).to_string_lossy();
    let ok = update(&o, |f| f.set_text_attribute(attr, &v));
    if !ok {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            ue::U_UNSUPPORTED_ERROR,
            "Error setting text attribute",
        )?;
    }
    Ok(Value::Bool(ok))
}

fn n_get_text_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    match f.text_attribute(a.int(off)) {
        Some(s) => Ok(Value::from(s.as_str())),
        None => {
            fail_obj::<NumberFormat>(
                vm,
                a,
                &o,
                ue::U_UNSUPPORTED_ERROR,
                "Error getting attribute value",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn n_set_symbol(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let s = a.int(off);
    if !(0..nf::SYMBOL_COUNT as i64).contains(&s) {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "invalid symbol value",
        )?;
        return Ok(Value::Bool(false));
    }
    let v = a.str(off + 1).to_string_lossy();
    let ok = update(&o, |f| f.set_symbol(s, &v));
    Ok(Value::Bool(ok))
}

fn n_get_symbol(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let s = a.int(off);
    if !(0..nf::SYMBOL_COUNT as i64).contains(&s) {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "invalid symbol value",
        )?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::from(f.symbol(s).unwrap_or("")))
}

fn n_set_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let p = a.str(off).to_string_lossy();
    let ok = update(&o, |f| f.set_pattern(&p).is_ok());
    if !ok {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            0x10013,
            "Error setting pattern value at line 0, offset 0",
        )?;
    }
    Ok(Value::Bool(ok))
}

fn n_get_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    Ok(Value::from(f.to_pattern().as_str()))
}

fn n_get_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, f) = fmt_of(vm, a)?;
    reset_obj::<NumberFormat>(vm, &o);
    let t = a.int_or(off, 0);
    if t != 0 && t != 1 {
        fail_obj::<NumberFormat>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "Error getting locale by type",
        )?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::from(resolved_locale(&f.locale).as_str()))
}

/// The most specific available locale of `id` (ICU's valid/actual
/// locale of the number format data).
pub(crate) fn resolved_locale(id: &str) -> String {
    let p = goro_ext_intl::locale::parse(id);
    let avail = goro_ext_intl::locale::available_locales();
    let has = |s: &str| avail.iter().any(|a| a.eq_ignore_ascii_case(s));
    let mut cands = Vec::new();
    if !p.script.is_empty() && !p.region.is_empty() {
        cands.push(format!("{}_{}_{}", p.language, p.script, p.region));
    }
    if !p.region.is_empty() {
        cands.push(format!("{}_{}", p.language, p.region));
    }
    if !p.script.is_empty() {
        cands.push(format!("{}_{}", p.language, p.script));
    }
    cands.push(p.language.clone());
    cands
        .into_iter()
        .find(|c| has(c))
        .unwrap_or_else(|| "root".to_string())
}

fn n_get_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let _ = vm;
    Ok(Value::Int(obj_err::<NumberFormat>(&o).code))
}

fn n_get_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let _ = vm;
    Ok(Value::from(
        obj_err::<NumberFormat>(&o).full_message().as_str(),
    ))
}
