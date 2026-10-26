//! ext/intl/formatter: NumberFormatter and the numfmt_*() functions, over
//! the DecimalFormat engine of [`crate::decfmt`] (unum_open() styles,
//! attributes, symbols, patterns, parsing) and the `intl` crate's compact,
//! ordinal and spell-out formatters.

use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::decfmt::{DecimalFormat, Dq};
use crate::error::{self, IntlError};
use crate::numdata::{self, LocaleData};
use crate::{Call, Obj, obj, s, sv};

crate::intl_functions! { pub FUNCTIONS:
    "numfmt_create" => create,
    "numfmt_format" => format,
    "numfmt_parse" => parse,
    "numfmt_format_currency" => format_currency,
    "numfmt_parse_currency" => parse_currency,
    "numfmt_set_attribute" => set_attribute,
    "numfmt_get_attribute" => get_attribute,
    "numfmt_set_text_attribute" => set_text_attribute,
    "numfmt_get_text_attribute" => get_text_attribute,
    "numfmt_set_symbol" => set_symbol,
    "numfmt_get_symbol" => get_symbol,
    "numfmt_set_pattern" => set_pattern,
    "numfmt_get_pattern" => get_pattern,
    "numfmt_get_locale" => get_locale,
    "numfmt_get_error_code" => get_error_code,
    "numfmt_get_error_message" => get_error_message,
}

pub const METHODS: &[(&str, crate::Imp)] = &[("NumberFormatter::__construct", construct)];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] =
    &[("NumberFormatter", Some(create_object))];

// UNumberFormatStyle
pub const PATTERN_DECIMAL: i64 = 0;
pub const DECIMAL: i64 = 1;
pub const CURRENCY: i64 = 2;
pub const PERCENT: i64 = 3;
pub const SCIENTIFIC: i64 = 4;
pub const SPELLOUT: i64 = 5;
pub const ORDINAL: i64 = 6;
pub const DURATION: i64 = 7;
pub const NUMBERING_SYSTEM: i64 = 8;
pub const PATTERN_RULEBASED: i64 = 9;
pub const CURRENCY_ISO: i64 = 10;
pub const CURRENCY_PLURAL: i64 = 11;
pub const CURRENCY_ACCOUNTING: i64 = 12;
pub const CASH_CURRENCY: i64 = 13;
pub const COMPACT_SHORT: i64 = 14;
pub const COMPACT_LONG: i64 = 15;
pub const CURRENCY_STANDARD: i64 = 16;

// format types
const TYPE_DEFAULT: i64 = 0;
const TYPE_INT32: i64 = 1;
const TYPE_INT64: i64 = 2;
const TYPE_DOUBLE: i64 = 3;
const TYPE_CURRENCY: i64 = 4;

/// What a NumberFormatter formats with.
#[derive(Clone, Debug)]
pub enum Kind {
    Decimal(DecimalFormat),
    Compact(bool, DecimalFormat),
    /// A rule-based format: SPELLOUT / ORDINAL / DURATION /
    /// NUMBERING_SYSTEM, or custom rules.
    Rbnf(i64, Option<String>, DecimalFormat),
}

/// An open number format (UNumberFormat).
#[derive(Clone, Debug)]
pub struct Nf {
    pub kind: Kind,
    pub style: i64,
    /// The valid locale.
    pub valid: String,
    pub data: Rc<LocaleData>,
    /// RBNF attributes.
    pub rb_lenient: bool,
    pub rb_rounding: i32,
}

