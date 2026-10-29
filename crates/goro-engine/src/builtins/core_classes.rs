//! Core classes and interfaces: Traversable & co, Throwable and the
//! exception hierarchy, Closure, Generator, enums' interfaces, attributes,
//! WeakReference / WeakMap. Declarations follow the PHP manual's class
//! synopses.

use crate::class::DisplayName;
use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::calls::{Callee, ClosureData};
use crate::vm::{R, Vm};

const EXCEPTION_BODY: &str = r#"
    protected $message = "";
    private string $string = "";
    protected $code = 0;
    protected string $file = "";
    protected int $line = 0;
    private array $trace = [];
    private ?Throwable $previous = null;
    public function __construct(string $message = "", int $code = 0, ?Throwable $previous = null) {}
    public function __wakeup(): void {}
    final public function getMessage(): string {}
    final public function getCode() {}
    final public function getFile(): string {}
    final public function getLine(): int {}
    final public function getTrace(): array {}
    final public function getPrevious(): ?Throwable {}
    final public function getTraceAsString(): string {}
    public function __toString(): string {}
"#;

macro_rules! exception_class {
    ($name:literal, $parent:literal) => {
        concat!("<?php class ", $name, " extends ", $parent, " {}")
    };
}

pub static MODULE: Module = Module {
    name: "Core",
    classes: &[
        (
            "stdclass",
            "<?php #[AllowDynamicProperties] class stdClass {}",
        ),
        ("traversable", "<?php interface Traversable {}"),
        (
            "iterator",
            "<?php interface Iterator extends Traversable { public function current(): mixed; public function next(): void; public function key(): mixed; public function valid(): bool; public function rewind(): void; }",
        ),
        (
            "iteratoraggregate",
            "<?php interface IteratorAggregate extends Traversable { public function getIterator(): Traversable; }",
        ),
        (
            "arrayaccess",
            "<?php interface ArrayAccess { public function offsetExists(mixed $offset): bool; public function offsetGet(mixed $offset): mixed; public function offsetSet(mixed $offset, mixed $value): void; public function offsetUnset(mixed $offset): void; }",
        ),
        (
            "countable",
            "<?php interface Countable { public function count(): int; }",
        ),
        (
            "stringable",
            "<?php interface Stringable { public function __toString(): string; }",
        ),
        (
            "serializable",
            "<?php interface Serializable { public function serialize(); public function unserialize(string $data); }",
        ),
        (
            "jsonserializable",
            "<?php interface JsonSerializable { public function jsonSerialize(): mixed; }",
        ),
        (
            "unitenum",
            "<?php interface UnitEnum { public static function cases(): array; }",
        ),
        (
            "backedenum",
            "<?php interface BackedEnum extends UnitEnum { public static function from(int|string $value): static; public static function tryFrom(int|string $value): ?static; }",
        ),
        (
            "throwable",
            "<?php interface Throwable extends Stringable { public function getMessage(): string; public function getCode(); public function getFile(): string; public function getLine(): int; public function getTrace(): array; public function getPrevious(): ?Throwable; public function getTraceAsString(): string; }",
        ),
        ("exception", const_format_exception()),
        ("error", const_format_error()),
        (
            "errorexception",
            "<?php class ErrorException extends Exception { protected int $severity = E_ERROR; public function __construct(string $message = \"\", int $code = 0, int $severity = E_ERROR, ?string $filename = null, ?int $line = null, ?Throwable $previous = null) {} final public function getSeverity(): int {} }",
        ),
        ("compileerror", exception_class!("CompileError", "Error")),
        ("parseerror", exception_class!("ParseError", "CompileError")),
        ("typeerror", exception_class!("TypeError", "Error")),
        (
            "argumentcounterror",
            exception_class!("ArgumentCountError", "TypeError"),
        ),
        ("valueerror", exception_class!("ValueError", "Error")),
        (
            "arithmeticerror",
            exception_class!("ArithmeticError", "Error"),
        ),
        (
            "divisionbyzeroerror",
            exception_class!("DivisionByZeroError", "ArithmeticError"),
        ),
        (
            "unhandledmatcherror",
            exception_class!("UnhandledMatchError", "Error"),
        ),
        (
            "assertionerror",
            exception_class!("AssertionError", "Error"),
        ),
        (
            "requestparsingerror",
            exception_class!("RequestParsingError", "Error"),
        ),
        (
            "closure",
            "<?php final class Closure { private function __construct() {} public static function bind(Closure $closure, ?object $newThis, object|string|null $newScope = \"static\"): ?Closure {} public function bindTo(?object $newThis, object|string|null $newScope = \"static\"): ?Closure {} public function call(object $newThis, mixed ...$args): mixed {} public static function fromCallable(callable $callback): Closure {} public function __invoke(mixed ...$args): mixed {} public static function getCurrent(): Closure {} }",
        ),
        (
            "generator",
            "<?php final class Generator implements Iterator { private function __construct() {} public function current(): mixed {} public function key(): mixed {} public function next(): void {} public function rewind(): void {} public function send(mixed $value): mixed {} public function throw(Throwable $exception): mixed {} public function valid(): bool {} public function getReturn(): mixed {} public function __destruct() {} }",
        ),
        ("generatorexit_", "<?php final class GeneratorExit_ {}"),
        (
            "closedgeneratorexception",
            exception_class!("ClosedGeneratorException", "Exception"),
        ),
        (
            "attribute",
            "<?php #[Attribute(Attribute::TARGET_CLASS)] final class Attribute { const TARGET_CLASS = 1; const TARGET_FUNCTION = 2; const TARGET_METHOD = 4; const TARGET_PROPERTY = 8; const TARGET_CLASS_CONSTANT = 16; const TARGET_PARAMETER = 32; const TARGET_CONSTANT = 64; const TARGET_ALL = 127; const IS_REPEATABLE = 128; public int $flags; public function __construct(int $flags = Attribute::TARGET_ALL) { $this->flags = $flags; } }",
        ),
        (
            "returntypewillchange",
            "<?php #[Attribute(Attribute::TARGET_METHOD)] final class ReturnTypeWillChange { public function __construct() {} }",
        ),
        (
            "allowdynamicproperties",
            "<?php #[Attribute(Attribute::TARGET_CLASS)] final class AllowDynamicProperties { public function __construct() {} }",
        ),
        (
            "sensitiveparameter",
            "<?php #[Attribute(Attribute::TARGET_PARAMETER)] final class SensitiveParameter { public function __construct() {} }",
        ),
        (
            "sensitiveparametervalue",
            "<?php final class SensitiveParameterValue { private readonly mixed $value; public function __construct(mixed $value) { $this->value = $value; } public function getValue(): mixed { return $this->value; } public function __debugInfo(): array { return []; } }",
        ),
        (
            "override",
            "<?php #[Attribute(Attribute::TARGET_METHOD)] final class Override { public function __construct() {} }",
        ),
        (
            "deprecated",
            "<?php #[Attribute(Attribute::TARGET_METHOD|Attribute::TARGET_FUNCTION|Attribute::TARGET_CLASS_CONSTANT)] final class Deprecated { public readonly ?string $message; public readonly ?string $since; public function __construct(?string $message = null, ?string $since = null) { $this->message = $message; $this->since = $since; } }",
        ),
        (
            "nodiscard",
            "<?php #[Attribute(Attribute::TARGET_METHOD|Attribute::TARGET_FUNCTION)] final class NoDiscard { public readonly ?string $message; public function __construct(?string $message = null) { $this->message = $message; } }",
        ),
        (
            "weakreference",
            "<?php final class WeakReference { public function __construct() {} public static function create(object $object): WeakReference {} public function get(): ?object {} }",
        ),
        (
            "internaliterator",
            "<?php final class InternalIterator implements Iterator { private function __construct() {} public function current(): mixed {} public function key(): mixed {} public function next(): void {} public function valid(): bool {} public function rewind(): void {} }",
        ),
    ],
    methods: &[
        ("exception::__construct", exc_construct),
        ("exception::__wakeup", exc_wakeup),
        ("exception::getmessage", exc_get_message),
        ("exception::getcode", exc_get_code),
        ("exception::getfile", exc_get_file),
        ("exception::getline", exc_get_line),
        ("exception::gettrace", exc_get_trace),
        ("exception::getprevious", exc_get_previous),
        ("exception::gettraceasstring", exc_get_trace_as_string),
        ("exception::__tostring", exc_to_string),
        ("error::__construct", exc_construct),
        ("error::__wakeup", exc_wakeup),
        ("error::getmessage", exc_get_message),
        ("error::getcode", exc_get_code),
        ("error::getfile", exc_get_file),
        ("error::getline", exc_get_line),
        ("error::gettrace", exc_get_trace),
        ("error::getprevious", exc_get_previous),
        ("error::gettraceasstring", exc_get_trace_as_string),
        ("error::__tostring", exc_to_string),
        ("errorexception::__construct", error_exc_construct),
        ("errorexception::getseverity", error_exc_severity),
        ("closure::bind", closure_bind_static),
        ("closure::bindto", closure_bind_to),
        ("closure::call", closure_call),
        ("closure::fromcallable", closure_from_callable),
        ("closure::__invoke", closure_invoke),
        ("closure::getcurrent", closure_get_current),
        ("generator::current", gen_current),
        ("generator::__destruct", gen_destruct),
        ("generator::key", gen_key),
        ("generator::next", gen_next),
        ("generator::rewind", gen_rewind),
        ("generator::send", gen_send),
        ("generator::throw", gen_throw),
        ("generator::valid", gen_valid),
        ("generator::getreturn", gen_get_return),
        ("weakreference::__construct", weakref_construct),
        ("weakreference::create", weakref_create),
        ("weakreference::get", weakref_get),
    ],
    hooks: &[
        ("exception", exception_hooks),
        ("error", exception_hooks),
        ("closure", closure_hooks),
        ("generator", generator_hooks),
        ("weakreference", weakref_hooks),
    ],
    listed: true,
    ..Module::EMPTY
};

