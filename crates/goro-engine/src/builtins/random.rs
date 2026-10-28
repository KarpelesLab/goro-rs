//! The random extension (PHP manual: Random): the `Random\*` engines and
//! `Random\Randomizer`, `Random\IntervalBoundary`, the random exceptions,
//! and the global functions (`mt_rand`, `rand`, `mt_srand`, `random_int`,
//! `random_bytes`, `lcg_value`, ...). The algorithms, state formats and
//! messages live in the goro-ext-random crate; this module wires them to
//! PHP values. `shuffle()`, `str_shuffle()` and `array_rand()` (in the
//! array and string modules) draw from [`legacy`].
//!
//! Behaviour sources: the PHP manual, the Random extension RFCs, and the
//! expected output of ext/random/tests/*.phpt.

use goro_ext_random::legacy::{self as rl, LegacyRandom};
use goro_ext_random::randomizer as rz;
use goro_ext_random::state::{StateItem, StateValue};
use goro_ext_random::{
    Engine, Generated, IntervalBoundary, Mt19937, NativeEngine, PcgOneseq128XslRr64, RandomError,
    Secure, Xoshiro256StarStar,
};
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "random",
    functions: &[
        (
            "function mt_rand(int $min = UNKNOWN, int $max = UNKNOWN): int",
            mt_rand,
        ),
        (
            "function mt_srand(?int $seed = null, int $mode = MT_RAND_MT19937): void",
            mt_srand,
        ),
        ("function mt_getrandmax(): int", getrandmax),
        (
            "function rand(int $min = UNKNOWN, int $max = UNKNOWN): int",
            rand,
        ),
        (
            "function srand(?int $seed = null, int $mode = MT_RAND_MT19937): void",
            mt_srand,
        ),
        ("function getrandmax(): int", getrandmax),
        ("function random_int(int $min, int $max): int", random_int),
        ("function random_bytes(int $length): string", random_bytes),
        ("function lcg_value(): float", lcg_value),
    ],
    classes: CLASSES,
    methods: METHODS,
    hooks: &[
        ("random\\engine\\mt19937", || engine_hooks(new_mt)),
        ("random\\engine\\pcgoneseq128xslrr64", || {
            engine_hooks(new_pcg)
        }),
        ("random\\engine\\xoshiro256starstar", || {
            engine_hooks(new_xoshiro)
        }),
        ("random\\engine\\secure", || engine_hooks(new_secure)),
    ],
    constants: &[
        ("MT_RAND_MT19937", ConstVal::Int(0)),
        ("MT_RAND_PHP", ConstVal::Int(1)),
    ],
    ..Module::EMPTY
};

const CLASSES: &[(&str, &str)] = &[
    (
        "random\\randomerror",
        "<?php namespace Random; class RandomError extends \\Error {}",
    ),
    (
        "random\\brokenrandomengineerror",
        "<?php namespace Random; class BrokenRandomEngineError extends RandomError {}",
    ),
    (
        "random\\randomexception",
        "<?php namespace Random; class RandomException extends \\Exception {}",
    ),
    (
        "random\\engine",
        "<?php namespace Random; interface Engine { public function generate(): string; }",
    ),
    (
        "random\\cryptosafeengine",
        "<?php namespace Random; interface CryptoSafeEngine extends Engine {}",
    ),
    (
        "random\\intervalboundary",
        "<?php namespace Random; enum IntervalBoundary { case ClosedOpen; case ClosedClosed; case OpenClosed; case OpenOpen; }",
    ),
    (
        "random\\engine\\mt19937",
        r#"<?php namespace Random\Engine; final class Mt19937 implements \Random\Engine {
    public function __construct(?int $seed = null, int $mode = MT_RAND_MT19937) {}
    public function generate(): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function __debugInfo(): array {}
}"#,
    ),
    (
        "random\\engine\\pcgoneseq128xslrr64",
        r#"<?php namespace Random\Engine; final class PcgOneseq128XslRr64 implements \Random\Engine {
    public function __construct(string|int|null $seed = null) {}
    public function generate(): string {}
    public function jump(int $advance): void {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function __debugInfo(): array {}
}"#,
    ),
    (
        "random\\engine\\xoshiro256starstar",
        r#"<?php namespace Random\Engine; final class Xoshiro256StarStar implements \Random\Engine {
    public function __construct(string|int|null $seed = null) {}
    public function generate(): string {}
    public function jump(): void {}
    public function jumpLong(): void {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function __debugInfo(): array {}
}"#,
    ),
    (
        "random\\engine\\secure",
        r#"<?php namespace Random\Engine; #[\NotSerializable] final class Secure implements \Random\CryptoSafeEngine {
    public function generate(): string {}
}"#,
    ),
    // The constructor runs as PHP so that a second call fails on the
    // readonly property (test 03_randomizer/construct_twice.phpt).
    (
        "random\\randomizer",
        r#"<?php namespace Random; final class Randomizer {
    public readonly Engine $engine;
    public function __construct(?Engine $engine = null) { $this->engine = $engine ?? new \Random\Engine\Secure(); }
    public function nextInt(): int {}
    public function nextFloat(): float {}
    public function getFloat(float $min, float $max, IntervalBoundary $boundary = IntervalBoundary::ClosedOpen): float {}
    public function getInt(int $min, int $max): int {}
    public function getBytes(int $length): string {}
    public function getBytesFromString(string $string, int $length): string {}
    public function shuffleArray(array $array): array {}
    public function shuffleBytes(string $bytes): string {}
    public function pickArrayKeys(array $array, int $num): array {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
}"#,
    ),
];

