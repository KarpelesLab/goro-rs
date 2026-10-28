//! getopt(), escapeshellarg(), escapeshellcmd(), assert_options() and
//! the assertion failure handling of assert() (PHP manual pages of these
//! functions; behaviours observed on PHP 8.5 and in tests
//! ext/standard/tests/general_functions/getopt_*.phpt and
//! ext/standard/tests/assert/*.phpt).

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::string::sv;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function getopt(string $short_options, array $long_options = [], &$rest_index = null): array|false",
            getopt,
        ),
        (
            "function escapeshellarg(string $arg): string",
            escapeshellarg,
        ),
        (
            "function escapeshellcmd(string $command): string",
            escapeshellcmd,
        ),
        (
            "function assert_options(int $option, mixed $value = UNKNOWN): mixed",
            assert_options,
        ),
        (
            "function version_compare(string $version1, string $version2, ?string $operator = null): int|bool",
            version_compare,
        ),
    ],
    constants: &[
        ("CONNECTION_ABORTED", ConstVal::Int(1)),
        ("CONNECTION_NORMAL", ConstVal::Int(0)),
        ("CONNECTION_TIMEOUT", ConstVal::Int(2)),
        ("ASSERT_ACTIVE", ConstVal::Int(1)),
        ("ASSERT_CALLBACK", ConstVal::Int(2)),
        ("ASSERT_BAIL", ConstVal::Int(3)),
        ("ASSERT_WARNING", ConstVal::Int(4)),
        ("ASSERT_EXCEPTION", ConstVal::Int(5)),
    ],
    listed: false,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- getopt

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arity {
    None,
    Required,
    Optional,
}

fn parse_spec(s: &[u8]) -> (Vec<u8>, Arity) {
    if let Some(n) = s.strip_suffix(b"::") {
        (n.to_vec(), Arity::Optional)
    } else if let Some(n) = s.strip_suffix(b":") {
        (n.to_vec(), Arity::Required)
    } else {
        (s.to_vec(), Arity::None)
    }
}

fn add_opt(out: &mut PhpArray, name: &[u8], v: Value) {
    let k = Key::from_bytes(name);
    match out.get(k.as_ref()).cloned() {
        None => {
            out.insert(k, v);
        }
        Some(Value::Array(mut list)) => {
            let _ = list.push(v);
            out.insert(k, Value::Array(list));
        }
        Some(old) => {
            out.insert(k, Value::Array(PhpArray::from_values([old, v])));
        }
    }
}

