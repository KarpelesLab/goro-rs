//! ext/intl/collator: Collator and the collator_*() functions, over
//! ICU4X's collator (CLDR root collation and tailorings, the collator
//! attributes and ICU-compatible sort keys).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use icu_collator::options::{AlternateHandling, CaseLevel, CollatorOptions, Strength};
use icu_collator::preferences::{CollationCaseFirst, CollationNumericOrdering};
use icu_collator::{CollatorBorrowed, CollatorPreferences};

use crate::error::{self, IntlError, U_ILLEGAL_ARGUMENT_ERROR, U_INVALID_CHAR_FOUND};
use crate::locale::uloc;
use crate::{Call, Obj, arr, s, sv};

#[path = "collator_data.rs"]
mod data;

crate::intl_functions! { pub FUNCTIONS:
    "collator_create" => create,
    "collator_compare" => compare,
    "collator_get_attribute" => get_attribute,
    "collator_set_attribute" => set_attribute,
    "collator_get_strength" => get_strength,
    "collator_set_strength" => set_strength,
    "collator_sort" => sort,
    "collator_sort_with_sort_keys" => sort_with_sort_keys,
    "collator_asort" => asort,
    "collator_get_locale" => get_locale,
    "collator_get_error_code" => get_error_code,
    "collator_get_error_message" => get_error_message,
    "collator_get_sort_key" => get_sort_key,
}

pub const METHODS: &[(&str, crate::Imp)] = &[("Collator::__construct", construct)];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("Collator", Some(create_object))];

// UColAttribute
const FRENCH_COLLATION: i64 = 0;
const ALTERNATE_HANDLING: i64 = 1;
const CASE_FIRST: i64 = 2;
const CASE_LEVEL: i64 = 3;
const NORMALIZATION_MODE: i64 = 4;
const STRENGTH: i64 = 5;
const HIRAGANA_QUATERNARY_MODE: i64 = 6;
const NUMERIC_COLLATION: i64 = 7;
// UColAttributeValue
const DEFAULT: i64 = -1;
const PRIMARY: i64 = 0;
const SECONDARY: i64 = 1;
const TERTIARY: i64 = 2;
const QUATERNARY: i64 = 3;
const IDENTICAL: i64 = 15;
const OFF: i64 = 16;
const ON: i64 = 17;
const SHIFTED: i64 = 20;
const NON_IGNORABLE: i64 = 21;
const LOWER_FIRST: i64 = 24;
const UPPER_FIRST: i64 = 25;

const SORT_STRING: i64 = 1;
const SORT_NUMERIC: i64 = 2;

/// An open collator (UCollator).
pub struct UCollator {
    /// The locale it was opened for, as an ICU4X locale.
    locale: icu_locale_core::Locale,
    valid: String,
    actual: String,
    /// Current attribute values, by UColAttribute.
    attrs: [i64; 8],
    /// The locale's defaults.
    defaults: [i64; 8],
    coll: CollatorBorrowed<'static>,
}

