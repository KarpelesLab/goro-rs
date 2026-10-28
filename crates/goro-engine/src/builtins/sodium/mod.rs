//! The sodium extension (PHP manual: Sodium): every `sodium_*()` function,
//! the `SODIUM_*` constants and `SodiumException`. The cryptography,
//! argument checks and messages live in the goro-ext-sodium crate; the
//! handlers in [`funcs`] follow its `SIGNATURES` table (PHP's parameter
//! names, types, defaults, references and sensitive parameters).
//!
//! Behaviour sources: the PHP manual and the expected output of
//! ext/sodium/tests/*.phpt.

mod funcs;

use goro_ext_sodium as s;
use goro_ext_sodium::{ConstValue, SodiumError};
use goro_runtime::{Key, PhpArray, PhpRef, PhpStr, Value};

use crate::func::{Args, NativeFn};
use crate::registry::Module;
use crate::vm::{R, Unwind, Vm};

use funcs::*;

pub static MODULE: Module = Module {
    name: "sodium",
    functions: FUNCTIONS,
    classes: &[(
        "sodiumexception",
        "<?php class SodiumException extends Exception {}",
    )],
    init: Some(init),
    ..Module::EMPTY
};

fn init(vm: &mut Vm) {
    for (name, v) in s::CONSTANTS {
        let v = match v {
            ConstValue::Int(i) => Value::Int(*i),
            ConstValue::Str(t) => Value::Str(PhpStr::intern(t.as_bytes())),
        };
        vm.constants.insert(PhpStr::intern(name.as_bytes()), v);
    }
}

/// Bounds argument-sized results by `memory_limit` (crate wiring notes).
fn set_limit(vm: &Vm) {
    let l = vm.ini.memory_limit;
    s::set_max_string_len(if l < 0 { usize::MAX } else { l as usize });
}

fn sr<T>(vm: &mut Vm, r: Result<T, SodiumError>) -> R<T> {
    r.map_err(|e| sodium_err(vm, e))
}

/// A `SodiumException`, whose trace omits the arguments (test
/// ext/sodium/tests/exception_trace_without_args.phpt), or PHP's fatal
/// "Out of memory" error.
fn sodium_err(vm: &mut Vm, e: SodiumError) -> Unwind {
    match e {
        SodiumError::Exception(m) => {
            let ex = match vm.create_exception(s::EXCEPTION_CLASS, &m, 0) {
                Ok(ex) => ex,
                Err(u) => return u,
            };
            if let Value::Array(mut t) = vm.get_prop_raw(&ex, b"trace") {
                let keys: Vec<Key> = t.iter().map(|(k, _)| k.to_owned()).collect();
                for k in keys {
                    if let Some(Value::Array(fr)) = t.get_mut(k.as_ref()) {
                        fr.remove(Key::from("args").as_ref());
                    }
                }
                vm.set_prop_raw(&ex, b"trace", Value::Array(t));
            }
            Unwind::Throw(ex)
        }
        SodiumError::OutOfMemory(n) => {
            let l = vm.ini.memory_limit;
            vm.fatal(&format!(
                "Allowed memory size of {} bytes exhausted (tried to allocate {} bytes)",
                if l < 0 { i64::MAX } else { l },
                n
            ))
        }
    }
}

fn bytes(b: Vec<u8>) -> Value {
    Value::Str(PhpStr::from(b))
}

fn list<const N: usize>(v: [Value; N]) -> Value {
    Value::Array(PhpArray::from_values(v))
}

/// The cell of a by-reference parameter.
fn by_ref(a: &Args, i: usize) -> PhpRef {
    match a.ref_at(i) {
        Some(r) => r,
        None => PhpRef::new(a.val(i)),
    }
}
