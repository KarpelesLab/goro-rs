//! The bcmath extension (PHP manual: BCMath Arbitrary Precision
//! Mathematics): the `bc*()` functions, the `bcmath.scale` setting and the
//! `BcMath\Number` class with its operators. The arithmetic, scale rules
//! and messages live in the goro-ext-bcmath crate; this module wires them
//! to PHP values.
//!
//! Behaviour sources: the PHP manual, the RFC "Support object type in
//! BCMath", and the expected output of ext/bcmath/tests/*.phpt.

use std::rc::Rc;

use goro_ext_bcmath as bc;
use goro_ext_bcmath::{BcError, BcErrorKind, Number, NumberArg, RoundingMode};
use goro_runtime::ops::BinOp;
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::bytecode::CastTo;
use crate::class::{Class, NativeHooks, UnaryOp};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "bcmath",
    functions: &[
        (
            "function bcadd(string $num1, string $num2, ?int $scale = null): string",
            bcadd,
        ),
        (
            "function bcsub(string $num1, string $num2, ?int $scale = null): string",
            bcsub,
        ),
        (
            "function bcmul(string $num1, string $num2, ?int $scale = null): string",
            bcmul,
        ),
        (
            "function bcdiv(string $num1, string $num2, ?int $scale = null): string",
            bcdiv,
        ),
        (
            "function bcmod(string $num1, string $num2, ?int $scale = null): string",
            bcmod,
        ),
        (
            "function bcdivmod(string $num1, string $num2, ?int $scale = null): array",
            bcdivmod,
        ),
        (
            "function bcpow(string $num, string $exponent, ?int $scale = null): string",
            bcpow,
        ),
        (
            "function bcpowmod(string $num, string $exponent, string $modulus, ?int $scale = null): string",
            bcpowmod,
        ),
        (
            "function bcsqrt(string $num, ?int $scale = null): string",
            bcsqrt,
        ),
        (
            "function bccomp(string $num1, string $num2, ?int $scale = null): int",
            bccomp,
        ),
        ("function bcscale(?int $scale = null): int", bcscale),
        ("function bcfloor(string $num): string", bcfloor),
        ("function bcceil(string $num): string", bcceil),
        (
            "function bcround(string $num, int $precision = 0, RoundingMode $mode = RoundingMode::HalfAwayFromZero): string",
            bcround,
        ),
    ],
    classes: &[(
        "bcmath\\number",
        r#"<?php namespace BcMath; final readonly class Number implements \Stringable {
    public string $value;
    public int $scale;
    public function __construct(string|int $num) {}
    public function add(mixed $num, ?int $scale = null): Number {}
    public function sub(mixed $num, ?int $scale = null): Number {}
    public function mul(mixed $num, ?int $scale = null): Number {}
    public function div(mixed $num, ?int $scale = null): Number {}
    public function mod(mixed $num, ?int $scale = null): Number {}
    public function divmod(mixed $num, ?int $scale = null): array {}
    public function powmod(mixed $exponent, mixed $modulus, ?int $scale = null): Number {}
    public function pow(mixed $exponent, ?int $scale = null): Number {}
    public function sqrt(?int $scale = null): Number {}
    public function floor(): Number {}
    public function ceil(): Number {}
    public function round(int $precision = 0, \RoundingMode $mode = \RoundingMode::HalfAwayFromZero): Number {}
    public function compare(mixed $num, ?int $scale = null): int {}
    public function __toString(): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
}"#,
    )],
    methods: &[
        ("bcmath\\number::__construct", num_construct),
        ("bcmath\\number::add", num_add),
        ("bcmath\\number::sub", num_sub),
        ("bcmath\\number::mul", num_mul),
        ("bcmath\\number::div", num_div),
        ("bcmath\\number::mod", num_mod),
        ("bcmath\\number::divmod", num_divmod),
        ("bcmath\\number::powmod", num_powmod),
        ("bcmath\\number::pow", num_pow),
        ("bcmath\\number::sqrt", num_sqrt),
        ("bcmath\\number::floor", num_floor),
        ("bcmath\\number::ceil", num_ceil),
        ("bcmath\\number::round", num_round),
        ("bcmath\\number::compare", num_compare),
        ("bcmath\\number::__tostring", num_to_string),
        ("bcmath\\number::__serialize", num_serialize),
        ("bcmath\\number::__unserialize", num_unserialize),
    ],
    hooks: &[("bcmath\\number", number_hooks)],
    // PHP manual: BCMath runtime configuration.
    ini: &[("bcmath.scale", "0")],
    on_ini_set: Some(ini_set_hook),
    ..Module::EMPTY
};

