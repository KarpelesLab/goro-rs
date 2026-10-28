//! The filter functions: options, scalar conversion, failure handling,
//! recursion into arrays.
//!
//! Observed on PHP 8.5 (the fixture's semantic section, tests
//! ext/filter/tests/010.phpt, 038.phpt, 039.phpt, 052.phpt, 054.phpt,
//! filter_var_array_with_ref.phpt, throw-on-failure/*.phpt):
//!
//! - Without FILTER_REQUIRE_ARRAY or FILTER_FORCE_ARRAY every filter but
//!   FILTER_CALLBACK requires a scalar. An array is then a failure; with
//!   one of those flags a scalar is a failure (FORCE_ARRAY wraps it
//!   instead) and arrays are filtered element by element, recursively.
//!   These type failures give false/null (or throw) but never the
//!   `default` option.
//! - A scalar is converted to a string first (null and false are "", floats
//!   use ini precision, resources "Resource id #n", objects need
//!   `__toString()`), then filtered. A failing validator gives the
//!   `default` option if there is one, else null with
//!   FILTER_NULL_ON_FAILURE, else false; FILTER_THROW_ON_FAILURE throws
//!   instead and wins over `default`.
//! - Option errors (callback, regexp, decimal, thousand, separator) are
//!   raised when a scalar reaches the filter, so an empty array or an
//!   object without `__toString()` never raises them.
//! - Elements that are references are filtered in place: the referenced
//!   value changes too, and a reference cycle is visited once.

use goro_runtime::convert::{to_float, to_int, to_str};
use goro_runtime::{ErrorKind, Key, Level, PhpArray, PhpError, PhpStr, Value};

use crate::consts::*;
use crate::{Filter, FilterHost, Func, InputType, LIST, Options, c_str, sanitize, url, validate};

/// What a filter did with a string.
pub(crate) enum Outcome {
    Value(Value),
    /// The value failed validation.
    Fail,
    /// FILTER_VALIDATE_BOOL's "false" words without FILTER_NULL_ON_FAILURE:
    /// false, or the `default` option, but never an exception (observed on
    /// PHP 8.5: `filter_var("no", FILTER_VALIDATE_BOOL, ["options" =>
    /// ["default" => "D"], "flags" => FILTER_THROW_ON_FAILURE])` is "D").
    FalseWord,
}

/// The entries of an options array that the filters read, looked up in one
/// pass. They may be references (read with `deref()` or the conversions,
/// which look through them).
#[derive(Default, Clone, Copy)]
pub(crate) struct OptView<'a> {
    pub default: Option<&'a Value>,
    pub min_range: Option<&'a Value>,
    pub max_range: Option<&'a Value>,
    pub decimal: Option<&'a Value>,
    pub thousand: Option<&'a Value>,
    pub regexp: Option<&'a Value>,
    pub separator: Option<&'a Value>,
    pub uri_parser_class: Option<&'a Value>,
}

impl<'a> OptView<'a> {
    fn scan(a: &'a PhpArray) -> OptView<'a> {
        let mut v = OptView::default();
        for (k, val) in a.iter() {
            let goro_runtime::KeyRef::Str(k) = k else {
                continue;
            };
            let slot = match &k[..] {
                b"default" => &mut v.default,
                b"min_range" => &mut v.min_range,
                b"max_range" => &mut v.max_range,
                b"decimal" => &mut v.decimal,
                b"thousand" => &mut v.thousand,
                b"regexp" => &mut v.regexp,
                b"separator" => &mut v.separator,
                b"uri_parser_class" => &mut v.uri_parser_class,
                _ => continue,
            };
            *slot = Some(val);
        }
        v
    }
}

/// A filter with its flags and options, as given for one value.
pub(crate) struct Spec<'a> {
    pub filter: Filter,
    pub flags: i64,
    /// The "options" entry (an array, or FILTER_CALLBACK's callable), not a
    /// reference.
    pub options: Option<&'a Value>,
    pub opts: OptView<'a>,
}