impl Nf {
    /// unum_open(): Err(UErrorCode).
    pub fn open(vm: &Vm, locale: &str, style: i64, pattern: Option<&str>) -> Result<Nf, i32> {
        let canon = numdata::canonicalize(locale);
        let default = crate::common::default_locale(vm);
        let valid = numdata::valid_locale(&canon, &default);
        let data = numdata::locale_data(&valid, &canon);
        let loc_cur = numdata::locale_currency(&canon);
        let mut df = DecimalFormat::new(&data, loc_cur.clone());
        let pat = |p: &str| p.to_string();
        let set = |df: &mut DecimalFormat, p: &str| -> Result<(), i32> {
            crate::decfmt::apply_pattern(&mut df.props, p, 1)
        };
        let kind = match style {
            PATTERN_DECIMAL => {
                set(&mut df, pattern.unwrap_or(""))?;
                Kind::Decimal(df)
            }
            DECIMAL => {
                set(&mut df, &data.decimal)?;
                Kind::Decimal(df)
            }
            CURRENCY | CURRENCY_STANDARD | CASH_CURRENCY | CURRENCY_ACCOUNTING | CURRENCY_ISO
            | CURRENCY_PLURAL => {
                let p = match style {
                    CURRENCY_ACCOUNTING => pat(&data.accounting),
                    CURRENCY_ISO => data.currency.replace('¤', "¤¤"),
                    CURRENCY_PLURAL => {
                        let num = data.decimal.split(';').next().unwrap_or("#,##0.###");
                        format!("{} ¤¤¤", num.replace(".###", ".00"))
                    }
                    _ => pat(&data.currency),
                };
                set(&mut df, &p)?;
                df.currency_attr = Some(loc_cur.clone().unwrap_or_else(|| "XXX".into()));
                df.props.currency = loc_cur.clone();
                if style == CASH_CURRENCY {
                    df.props.cash = true;
                }
                Kind::Decimal(df)
            }
            PERCENT => {
                set(&mut df, &data.percent)?;
                Kind::Decimal(df)
            }
            SCIENTIFIC => {
                set(&mut df, &data.scientific)?;
                Kind::Decimal(df)
            }
            COMPACT_SHORT | COMPACT_LONG => {
                set(&mut df, &data.decimal)?;
                df.props.max_frac = 0;
                df.props.min_frac = 0;
                Kind::Compact(style == COMPACT_LONG, df)
            }
            SPELLOUT | ORDINAL | DURATION | NUMBERING_SYSTEM => {
                set(&mut df, &data.decimal)?;
                Kind::Rbnf(style, None, df)
            }
            PATTERN_RULEBASED => match pattern {
                Some(p) if !p.is_empty() => {
                    set(&mut df, &data.decimal)?;
                    Kind::Rbnf(style, Some(p.to_string()), df)
                }
                _ => return Err(error::U_MEMORY_ALLOCATION_ERROR),
            },
            _ => return Err(error::U_UNSUPPORTED_ERROR),
        };
        Ok(Nf {
            kind,
            style,
            valid,
            data,
            rb_lenient: false,
            rb_rounding: crate::decfmt::ROUND_UNNECESSARY,
        })
    }

    pub fn df(&self) -> Option<&DecimalFormat> {
        match &self.kind {
            Kind::Decimal(d) => Some(d),
            _ => None,
        }
    }

    pub fn df_mut(&mut self) -> Option<&mut DecimalFormat> {
        match &mut self.kind {
            Kind::Decimal(d) => Some(d),
            _ => None,
        }
    }

    /// Format a quantity.
    pub fn format_dq(&self, q: Dq) -> Result<String, i32> {
        match &self.kind {
            Kind::Decimal(d) => d.format_dq(q, None),
            // (intl's compact rounding cannot take NaN, infinity or -0)
            Kind::Compact(_, d) if !q.is_finite() || q.is_zero() => d.format_dq(q, None),
            Kind::Compact(long, _) => Ok(compact(&self.data.tag, *long, q.to_f64())),
            Kind::Rbnf(style, rules, df) => Ok(rbnf_format(self, *style, rules.as_deref(), df, q)),
        }
    }

    pub fn format_f64(&self, v: f64) -> Result<String, i32> {
        self.format_dq(Dq::from_f64(v))
    }

    pub fn format_i64(&self, v: i64) -> Result<String, i32> {
        self.format_dq(Dq::from_i64(v))
    }

    pub fn to_pattern(&self) -> String {
        match &self.kind {
            Kind::Decimal(d) => d.to_pattern(),
            Kind::Compact(..) => "#".to_string(),
            Kind::Rbnf(_, Some(r), _) => r.clone(),
            Kind::Rbnf(..) => String::new(),
        }
    }

    /// Parse from `pos` (code point offsets): (value, end) or Err(error
    /// position).
    pub fn parse(&self, text: &[char], pos: usize) -> Result<(Dq, usize), usize> {
        match &self.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) | Kind::Rbnf(_, _, d) => {
                d.parse(text, pos, false).map(|(q, e, _)| (q, e))
            }
        }
    }
}

/// Compact notation (DECIMAL_COMPACT_SHORT / LONG).
fn compact(tag: &str, long: bool, v: f64) -> String {
    let mut o = intl::number::NumberFormatOptions::default();
    o.notation = intl::number::Notation::Compact;
    if long {
        o.compact_display = intl::number::CompactDisplay::Long;
    }
    intl::number::format(tag, v, &o)
}