// ---------------------------------------------------------------- state

/// The parsed `bcmath.scale`, cached by its ini text.
#[derive(Default)]
struct ScaleCache {
    text: Option<PhpStr>,
    scale: u32,
}

/// The default scale (`bcmath.scale`).
fn default_scale(vm: &mut Vm) -> u32 {
    let text = vm.get_ini(b"bcmath.scale").unwrap_or_default();
    let c = vm.ext::<ScaleCache>();
    if c.text.as_ref() == Some(&text) {
        return c.scale;
    }
    let s = bc::parse_scale_ini(text.as_bytes()).scale.unwrap_or(0);
    c.text = Some(text);
    c.scale = s;
    s
}

/// `ini_set('bcmath.scale', v)`: refused (false) outside 0..=2147483647,
/// with the ini quantity warnings (observed on PHP 8.5).
fn ini_set_hook(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    if k != b"bcmath.scale" {
        return Ok(true);
    }
    let r = bc::parse_scale_ini(v);
    if let Some(w) = &r.warning {
        vm.warn(w)?;
    }
    Ok(r.scale.is_some())
}

/// Bounds result sizes by `memory_limit` (test gh17398.phpt).
fn set_limit(vm: &Vm) {
    let l = vm.ini.memory_limit;
    let cap = if l < 0 { 1usize << 40 } else { l as usize };
    bc::set_max_result_len(cap);
}

fn bc_err(vm: &mut Vm, e: BcError) -> Unwind {
    if e.kind == BcErrorKind::OutOfMemory {
        let l = vm.ini.memory_limit;
        return vm.fatal(&format!(
            "Allowed memory size of {} bytes exhausted (tried to allocate {} bytes)",
            if l < 0 { 1u64 << 40 } else { l as u64 },
            e.bytes
        ));
    }
    vm.throw_error_msg(e.class_name(), &e.message)
}

fn br<T>(vm: &mut Vm, r: Result<T, BcError>) -> R<T> {
    r.map_err(|e| bc_err(vm, e))
}

fn sv(b: Vec<u8>) -> Value {
    Value::Str(PhpStr::from(b))
}

// ---------------------------------------------------------------- functions

type Two = fn(&mut Vec<u8>, &[u8], &[u8], Option<i64>, u32) -> Result<(), BcError>;

fn two(vm: &mut Vm, a: &Args, f: Two) -> R<Value> {
    let ds = default_scale(vm);
    set_limit(vm);
    let mut out = Vec::new();
    let r = f(&mut out, a.bytes(0), a.bytes(1), a.opt_int(2), ds);
    br(vm, r).map(|_| sv(out))
}

fn bcadd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    two(vm, a, bc::bcadd_into)
}
fn bcsub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    two(vm, a, bc::bcsub_into)
}
fn bcmul(vm: &mut Vm, a: &mut Args) -> R<Value> {
    two(vm, a, bc::bcmul_into)
}
fn bcdiv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    two(vm, a, bc::bcdiv_into)
}
fn bcmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    two(vm, a, bc::bcmod_into)
}
fn bcpow(vm: &mut Vm, a: &mut Args) -> R<Value> {
    two(vm, a, bc::bcpow_into)
}

fn bcdivmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ds = default_scale(vm);
    set_limit(vm);
    let r = bc::bcdivmod(a.bytes(0), a.bytes(1), a.opt_int(2), ds);
    let (q, m) = br(vm, r)?;
    Ok(Value::Array(PhpArray::from_values([sv(q), sv(m)])))
}

fn bcpowmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ds = default_scale(vm);
    set_limit(vm);
    let mut out = Vec::new();
    let r = bc::bcpowmod_into(
        &mut out,
        a.bytes(0),
        a.bytes(1),
        a.bytes(2),
        a.opt_int(3),
        ds,
    );
    br(vm, r).map(|_| sv(out))
}

fn bcsqrt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ds = default_scale(vm);
    set_limit(vm);
    let mut out = Vec::new();
    let r = bc::bcsqrt_into(&mut out, a.bytes(0), a.opt_int(1), ds);
    br(vm, r).map(|_| sv(out))
}

fn bccomp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ds = default_scale(vm);
    let r = bc::bccomp(a.bytes(0), a.bytes(1), a.opt_int(2), ds);
    br(vm, r).map(Value::Int)
}

fn bcscale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = default_scale(vm);
    let r = bc::bcscale(a.opt_int(0));
    if let Some(new) = br(vm, r)? {
        vm.set_ini(b"bcmath.scale", new.to_string().as_bytes());
    }
    Ok(Value::Int(old as i64))
}