/// An "options" entry with a reference resolved into `holder`.
fn resolve<'a>(v: Option<&'a Value>, holder: &'a mut Value) -> Option<&'a Value> {
    match v {
        Some(Value::Ref(r)) => {
            *holder = r.get();
            Some(&*holder)
        }
        other => other,
    }
}

/// The entry with string key `key` (a non-numeric name, so no key
/// normalisation applies). Options arrays are small: a scan is cheaper than
/// hashing the name.
#[inline]
pub(crate) fn find<'a>(a: &'a PhpArray, key: &[u8]) -> Option<&'a Value> {
    if a.len() > 8 {
        return a.get_bytes(key);
    }
    a.iter()
        .find(|(k, _)| matches!(k, goro_runtime::KeyRef::Str(s) if s[..] == *key))
        .map(|(_, v)| v)
}

fn value_error<H: FilterHost>(host: &mut H, func: Func, msg: &str) -> H::Exception {
    host.throw_error(PhpError::new(
        ErrorKind::ValueError,
        format!("{}(): {msg}", func.name()),
    ))
}

fn unknown_filter<H: FilterHost>(host: &mut H, func: Func, id: i64) {
    host.notice(
        Level::Warning,
        format!("{}(): Unknown filter with ID {id}", func.name()),
    );
}

/// Builds the spec from an `$options` value (int flags or an array with
/// "flags" and "options"), checking the flag combination.
fn make_spec<'a, H: FilterHost>(
    host: &mut H,
    func: Func,
    filter: Filter,
    flags: i64,
    options: Option<&'a Value>,
) -> Result<Spec<'a>, H::Exception> {
    if flags & FILTER_NULL_ON_FAILURE != 0 && flags & FILTER_THROW_ON_FAILURE != 0 {
        return Err(value_error(
            host,
            func,
            &format!(
                "Argument #{} ($options) cannot use both FILTER_NULL_ON_FAILURE and FILTER_THROW_ON_FAILURE",
                func.options_arg()
            ),
        ));
    }
    let opts = match options {
        Some(Value::Array(a)) if filter != Filter::Callback => OptView::scan(a),
        _ => OptView::default(),
    };
    let mut flags = flags;
    if filter != Filter::Callback && flags & (FILTER_REQUIRE_ARRAY | FILTER_FORCE_ARRAY) == 0 {
        flags |= FILTER_REQUIRE_SCALAR;
    }
    Ok(Spec {
        filter,
        flags,
        options,
        opts,
    })
}

/// Flags and options of an options array.
fn split_options<'a, H: FilterHost>(host: &mut H, a: &'a PhpArray) -> (i64, Option<&'a Value>) {
    let mut flags = None;
    let mut options = None;
    for (k, v) in a.iter() {
        match k {
            goro_runtime::KeyRef::Str(k) if &k[..] == b"flags" => flags = Some(v),
            goro_runtime::KeyRef::Str(k) if &k[..] == b"options" => options = Some(v),
            _ => {}
        }
    }
    let flags = match flags {
        Some(v) => to_int(v, host),
        None => 0,
    };
    (flags, options)
}

fn spec_from_options<'a, H: FilterHost>(
    host: &mut H,
    func: Func,
    filter: Filter,
    options: Options<'a>,
    holder: &'a mut Value,
) -> Result<Spec<'a>, H::Exception> {
    let (flags, opts) = match options {
        Options::Int(f) => (f, None),
        Options::Array(a) => split_options(host, a),
    };
    make_spec(host, func, filter, flags, resolve(opts, holder))
}

/// "not an array (got ...)": the value's type as PHP 8.5 names it there.
fn type_desc(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Int(_) => "int".into(),
        Value::Float(_) => "float".into(),
        Value::Str(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => o.class_name().to_string_lossy(),
        Value::Resource(_) => "resource".into(),
        Value::Ref(r) => type_desc(&r.get()),
    }
}

fn failed_msg(rest: &[u8]) -> PhpStr {
    let mut m = b"filter validation failed: ".to_vec();
    m.extend_from_slice(rest);
    PhpStr::new(&m)
}

