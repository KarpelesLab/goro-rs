//! Math functions (PHP manual: Math functions, Random number generation
//! functions of ext/random's legacy API), wired to `goro_ext_standard`.
//!
//! Behaviour sources: the manual pages, the RoundingMode RFC
//! (wiki.php.net/rfc/new_rounding_modes_to_round_function), tests
//! ext/standard/tests/math/*.phpt, ext/random/tests, and observations of
//! PHP 8.5.

use goro_ext_standard as std_ext;
use goro_ext_standard::math::{Number, RoundMode};
use goro_ext_standard::mt_rand::{Mt19937, MtMode};
use goro_runtime::compare::compare;
use goro_runtime::{PhpStr, Value};

use super::string::{sr, sv};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        ("function abs(int|float $num): int|float", abs),
        ("function ceil(int|float $num): float", ceil),
        ("function floor(int|float $num): float", floor),
        (
            "function round(int|float $num, int $precision = 0, RoundingMode|int $mode = RoundingMode::HalfAwayFromZero): float",
            round,
        ),
        ("function sqrt(float $num): float", sqrt),
        (
            "function pow(mixed $num, mixed $exponent): object|int|float",
            pow,
        ),
        ("function intdiv(int $num1, int $num2): int", intdiv),
        ("function fmod(float $num1, float $num2): float", fmod),
        ("function fdiv(float $num1, float $num2): float", fdiv),
        ("function fpow(float $num, float $exponent): float", fpow),
        ("function exp(float $num): float", exp),
        ("function log(float $num, float $base = M_E): float", log),
        ("function log10(float $num): float", log10),
        ("function log1p(float $num): float", log1p),
        ("function expm1(float $num): float", expm1),
        ("function sin(float $num): float", sin),
        ("function cos(float $num): float", cos),
        ("function tan(float $num): float", tan),
        ("function asin(float $num): float", asin),
        ("function acos(float $num): float", acos),
        ("function atan(float $num): float", atan),
        ("function atan2(float $y, float $x): float", atan2),
        ("function sinh(float $num): float", sinh),
        ("function cosh(float $num): float", cosh),
        ("function tanh(float $num): float", tanh),
        ("function asinh(float $num): float", asinh),
        ("function acosh(float $num): float", acosh),
        ("function atanh(float $num): float", atanh),
        ("function pi(): float", pi),
        ("function hypot(float $x, float $y): float", hypot),
        ("function deg2rad(float $num): float", deg2rad),
        ("function rad2deg(float $num): float", rad2deg),
        ("function is_nan(float $num): bool", is_nan),
        ("function is_finite(float $num): bool", is_finite),
        ("function is_infinite(float $num): bool", is_infinite),
        ("function max(mixed $value, mixed ...$values): mixed", max),
        ("function min(mixed $value, mixed ...$values): mixed", min),
        (
            "function base_convert(string $num, int $from_base, int $to_base): string",
            base_convert,
        ),
        ("function bindec(string $binary_string): int|float", bindec),
        ("function hexdec(string $hex_string): int|float", hexdec),
        ("function octdec(string $octal_string): int|float", octdec),
        ("function decbin(int $num): string", decbin),
        ("function dechex(int $num): string", dechex),
        ("function decoct(int $num): string", decoct),
        (
            "function mt_rand(int $min = UNKNOWN, int $max = UNKNOWN): int",
            mt_rand,
        ),
        (
            "function mt_srand(?int $seed = null, int $mode = MT_RAND_MT19937): void",
            mt_srand,
        ),
        ("function mt_getrandmax(): int", mt_getrandmax),
        (
            "function rand(int $min = UNKNOWN, int $max = UNKNOWN): int",
            rand,
        ),
        (
            "function srand(?int $seed = null, int $mode = MT_RAND_MT19937): void",
            mt_srand,
        ),
        ("function getrandmax(): int", mt_getrandmax),
        ("function random_int(int $min, int $max): int", random_int),
        ("function random_bytes(int $length): string", random_bytes),
        ("function lcg_value(): float", lcg_value),
    ],
    classes: &[(
        "roundingmode",
        "<?php enum RoundingMode { case HalfAwayFromZero; case HalfTowardsZero; case HalfEven; case HalfOdd; case TowardsZero; case AwayFromZero; case NegativeInfinity; case PositiveInfinity; }",
    )],
    constants: &[
        ("M_E", ConstVal::Float(std::f64::consts::E)),
        ("M_LOG2E", ConstVal::Float(std::f64::consts::LOG2_E)),
        ("M_LOG10E", ConstVal::Float(std::f64::consts::LOG10_E)),
        ("M_LN2", ConstVal::Float(std::f64::consts::LN_2)),
        ("M_LN10", ConstVal::Float(std::f64::consts::LN_10)),
        ("M_PI", ConstVal::Float(std::f64::consts::PI)),
        ("M_PI_2", ConstVal::Float(std::f64::consts::FRAC_PI_2)),
        ("M_PI_4", ConstVal::Float(std::f64::consts::FRAC_PI_4)),
        ("M_1_PI", ConstVal::Float(std::f64::consts::FRAC_1_PI)),
        ("M_2_PI", ConstVal::Float(std::f64::consts::FRAC_2_PI)),
        ("M_SQRTPI", ConstVal::Float(1.772_453_850_905_516)),
        (
            "M_2_SQRTPI",
            ConstVal::Float(std::f64::consts::FRAC_2_SQRT_PI),
        ),
        ("M_LNPI", ConstVal::Float(1.144_729_885_849_400_2)),
        ("M_EULER", ConstVal::Float(std::f64::consts::EULER_GAMMA)),
        ("M_SQRT2", ConstVal::Float(std::f64::consts::SQRT_2)),
        (
            "M_SQRT1_2",
            ConstVal::Float(std::f64::consts::FRAC_1_SQRT_2),
        ),
        ("M_SQRT3", ConstVal::Float(1.732_050_807_568_877_2)),
        ("INF", ConstVal::Float(f64::INFINITY)),
        ("NAN", ConstVal::Float(f64::NAN)),
        ("PHP_ROUND_HALF_UP", ConstVal::Int(1)),
        ("PHP_ROUND_HALF_DOWN", ConstVal::Int(2)),
        ("PHP_ROUND_HALF_EVEN", ConstVal::Int(3)),
        ("PHP_ROUND_HALF_ODD", ConstVal::Int(4)),
        ("MT_RAND_MT19937", ConstVal::Int(0)),
        ("MT_RAND_PHP", ConstVal::Int(1)),
    ],
    listed: false,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- random state