const fn const_format_exception() -> &'static str {
    concat!(
        "<?php class Exception implements Throwable {",
        r#"
    protected $message = "";
    private string $string = "";
    protected $code = 0;
    protected string $file = "";
    protected int $line = 0;
    private array $trace = [];
    private ?Throwable $previous = null;
    public function __construct(string $message = "", int $code = 0, ?Throwable $previous = null) {}
    public function __wakeup(): void {}
    final public function getMessage(): string {}
    final public function getCode() {}
    final public function getFile(): string {}
    final public function getLine(): int {}
    final public function getTrace(): array {}
    final public function getPrevious(): ?Throwable {}
    final public function getTraceAsString(): string {}
    public function __toString(): string {}
"#,
        "}"
    )
}

const fn const_format_error() -> &'static str {
    concat!(
        "<?php class Error implements Throwable {",
        r#"
    protected $message = "";
    private string $string = "";
    protected $code = 0;
    protected string $file = "";
    protected int $line = 0;
    private array $trace = [];
    private ?Throwable $previous = null;
    public function __construct(string $message = "", int $code = 0, ?Throwable $previous = null) {}
    public function __wakeup(): void {}
    final public function getMessage(): string {}
    final public function getCode() {}
    final public function getFile(): string {}
    final public function getLine(): int {}
    final public function getTrace(): array {}
    final public function getPrevious(): ?Throwable {}
    final public function getTraceAsString(): string {}
    public function __toString(): string {}
"#,
        "}"
    )
}

