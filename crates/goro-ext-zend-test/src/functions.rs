//! The functions of ext/zend_test (test.c).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, Vm, VmError};

type R = Result<Value, VmError>;

/// zpp verified flags (arginfo.rs): the generic layer reproduces
/// ZEND_PARSE_PARAMETERS for functions taking scalars.
const ARITY: u8 = 1;
const TYPES: u8 = 2;
const ALL: u8 = ARITY | TYPES;

/// (name, implementation, zpp flags handled by the generic layer)
const FUNCTIONS: &[(&str, BuiltinFn, u8)] = &[
    ("zend_trigger_bailout", zend_trigger_bailout, ALL),
    ("zend_test_array_return", ret_null, ALL),
    ("zend_test_nullable_array_return", ret_null, ALL),
    ("zend_test_void_return", ret_null, ALL),
    ("zend_test_compile_string", zend_test_compile_string, ALL),
    ("zend_test_compile_to_ast", zend_test_compile_to_ast, ALL),
    ("zend_test_deprecated", zend_test_deprecated, ALL),
    ("zend_test_deprecated_attr", ret_null, ALL),
    ("zend_test_nodiscard", ret_one, ALL),
    (
        "zend_test_deprecated_nodiscard",
        zend_test_deprecated_nodiscard,
        ALL,
    ),
    ("zend_test_aliased", ret_null, ALL),
    (
        "zend_test_deprecated_aliased",
        zend_test_deprecated_aliased,
        ALL,
    ),
    (
        "zend_create_unterminated_string",
        zend_create_unterminated_string,
        ALL,
    ),
    ("zend_terminate_string", ret_null_checked_str, ALL),
    ("zend_leak_variable", zend_leak_variable, ALL),
    ("zend_leak_bytes", ret_null, ALL),
    ("zend_delref", ret_null, ALL),
    ("zend_string_or_object", zend_string_or_object, ALL),
    (
        "zend_string_or_object_or_null",
        zend_string_or_object_or_null,
        ALL,
    ),
    ("zend_string_or_stdclass", zend_string_or_stdclass, ARITY),
    (
        "zend_string_or_stdclass_or_null",
        zend_string_or_stdclass_or_null,
        ARITY,
    ),
    ("zend_number_or_string", zend_number_or_string, ARITY),
    (
        "zend_number_or_string_or_null",
        zend_number_or_string_or_null,
        ARITY,
    ),
    ("zend_iterable", zend_iterable, ALL),
    ("zend_iterable_legacy", zend_iterable_legacy, 0),
    ("zend_weakmap_attach", zend_weakmap_attach, ALL),
    ("zend_weakmap_remove", zend_weakmap_remove, ALL),
    ("zend_weakmap_dump", zend_weakmap_dump, ALL),
    ("zend_get_unit_enum", zend_get_unit_enum, ALL),
    ("zend_test_parameter_with_attribute", ret_one, ALL),
    ("zend_test_attribute_with_named_argument", ret_null, ALL),
    (
        "zend_get_current_func_name",
        zend_get_current_func_name,
        ALL,
    ),
    ("zend_call_method", zend_call_method, ALL),
    (
        "zend_object_init_with_constructor",
        zend_object_init_with_constructor,
        ARITY,
    ),
    (
        "zend_call_method_if_exists",
        zend_call_method_if_exists,
        ALL,
    ),
    (
        "zend_test_zend_ini_parse_quantity",
        zend_test_zend_ini_parse_quantity,
        ALL,
    ),
    (
        "zend_test_zend_ini_parse_uquantity",
        zend_test_zend_ini_parse_uquantity,
        ALL,
    ),
    ("zend_test_zend_ini_str", zend_test_zend_ini_str, ALL),
    (
        "zend_test_is_string_marked_as_valid_utf8",
        zend_test_is_string_marked_as_valid_utf8,
        ALL,
    ),
    ("zend_get_map_ptr_last", zend_get_map_ptr_last, ALL),
    (
        "zend_test_fill_packed_array",
        zend_test_fill_packed_array,
        ARITY,
    ),
    ("get_open_basedir", get_open_basedir, ALL),
    ("zend_test_is_pcre_bundled", zend_test_is_pcre_bundled, ALL),
    ("zend_test_is_zend_ptr", ret_false_checked_int, ALL),
    ("zend_test_opcache_preloading", ret_false, ALL),
    ("zend_test_gh19792", zend_test_gh19792, ALL),
    ("zend_test_gh18756", ret_null, ALL),
    (
        "zend_test_internal_dnf_arguments",
        zend_test_internal_dnf_arguments,
        ARITY,
    ),
    ("ZendTestNS2\\namespaced_func", ret_true, ALL),
    (
        "ZendTestNS2\\namespaced_deprecated_func",
        ns2_deprecated_func,
        ALL,
    ),
    ("ZendTestNS2\\namespaced_aliased_func", ret_null, ALL),
    (
        "ZendTestNS2\\namespaced_deprecated_aliased_func",
        ns2_deprecated_aliased_func,
        ALL,
    ),
    ("ZendTestNS2\\ZendSubNS\\namespaced_func", ret_true, ALL),
    (
        "ZendTestNS2\\ZendSubNS\\namespaced_deprecated_func",
        subns_deprecated_func,
        ALL,
    ),
    (
        "ZendTestNS2\\ZendSubNS\\namespaced_aliased_func",
        ret_null,
        ALL,
    ),
    (
        "ZendTestNS2\\ZendSubNS\\namespaced_deprecated_aliased_func",
        subns_deprecated_aliased_func,
        ALL,
    ),
];

