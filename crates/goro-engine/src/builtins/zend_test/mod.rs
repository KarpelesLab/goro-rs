//! An emulation of php-src's `zend_test` extension, the engine's test
//! helper, for the php-src tests that need it.
//!
//! Test-only: the module exists only when a startup setting asks for it
//! (`-d extension=zend_test`, as for the shared module), which the phpt
//! runner passes to tests listing zend_test in EXTENSIONS (or checking for
//! it in SKIPIF). Without it `extension_loaded('zend_test')` is false and
//! none of this is defined.
//!
//! Clean room: everything here is written from the expected output of the
//! php-src tests that use zend_test (ext/zend_test/tests/*.phpt and the
//! Zend/tests, ext/*/tests that list it), each function citing its tests.
//! Behaviour that would need engine internals goro doesn't have (the
//! observer API's output, string UTF-8 flags, the memory manager, opcache)
//! is not emulated: those tests fail honestly.

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpStr, Value, WeakObj};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

/// Whether an `extension=` setting names this module (`zend_test`,
/// `zend_test.so`, `php_zend_test.dll`, or a path to one).
pub fn is_requested(v: &[u8]) -> bool {
    let base = v.rsplit(|&c| c == b'/').next().unwrap_or(v);
    let base = base.strip_prefix(b"php_").unwrap_or(base);
    let base = base
        .strip_suffix(b".so")
        .or_else(|| base.strip_suffix(b".dll"))
        .unwrap_or(base);
    base.eq_ignore_ascii_case(b"zend_test")
}

const CLASSES: &str = include_str!("classes.php");

/// Namespaced classes (test ext/zend_test/tests/gen_stub_test_01.phpt).
const NS_CLASSES: &str = r#"<?php
namespace ZendTestNS {
    class Foo {
        public function method(): int { return 0; }
    }
    class UnlikelyCompileError {
        public static function method(): ?UnlikelyCompileError { return null; }
    }
    class NotUnlikelyCompileError {
        public static function method(): ?NotUnlikelyCompileError { return null; }
    }
}
namespace ZendTestNS2 {
    class Foo {
        public ZendSubNS\Foo $foo;
        public function method(): void {}
    }
}
namespace ZendTestNS2\ZendSubNS {
    class Foo {
        public function method(): void {}
    }
}
"#;

/// Functions declared with attributes or doc comments, which native
/// signatures can't carry: compiled from PHP at startup.
const FUNCTIONS: &str = r#"<?php
#[\Deprecated]
function zend_test_deprecated(mixed $arg = null): void {}

#[\Deprecated(message: "custom message")]
function zend_test_deprecated_attr(): void {}

#[\NoDiscard(message: "custom message")]
function zend_test_nodiscard(): int { return 1; }

#[\Deprecated(message: "custom message")]
#[\NoDiscard(message: "custom message 2")]
function zend_test_deprecated_nodiscard(): int { return 1; }