impl UCollator {
    /// ucol_open(): Err(UErrorCode) on failure.
    pub fn open(id: &str) -> Result<UCollator, i32> {
        if !crate::common::locale_id_ok(id) {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let name = uloc::get_name(id.as_bytes()).map_err(|_| U_ILLEGAL_ARGUMENT_ERROR)?;
        let locale = crate::common::icu4x_locale(&name);
        let (valid, actual) = collation_locales(&name);
        let base = Self::build(&locale, None)?;
        let r = base.resolved_options();
        let mut defaults = [OFF; 8];
        defaults[STRENGTH as usize] = match r.strength {
            Strength::Primary => PRIMARY,
            Strength::Secondary => SECONDARY,
            Strength::Quaternary => QUATERNARY,
            Strength::Identical => IDENTICAL,
            _ => TERTIARY,
        };
        defaults[ALTERNATE_HANDLING as usize] = match r.alternate_handling {
            AlternateHandling::Shifted => SHIFTED,
            _ => NON_IGNORABLE,
        };
        defaults[CASE_FIRST as usize] = match r.case_first {
            CollationCaseFirst::Upper => UPPER_FIRST,
            CollationCaseFirst::Lower => LOWER_FIRST,
            _ => OFF,
        };
        defaults[CASE_LEVEL as usize] = if r.case_level == CaseLevel::On { ON } else { OFF };
        defaults[NUMERIC_COLLATION as usize] = if r.numeric == CollationNumericOrdering::True {
            ON
        } else {
            OFF
        };
        let lang = locale.id.language.as_str();
        let region = locale.id.region.map(|r| r.to_string()).unwrap_or_default();
        if lang == "fr" && region == "CA" {
            defaults[FRENCH_COLLATION as usize] = ON;
        }
        Ok(UCollator {
            locale,
            valid,
            actual,
            attrs: defaults,
            defaults,
            coll: base,
        })
    }

    fn build(locale: &icu_locale_core::Locale, attrs: Option<&[i64; 8]>) -> Result<CollatorBorrowed<'static>, i32> {
        let mut prefs = CollatorPreferences::from(locale);
        let mut opts = crate::common::collator_options(locale);
        if let Some(a) = attrs {
            opts = CollatorOptions::default();
            opts.strength = Some(match a[STRENGTH as usize] {
                PRIMARY => Strength::Primary,
                SECONDARY => Strength::Secondary,
                QUATERNARY => Strength::Quaternary,
                IDENTICAL => Strength::Identical,
                _ => Strength::Tertiary,
            });
            opts.alternate_handling = Some(if a[ALTERNATE_HANDLING as usize] == SHIFTED {
                AlternateHandling::Shifted
            } else {
                AlternateHandling::NonIgnorable
            });
            opts.case_level = Some(if a[CASE_LEVEL as usize] == ON {
                CaseLevel::On
            } else {
                CaseLevel::Off
            });
            prefs.case_first = Some(match a[CASE_FIRST as usize] {
                UPPER_FIRST => CollationCaseFirst::Upper,
                LOWER_FIRST => CollationCaseFirst::Lower,
                _ => CollationCaseFirst::False,
            });
            prefs.numeric_ordering = Some(if a[NUMERIC_COLLATION as usize] == ON {
                CollationNumericOrdering::True
            } else {
                CollationNumericOrdering::False
            });
        }
        icu_collator::Collator::try_new(prefs, opts)
            .or_else(|_| icu_collator::Collator::try_new(Default::default(), opts))
            .map_err(|_| crate::error::U_INTERNAL_PROGRAM_ERROR)
    }