pub(crate) fn register(vm: &mut Vm) {
    WEAKMAP.with(|w| w.borrow_mut().clear());
    // Z_PARAM_VARIADIC_WITH_NAMED
    vm.builtin_named_variadics
        .insert(b"zend_object_init_with_constructor".to_vec());
    for &(name, f, flags) in FUNCTIONS {
        let lower = name.to_ascii_lowercase().into_bytes();
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        if flags != 0 && goro_core::stubs::find_function(name.as_bytes()).is_some() {
            vm.zpp_extension_verified.insert(lower, flags);
        }
    }
}

// ---------------------------------------------------------------------------
// helpers

fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

fn s(v: &str) -> Value {
    Value::String(PhpString::from_string(v.to_string()))
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

/// Throw `class` with `msg` (sets the pending exception).
fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> R {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Ok(Value::Null)
}

fn deprecated(vm: &mut Vm, msg: &str) {
    let line = vm.current_line;
    vm.emit_deprecated_at(msg, line);
}

/// "zend_string_or_stdclass(): Argument #1 ($param) must be of type ..."
fn type_error(vm: &mut Vm, func: &str, n: usize, param: &str, ty: &str, given: &Value) -> R {
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type {}, {} given",
        func,
        n,
        param,
        ty,
        Vm::value_type_name(given)
    );
    throw(vm, b"TypeError", &msg)
}

fn instance_of(vm: &Vm, v: &Value, class: &[u8]) -> bool {
    crate::support::instance_of(vm, v, class)
}

// ---------------------------------------------------------------------------
// trivial functions

fn ret_null(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Null)
}

fn ret_true(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::True)
}

fn ret_false(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::False)
}

fn ret_one(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(1))
}

fn ret_null_checked_str(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Null)
}

fn ret_false_checked_int(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::False)
}

fn zend_trigger_bailout(vm: &mut Vm, _args: &[Value]) -> R {
    let line = vm.current_line;
    Err(vm.emit_fatal_at("Bailout", line))
}

fn zend_test_deprecated(vm: &mut Vm, _args: &[Value]) -> R {
    deprecated(vm, "Function zend_test_deprecated() is deprecated");
    Ok(Value::Null)
}

fn zend_test_deprecated_nodiscard(_vm: &mut Vm, _args: &[Value]) -> R {
    // #[\Deprecated] and #[\NoDiscard] are checked by the VM (declaration
    // in prelude.php).
    Ok(Value::Long(1))
}

fn zend_test_deprecated_aliased(vm: &mut Vm, _args: &[Value]) -> R {
    deprecated(vm, "Function zend_test_deprecated_aliased() is deprecated");
    Ok(Value::Null)
}

fn ns2_deprecated_func(vm: &mut Vm, _args: &[Value]) -> R {
    deprecated(
        vm,
        "Function ZendTestNS2\\namespaced_deprecated_func() is deprecated",
    );
    Ok(Value::Null)
}