const METHODS: &[(&str, crate::func::NativeFn)] = &[
    ("random\\engine\\mt19937::__construct", mt_construct),
    ("random\\engine\\mt19937::generate", engine_generate),
    ("random\\engine\\mt19937::__serialize", engine_serialize),
    ("random\\engine\\mt19937::__unserialize", engine_unserialize),
    ("random\\engine\\mt19937::__debuginfo", engine_debug_info),
    (
        "random\\engine\\pcgoneseq128xslrr64::__construct",
        pcg_construct,
    ),
    (
        "random\\engine\\pcgoneseq128xslrr64::generate",
        engine_generate,
    ),
    ("random\\engine\\pcgoneseq128xslrr64::jump", pcg_jump),
    (
        "random\\engine\\pcgoneseq128xslrr64::__serialize",
        engine_serialize,
    ),
    (
        "random\\engine\\pcgoneseq128xslrr64::__unserialize",
        engine_unserialize,
    ),
    (
        "random\\engine\\pcgoneseq128xslrr64::__debuginfo",
        engine_debug_info,
    ),
    (
        "random\\engine\\xoshiro256starstar::__construct",
        xoshiro_construct,
    ),
    (
        "random\\engine\\xoshiro256starstar::generate",
        engine_generate,
    ),
    ("random\\engine\\xoshiro256starstar::jump", xoshiro_jump),
    (
        "random\\engine\\xoshiro256starstar::jumplong",
        xoshiro_jump_long,
    ),
    (
        "random\\engine\\xoshiro256starstar::__serialize",
        engine_serialize,
    ),
    (
        "random\\engine\\xoshiro256starstar::__unserialize",
        engine_unserialize,
    ),
    (
        "random\\engine\\xoshiro256starstar::__debuginfo",
        engine_debug_info,
    ),
    ("random\\engine\\secure::generate", engine_generate),
    ("random\\randomizer::nextint", rz_next_int),
    ("random\\randomizer::nextfloat", rz_next_float),
    ("random\\randomizer::getfloat", rz_get_float),
    ("random\\randomizer::getint", rz_get_int),
    ("random\\randomizer::getbytes", rz_get_bytes),
    (
        "random\\randomizer::getbytesfromstring",
        rz_get_bytes_from_string,
    ),
    ("random\\randomizer::shufflearray", rz_shuffle_array),
    ("random\\randomizer::shufflebytes", rz_shuffle_bytes),
    ("random\\randomizer::pickarraykeys", rz_pick_array_keys),
    ("random\\randomizer::__serialize", rz_serialize),
    ("random\\randomizer::__unserialize", rz_unserialize),
];

// ---------------------------------------------------------------- errors