fn getopt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Short options: a letter followed by ':' (required) or '::'.
    let spec = a.str(0);
    let mut short: Vec<(u8, Arity)> = Vec::new();
    let mut i = 0;
    while i < spec.len() {
        let c = spec[i];
        i += 1;
        let mut ar = Arity::None;
        if spec.get(i) == Some(&b':') {
            i += 1;
            ar = Arity::Required;
            if spec.get(i) == Some(&b':') {
                i += 1;
                ar = Arity::Optional;
            }
        }
        short.push((c, ar));
    }
    let mut long: Vec<(Vec<u8>, Arity)> = Vec::new();
    for (_, v) in a.array(1).iter() {
        let s = vm.to_php_str(&v.deref())?;
        long.push(parse_spec(&s));
    }
    let argv: Vec<PhpStr> = vm.argv.clone();
    let mut out = PhpArray::new();
    let mut idx = 1;
    while idx < argv.len() {
        let arg = argv[idx].as_bytes();
        if arg == b"--" {
            idx += 1;
            break;
        }
        if arg.len() < 2 || arg[0] != b'-' {
            break;
        }
        if let Some(body) = arg.strip_prefix(b"--") {
            let (name, inline) = match body.iter().position(|&c| c == b'=') {
                Some(p) => (&body[..p], Some(&body[p + 1..])),
                None => (body, None),
            };
            if let Some((_, ar)) = long.iter().find(|(n, _)| n.as_slice() == name) {
                match ar {
                    Arity::None => add_opt(&mut out, name, Value::Bool(false)),
                    Arity::Optional => add_opt(
                        &mut out,
                        name,
                        inline.map_or(Value::Bool(false), |v| sv(v.to_vec())),
                    ),
                    Arity::Required => {
                        if let Some(v) = inline {
                            add_opt(&mut out, name, sv(v.to_vec()));
                        } else if idx + 1 < argv.len() {
                            idx += 1;
                            add_opt(&mut out, name, Value::Str(argv[idx].clone()));
                        }
                    }
                }
            }
            idx += 1;
            continue;
        }
        let chars = &arg[1..];
        let mut j = 0;
        while j < chars.len() {
            let c = chars[j];
            let Some(&(_, ar)) = short.iter().find(|(x, _)| *x == c) else {
                j += 1;
                continue;
            };
            let name = [c];
            match ar {
                Arity::None => {
                    add_opt(&mut out, &name, Value::Bool(false));
                    j += 1;
                }
                Arity::Optional => {
                    let rest = &chars[j + 1..];
                    let rest = rest.strip_prefix(b"=").unwrap_or(rest);
                    let v = if rest.is_empty() {
                        Value::Bool(false)
                    } else {
                        sv(rest.to_vec())
                    };
                    add_opt(&mut out, &name, v);
                    break;
                }
                Arity::Required => {
                    let rest = &chars[j + 1..];
                    let rest = rest.strip_prefix(b"=").unwrap_or(rest);
                    if !rest.is_empty() {
                        add_opt(&mut out, &name, sv(rest.to_vec()));
                    } else if idx + 1 < argv.len() {
                        idx += 1;
                        add_opt(&mut out, &name, Value::Str(argv[idx].clone()));
                    }
                    break;
                }
            }
        }
        idx += 1;
    }
    if a.has(2) {
        a.set_ref(2, Value::Int(idx as i64));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- shell escaping

fn escapeshellarg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0);
    if s.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($arg) must not contain any null bytes",
        );
    }
    // Observed on PHP 8.5: arguments are limited to 2 MiB with the quotes.
    if s.len() + 2 >= SHELL_ARG_MAX {
        return vm.throw(
            "ValueError",
            &format!(
                "Argument exceeds the allowed length of {} bytes",
                SHELL_ARG_MAX
            ),
        );
    }
    let mut out = Vec::with_capacity(s.len() + 2);
    out.push(b'\'');
    for &c in s {
        if c == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(c);
        }
    }
    out.push(b'\'');
    if out.len() > SHELL_ARG_MAX {
        return vm.throw(
            "ValueError",
            &format!(
                "Escaped argument exceeds the allowed length of {} bytes",
                SHELL_ARG_MAX
            ),
        );
    }
    Ok(sv(out))
}

const SHELL_ARG_MAX: usize = 2 * 1024 * 1024;

fn escapeshellcmd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0);
    if s.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($command) must not contain any null bytes",
        );
    }
    if s.len() + 2 >= SHELL_ARG_MAX {
        return vm.throw(
            "ValueError",
            &format!(
                "Command exceeds the allowed length of {} bytes",
                SHELL_ARG_MAX
            ),
        );
    }
    let mut out = Vec::with_capacity(s.len() + 8);
    // Quotes are kept when they pair up (observed on PHP 8.5): the
    // position of the closing quote of an open pair.
    let mut pending: Option<usize> = None;
    let mut skip = 0usize;
    for (i, &c) in s.iter().enumerate() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if c >= 0x80 {
            // Observed on PHP 8.5 (UTF-8 locale): a valid multibyte
            // character is copied, an invalid byte is dropped.
            if let (Some(_), n) = super::string::decode_utf8(&s[i..]) {
                out.extend_from_slice(&s[i..i + n]);
                skip = n - 1;
            }
            continue;
        }
        match c {
            b'"' | b'\'' => {
                if pending == Some(i) {
                    pending = None;
                    out.push(c);
                } else if pending.is_none()
                    && let Some(p) = s[i + 1..].iter().position(|&x| x == c)
                {
                    pending = Some(i + 1 + p);
                    out.push(c);
                } else {
                    out.push(b'\\');
                    out.push(c);
                }
            }
            b'#' | b'&' | b';' | b'`' | b'|' | b'*' | b'?' | b'~' | b'<' | b'>' | b'^' | b'('
            | b')' | b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b'\n' | 0xff => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    if out.len() > SHELL_ARG_MAX {
        return vm.throw(
            "ValueError",
            &format!(
                "Escaped command exceeds the allowed length of {} bytes",
                SHELL_ARG_MAX
            ),
        );
    }
    Ok(sv(out))
}