#[allow(dead_code)]
const _UNUSED: &str = EXCEPTION_BODY;

// ---------------------------------------------------------------- exceptions

fn exception_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(exception_create),
        ..Default::default()
    }
}

/// Exceptions record where they were created and the call stack.
fn exception_create(vm: &mut Vm, o: &ObjRef) {
    let file = vm.cur_file();
    let line = vm.cur_line();
    let ignore_args = vm.ini_bool(b"zend.exception_ignore_args", false);
    let frames = vm.backtrace_sealed(!ignore_args);
    let trace = vm.trace_array(&frames, false);
    vm.set_prop_raw(o, b"file", Value::Str(file));
    vm.set_prop_raw(o, b"line", Value::Int(line as i64));
    vm.set_prop_raw(o, b"trace", Value::Array(trace));
}

fn exc_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if a.has(0) {
        vm.set_prop_raw(&o, b"message", a.val(0));
    }
    if a.has(1) {
        vm.set_prop_raw(&o, b"code", a.val(1));
    }
    if a.has(2) && !a.val(2).is_null() {
        vm.set_prop_raw(&o, b"previous", a.val(2));
    }
    Ok(Value::Null)
}

fn exc_wakeup(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}

fn error_exc_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if a.has(0) {
        vm.set_prop_raw(&o, b"message", a.val(0));
    }
    if a.has(1) {
        vm.set_prop_raw(&o, b"code", a.val(1));
    }
    if a.has(2) {
        vm.set_prop_raw(&o, b"severity", a.val(2));
    }
    // A file name given resets the line unless one is given too.
    if a.has(3) && !a.val(3).is_null() {
        vm.set_prop_raw(&o, b"file", a.val(3));
        vm.set_prop_raw(&o, b"line", Value::Int(0));
    }
    if a.has(4) && !a.val(4).is_null() {
        vm.set_prop_raw(&o, b"line", a.val(4));
    }
    if a.has(5) && !a.val(5).is_null() {
        vm.set_prop_raw(&o, b"previous", a.val(5));
    }
    Ok(Value::Null)
}