    /// ucol_getAttribute()
    pub fn get_attribute(&self, attr: i64) -> Result<i64, i32> {
        if !(0..8).contains(&attr) {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        Ok(self.attrs[attr as usize])
    }

    /// ucol_setAttribute()
    pub fn set_attribute(&mut self, attr: i64, value: i64) -> Result<(), i32> {
        if !(0..8).contains(&attr) {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let ok = value == DEFAULT
            || match attr {
                ALTERNATE_HANDLING => matches!(value, SHIFTED | NON_IGNORABLE),
                CASE_FIRST => matches!(value, OFF | LOWER_FIRST | UPPER_FIRST),
                STRENGTH => matches!(value, PRIMARY | SECONDARY | TERTIARY | QUATERNARY | IDENTICAL),
                _ => matches!(value, ON | OFF),
            };
        if !ok {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let v = if value == DEFAULT {
            self.defaults[attr as usize]
        } else {
            value
        };
        if self.attrs[attr as usize] != v {
            self.attrs[attr as usize] = v;
            if !matches!(attr, FRENCH_COLLATION | NORMALIZATION_MODE | HIRAGANA_QUATERNARY_MODE) {
                self.coll = Self::build(&self.locale, Some(&self.attrs))?;
            }
        }
        Ok(())
    }

    /// ucol_strcoll() on valid UTF-8: -1, 0, 1.
    pub fn strcoll(&self, a: &str, b: &str) -> i64 {
        match self.coll.compare(a, b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    /// ucol_getSortKey() (without the terminating NUL).
    pub fn sort_key(&self, s: &str) -> Vec<u8> {
        let mut k: Vec<u8> = Vec::new();
        let _ = self.coll.write_sort_key_to(s, &mut k);
        // ICU4X keeps the trailing NO_CE byte of the secondary, tertiary and
        // quaternary levels, which ICU drops ("Ignore the trailing NO_CE").
        let strength = self.attrs[STRENGTH as usize];
        let mut levels: Vec<bool> = Vec::new();
        if strength >= SECONDARY {
            levels.push(true);
        }
        if self.attrs[CASE_LEVEL as usize] == ON {
            levels.push(false);
        }
        if strength >= TERTIARY {
            levels.push(true);
        }
        if strength >= QUATERNARY {
            levels.push(true);
        }
        let mut pos = k.iter().position(|&b| b == 1).unwrap_or(k.len());
        let mut out = k[..pos].to_vec();
        for trailing in levels {
            if pos >= k.len() || k[pos] != 1 {
                break;
            }
            out.push(1);
            pos += 1;
            let end = k[pos..].iter().position(|&b| b == 1).map(|e| pos + e).unwrap_or(k.len());
            out.extend_from_slice(&k[pos..end]);
            pos = end;
            if trailing && pos < k.len() && k[pos] == 1 {
                pos += 1;
            }
        }
        out.extend_from_slice(&k[pos..]);
        out
    }
}

/// The valid and actual locales of a collator opened for ICU ID `name`
/// (uloc_getName() form), from ICU's collation locales.
fn collation_locales(name: &str) -> (String, String) {
    let (base, kw) = match name.find('@') {
        Some(p) => (&name[..p], &name[p..]),
        None => (name, ""),
    };
    let collation = uloc::keyword_value(name.as_bytes(), "collation")
        .ok()
        .flatten()
        .filter(|c| c != "standard");
    let _ = kw;
    let mut cur = base.to_string();
    loop {
        if let Some((_, v, a)) = data::COLLATION_LOCALES
            .iter()
            .find(|(l, _, _)| l.eq_ignore_ascii_case(&cur))
        {
            return match &collation {
                Some(c) => (format!("{}@collation={}", v, c), format!("{}@collation={}", a, c)),
                None => (v.to_string(), a.to_string()),
            };
        }
        match cur.rfind('_') {
            Some(p) => {
                cur.truncate(p);
                while cur.ends_with('_') {
                    cur.pop();
                }
                if cur.is_empty() {
                    break;
                }
            }
            None => break,
        }
    }
    ("root".to_string(), "root".to_string())
}

/// ICU's collation locales (the ICUDATA-coll package).
pub fn collation_locale_list() -> Vec<String> {
    data::COLLATION_LOCALES
        .iter()
        .map(|(l, _, _)| l.to_string())
        .collect()
}

/// Collator_object
#[derive(Default)]
pub struct CollatorObject {
    pub err: IntlError,
    pub ucoll: Option<UCollator>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn clone_obj(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    crate::obj::uncloneable(vm, src);
}

fn create_object(_vm: &mut Vm, o: &Obj) {
    crate::obj::attach(o, CollatorObject::default(), &HANDLERS);
}

fn state(o: &Obj) -> Rc<RefCell<CollatorObject>> {
    crate::obj::state::<CollatorObject>(o).unwrap_or_else(|| Rc::new(RefCell::new(CollatorObject::default())))
}

/// collator_ctor(): false on failure (error set).
fn ctor(vm: &mut Vm, c: &Call, o: &Obj) -> bool {
    error::reset_global();
    let loc = c.str(0);
    if loc.len() > crate::common::MAX_LOCALE_LEN {
        error::set_global(
            vm,
            &c.name,
            U_ILLEGAL_ARGUMENT_ERROR,
            &format!(
                "Locale string too long, should be no longer than {} characters",
                crate::common::MAX_LOCALE_LEN
            ),
        );
        return false;
    }
    let id = crate::common::locale_or_default(vm, &loc);
    let st = state(o);
    let mut st = st.borrow_mut();
    error::reset_both(&mut st.err);
    match UCollator::open(&id) {
        Ok(u) => {
            st.ucoll = Some(u);
            true
        }
        Err(code) => {
            error::set_both(vm, &c.name, &mut st.err, code, "unable to open ICU collator");
            false
        }
    }
}

/// collator_create()
fn create(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = crate::obj::object_init_ex(vm, b"collator") else {
        return Value::Null;
    };
    if ctor(vm, c, &o) {
        Value::Object(o)
    } else {
        Value::Null
    }
}

/// Run `f` with intl.use_exceptions on and intl.error_level 0, as the
/// constructors of the intl classes do.
pub fn with_ctor_errors<T>(vm: &mut Vm, f: impl FnOnce(&mut Vm) -> T) -> T {
    let ue = vm.constants.get(b"intl.use_exceptions".as_slice()).cloned();
    let el = vm.constants.get(b"intl.error_level".as_slice()).cloned();
    vm.constants.insert(b"intl.use_exceptions".to_vec(), s(b"1"));
    vm.constants.insert(b"intl.error_level".to_vec(), s(b"0"));
    let r = f(vm);
    for (k, v) in [(b"intl.use_exceptions".as_slice(), ue), (b"intl.error_level".as_slice(), el)] {
        match v {
            Some(v) => vm.constants.insert(k.to_vec(), v),
            None => vm.constants.remove(k),
        };
    }
    r
}

/// Collator::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    with_ctor_errors(vm, |vm| ctor(vm, c, &o));
    Value::Null
}

/// "Object not initialized" (an unconstructed Collator).
fn not_initialized(vm: &mut Vm, c: &Call, st: &mut CollatorObject) {
    let code = st.err.code;
    error::set_global_code(code);
    st.err.msg = Some(format!("{}(): Object not initialized", c.name));
    crate::zpp::throw(vm, b"Error", "Object not initialized");
}

/// COLLATOR_METHOD_FETCH_OBJECT: the object state, its error reset.
fn fetch(c: &Call) -> Rc<RefCell<CollatorObject>> {
    error::reset_global();
    let st = state(&c.obj());
    st.borrow_mut().err.reset();
    st
}

/// COLLATOR_CHECK_STATUS after an error `code`.
fn fail(vm: &mut Vm, c: &Call, st: &mut CollatorObject, code: i32, msg: &str) -> Value {
    error::set_both(vm, &c.name, &mut st.err, code, msg);
    Value::False
}

/// collator_compare()
fn compare(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    if st.ucoll.is_none() {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    }
    let a = c.str(0);
    let b = c.str(1);
    let Ok(a) = std::str::from_utf8(&a) else {
        return fail(vm, c, &mut st, U_INVALID_CHAR_FOUND, "Error converting first argument to UTF-16");
    };
    let Ok(b) = std::str::from_utf8(&b) else {
        return fail(vm, c, &mut st, U_INVALID_CHAR_FOUND, "Error converting second argument to UTF-16");
    };
    Value::Long(st.ucoll.as_ref().map(|u| u.strcoll(a, b)).unwrap_or(0))
}

/// collator_get_attribute()
fn get_attribute(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    let Some(u) = st.ucoll.as_ref() else {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    };
    match u.get_attribute(c.long(0)) {
        Ok(v) => Value::Long(v),
        Err(code) => fail(vm, c, &mut st, code, "Error getting attribute value"),
    }
}

/// collator_set_attribute()
fn set_attribute(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    let Some(u) = st.ucoll.as_mut() else {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    };
    match u.set_attribute(c.long(0), c.long(1)) {
        Ok(()) => Value::True,
        Err(code) => fail(vm, c, &mut st, code, "Error setting attribute value"),
    }
}

/// collator_get_strength()
fn get_strength(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    let Some(u) = st.ucoll.as_ref() else {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    };
    Value::Long(u.attrs[STRENGTH as usize])
}

/// collator_set_strength()
fn set_strength(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    let Some(u) = st.ucoll.as_mut() else {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    };
    let _ = u.set_attribute(STRENGTH, c.long(0));
    Value::True
}

/// collator_get_locale()
fn get_locale(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    let Some(u) = st.ucoll.as_ref() else {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    };
    match c.long(0) {
        0 => s(u.actual.clone()),
        1 => s(u.valid.clone()),
        _ => fail(vm, c, &mut st, U_ILLEGAL_ARGUMENT_ERROR, "Error getting locale by type"),
    }
}

/// collator_get_error_code()
fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    let st = state(&c.obj());
    Value::Long(st.borrow().err.code as i64)
}

/// collator_get_error_message()
fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    let st = state(&c.obj());
    s(st.borrow().err.message())
}

/// collator_get_sort_key()
fn get_sort_key(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut st = st.borrow_mut();
    if st.ucoll.is_none() {
        not_initialized(vm, c, &mut st);
        return Value::Null;
    }
    let text = c.str(0);
    let Ok(t) = std::str::from_utf8(&text) else {
        return fail(vm, c, &mut st, U_INVALID_CHAR_FOUND, "Error converting first argument to UTF-16");
    };
    sv(st.ucoll.as_ref().map(|u| u.sort_key(t)).unwrap_or_default())
}

// ---------------------------------------------------------------------------
// collator_sort.c

/// u_isspace()
fn u_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1C}'..='\u{1F}').contains(&c)
}

/// collator_is_numeric(): Some(Long) / Some(Double) for a numeric string.
fn collator_is_numeric(s: &str) -> Option<Value> {
    let ch: Vec<char> = s.chars().collect();
    if ch.is_empty() {
        return None;
    }
    let at = |i: usize| ch.get(i).copied().unwrap_or('\0');
    // collator_u_strtol(base 10)
    let mut i = 0;
    while u_isspace(at(i)) {
        i += 1;
    }
    let mut neg = false;
    if at(i) == '-' {
        neg = true;
        i += 1;
    } else if at(i) == '+' {
        i += 1;
    }
    let mut acc: u64 = 0;
    let mut any = 0i32;
    let cutoff: u64 = if neg { i64::MIN.unsigned_abs() } else { i64::MAX as u64 };
    let cutlim = cutoff % 10;
    let cutoff = cutoff / 10;
    while let Some(d) = at(i).to_digit(10).filter(|_| at(i).is_ascii_digit()) {
        let d = d as u64;
        if any < 0 || acc > cutoff || (acc == cutoff && d > cutlim) {
            any = -1;
        } else {
            any = 1;
            acc = acc * 10 + d;
        }
        i += 1;
    }
    let erange = any < 0;
    let lval = if erange {
        if neg { i64::MIN } else { i64::MAX }
    } else if neg {
        (acc as i64).wrapping_neg()
    } else {
        acc as i64
    };
    let mut end_long = if any != 0 { Some(i) } else { Some(0) };
    if !erange {
        let mut e = end_long.unwrap_or(0);
        while u_isspace(at(e)) {
            e += 1;
        }
        if e == ch.len() {
            return Some(Value::Long(lval));
        }
        if e == 0 && at(0) != '\0' && at(0) != '.' && at(0) != '-' {
            return None;
        }
        end_long = Some(e);
    } else {
        end_long = None;
    }
    let _ = end_long;
    // collator_u_strtod()
    let mut u = 0;
    while u_isspace(at(u)) {
        u += 1;
    }
    let nstart = u;
    if at(u) == '-' || at(u) == '+' {
        u += 1;
    }
    let mut anyd = false;
    while at(u).is_ascii_digit() {
        anyd = true;
        u += 1;
    }
    if at(u) == '.' {
        u += 1;
        while at(u).is_ascii_digit() {
            anyd = true;
            u += 1;
        }
    }
    if (at(u) == 'e' || at(u) == 'E') && anyd {
        let e = u;
        u += 1;
        if at(u) == '-' || at(u) == '+' {
            u += 1;
        }
        let mut any_exp = false;
        while at(u).is_ascii_digit() {
            any_exp = true;
            u += 1;
        }
        if !any_exp {
            u = e;
        }
    }
    if !anyd {
        return None;
    }
    let num: String = ch[nstart..u].iter().collect();
    let dval = goro_core::value::Value::String(goro_core::string::PhpString::from_string(num)).to_double();
    let mut e = u;
    while u_isspace(at(e)) {
        e += 1;
    }
    if e == ch.len() {
        if !dval.is_finite() {
            return None;
        }
        return Some(Value::Double(dval));
    }
    None
}

/// collator_convert_object_to_string(): an object's string cast (None
/// when it has none).
fn object_to_string(vm: &mut Vm, v: &Value) -> Option<Value> {
    if !matches!(v, Value::Object(_)) {
        return None;
    }
    let line = vm.current_line;
    match vm.zpp_coerce_scalar(goro_core::arginfo::T_STRING, v, false, line) {
        Ok(Some(s @ Value::String(_))) => Some(s),
        _ => None,
    }
}

/// collator_normalize_sort_argument()
fn normalize_sort_argument(v: &Value) -> Value {
    if let Value::String(s) = v {
        if let Some(n) = collator_is_numeric(&s.to_string_lossy()) {
            return n;
        }
    }
    v.clone()
}

fn icu_cmp(u: &UCollator, a: &Value, b: &Value) -> i64 {
    let sa = a.to_php_string().to_string_lossy();
    let sb = b.to_php_string().to_string_lossy();
    u.strcoll(&sa, &sb)
}

/// collator_regular_compare_function()
fn regular_compare(vm: &mut Vm, u: &UCollator, a: &Value, b: &Value) -> i64 {
    let s1 = object_to_string(vm, a).unwrap_or_else(|| a.clone());
    let s2 = object_to_string(vm, b).unwrap_or_else(|| b.clone());
    if let (Value::String(x), Value::String(y)) = (&s1, &s2) {
        let n1 = collator_is_numeric(&x.to_string_lossy());
        let n2 = if n1.is_some() {
            collator_is_numeric(&y.to_string_lossy())
        } else {
            None
        };
        match (n1, n2) {
            (Some(n1), Some(n2)) => return n1.compare(&n2),
            _ => return icu_cmp(u, &s1, &s2),
        }
    }
    let n1 = normalize_sort_argument(&s1);
    let n2 = normalize_sort_argument(&s2);
    n1.compare(&n2)
}

/// collator_numeric_compare_function()
fn numeric_compare(a: &Value, b: &Value) -> i64 {
    let conv = |v: &Value| -> f64 {
        match v {
            Value::String(s) => match collator_is_numeric(&s.to_string_lossy()) {
                Some(n) => n.to_double(),
                None => 0.0,
            },
            v => v.to_double(),
        }
    };
    let (x, y) = (conv(a), conv(b));
    if x < y {
        -1
    } else if x > y {
        1
    } else {
        0
    }
}

/// zend_sort() (hybrid insertion sort / quicksort), as zend_hash_sort()
/// runs it.
mod zsort {
    fn sort2<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, cmp: &mut F) {
        if cmp(&v[a], &v[b]) > 0 {
            v.swap(a, b);
        }
    }

