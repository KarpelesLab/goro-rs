//! The gmp extension (PHP manual: GMP): the `gmp_*()` functions, the
//! `GMP_*` constants and the `GMP` class with its operators, comparisons,
//! casts and serialization. The arithmetic, argument rules and messages
//! live in the goro-ext-gmp crate; this module wires them to PHP values.
//!
//! `GMP|string|int` parameters are declared `mixed`: the crate applies
//! PHP's conversion rules and messages to the raw value (see its wiring
//! notes).
//!
//! Behaviour sources: the PHP manual and the expected output of
//! ext/gmp/tests/*.phpt.

use std::rc::Rc;

use goro_ext_gmp as g;
use goro_ext_gmp::{Env, Gmp, GmpError, GmpErrorKind, Num};
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::bytecode::CastTo;
use crate::class::{Class, NativeHooks, UnaryOp};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "gmp",
    functions: &[
        (
            "function gmp_init(mixed $num, int $base = 0): GMP",
            gmp_init,
        ),
        (
            "function gmp_import(string $data, int $word_size = 1, int $flags = GMP_MSW_FIRST | GMP_NATIVE_ENDIAN): GMP",
            gmp_import,
        ),
        (
            "function gmp_export(mixed $num, int $word_size = 1, int $flags = GMP_MSW_FIRST | GMP_NATIVE_ENDIAN): string",
            gmp_export,
        ),
        ("function gmp_intval(mixed $num): int", gmp_intval),
        (
            "function gmp_strval(mixed $num, int $base = 10): string",
            gmp_strval,
        ),
        ("function gmp_add(mixed $num1, mixed $num2): GMP", gmp_add),
        ("function gmp_sub(mixed $num1, mixed $num2): GMP", gmp_sub),
        ("function gmp_mul(mixed $num1, mixed $num2): GMP", gmp_mul),
        (
            "function gmp_div_qr(mixed $num1, mixed $num2, int $rounding_mode = GMP_ROUND_ZERO): array",
            gmp_div_qr,
        ),
        (
            "function gmp_div_q(mixed $num1, mixed $num2, int $rounding_mode = GMP_ROUND_ZERO): GMP",
            gmp_div_q,
        ),
        (
            "function gmp_div_r(mixed $num1, mixed $num2, int $rounding_mode = GMP_ROUND_ZERO): GMP",
            gmp_div_r,
        ),
        (
            "function gmp_div(mixed $num1, mixed $num2, int $rounding_mode = GMP_ROUND_ZERO): GMP",
            gmp_div,
        ),
        ("function gmp_mod(mixed $num1, mixed $num2): GMP", gmp_mod),
        (
            "function gmp_divexact(mixed $num1, mixed $num2): GMP",
            gmp_divexact,
        ),
        ("function gmp_neg(mixed $num): GMP", gmp_neg),
        ("function gmp_abs(mixed $num): GMP", gmp_abs),
        ("function gmp_fact(mixed $num): GMP", gmp_fact),
        ("function gmp_sqrt(mixed $num): GMP", gmp_sqrt),
        ("function gmp_sqrtrem(mixed $num): array", gmp_sqrtrem),
        ("function gmp_root(mixed $num, int $nth): GMP", gmp_root),
        (
            "function gmp_rootrem(mixed $num, int $nth): array",
            gmp_rootrem,
        ),
        ("function gmp_pow(mixed $num, int $exponent): GMP", gmp_pow),
        (
            "function gmp_powm(mixed $num, mixed $exponent, mixed $modulus): GMP",
            gmp_powm,
        ),
        (
            "function gmp_perfect_square(mixed $num): bool",
            gmp_perfect_square,
        ),
        (
            "function gmp_perfect_power(mixed $num): bool",
            gmp_perfect_power,
        ),
        (
            "function gmp_prob_prime(mixed $num, int $repetitions = 10): int",
            gmp_prob_prime,
        ),
        ("function gmp_gcd(mixed $num1, mixed $num2): GMP", gmp_gcd),
        (
            "function gmp_gcdext(mixed $num1, mixed $num2): array",
            gmp_gcdext,
        ),
        ("function gmp_lcm(mixed $num1, mixed $num2): GMP", gmp_lcm),
        (
            "function gmp_invert(mixed $num1, mixed $num2): GMP|false",
            gmp_invert,
        ),
        (
            "function gmp_jacobi(mixed $num1, mixed $num2): int",
            gmp_jacobi,
        ),
        (
            "function gmp_legendre(mixed $num1, mixed $num2): int",
            gmp_legendre,
        ),
        (
            "function gmp_kronecker(mixed $num1, mixed $num2): int",
            gmp_kronecker,
        ),
        ("function gmp_cmp(mixed $num1, mixed $num2): int", gmp_cmp),
        ("function gmp_sign(mixed $num): int", gmp_sign),
        (
            "function gmp_random_seed(mixed $seed): void",
            gmp_random_seed,
        ),
        ("function gmp_random_bits(int $bits): GMP", gmp_random_bits),
        (
            "function gmp_random_range(mixed $min, mixed $max): GMP",
            gmp_random_range,
        ),
        ("function gmp_and(mixed $num1, mixed $num2): GMP", gmp_and),
        ("function gmp_or(mixed $num1, mixed $num2): GMP", gmp_or),
        ("function gmp_com(mixed $num): GMP", gmp_com),
        ("function gmp_xor(mixed $num1, mixed $num2): GMP", gmp_xor),
        (
            "function gmp_setbit(mixed $num, int $index, bool $value = true): void",
            gmp_setbit,
        ),
        (
            "function gmp_clrbit(mixed $num, int $index): void",
            gmp_clrbit,
        ),
        (
            "function gmp_testbit(mixed $num, int $index): bool",
            gmp_testbit,
        ),
        (
            "function gmp_scan0(mixed $num1, int $start): int",
            gmp_scan0,
        ),
        (
            "function gmp_scan1(mixed $num1, int $start): int",
            gmp_scan1,
        ),
        ("function gmp_popcount(mixed $num): int", gmp_popcount),
        (
            "function gmp_hamdist(mixed $num1, mixed $num2): int",
            gmp_hamdist,
        ),
        ("function gmp_nextprime(mixed $num): GMP", gmp_nextprime),
        ("function gmp_binomial(mixed $n, int $k): GMP", gmp_binomial),
    ],
    classes: &[(
        "gmp",
        r#"<?php final class GMP {
    public function __construct(int|string $num = 0, int $base = 0) {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
}"#,
    )],
    methods: &[
        ("gmp::__construct", gmp_construct),
        ("gmp::__serialize", gmp_serialize),
        ("gmp::__unserialize", gmp_unserialize),
    ],
    hooks: &[("gmp", gmp_hooks)],
    constants: &[
        ("GMP_ROUND_ZERO", ConstVal::Int(g::GMP_ROUND_ZERO)),
        ("GMP_ROUND_PLUSINF", ConstVal::Int(g::GMP_ROUND_PLUSINF)),
        ("GMP_ROUND_MINUSINF", ConstVal::Int(g::GMP_ROUND_MINUSINF)),
        ("GMP_MSW_FIRST", ConstVal::Int(g::GMP_MSW_FIRST)),
        ("GMP_LSW_FIRST", ConstVal::Int(g::GMP_LSW_FIRST)),
        ("GMP_LITTLE_ENDIAN", ConstVal::Int(g::GMP_LITTLE_ENDIAN)),
        ("GMP_BIG_ENDIAN", ConstVal::Int(g::GMP_BIG_ENDIAN)),
        ("GMP_NATIVE_ENDIAN", ConstVal::Int(g::GMP_NATIVE_ENDIAN)),
        ("GMP_VERSION", ConstVal::Str(g::GMP_VERSION)),
    ],
    ..Module::EMPTY
};