fn error_exc_severity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"severity"))
}

fn exc_get_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"message"))
}
fn exc_get_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"code"))
}
fn exc_get_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"file"))
}
fn exc_get_line(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"line"))
}
fn exc_get_trace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"trace"))
}
fn exc_get_previous(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(vm.get_prop_raw(a.this_obj(), b"previous"))
}
fn exc_get_trace_as_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = vm.get_prop_raw(a.this_obj(), b"trace");
    let t = match t {
        Value::Array(a) => a,
        _ => PhpArray::new(),
    };
    Ok(Value::Str(PhpStr::from(vm.trace_string(&t))))
}

/// `Exception::__toString()`: the chain from the innermost previous
/// exception ("Next ...").
fn exc_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut chain = Vec::new();
    let mut cur = Some(a.this_obj().clone());
    let mut guard = 0;
    while let Some(o) = cur {
        chain.push(o.clone());
        cur = match vm.get_prop_raw(&o, b"previous") {
            Value::Object(p) => Some(p),
            _ => None,
        };
        guard += 1;
        if guard > 1000 {
            break;
        }
    }
    let mut out = String::new();
    for (i, o) in chain.iter().rev().enumerate() {
        let cls = vm.class_of(o);
        let msg = vm.get_prop_raw(o, b"message");
        let msg = vm.to_php_str(&msg)?.to_string_lossy();
        let file = vm.get_prop_raw(o, b"file");
        let file = file
            .as_str()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let line = match vm.get_prop_raw(o, b"line") {
            Value::Int(n) => n,
            _ => 0,
        };
        let trace = match vm.get_prop_raw(o, b"trace") {
            Value::Array(t) => t,
            _ => PhpArray::new(),
        };
        let ts = vm.trace_string(&trace);
        let defined_in = vm
            .defined_in_errors
            .iter()
            .any(|w| w.upgrade().is_some_and(|x| ObjRef::ptr_eq(&x, o)));
        let sep = if defined_in { "and defined in" } else { "in" };
        let head = if msg.is_empty() {
            format!("{} {} {}:{}", cls.name.dname(), sep, file, line)
        } else {
            format!("{}: {} {} {}:{}", cls.name.dname(), msg, sep, file, line)
        };
        if i == 0 {
            out = format!("{}\nStack trace:\n{}", head, ts);
        } else {
            out = format!("{}\n\nNext {}\nStack trace:\n{}", out, head, ts);
        }
    }
    let s = PhpStr::from(out);
    vm.set_prop_raw(a.this_obj(), b"string", Value::Str(s.clone()));
    Ok(Value::Str(s))
}