function zend_test_parameter_with_attribute(#[ZendTestParameterAttribute("value1")] string $parameter): int { return 1; }

#[ZendTestAttributeWithArguments(arg: "foo")]
function zend_test_attribute_with_named_argument(): void {}

/**
     * "Lorem ipsum"
     * @see https://www.php.net
     * @since 8.3
     */
function zend_test_nullable_array_return(): ?array { return null; }

#[\Deprecated]
function __zend_test_const_deprecated() {}

#[\Deprecated(since: "version 1.5", message: "use something else")]
function __zend_test_const_attributed() {}
"#;

macro_rules! c {
    ($l:literal) => {
        ($l, CLASSES)
    };
}
macro_rules! ns {
    ($l:literal) => {
        ($l, NS_CLASSES)
    };
}

pub static MODULE: Module = Module {
    name: "zend_test",
    functions: &[
        ("function zend_test_array_return(): array", array_return),
        ("function zend_test_void_return(): void", null_return),
        (
            "function zend_test_compile_string(string $source_string, string $filename, int $position): void",
            compile_string,
        ),
        (
            "function zend_create_unterminated_string(string $str): string",
            same_string,
        ),
        (
            "function zend_terminate_string(string &$str): void",
            null_return,
        ),
        (
            "function zend_leak_variable(mixed $variable): void",
            null_return,
        ),
        (
            "function zend_leak_bytes(int $bytes = 3): void",
            null_return,
        ),
        (
            "function zend_string_or_object(mixed $param): object|string",
            string_or_object,
        ),
        (
            "function zend_string_or_object_or_null(mixed $param): object|string|null",
            string_or_object_or_null,
        ),
        (
            "function zend_string_or_stdclass(mixed $param): stdClass|string",
            string_or_stdclass,
        ),
        (
            "function zend_string_or_stdclass_or_null(mixed $param): stdClass|string|null",
            string_or_stdclass_or_null,
        ),
        (
            "function zend_number_or_string(mixed $param): string|int|float",
            number_or_string,
        ),
        (
            "function zend_number_or_string_or_null(mixed $param): string|int|float|null",
            number_or_string_or_null,
        ),
        (
            "function zend_iterable(mixed $arg1, mixed $arg2 = null): void",
            iterable,
        ),
        (
            "function zend_iterable_legacy(iterable $arg1, ?iterable $arg2 = null): iterable",
            first_arg,
        ),
        (
            "function zend_weakmap_attach(object $object, mixed $value): bool",
            weakmap_attach,
        ),
        (
            "function zend_weakmap_remove(object $object): bool",
            weakmap_remove,
        ),
        ("function zend_weakmap_dump(): array", weakmap_dump),
        (
            "function zend_get_unit_enum(): ZendTestUnitEnum",
            get_unit_enum,
        ),
        (
            "function zend_get_current_func_name(): string",
            current_func_name,
        ),
        (
            "function zend_call_method(object|string $obj_or_class, string $method, mixed $arg1 = UNKNOWN, mixed $arg2 = UNKNOWN): mixed",
            call_method,
        ),
        (
            "function zend_call_method_if_exists(object $obj, string $method, mixed ...$args): mixed",
            call_method_if_exists,
        ),
        (
            "function zend_object_init_with_constructor(string $class, mixed ...$args): mixed",
            object_init_with_constructor,
        ),
        (
            "function zend_test_zend_ini_parse_quantity(string $str): int",
            ini_parse_quantity,
        ),
        (
            "function zend_test_zend_ini_parse_uquantity(string $str): int",
            ini_parse_uquantity,
        ),
        ("function zend_test_zend_ini_str(): string", ini_str),
        ("function zend_test_is_pcre_bundled(): bool", false_return),
        (
            "function zend_test_set_fmode(bool $binary): void",
            null_return,
        ),
        ("function zend_test_cast_fread($stream): void", cast_fread),
        (
            "function zend_test_fill_packed_array(array &$array): void",
            fill_packed_array,
        ),
        (
            "function zend_test_is_zend_ptr(int $addr): bool",
            false_return,
        ),
        (
            "function zend_test_log_err_debug(string $str): void",
            null_return,
        ),
        ("function zend_test_gh18756(): void", null_return),
        (
            "function zend_test_opcache_preloading(): bool",
            false_return,
        ),
        ("function zend_trigger_bailout(): never", trigger_bailout),
        (
            "function zend_test_internal_dnf_arguments(Iterator|(Traversable&Countable) $arg): Iterator|(Traversable&Countable)",
            first_arg,
        ),
        (
            "function zend_test_zend_call_stack_get(): ?array",
            call_stack_get,
        ),
        (
            "function zend_test_compile_to_ast(string $str): string",
            compile_to_ast,
        ),
    ],
    classes: &[
        c!("_zendtestinterface"),
        c!("_zendtestclass"),
        c!("_zendtestchildclass"),
        c!("_zendtesttrait"),
        c!("_zendtestmagiccall"),
        c!("_zendtestmagiccallforward"),
        c!("zendtestattribute"),
        c!("zendtestattributewitharguments"),
        c!("zendtestrepeatableattribute"),
        c!("zendtestparameterattribute"),
        c!("zendtestpropertyattribute"),
        c!("zendtestclasswithmethodwithparameterattribute"),
        c!("zendtestchildclasswithmethodwithparameterattribute"),
        c!("zendtestclasswithpropertyattribute"),
        c!("zendattributetest"),
        c!("zendtestforbiddynamiccall"),
        c!("zendtestunitenum"),
        c!("zendteststringenum"),
        c!("zendtestintenum"),
        c!("zendtestenumwithinterface"),
        c!("dooperationnocast"),
        c!("longcastablenooperations"),
        c!("floatcastablenooperations"),
        c!("numericcastablenooperations"),
        ns!("zendtestns\\foo"),
        ns!("zendtestns\\unlikelycompileerror"),
        ns!("zendtestns\\notunlikelycompileerror"),
        ns!("zendtestns2\\foo"),
        ns!("zendtestns2\\zendsubns\\foo"),
    ],
    methods: &[("_zendtestmagiccallforward::__call", magic_call_forward)],
    hooks: &[
        ("dooperationnocast", do_operation_hooks),
        ("longcastablenooperations", cast_hooks),
        ("floatcastablenooperations", cast_hooks),
        ("numericcastablenooperations", numeric_hooks),
    ],
    // The order ReflectionExtension::getConstants() shows (test
    // ext/zend_test/tests/gh11423.phpt).
    constants: &[
        ("ZEND_TEST_DEPRECATED", ConstVal::Int(42)),
        ("ZEND_CONSTANT_A", ConstVal::Str("global")),
        ("ZEND_TEST_ATTRIBUTED_CONSTANT", ConstVal::Int(42)),
        ("ZendTestNS2\\ZEND_CONSTANT_A", ConstVal::Str("namespaced")),
        (
            "ZendTestNS2\\ZendSubNS\\ZEND_CONSTANT_A",
            ConstVal::Str("namespaced"),
        ),
    ],
    ini: &[
        ("zend_test.replace_zend_execute_ex", "0"),
        ("zend_test.register_passes", "0"),
        ("zend_test.print_stderr_mshutdown", "0"),
        ("zend_test.limit_copy_file_range", "-1"),
        ("zend_test.quantity_value", "0"),
        ("zend_test.str_test", ""),
        ("zend_test.not_empty_str_test", "val"),
        ("zend_test.observe_opline_in_zendmm", "0"),
        ("zend_test.observer.enabled", "0"),
        ("zend_test.observer.show_output", "1"),
        ("zend_test.observer.observe_all", "0"),
        ("zend_test.observer.observe_includes", "0"),
        ("zend_test.observer.observe_functions", "0"),
        ("zend_test.observer.observe_declaring", "0"),
        ("zend_test.observer.observe_function_names", ""),
        ("zend_test.observer.show_return_type", "0"),
        ("zend_test.observer.show_return_value", "0"),
        ("zend_test.observer.show_init_backtrace", "0"),
        ("zend_test.observer.show_opcode", "0"),
        ("zend_test.observer.show_opcode_in_user_handler", ""),
        ("zend_test.observer.fiber_init", "0"),
        ("zend_test.observer.fiber_switch", "0"),
        ("zend_test.observer.fiber_destroy", "0"),
        ("zend_test.observer.execute_internal", "0"),
    ],
    init: Some(init),
    shutdown: Some(shutdown),
    startup_ini: Some(startup_ini),
    on_ini_set: Some(on_ini_set),
    ..Module::EMPTY
};

/// Declares the PHP-written functions and the attributes of the
/// deprecated constants.
fn init(vm: &mut Vm) {
    let unit = match vm.compile_source(FUNCTIONS.as_bytes(), b"", false) {
        Ok(u) => u,
        Err(e) => panic!("zend_test function stubs: {:?}", e.message),
    };
    let main = unit.main.clone();
    vm.units.push(main.clone());
    for &i in &unit.hoisted_funcs {
        let code = main.children[i as usize].clone();
        let lname = crate::class::lower(&code.name);
        let attrs = code.attrs.clone();
        // Constant attributes ride on helper functions that are not
        // declared themselves.
        let konst: &[u8] = match lname.as_bytes() {
            b"__zend_test_const_deprecated" => b"ZEND_TEST_DEPRECATED",
            b"__zend_test_const_attributed" => b"ZEND_TEST_ATTRIBUTED_CONSTANT",
            _ => {
                let f = std::rc::Rc::new(crate::func::Func::user(code, 0));
                vm.functions.insert(lname, f);
                continue;
            }
        };
        vm.const_attrs.insert(PhpStr::from(konst), attrs);
    }
}

/// `zend_test.print_stderr_mshutdown` (test ext/zend_test/tests/gh8575.phpt).
fn shutdown(vm: &mut Vm) {
    if vm
        .get_ini(b"zend_test.print_stderr_mshutdown")
        .is_some_and(|v| super::misc::assert_ini_truthy(&v))
    {
        vm.write_stderr(b"[zend_test] MSHUTDOWN\n");
    }
}

/// Startup diagnostics of quantity settings (test Zend/tests/zend_ini/
/// zend_ini_parse_quantity_ini_setting_error.phpt).
fn startup_ini(vm: &mut Vm, ini: &[(Vec<u8>, Vec<u8>)]) {
    for (k, v) in ini {
        if k == b"zend_test.quantity_value"
            && let (_, Some(w)) = crate::vm::setup::parse_quantity(v)
        {
            let _ = vm.warn(&format!(
                "Invalid \"zend_test.quantity_value\" setting. {}",
                w
            ));
        }
    }
}

fn on_ini_set(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    match k {
        // OnUpdateStrNotEmpty (test zend_ini_str_validator_basic.phpt).
        b"zend_test.not_empty_str_test" => Ok(!v.is_empty()),
        // Test Zend/tests/zend_ini/zend_ini_parse_quantity_ini_set_error.
        b"zend_test.quantity_value" => {
            if let (_, Some(w)) = crate::vm::setup::parse_quantity(v) {
                vm.warn(&format!(
                    "Invalid \"zend_test.quantity_value\" setting. {}",
                    w
                ))?;
            }
            Ok(true)
        }
        // Startup-only settings (test ext/zend_test/tests/gh16414.phpt).
        _ if k.starts_with(b"zend_test.observer.") => Ok(false),
        _ => Ok(true),
    }
}

fn null_return(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}

fn false_return(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}

fn array_return(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::new()))
}