fn bcfloor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut out = Vec::new();
    let r = bc::bcfloor_into(&mut out, a.bytes(0));
    br(vm, r).map(|_| sv(out))
}

fn bcceil(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut out = Vec::new();
    let r = bc::bcceil_into(&mut out, a.bytes(0));
    br(vm, r).map(|_| sv(out))
}

/// The `RoundingMode` case of an enum argument.
fn rounding_mode(vm: &mut Vm, v: &Value) -> RoundingMode {
    if let Value::Object(o) = v {
        let n = vm.get_prop_raw(o, b"name");
        if let Value::Str(s) = n
            && let Some(m) = RoundingMode::from_name(&s.to_string_lossy())
        {
            return m;
        }
    }
    RoundingMode::HalfAwayFromZero
}

fn bcround(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = rounding_mode(vm, &a.val(2));
    set_limit(vm);
    let mut out = Vec::new();
    let r = bc::bcround_into(&mut out, a.bytes(0), a.int_or(1, 0), mode);
    br(vm, r).map(|_| sv(out))
}

// ---------------------------------------------------------------- BcMath\Number

struct NumObj(Number);

impl goro_runtime::NativeData for NumObj {}

fn peek(o: &ObjRef) -> Option<Number> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<NumObj>().map(|d| d.0.clone())
}

fn is_number(o: &ObjRef) -> bool {
    let n = o.obj().native.borrow();
    n.as_ref()
        .is_some_and(|b| (&**b as &dyn std::any::Any).is::<NumObj>())
}

fn number_class(vm: &mut Vm) -> R<Rc<Class>> {
    match vm.find_class(b"BcMath\\Number", false)? {
        Some(c) => Ok(c),
        None => Err(vm.fatal("Class \"BcMath\\Number\" not found")),
    }
}

/// Stores `n` in `o` (its native value and readonly properties).
fn store(vm: &mut Vm, o: &ObjRef, n: Number) {
    vm.set_prop_raw(o, b"value", Value::Str(PhpStr::from(n.value())));
    vm.set_prop_raw(o, b"scale", Value::Int(n.scale()));
    *o.obj().native.borrow_mut() = Some(Box::new(NumObj(n)));
}

fn new_number(vm: &mut Vm, n: Number) -> R<Value> {
    let cls = number_class(vm)?;
    let o = vm.instantiate_raw(&cls)?;
    store(vm, &o, n);
    Ok(Value::Object(o))
}

fn this_num(vm: &mut Vm, a: &Args) -> R<Number> {
    match peek(a.this_obj()) {
        Some(n) => Ok(n),
        None => vm.throw(
            "Error",
            "Typed property BcMath\\Number::$value must not be accessed before initialization",
        ),
    }
}

/// An operand of a method (`BcMath\Number|string|int`), owned.
enum Operand {
    Int(i64),
    Str(PhpStr),
    Num(Number),
}

impl Operand {
    fn arg(&self) -> NumberArg<'_> {
        match self {
            Operand::Int(i) => NumberArg::Int(*i),
            Operand::Str(s) => NumberArg::Str(s.as_bytes()),
            Operand::Num(n) => NumberArg::Number(n),
        }
    }
}

fn operand(v: &Value) -> Operand {
    match v {
        Value::Int(i) => Operand::Int(*i),
        Value::Object(o) => match peek(o) {
            Some(n) => Operand::Num(n),
            None => Operand::Str(PhpStr::empty()),
        },
        Value::Str(s) => Operand::Str(s.clone()),
        _ => Operand::Str(PhpStr::empty()),
    }
}

/// A `BcMath\Number|string|int` parameter, declared `mixed` so that the
/// type error carries PHP's wording ("must be of type int, string, or
/// BcMath\Number"; tests number/methods/calc_methods_num_arg_error.phpt,
/// compare_arg_error.phpt, powmod_arg_error.phpt). Null and floats coerce
/// as for a `Number|string|int` parameter.
fn num_param(vm: &mut Vm, a: &Args, i: usize, pname: &str) -> R<Operand> {
    let v = a.val(i);
    Ok(match &v {
        Value::Null => {
            vm.fdeprecated(
                a,
                &format!(
                    "Passing null to parameter #{} (${pname}) of type BcMath\\Number|string|int is deprecated",
                    i + 1
                ),
            )?;
            Operand::Str(PhpStr::empty())
        }
        Value::Object(o) if is_number(o) => operand(&v),
        Value::Int(_) | Value::Str(_) | Value::Float(_) | Value::Bool(_) => {
            match op_operand(vm, &v)? {
                Some(o) => o,
                None => Operand::Str(PhpStr::empty()),
            }
        }
        _ => {
            let given = type_desc(vm, &v);
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #{} (${pname}) must be of type int, string, or BcMath\\Number, {given} given",
                    i + 1
                ),
            );
        }
    })
}