/// A type failure (REQUIRE_SCALAR / REQUIRE_ARRAY): no `default`.
fn type_fail<H: FilterHost>(host: &mut H, spec: &Spec, msg: String) -> Result<Value, H::Exception> {
    if spec.flags & FILTER_THROW_ON_FAILURE != 0 {
        return Err(host.filter_failed(failed_msg(msg.as_bytes())));
    }
    Ok(if spec.flags & FILTER_NULL_ON_FAILURE != 0 {
        Value::Null
    } else {
        Value::Bool(false)
    })
}

/// A filter failure: the exception, `default`, null or false.
fn fail<H: FilterHost>(
    host: &mut H,
    spec: &Spec,
    msg: impl FnOnce() -> PhpStr,
) -> Result<Value, H::Exception> {
    if spec.flags & FILTER_THROW_ON_FAILURE != 0 {
        return Err(host.filter_failed(msg()));
    }
    if let Some(d) = spec.opts.default {
        return Ok(d.deref());
    }
    Ok(if spec.flags & FILTER_NULL_ON_FAILURE != 0 {
        Value::Null
    } else {
        Value::Bool(false)
    })
}

/// Filters a value that is not a reference: arrays recursively, scalars
/// after the type checks.
pub(crate) fn apply<H: FilterHost>(
    host: &mut H,
    func: Func,
    spec: &Spec,
    v: &Value,
) -> Result<Value, H::Exception> {
    match v {
        Value::Ref(r) => apply(host, func, spec, &r.get()),
        Value::Array(a) => {
            if spec.flags & FILTER_REQUIRE_SCALAR != 0 {
                return type_fail(host, spec, "not a scalar value (got an array)".into());
            }
            let mut a = a.clone();
            let mut seen = Vec::new();
            filter_array(host, func, spec, &mut a, &mut seen)?;
            Ok(Value::Array(a))
        }
        _ => {
            if spec.flags & FILTER_REQUIRE_ARRAY != 0 {
                return type_fail(host, spec, format!("not an array (got {})", type_desc(v)));
            }
            let r = filter_scalar(host, func, spec, v)?;
            if spec.flags & FILTER_FORCE_ARRAY != 0 {
                Ok(Value::Array(PhpArray::from_values([r])))
            } else {
                Ok(r)
            }
        }
    }
}

/// Filters every element of `a` in place (nested arrays recursively,
/// referenced values through the reference).
fn filter_array<H: FilterHost>(
    host: &mut H,
    func: Func,
    spec: &Spec,
    a: &mut PhpArray,
    seen: &mut Vec<usize>,
) -> Result<(), H::Exception> {
    if a.is_empty() {
        return Ok(());
    }
    let d = a.make_mut();
    let mut p = 0;
    while let Some((pos, _, _)) = d.next_from(p) {
        p = pos + 1;
        let elem = d.value_at_mut(pos).expect("valid position");
        match elem {
            Value::Ref(r) => {
                let r = r.clone();
                if seen.contains(&r.addr()) {
                    continue;
                }
                match r.get() {
                    Value::Array(mut inner) => {
                        seen.push(r.addr());
                        let res = filter_array(host, func, spec, &mut inner, seen);
                        seen.pop();
                        res?;
                        r.set(Value::Array(inner));
                    }
                    other => {
                        let nv = filter_scalar(host, func, spec, &other)?;
                        r.set(nv);
                    }
                }
            }
            Value::Array(inner) => {
                let mut inner = std::mem::take(inner);
                filter_array(host, func, spec, &mut inner, seen)?;
                *d.value_at_mut(pos).expect("valid position") = Value::Array(inner);
            }
            other => {
                let nv = filter_scalar(host, func, spec, other)?;
                *d.value_at_mut(pos).expect("valid position") = nv;
            }
        }
    }
    Ok(())
}