fn first_arg(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(a.val(0))
}

fn same_string(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(a.str(0)))
}

// ------------------------------------------------------------ ZPP types

/// What a parameter accepts, for the hand-written argument checks of the
/// functions testing PHP's parameter parsing macros (their messages name
/// the types in the macros' order: tests Zend/tests/str_or_obj_zpp.phpt,
/// str_or_obj_of_class_zpp.phpt, number_or_str_zpp.phpt,
/// iterable_or_null.phpt).
fn arg_type_error<T>(vm: &mut Vm, a: &Args, n: usize, name: &str, ty: &str) -> R<T> {
    let given = crate::vm::Vm::type_name_given(&a.val(n));
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type {}, {} given",
        a.fname(),
        n + 1,
        name,
        ty,
        given
    );
    vm.type_error(&msg)
}

fn null_deprecated(vm: &mut Vm, a: &Args, ty: &str) -> R<()> {
    let msg = format!(
        "Passing null to parameter #1 ($param) of type {} is deprecated",
        ty
    );
    vm.fdeprecated(a, &msg)
}

/// A string from a scalar or a Stringable object; `None` for other values.
fn scalar_string(vm: &mut Vm, v: &Value) -> R<Option<Value>> {
    Ok(match v {
        Value::Str(_) => Some(v.clone()),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => Some(Value::Str(vm.to_php_str(v)?)),
        _ => None,
    })
}