/// The word between the integer and the fraction digits of a spelled-out
/// number.
fn point_word(lang: &str) -> &'static str {
    match lang {
        "de" | "nl" | "sv" | "da" | "nb" | "no" | "nn" => match lang {
            "de" => "Komma",
            _ => "komma",
        },
        "fr" => "virgule",
        "es" | "ca" => "coma",
        "it" => "virgola",
        "pt" => "vírgula",
        "pl" => "przecinek",
        "ro" => "virgulă",
        "fi" => "pilkku",
        "tr" => "virgül",
        "uk" => "кома",
        // (CLDR ja: "x.x: ←←・→→→", zh: "x.x: ←←点→→→")
        "ja" => "・",
        "zh" => "点",
        _ => "point",
    }
}

/// Spell out a quantity.
fn spellout(tag: &str, q: &Dq, df: &DecimalFormat) -> String {
    let lang = numdata::language(tag);
    if !q.is_finite() {
        return df.format_dq(q.clone(), None).unwrap_or_default();
    }
    let ip = {
        let mut t = q.clone();
        t.neg = false;
        let _ = t.round_at(0, crate::decfmt::ROUND_DOWN);
        t.to_i64()
    };
    let Some(ip) = ip else {
        return df.format_dq(q.clone(), None).unwrap_or_default();
    };
    let Some(int_words) = intl::spellout::spell_cardinal(tag, ip) else {
        return df.format_dq(q.clone(), None).unwrap_or_default();
    };
    let mut out = String::new();
    if q.neg && !q.is_zero() {
        let m1 = intl::spellout::spell_cardinal(tag, -1).unwrap_or_default();
        let p1 = intl::spellout::spell_cardinal(tag, 1).unwrap_or_default();
        let pre = m1.strip_suffix(&p1).unwrap_or("minus ");
        // (intl's rule sets lack some -x rules and fall back to English)
        let pre = match (pre, lang.as_str()) {
            ("minus ", "fr") => "moins ",
            ("minus ", "es" | "pt") => "menos ",
            ("minus ", "it") => "meno ",
            ("minus ", "ca") => "menys ",
            ("minus ", "nl") => "min ",
            (p, _) => p,
        };
        out.push_str(pre);
    }
    out.push_str(&int_words);
    if q.lower() < 0 {
        // (`→→→`: the digits without spaces, nor around the separator)
        let sep = if matches!(lang.as_str(), "ja" | "zh") { "" } else { " " };
        out.push_str(sep);
        out.push_str(point_word(&lang));
        let mut m = -1;
        while m >= q.lower() {
            let dg = q.digit(m);
            out.push_str(sep);
            out.push_str(&intl::spellout::spell_cardinal(tag, dg as i64).unwrap_or_else(|| dg.to_string()));
            m -= 1;
        }
    }
    out
}

/// %duration (English RBNF): seconds as h:mm:ss.
fn duration(q: &Dq, df: &DecimalFormat) -> String {
    let mut t = q.clone();
    let _ = t.round_at(0, crate::decfmt::ROUND_HALFEVEN);
    let neg = t.neg && !t.is_zero();
    t.neg = false;
    let v = t.to_i64().unwrap_or(0);
    let body = if v < 60 {
        format!("{} sec.", v)
    } else if v < 3600 {
        format!("{}:{:02}", v / 60, v % 60)
    } else {
        let h = df.format_i64(v / 3600).unwrap_or_else(|_| (v / 3600).to_string());
        format!("{}:{:02}:{:02}", h, (v / 60) % 60, v % 60)
    };
    if neg { format!("-{}", body) } else { body }
}

fn rbnf_format(nf: &Nf, style: i64, rules: Option<&str>, df: &DecimalFormat, q: Dq) -> String {
    let tag = &nf.data.tag;
    let lang = numdata::language(tag);
    match style {
        // (intl leaves RBNF's leading-space apostrophes in)
        SPELLOUT => spellout(tag, &q, df).replace("' ", " "),
        ORDINAL => {
            // (rounded to an integer first, as ICU's RBNF rounds with no
            // fraction digits)
            let mut t = q.clone();
            let _ = t.round_at(0, crate::decfmt::ROUND_HALFEVEN);
            if t.to_i64().is_none() {
                return df.format_dq(q, None).unwrap_or_default();
            }
            if t.is_zero() {
                t.neg = false;
            }
            let mut d = df.clone();
            d.props.max_frac = 0;
            let rules = crate::rbnf::digits_ordinal_rules(&nf.valid);
            crate::rbnf::format_with(rules, Some("%digits-ordinal"), &t, &d, &lang)
        }
        DURATION => {
            if lang == "en" {
                duration(&q, df)
            } else {
                let mut d = df.clone();
                d.props.max_frac = 0;
                d.format_dq(q, None).unwrap_or_default()
            }
        }
        PATTERN_RULEBASED => crate::rbnf::format(rules.unwrap_or(""), &q, df),
        _ => df.format_dq(q, None).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// The object

/// NumberFormatter_object: the formatter (None until constructed) and the
/// object's error.
#[derive(Clone, Debug, Default)]
pub struct NfState {
    pub nf: Option<Nf>,
    pub err: IntlError,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_object),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, NfState::default(), &HANDLERS);
}

