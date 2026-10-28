//! The filter extension (PHP manual: Data Filtering): `filter_var()` and
//! friends, the `FILTER_*`/`INPUT_*` constants, the `filter.default` ini
//! settings and the `Filter\*` exceptions. The filters live in the
//! goro-ext-filter crate; the engine is its [`FilterHost`].
//!
//! Behaviour sources: the PHP manual and the expected output of
//! ext/filter/tests/*.phpt.

use goro_ext_filter as f;
use goro_ext_filter::{FilterHost, InputType, Options, UriParser, UriParts};
use goro_runtime::{ObjRef, PhpArray, PhpError, PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "filter",
    functions: &[
        (
            "function filter_has_var(int $input_type, string $var_name): bool",
            filter_has_var,
        ),
        (
            "function filter_input(int $type, string $var_name, int $filter = FILTER_DEFAULT, array|int $options = 0): mixed",
            filter_input,
        ),
        (
            "function filter_var(mixed $value, int $filter = FILTER_DEFAULT, array|int $options = 0): mixed",
            filter_var,
        ),
        (
            "function filter_input_array(int $type, array|int $options = FILTER_DEFAULT, bool $add_empty = true): array|false|null",
            filter_input_array,
        ),
        (
            "function filter_var_array(array $array, array|int $options = FILTER_DEFAULT, bool $add_empty = true): array|false|null",
            filter_var_array,
        ),
        ("function filter_list(): array", filter_list),
        ("function filter_id(string $name): int|false", filter_id),
    ],
    classes: &[
        (
            "filter\\filterexception",
            "<?php namespace Filter; class FilterException extends \\Exception {}",
        ),
        (
            "filter\\filterfailedexception",
            "<?php namespace Filter; class FilterFailedException extends FilterException {}",
        ),
    ],
    // PHP manual: Filter runtime configuration.
    ini: &[
        ("filter.default", "unsafe_raw"),
        ("filter.default_flags", ""),
    ],
    init: Some(init),
    startup_ini: Some(startup_ini),
    ..Module::EMPTY
};

/// "PHP Startup"-time deprecation of `filter.default` set to anything but
/// unsafe_raw (test filter_default_deprecation.phpt).
fn startup_ini(vm: &mut Vm, ini: &[(Vec<u8>, Vec<u8>)]) {
    let Some((_, v)) = ini.iter().rev().find(|(n, _)| n == b"filter.default") else {
        return;
    };
    if f::InputFilter::deprecated(v) {
        let _ = vm.raise_at(
            crate::vm::errors::E_DEPRECATED,
            "The filter.default ini setting is deprecated",
            PhpStr::from("Unknown"),
            0,
        );
    }
}

fn init(vm: &mut Vm) {
    for c in f::consts::ALL.iter() {
        vm.constants
            .insert(PhpStr::intern(c.name.as_bytes()), Value::Int(c.value));
    }
}

impl FilterHost for Vm {
    type Exception = Unwind;

    fn object_string(&mut self, obj: &ObjRef) -> Result<Option<PhpStr>, Unwind> {
        let cls = self.class_of(obj);
        if cls.magic.borrow().to_string.is_none() {
            return Ok(None);
        }
        self.obj_to_string(obj).map(Some)
    }

    fn is_callable(&mut self, callback: &Value) -> bool {
        self.is_callable_value(callback, false)
    }

    fn call(&mut self, callback: &Value, arg: Value) -> Result<Value, Unwind> {
        self.call_value(callback, vec![arg])
    }

    fn throw_error(&mut self, err: PhpError) -> Unwind {
        self.php_error(err)
    }

    fn filter_failed(&mut self, message: PhpStr) -> Unwind {
        match self.create_exception("Filter\\FilterFailedException", "", 0) {
            Ok(ex) => {
                self.set_prop_raw(&ex, b"message", Value::Str(message));
                Unwind::Throw(ex)
            }
            Err(u) => u,
        }
    }