fn string_or_object_impl(vm: &mut Vm, a: &Args, nullable: bool) -> R<Value> {
    let v = a.val(0);
    let ty = if nullable {
        "object|string|null"
    } else {
        "object|string"
    };
    match &v {
        Value::Object(_) => Ok(v),
        Value::Null if nullable => Ok(Value::Null),
        Value::Null => {
            null_deprecated(vm, a, ty)?;
            Ok(Value::Str(PhpStr::empty()))
        }
        _ => match scalar_string(vm, &v)? {
            Some(s) => Ok(s),
            None => arg_type_error(vm, a, 0, "param", ty),
        },
    }
}

fn string_or_object(vm: &mut Vm, a: &mut Args) -> R<Value> {
    string_or_object_impl(vm, a, false)
}

fn string_or_object_or_null(vm: &mut Vm, a: &mut Args) -> R<Value> {
    string_or_object_impl(vm, a, true)
}

fn string_or_stdclass_impl(vm: &mut Vm, a: &Args, nullable: bool) -> R<Value> {
    let v = a.val(0);
    let ty = if nullable {
        "stdClass|string|null"
    } else {
        "stdClass|string"
    };
    match &v {
        Value::Object(o) if vm.class_of(o).lname.as_bytes() == b"stdclass" => Ok(v.clone()),
        Value::Object(o) if vm.class_of(o).find_method(b"__tostring").is_some() => {
            Ok(Value::Str(vm.to_php_str(&v)?))
        }
        Value::Null if nullable => Ok(Value::Null),
        Value::Null => {
            // The deprecation names the string part only (observed in
            // test Zend/tests/str_or_obj_of_class_zpp.phpt).
            null_deprecated(vm, a, "string")?;
            Ok(Value::Str(PhpStr::empty()))
        }
        _ => match scalar_string(vm, &v)? {
            Some(s) => Ok(s),
            None => arg_type_error(vm, a, 0, "param", ty),
        },
    }
}

