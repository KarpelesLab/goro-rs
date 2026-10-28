//! `MessageFormatter` and the `msgfmt_*()` functions (PHP manual:
//! MessageFormatter).

use goro_ext_intl::error as ue;
use goro_ext_intl::msgfmt::{self as mf, ArgClass, ArgName, MessagePattern, MsgError, MsgValue};
use goro_runtime::{Key, ObjRef, PhpArray, Value};

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
            "function msgfmt_create(string $locale, string $pattern): ?MessageFormatter",
            m_create,
        ),
        (
            "function msgfmt_format(MessageFormatter $formatter, array $values): string|false",
            m_format,
        ),
        (
            "function msgfmt_format_message(string $locale, string $pattern, array $values): string|false",
            m_format_message,
        ),
        (
            "function msgfmt_parse(MessageFormatter $formatter, string $string): array|false",
            m_parse,
        ),
        (
            "function msgfmt_parse_message(string $locale, string $pattern, string $message): array|false",
            m_parse_message,
        ),
        (
            "function msgfmt_set_pattern(MessageFormatter $formatter, string $pattern): bool",
            m_set_pattern,
        ),
        (
            "function msgfmt_get_pattern(MessageFormatter $formatter): string|false",
            m_get_pattern,
        ),
        (
            "function msgfmt_get_locale(MessageFormatter $formatter): string",
            m_get_locale,
        ),
        (
            "function msgfmt_get_error_code(MessageFormatter $formatter): int",
            m_get_error_code,
        ),
        (
            "function msgfmt_get_error_message(MessageFormatter $formatter): string",
            m_get_error_message,
        ),
    ],
    classes: &[("messageformatter", MF_CLASS)],
    methods: &[
        ("messageformatter::__construct", m_construct),
        ("messageformatter::create", m_create),
        ("messageformatter::format", m_format),
        ("messageformatter::formatmessage", m_format_message),
        ("messageformatter::parse", m_parse),
        ("messageformatter::parsemessage", m_parse_message),
        ("messageformatter::setpattern", m_set_pattern),
        ("messageformatter::getpattern", m_get_pattern),
        ("messageformatter::getlocale", m_get_locale),
        ("messageformatter::geterrorcode", m_get_error_code),
        ("messageformatter::geterrormessage", m_get_error_message),
    ],
    hooks: &[("messageformatter", hooks)],
    ..Module::EMPTY
};

const MF_CLASS: &str = r#"<?php class MessageFormatter {
    public function __construct(string $locale, string $pattern) {}
    public static function create(string $locale, string $pattern): ?MessageFormatter {}
    public function format(array $values): string|false {}
    public static function formatMessage(string $locale, string $pattern, array $values): string|false {}
    public function parse(string $string): array|false {}
    public static function parseMessage(string $locale, string $pattern, string $message): array|false {}
    public function setPattern(string $pattern): bool {}
    public function getPattern(): string|false {}
    public function getLocale(): string {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): string {}
}"#;

/// A message formatter: its locale, pattern text and parsed pattern.
#[derive(Clone)]
pub(crate) struct Msg {
    locale: String,
    text: String,
    pat: MessagePattern,
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(super::numfmt::on_create),
        clone: Some(
            |vm, from, to| match with_obj::<Msg, Msg>(from, |x| x.v.clone()) {
                Some(m) => store(to, m),
                None => {
                    super::timezone::clone_error(vm, "Cannot clone uninitialized MessageFormatter")
                }
            },
        ),
        ..Default::default()
    }
}

/// The error of a pattern: code and message text.
fn pattern_error(text: &str, e: &MsgError) -> (i64, String) {
    match e {
        MsgError::Syntax(off) => {
            let units: Vec<u16> = text.encode_utf16().collect();
            let off = (*off).min(units.len());
            let pre_start = off.saturating_sub(15);
            let post_end = (off + 15).min(units.len());
            let pre = String::from_utf16_lossy(&units[pre_start..off]);
            let post = String::from_utf16_lossy(&units[off..post_end]);
            (
                ue::U_PATTERN_SYNTAX_ERROR,
                format!(
                    "pattern syntax error (parse error at offset {off}, after \"{pre}\", before or at \"{post}\")"
                ),
            )
        }
        MsgError::UnmatchedBraces => (
            ue::U_UNMATCHED_BRACES,
            "message formatter creation failed".to_string(),
        ),
        MsgError::IllegalArgument => (
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "message formatter creation failed".to_string(),
        ),
    }
}