/// The global Mt19937 state shared by `mt_rand()`, `rand()`, `shuffle()`,
/// `str_shuffle()` and `array_rand()` (PHP manual: mt_srand, "seeds the
/// random number generator" used by these functions).
#[derive(Default)]
pub struct MtState {
    pub generator: Option<Mt19937>,
}

/// Fills `buf` from the operating system's CSPRNG.
pub fn os_random(buf: &mut [u8]) -> bool {
    let mut done = 0;
    while done < buf.len() {
        // SAFETY: getrandom writes at most `len` bytes into the buffer.
        let r = unsafe {
            libc::getrandom(
                buf[done..].as_mut_ptr() as *mut libc::c_void,
                buf.len() - done,
                0,
            )
        };
        if r < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        done += r as usize;
    }
    true
}

fn os_u64() -> u64 {
    let mut b = [0u8; 8];
    os_random(&mut b);
    u64::from_le_bytes(b)
}

/// The global generator, seeded randomly on first use.
pub fn mt(vm: &mut Vm) -> &mut Mt19937 {
    let st = vm.ext::<MtState>();
    st.generator
        .get_or_insert_with(|| Mt19937::new(os_u64() as u32 as i64, MtMode::Mt19937))
}

/// A uniform value in `min..=max` from the CSPRNG (`random_int()`).
pub fn os_range(min: i64, max: i64) -> i64 {
    let umax = (max as u64).wrapping_sub(min as u64);
    if umax == u64::MAX {
        return os_u64() as i64;
    }
    let n = umax + 1;
    // Rejection sampling for uniformity.
    let limit = u64::MAX - (u64::MAX % n) - 1;
    loop {
        let r = os_u64();
        if r <= limit || n.is_power_of_two() {
            return (min as u64).wrapping_add(r % n) as i64;
        }
    }
}

// ---------------------------------------------------------------- basics

fn num_arg(a: &Args, i: usize) -> Value {
    a.val(i)
}

fn abs(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match num_arg(a, 0) {
        Value::Int(i) => match i.checked_abs() {
            Some(v) => Value::Int(v),
            None => Value::Float(-(i as f64)),
        },
        Value::Float(f) => Value::Float(f.abs()),
        v => v,
    })
}

fn as_f64(v: &Value) -> f64 {
    match v {
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        _ => 0.0,
    }
}

fn ceil(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(as_f64(&num_arg(a, 0)).ceil()))
}