/// Converts a scalar to its string and runs the filter.
fn filter_scalar<H: FilterHost>(
    host: &mut H,
    func: Func,
    spec: &Spec,
    v: &Value,
) -> Result<Value, H::Exception> {
    let s = match v {
        Value::Str(s) => s.clone(),
        Value::Object(o) => match host.object_string(o)? {
            Some(s) => s,
            None => {
                return fail(host, spec, || {
                    let msg = format!(
                        "object of type {} has no __toString() method",
                        o.class_name().to_string_lossy()
                    );
                    failed_msg(msg.as_bytes())
                });
            }
        },
        Value::Ref(r) => return filter_scalar(host, func, spec, &r.get()),
        other => match to_str(other, host) {
            Ok(s) => s,
            Err(e) => return Err(host.throw_error(e)),
        },
    };
    if spec.filter == Filter::Callback {
        let cb = match spec.options.map(Value::deref) {
            Some(cb) if host.is_callable(&cb) => cb,
            _ => {
                return Err(host.throw_error(PhpError::type_error(format!(
                    "{}(): Option must be a valid callback",
                    func.name()
                ))));
            }
        };
        return host.call(&cb, Value::Str(s));
    }
    match run_filter(host, func, spec, &s)? {
        Outcome::Value(v) => Ok(v),
        Outcome::FalseWord => Ok(spec.opts.default.map_or(Value::Bool(false), Value::deref)),
        Outcome::Fail => fail(host, spec, || {
            let mut m = b"filter ".to_vec();
            m.extend_from_slice(spec.filter.name().as_bytes());
            m.extend_from_slice(b" not satisfied by '");
            m.extend_from_slice(c_str(&s));
            m.push(b'\'');
            failed_msg(&m)
        }),
    }
}

/// A string option that must be one byte long ("decimal", "separator").
fn one_char<H: FilterHost>(
    host: &mut H,
    func: Func,
    value: Option<&Value>,
    name: &str,
) -> Result<Option<u8>, H::Exception> {
    match value.map(Value::deref) {
        Some(Value::Str(s)) => {
            if s.len() != 1 {
                Err(value_error(
                    host,
                    func,
                    &format!("\"{name}\" option must be one character long"),
                ))
            } else {
                Ok(Some(s[0]))
            }
        }
        _ => Ok(None),
    }
}

/// Runs one filter on a string.
fn run_filter<H: FilterHost>(
    host: &mut H,
    func: Func,
    spec: &Spec,
    s: &PhpStr,
) -> Result<Outcome, H::Exception> {
    let flags = spec.flags;
    let ok = |b: bool| {
        if b {
            Outcome::Value(Value::Str(s.clone()))
        } else {
            Outcome::Fail
        }
    };
    Ok(match spec.filter {
        Filter::Int => {
            let min = match spec.opts.min_range {
                Some(v) => to_int(v, host),
                None => i64::MIN,
            };
            let max = match spec.opts.max_range {
                Some(v) => to_int(v, host),
                None => i64::MAX,
            };
            match validate::int(s, flags) {
                Some(i) if i >= min && i <= max => Outcome::Value(Value::Int(i)),
                _ => Outcome::Fail,
            }
        }
        Filter::Float => {
            let dec = one_char(host, func, spec.opts.decimal, "decimal")?.unwrap_or(b'.');
            let thousand = match spec.opts.thousand.map(Value::deref) {
                Some(Value::Str(t)) => {
                    if t.is_empty() {
                        return Err(value_error(
                            host,
                            func,
                            "\"thousand\" option must not be empty",
                        ));
                    }
                    Some(t)
                }
                _ => None,
            };
            let min = spec.opts.min_range.map(|v| to_float(v, host));
            let max = spec.opts.max_range.map(|v| to_float(v, host));
            let thousand: &[u8] = match &thousand {
                Some(t) => t,
                None => b"',.",
            };
            match validate::float(s, flags, dec, thousand) {
                Some(f) if !min.is_some_and(|m| f < m) && !max.is_some_and(|m| f > m) => {
                    Outcome::Value(Value::Float(f))
                }
                _ => Outcome::Fail,
            }
        }
        Filter::Bool => match validate::bool(s) {
            Some(true) => Outcome::Value(Value::Bool(true)),
            Some(false) if flags & FILTER_NULL_ON_FAILURE != 0 => {
                Outcome::Value(Value::Bool(false))
            }
            Some(false) => Outcome::FalseWord,
            None => Outcome::Fail,
        },
        Filter::Regexp => {
            let Some(Value::Str(re)) = spec.opts.regexp.map(Value::deref) else {
                return Err(value_error(host, func, "\"regexp\" option is missing"));
            };
            match validate::regexp(host, &re, s) {
                Ok(b) => ok(b),
                Err(msg) => {
                    host.notice(Level::Warning, format!("{}(): {msg}", func.name()));
                    Outcome::Fail
                }
            }
        }
        Filter::Domain => ok(validate::valid_domain(s, flags & FILTER_FLAG_HOSTNAME != 0)),
        Filter::Url => {
            let parser = match spec.opts.uri_parser_class.map(Value::deref) {
                Some(Value::Str(c)) => match &c[..] {
                    b"Uri\\Rfc3986\\Uri" => Some(crate::UriParser::Rfc3986),
                    b"Uri\\WhatWg\\Url" => Some(crate::UriParser::WhatWg),
                    _ => {
                        return Err(value_error(
                            host,
                            func,
                            "\"uri_parser_class\" option has invalid value",
                        ));
                    }
                },
                _ => None,
            };
            ok(url::validate(host, s, flags, parser))
        }
        Filter::Email => ok(crate::email::valid_email(
            s,
            flags & FILTER_FLAG_EMAIL_UNICODE != 0,
        )),
        Filter::Ip => ok(crate::ip::validate(s, flags)),
        Filter::Mac => {
            let sep = one_char(host, func, spec.opts.separator, "separator")?;
            ok(validate::mac(s, sep))
        }
        Filter::Callback => unreachable!("handled by the caller"),
        _ => sanitize::run(host, spec.filter, flags, s),
    })
}