// ---------------------------------------------------------------- values

/// A `GMP|string|int` value taken out of a PHP value (GMP objects cloned,
/// so that no borrow of the object is held while the operation runs).
enum Owned {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(PhpStr),
    Gmp(Gmp),
    Other(String),
}

impl Owned {
    fn num(&self) -> Num<'_> {
        match self {
            Owned::Null => Num::Null,
            Owned::Bool(b) => Num::Bool(*b),
            Owned::Int(i) => Num::Int(*i),
            Owned::Float(f) => Num::Float(*f),
            Owned::Str(s) => Num::Str(s.as_bytes()),
            Owned::Gmp(g) => Num::Gmp(g),
            Owned::Other(t) => Num::Other(t),
        }
    }
}

fn owned(vm: &Vm, v: &Value) -> Owned {
    match v {
        Value::Null => Owned::Null,
        Value::Bool(b) => Owned::Bool(*b),
        Value::Int(i) => Owned::Int(*i),
        Value::Float(f) => Owned::Float(*f),
        Value::Str(s) => Owned::Str(s.clone()),
        Value::Object(o) => match peek(o) {
            Some(g) => Owned::Gmp(g),
            None => Owned::Other(vm.class_of(o).name.to_string_lossy()),
        },
        Value::Array(_) => Owned::Other("array".into()),
        Value::Resource(_) => Owned::Other("resource".into()),
        Value::Ref(r) => owned(vm, &r.get()),
    }
}