/// A goro-ext-random error as the PHP exception it names. `pending` is the
/// exception or exit a user engine's `generate()` raised.
pub(crate) fn random_err(vm: &mut Vm, e: RandomError, pending: Option<Unwind>) -> Unwind {
    let (cls, m) = match e {
        RandomError::ValueError(m) => ("ValueError", m),
        RandomError::BrokenEngine(m) => ("Random\\BrokenRandomEngineError", m.to_string()),
        RandomError::RandomException(m) => ("Random\\RandomException", m),
        RandomError::Exception(m) => ("Exception", m),
        RandomError::Propagate => {
            return pending.unwrap_or_else(|| vm.throw_error_msg("Error", "random engine failure"));
        }
    };
    vm.throw_error_msg(cls, &m)
}

fn rr<T>(vm: &mut Vm, r: Result<T, RandomError>) -> R<T> {
    r.map_err(|e| random_err(vm, e, None))
}

// ---------------------------------------------------------------- legacy

/// The per-request state of the global functions.
#[derive(Default)]
pub struct LegacyState(pub LegacyRandom);

/// The global generator behind `mt_rand()`, `shuffle()`, `str_shuffle()`
/// and `array_rand()`.
pub fn legacy(vm: &mut Vm) -> &mut LegacyRandom {
    &mut vm.ext::<LegacyState>().0
}

fn two_args(vm: &mut Vm, a: &Args) -> Unwind {
    vm.throw_error_msg(
        "ArgumentCountError",
        &format!("{}() expects exactly 2 arguments, 1 given", a.fname()),
    )
}

fn mt_rand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.len() {
        0 => Ok(Value::Int(legacy(vm).mt_rand())),
        1 => Err(two_args(vm, a)),
        _ => {
            let r = legacy(vm).mt_rand_range(a.int(0), a.int(1));
            rr(vm, r).map(Value::Int)
        }
    }
}

fn rand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.len() {
        0 => Ok(Value::Int(legacy(vm).mt_rand())),
        1 => Err(two_args(vm, a)),
        _ => Ok(Value::Int(legacy(vm).rand_range(a.int(0), a.int(1)))),
    }
}

fn mt_srand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (seed, mode) = (a.opt_int(0), a.int_or(1, 0));
    if let Some(msg) = legacy(vm).mt_srand(seed, mode) {
        vm.deprecated(msg)?;
    }
    Ok(Value::Null)
}

fn getrandmax(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(LegacyRandom::getrandmax()))
}

fn random_int(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = rl::random_int(a.int(0), a.int(1));
    rr(vm, r).map(Value::Int)
}

fn random_bytes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = rl::random_bytes(a.int(0));
    rr(vm, r).map(|b| Value::Str(PhpStr::from(b)))
}

fn lcg_value(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated(goro_ext_random::MSG_LCG_VALUE_DEPRECATED)?;
    Ok(Value::Float(legacy(vm).lcg_value()))
}

/// An array's storage layout for `array_rand()`'s single pick.
pub(crate) struct ArraySlots<'a>(pub &'a PhpArray);

impl rz::PickSource for ArraySlots<'_> {
    fn count(&self) -> usize {
        self.0.len()
    }
    fn slots(&self) -> usize {
        self.0.data().last_pos().map_or(0, |p| p as usize + 1)
    }
    fn slot_live(&self, i: usize) -> bool {
        self.0.data().entry_at(i as u32).is_some()
    }
    fn slot_to_index(&self, i: usize) -> usize {
        let d = self.0.data();
        (0..i).filter(|&p| d.entry_at(p as u32).is_some()).count()
    }
}

/// `array_rand($array, $num)`: the picked ordinal positions.
pub(crate) fn array_rand_positions(vm: &mut Vm, arr: &PhpArray, num: i64) -> R<Vec<usize>> {
    let r = legacy(vm).array_rand(&ArraySlots(arr), num);
    rr(vm, r)
}

/// `shuffle()`'s and `str_shuffle()`'s permutation, in place.
pub(crate) fn legacy_shuffle<T>(vm: &mut Vm, items: &mut [T]) {
    legacy(vm).shuffle(items);
}

// ---------------------------------------------------------------- engines

/// The native data of an engine object.
struct EngineObj(NativeEngine);

impl goro_runtime::NativeData for EngineObj {}

fn new_mt() -> NativeEngine {
    NativeEngine::Mt19937(Box::new(Mt19937::new(0, goro_ext_random::MtMode::Mt19937)))
}
fn new_pcg() -> NativeEngine {
    NativeEngine::Pcg(PcgOneseq128XslRr64::from_int(0))
}
fn new_xoshiro() -> NativeEngine {
    NativeEngine::Xoshiro(Xoshiro256StarStar::from_int(0))
}
fn new_secure() -> NativeEngine {
    NativeEngine::Secure(Secure)
}