fn ns2_deprecated_aliased_func(vm: &mut Vm, _args: &[Value]) -> R {
    deprecated(
        vm,
        "Function ZendTestNS2\\namespaced_deprecated_aliased_func() is deprecated",
    );
    Ok(Value::Null)
}

fn subns_deprecated_func(vm: &mut Vm, _args: &[Value]) -> R {
    deprecated(
        vm,
        "Function ZendTestNS2\\ZendSubNS\\namespaced_deprecated_func() is deprecated",
    );
    Ok(Value::Null)
}

fn subns_deprecated_aliased_func(vm: &mut Vm, _args: &[Value]) -> R {
    deprecated(
        vm,
        "Function ZendTestNS2\\ZendSubNS\\namespaced_deprecated_aliased_func() is deprecated",
    );
    Ok(Value::Null)
}

fn zend_create_unterminated_string(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::String(arg(args, 0).to_php_string()))
}

fn zend_leak_variable(vm: &mut Vm, args: &[Value]) -> R {
    // Z_REFCOUNTED: strings and arrays built at runtime, objects and
    // resources. Literal strings and arrays are interned/immutable.
    let refcounted = match args.first() {
        Some(Value::Reference(_)) => true,
        Some(v) => matches!(
            v,
            Value::Object(_) | Value::Generator(_) | Value::Resource(_)
        ),
        None => false,
    };
    if !refcounted {
        vm.emit_warning("Cannot leak variable that is not refcounted");
    }
    Ok(Value::Null)
}

fn zend_get_map_ptr_last(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(0))
}

fn get_open_basedir(vm: &mut Vm, _args: &[Value]) -> R {
    Ok(match vm.constants.get(b"open_basedir".as_slice()) {
        Some(v) if !matches!(v, Value::Null) && !v.to_php_string().as_bytes().is_empty() => {
            Value::String(v.to_php_string())
        }
        _ => Value::Null,
    })
}

fn zend_test_is_pcre_bundled(_vm: &mut Vm, _args: &[Value]) -> R {
    // goro's PCRE engine is its own (pure Rust), not a system libpcre2.
    Ok(Value::True)
}

fn zend_test_is_string_marked_as_valid_utf8(_vm: &mut Vm, args: &[Value]) -> R {
    // goro strings do not carry php-src's IS_STR_VALID_UTF8 flag; the flag
    // is only ever set on strings that are valid UTF-8, which is what the
    // tests probe for. (Strings that are valid but not flagged by php-src
    // can't be told apart.)
    let s = arg(args, 0).to_php_string();
    Ok(bool_val(std::str::from_utf8(s.as_bytes()).is_ok()))
}

fn zend_test_gh19792(vm: &mut Vm, _args: &[Value]) -> R {
    vm.emit_warning("a warning");
    throw(vm, b"Error", "an exception")
}

fn zend_test_zend_ini_str(vm: &mut Vm, _args: &[Value]) -> R {
    Ok(vm
        .constants
        .get(b"zend_test.str_test".as_slice())
        .map(|v| Value::String(v.to_php_string()))
        .unwrap_or_else(|| s("")))
}

fn ini_quantity(vm: &mut Vm, args: &[Value], signed: bool) -> R {
    let v = arg(args, 0).to_php_string();
    let (n, err) = goro_core::ini_quantity::parse_quantity(v.as_bytes(), signed);
    if let Some(e) = err {
        vm.emit_warning(&e);
    }
    Ok(Value::Long(n as i64))
}

fn zend_test_zend_ini_parse_quantity(vm: &mut Vm, args: &[Value]) -> R {
    ini_quantity(vm, args, true)
}

fn zend_test_zend_ini_parse_uquantity(vm: &mut Vm, args: &[Value]) -> R {
    ini_quantity(vm, args, false)
}