fn string_or_stdclass(vm: &mut Vm, a: &mut Args) -> R<Value> {
    string_or_stdclass_impl(vm, a, false)
}

fn string_or_stdclass_or_null(vm: &mut Vm, a: &mut Args) -> R<Value> {
    string_or_stdclass_impl(vm, a, true)
}

fn number_or_string_impl(vm: &mut Vm, a: &Args, nullable: bool) -> R<Value> {
    let v = a.val(0);
    let ty = if nullable {
        "string|int|float|null"
    } else {
        "string|int|float"
    };
    match &v {
        Value::Str(_) | Value::Int(_) | Value::Float(_) => Ok(v),
        Value::Bool(b) => Ok(Value::Int(*b as i64)),
        Value::Null if nullable => Ok(Value::Null),
        Value::Null => {
            null_deprecated(vm, a, ty)?;
            Ok(Value::Int(0))
        }
        Value::Object(o) if vm.class_of(o).find_method(b"__tostring").is_some() => {
            Ok(Value::Str(vm.to_php_str(&v)?))
        }
        _ => arg_type_error(vm, a, 0, "param", ty),
    }
}

fn number_or_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    number_or_string_impl(vm, a, false)
}

fn number_or_string_or_null(vm: &mut Vm, a: &mut Args) -> R<Value> {
    number_or_string_impl(vm, a, true)
}

fn is_iterable(vm: &mut Vm, v: &Value) -> bool {
    match v {
        Value::Array(_) => true,
        Value::Object(o) => {
            let cls = vm.class_of(o);
            vm.instance_of_lname(&cls, b"traversable")
        }
        _ => false,
    }
}

fn iterable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    if !is_iterable(vm, &v) {
        return arg_type_error(vm, a, 0, "arg1", "Traversable|array");
    }
    if a.has(1) {
        let w = a.val(1);
        if !matches!(w, Value::Null) && !is_iterable(vm, &w) {
            return arg_type_error(vm, a, 1, "arg2", "Traversable|array|null");
        }
    }
    Ok(Value::Null)
}

// ------------------------------------------------------------ weak map

/// The extension's own weak map (test ext/zend_test/tests/zend_weakmap.phpt,
/// Zend/tests/gh19543-*.phpt).
#[derive(Default)]
struct WeakMapState {
    entries: Vec<(WeakObj, Value)>,
}

fn weakmap_prune(vm: &mut Vm) {
    let st = vm.ext::<WeakMapState>();
    st.entries.retain(|(w, _)| w.upgrade().is_some());
}

fn weakmap_find(vm: &mut Vm, o: &ObjRef) -> Option<usize> {
    weakmap_prune(vm);
    let st = vm.ext::<WeakMapState>();
    st.entries
        .iter()
        .position(|(w, _)| w.upgrade().is_some_and(|k| ObjRef::ptr_eq(&k, o)))
}

fn weakmap_attach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    if weakmap_find(vm, &o).is_some() {
        return Ok(Value::Bool(false));
    }
    let v = a.val(1);
    vm.ext::<WeakMapState>().entries.push((o.downgrade(), v));
    Ok(Value::Bool(true))
}

fn weakmap_remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    match weakmap_find(vm, &o) {
        Some(i) => {
            let old = vm.ext::<WeakMapState>().entries.remove(i);
            drop(old);
            Ok(Value::Bool(true))
        }
        None => Ok(Value::Bool(false)),
    }
}

fn weakmap_dump(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    weakmap_prune(vm);
    let mut out = PhpArray::new();
    for (w, v) in &vm.ext::<WeakMapState>().entries {
        out.insert(Key::Int(w.addr() as i64), v.clone());
    }
    Ok(Value::Array(out))
}

// ------------------------------------------------------------ calls

fn get_unit_enum(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let Some(cls) = vm.find_class(b"ZendTestUnitEnum", false)? else {
        return Ok(Value::Null);
    };
    vm.enum_case(&cls, &PhpStr::from("Foo"))
}