// ---------------------------------------------------------------- closures

fn closure_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(closure_clone),
        debug_props: Some(closure_debug),
        compare: Some(closure_compare),
        ..Default::default()
    }
}

/// Two closures are equal when they run the same function with the same
/// `$this`, scope and magic method name (test Zend/tests/closures/
/// closure_compare).
fn closure_compare(vm: &mut Vm, a: &Value, b: &Value) -> Option<R<i32>> {
    let (Value::Object(x), Value::Object(y)) = (a, b) else {
        return None;
    };
    let (cx, cy) = (vm.closure_data(x)?, vm.closure_data(y)?);
    let same_this = match (&cx.this, &cy.this) {
        (Some(p), Some(q)) => ObjRef::ptr_eq(p, q),
        (None, None) => true,
        _ => false,
    };
    let eq = Rc::ptr_eq(&cx.func, &cy.func)
        && same_this
        && std::ptr::eq(cx.scope, cy.scope)
        && std::ptr::eq(cx.static_class, cy.static_class)
        && cx.magic_name == cy.magic_name;
    Some(Ok(if eq { 0 } else { 1 }))
}

fn closure_clone(vm: &mut Vm, from: &ObjRef, to: &ObjRef) {
    if let Some(c) = vm.closure_data(from) {
        let cd = clone_closure_data(from, c);
        *to.obj().native.borrow_mut() = Some(Box::new(cd));
    }
}

fn clone_closure_data(o: &ObjRef, c: Callee) -> ClosureData {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref().unwrap();
    let cd = any.downcast_ref::<ClosureData>().unwrap();
    ClosureData {
        func: c.func,
        this: cd.this.clone(),
        scope: cd.scope,
        called_scope: cd.called_scope,
        captured: cd.captured.clone(),
        statics: std::cell::RefCell::new(cd.statics.borrow().clone()),
        magic_name: cd.magic_name.clone(),
        from_callable: cd.from_callable,
    }
}

/// var_dump of a closure: name, file, line, static (use) variables,
/// $this, parameters (observed on PHP 8.5).
fn closure_debug(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
    let c = vm.closure_data(o)?;
    let mut a = PhpArray::new();
    let real = c
        .func
        .code()
        .is_some_and(|code| code.flags & crate::bytecode::fflags::CLOSURE != 0);
    if !real {
        // Closures of functions and methods: "function", no location
        // (observed on PHP 8.5; tests Zend/tests/first_class_callable/*).
        a.insert(
            Key::from("function"),
            Value::Str(PhpStr::from(c.func.display_name())),
        );
    } else {
        a.insert(
            Key::from("name"),
            Value::Str(c.func.code().unwrap().name.clone()),
        );
    }
    if let Some(code) = c.func.code().filter(|_| real) {
        a.insert(Key::from("file"), Value::Str(code.file.clone()));
        a.insert(Key::from("line"), Value::Int(code.line_start as i64));
        let n = o.obj().native.borrow();
        let any: &dyn std::any::Any = &**n.as_ref().unwrap();
        let cd = any.downcast_ref::<ClosureData>().unwrap();
        let mut st = PhpArray::new();
        for (k, (inner, _, _)) in code.uses.iter().enumerate() {
            let name = code.cv_names[*inner as usize].clone();
            if let Some(Some(v)) = cd.captured.get(k) {
                st.insert(Key::Str(name), v.clone());
            }
        }
        // Static variables, with their initial value before the first call
        // (observed on PHP 8.5).
        for op in &code.ops {
            if let crate::bytecode::Op::BindStatic { var, index, .. } = op {
                let name = code
                    .cv_names
                    .get(*var as usize)
                    .cloned()
                    .unwrap_or_default();
                let cell = cd.statics.borrow().get(*index as usize).cloned().flatten();
                let v = match cell {
                    Some(r) => r.get(),
                    None => code
                        .ops
                        .iter()
                        .find_map(|op| match op {
                            crate::bytecode::Op::StaticInit { index: i, src }
                                if i == index && crate::bytecode::is_const(*src) =>
                            {
                                code.consts.get(crate::bytecode::slot_of(*src)).cloned()
                            }
                            _ => None,
                        })
                        .unwrap_or(Value::Null),
                };
                st.insert(Key::Str(name), v);
            }
        }
        if !st.is_empty() {
            a.insert(Key::from("static"), Value::Array(st));
        }
    }
    if let Some(t) = &c.this {
        a.insert(Key::from("this"), Value::Object(t.clone()));
    }
    let params = vm.params_of(&c.func).to_vec();
    if !params.is_empty() {
        let mut p = PhpArray::new();
        for pi in params {
            let mut k = String::new();
            if pi.by_ref {
                k.push('&');
            }
            k.push('$');
            k.push_str(&pi.name.dname());
            let opt = !matches!(pi.default, crate::bytecode::ParamDefault::None) || pi.variadic;
            p.insert(
                Key::Str(PhpStr::from(k)),
                Value::Str(PhpStr::from(if opt { "<optional>" } else { "<required>" })),
            );
        }
        a.insert(Key::from("parameter"), Value::Array(p));
    }
    Some(a)
}