fn already_constructed(vm: &mut Vm, o: &ObjRef) -> Option<Unwind> {
    if is_number(o) {
        return Some(vm.throw_error_msg(
            "Error",
            "Cannot modify readonly property BcMath\\Number::$value",
        ));
    }
    None
}

fn num_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some(e) = already_constructed(vm, &o) {
        return Err(e);
    }
    let op = operand(&a.val(0));
    let n = br(vm, Number::new(op.arg()))?;
    store(vm, &o, n);
    Ok(Value::Null)
}

type Method = fn(&Number, NumberArg, Option<i64>) -> Result<Number, BcError>;

fn method(vm: &mut Vm, a: &Args, f: Method, pname: &str) -> R<Value> {
    let n = this_num(vm, a)?;
    let op = num_param(vm, a, 0, pname)?;
    set_limit(vm);
    let r = f(&n, op.arg(), a.opt_int(1));
    let r = br(vm, r)?;
    new_number(vm, r)
}

fn num_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    method(vm, a, Number::add, "num")
}
fn num_sub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    method(vm, a, Number::sub, "num")
}
fn num_mul(vm: &mut Vm, a: &mut Args) -> R<Value> {
    method(vm, a, Number::mul, "num")
}
fn num_div(vm: &mut Vm, a: &mut Args) -> R<Value> {
    method(vm, a, Number::div, "num")
}
fn num_mod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    method(vm, a, Number::modulo, "num")
}
fn num_pow(vm: &mut Vm, a: &mut Args) -> R<Value> {
    method(vm, a, Number::pow, "exponent")
}

fn num_divmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let op = num_param(vm, a, 0, "num")?;
    set_limit(vm);
    let r = n.divmod(op.arg(), a.opt_int(1));
    let (q, m) = br(vm, r)?;
    let q = new_number(vm, q)?;
    let m = new_number(vm, m)?;
    Ok(Value::Array(PhpArray::from_values([q, m])))
}

fn num_powmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let e = num_param(vm, a, 0, "exponent")?;
    let m = num_param(vm, a, 1, "modulus")?;
    set_limit(vm);
    let r = n.powmod(e.arg(), m.arg(), a.opt_int(2));
    let r = br(vm, r)?;
    new_number(vm, r)
}

fn num_sqrt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    set_limit(vm);
    let r = n.sqrt(a.opt_int(0));
    let r = br(vm, r)?;
    new_number(vm, r)
}

fn num_floor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let r = br(vm, n.floor())?;
    new_number(vm, r)
}

fn num_ceil(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let r = br(vm, n.ceil())?;
    new_number(vm, r)
}

fn num_round(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let mode = rounding_mode(vm, &a.val(1));
    set_limit(vm);
    let r = n.round(a.int_or(0, 0), mode);
    let r = br(vm, r)?;
    new_number(vm, r)
}

fn num_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let op = num_param(vm, a, 0, "num")?;
    let r = n.compare(op.arg(), a.opt_int(1));
    br(vm, r).map(Value::Int)
}

fn num_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    Ok(Value::Str(PhpStr::from(n.value())))
}

fn num_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_num(vm, a)?;
    let mut out = PhpArray::new();
    out.insert(
        Key::Str(PhpStr::from("value")),
        Value::Str(PhpStr::from(n.serialize_value())),
    );
    Ok(Value::Array(out))
}

fn num_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if let Some(e) = already_constructed(vm, &o) {
        return Err(e);
    }
    let data = a.array(0);
    let v = data.get_bytes(b"value").map(|v| v.deref());
    let s = match &v {
        Some(Value::Str(s)) => Some(s.as_bytes()),
        _ => None,
    };
    let n = br(vm, Number::unserialize(s))?;
    store(vm, &o, n);
    Ok(Value::Null)
}

// ---------------------------------------------------------------- hooks

fn number_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|_, from, to| {
            if let Some(n) = peek(from) {
                *to.obj().native.borrow_mut() = Some(Box::new(NumObj(n)));
            }
        }),
        compare: Some(number_compare),
        cast: Some(|_, o, to| match to {
            CastTo::Bool => peek(o).map(|n| Ok(Value::Bool(n.to_bool()))),
            _ => None,
        }),
        do_operation: Some(number_op),
        do_unary: Some(number_unary),
        ..Default::default()
    }
}