fn make(vm: &mut Vm, a: &Args) -> R<Result<Msg, (i64, String)>> {
    let l = a.str(0).to_string_lossy();
    if l.len() > MAX_LOCALE_LEN {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($locale) must be less than or equal to 156 characters",
        );
    }
    let locale = if l.is_empty() {
        super::default_locale(vm)
    } else {
        l
    };
    let Ok(text) = std::str::from_utf8(a.bytes(1)).map(str::to_string) else {
        return Ok(Err((
            ue::U_INVALID_CHAR_FOUND,
            "error converting pattern to UTF-16".to_string(),
        )));
    };
    if text.is_empty() {
        return Ok(Err((
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "message formatter creation failed".to_string(),
        )));
    }
    match MessagePattern::parse(&text) {
        Ok(pat) => Ok(Ok(Msg { locale, text, pat })),
        Err(e) => Ok(Err(pattern_error(&text, &e))),
    }
}

fn m_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Msg>(&o) {
        return already_constructed(vm, "MessageFormatter");
    }
    reset(vm);
    match make(vm, a)? {
        Ok(m) => {
            store(&o, m);
            Ok(Value::Null)
        }
        Err((code, m)) => {
            let msg = format!("{}(): {}", a.fname(), m);
            super::set_error_quiet(vm, code, msg.clone());
            vm.throw("IntlException", &msg)
        }
    }
}

fn m_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    match make(vm, a)? {
        Ok(m) => Ok(Value::Object(new_obj(vm, "MessageFormatter", m)?)),
        Err((code, m)) => {
            fail(vm, a, code, &m)?;
            Ok(Value::Null)
        }
    }
}

fn msg_of(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize, Msg)> {
    let (o, off) = this_or_first(a);
    match with_obj::<Msg, Msg>(&o, |x| x.v.clone()) {
        Some(m) => Ok((o, off, m)),
        None => vm.throw("Error", "Found unconstructed MessageFormatter"),
    }
}