/// `Closure::bind` / `bindTo`: a copy with another `$this` and scope.
fn do_bind(vm: &mut Vm, cl: &ObjRef, new_this: Value, scope_arg: Option<Value>) -> R<Value> {
    let Some(c) = vm.closure_data(cl) else {
        return Ok(Value::Null);
    };
    let mut cd = clone_closure_data(cl, c.clone());
    let is_static = c
        .func
        .code()
        .is_some_and(|code| code.flags & crate::bytecode::fflags::STATIC_CLOSURE != 0);
    // New scope.
    if let Some(sv) = scope_arg {
        let scope = match &sv {
            Value::Object(o) => Some(vm.class_of(o)),
            Value::Null => None,
            Value::Str(s) if s.as_bytes() == b"static" => {
                if cd.scope.is_null() {
                    None
                } else {
                    Some(vm.rc_class(cd.scope))
                }
            }
            Value::Str(s) => match vm.find_class(s, true)? {
                Some(c) => Some(c),
                None => {
                    vm.warn(&format!("Class \"{}\" not found", s.to_string_lossy()))?;
                    return Ok(Value::Null);
                }
            },
            _ => None,
        };
        if let Some(s) = &scope
            && s.has_flag(crate::class::cflags::INTERNAL)
            && s.lname.as_bytes() != b"closure"
            && !c.func.is_native()
        {
            vm.warn(&format!(
                "Cannot bind closure to scope of internal class {}, this will be an error in PHP 9",
                s.name.dname()
            ))?;
            return Ok(Value::Null);
        }
        let new_scope = scope.as_ref().map(Rc::as_ptr).unwrap_or(std::ptr::null());
        // Closures of functions and methods keep their scope (observed on
        // PHP 8.5; tests Zend/tests/closures/closure_from_callable_rebinding*).
        if cd.from_callable && !std::ptr::eq(new_scope, cd.scope) {
            vm.warn(&format!(
                "Cannot rebind scope of closure created from {}, this will be an error in PHP 9",
                if c.func.scope().is_some() {
                    "method"
                } else {
                    "function"
                }
            ))?;
            return Ok(Value::Null);
        }
        cd.scope = new_scope;
        cd.called_scope = cd.scope;
    }
    match new_this {
        Value::Object(t) => {
            if is_static {
                vm.warn(
                    "Cannot bind an instance to a static closure, this will be an error in PHP 9",
                )?;
                return Ok(Value::Null);
            }
            if c.func.is_native() || cd.from_callable {
                // Method closures can't be rebound to another class.
                if let Some(old) = &cd.this
                    && !Rc::ptr_eq(&vm.class_of(old), &vm.class_of(&t))
                    && !vm.class_of(&t).is_subclass_of(&vm.class_of(old))
                {
                    vm.warn(&format!(
                        "Cannot bind method {}() to object of class {}, this will be an error in PHP 9",
                        c.func.display_name(),
                        vm.class_of(&t).name.dname()
                    ))?;
                    return Ok(Value::Null);
                }
            }
            let tc = vm.class_of(&t);
            if cd.called_scope.is_null() || !tc.is_subclass_of(unsafe { &*cd.called_scope }) {
                cd.called_scope = Rc::as_ptr(&tc);
            }
            if cd.scope.is_null() && c.func.scope().is_none() {
                // keep null scope
            }
            cd.this = Some(t);
        }
        Value::Null => {
            if cd.this.is_some()
                && c.func
                    .code()
                    .is_some_and(|code| code.flags & crate::bytecode::fflags::USES_THIS != 0)
            {
                vm.warn(
                    "Cannot unbind $this of closure using $this, this will be an error in PHP 9",
                )?;
                return Ok(Value::Null);
            }
            if cd.from_callable
                && cd.this.is_some()
                && c.func.scope().is_some()
                && c.func.flags & crate::class::mflags::STATIC == 0
            {
                vm.warn("Cannot unbind $this of method, this will be an error in PHP 9")?;
                return Ok(Value::Null);
            }
            cd.this = None;
        }
        _ => {}
    }
    let o = vm.new_closure_object(cd)?;
    Ok(Value::Object(o))
}