    fn sort3<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, c: usize, cmp: &mut F) {
        if cmp(&v[a], &v[b]) <= 0 {
            if cmp(&v[b], &v[c]) <= 0 {
                return;
            }
            v.swap(b, c);
            if cmp(&v[a], &v[b]) > 0 {
                v.swap(a, b);
            }
            return;
        }
        if cmp(&v[c], &v[b]) <= 0 {
            v.swap(a, c);
            return;
        }
        v.swap(a, b);
        if cmp(&v[b], &v[c]) > 0 {
            v.swap(b, c);
        }
    }

    fn sort4<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, c: usize, d: usize, cmp: &mut F) {
        sort3(v, a, b, c, cmp);
        if cmp(&v[c], &v[d]) > 0 {
            v.swap(c, d);
            if cmp(&v[b], &v[c]) > 0 {
                v.swap(b, c);
                if cmp(&v[a], &v[b]) > 0 {
                    v.swap(a, b);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sort5<T, F: FnMut(&T, &T) -> i32>(
        v: &mut [T],
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        e: usize,
        cmp: &mut F,
    ) {
        sort4(v, a, b, c, d, cmp);
        if cmp(&v[d], &v[e]) > 0 {
            v.swap(d, e);
            if cmp(&v[c], &v[d]) > 0 {
                v.swap(c, d);
                if cmp(&v[b], &v[c]) > 0 {
                    v.swap(b, c);
                    if cmp(&v[a], &v[b]) > 0 {
                        v.swap(a, b);
                    }
                }
            }
        }
    }

    fn insert_sort<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], cmp: &mut F) {
        let n = v.len();
        match n {
            0 | 1 => {}
            2 => sort2(v, 0, 1, cmp),
            3 => sort3(v, 0, 1, 2, cmp),
            4 => sort4(v, 0, 1, 2, 3, cmp),
            5 => sort5(v, 0, 1, 2, 3, 4, cmp),
            _ => {
                let sentry = 6.min(n);
                for i in 1..sentry {
                    let mut j = i - 1;
                    if cmp(&v[j], &v[i]) <= 0 {
                        continue;
                    }
                    while j != 0 {
                        j -= 1;
                        if cmp(&v[j], &v[i]) <= 0 {
                            j += 1;
                            break;
                        }
                    }
                    let mut k = i;
                    while k > j {
                        v.swap(k, k - 1);
                        k -= 1;
                    }
                }
                for i in sentry..n {
                    let mut j = i - 1;
                    if cmp(&v[j], &v[i]) <= 0 {
                        continue;
                    }
                    loop {
                        j -= 2;
                        if cmp(&v[j], &v[i]) <= 0 {
                            j += 1;
                            if cmp(&v[j], &v[i]) <= 0 {
                                j += 1;
                            }
                            break;
                        }
                        if j == 0 {
                            break;
                        }
                        if j == 1 {
                            j -= 1;
                            if cmp(&v[i], &v[j]) > 0 {
                                j += 1;
                            }
                            break;
                        }
                    }
                    let mut k = i;
                    while k > j {
                        v.swap(k, k - 1);
                        k -= 1;
                    }
                }
            }
        }
    }

    pub fn zend_sort<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], cmp: &mut F) {
        let mut base = 0usize;
        let mut nmemb = v.len();
        loop {
            if nmemb <= 16 {
                insert_sort(&mut v[base..base + nmemb], cmp);
                return;
            }
            let start = base;
            let end = base + nmemb;
            let offset = nmemb >> 1;
            let pivot0 = start + offset;
            if (nmemb >> 10) != 0 {
                let delta = offset >> 1;
                sort5(v, start, start + delta, pivot0, pivot0 + delta, end - 1, cmp);
            } else {
                sort3(v, start, pivot0, end - 1, cmp);
            }
            v.swap(start + 1, pivot0);
            let pivot = start + 1;
            let mut i = pivot + 1;
            let mut j = end - 1;
            'outer: loop {
                while cmp(&v[pivot], &v[i]) > 0 {
                    i += 1;
                    if i == j {
                        break 'outer;
                    }
                }
                j -= 1;
                if j == i {
                    break 'outer;
                }
                while cmp(&v[j], &v[pivot]) > 0 {
                    j -= 1;
                    if j == i {
                        break 'outer;
                    }
                }
                v.swap(i, j);
                i += 1;
                if i == j {
                    break 'outer;
                }
            }
            v.swap(pivot, i - 1);
            if (i - 1) - start < end - i {
                zend_sort(&mut v[start..i - 1], cmp);
                base = i;
                nmemb = end - i;
            } else {
                zend_sort(&mut v[i..end], cmp);
                base = start;
                nmemb = (i - start) - 1;
            }
        }
    }
}