fn clone_object(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let Some(st) = obj::state::<NfState>(src) else {
        return;
    };
    let constructed = st.borrow().nf.is_some();
    obj::clone_state::<NfState>(src, dst, |s| s.clone());
    if !constructed {
        crate::zpp::throw(vm, b"Error", "Cannot clone uninitialized NumberFormatter");
    }
}

fn state(o: &Obj) -> Option<Rc<std::cell::RefCell<NfState>>> {
    obj::state::<NfState>(o)
}

/// FORMATTER_METHOD_FETCH_OBJECT: the state, with the error reset; throws
/// for an unconstructed object.
fn fetch(vm: &mut Vm, c: &Call) -> Option<Rc<std::cell::RefCell<NfState>>> {
    let st = state(&c.obj())?;
    st.borrow_mut().err.reset();
    if st.borrow().nf.is_none() {
        crate::zpp::throw(vm, b"Error", "Found unconstructed NumberFormatter");
        return None;
    }
    Some(st)
}

/// INTL_METHOD_CHECK_STATUS: set the global code; on failure the message
/// on both errors. True when failed.
fn check_status(vm: &mut Vm, c: &Call, st: &Rc<std::cell::RefCell<NfState>>, msg: &str) -> bool {
    let code = st.borrow().err.code;
    error::set_global_code(code);
    if error::failure(code) {
        let mut e = st.borrow().err.clone();
        error::set_both(vm, &c.name, &mut e, code, msg);
        st.borrow_mut().err = e;
        return true;
    }
    false
}

fn set_code(st: &Rc<std::cell::RefCell<NfState>>, code: i32) {
    st.borrow_mut().err.code = code;
}

/// numfmt_ctor()
fn ctor(vm: &mut Vm, c: &mut Call, o: &Obj) -> bool {
    let locale = c.str(0);
    let style = c.long(1);
    let pattern = c.opt_str(2);
    if locale.len() > crate::common::MAX_LOCALE_LEN {
        let msg = format!(
            "Locale string too long, should be no longer than {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
        return false;
    }
    let Some(st) = state(o) else {
        return false;
    };
    st.borrow_mut().err.reset();
    if st.borrow().nf.is_some() {
        crate::zpp::throw(vm, b"Error", "NumberFormatter object is already constructed");
        return false;
    }
    let pattern = match pattern {
        Some(p) if !p.is_empty() => match std::str::from_utf8(&p) {
            Ok(t) => Some(t.to_string()),
            Err(_) => {
                set_code(&st, error::U_INVALID_CHAR_FOUND);
                check_status(vm, c, &st, "error converting pattern to UTF-16");
                return false;
            }
        },
        _ => None,
    };
    let locale = crate::common::locale_or_default(vm, &locale);
    if !numdata::has_iso3_language(&locale) {
        crate::zpp::value_error(vm, &c.name, 1, "locale", &format!("\"{}\" is invalid", locale));
        return false;
    }
    match Nf::open(vm, &locale, style, pattern.as_deref()) {
        Ok(nf) => {
            st.borrow_mut().nf = Some(nf);
            error::set_global_code(error::U_ZERO_ERROR);
            true
        }
        Err(code) => {
            set_code(&st, code);
            check_status(vm, c, &st, "number formatter creation failed");
            false
        }
    }
}

/// numfmt_create() / NumberFormatter::create()
fn create(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = obj::object_init_ex(vm, b"numberformatter") else {
        return Value::Null;
    };
    if ctor(vm, c, &o) {
        Value::Object(o)
    } else {
        Value::Null
    }
}

/// Run `f` with intl.use_exceptions on and intl.error_level 0 (the
/// constructors).
pub fn with_ctor_errors<T>(vm: &mut Vm, f: impl FnOnce(&mut Vm) -> T) -> T {
    let old_ue = vm.constants.get(b"intl.use_exceptions".as_slice()).cloned();
    let old_el = vm.constants.get(b"intl.error_level".as_slice()).cloned();
    vm.constants.insert(b"intl.use_exceptions".to_vec(), s(b"1"));
    vm.constants.insert(b"intl.error_level".to_vec(), s(b"0"));
    let r = f(vm);
    match old_ue {
        Some(v) => vm.constants.insert(b"intl.use_exceptions".to_vec(), v),
        None => vm.constants.shift_remove(b"intl.use_exceptions".as_slice()),
    };
    match old_el {
        Some(v) => vm.constants.insert(b"intl.error_level".to_vec(), v),
        None => vm.constants.shift_remove(b"intl.error_level".as_slice()),
    };
    r
}

/// NumberFormatter::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    with_ctor_errors(vm, |vm| ctor(vm, c, &o));
    Value::Null
}