// ---------------------------------------------------------------- functions

/// `filter_list(): array`.
pub fn filter_list() -> PhpArray {
    PhpArray::from_values(LIST.iter().map(|(n, _)| Value::Str(PhpStr::from(*n))))
}

/// `filter_id(string $name): int|false`: names compare exactly, up to a NUL
/// byte (observed on PHP 8.5: "int\0x" is 257, "INT" is false).
pub fn filter_id(name: &[u8]) -> Option<i64> {
    let name = c_str(name);
    LIST.iter()
        .find(|(n, _)| n.as_bytes() == name)
        .map(|(_, i)| *i)
}

/// `filter_var(mixed $value, int $filter = FILTER_DEFAULT, array|int
/// $options = 0): mixed`.
pub fn filter_var<H: FilterHost>(
    host: &mut H,
    value: &Value,
    filter: i64,
    options: Options<'_>,
) -> Result<Value, H::Exception> {
    let func = Func::Var;
    let Some(f) = Filter::from_id(filter) else {
        unknown_filter(host, func, filter);
        return Ok(Value::Bool(false));
    };
    if let (Options::Int(flags), Value::Str(s)) = (options, value)
        && flags
            & (FILTER_REQUIRE_ARRAY
                | FILTER_FORCE_ARRAY
                | FILTER_NULL_ON_FAILURE
                | FILTER_THROW_ON_FAILURE)
            == 0
        && let Some(v) = fast_path(host, f, flags, s)
    {
        return Ok(v);
    }
    let mut holder = Value::Null;
    let spec = spec_from_options(host, func, f, options, &mut holder)?;
    apply(host, func, &spec, value)
}