/// The function running the caller's code; "main" for top-level code of
/// a file (test ext/zend_test/tests/get_function_or_method_name_01.phpt).
fn current_func_name(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let bt = vm.backtrace(false);
    let name = match bt.get(1) {
        Some(f)
            if !matches!(
                f.function.as_bytes(),
                b"include" | b"include_once" | b"require" | b"require_once" | b"eval"
            ) =>
        {
            match &f.class {
                Some(c) => format!("{}::{}", c.to_string_lossy(), f.function.to_string_lossy()),
                None => f.function.to_string_lossy(),
            }
        }
        _ => "main".to_string(),
    };
    Ok(Value::from(name.as_str()))
}

/// Calls a method of an object or class with up to two arguments (test
/// Zend/tests/.../internal-call-internal-static-return.phpt).
fn call_method(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let target = a.val(0);
    let method = a.val(1);
    let mut args = Vec::new();
    for i in 2..a.len().min(4) {
        args.push(a.val(i));
    }
    let callable = Value::Array(PhpArray::from_values([target, method]));
    vm.call_value(&callable, args)
}

/// Calls a public method (or one visible from the caller's scope), or
/// `__call` for a missing one; NULL when there is none (tests ext/
/// zend_test/tests/zend_call_method_if_exists/*.phpt).
fn call_method_if_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let name = a.str(1);
    let cls = vm.class_of(&o);
    let lname = crate::class::lower(&name);
    if cls.find_method(&lname).is_none() && cls.magic.borrow().call.is_none() {
        return Ok(Value::Null);
    }
    let callable = Value::Array(PhpArray::from_values([
        Value::Object(o.clone()),
        Value::Str(name),
    ]));
    if !vm.is_callable_value(&callable, false) {
        return Ok(Value::Null);
    }
    let args: Vec<Value> = (2..a.len()).map(|i| a.val(i)).collect();
    vm.call_value(&callable, args)
}

/// `new $class(...$args)` from internal code: errors as `new` gives them
/// from the global scope (tests ext/zend_test/tests/
/// zend_object_init_with_constructor*.phpt).
fn object_init_with_constructor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    let Some(cls) = vm.find_class(&name, true)? else {
        return vm.throw(
            "Error",
            &format!("Class \"{}\" not found", name.to_string_lossy()),
        );
    };
    let o = vm.instantiate(&cls)?;
    let mut args = PhpArray::new();
    for i in 1..a.len() {
        let _ = args.push(a.val(i));
    }
    if let Some(named) = &a.named {
        for (k, v) in named.iter() {
            args.insert(k.to_owned(), v.clone());
        }
    }
    let ctor = cls.magic.borrow().construct.clone();
    let Some(f) = ctor else {
        if let Some((KeyRef::Str(k), _)) = args.iter().find(|(k, _)| matches!(k, KeyRef::Str(_))) {
            o.mark_destructor_called();
            return vm.throw(
                "Error",
                &format!("Unknown named parameter ${}", k.to_string_lossy()),
            );
        }
        return Ok(Value::Object(o));
    };
    if !vm.method_visible(&f, std::ptr::null()) {
        o.mark_destructor_called();
        let vis = if f.flags & crate::class::mflags::PRIVATE != 0 {
            "private"
        } else {
            "protected"
        };
        return vm.throw(
            "Error",
            &format!(
                "Call to {} {}::__construct() from global scope",
                vis,
                cls.name.to_string_lossy()
            ),
        );
    }
    let callee = crate::vm::calls::Callee {
        scope: f.scope.get(),
        func: f,
        this: Some(o.clone()),
        static_class: std::rc::Rc::as_ptr(&cls),
        closure: None,
        magic_name: None,
    };
    match vm.call_callee_named(callee, args) {
        Ok(_) => Ok(Value::Object(o)),
        Err(e) => {
            o.mark_destructor_called();
            Err(e)
        }
    }
}

/// `_ZendTestMagicCallForward::__call()` calls the function of that name
/// (test ext/zend_test/tests/gh16233.phpt).
fn magic_call_forward(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.val(0);
    let args: Vec<Value> = match a.val(1) {
        Value::Array(arr) => arr.iter().map(|(_, v)| v.clone()).collect(),
        _ => Vec::new(),
    };
    vm.call_value(&name, args)
}