/// The array of a by-reference array argument.
fn ref_array(c: &Call, i: usize) -> Option<PhpArray> {
    match c.arg(i) {
        Value::Array(a) => Some(a.borrow().clone()),
        _ => None,
    }
}

/// collator_sort_internal()
fn sort_internal(vm: &mut Vm, c: &mut Call, renumber: bool) -> Value {
    let st = fetch(c);
    if st.borrow().ucoll.is_none() {
        crate::zpp::throw(vm, b"Error", "Object not initialized");
        return Value::Null;
    }
    let Some(a) = ref_array(c, 0) else {
        return Value::False;
    };
    let flags = c.long(1);
    // collator_convert_hash_from_utf8_to_utf16()
    for (_, v) in a.iter() {
        if let Value::String(s) = v.deref() {
            if std::str::from_utf8(s.as_bytes()).is_err() {
                let mut stb = st.borrow_mut();
                return fail(vm, c, &mut stb, U_INVALID_CHAR_FOUND, "Error converting hash from UTF-8 to UTF-16");
            }
        }
    }
    let mut items: Vec<(goro_core::array::ArrayKey, Value)> =
        a.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    {
        let stb = st.borrow();
        let u = stb.ucoll.as_ref().expect("collator");
        let mut cmp = |x: &(goro_core::array::ArrayKey, Value), y: &(goro_core::array::ArrayKey, Value)| -> i32 {
            let (x, y) = (x.1.deref(), y.1.deref());
            let r = match flags {
                SORT_NUMERIC => numeric_compare(&x, &y),
                SORT_STRING => icu_cmp(u, &x, &y),
                _ => regular_compare(vm, u, &x, &y),
            };
            r.signum() as i32
        };
        zsort::zend_sort(&mut items, &mut cmp);
    }
    let mut out = PhpArray::new();
    for (k, v) in items {
        if renumber {
            out.push(v);
        } else {
            out.set(k, v);
        }
    }
    c.set_ref(vm, 0, arr(out));
    Value::True
}