fn floor(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(as_f64(&num_arg(a, 0)).floor()))
}

fn round(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let places = a.int_or(1, 0);
    let mode = match a.get(2).map(|v| v.deref()) {
        None => RoundMode::HalfAwayFromZero,
        Some(Value::Object(o)) => {
            let name = vm.get_prop_raw(&o, b"name");
            let n = match name {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            RoundMode::from_case_name(&n).unwrap_or(RoundMode::HalfAwayFromZero)
        }
        Some(v) => {
            let m = match v {
                Value::Int(i) => i,
                _ => 0,
            };
            sr(vm, RoundMode::from_int(m))?
        }
    };
    Ok(Value::Float(match num_arg(a, 0) {
        Value::Int(i) if places >= 0 => i as f64,
        v => std_ext::math::round(as_f64(&v), places, mode),
    }))
}

fn sqrt(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0).sqrt()))
}

fn pow(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = a.val(0);
    let y = a.val(1);
    let r = goro_runtime::ops::pow(&x, &y, vm);
    vm.check_deferred()?;
    r.map_err(|e| vm.php_error(e))
}

fn intdiv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = std_ext::math::intdiv(a.int(0), a.int(1));
    Ok(Value::Int(sr(vm, r)?))
}

fn fmod(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(std_ext::math::fmod(a.float(0), a.float(1))))
}

fn fdiv(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0) / a.float(1)))
}

fn fpow(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0).powf(a.float(1))))
}

macro_rules! unary_float {
    ($($name:ident => $f:expr),* $(,)?) => {
        $(
            fn $name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
                let f: fn(f64) -> f64 = $f;
                Ok(Value::Float(f(a.float(0))))
            }
        )*
    };
}

unary_float! {
    exp => f64::exp,
    log10 => f64::log10,
    log1p => f64::ln_1p,
    expm1 => f64::exp_m1,
    sin => f64::sin,
    cos => f64::cos,
    tan => f64::tan,
    asin => f64::asin,
    acos => f64::acos,
    atan => f64::atan,
    sinh => f64::sinh,
    cosh => f64::cosh,
    tanh => f64::tanh,
    asinh => f64::asinh,
    acosh => f64::acosh,
    atanh => f64::atanh,
}

// Tests math/deg2rad_variation.phpt, rad2deg_basiclong_64bit.phpt: the
// division comes first.
fn deg2rad(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0) / 180.0 * std::f64::consts::PI))
}
fn rad2deg(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0) / std::f64::consts::PI * 180.0))
}

/// Raises base-conversion diagnostics, which PHP reports without the
/// function name (tests math/bindec_basic_64bit.phpt, hexdec.phpt).
fn base_diags(vm: &mut Vm, d: Vec<goro_ext_standard::error::Diag>) -> R<()> {
    use goro_ext_standard::error::Diag;
    for x in d {
        match x {
            Diag::Warning(m) => vm.warn(&m)?,
            Diag::Notice(m) => vm.notice_msg(&m)?,
            Diag::Deprecated(m) => vm.deprecated(&m)?,
        }
    }
    Ok(())
}

fn log(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let x = a.float(0);
    if !a.has(1) {
        return Ok(Value::Float(x.ln()));
    }
    let base = a.float(1);
    if base <= 0.0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($base) must be greater than 0",
        );
    }
    if base == 1.0 {
        return Ok(Value::Float(f64::NAN));
    }
    if base == 2.0 {
        return Ok(Value::Float(x.log2()));
    }
    if base == 10.0 {
        return Ok(Value::Float(x.log10()));
    }
    Ok(Value::Float(x.ln() / base.ln()))
}

fn atan2(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0).atan2(a.float(1))))
}

fn pi(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Float(std::f64::consts::PI))
}

fn hypot(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Float(a.float(0).hypot(a.float(1))))
}

fn is_nan(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.float(0).is_nan()))
}
fn is_finite(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.float(0).is_finite()))
}
fn is_infinite(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(a.float(0).is_infinite()))
}

/// `$a <=> $b` with the engine as context.
pub(crate) fn php_compare(vm: &mut Vm, x: &Value, y: &Value) -> R<i32> {
    let r = compare(x, y, vm);
    vm.check_deferred()?;
    r.map_err(|e| vm.php_error(e))
}