    fn input(&mut self, ty: InputType) -> Option<PhpArray> {
        // The CLI registers no GET/POST/COOKIE arrays (test 012.phpt).
        let name: &[u8] = match ty {
            InputType::Server => b"_SERVER",
            InputType::Env => b"_ENV",
            _ => return None,
        };
        let st = self.ext::<InputArrays>();
        if let Some(a) = match ty {
            InputType::Server => &st.server,
            _ => &st.env,
        } {
            return Some(a.clone());
        }
        let v = self.globals_array().get_bytes(name).map(|v| v.deref());
        let arr = match v {
            Some(Value::Array(a)) => a,
            _ => PhpArray::new(),
        };
        let st = self.ext::<InputArrays>();
        match ty {
            InputType::Server => st.server = Some(arr.clone()),
            _ => st.env = Some(arr.clone()),
        }
        Some(arr)
    }

    fn default_charset(&self) -> Vec<u8> {
        match self.get_ini(b"default_charset") {
            Some(s) if !s.is_empty() => s.to_vec(),
            _ => b"UTF-8".to_vec(),
        }
    }

    fn pcre_limits(&self) -> goro_regex::Limits {
        super::pcre::limits(self)
    }

    /// The `uri_parser_class` option of FILTER_VALIDATE_URL, served by
    /// ext/uri's parsers (test ext/filter/tests/062.phpt).
    fn parse_uri(&mut self, p: UriParser, uri: &[u8]) -> Option<Option<UriParts>> {
        let parser = match p {
            UriParser::Rfc3986 => goro_ext_uri::FilterParser::Rfc3986,
            UriParser::WhatWg => goro_ext_uri::FilterParser::WhatWg,
        };
        Some(goro_ext_uri::filter_parts(parser, uri).map(|v| UriParts {
            scheme: v.scheme,
            host: v.host,
            has_path: v.has_path,
            has_query: v.has_query,
        }))
    }
}

/// The input arrays as first seen (later changes to the superglobals do
/// not affect filter_input(); crate wiring notes).
#[derive(Default)]
struct InputArrays {
    server: Option<PhpArray>,
    env: Option<PhpArray>,
}

fn options(a: &Args, i: usize, default: i64) -> (Option<PhpArray>, i64) {
    match a.val(i) {
        Value::Array(arr) => (Some(arr), 0),
        Value::Int(n) => (None, n),
        _ => (None, default),
    }
}

fn opts(arr: &Option<PhpArray>, n: i64) -> Options<'_> {
    match arr {
        Some(a) => Options::Array(a),
        None => Options::Int(n),
    }
}

fn done<T>(vm: &mut Vm, r: Result<T, Unwind>) -> R<T> {
    let r = r?;
    vm.check_deferred()?;
    Ok(r)
}

fn filter_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let filter = a.int_or(1, f::consts::FILTER_DEFAULT);
    let (arr, n) = options(a, 2, 0);
    let r = f::filter_var(vm, &v, filter, opts(&arr, n));
    done(vm, r)
}

fn filter_var_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let input = a.array(0);
    let (arr, n) = options(a, 1, f::consts::FILTER_DEFAULT);
    let add_empty = a.bool_or(2, true);
    let r = f::filter_var_array(vm, &input, opts(&arr, n), add_empty);
    done(vm, r)
}

fn filter_input(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ty = a.int(0);
    let name = a.str(1);
    let filter = a.int_or(2, f::consts::FILTER_DEFAULT);
    let (arr, n) = options(a, 3, 0);
    let r = f::filter_input(vm, ty, name.as_bytes(), filter, opts(&arr, n));
    done(vm, r)
}

fn filter_input_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ty = a.int(0);
    let (arr, n) = options(a, 1, f::consts::FILTER_DEFAULT);
    let add_empty = a.bool_or(2, true);
    let r = f::filter_input_array(vm, ty, opts(&arr, n), add_empty);
    done(vm, r)
}

fn filter_has_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ty = a.int(0);
    let name = a.str(1);
    let r = f::filter_has_var(vm, ty, name.as_bytes());
    done(vm, r).map(Value::Bool)
}

fn filter_list(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(f::filter_list()))
}

fn filter_id(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match f::filter_id(a.bytes(0)) {
        Some(i) => Value::Int(i),
        None => Value::Bool(false),
    })
}