fn type_currency_error(vm: &mut Vm, c: &Call, method: &str, func: &str) {
    if c.is_method {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            "type",
            &format!(
                "cannot be NumberFormatter::TYPE_CURRENCY constant, use NumberFormatter::{}() method instead",
                method
            ),
        );
    } else {
        crate::zpp::value_error(
            vm,
            &c.name,
            3,
            "type",
            &format!(
                "cannot be NumberFormatter::TYPE_CURRENCY constant, use {}() function instead",
                func
            ),
        );
    }
}

/// numfmt_format()
fn format(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let num = c.arg(0);
    let mut ty = if c.passed(1) { c.long(1) } else { TYPE_DEFAULT };
    if ty == TYPE_DEFAULT {
        ty = match num {
            Value::Long(_) => TYPE_INT64,
            _ => TYPE_DOUBLE,
        };
    }
    let r = {
        let b = st.borrow();
        let nf = b.nf.as_ref().unwrap();
        match ty {
            TYPE_INT32 => {
                let v = match num {
                    Value::Double(d) => goro_core::value::Value::Double(d).to_long(),
                    v => v.to_long(),
                };
                nf.format_i64(v as i32 as i64)
            }
            TYPE_INT64 => {
                let v = match num {
                    Value::Double(d) => {
                        if d.is_nan() {
                            i64::MIN
                        } else {
                            d as i64
                        }
                    }
                    v => v.to_long(),
                };
                nf.format_i64(v)
            }
            TYPE_DOUBLE => nf.format_f64(num.to_double()),
            TYPE_CURRENCY => {
                drop(b);
                type_currency_error(vm, c, "formatCurrency", "numfmt_format_currency");
                return Value::Null;
            }
            _ => {
                drop(b);
                let n = c.argn(1);
                crate::zpp::value_error(vm, &c.name, n, "type", "must be a NumberFormatter::TYPE_* constant");
                return Value::Null;
            }
        }
    };
    match r {
        Ok(s) => {
            error::set_global_code(error::U_ZERO_ERROR);
            sv(s.into_bytes())
        }
        Err(code) => {
            set_code(&st, code);
            check_status(vm, c, &st, "Number formatting failed");
            Value::False
        }
    }
}

/// UTF-16 offset -> code point index.
fn u16_to_cp(text: &[char], pos: i64) -> Option<usize> {
    if pos < 0 {
        return None;
    }
    let mut u = 0i64;
    for (i, ch) in text.iter().enumerate() {
        if u >= pos {
            return Some(i);
        }
        u += ch.len_utf16() as i64;
    }
    if u >= pos { Some(text.len()) } else { None }
}

fn cp_to_u16(text: &[char], i: usize) -> i64 {
    text[..i.min(text.len())].iter().map(|c| c.len_utf16() as i64).sum()
}

/// numfmt_parse()
fn parse(vm: &mut Vm, c: &mut Call) -> Value {
    let input = c.str(0);
    let ty = if c.passed(1) { c.long(1) } else { TYPE_DOUBLE };
    let zpos = c.arg(2);
    let has_pos = c.passed(2) && !matches!(zpos, Value::Null | Value::Undef);
    let position = if has_pos { zpos.to_long() as i32 as i64 } else { 0 };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Ok(text) = std::str::from_utf8(&input) else {
        set_code(&st, error::U_INVALID_CHAR_FOUND);
        check_status(vm, c, &st, "String conversion to UTF-16 failed");
        return Value::False;
    };
    if ty == TYPE_CURRENCY {
        type_currency_error(vm, c, "parseCurrency", "numfmt_parse_currency");
        check_status(vm, c, &st, "Number parsing failed");
        return Value::Null;
    }
    if !matches!(ty, TYPE_INT32 | TYPE_INT64 | TYPE_DOUBLE) {
        let n = c.argn(1);
        crate::zpp::value_error(vm, &c.name, n, "type", "must be a NumberFormatter::TYPE_* constant");
        return Value::Null;
    }
    let chars: Vec<char> = text.chars().collect();
    let res = {
        let b = st.borrow();
        let nf = b.nf.as_ref().unwrap();
        match u16_to_cp(&chars, position) {
            Some(p) if p <= chars.len() => nf.parse(&chars, p),
            _ => Err(0),
        }
    };
    let (ret, newpos) = match res {
        Ok((q, end)) => {
            let v = match ty {
                TYPE_INT32 => Value::Long(dq_to_int32(&q, &st)),
                TYPE_INT64 => match dq_to_int64(&q) {
                    Ok(n) => Value::Long(n),
                    Err(n) => {
                        set_code(&st, error::U_INVALID_FORMAT_ERROR);
                        Value::Long(n)
                    }
                },
                _ => Value::Double(q.to_f64()),
            };
            (v, cp_to_u16(&chars, end))
        }
        Err(e) => {
            set_code(&st, error::U_PARSE_ERROR);
            (Value::False, cp_to_u16(&chars, e))
        }
    };
    if has_pos && !c.set_ref(vm, 2, Value::Long(newpos)) {
        return Value::Null;
    }
    if check_status(vm, c, &st, "Number parsing failed") {
        return Value::False;
    }
    ret
}