fn closure_bind_static(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(cl) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let scope = if a.has(2) { Some(a.val(2)) } else { None };
    do_bind(vm, &cl, a.val(1), scope)
}

fn closure_bind_to(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cl = a.this_obj().clone();
    let scope = if a.has(1) { Some(a.val(1)) } else { None };
    do_bind(vm, &cl, a.val(0), scope)
}

fn closure_call(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cl = a.this_obj().clone();
    let Some(t) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let Some(mut c) = vm.closure_data(&cl) else {
        return Ok(Value::Null);
    };
    let tc = vm.class_of(&t);
    if c.func
        .code()
        .is_some_and(|code| code.flags & crate::bytecode::fflags::STATIC_CLOSURE != 0)
    {
        vm.warn("Cannot bind an instance to a static closure, this will be an error in PHP 9")?;
        return Ok(Value::Null);
    }
    // test Zend/tests/closures/closure_call
    if tc.has_flag(crate::class::cflags::INTERNAL) && !c.func.is_native() {
        vm.warn(&format!(
            "Cannot bind closure to scope of internal class {}, this will be an error in PHP 9",
            tc.name.dname()
        ))?;
        return Ok(Value::Null);
    }
    c.this = Some(t);
    c.scope = Rc::as_ptr(&tc);
    c.static_class = Rc::as_ptr(&tc);
    let args: Vec<Value> = a.vals[1..].to_vec();
    // Run with the original closure object for its `use` variables.
    vm.call_callee(c, args)
}

fn closure_from_callable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    if let Value::Object(o) = &v
        && vm.closure_data(o).is_some()
    {
        return Ok(v);
    }
    let c = vm.resolve_callable(&v, false)?;
    let o = vm.closure_from_callee(c)?;
    Ok(Value::Object(o))
}

fn closure_invoke(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cl = a.this_obj().clone();
    let Some(c) = vm.closure_data(&cl) else {
        return Ok(Value::Null);
    };
    let args = a.vals.clone();
    vm.call_callee(c, args)
}

fn closure_get_current(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // The calling function only (test Zend/tests/closures/
    // closure_get_current).
    if let Some(f) = vm.ex.frames.last()
        && let Some(c) = &f.closure
    {
        return Ok(Value::Object(c.clone()));
    }
    vm.throw("Error", "Current function is not a closure")
}

// ---------------------------------------------------------------- generators

fn generator_hooks() -> NativeHooks {
    NativeHooks {
        // var_dump() shows the generator's function (observed on PHP 8.5).
        debug_props: Some(|vm, o| {
            let (f, _, _) = vm.generator_func_info(o)?;
            let mut out = PhpArray::new();
            out.insert(
                Key::from("function"),
                Value::Str(PhpStr::from(vm.func_display_name(&f))),
            );
            Some(out)
        }),
        // ...but (array) and var_export() see no properties.
        props_for: Some(|_, _| Some(PhpArray::new())),
        ..NativeHooks::default()
    }
}