/// Fatal error "Bailout" (test ext/standard/tests/streams/
/// user-stream-open-bailout.phpt).
fn trigger_bailout(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Err(vm.fatal("Bailout"))
}

// ------------------------------------------------------------ ini

fn ini_parse_quantity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (n, w) = crate::vm::setup::parse_quantity(&a.str(0));
    if let Some(w) = w {
        vm.warn(&w)?;
    }
    Ok(Value::Int(n))
}

/// The unsigned variant: a negative quantity other than -1 (all bits set)
/// is out of range (test Zend/tests/zend_ini/
/// zend_ini_parse_uquantity_overflow.phpt).
fn ini_parse_uquantity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let t = s.trim_ascii();
    if let Some(rest) = t.strip_prefix(b"-") {
        let (n, w) = crate::vm::setup::parse_quantity(rest);
        if let Some(w) = w {
            vm.warn(&w.replace(
                &format!("\"{}\"", String::from_utf8_lossy(rest)),
                &format!("\"{}\"", String::from_utf8_lossy(&s)),
            ))?;
        } else if n > 1 {
            vm.warn(&format!(
                "Invalid quantity \"{}\": value is out of range, using overflow result for backwards compatibility",
                String::from_utf8_lossy(&s)
            ))?;
        }
        return Ok(Value::Int(n.wrapping_neg()));
    }
    ini_parse_quantity(vm, a)
}

/// The value of zend_test.str_test (test
/// zend_ini_str_validator_return_from_func.phpt).
fn ini_str(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Str(
        vm.get_ini(b"zend_test.str_test").unwrap_or_default(),
    ))
}

// ------------------------------------------------------------ misc

/// Reads the whole stream (test ext/zend_test/tests/gh11078.phpt).
fn cast_fread(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.val(0);
    vm.call_value(&Value::from("stream_get_contents"), vec![s])?;
    Ok(Value::Null)
}

/// Appends 0..9 (test ext/zend_test/tests/hash_fill_packed_nr_elements.phpt).
fn fill_packed_array(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(r) = a.ref_at(0) else {
        return Ok(Value::Null);
    };
    let mut arr = match r.get() {
        Value::Array(arr) => arr,
        _ => PhpArray::new(),
    };
    for i in 0..10 {
        let _ = arr.push(Value::Int(i));
    }
    r.set(Value::Array(arr));
    Ok(Value::Null)
}

/// The native stack of the running thread (tests Zend/tests/stack_limit/*:
/// keys and hex formatting).
fn call_stack_get(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let marker = 0u8;
    let pos = &marker as *const u8 as usize;
    let Some((base, size)) = thread_stack() else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    let hex = |n: usize| Value::from(format!("0x{:x}", n).as_str());
    out.insert(Key::from("base"), hex(base));
    out.insert(Key::from("max_size"), hex(size));
    out.insert(Key::from("position"), hex(pos));
    out.insert(Key::from("EG(stack_limit)"), hex(base.saturating_sub(size)));
    Ok(Value::Array(out))
}

/// (top of the stack, size) of the current thread.
fn thread_stack() -> Option<(usize, usize)> {
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let ok = libc::pthread_attr_getstack(&attr, &mut addr, &mut size) == 0;
        libc::pthread_attr_destroy(&mut attr);
        ok.then(|| (addr as usize + size, size))
    }
}

/// Runs `$source_string` as the file `$filename`: from a shebang line
/// (skipped), from an open tag (inline HTML first) or after the open tag
/// (tests ext/zend_test/tests/zend_test_compile_string.phpt, gh17797).
fn compile_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    let file = a.str(1);
    if file.contains(&0) {
        return vm.value_error(&format!(
            "{}(): Argument #2 ($filename) must not contain any null bytes",
            a.fname()
        ));
    }
    let pos = a.int(2);
    let code: Vec<u8> = match pos {
        0 => {
            let s: &[u8] = &src;
            if s.starts_with(b"#!") {
                match s.iter().position(|&c| c == b'\n') {
                    Some(i) => s[i + 1..].to_vec(),
                    None => Vec::new(),
                }
            } else {
                s.to_vec()
            }
        }
        2 => {
            let mut v = b"<?php ".to_vec();
            v.extend_from_slice(&src);
            v
        }
        _ => src.to_vec(),
    };
    vm.run_front_file(&code, &file)?;
    Ok(Value::Null)
}