fn engine_hooks(init: fn() -> NativeEngine) -> NativeHooks {
    // `create` has no way to know which class it is for but through the
    // hook table, so each class gets its own closure-free initializer: the
    // initial state is replaced by the constructor or __unserialize().
    let create: fn(&mut Vm, &ObjRef) = match init() {
        NativeEngine::Mt19937(_) => |_, o| set_engine(o, new_mt()),
        NativeEngine::Pcg(_) => |_, o| set_engine(o, new_pcg()),
        NativeEngine::Xoshiro(_) => |_, o| set_engine(o, new_xoshiro()),
        NativeEngine::Secure(_) => |_, o| set_engine(o, new_secure()),
    };
    NativeHooks {
        create: Some(create),
        clone: Some(|_, from, to| {
            if let Some(e) = with_engine_obj(from, |e| e.clone()) {
                set_engine(to, e);
            }
        }),
        ..Default::default()
    }
}

fn set_engine(o: &ObjRef, e: NativeEngine) {
    *o.obj().native.borrow_mut() = Some(Box::new(EngineObj(e)));
}

fn with_engine_obj<U>(o: &ObjRef, f: impl FnOnce(&mut NativeEngine) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<EngineObj>().map(|e| f(&mut e.0))
}

fn this_engine<U>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut NativeEngine) -> U) -> R<U> {
    match with_engine_obj(a.this_obj(), f) {
        Some(u) => Ok(u),
        None => vm.throw("Error", "Random engine object is not initialized"),
    }
}

fn mt_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (seed, mode) = (a.opt_int(0), a.int_or(1, 0));
    let g = rr(vm, Mt19937::construct(seed, mode))?;
    if mode == 1 {
        vm.deprecated(goro_ext_random::MSG_MT_RAND_PHP_DEPRECATED)?;
    }
    set_engine(a.this_obj(), NativeEngine::Mt19937(Box::new(g)));
    Ok(Value::Null)
}

fn pcg_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = match a.val(0) {
        Value::Null => PcgOneseq128XslRr64::new_random(),
        Value::Str(s) => PcgOneseq128XslRr64::from_string(s.as_bytes()),
        v => Ok(PcgOneseq128XslRr64::from_int(vm.to_int(&v)?)),
    };
    let g = rr(vm, g)?;
    set_engine(a.this_obj(), NativeEngine::Pcg(g));
    Ok(Value::Null)
}

fn xoshiro_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let g = match a.val(0) {
        Value::Null => Xoshiro256StarStar::new_random(),
        Value::Str(s) => Xoshiro256StarStar::from_string(s.as_bytes()),
        v => Ok(Xoshiro256StarStar::from_int(vm.to_int(&v)?)),
    };
    let g = rr(vm, g)?;
    set_engine(a.this_obj(), NativeEngine::Xoshiro(g));
    Ok(Value::Null)
}

fn engine_generate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = this_engine(vm, a, |e| e.generate())?;
    let g = rr(vm, r)?;
    Ok(Value::Str(PhpStr::from(g.bytes())))
}

fn pcg_jump(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    let r = this_engine(vm, a, |e| match e {
        NativeEngine::Pcg(p) => p.jump(n),
        _ => Ok(()),
    })?;
    rr(vm, r).map(|_| Value::Null)
}

fn xoshiro_jump(vm: &mut Vm, a: &mut Args) -> R<Value> {
    this_engine(vm, a, |e| {
        if let NativeEngine::Xoshiro(x) = e {
            x.jump()
        }
    })?;
    Ok(Value::Null)
}

fn xoshiro_jump_long(vm: &mut Vm, a: &mut Args) -> R<Value> {
    this_engine(vm, a, |e| {
        if let NativeEngine::Xoshiro(x) = e {
            x.jump_long()
        }
    })?;
    Ok(Value::Null)
}

fn state_items(e: &NativeEngine) -> Vec<StateItem> {
    match e {
        NativeEngine::Mt19937(m) => m.state(),
        NativeEngine::Pcg(p) => p.state(),
        NativeEngine::Xoshiro(x) => x.state(),
        NativeEngine::Secure(_) => Vec::new(),
    }
}