/// A suspended generator that goes away runs the `finally` blocks around
/// its paused yield, but no catch blocks (Zend/tests/try/bug71604.phpt).
fn gen_destruct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    let pending = vm
        .with_gen(&g, |d| {
            d.state == crate::vm::iter::GenState::Suspended
                && d.frame.as_ref().is_some_and(|f| {
                    let ip = f.ip.saturating_sub(1);
                    f.code()
                        .try_ranges
                        .iter()
                        .any(|t| t.finally != 0 && t.start <= ip && ip < t.finally)
                })
        })
        .unwrap_or(false);
    if !pending {
        return Ok(Value::Null);
    }
    let cls = vm
        .find_class(b"GeneratorExit_", false)?
        .expect("GeneratorExit_");
    let marker = vm.instantiate_raw(&cls)?;
    vm.with_gen(&g, |d| d.delegate = None);
    match vm.gen_resume(&g, None, Some(marker.clone())) {
        Err(crate::vm::Unwind::Throw(e)) if ObjRef::ptr_eq(&e, &marker) => Ok(Value::Null),
        Err(e) => Err(e),
        Ok(()) => Ok(Value::Null),
    }
}

fn gen_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    let (_, v) = vm.gen_current(&g)?;
    Ok(v.into_deref())
}
fn gen_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    let (k, _) = vm.gen_current(&g)?;
    Ok(k)
}
fn gen_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    vm.gen_next(&g)?;
    Ok(Value::Null)
}
fn gen_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    vm.gen_rewind(&g)?;
    Ok(Value::Null)
}
fn gen_send(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    vm.gen_send(&g, a.val(0))
}
fn gen_throw(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    let Some(e) = a.obj(0) else {
        return Ok(Value::Null);
    };
    vm.gen_throw(&g, e)
}
fn gen_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    Ok(Value::Bool(vm.gen_valid(&g)?))
}
fn gen_get_return(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = a.this_obj().clone();
    vm.gen_return_value(&g)
}

// ---------------------------------------------------------------- WeakReference

struct WeakData(Option<goro_runtime::WeakObj>);
impl goro_runtime::NativeData for WeakData {}

fn weakref_hooks() -> NativeHooks {
    NativeHooks {
        debug_props: Some(|_, o| {
            let n = o.obj().native.borrow();
            let any: &dyn std::any::Any = &**n.as_ref()?;
            let w = any.downcast_ref::<WeakData>()?;
            let target = w.0.as_ref().and_then(|w| w.upgrade());
            let mut out = PhpArray::new();
            out.insert(
                Key::from("object"),
                target.map(Value::Object).unwrap_or(Value::Null),
            );
            Some(out)
        }),
        ..Default::default()
    }
}

fn weakref_construct(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw(
        "Error",
        "Direct instantiation of WeakReference is not allowed, use WeakReference::create instead",
    )
}

fn weakref_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    // The same WeakReference object while one exists.
    let h = o.handle();
    if let Some(w) = vm.weakrefs.get(&h)
        && let Some(existing) = w.1.upgrade()
        && w.0.upgrade().is_some_and(|t| ObjRef::ptr_eq(&t, &o))
    {
        return Ok(Value::Object(existing));
    }
    let cls = vm.find_class(b"WeakReference", false)?.unwrap();
    let info: Rc<dyn goro_runtime::ClassInfo> = cls;
    let r = ObjRef::new(info, Vec::new());
    *r.obj().native.borrow_mut() = Some(Box::new(WeakData(Some(o.downgrade()))));
    vm.weakrefs.insert(h, (o.downgrade(), r.downgrade()));
    Ok(Value::Object(r))
}

fn weakref_get(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj();
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref().unwrap();
    let w = any.downcast_ref::<WeakData>().unwrap();
    Ok(match w.0.as_ref().and_then(|w| w.upgrade()) {
        Some(o) => Value::Object(o),
        None => Value::Null,
    })
}