fn dq_to_int32(q: &Dq, st: &Rc<std::cell::RefCell<NfState>>) -> i64 {
    let v = q.to_f64();
    if v.is_nan() {
        return 0;
    }
    if v > i32::MAX as f64 {
        set_code(st, error::U_INVALID_FORMAT_ERROR);
        return i32::MAX as i64;
    }
    if v < i32::MIN as f64 {
        set_code(st, error::U_INVALID_FORMAT_ERROR);
        return i32::MIN as i64;
    }
    v.trunc() as i64
}

fn dq_to_int64(q: &Dq) -> Result<i64, i64> {
    let mut t = q.clone();
    let _ = t.round_at(0, crate::decfmt::ROUND_DOWN);
    if let Some(n) = t.to_i64() {
        return Ok(n);
    }
    let v = q.to_f64();
    if v.is_nan() {
        return Ok(0);
    }
    if v > 0.0 { Err(i64::MAX) } else { Err(i64::MIN) }
}

/// numfmt_format_currency()
fn format_currency(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let v = c.double(0);
    let code = c.str(1);
    let Ok(code) = std::str::from_utf8(&code) else {
        set_code(&st, error::U_INVALID_CHAR_FOUND);
        check_status(vm, c, &st, "Currency conversion to UTF-16 failed");
        return Value::False;
    };
    let r = {
        let b = st.borrow();
        let nf = b.nf.as_ref().unwrap();
        if code.chars().count() > 3 || code.is_empty() && false {
            Err(error::U_ILLEGAL_ARGUMENT_ERROR)
        } else {
            match &nf.kind {
                Kind::Decimal(d) => {
                    let code = code.to_ascii_uppercase();
                    d.format_dq(Dq::from_f64(v), Some(&code))
                }
                _ => nf.format_f64(v),
            }
        }
    };
    match r {
        Ok(s) => {
            error::set_global_code(error::U_ZERO_ERROR);
            sv(s.into_bytes())
        }
        Err(code) => {
            set_code(&st, code);
            check_status(vm, c, &st, "Number formatting failed");
            Value::False
        }
    }
}

/// numfmt_parse_currency()
fn parse_currency(vm: &mut Vm, c: &mut Call) -> Value {
    let input = c.str(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Ok(text) = std::str::from_utf8(&input) else {
        set_code(&st, error::U_INVALID_CHAR_FOUND);
        check_status(vm, c, &st, "String conversion to UTF-16 failed");
        return Value::False;
    };
    let zpos = c.arg(2);
    let has_pos = c.passed(2) && !matches!(zpos, Value::Null | Value::Undef);
    let position = if has_pos { zpos.to_long() as i32 as i64 } else { 0 };
    let chars: Vec<char> = text.chars().collect();
    let res = {
        let b = st.borrow();
        let nf = b.nf.as_ref().unwrap();
        let d = match &nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) | Kind::Rbnf(_, _, d) => d,
        };
        match u16_to_cp(&chars, position) {
            Some(p) if p <= chars.len() => d.parse(&chars, p, true),
            _ => Err(0),
        }
    };
    let (num, cur, newpos) = match res {
        Ok((q, end, cur)) => (q.to_f64(), cur.unwrap_or_default(), cp_to_u16(&chars, end)),
        Err(e) => {
            set_code(&st, error::U_PARSE_ERROR);
            (0.0, String::new(), cp_to_u16(&chars, e))
        }
    };
    if has_pos && !c.set_ref(vm, 2, Value::Long(newpos)) {
        return Value::Null;
    }
    if check_status(vm, c, &st, "Number parsing failed") {
        return Value::False;
    }
    if !c.set_ref(vm, 1, s(cur.as_bytes())) {
        return Value::Null;
    }
    Value::Double(num)
}