/// A string filtered with plain flags: no options, so no `default`, and
/// failure is false (the general path gives the same results).
#[inline]
fn fast_path<H: FilterHost>(host: &mut H, f: Filter, flags: i64, s: &PhpStr) -> Option<Value> {
    let ok = |b: bool| {
        if b {
            Value::Str(s.clone())
        } else {
            Value::Bool(false)
        }
    };
    Some(match f {
        Filter::Int => validate::int(s, flags).map_or(Value::Bool(false), Value::Int),
        Filter::Bool => Value::Bool(validate::bool(s) == Some(true)),
        Filter::Float => {
            validate::float(s, flags, b'.', b"',.").map_or(Value::Bool(false), Value::Float)
        }
        Filter::Ip => ok(crate::ip::validate(s, flags)),
        Filter::Email => ok(crate::email::valid_email(
            s,
            flags & FILTER_FLAG_EMAIL_UNICODE != 0,
        )),
        Filter::Domain => ok(validate::valid_domain(s, flags & FILTER_FLAG_HOSTNAME != 0)),
        Filter::Mac => ok(validate::mac(s, None)),
        Filter::Url => ok(url::validate(host, s, flags, None)),
        Filter::Regexp | Filter::Callback => return None,
        _ => match sanitize::run(host, f, flags, s) {
            Outcome::Value(v) => v,
            _ => return None,
        },
    })
}

/// The type check of the `INPUT_*` argument.
fn input_type<H: FilterHost>(
    host: &mut H,
    func: &str,
    arg: &str,
    ty: i64,
) -> Result<InputType, H::Exception> {
    InputType::from_i64(ty).ok_or_else(|| {
        host.throw_error(PhpError::new(
            ErrorKind::ValueError,
            format!("{func}(): Argument #1 (${arg}) must be an INPUT_* constant"),
        ))
    })
}

fn lookup(a: &PhpArray, name: &[u8]) -> Option<Value> {
    match Key::from_bytes(name) {
        Key::Int(i) => a.get_int(i),
        Key::Str(s) => a.get_str(&s),
    }
    .map(|v| v.deref())
}

/// `filter_has_var(int $input_type, string $var_name): bool`.
pub fn filter_has_var<H: FilterHost>(
    host: &mut H,
    ty: i64,
    name: &[u8],
) -> Result<bool, H::Exception> {
    let ty = input_type(host, "filter_has_var", "input_type", ty)?;
    Ok(host.input(ty).is_some_and(|a| lookup(&a, name).is_some()))
}

/// `filter_input(int $type, string $var_name, int $filter =
/// FILTER_DEFAULT, array|int $options = 0): mixed`.
///
/// Observed on PHP 8.5: an unknown filter is reported before the type is
/// checked; a missing variable gives the `default` option, else false with
/// FILTER_NULL_ON_FAILURE and null without (test 012.phpt), or throws "input
/// value '...' not found" (test throw-on-failure/filter_input_failure.phpt).
pub fn filter_input<H: FilterHost>(
    host: &mut H,
    ty: i64,
    name: &[u8],
    filter: i64,
    options: Options<'_>,
) -> Result<Value, H::Exception> {
    let func = Func::Input;
    let Some(f) = Filter::from_id(filter) else {
        unknown_filter(host, func, filter);
        return Ok(Value::Bool(false));
    };
    let ty = input_type(host, func.name(), "type", ty)?;
    let mut holder = Value::Null;
    let spec = spec_from_options(host, func, f, options, &mut holder)?;
    match host.input(ty).and_then(|a| lookup(&a, name)) {
        Some(v) => apply(host, func, &spec, &v),
        None => {
            if spec.flags & FILTER_THROW_ON_FAILURE != 0 {
                let mut m = b"input value '".to_vec();
                m.extend_from_slice(c_str(name));
                m.extend_from_slice(b"' not found");
                return Err(host.filter_failed(PhpStr::new(&m)));
            }
            if let Some(d) = spec.opts.default {
                return Ok(d.deref());
            }
            Ok(if spec.flags & FILTER_NULL_ON_FAILURE != 0 {
                Value::Bool(false)
            } else {
                Value::Null
            })
        }
    }
}

/// `filter_var_array(array $array, array|int $options = FILTER_DEFAULT,
/// bool $add_empty = true): array|false|null`.
pub fn filter_var_array<H: FilterHost>(
    host: &mut H,
    array: &PhpArray,
    options: Options<'_>,
    add_empty: bool,
) -> Result<Value, H::Exception> {
    let func = Func::VarArray;
    if let Options::Int(id) = options
        && Filter::from_id(id).is_none()
    {
        unknown_filter(host, func, id);
        return Ok(Value::Bool(false));
    }
    var_array(host, func, array, options, add_empty)
}