// ---------------------------------------------------------------- assertions

/// A callback set by assert_options(ASSERT_CALLBACK, ...).
#[derive(Default)]
struct AssertState {
    callback: Option<Value>,
}

fn ini_flag(vm: &Vm, name: &[u8]) -> i64 {
    vm.get_ini(name)
        .map(|v| goro_runtime::convert::str_to_int(&v))
        .unwrap_or(0)
}

/// Whether setting `k` to `v` changes a deprecated assert.* setting from
/// its default (observed on PHP 8.5: "assert.bail INI setting is
/// deprecated" for `ini_set("assert.bail", "1")`, not for "0").
fn assert_ini_truthy(v: &[u8]) -> bool {
    let l = v.to_ascii_lowercase();
    match &l[..] {
        b"on" | b"yes" | b"true" => true,
        b"off" | b"no" | b"false" | b"none" | b"" => false,
        _ => goro_runtime::convert::str_to_int(&l) != 0,
    }
}

pub(crate) fn assert_ini_deprecated(k: &[u8], v: &[u8]) -> bool {
    let truthy = |v: &[u8]| {
        let l = v.to_ascii_lowercase();
        match &l[..] {
            b"on" | b"yes" | b"true" => true,
            b"off" | b"no" | b"false" | b"none" | b"" => false,
            _ => goro_runtime::convert::str_to_int(&l) != 0,
        }
    };
    match k {
        b"assert.active" | b"assert.warning" | b"assert.exception" => !truthy(v),
        b"assert.bail" => truthy(v),
        b"assert.callback" => !v.is_empty(),
        _ => false,
    }
}

/// Startup diagnostics for deprecated `-d` settings, in registration
/// order (observed on PHP 8.5).
pub(crate) fn startup_ini_deprecations(vm: &mut Vm, ini: &[(Vec<u8>, Vec<u8>)]) {
    for k in [
        &b"assert.active"[..],
        b"assert.bail",
        b"assert.warning",
        b"assert.callback",
        b"assert.exception",
    ] {
        let Some((_, v)) = ini.iter().rev().find(|(n, _)| n.as_slice() == k) else {
            continue;
        };
        if assert_ini_deprecated(k, v) {
            let _ = vm.raise_at(
                crate::vm::errors::E_DEPRECATED,
                &format!(
                    "PHP Startup: {} INI setting is deprecated",
                    String::from_utf8_lossy(k)
                ),
                goro_runtime::PhpStr::from("Unknown"),
                0,
            );
        }
    }
    if ini
        .iter()
        .rev()
        .find(|(n, _)| n.as_slice() == b"allow_url_include")
        .is_some_and(|(_, v)| assert_ini_truthy(v))
    {
        let _ = vm.raise_at(
            crate::vm::errors::E_DEPRECATED,
            "Directive 'allow_url_include' is deprecated",
            goro_runtime::PhpStr::from("Unknown"),
            0,
        );
    }
    for k in [
        &b"mbstring.http_input"[..],
        b"mbstring.http_output",
        b"mbstring.internal_encoding",
    ] {
        if ini.iter().rev().find(|(n, _)| n.as_slice() == k).is_some_and(|(_, v)| !v.is_empty()) {
            let _ = vm.raise_at(
                crate::vm::errors::E_DEPRECATED,
                &format!("PHP Startup: Use of {} is deprecated", String::from_utf8_lossy(k)),
                goro_runtime::PhpStr::from("Unknown"),
                0,
            );
        }
    }
}

/// ini_set("assert.callback") replaces a callback set by assert_options()
/// (test assert/assert03.phpt).
pub(crate) fn assert_callback_reset(vm: &mut Vm) {
    vm.ext::<AssertState>().callback = None;
}

fn assert_callback(vm: &mut Vm) -> Value {
    if let Some(cb) = vm.ext::<AssertState>().callback.clone() {
        return cb;
    }
    match vm.get_ini(b"assert.callback") {
        Some(s) if !s.is_empty() => Value::Str(s),
        _ => Value::Null,
    }
}