fn state_array(items: Vec<StateItem>) -> Value {
    Value::Array(PhpArray::from_values(items.into_iter().map(|i| match i {
        StateItem::Hex(h) => Value::Str(PhpStr::from(h)),
        StateItem::Int(n) => Value::Int(n),
    })))
}

fn key(s: &str) -> Key {
    Key::Str(PhpStr::from(s))
}

fn engine_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let items = this_engine(vm, a, |e| state_items(e))?;
    let props = vm.plain_props_array(&o, true);
    Ok(Value::Array(PhpArray::from_values([
        Value::Array(props),
        state_array(items),
    ])))
}

fn engine_debug_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let items = this_engine(vm, a, |e| state_items(e))?;
    let mut out = vm.plain_props_array(&o, true);
    out.insert(key("__states"), state_array(items));
    Ok(Value::Array(out))
}

fn engine_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let cls_name = with_engine_obj(&o, |e| e.class_name()).unwrap_or("Random\\Engine");
    let data = a.array(0);
    let bad = |vm: &mut Vm| -> R<Value> {
        let e = goro_ext_random::invalid_serialization(cls_name);
        Err(random_err(vm, e, None))
    };
    if data.len() != 2 {
        return bad(vm);
    }
    let (Some(Value::Array(props)), Some(Value::Array(st))) = (
        data.get_int(0).map(|v| v.deref()),
        data.get_int(1).map(|v| v.deref()),
    ) else {
        return bad(vm);
    };
    if !props.is_empty() {
        return bad(vm);
    }
    let vals: Vec<Option<Value>> = (0..st.len() as i64)
        .map(|i| st.get_int(i).map(|v| v.deref()))
        .collect();
    let items: Vec<StateValue<'_>> = vals
        .iter()
        .map(|v| match v {
            Some(Value::Str(s)) => StateValue::Str(s.as_bytes()),
            Some(Value::Int(n)) => StateValue::Int(*n),
            None => StateValue::Missing,
            _ => StateValue::Other,
        })
        .collect();
    let r = match with_engine_obj(&o, |e| std::mem::discriminant(e)) {
        Some(d) if d == std::mem::discriminant(&new_pcg()) => {
            PcgOneseq128XslRr64::from_state(&items).map(NativeEngine::Pcg)
        }
        Some(d) if d == std::mem::discriminant(&new_xoshiro()) => {
            Xoshiro256StarStar::from_state(&items).map(NativeEngine::Xoshiro)
        }
        Some(d) if d == std::mem::discriminant(&new_secure()) => return bad(vm),
        _ => Mt19937::from_state(&items).map(|m| NativeEngine::Mt19937(Box::new(m))),
    };
    let e = rr(vm, r)?;
    set_engine(&o, e);
    Ok(Value::Null)
}

// ---------------------------------------------------------------- randomizer

/// A `Random\Engine` implemented in PHP: `generate()` calls the method.
struct UserEngine<'a> {
    vm: &'a mut Vm,
    obj: ObjRef,
    pending: Option<Unwind>,
}

impl Engine for UserEngine<'_> {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        let v = match self.vm.call_method(&self.obj, b"generate", Vec::new()) {
            Ok(v) => v.deref(),
            Err(u) => {
                self.pending = Some(u);
                return Err(RandomError::Propagate);
            }
        };
        let s = match v {
            Value::Str(s) => s,
            v => match self.vm.to_php_str(&v) {
                Ok(s) => s,
                Err(u) => {
                    self.pending = Some(u);
                    return Err(RandomError::Propagate);
                }
            },
        };
        Generated::from_bytes(s.as_bytes())
    }
}

/// Runs `f` on the Randomizer's engine, native or user.
fn with_rz_engine<T>(
    vm: &mut Vm,
    a: &Args,
    f: impl FnOnce(&mut dyn Engine) -> Result<T, RandomError>,
) -> R<T> {
    let this = a.this_obj();
    let eng = match vm.get_prop_raw(this, b"engine").deref() {
        Value::Object(o) => o,
        _ => {
            return vm.throw(
                "Error",
                "Typed property Random\\Randomizer::$engine must not be accessed before initialization",
            );
        }
    };
    let mut slot = Some(f);
    let native = with_engine_obj(&eng, |e| (slot.take().unwrap())(e));
    let r = match native {
        Some(r) => r,
        None => {
            let f = slot.take().unwrap();
            let mut ue = UserEngine {
                vm,
                obj: eng,
                pending: None,
            };
            let r = f(&mut ue);
            let pending = ue.pending.take();
            return r.map_err(|e| random_err(vm, e, pending));
        }
    };
    rr(vm, r)
}