fn arg(vm: &Vm, a: &Args, i: usize) -> Owned {
    owned(vm, &a.val(i))
}

fn peek(o: &ObjRef) -> Option<Gmp> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<Gmp>().cloned()
}

fn is_gmp(o: &ObjRef) -> bool {
    let n = o.obj().native.borrow();
    n.as_ref()
        .is_some_and(|b| (&**b as &dyn std::any::Any).is::<Gmp>())
}

fn gmp_class(vm: &mut Vm) -> R<Rc<Class>> {
    match vm.find_class(b"GMP", false)? {
        Some(c) => Ok(c),
        None => Err(vm.fatal("Class \"GMP\" not found")),
    }
}

fn new_gmp(vm: &mut Vm, v: Gmp) -> R<Value> {
    let cls = gmp_class(vm)?;
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(v));
    Ok(Value::Object(o))
}

fn gmp_err(vm: &mut Vm, e: GmpError) -> Unwind {
    if e.kind == GmpErrorKind::OutOfMemory {
        return vm.fatal(&e.message);
    }
    vm.throw_error_msg(e.class_name(), &e.message)
}

/// Bounds result sizes by `memory_limit`.
fn set_limit(vm: &Vm) {
    let l = vm.ini.memory_limit;
    let bits = if l < 0 { 1u64 << 35 } else { (l as u64) * 8 };
    g::set_max_bits(bits);
}

/// Runs a crate function with the calling context, then surfaces any
/// diagnostic the error handler turned into an exception.
fn run<T>(vm: &mut Vm, f: impl FnOnce(&mut Env) -> Result<T, GmpError>) -> R<T> {
    set_limit(vm);
    let strict = vm.caller_strict();
    let r = {
        let mut env = Env::new(vm, strict);
        f(&mut env)
    };
    vm.check_deferred()?;
    r.map_err(|e| gmp_err(vm, e))
}

fn pair(vm: &mut Vm, (x, y): (Gmp, Gmp)) -> R<Value> {
    let x = new_gmp(vm, x)?;
    let y = new_gmp(vm, y)?;
    Ok(Value::Array(PhpArray::from_values([x, y])))
}

// ---------------------------------------------------------------- functions

macro_rules! one_gmp {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            let r = run(vm, |e| g::$name(e, x.num()))?;
            new_gmp(vm, r)
        }
    )*};
}
one_gmp!(gmp_neg, gmp_abs, gmp_fact, gmp_sqrt, gmp_com, gmp_nextprime);

macro_rules! two_gmp {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            let y = arg(vm, a, 1);
            let r = run(vm, |e| g::$name(e, x.num(), y.num()))?;
            new_gmp(vm, r)
        }
    )*};
}
two_gmp!(
    gmp_add,
    gmp_sub,
    gmp_mul,
    gmp_mod,
    gmp_divexact,
    gmp_gcd,
    gmp_lcm,
    gmp_and,
    gmp_or,
    gmp_xor,
    gmp_random_range
);