/// The milliseconds of a date/time value (DateTimeInterface, IntlCalendar
/// or a number of seconds).
fn date_ms(vm: &mut Vm, v: &Value) -> R<Option<f64>> {
    Ok(match v {
        Value::Int(i) => Some(*i as f64 * 1000.0),
        Value::Float(f) => Some(f * 1000.0),
        Value::Str(s) if goro_runtime::numeric::is_numeric_str(s.as_bytes()) => {
            Some(goro_runtime::convert::str_to_float(s.as_bytes()) * 1000.0)
        }
        Value::Object(o) => {
            if let Some(dt) = super::super::date::peek_dt(o) {
                let (ts, us) = (dt.ts, dt.us);
                Some(ts as f64 * 1000.0 + (us / 1000) as f64)
            } else if vm.class_of(o).lname.as_bytes() == b"intlcalendar"
                || vm
                    .find_class(b"IntlCalendar", false)?
                    .is_some_and(|c| vm.class_of(o).is_subclass_of(&c))
            {
                match vm.call_method(o, b"gettime", vec![])? {
                    Value::Float(f) => Some(f),
                    Value::Int(i) => Some(i as f64),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    })
}

/// Converts the PHP arguments to message values, with the PHP errors of
/// msgfmt_format().
fn convert_args(
    vm: &mut Vm,
    m: &Msg,
    values: &PhpArray,
) -> R<Result<Vec<(ArgName, MsgValue)>, Vec<u8>>> {
    let types = match m.pat.arg_types() {
        Ok(t) => t,
        Err(_) => return Ok(Err("Inconsistent types declared for an argument".into())),
    };
    let mut out = Vec::new();
    for (k, v) in values.iter() {
        let v = v.deref();
        let name = match k.to_owned() {
            Key::Int(i) => {
                if i < 0 || i > i32::MAX as i64 {
                    return Ok(Err(b"Found negative or too large array key".to_vec()));
                }
                ArgName::Index(i)
            }
            Key::Str(s) => match std::str::from_utf8(s.as_bytes()) {
                Ok(t) => ArgName::Name(t.to_string()),
                Err(_) => {
                    return Ok(Err(quoted(
                        "Invalid UTF-8 data in argument key: ",
                        s.as_bytes(),
                    )));
                }
            },
        };
        let key_text = name.to_string();
        let class = types
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(ArgClass::Any, |(_, c)| *c);
        let val = match class {
            ArgClass::Date => match date_ms(vm, &v)? {
                Some(ms) => MsgValue::Date(ms),
                None => {
                    return Ok(Err(format!(
                        "The argument for key '{key_text}' cannot be used as a date or time"
                    )
                    .into_bytes()));
                }
            },
            ArgClass::Number => match &v {
                Value::Int(i) => MsgValue::Int(*i),
                Value::Float(f) => MsgValue::Double(*f),
                Value::Bool(b) => MsgValue::Int(*b as i64),
                Value::Null => MsgValue::Int(0),
                Value::Str(s) => {
                    let f = goro_runtime::convert::str_to_float(s.as_bytes());
                    if f.fract() == 0.0 && f.abs() < 9.2e18 && !s.as_bytes().contains(&b'.') {
                        MsgValue::Int(f as i64)
                    } else {
                        MsgValue::Double(f)
                    }
                }
                _ => {
                    return Ok(Err(format!(
                        "No strategy to convert the value given for the argument with key '{key_text}' is available"
                    ).into_bytes()));
                }
            },
            ArgClass::String | ArgClass::Any => match &v {
                Value::Str(s) => match std::str::from_utf8(s.as_bytes()) {
                    Ok(t) => MsgValue::Str(t.to_string()),
                    Err(_) => {
                        return Ok(Err(quoted(
                            "Invalid UTF-8 data in string argument: ",
                            s.as_bytes(),
                        )));
                    }
                },
                Value::Int(i) if class == ArgClass::Any => MsgValue::Int(*i),
                Value::Float(f) if class == ArgClass::Any => MsgValue::Double(*f),
                Value::Int(i) => MsgValue::Str(i.to_string()),
                Value::Float(f) => {
                    MsgValue::Str(vm.to_php_str(&Value::Float(*f))?.to_string_lossy())
                }
                Value::Bool(b) => MsgValue::Int(*b as i64),
                Value::Null => MsgValue::Str(String::new()),
                Value::Object(o) => {
                    if let Some(ms) = date_ms(vm, &v)?.filter(|_| class == ArgClass::Any) {
                        MsgValue::Date(ms)
                    } else if vm.class_of(o).magic.borrow().to_string.is_some() {
                        let s = vm.obj_to_string(o)?;
                        match std::str::from_utf8(s.as_bytes()) {
                            Ok(t) => MsgValue::Str(t.to_string()),
                            Err(_) => {
                                return Ok(Err(quoted(
                                    "Invalid UTF-8 data in string argument: ",
                                    s.as_bytes(),
                                )));
                            }
                        }
                    } else {
                        return Ok(Err(format!(
                            "No strategy to convert the value given for the argument with key '{key_text}' is available"
                        ).into_bytes()));
                    }
                }
                _ => {
                    return Ok(Err(format!(
                        "No strategy to convert the value given for the argument with key '{key_text}' is available"
                    ).into_bytes()));
                }
            },
        };
        out.push((name, val));
    }
    Ok(Ok(out))
}

/// `prefix'bytes'`.
fn quoted(prefix: &str, b: &[u8]) -> Vec<u8> {
    let mut v = prefix.as_bytes().to_vec();
    v.push(b'\'');
    v.extend_from_slice(b);
    v.push(b'\'');
    v
}

/// Formats a date/time argument (ICU date styles and patterns).
fn date_hook(vm: &mut Vm, locale: &str) -> impl Fn(&str, Option<&str>, f64) -> String + use<> {
    let tz = super::super::date::default_tz(vm);
    let locale = locale.to_string();
    move |ty: &str, style: Option<&str>, ms: f64| {
        super::datehook::format(&locale, &tz, ty, style, ms)
    }
}

fn do_format(vm: &mut Vm, m: &Msg, values: &PhpArray) -> R<Result<String, Vec<u8>>> {
    let args = match convert_args(vm, m, values)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    let hook = date_hook(vm, &m.locale);
    let lookup = |n: &ArgName| args.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    Ok(Ok(mf::format(&m.locale, &m.pat, &lookup, &hook)))
}

fn m_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, m) = msg_of(vm, a)?;
    reset_obj::<Msg>(vm, &o);
    let values = a.array(off);
    match do_format(vm, &m, &values)? {
        Ok(s) => Ok(Value::from(s.as_str())),
        Err(e) => {
            super::fail_obj_bytes::<Msg>(vm, a, Some(&o), ue::U_ILLEGAL_ARGUMENT_ERROR, &e)?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_format_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let m = match make(vm, a)? {
        Ok(m) => m,
        Err((code, msg)) => {
            if code == ue::U_PATTERN_SYNTAX_ERROR {
                fail(vm, a, code, &msg)?;
            } else {
                // the formatter's error code is lost on this path
                // (ext/intl/tests/msgfmt_format_message_errors.phpt)
                fail(vm, a, ue::U_ZERO_ERROR, "Creating message formatter failed")?;
            }
            return Ok(Value::Bool(false));
        }
    };
    let values = a.array(2);
    match do_format(vm, &m, &values)? {
        Ok(s) => Ok(Value::from(s.as_str())),
        Err(e) => {
            super::fail_obj_bytes::<Msg>(vm, a, None, ue::U_ILLEGAL_ARGUMENT_ERROR, &e)?;
            Ok(Value::Bool(false))
        }
    }
}

fn to_php(vals: Vec<(ArgName, MsgValue)>) -> Value {
    let mut arr = PhpArray::new();
    for (k, v) in vals {
        let key = match k {
            ArgName::Index(i) => Key::Int(i),
            ArgName::Name(n) => Key::from_bytes(n.as_bytes()),
        };
        let val = match v {
            MsgValue::Int(i) => Value::Int(i),
            MsgValue::Double(d) => Value::Float(d),
            MsgValue::Str(s) => Value::from(s.as_str()),
            MsgValue::Date(d) => Value::Float(d / 1000.0),
        };
        arr.insert(key, val);
    }
    Value::Array(arr)
}

fn m_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, m) = msg_of(vm, a)?;
    reset_obj::<Msg>(vm, &o);
    let text = String::from_utf8_lossy(a.bytes(off)).into_owned();
    match mf::parse(
        &m.locale,
        &m.pat,
        &text,
        &super::super::date::default_tz(vm).name(),
    ) {
        Some(v) => Ok(to_php(v)),
        None => {
            fail_obj::<Msg>(vm, a, &o, ue::U_PARSE_ERROR, "Parsing failed")?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_parse_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let m = match make(vm, a)? {
        Ok(m) => m,
        Err((code, msg)) => {
            if code == ue::U_PATTERN_SYNTAX_ERROR {
                fail(vm, a, code, &msg)?;
            } else {
                fail(vm, a, code, "Creating message formatter failed")?;
            }
            return Ok(Value::Bool(false));
        }
    };
    let text = String::from_utf8_lossy(a.bytes(2)).into_owned();
    match mf::parse(
        &m.locale,
        &m.pat,
        &text,
        &super::super::date::default_tz(vm).name(),
    ) {
        Some(v) => Ok(to_php(v)),
        None => {
            fail(vm, a, ue::U_PARSE_ERROR, "Parsing failed")?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_set_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = msg_of(vm, a)?;
    reset_obj::<Msg>(vm, &o);
    let Ok(text) = std::str::from_utf8(a.bytes(off)).map(str::to_string) else {
        fail_obj::<Msg>(
            vm,
            a,
            &o,
            ue::U_INVALID_CHAR_FOUND,
            "Error converting pattern to UTF-16",
        )?;
        return Ok(Value::Bool(false));
    };
    match MessagePattern::parse(&text) {
        Ok(pat) => {
            with_obj::<Msg, ()>(&o, |x| {
                x.v.text = text;
                x.v.pat = pat;
            });
            Ok(Value::Bool(true))
        }
        Err(e) => {
            let (code, off16) = match e {
                MsgError::Syntax(off) => (ue::U_PATTERN_SYNTAX_ERROR, off),
                MsgError::UnmatchedBraces => (ue::U_UNMATCHED_BRACES, 0),
                MsgError::IllegalArgument => (ue::U_ILLEGAL_ARGUMENT_ERROR, 0),
            };
            fail_obj::<Msg>(
                vm,
                a,
                &o,
                code,
                &format!("Error setting symbol value at line 0, offset {off16}"),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_get_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, m) = msg_of(vm, a)?;
    reset_obj::<Msg>(vm, &o);
    Ok(Value::from(m.text.as_str()))
}

fn m_get_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, m) = msg_of(vm, a)?;
    reset_obj::<Msg>(vm, &o);
    Ok(Value::from(m.locale.as_str()))
}

fn m_get_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let _ = vm;
    Ok(Value::Int(obj_err::<Msg>(&o).code))
}

fn m_get_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let _ = vm;
    Ok(Value::from(obj_err::<Msg>(&o).full_message().as_str()))
}