/// The operand type as "Unsupported operand types" names it.
fn type_desc(vm: &Vm, v: &Value) -> String {
    match v {
        Value::Object(o) => vm.class_of(o).name.to_string_lossy(),
        Value::Null => "null".into(),
        Value::Bool(_) => "bool".into(),
        Value::Int(_) => "int".into(),
        Value::Float(_) => "float".into(),
        Value::Str(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Resource(_) => "resource".into(),
        Value::Ref(r) => type_desc(vm, &r.get()),
    }
}

/// An operator operand: ints, strings and Numbers as they are; floats
/// and bools as ints (with the float-to-int deprecation); `None` for the
/// other types.
fn op_operand(vm: &mut Vm, v: &Value) -> R<Option<Operand>> {
    Ok(Some(match v {
        Value::Int(i) => Operand::Int(*i),
        Value::Str(s) => Operand::Str(s.clone()),
        Value::Bool(b) => Operand::Int(*b as i64),
        Value::Float(f) => {
            let i = *f as i64;
            if i as f64 != *f {
                vm.deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_runtime::convert::float_repr(*f)
                ))?;
            }
            Operand::Int(i)
        }
        Value::Object(o) => match peek(o) {
            Some(n) => Operand::Num(n),
            None => return Ok(None),
        },
        _ => return Ok(None),
    }))
}

fn number_op(vm: &mut Vm, op: BinOp, x: &Value, y: &Value) -> Option<R<Value>> {
    let bop = match op {
        BinOp::Add => bc::BinOp::Add,
        BinOp::Sub => bc::BinOp::Sub,
        BinOp::Mul => bc::BinOp::Mul,
        BinOp::Div => bc::BinOp::Div,
        BinOp::Mod => bc::BinOp::Mod,
        BinOp::Pow => bc::BinOp::Pow,
        _ => return None,
    };
    let x = x.deref();
    let y = y.deref();
    let is_num = |v: &Value| matches!(v, Value::Object(o) if is_number(o));
    if !is_num(&x) && !is_num(&y) {
        return None;
    }
    Some((|| {
        let unsupported = |vm: &mut Vm| -> R<Value> {
            let m = format!(
                "Unsupported operand types: {} {} {}",
                type_desc(vm, &x),
                op.symbol(),
                type_desc(vm, &y)
            );
            vm.throw("TypeError", &m)
        };
        let (Some(a), Some(b)) = (op_operand(vm, &x)?, op_operand(vm, &y)?) else {
            return unsupported(vm);
        };
        set_limit(vm);
        let r = Number::binary_op(bop, a.arg(), b.arg());
        let r = br(vm, r)?;
        new_number(vm, r)
    })())
}

fn number_unary(vm: &mut Vm, op: UnaryOp, o: &ObjRef) -> Option<R<Value>> {
    let n = peek(o)?;
    let r = match op {
        UnaryOp::Neg => Ok(n.negate()),
        UnaryOp::Inc => n.increment(),
        UnaryOp::Dec => n.decrement(),
        UnaryOp::BitNot => return None,
    };
    Some(br(vm, r).and_then(|r| new_number(vm, r)))
}

/// `<=>` with a Number on one side: ints, strings and Numbers compare by
/// value, a malformed string is uncomparable (1, so every ordered
/// comparison is false), as are arrays, other objects and resources; null
/// and bools follow the engine's object rules (test
/// number/operators/compare_with_invalid_types.phpt).
fn number_compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    let x = x.deref();
    let y = y.deref();
    let side = |v: &Value| -> Option<Option<Operand>> {
        match v {
            Value::Null | Value::Bool(_) => None,
            Value::Int(i) => Some(Some(Operand::Int(*i))),
            Value::Str(s) => Some(Some(Operand::Str(s.clone()))),
            Value::Float(f) if f.fract() == 0.0 && f.is_finite() => {
                Some(Some(Operand::Int(*f as i64)))
            }
            Value::Object(o) => Some(peek(o).map(Operand::Num)),
            _ => Some(None),
        }
    };
    let a = side(&x)?;
    let b = side(&y)?;
    let _ = vm;
    let (Some(a), Some(b)) = (a, b) else {
        return Some(Ok(1));
    };
    Some(Ok(match Number::compare_operands(a.arg(), b.arg()) {
        Some(std::cmp::Ordering::Less) => -1,
        Some(std::cmp::Ordering::Equal) => 0,
        Some(std::cmp::Ordering::Greater) => 1,
        None => 1,
    }))
}