macro_rules! two_int {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            let y = arg(vm, a, 1);
            run(vm, |e| g::$name(e, x.num(), y.num())).map(Value::Int)
        }
    )*};
}
two_int!(
    gmp_jacobi,
    gmp_legendre,
    gmp_kronecker,
    gmp_cmp,
    gmp_hamdist
);

macro_rules! one_int {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            run(vm, |e| g::$name(e, x.num())).map(Value::Int)
        }
    )*};
}
one_int!(gmp_intval, gmp_sign, gmp_popcount);

macro_rules! one_bool {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            run(vm, |e| g::$name(e, x.num())).map(Value::Bool)
        }
    )*};
}
one_bool!(gmp_perfect_square, gmp_perfect_power);

macro_rules! div_gmp {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            let y = arg(vm, a, 1);
            let m = a.int_or(2, g::GMP_ROUND_ZERO);
            let r = run(vm, |e| g::$name(e, x.num(), y.num(), m))?;
            new_gmp(vm, r)
        }
    )*};
}
div_gmp!(gmp_div_q, gmp_div_r, gmp_div);

macro_rules! num_int_gmp {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            let n = a.int(1);
            let r = run(vm, |e| g::$name(e, x.num(), n))?;
            new_gmp(vm, r)
        }
    )*};
}
num_int_gmp!(gmp_root, gmp_pow, gmp_binomial);

macro_rules! num_int_int {
    ($($name:ident),*) => {$(
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let x = arg(vm, a, 0);
            let n = a.int(1);
            run(vm, |e| g::$name(e, x.num(), n)).map(Value::Int)
        }
    )*};
}
num_int_int!(gmp_scan0, gmp_scan1);

fn gmp_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let base = a.int_or(1, 0);
    let r = run(vm, |e| g::gmp_init(e, x.num(), base))?;
    new_gmp(vm, r)
}

fn gmp_import(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (ws, fl) = (
        a.int_or(1, 1),
        a.int_or(2, g::GMP_MSW_FIRST | g::GMP_NATIVE_ENDIAN),
    );
    let data = a.str(0);
    let r = run(vm, |_| g::gmp_import(data.as_bytes(), ws, fl))?;
    new_gmp(vm, r)
}

fn gmp_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let (ws, fl) = (
        a.int_or(1, 1),
        a.int_or(2, g::GMP_MSW_FIRST | g::GMP_NATIVE_ENDIAN),
    );
    run(vm, |e| g::gmp_export(e, x.num(), ws, fl)).map(|b| Value::Str(PhpStr::from(b)))
}

fn gmp_strval(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let base = a.int_or(1, 10);
    run(vm, |e| g::gmp_strval(e, x.num(), base)).map(|s| Value::Str(PhpStr::from(s)))
}

fn gmp_div_qr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let y = arg(vm, a, 1);
    let m = a.int_or(2, g::GMP_ROUND_ZERO);
    let r = run(vm, |e| g::gmp_div_qr(e, x.num(), y.num(), m))?;
    pair(vm, r)
}

fn gmp_sqrtrem(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let r = run(vm, |e| g::gmp_sqrtrem(e, x.num()))?;
    pair(vm, r)
}

fn gmp_rootrem(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let n = a.int(1);
    let r = run(vm, |e| g::gmp_rootrem(e, x.num(), n))?;
    pair(vm, r)
}

fn gmp_powm(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let y = arg(vm, a, 1);
    let z = arg(vm, a, 2);
    let r = run(vm, |e| g::gmp_powm(e, x.num(), y.num(), z.num()))?;
    new_gmp(vm, r)
}

fn gmp_prob_prime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let n = a.int_or(1, 10);
    run(vm, |e| g::gmp_prob_prime(e, x.num(), n)).map(Value::Int)
}

fn gmp_gcdext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let y = arg(vm, a, 1);
    let r = run(vm, |e| g::gmp_gcdext(e, x.num(), y.num()))?;
    let mut out = PhpArray::new();
    for (k, v) in [("g", r.g), ("s", r.s), ("t", r.t)] {
        let v = new_gmp(vm, v)?;
        out.insert(Key::Str(PhpStr::from(k)), v);
    }
    Ok(Value::Array(out))
}

