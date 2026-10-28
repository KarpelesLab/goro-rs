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
    ],
    constants: &[
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
    Ok(sv(out))
}

fn escapeshellcmd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0);
    if s.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($command) must not contain any null bytes",
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
        let m = vm.to_php_str(&msg)?;
        vm.warn(&format!("assert(): {} failed", m.to_string_lossy()))?;
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