/// numfmt_get_attribute()
fn get_attribute(vm: &mut Vm, c: &mut Call) -> Value {
    let attr = c.long(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let mut ret = Value::Null;
    {
        let mut b = st.borrow_mut();
        let b = &mut *b;
        let nf = b.nf.as_ref().unwrap();
        let v: Option<Value> = match &nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) => match attr {
                0..=11 | 13..=19 => {
                    let v = d.get_attribute(attr);
                    (v != -1).then_some(Value::Long(v))
                }
                12 => {
                    let v = d.rounding_increment();
                    (v != -1.0).then_some(Value::Double(v))
                }
                _ => None,
            },
            Kind::Rbnf(..) => match attr {
                19 => Some(Value::Long(nf.rb_lenient as i64)),
                3 => Some(Value::Long(2_000_000_000)),
                4 | 5 => Some(Value::Long(1)),
                6 => Some(Value::Long(3)),
                7 | 8 => Some(Value::Long(0)),
                11 => Some(Value::Long(nf.rb_rounding as i64)),
                _ => None,
            },
        };
        match v {
            Some(v) => ret = v,
            None => b.err.code = error::U_UNSUPPORTED_ERROR,
        }
    }
    if check_status(vm, c, &st, "Error getting attribute value") {
        return Value::False;
    }
    ret
}

/// numfmt_set_attribute()
fn set_attribute(vm: &mut Vm, c: &mut Call) -> Value {
    let attr = c.long(0);
    let value = c.arg(1);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    {
        let mut b = st.borrow_mut();
        let b = &mut *b;
        let nf = b.nf.as_mut().unwrap();
        match attr {
            0..=11 | 13..=19 => {
                let v = value.to_long();
                match &mut nf.kind {
                    Kind::Decimal(d) | Kind::Compact(_, d) => d.set_attribute(attr, v),
                    Kind::Rbnf(..) => match attr {
                        19 => nf.rb_lenient = v != 0,
                        11 => nf.rb_rounding = v as i32,
                        _ => {}
                    },
                }
            }
            12 => {
                let v = value.to_double();
                if let Kind::Decimal(d) | Kind::Compact(_, d) = &mut nf.kind {
                    d.set_rounding_increment(v);
                }
            }
            _ => b.err.code = error::U_UNSUPPORTED_ERROR,
        }
    }
    if check_status(vm, c, &st, "Error setting attribute value") {
        return Value::False;
    }
    Value::True
}

/// numfmt_get_text_attribute()
fn get_text_attribute(vm: &mut Vm, c: &mut Call) -> Value {
    let attr = c.long(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let mut out = String::new();
    {
        let mut b = st.borrow_mut();
        let b = &mut *b;
        let nf = b.nf.as_ref().unwrap();
        let r: Option<String> = match &nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) => match attr {
                0..=3 => Some(d.affix_text(attr as usize)),
                4 => Some(d.props.pad_string.clone().unwrap_or_else(|| " ".to_string())),
                5 => Some(d.currency_attr.clone().unwrap_or_else(|| "XXX".to_string())),
                _ => None,
            },
            Kind::Rbnf(style, ..) => match attr {
                6 => Some(match *style {
                    SPELLOUT => "%spellout-numbering".to_string(),
                    ORDINAL => "%digits-ordinal".to_string(),
                    DURATION => "%duration".to_string(),
                    _ => String::new(),
                }),
                7 => Some(match *style {
                    SPELLOUT => "%spellout-numbering-year;%spellout-numbering;%spellout-cardinal;%spellout-ordinal".to_string(),
                    ORDINAL => "%digits-ordinal".to_string(),
                    DURATION => "%duration".to_string(),
                    _ => String::new(),
                }),
                _ => None,
            },
        };
        match r {
            Some(v) => out = v,
            None => b.err.code = error::U_UNSUPPORTED_ERROR,
        }
    }
    if check_status(vm, c, &st, "Error getting attribute value") {
        return Value::False;
    }
    sv(out.into_bytes())
}