fn gmp_invert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let y = arg(vm, a, 1);
    match run(vm, |e| g::gmp_invert(e, x.num(), y.num()))? {
        Some(r) => new_gmp(vm, r),
        None => Ok(Value::Bool(false)),
    }
}

fn gmp_random_seed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    run(vm, |e| g::gmp_random_seed(e, x.num()))?;
    Ok(Value::Null)
}

fn gmp_random_bits(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    let r = run(vm, |_| g::gmp_random_bits(n))?;
    new_gmp(vm, r)
}

fn gmp_testbit(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = arg(vm, a, 0);
    let n = a.int(1);
    run(vm, |e| g::gmp_testbit(e, x.num(), n)).map(Value::Bool)
}

/// `gmp_setbit()`/`gmp_clrbit()`: the GMP object is changed in place.
fn bit_target(vm: &mut Vm, a: &Args) -> R<ObjRef> {
    match a.val(0) {
        Value::Object(o) if is_gmp(&o) => Ok(o),
        v => {
            let given = match owned(vm, &v) {
                Owned::Other(t) => t,
                o => match o.num() {
                    Num::Null => "null".into(),
                    Num::Bool(true) => "true".into(),
                    Num::Bool(false) => "false".into(),
                    Num::Int(_) => "int".into(),
                    Num::Float(_) => "float".into(),
                    _ => "string".into(),
                },
            };
            let e = g::gmp_object_type_error(&a.fname(), &given);
            Err(gmp_err(vm, e))
        }
    }
}

fn with_gmp_mut<T>(o: &ObjRef, f: impl FnOnce(&mut Gmp) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<Gmp>().map(f)
}

fn gmp_setbit(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = bit_target(vm, a)?;
    let (i, v) = (a.int(1), a.bool_or(2, true));
    set_limit(vm);
    if let Some(Err(e)) = with_gmp_mut(&o, |x| g::gmp_setbit(x, i, v)) {
        return Err(gmp_err(vm, e));
    }
    Ok(Value::Null)
}

fn gmp_clrbit(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = bit_target(vm, a)?;
    let i = a.int(1);
    if let Some(Err(e)) = with_gmp_mut(&o, |x| g::gmp_clrbit(x, i)) {
        return Err(gmp_err(vm, e));
    }
    Ok(Value::Null)
}

// ---------------------------------------------------------------- class

fn gmp_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = if a.has(0) { Some(arg(vm, a, 0)) } else { None };
    let base = a.int_or(1, 0);
    let r = run(vm, |e| {
        g::gmp_construct(e, x.as_ref().map(|x| x.num()), base)
    })?;
    *a.this_obj().obj().native.borrow_mut() = Some(Box::new(r));
    Ok(Value::Null)
}

fn this_gmp(a: &Args) -> Gmp {
    peek(a.this_obj()).unwrap_or_default()
}

fn gmp_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = this_gmp(a);
    let mut out = PhpArray::new();
    out.insert(
        Key::Int(0),
        Value::Str(PhpStr::from(g::object::serialize_value(&v))),
    );
    let props = a.this_obj().obj().props.borrow().clone();
    if !props.is_empty() {
        out.insert(Key::Int(1), Value::Array(props));
    }
    let _ = vm;
    Ok(Value::Array(out))
}

fn gmp_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let d0 = match data.get_int(0) {
        Some(v) => owned(vm, &v.deref()),
        None => Owned::Null,
    };
    let r = g::object::unserialize_value(d0.num()).map_err(|e| gmp_err(vm, e))?;
    if let Some(p) = data.get_int(1) {
        let Value::Array(props) = p.deref() else {
            return vm.throw("Exception", g::object::UNSERIALIZE_PROPERTIES_ERROR);
        };
        for (k, v) in props.iter() {
            vm.unser_set_prop(&o, k.to_owned(), v.deref())?;
        }
    }
    *o.obj().native.borrow_mut() = Some(Box::new(r));
    Ok(Value::Null)
}