/// What assert() does on a failed assertion: the callback, then an
/// AssertionError or a warning, as assert.* settings say (PHP manual:
/// assert). `desc` is the description argument (a string or Throwable).
pub(crate) fn assert_failed(vm: &mut Vm, desc: Value) -> R<Value> {
    let cb = assert_callback(vm);
    if !cb.is_null() && !vm.is_callable_value(&cb, false) {
        // Test assert/assert_variation.phpt.
        let why = vm.callable_error(&cb);
        let shown = match &cb {
            Value::Str(s) => s.to_string_lossy(),
            _ => "Array".to_string(),
        };
        return vm.throw("Error", &format!("Invalid callback {}, {}", shown, why));
    }
    if !cb.is_null() {
        let file = vm.cur_file();
        let line = vm.cur_line() as i64;
        let d = match &desc {
            Value::Object(_) => Value::Null,
            other => other.clone(),
        };
        vm.call_value(
            &cb,
            vec![Value::Str(file), Value::Int(line), Value::Null, d],
        )?;
    }
    if ini_flag(vm, b"assert.exception") != 0 {
        return match desc {
            Value::Object(o) => Err(Unwind::Throw(o)),
            Value::Null => {
                let e = vm.create_exception("AssertionError", "", 0)?;
                Err(Unwind::Throw(e))
            }
            other => {
                let msg = vm.to_php_str(&other)?;
                let e = vm.create_exception("AssertionError", &msg.to_string_lossy(), 0)?;
                Err(Unwind::Throw(e))
            }
        };
    }
    if ini_flag(vm, b"assert.warning") != 0 {
        let msg = match &desc {
            Value::Object(o) => vm.get_prop_raw(o, b"message"),
            other => other.clone(),
        };
        // Observed on PHP 8.5: a null description gives "Assertion failed".
        if msg.is_null() {
            vm.warn("assert(): Assertion failed")?;
        } else {
            let m = vm.to_php_str(&msg)?;
            vm.warn(&format!("assert(): {} failed", m.to_string_lossy()))?;
        }
    }
    if ini_flag(vm, b"assert.bail") != 0 {
        vm.exit_status = 255;
        return Err(Unwind::Exit);
    }
    Ok(Value::Bool(false))
}

fn assert_options(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated("Function assert_options() is deprecated since 8.3")?;
    let opt = a.int(0);
    let name: &[u8] = match opt {
        1 => b"assert.active",
        2 => b"assert.callback",
        3 => b"assert.bail",
        4 => b"assert.warning",
        5 => b"assert.exception",
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($option) must be an ASSERT_* constant",
            );
        }
    };
    let old = if opt == 2 {
        assert_callback(vm)
    } else {
        Value::Int(ini_flag(vm, name))
    };
    if a.has(1) {
        let v = a.val(1);
        if opt == 2 {
            vm.ext::<AssertState>().callback = if v.is_null() { None } else { Some(v) };
            if vm.ext::<AssertState>().callback.is_none() {
                vm.set_ini(name, b"");
            }
        } else {
            let s = vm.to_php_str(&v)?;
            vm.set_ini(name, &s);
        }
    }
    Ok(old)
}

// ---------------------------------------------------------------- version_compare

/// Canonical form (PHP manual: version_compare): '-', '_' and '+' become
/// '.', and a '.' separates digits from other characters.
fn canon_version(v: &[u8]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let flush = |cur: &mut Vec<u8>, parts: &mut Vec<Vec<u8>>| {
        if !cur.is_empty() {
            parts.push(std::mem::take(cur));
        }
    };
    for &c in v {
        if matches!(c, b'.' | b'-' | b'_' | b'+') {
            flush(&mut cur, &mut parts);
            continue;
        }
        if let Some(&last) = cur.last()
            && last.is_ascii_digit() != c.is_ascii_digit()
        {
            flush(&mut cur, &mut parts);
        }
        cur.push(c);
    }
    flush(&mut cur, &mut parts);
    parts
}

