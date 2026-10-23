//! php-src's ext/sodium (libsodium.c, sodium_pwhash.c) with the libsodium
//! primitives over purecrypto (and, where purecrypto has no public
//! equivalent, the local ports in salsa.rs, blake2b.rs, ed25519.rs,
//! codecs.rs and pwhash.rs).

// The primitives follow libsodium's 0 / -1 convention.
#![allow(clippy::result_unit_err)]

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_INT, T_STRING};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::ClassEntry;
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod blake2b;
pub mod codecs;
pub mod ed25519;
mod funcs;
mod password;
pub mod pwhash;
pub mod salsa;

/// The extension name (`extension_loaded('sodium')`).
pub const NAME: &[u8] = b"sodium";

/// The libsodium release whose API (and limits) this reproduces.
pub const LIBRARY_VERSION: &str = "1.0.22";
pub const LIBRARY_VERSION_MAJOR: i64 = 26;
pub const LIBRARY_VERSION_MINOR: i64 = 4;

pub(crate) const EXC: &[u8] = b"SodiumException";

pub(crate) type R = Result<Value, VmError>;

/// randombytes_buf()
pub fn random_bytes(buf: &mut [u8]) {
    use purecrypto::rng::{OsRng, RngCore};
    OsRng.fill_bytes(buf);
}

// ---------------------------------------------------------------------------
// exceptions

/// sodium_remove_param_values_from_backtrace(): every frame of the
/// exception's trace gets an empty "args".
pub(crate) fn strip_trace_args(exc: &Value) {
    let Value::Object(o) = exc else { return };
    let trace = o.borrow().throwable_get(b"trace");
    let Value::Array(t) = trace else { return };
    let frames: Vec<Value> = t.borrow().iter().map(|(_, v)| v.clone()).collect();
    for f in frames {
        if let Value::Array(fa) = f {
            let has = fa.borrow().get_str(b"args").is_some();
            if has {
                fa.borrow_mut().set(
                    ArrayKey::String(PhpString::from_bytes(b"args")),
                    Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                );
            }
        }
    }
}

/// zend_throw_exception(): a pending exception becomes the new one's
/// previous.
pub(crate) fn throw_class(vm: &mut Vm, class: &[u8], msg: &str, strip: bool) {
    let e = vm.create_exception(class, msg, vm.current_line);
    if strip {
        strip_trace_args(&e);
    }
    if let Some(prev) = vm.current_exception.take() {
        if let Value::Object(o) = &e {
            o.borrow_mut().throwable_set(b"previous", prev);
        }
    }
    vm.current_exception = Some(e);
}

/// zend_throw_exception(sodium_exception_ce, msg, 0)
pub(crate) fn throw(vm: &mut Vm, msg: &str) -> R {
    throw_class(vm, EXC, msg, true);
    Ok(Value::Null)
}

pub(crate) fn param_name(f: &str, n: usize) -> &'static str {
    goro_core::stubs::find_function(f.as_bytes())
        .and_then(|s| s.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("")
}

/// zend_argument_error(sodium_exception_ce, n, msg): nothing while an
/// exception is pending.
pub(crate) fn arg_error(vm: &mut Vm, f: &str, n: usize, msg: &str) -> R {
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let m = format!("{}(): Argument #{} (${}) {}", f, n, param_name(f, n), msg);
    throw(vm, &m)
}

// ---------------------------------------------------------------------------
// zend_parse_parameters()

/// A parsed argument.
pub(crate) enum A {
    Absent,
    S(Vec<u8>),
    L(i64),
    /// 'z': the argument as passed (a reference for by-ref parameters)
    Z(Value),
}

impl A {
    pub fn s(&self) -> &[u8] {
        match self {
            A::S(s) => s,
            _ => b"",
        }
    }
    pub fn opt_s(&self) -> Option<&[u8]> {
        match self {
            A::S(s) => Some(s),
            _ => None,
        }
    }
    pub fn l(&self) -> i64 {
        match self {
            A::L(n) => *n,
            _ => 0,
        }
    }
    pub fn l_or(&self, d: i64) -> i64 {
        match self {
            A::L(n) => *n,
            _ => d,
        }
    }
    pub fn z(&self) -> Value {
        match self {
            A::Z(v) => v.clone(),
            _ => Value::Null,
        }
    }
}

fn zpp_fail(vm: &mut Vm, class: &[u8], msg: &str, strip: bool) -> Option<Vec<A>> {
    throw_class(vm, class, msg, strip);
    None
}