/// `C:3:"GMP":N:{s:2:"42";a:0:{}}`: a decimal string then a property
/// array (crate wiring notes; test ext/gmp/tests/serialize.phpt).
fn gmp_unserialize_legacy(vm: &mut Vm, o: &ObjRef, data: Vec<Value>) -> R<()> {
    let n = match data.first() {
        Some(v) => owned(vm, v),
        None => Owned::Null,
    };
    let r = g::object::unserialize_legacy_value(n.num()).map_err(|e| gmp_err(vm, e))?;
    let Some(Value::Array(props)) = data.get(1) else {
        return vm.throw("Exception", g::object::UNSERIALIZE_PROPERTIES_ERROR);
    };
    *o.obj().native.borrow_mut() = Some(Box::new(r));
    // (no dynamic-property deprecation in this format: test
    // unserialize_with_reference.phpt)
    let mut t = o.obj().props.borrow_mut();
    for (k, v) in props.iter() {
        t.insert(k.to_owned(), v.clone());
    }
    Ok(())
}

// ---------------------------------------------------------------- hooks

fn gmp_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(Gmp::default()));
        }),
        clone: Some(|_, from, to| {
            if let Some(v) = peek(from) {
                *to.obj().native.borrow_mut() = Some(Box::new(v));
            }
        }),
        compare: Some(gmp_compare),
        cast: Some(|_, o, to| {
            let v = peek(o)?;
            Some(Ok(match to {
                CastTo::Int => Value::Int(g::object::to_int(&v)),
                CastTo::Float => Value::Float(g::object::to_float(&v)),
                CastTo::Bool => Value::Bool(g::object::to_bool(&v)),
                CastTo::String => Value::Str(PhpStr::from(g::object::to_string(&v))),
                _ => return None,
            }))
        }),
        debug_props: Some(|_, o| {
            let v = peek(o)?;
            // numeric property names show as int keys (test
            // gmp_dynamic_property.phpt)
            let mut out = PhpArray::new();
            for (k, v) in o.obj().props.borrow().iter() {
                let k = match k {
                    goro_runtime::KeyRef::Str(s) => Key::from_bytes(s.as_bytes()),
                    k => k.to_owned(),
                };
                out.insert(k, v.clone());
            }
            out.insert(
                Key::Str(PhpStr::from("num")),
                Value::Str(PhpStr::from(g::object::to_string(&v))),
            );
            Some(out)
        }),
        do_operation: Some(gmp_op),
        do_unary: Some(gmp_unary),
        unserialize_legacy: Some(gmp_unserialize_legacy),
        ..Default::default()
    }
}

fn gmp_op(vm: &mut Vm, op: goro_runtime::ops::BinOp, x: &Value, y: &Value) -> Option<R<Value>> {
    let x = owned(vm, &x.deref());
    let y = owned(vm, &y.deref());
    if !matches!(x, Owned::Gmp(_)) && !matches!(y, Owned::Gmp(_)) {
        return None;
    }
    set_limit(vm);
    let r = g::object::binary_op(vm, op, x.num(), y.num())?;
    Some((|| {
        vm.check_deferred()?;
        let r = r.map_err(|e| gmp_err(vm, e))?;
        new_gmp(vm, r)
    })())
}

fn gmp_unary(vm: &mut Vm, op: UnaryOp, o: &ObjRef) -> Option<R<Value>> {
    let v = peek(o)?;
    let r = match op {
        UnaryOp::Neg => g::object::negate(&v),
        UnaryOp::BitNot => g::object::bitwise_not(&v),
        UnaryOp::Inc => g::object::increment(&v),
        UnaryOp::Dec => g::object::decrement(&v),
    };
    Some(new_gmp(vm, r))
}

/// Comparisons with a GMP object on one side. A non-GMP object on the
/// left keeps its own comparison (`stdClass <=> GMP` is uncomparable, 1;
/// crate wiring notes, observed on PHP 8.5).
fn gmp_compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    let xo = owned(vm, &x.deref());
    if let Owned::Other(_) = xo
        && matches!(x.deref(), Value::Object(_))
    {
        return Some(Ok(1));
    }
    let yo = owned(vm, &y.deref());
    let r = g::object::compare(vm, xo.num(), yo.num());
    Some((|| {
        vm.check_deferred()?;
        let o = r.map_err(|e| gmp_err(vm, e))?;
        Ok(match o {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        })
    })())
}