fn minmax(vm: &mut Vm, a: &mut Args, want_max: bool) -> R<Value> {
    let vals: Vec<Value> = if a.len() == 1 {
        match a.val(0) {
            Value::Array(arr) => {
                if arr.is_empty() {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        "Argument #1 ($value) must contain at least one element",
                    );
                }
                arr.iter().map(|(_, v)| v.deref()).collect()
            }
            v => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #1 ($value) must be of type array, {} given",
                        Vm::type_name_given(&v)
                    ),
                );
            }
        }
    } else {
        a.vals.iter().map(|v| v.deref()).collect()
    };
    let mut it = vals.into_iter();
    let mut best = it.next().unwrap_or(Value::Null);
    for v in it {
        let c = php_compare(vm, &v, &best)?;
        if (want_max && c > 0) || (!want_max && c < 0) {
            best = v;
        }
    }
    Ok(best)
}

fn max(vm: &mut Vm, a: &mut Args) -> R<Value> {
    minmax(vm, a, true)
}
fn min(vm: &mut Vm, a: &mut Args) -> R<Value> {
    minmax(vm, a, false)
}

// ---------------------------------------------------------------- bases

fn number(n: Number) -> Value {
    match n {
        Number::Int(i) => Value::Int(i),
        Number::Float(f) => Value::Float(f),
    }
}

fn base_convert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut d = Vec::new();
    let r = std_ext::math::base_convert(a.bytes(0), a.int(1), a.int(2), &mut d);
    let r = sr(vm, r)?;
    base_diags(vm, d)?;
    Ok(sv(r))
}

fn parse_base_fn(vm: &mut Vm, a: &mut Args, base: u32) -> R<Value> {
    let mut d = Vec::new();
    let n = std_ext::math::parse_base(a.bytes(0), base, &mut d);
    base_diags(vm, d)?;
    Ok(number(n))
}

fn bindec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    parse_base_fn(vm, a, 2)
}
fn hexdec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    parse_base_fn(vm, a, 16)
}
fn octdec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    parse_base_fn(vm, a, 8)
}
fn decbin(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::math::dec_to_base(a.int(0), 2)))
}
fn dechex(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::math::dec_to_base(a.int(0), 16)))
}
fn decoct(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(std_ext::math::dec_to_base(a.int(0), 8)))
}

// ---------------------------------------------------------------- random

fn mt_rand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.len() {
        0 => Ok(Value::Int(mt(vm).next_int())),
        1 => Err(vm.throw_error_msg(
            "ArgumentCountError",
            &format!("{}() expects exactly 2 arguments, 1 given", a.fname()),
        )),
        _ => {
            let (lo, hi) = (a.int(0), a.int(1));
            let r = mt(vm).mt_rand_range(lo, hi);
            Ok(Value::Int(sr(vm, r)?))
        }
    }
}

fn rand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.len() {
        0 => Ok(Value::Int(mt(vm).next_int())),
        1 => Err(vm.throw_error_msg(
            "ArgumentCountError",
            &format!("{}() expects exactly 2 arguments, 1 given", a.fname()),
        )),
        _ => {
            let (lo, hi) = (a.int(0), a.int(1));
            Ok(Value::Int(mt(vm).rand_range(lo, hi)))
        }
    }
}

fn mt_srand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let seed = match a.opt_int(0) {
        Some(s) => s,
        None => os_u64() as u32 as i64,
    };
    let mode = MtMode::from_int(a.int_or(1, 0));
    if mode == MtMode::Php {
        vm.deprecated("The MT_RAND_PHP variant of Mt19937 is deprecated")?;
    }
    vm.ext::<MtState>().generator = Some(Mt19937::new(seed, mode));
    Ok(Value::Null)
}

fn mt_getrandmax(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(std_ext::mt_rand::MT_RAND_MAX))
}

fn random_int(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (lo, hi) = (a.int(0), a.int(1));
    if lo > hi {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($min) must be less than or equal to argument #2 ($max)",
        );
    }
    Ok(Value::Int(os_range(lo, hi)))
}

fn random_bytes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    if n < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($length) must be greater than 0",
        );
    }
    let mut buf = vec![0u8; n as usize];
    if !os_random(&mut buf) {
        return vm.throw("Random\\RandomException", "Failed to generate random bytes");
    }
    Ok(Value::Str(PhpStr::from(buf)))
}

fn lcg_value(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function lcg_value() is deprecated since 8.4, use \\Random\\Randomizer::getFloat() instead",
    )?;
    // A uniform float in (0, 1).
    loop {
        let r = (os_u64() >> 11) as f64 / (1u64 << 53) as f64;
        if r > 0.0 {
            return Ok(Value::Float(r));
        }
    }
}