/// zend_parse_parameters() for the specs ext/sodium uses: `s`, `l`, `z`
/// and `|`. None once the exception is pending; with `strip`, its trace
/// lost its argument values (sodium_remove_param_values_from_backtrace).
pub(crate) fn zpp(vm: &mut Vm, f: &str, spec: &str, args: &[Value], strip: bool) -> Option<Vec<A>> {
    let min = spec.find('|').unwrap_or(spec.len());
    let max = spec.chars().filter(|&c| c != '|').count();
    let given = args.len();
    if given < min || given > max {
        let (kind, n) = if min == max {
            ("exactly", min)
        } else if given < min {
            ("at least", min)
        } else {
            ("at most", max)
        };
        let msg = format!(
            "{}() expects {} {} argument{}, {} given",
            f,
            kind,
            n,
            if n == 1 { "" } else { "s" },
            given
        );
        return zpp_fail(vm, b"ArgumentCountError", &msg, strip);
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::new();
    for (i, c) in spec.chars().filter(|&c| c != '|').enumerate() {
        let n = i + 1;
        let Some(raw) = args.get(i) else {
            out.push(A::Absent);
            continue;
        };
        if matches!(raw, Value::Undef) {
            out.push(A::Absent);
            continue;
        }
        if c == 'z' {
            out.push(A::Z(raw.clone()));
            continue;
        }
        let v = raw.deref();
        let (mask, tyname) = if c == 's' {
            (T_STRING, "string")
        } else {
            (T_INT, "int")
        };
        let type_error = |vm: &mut Vm, v: &Value| {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type {}, {} given",
                f,
                n,
                param_name(f, n),
                tyname,
                Vm::value_type_name(v)
            );
            zpp_fail(vm, b"TypeError", &msg, strip)
        };
        let coerced = if matches!(v, Value::Null) {
            if strict {
                return type_error(vm, &v);
            }
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                f,
                n,
                param_name(f, n),
                tyname
            ));
            if vm.current_exception.is_some() {
                if strip {
                    if let Some(e) = vm.current_exception.clone() {
                        strip_trace_args(&e);
                    }
                }
                return None;
            }
            if c == 's' {
                Value::String(PhpString::empty())
            } else {
                Value::Long(0)
            }
        } else {
            match vm.zpp_coerce_scalar(mask, &v, strict, line) {
                Ok(Some(v)) => v,
                Ok(None) => return type_error(vm, &v),
                Err(()) => {
                    if strip {
                        if let Some(e) = vm.current_exception.clone() {
                            strip_trace_args(&e);
                        }
                    }
                    return None;
                }
            }
        };
        if vm.current_exception.is_some() {
            return None;
        }
        match coerced {
            Value::String(s) => out.push(A::S(s.as_bytes().to_vec())),
            Value::Long(l) => out.push(A::L(l)),
            other => out.push(A::S(other.to_php_string().as_bytes().to_vec())),
        }
    }
    Some(out)
}

/// zend_parse_parameters_none()
pub(crate) fn zpp_none(vm: &mut Vm, f: &str, args: &[Value]) -> bool {
    zpp(vm, f, "", args, false).is_some()
}

pub(crate) fn bv(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub(crate) fn sval(b: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(b))
}

pub(crate) fn list(items: Vec<Value>) -> Value {
    let mut a = PhpArray::new();
    for v in items {
        a.push(v);
    }
    Value::Array(Rc::new(RefCell::new(a)))
}

/// Store into a by-reference argument.
pub(crate) fn write_ref(arg: &Value, v: Value) {
    if let Value::Reference(r) = arg {
        *r.borrow_mut() = v;
    }
}

// ---------------------------------------------------------------------------
// registration

fn compile_prelude(src: &[u8]) -> Option<(OpArray, Vec<ClassEntry>)> {
    let mut lexer = goro_parser::Lexer::new(src);
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(src);
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (mut op_array, classes, _) = compiler.compile(&program).ok()?;
    let _ = goro_core::compiler::take_pending_compile_warnings();
    for o in &mut op_array.ops {
        o.line = 0;
    }
    op_array.filename.clear();
    Some((op_array, classes))
}

fn register_classes(vm: &mut Vm) {
    const PRELUDE: &[u8] = b"<?php\nclass SodiumException extends Exception {}\n";
    let Some((op_array, classes)) = compile_prelude(PRELUDE) else {
        return;
    };
    let saved_file = std::mem::take(&mut vm.current_file);
    let saved_line = vm.current_line;
    let _ = vm.run_compiled(&op_array, classes);
    vm.current_file = saved_file;
    vm.current_line = saved_line;
    vm.current_exception = None;
    if let Some(ce) = vm.classes.get_mut(b"sodiumexception".as_slice()) {
        ce.filename = None;
        ce.start_line = 0;
        ce.end_line = 0;
    }
    // sodium_exception_create_object()
    vm.trace_without_args_classes
        .push(b"sodiumexception".to_vec());
}

fn phpinfo(_vm: &mut Vm) -> String {
    format!(
        "\nsodium\n\nsodium support => enabled\nlibsodium headers version => {v}\nlibsodium library version => {v}\n",
        v = LIBRARY_VERSION
    )
}

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    register_classes(vm);
    let mut names: Vec<Vec<u8>> = Vec::new();
    password::minit(vm, &mut names);
    for (name, v) in funcs::constants() {
        vm.constants.insert(name.as_bytes().to_vec(), v);
        names.push(name.as_bytes().to_vec());
    }
    vm.module_constants.push((NAME.to_vec(), names));
    for &(name, f) in funcs::FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
    }
    vm.phpinfo_hooks.push(phpinfo);
}