fn zend_get_unit_enum(vm: &mut Vm, _args: &[Value]) -> R {
    Ok(vm
        .get_enum_case(b"zendtestunitenum", b"Foo")
        .unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// zpp variants

/// Z_PARAM_OBJ_OR_STR(_OR_NULL)
fn obj_or_str(vm: &mut Vm, args: &[Value], func: &str, nullable: bool) -> R {
    let v = arg(args, 0);
    match &v {
        Value::Object(_) | Value::String(_) | Value::Generator(_) => Ok(v),
        Value::Null | Value::Undef if nullable => Ok(Value::Null),
        _ => {
            let ty = if nullable {
                "object|string|null"
            } else {
                "object|string"
            };
            str_param(vm, &v, func, ty).map(|r| r.unwrap_or(Value::Null))
        }
    }
}

/// The weak string coercion of zpp for a non-object, non-string argument of
/// a parameter displayed as `ty` (null deprecation shown with `null_ty`).
/// Ok(None) when an exception was thrown.
fn str_param(vm: &mut Vm, v: &Value, func: &str, ty: &str) -> Result<Option<Value>, VmError> {
    str_param_ex(vm, v, func, ty, ty)
}

fn str_param_ex(
    vm: &mut Vm,
    v: &Value,
    func: &str,
    ty: &str,
    null_ty: &str,
) -> Result<Option<Value>, VmError> {
    use goro_core::arginfo::T_STRING;
    if matches!(v, Value::Null | Value::Undef) {
        if vm.caller_strict_types {
            type_error(vm, func, 1, "param", ty, v)?;
            return Ok(None);
        }
        deprecated(
            vm,
            &format!(
                "{}(): Passing null to parameter #1 ($param) of type {} is deprecated",
                func, null_ty
            ),
        );
        if vm.current_exception.is_some() {
            return Ok(None);
        }
        return Ok(Some(s("")));
    }
    if vm.caller_strict_types {
        type_error(vm, func, 1, "param", ty, v)?;
        return Ok(None);
    }
    match vm.zpp_coerce_scalar(T_STRING, v, false, vm.current_line) {
        Ok(Some(c)) => Ok(Some(c)),
        Ok(None) => {
            type_error(vm, func, 1, "param", ty, v)?;
            Ok(None)
        }
        Err(()) => Ok(None),
    }
}

fn zend_string_or_object(vm: &mut Vm, args: &[Value]) -> R {
    obj_or_str(vm, args, "zend_string_or_object", false)
}

fn zend_string_or_object_or_null(vm: &mut Vm, args: &[Value]) -> R {
    obj_or_str(vm, args, "zend_string_or_object_or_null", true)
}

/// Z_PARAM_OBJ_OF_CLASS_OR_STR(_OR_NULL) with stdClass
fn stdclass_or_str(vm: &mut Vm, args: &[Value], func: &str, nullable: bool) -> R {
    let v = arg(args, 0);
    let ty = if nullable {
        "stdClass|string|null"
    } else {
        "stdClass|string"
    };
    match &v {
        Value::String(_) => Ok(v),
        Value::Object(_) if instance_of(vm, &v, b"stdClass") => Ok(v),
        Value::Null | Value::Undef if nullable => Ok(Value::Null),
        Value::Object(_) => {
            // zend_parse_arg_str_weak(): objects with __toString()
            if vm.caller_strict_types {
                return type_error(vm, func, 1, "param", ty, &v);
            }
            str_param(vm, &v, func, ty).map(|r| r.unwrap_or(Value::Null))
        }
        // The null deprecation of zend_parse_arg_str_slow() names "string".
        _ => str_param_ex(vm, &v, func, ty, "string").map(|r| r.unwrap_or(Value::Null)),
    }
}

fn zend_string_or_stdclass(vm: &mut Vm, args: &[Value]) -> R {
    stdclass_or_str(vm, args, "zend_string_or_stdclass", false)
}

fn zend_string_or_stdclass_or_null(vm: &mut Vm, args: &[Value]) -> R {
    stdclass_or_str(vm, args, "zend_string_or_stdclass_or_null", true)
}

/// Z_PARAM_NUMBER_OR_STR(_OR_NULL)
fn number_or_str(vm: &mut Vm, args: &[Value], func: &str, nullable: bool) -> R {
    use goro_core::arginfo::{T_FLOAT, T_INT, T_STRING};
    let v = arg(args, 0);
    let ty = if nullable {
        "string|int|float|null"
    } else {
        "string|int|float"
    };
    match &v {
        Value::Long(_) | Value::Double(_) | Value::String(_) => return Ok(v),
        Value::Null | Value::Undef if nullable => return Ok(Value::Null),
        _ => {}
    }
    if vm.caller_strict_types {
        return type_error(vm, func, 1, "param", ty, &v);
    }
    if matches!(v, Value::Null | Value::Undef) {
        deprecated(
            vm,
            &format!(
                "{}(): Passing null to parameter #1 ($param) of type {} is deprecated",
                func, ty
            ),
        );
        return Ok(Value::Long(0));
    }
    match &v {
        Value::False => return Ok(Value::Long(0)),
        Value::True => return Ok(Value::Long(1)),
        _ => {}
    }
    match vm.zpp_coerce_scalar(T_STRING | T_INT | T_FLOAT, &v, false, vm.current_line) {
        Ok(Some(c)) => Ok(c),
        Ok(None) => type_error(vm, func, 1, "param", ty, &v),
        Err(()) => Ok(Value::Null),
    }
}

fn zend_number_or_string(vm: &mut Vm, args: &[Value]) -> R {
    number_or_str(vm, args, "zend_number_or_string", false)
}

fn zend_number_or_string_or_null(vm: &mut Vm, args: &[Value]) -> R {
    number_or_str(vm, args, "zend_number_or_string_or_null", true)
}

/// Z_PARAM_ITERABLE / Z_PARAM_ITERABLE_OR_NULL
fn check_iterable(vm: &mut Vm, func: &str, args: &[Value]) -> Result<bool, VmError> {
    let a1 = arg(args, 0);
    let is_iter =
        |vm: &Vm, v: &Value| matches!(v, Value::Array(_)) || instance_of(vm, v, b"Traversable");
    if !is_iter(vm, &a1) {
        type_error(vm, func, 1, "arg1", "Traversable|array", &a1)?;
        return Ok(false);
    }
    if args.len() > 1 {
        let a2 = arg(args, 1);
        if !matches!(a2, Value::Null | Value::Undef) && !is_iter(vm, &a2) {
            type_error(vm, func, 2, "arg2", "Traversable|array|null", &a2)?;
            return Ok(false);
        }
    }
    Ok(true)
}

fn zend_iterable(vm: &mut Vm, args: &[Value]) -> R {
    check_iterable(vm, "zend_iterable", args)?;
    Ok(Value::Null)
}

fn zend_iterable_legacy(vm: &mut Vm, args: &[Value]) -> R {
    if args.is_empty() || args.len() > 2 {
        let msg = format!(
            "zend_iterable_legacy() expects {} {} argument{}, {} given",
            if args.is_empty() {
                "at least"
            } else {
                "at most"
            },
            if args.is_empty() { 1 } else { 2 },
            if args.is_empty() { "" } else { "s" },
            args.len()
        );
        return throw(vm, b"ArgumentCountError", &msg);
    }
    if check_iterable(vm, "zend_iterable_legacy", args)? {
        return Ok(arg(args, 0));
    }
    Ok(Value::Null)
}

fn zend_test_internal_dnf_arguments(vm: &mut Vm, args: &[Value]) -> R {
    let v = arg(args, 0);
    if !matches!(v, Value::Object(_) | Value::Generator(_)) {
        return type_error(
            vm,
            "zend_test_internal_dnf_arguments",
            1,
            "arg",
            "object",
            &v,
        );
    }
    let ok = instance_of(vm, &v, b"Iterator")
        || (instance_of(vm, &v, b"Traversable") && instance_of(vm, &v, b"Countable"));
    if !ok {
        return type_error(
            vm,
            "zend_test_internal_dnf_arguments",
            1,
            "arg",
            "Iterator|(Traversable&Countable)",
            &v,
        );
    }
    Ok(v)
}

fn zend_test_fill_packed_array(vm: &mut Vm, args: &[Value]) -> R {
    let Some(Value::Reference(r)) = args.first() else {
        return Ok(Value::Null);
    };
    let arr = match &*r.borrow() {
        Value::Array(a) => a.clone(),
        other => {
            let other = other.clone();
            return type_error(
                vm,
                "zend_test_fill_packed_array",
                1,
                "array",
                "array",
                &other,
            );
        }
    };
    // HT_IS_PACKED: integer keys in increasing order (holes allowed).
    let packed = {
        let a = arr.borrow();
        let mut prev: i64 = -1;
        a.keys().all(|k| match k {
            ArrayKey::Int(n) if *n > prev => {
                prev = *n;
                true
            }
            _ => false,
        })
    };
    if !packed {
        return throw(
            vm,
            b"ValueError",
            "zend_test_fill_packed_array(): Argument #1 ($array) must be a packed array",
        );
    }
    let mut a = (*arr.borrow()).clone();
    for i in 0..10 {
        a.push(Value::Long(i));
    }
    *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(a)));
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// weakmap

thread_local! {
    /// ZT_G(global_weakmap): object -> value, entries vanish with the object.
    static WEAKMAP: RefCell<Vec<(u64, Weak<RefCell<PhpObject>>, Value)>> =
        const { RefCell::new(Vec::new()) };
}

fn weakmap_prune() {
    WEAKMAP.with(|w| w.borrow_mut().retain(|(_, o, _)| o.strong_count() > 0));
}

fn zend_weakmap_attach(vm: &mut Vm, args: &[Value]) -> R {
    let Value::Object(o) = arg(args, 0) else {
        return type_error(
            vm,
            "zend_weakmap_attach",
            1,
            "object",
            "object",
            &arg(args, 0),
        );
    };
    let value = arg(args, 1);
    weakmap_prune();
    let id = o.borrow().object_id;
    let added = WEAKMAP.with(|w| {
        let mut w = w.borrow_mut();
        if w.iter().any(|(i, wo, _)| *i == id && wo.strong_count() > 0) {
            false
        } else {
            w.push((id, Rc::downgrade(&o), value));
            true
        }
    });
    Ok(bool_val(added))
}

fn zend_weakmap_remove(vm: &mut Vm, args: &[Value]) -> R {
    let Value::Object(o) = arg(args, 0) else {
        return type_error(
            vm,
            "zend_weakmap_remove",
            1,
            "object",
            "object",
            &arg(args, 0),
        );
    };
    weakmap_prune();
    let id = o.borrow().object_id;
    let removed = WEAKMAP.with(|w| {
        let mut w = w.borrow_mut();
        let before = w.len();
        w.retain(|(i, _, _)| *i != id);
        w.len() != before
    });
    Ok(bool_val(removed))
}

fn zend_weakmap_dump(_vm: &mut Vm, _args: &[Value]) -> R {
    weakmap_prune();
    let mut arr = PhpArray::new();
    WEAKMAP.with(|w| {
        for (id, _, v) in w.borrow().iter() {
            arr.set(ArrayKey::Int(*id as i64), v.clone());
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

// ---------------------------------------------------------------------------
// calling into PHP

/// get_function_or_method_name() of the calling frame ("main" for code
/// outside of functions).
fn zend_get_current_func_name(vm: &mut Vm, _args: &[Value]) -> R {
    Ok(s(&crate::support::current_function_name(vm)))
}

/// zend_call_method(): call a method of an object or class (by name, no
/// visibility checks) with up to two arguments.
fn zend_call_method(vm: &mut Vm, args: &[Value]) -> R {
    let target = arg(args, 0);
    let method = arg(args, 1).to_php_string();
    let call_args: Vec<Value> = args.iter().skip(2).cloned().collect();
    match &target {
        Value::Object(_) => {
            crate::support::call_method(vm, &target, None, method.as_bytes(), &call_args)
        }
        Value::String(c) => {
            let lower = c.as_bytes().to_ascii_lowercase();
            if !crate::support::class_exists_autoload(vm, c.as_bytes()) {
                let line = vm.current_line;
                return Err(
                    vm.emit_fatal_at(&format!("Unknown class '{}'", c.to_string_lossy()), line)
                );
            }
            crate::support::call_method(
                vm,
                &Value::Null,
                Some(&lower),
                method.as_bytes(),
                &call_args,
            )
        }
        other => type_error(
            vm,
            "zend_call_method",
            1,
            "obj_or_class",
            "object|string",
            other,
        ),
    }
}

/// zend_call_method_if_exists(): call a method the way zend_call_function()
/// does with a callable [$obj, $method]: returns null when it isn't
/// callable from the global scope (no error).
fn zend_call_method_if_exists(vm: &mut Vm, args: &[Value]) -> R {
    let obj = arg(args, 0);
    let method = arg(args, 1);
    let mut cb = PhpArray::new();
    cb.push(obj);
    cb.push(method);
    let cb = Value::Array(Rc::new(RefCell::new(cb)));
    let check = vm.zend_is_callable_ex(&cb, goro_core::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS);
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    if !check.ok {
        return Ok(Value::Null);
    }
    // Called like zend_call_function() from the caller's scope: a method
    // inaccessible from there reaches __call().
    let mut call_args = PhpArray::new();
    for a in args.iter().skip(2) {
        call_args.push(a.clone());
    }
    let r = crate::support::run_snippet(
        vm,
        "return $o->$m(...$a);",
        &[
            ("o", arg(args, 0)),
            ("m", arg(args, 1)),
            ("a", Value::Array(Rc::new(RefCell::new(call_args)))),
        ],
    )?;
    Ok(r.unwrap_or(Value::Null))
}

/// object_init_with_constructor(): instantiate `class` and run its
/// constructor with the (positional and named) arguments.
fn zend_object_init_with_constructor(vm: &mut Vm, args: &[Value]) -> R {
    let class = arg(args, 0);
    let Value::String(cname) = &class else {
        return type_error(
            vm,
            "zend_object_init_with_constructor",
            1,
            "class",
            "string",
            &class,
        );
    };
    if !crate::support::class_exists_autoload(vm, cname.as_bytes()) {
        return throw(
            vm,
            b"TypeError",
            &format!(
                "zend_object_init_with_constructor(): Argument #1 ($class) must be a valid class name, {} given",
                cname.to_string_lossy()
            ),
        );
    }
    let mut ctor_args = PhpArray::new();
    for a in args.iter().skip(1) {
        ctor_args.push(a.deref());
    }
    for (k, v) in std::mem::take(&mut vm.pending_named_args) {
        ctor_args.set(ArrayKey::String(PhpString::from_vec(k)), v);
    }
    let r = crate::support::run_snippet(
        vm,
        "return new $c(...$a);",
        &[
            ("c", class.clone()),
            ("a", Value::Array(Rc::new(RefCell::new(ctor_args)))),
        ],
    )?;
    Ok(r.unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// zend_test_compile_to_ast()

/// zend_compile_string_to_ast() + zend_ast_export(): the source rebuilt from
/// its AST. A syntax error throws ParseError.
fn zend_test_compile_to_ast(vm: &mut Vm, args: &[Value]) -> R {
    let source = arg(args, 0).to_php_string();
    let src = source.as_bytes();
    let tokens = goro_parser::Lexer::new(src).tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(src);
    match parser.parse() {
        Ok(program) => Ok(Value::String(PhpString::from_string(
            goro_core::ast_export::export_program(&program),
        ))),
        Err(e) => {
            let msg = if goro_parser::is_php_parse_error(&e.message) {
                e.message.clone()
            } else {
                "syntax error".to_string()
            };
            throw(vm, b"ParseError", &msg)
        }
    }
}

// ---------------------------------------------------------------------------
// zend_test_compile_string()

const ZEND_COMPILE_POSITION_AT_SHEBANG: i64 = 0;
const ZEND_COMPILE_POSITION_AFTER_OPEN_TAG: i64 = 2;

fn zend_test_compile_string(vm: &mut Vm, args: &[Value]) -> R {
    let source = arg(args, 0).to_php_string();
    let filename = arg(args, 1).to_php_string();
    let position = arg(args, 2).to_long();
    if filename.as_bytes().contains(&0) {
        return throw(
            vm,
            b"ValueError",
            "zend_test_compile_string(): Argument #2 ($filename) must not contain any null bytes",
        );
    }
    let mut src = source.as_bytes().to_vec();
    if position == ZEND_COMPILE_POSITION_AFTER_OPEN_TAG {
        let mut s = b"<?php ".to_vec();
        s.extend_from_slice(&src);
        src = s;
    }
    let file = filename.to_string_lossy();
    crate::support::compile_and_run(
        vm,
        &src,
        &file,
        position == ZEND_COMPILE_POSITION_AT_SHEBANG,
    )?;
    Ok(Value::Null)
}