fn rz_next_int(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_rz_engine(vm, a, |e| {
        // nextInt() needs 64 bits (observed on PHP 8.5: user engines'
        // short results are combined, as for getInt()).
        rz::next_int(e)
    })
    .map(Value::Int)
}

fn rz_next_float(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_rz_engine(vm, a, |e| rz::next_float(e)).map(Value::Float)
}

fn rz_get_float(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (min, max) = (a.float(0), a.float(1));
    let b = match a.obj(2) {
        Some(o) => {
            let n = vm.get_prop_raw(&o, b"name");
            let n = vm.to_php_str(&n)?;
            IntervalBoundary::from_name(&n.to_string_lossy())
                .unwrap_or(IntervalBoundary::ClosedOpen)
        }
        None => IntervalBoundary::ClosedOpen,
    };
    with_rz_engine(vm, a, |e| rz::get_float(e, min, max, b)).map(Value::Float)
}

fn rz_get_int(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (min, max) = (a.int(0), a.int(1));
    with_rz_engine(vm, a, |e| rz::get_int(e, min, max)).map(Value::Int)
}

fn rz_get_bytes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    with_rz_engine(vm, a, |e| rz::get_bytes(e, n)).map(|b| Value::Str(PhpStr::from(b)))
}

fn rz_get_bytes_from_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let n = a.int(1);
    with_rz_engine(vm, a, |e| rz::get_bytes_from_string(e, s.as_bytes(), n))
        .map(|b| Value::Str(PhpStr::from(b)))
}

fn rz_shuffle_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let mut vals: Vec<Value> = arr.iter().map(|(_, v)| v.clone()).collect();
    drop(arr);
    with_rz_engine(vm, a, |e| rz::shuffle_array(e, &mut vals))?;
    Ok(Value::Array(PhpArray::from_values(vals)))
}

fn rz_shuffle_bytes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut b = a.bytes(0).to_vec();
    with_rz_engine(vm, a, |e| rz::shuffle_bytes(e, &mut b))?;
    Ok(Value::Str(PhpStr::from(b)))
}

fn rz_pick_array_keys(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let num = a.int(1);
    let n = arr.len();
    let picked = with_rz_engine(vm, a, |e| rz::pick_array_keys(e, n, num))?;
    let keys: Vec<Value> = arr.iter().map(|(k, _)| k.to_value()).collect();
    Ok(Value::Array(PhpArray::from_values(
        picked.into_iter().map(|i| keys[i].clone()),
    )))
}

fn rz_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let eng = vm.get_prop_raw(&o, b"engine").deref();
    let mut props = vm.plain_props_array(&o, true);
    props.insert(key("engine"), eng);
    Ok(Value::Array(PhpArray::from_values([Value::Array(props)])))
}

fn rz_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let bad = |vm: &mut Vm| -> R<Value> {
        let e = goro_ext_random::invalid_serialization(goro_ext_random::class::RANDOMIZER);
        Err(random_err(vm, e, None))
    };
    let cur = vm.get_prop_raw(&o, b"engine");
    // (an uninitialized typed property reads as null here)
    if !matches!(cur, Value::Null) {
        return bad(vm);
    }
    if data.len() != 1 {
        return bad(vm);
    }
    let Some(Value::Array(props)) = data.get_int(0).map(|v| v.deref()) else {
        return bad(vm);
    };
    if props.len() != 1 {
        return bad(vm);
    }
    let Some(Value::Object(eng)) = props.get_bytes(b"engine").map(|v| v.deref()) else {
        return bad(vm);
    };
    let ec = vm.class_of(&eng);
    if !vm.instance_of_lname(&ec, b"random\\engine") {
        return bad(vm);
    }
    vm.set_prop_raw(&o, b"engine", Value::Object(eng));
    Ok(Value::Null)
}