/// `filter_input_array(int $type, array|int $options = FILTER_DEFAULT, bool
/// $add_empty = true): array|false|null`: null when the SAPI registered no
/// array for the source (before the options are looked at; an unknown int
/// filter is reported first, observed on PHP 8.5).
pub fn filter_input_array<H: FilterHost>(
    host: &mut H,
    ty: i64,
    options: Options<'_>,
    add_empty: bool,
) -> Result<Value, H::Exception> {
    let func = Func::InputArray;
    if let Options::Int(id) = options
        && Filter::from_id(id).is_none()
    {
        unknown_filter(host, func, id);
        return Ok(Value::Bool(false));
    }
    let ty = input_type(host, func.name(), "type", ty)?;
    let Some(array) = host.input(ty) else {
        return Ok(Value::Null);
    };
    var_array(host, func, &array, options, add_empty)
}

/// The common part of the `_array` functions.
///
/// Observed on PHP 8.5 (tests 032.phpt, 039.phpt, 054.phpt, gh16993.phpt):
/// an int filters the whole array element by element. In an options array
/// every key must be a non-empty string (TypeError/ValueError raised in
/// order while walking it); a missing variable is null with `$add_empty`
/// (no option is looked at) and skipped without. A spec that is not an
/// array is converted to an int filter id, an unknown one warns and falls
/// back to FILTER_DEFAULT; in an array spec an unknown "filter" silently
/// falls back.
fn var_array<H: FilterHost>(
    host: &mut H,
    func: Func,
    array: &PhpArray,
    options: Options<'_>,
    add_empty: bool,
) -> Result<Value, H::Exception> {
    let specs = match options {
        Options::Int(id) => {
            let f = Filter::from_id(id).expect("checked by the caller");
            let spec = make_spec(host, func, f, FILTER_REQUIRE_ARRAY, None)?;
            return apply(host, func, &spec, &Value::Array(array.clone()));
        }
        Options::Array(a) => a,
    };
    let mut out = PhpArray::with_capacity(specs.len());
    for (k, spec_v) in specs.iter() {
        let key = match k {
            goro_runtime::KeyRef::Int(_) => {
                return Err(host.throw_error(PhpError::type_error(format!(
                    "{}(): Argument #2 ($options) must contain only string keys",
                    func.name()
                ))));
            }
            goro_runtime::KeyRef::Str(s) => s.clone(),
        };
        if key.is_empty() {
            return Err(value_error(
                host,
                func,
                "Argument #2 ($options) cannot contain empty keys",
            ));
        }
        let Some(v) = array.get_str(&key).map(|v| v.deref()) else {
            if add_empty {
                out.insert(Key::Str(key), Value::Null);
            }
            continue;
        };
        let spec_v = spec_v.deref();
        let mut holder = Value::Null;
        let spec = match &spec_v {
            Value::Array(sa) => {
                let f = match find(sa, b"filter") {
                    Some(fv) => Filter::from_id(to_int(fv, host)),
                    None => None,
                }
                .unwrap_or(Filter::UnsafeRaw);
                let (flags, opts) = split_options(host, sa);
                make_spec(host, func, f, flags, resolve(opts, &mut holder))?
            }
            other => {
                let id = to_int(other, host);
                let f = match Filter::from_id(id) {
                    Some(f) => f,
                    None => {
                        unknown_filter(host, func, id);
                        if matches!(other, Value::Object(_)) {
                            // The id is read a second time (observed: the
                            // object's conversion warning comes again).
                            to_int(other, host);
                        }
                        Filter::UnsafeRaw
                    }
                };
                make_spec(host, func, f, 0, None)?
            }
        };
        let r = apply(host, func, &spec, &v)?;
        out.insert(Key::Str(key), r);
    }
    Ok(Value::Array(out))
}