/// The order of special forms: anything else < dev < alpha = a < beta = b
/// < RC = rc < # (numbers) < pl = p.
fn special_rank(p: &[u8]) -> i32 {
    let l = p.to_ascii_lowercase();
    let l = l.as_slice();
    if l.starts_with(b"dev") {
        0
    } else if l.starts_with(b"a") {
        1
    } else if l.starts_with(b"b") {
        2
    } else if l.starts_with(b"rc") {
        3
    } else if l == b"#" {
        4
    } else if l.starts_with(b"p") {
        5
    } else {
        -1
    }
}

fn cmp_part(a: &[u8], b: &[u8]) -> i32 {
    let ad = a[0].is_ascii_digit();
    let bd = b[0].is_ascii_digit();
    match (ad, bd) {
        (true, true) => {
            let x: u128 = std::str::from_utf8(a)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let y: u128 = std::str::from_utf8(b)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            (x > y) as i32 - (x < y) as i32
        }
        (false, false) => {
            let (x, y) = (special_rank(a), special_rank(b));
            (x > y) as i32 - (x < y) as i32
        }
        (true, false) => {
            let y = special_rank(b);
            (4 > y) as i32 - (4 < y) as i32
        }
        (false, true) => {
            let x = special_rank(a);
            (x > 4) as i32 - (x < 4) as i32
        }
    }
}

pub(crate) fn compare_versions(a: &[u8], b: &[u8]) -> i32 {
    let pa = canon_version(a);
    let pb = canon_version(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let c = match (pa.get(i), pb.get(i)) {
            (Some(x), Some(y)) => cmp_part(x, y),
            // A longer version: a number makes it newer, a special form is
            // compared with "#".
            (Some(x), None) => cmp_part(x, b"#"),
            (None, Some(y)) => cmp_part(b"#", y),
            (None, None) => 0,
        };
        if c != 0 {
            return c;
        }
    }
    0
}

fn version_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = compare_versions(a.bytes(0), a.bytes(1));
    let Some(op) = a.opt_str(2) else {
        return Ok(Value::Int(c as i64));
    };
    let r = match op.as_bytes() {
        b"<" | b"lt" => c < 0,
        b"<=" | b"le" => c <= 0,
        b">" | b"gt" => c > 0,
        b">=" | b"ge" => c >= 0,
        b"==" | b"=" | b"eq" => c == 0,
        b"!=" | b"<>" | b"ne" => c != 0,
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #3 ($operator) must be a valid comparison operator",
            );
        }
    };
    Ok(Value::Bool(r))
}

// ---------------------------------------------------------- deprecated constants

/// Why an internal constant is deprecated: the text after "Constant X is
/// deprecated since " (observed on PHP 8.5 through
/// ReflectionConstant::isDeprecated() and constant()).
pub(crate) fn const_deprecation(name: &[u8]) -> Option<&'static str> {
    const ASSERT: &str = "8.3, as assert_options() is deprecated";
    const SUN: &str = "8.4, as date_sunrise() and date_sunset() were deprecated in 8.1";
    const FILE: &str = "8.1, as the constant has no effect";
    const FILTER: &str = "8.1, use htmlspecialchars() instead";
    Some(match name {
        b"E_STRICT" => "8.4, the error level was removed",
        b"DATE_RFC7231" => {
            "8.5, as this format ignores the associated timezone and always uses GMT"
        }
        b"SUNFUNCS_RET_TIMESTAMP" | b"SUNFUNCS_RET_STRING" | b"SUNFUNCS_RET_DOUBLE" => SUN,
        b"FILTER_SANITIZE_STRING" | b"FILTER_SANITIZE_STRIPPED" => FILTER,
        b"MT_RAND_PHP" => "8.3, as it uses a biased non-standard variant of Mt19937",
        b"ASSERT_ACTIVE" | b"ASSERT_CALLBACK" | b"ASSERT_BAIL" | b"ASSERT_WARNING"
        | b"ASSERT_EXCEPTION" => ASSERT,
        b"FILE_TEXT" | b"FILE_BINARY" => FILE,
        b"CURLOPT_BINARYTRANSFER" => "8.4, as it had no effect since 5.1.2",
        b"DOM_PHP_ERR" => "8.4, as it is no longer used",
        b"SOAP_FUNCTIONS_ALL" => "8.4, as enabling all functions is a possible security concern",
        n if n.starts_with(b"MHASH_") => "8.5, as the mhash*() functions were deprecated",
        _ => return None,
    })
}