/// The code back from its AST (tests ext/zend_test/tests/compile_to_ast/*,
/// Zend/tests/attributes/constants/ast_export.phpt).
fn compile_to_ast(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    let arena = goro_syntax::Arena::new();
    match goro_syntax::parse(&arena, &src) {
        Ok(prog) => {
            let mut x = goro_syntax::export::Exporter::new(&src);
            x.stmts(prog.stmts);
            Ok(Value::Str(PhpStr::from(x.finish())))
        }
        Err(e) => vm.throw("ParseError", &e.to_string()),
    }
}

// ------------------------------------------------------------ operators

/// The `val` of a test object, or the operand itself.
fn operand(vm: &Vm, v: &Value) -> Value {
    match v {
        Value::Object(o) => vm.get_prop_raw(o, b"val"),
        _ => v.clone(),
    }
}

fn new_with_val(vm: &mut Vm, class: &[u8], val: Value) -> R<Value> {
    let Some(cls) = vm.find_class(class, false)? else {
        return Ok(Value::Null);
    };
    let o = vm.instantiate(&cls)?;
    vm.set_prop_raw(&o, b"val", val);
    Ok(Value::Object(o))
}

fn arith(vm: &mut Vm, op: goro_runtime::ops::BinOp, a: &Value, b: &Value) -> R<Value> {
    use crate::bytecode::Bin;
    use goro_runtime::ops::BinOp;
    let a = operand(vm, a);
    let b = operand(vm, b);
    let bin = match op {
        BinOp::Add => Bin::Add,
        BinOp::Sub => Bin::Sub,
        BinOp::Mul => Bin::Mul,
        BinOp::Div => Bin::Div,
        BinOp::Mod => Bin::Mod,
        BinOp::Pow => Bin::Pow,
        BinOp::Concat => Bin::Concat,
        BinOp::BitAnd => Bin::BitAnd,
        BinOp::BitOr => Bin::BitOr,
        BinOp::BitXor => Bin::BitXor,
        BinOp::Shl => Bin::Shl,
        BinOp::Shr => Bin::Shr,
    };
    vm.binary_op(bin, &a, &b)
}

/// DoOperationNoCast: arithmetic gives a new object, there is no cast
/// (tests ext/zend_test/tests/do_operation_not_cast.phpt, ext/standard/
/// tests/array/array_*_objects_operation_no_cast.phpt).
fn do_operation_hooks() -> NativeHooks {
    NativeHooks {
        do_operation: Some(|vm, op, a, b| {
            Some(arith(vm, op, a, b).and_then(|v| new_with_val(vm, b"DoOperationNoCast", v)))
        }),
        ..NativeHooks::default()
    }
}

fn cast_val(vm: &mut Vm, o: &ObjRef, to: crate::bytecode::CastTo) -> Option<R<Value>> {
    use crate::bytecode::CastTo;
    let v = vm.get_prop_raw(o, b"val");
    match to {
        CastTo::Int => Some(Ok(Value::Int(match v {
            Value::Float(f) => f as i64,
            Value::Int(i) => i,
            _ => 0,
        }))),
        CastTo::Float => Some(Ok(Value::Float(match v {
            Value::Float(f) => f,
            Value::Int(i) => i as f64,
            _ => 0.0,
        }))),
        _ => None,
    }
}

/// Castable to a number, no operators (tests Zend/tests/in-de-crement/
/// *_with_castable_objects_no_*.phpt).
fn cast_hooks() -> NativeHooks {
    NativeHooks {
        cast: Some(cast_val),
        ..NativeHooks::default()
    }
}

/// Castable to a number, which arithmetic and `++`/`--` use.
fn numeric_hooks() -> NativeHooks {
    use crate::class::UnaryOp;
    NativeHooks {
        cast: Some(cast_val),
        do_operation: Some(|vm, op, a, b| Some(arith(vm, op, a, b))),
        do_unary: Some(|vm, op, o| {
            let v = vm.get_prop_raw(o, b"val");
            let r = match op {
                UnaryOp::Inc => vm.binary_op(crate::bytecode::Bin::Add, &v, &Value::Int(1)),
                UnaryOp::Dec => vm.binary_op(crate::bytecode::Bin::Sub, &v, &Value::Int(1)),
                _ => return None,
            };
            Some(r)
        }),
        ..NativeHooks::default()
    }
}