/// numfmt_set_text_attribute()
fn set_text_attribute(vm: &mut Vm, c: &mut Call) -> Value {
    let attr = c.long(0);
    let value = c.str(1);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Ok(v) = std::str::from_utf8(&value) else {
        set_code(&st, error::U_INVALID_CHAR_FOUND);
        check_status(vm, c, &st, "Error converting attribute value to UTF-16");
        return Value::False;
    };
    {
        let mut b = st.borrow_mut();
        let b = &mut *b;
        let nf = b.nf.as_mut().unwrap();
        let ok = match &mut nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) => match attr {
                0 => {
                    d.props.pos_prefix = Some(v.to_string());
                    true
                }
                1 => {
                    d.props.pos_suffix = Some(v.to_string());
                    true
                }
                2 => {
                    d.props.neg_prefix = Some(v.to_string());
                    true
                }
                3 => {
                    d.props.neg_suffix = Some(v.to_string());
                    true
                }
                4 => {
                    d.props.pad_string = if v.is_empty() { None } else { Some(v.to_string()) };
                    true
                }
                5 => {
                    d.set_currency(v);
                    true
                }
                _ => false,
            },
            Kind::Rbnf(..) => attr == 6,
        };
        if !ok {
            b.err.code = error::U_UNSUPPORTED_ERROR;
        }
    }
    if check_status(vm, c, &st, "Error setting text attribute") {
        return Value::False;
    }
    Value::True
}

/// numfmt_get_symbol()
fn get_symbol(vm: &mut Vm, c: &mut Call) -> Value {
    let sym = c.long(0);
    if !(0..numdata::SYM_COUNT as i64).contains(&sym) {
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, "invalid symbol value");
        return Value::False;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let mut out = String::new();
    {
        let mut b = st.borrow_mut();
        let b = &mut *b;
        let nf = b.nf.as_ref().unwrap();
        match &nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) => out = d.symbols[sym as usize].clone(),
            Kind::Rbnf(..) => b.err.code = error::U_UNSUPPORTED_ERROR,
        }
    }
    if check_status(vm, c, &st, "Error getting symbol value") {
        return Value::False;
    }
    sv(out.into_bytes())
}

/// numfmt_set_symbol()
fn set_symbol(vm: &mut Vm, c: &mut Call) -> Value {
    let sym = c.long(0);
    let value = c.str(1);
    if !(0..numdata::SYM_COUNT as i64).contains(&sym) {
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, "invalid symbol value");
        return Value::False;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Ok(v) = std::str::from_utf8(&value) else {
        set_code(&st, error::U_INVALID_CHAR_FOUND);
        check_status(vm, c, &st, "Error converting symbol value to UTF-16");
        return Value::False;
    };
    {
        let mut b = st.borrow_mut();
        let b = &mut *b;
        let nf = b.nf.as_mut().unwrap();
        match &mut nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) => d.set_symbol(sym as usize, v),
            Kind::Rbnf(..) => b.err.code = error::U_UNSUPPORTED_ERROR,
        }
    }
    if check_status(vm, c, &st, "Error setting symbol value") {
        return Value::False;
    }
    Value::True
}

/// numfmt_get_pattern()
fn get_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let p = st.borrow().nf.as_ref().unwrap().to_pattern();
    error::set_global_code(error::U_ZERO_ERROR);
    sv(p.into_bytes())
}

/// numfmt_set_pattern()
fn set_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let pattern = c.str(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Ok(p) = std::str::from_utf8(&pattern) else {
        set_code(&st, error::U_INVALID_CHAR_FOUND);
        check_status(vm, c, &st, "Error converting pattern to UTF-16");
        return Value::False;
    };
    let r = {
        let mut b = st.borrow_mut();
        let nf = b.nf.as_mut().unwrap();
        match &mut nf.kind {
            Kind::Decimal(d) | Kind::Compact(_, d) => {
                let mut props = d.props.clone();
                match crate::decfmt::apply_pattern(&mut props, p, 0) {
                    Ok(()) => {
                        d.props = props;
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            Kind::Rbnf(_, rules, _) => {
                *rules = Some(p.to_string());
                Ok(())
            }
        }
    };
    if let Err(code) = r {
        set_code(&st, code);
        let mut e = st.borrow().err.clone();
        error::set_both(vm, &c.name, &mut e, code, "Error setting pattern value at line 0, offset 0");
        st.borrow_mut().err = e;
        return Value::False;
    }
    Value::True
}

/// numfmt_get_locale()
fn get_locale(vm: &mut Vm, c: &mut Call) -> Value {
    let ty = if c.passed(0) { c.long(0) } else { 0 };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    if ty != 0 && ty != 1 {
        set_code(&st, error::U_ILLEGAL_ARGUMENT_ERROR);
        check_status(vm, c, &st, "Error getting locale");
        return Value::False;
    }
    let v = st.borrow().nf.as_ref().unwrap().valid.clone();
    error::set_global_code(error::U_ZERO_ERROR);
    s(v.as_bytes())
}

/// numfmt_get_error_code()
fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    match state(&c.obj()) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

/// numfmt_get_error_message()
fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    match state(&c.obj()) {
        Some(st) => s(st.borrow().err.message().as_bytes()),
        None => s(b"U_ZERO_ERROR"),
    }
}