/// collator_sort()
fn sort(vm: &mut Vm, c: &mut Call) -> Value {
    sort_internal(vm, c, true)
}

/// collator_asort()
fn asort(vm: &mut Vm, c: &mut Call) -> Value {
    sort_internal(vm, c, false)
}

/// collator_sort_with_sort_keys()
fn sort_with_sort_keys(vm: &mut Vm, c: &mut Call) -> Value {
    let st = fetch(c);
    let mut stb = st.borrow_mut();
    if stb.ucoll.is_none() {
        not_initialized(vm, c, &mut stb);
        return Value::Null;
    }
    let Some(a) = ref_array(c, 0) else {
        return Value::False;
    };
    if a.is_empty() {
        return Value::True;
    }
    let mut keyed: Vec<(Vec<u8>, Value)> = Vec::with_capacity(a.len());
    {
        let u = stb.ucoll.as_ref().expect("collator");
        for (_, v) in a.iter() {
            let key = match v.deref() {
                Value::String(s) => match std::str::from_utf8(s.as_bytes()) {
                    Ok(t) => u.sort_key(t),
                    Err(_) => Vec::new(),
                },
                _ => u.sort_key(""),
            };
            if let Value::String(s) = v.deref() {
                if std::str::from_utf8(s.as_bytes()).is_err() {
                    drop(keyed);
                    return fail(vm, c, &mut stb, U_INVALID_CHAR_FOUND, "Sort with sort keys failed");
                }
            }
            keyed.push((key, v.clone()));
        }
    }
    drop(stb);
    let mut cmp = |x: &(Vec<u8>, Value), y: &(Vec<u8>, Value)| -> i32 {
        match x.0.cmp(&y.0) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    };
    zsort::zend_sort(&mut keyed, &mut cmp);
    let mut out = PhpArray::new();
    for (_, v) in keyed {
        out.push(v);
    }
    c.set_ref(vm, 0, arr(out));
    Value::True
}
