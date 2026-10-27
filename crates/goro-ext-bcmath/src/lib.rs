//! php-src's ext/bcmath: arbitrary precision decimal arithmetic.
//!
//! `num` is a port of libbcmath; this module holds the PHP functions
//! (bcmath.c) and the `bcmath.scale` INI entry; `number` the
//! `BcMath\Number` class.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod num;
mod number;

use num::Num;

/// The extension name (`extension_loaded('bcmath')`).
pub const NAME: &[u8] = b"bcmath";

type R = Result<Value, VmError>;

const INT_MAX: i64 = i32::MAX as i64;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("bcadd", bcadd),
    ("bcsub", bcsub),
    ("bcmul", bcmul),
    ("bcdiv", bcdiv),
    ("bcmod", bcmod),
    ("bcdivmod", bcdivmod),
    ("bcpowmod", bcpowmod),
    ("bcpow", bcpow),
    ("bcsqrt", bcsqrt),
    ("bccomp", bccomp),
    ("bcscale", bcscale),
    ("bcfloor", bcfloor),
    ("bcceil", bcceil),
    ("bcround", bcround),
];

/// MINIT: functions, the INI entry and the BcMath\Number class.
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        // Plain Z_PARAM_STR / Z_PARAM_LONG_OR_NULL parsing: the stub
        // arginfo reproduces it (bcround's RoundingMode is checked here).
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | 2,
        );
    }
    vm.constants.insert(
        b"bcmath.scale".to_vec(),
        Value::String(PhpString::from_bytes(b"0")),
    );
    vm.ini_validators
        .insert(b"bcmath.scale".to_vec(), on_update_scale);
    number::register(vm);
}

/// OnUpdateScale: zend_ini_parse_quantity_warn(), rejecting values outside
/// 0..=INT_MAX.
fn on_update_scale(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    let (v, err) = goro_core::ini_quantity::parse_quantity(value, true);
    if let Some(e) = err {
        vm.emit_warning(&format!("Invalid \"bcmath.scale\" setting. {}", e));
    }
    let v = v as i64;
    (0..=INT_MAX).contains(&v)
}

/// BCG(bc_precision)
pub(crate) fn default_scale(vm: &Vm) -> usize {
    let v = vm
        .constants
        .get(b"bcmath.scale".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let (n, _) = goro_core::ini_quantity::parse_quantity(&v, true);
    let n = n as i64;
    if (0..=INT_MAX).contains(&n) {
        n as usize
    } else {
        0
    }
}

pub(crate) fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> R {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Ok(Value::Null)
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

/// bc_num2str_ex() as the return value; the string is allocated first
/// (a huge scale exhausts memory_limit, as in PHP).
fn ret_num(vm: &mut Vm, n: &Num, scale: usize) -> R {
    num2str_check(vm, n, scale)?;
    Ok(str_val(num::num2str(n, scale)))
}

/// bc_num2str_ex()'s zend_string_alloc(n_len [+ scale + 1] + signch): the
/// 24-byte header and the NUL, rounded up to ZEND_MM_ALIGNMENT, against
/// memory_limit.
fn num2str_check(vm: &mut Vm, n: &Num, scale: usize) -> Result<(), VmError> {
    let signch = usize::from(n.neg && !n.is_zero_for_scale(n.scale.min(scale)));
    let len = if scale > 0 {
        n.len.saturating_add(scale).saturating_add(1)
    } else {
        n.len
    }
    .saturating_add(signch);
    let size = len.saturating_add(25);
    vm.memory_check(size.checked_next_multiple_of(8).unwrap_or(size))
}

/// The bc_num allocated for a quotient (or power) of `scale` digits:
/// sizeof(bc_struct) + 1 + scale, rounded up to ZEND_MM_ALIGNMENT.
pub(crate) fn check_alloc(vm: &mut Vm, scale: usize) -> Result<(), VmError> {
    let size = scale.saturating_add(33);
    vm.memory_check(size.checked_next_multiple_of(8).unwrap_or(size))
}

/// bc_standard_raise()'s safe_emalloc(n, sizeof(BC_VECTOR) * 3, 0): the
/// overflow and memory-limit fatal errors of a power too large to compute.
pub(crate) fn raise_alloc_check(
    vm: &mut Vm,
    base: &num::Num,
    exponent: i64,
) -> Result<(), VmError> {
    let Some(n) = num::raise_buffer_vectors(base, exponent) else {
        return Ok(());
    };
    match n.checked_mul(24) {
        Some(size) => vm.memory_check(usize::try_from(size).unwrap_or(usize::MAX)),
        None => {
            let line = vm.current_line;
            Err(vm.emit_fatal_at(
                &format!(
                    "Possible integer overflow in memory allocation ({} * 24 + 0)",
                    n
                ),
                line,
            ))
        }
    }
}

fn str_val(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

/// zend_argument_value_error() prefix: "f(): Argument #n ($name)".
fn arg_prefix(func: &str, n: usize) -> String {
    let name = goro_core::stubs::find_function(func.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("");
    format!("{}(): Argument #{} (${})", func, n, name)
}

fn value_error(vm: &mut Vm, func: &str, n: usize, what: &str) -> R {
    let msg = format!("{} {}", arg_prefix(func, n), what);
    throw(vm, b"ValueError", &msg)
}

/// The `?int $scale` argument at index `i`: Ok(None) when an exception was
/// thrown.
fn scale_arg(vm: &mut Vm, func: &str, args: &[Value], i: usize) -> Result<Option<usize>, VmError> {
    match arg(args, i) {
        Value::Undef | Value::Null => Ok(Some(default_scale(vm))),
        v => {
            let s = v.to_long();
            if !(0..=INT_MAX).contains(&s) {
                value_error(
                    vm,
                    func,
                    i + 1,
                    &format!("must be between 0 and {}", INT_MAX),
                )?;
                return Ok(None);
            }
            Ok(Some(s as usize))
        }
    }
}

/// php_str2num(): Ok(None) when the argument is malformed (ValueError
/// thrown).
fn str_arg(vm: &mut Vm, func: &str, args: &[Value], i: usize) -> Result<Option<Num>, VmError> {
    let s = arg(args, i).to_php_string();
    match num::str2num(s.as_bytes(), 0, true).0 {
        Some(n) => Ok(Some(n)),
        None => {
            value_error(vm, func, i + 1, "is not well-formed")?;
            Ok(None)
        }
    }
}

macro_rules! tri {
    ($e:expr) => {
        match $e? {
            Some(v) => v,
            None => return Ok(Value::Null),
        }
    };
}

fn binary(vm: &mut Vm, args: &[Value], func: &str, op: fn(&Num, &Num, usize) -> Num) -> R {
    let scale = tri!(scale_arg(vm, func, args, 2));
    let a = tri!(str_arg(vm, func, args, 0));
    let b = tri!(str_arg(vm, func, args, 1));
    let r = op(&a, &b, scale);
    ret_num(vm, &r, scale)
}

fn bcadd(vm: &mut Vm, args: &[Value]) -> R {
    binary(vm, args, "bcadd", num::add)
}

fn bcsub(vm: &mut Vm, args: &[Value]) -> R {
    binary(vm, args, "bcsub", num::sub)
}

fn bcmul(vm: &mut Vm, args: &[Value]) -> R {
    binary(vm, args, "bcmul", num::multiply)
}

fn bcdiv(vm: &mut Vm, args: &[Value]) -> R {
    let scale = tri!(scale_arg(vm, "bcdiv", args, 2));
    let a = tri!(str_arg(vm, "bcdiv", args, 0));
    let b = tri!(str_arg(vm, "bcdiv", args, 1));
    check_alloc(vm, scale)?;
    match num::divide(&a, &b, scale) {
        Some(r) => ret_num(vm, &r, scale),
        None => throw(vm, b"DivisionByZeroError", "Division by zero"),
    }
}

fn bcmod(vm: &mut Vm, args: &[Value]) -> R {
    let scale = tri!(scale_arg(vm, "bcmod", args, 2));
    let a = tri!(str_arg(vm, "bcmod", args, 0));
    let b = tri!(str_arg(vm, "bcmod", args, 1));
    match num::modulo(&a, &b, scale) {
        Some(r) => ret_num(vm, &r, scale),
        None => throw(vm, b"DivisionByZeroError", "Modulo by zero"),
    }
}

fn bcdivmod(vm: &mut Vm, args: &[Value]) -> R {
    let scale = tri!(scale_arg(vm, "bcdivmod", args, 2));
    let a = tri!(str_arg(vm, "bcdivmod", args, 0));
    let b = tri!(str_arg(vm, "bcdivmod", args, 1));
    match num::divmod(&a, &b, scale) {
        Some((q, r)) => {
            let mut arr = goro_core::array::PhpArray::new();
            arr.push(str_val(num::num2str(&q, 0)));
            num2str_check(vm, &r, scale)?;
            arr.push(str_val(num::num2str(&r, scale)));
            Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr))))
        }
        None => throw(vm, b"DivisionByZeroError", "Division by zero"),
    }
}

fn bcpowmod(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bcpowmod";
    let scale = tri!(scale_arg(vm, f, args, 3));
    let base = tri!(str_arg(vm, f, args, 0));
    let expo = tri!(str_arg(vm, f, args, 1));
    let md = tri!(str_arg(vm, f, args, 2));
    use num::RaiseModError::*;
    match num::raisemod(&base, &expo, &md, scale) {
        Ok(r) => ret_num(vm, &r, scale),
        Err(BaseHasFractional) => value_error(vm, f, 1, "cannot have a fractional part"),
        Err(ExpoHasFractional) => value_error(vm, f, 2, "cannot have a fractional part"),
        Err(ExpoIsNegative) => value_error(vm, f, 2, "must be greater than or equal to 0"),
        Err(ModHasFractional) => value_error(vm, f, 3, "cannot have a fractional part"),
        Err(ModIsZero) => throw(vm, b"DivisionByZeroError", "Modulo by zero"),
    }
}

/// bc_pow_err(): `arg_num` 0 for an operator.
pub(crate) fn pow_err(vm: &mut Vm, e: num::RaiseError, prefix: Option<String>) -> R {
    match e {
        num::RaiseError::DivideByZero => {
            throw(vm, b"DivisionByZeroError", "Negative power of zero")
        }
        _ => {
            let what = "exponent is too large, the number of digits overflowed";
            let msg = match prefix {
                Some(p) => format!("{} {}", p, what),
                None => what.to_string(),
            };
            throw(vm, b"ValueError", &msg)
        }
    }
}

fn bcpow(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bcpow";
    let scale = tri!(scale_arg(vm, f, args, 2));
    let base = tri!(str_arg(vm, f, args, 0));
    let expo = tri!(str_arg(vm, f, args, 1));
    if expo.scale != 0 {
        return value_error(vm, f, 2, "cannot have a fractional part");
    }
    let exponent = num::num2long(&expo);
    if exponent == 0 && (expo.len > 1 || expo.v[0] != 0) {
        return value_error(vm, f, 2, "is too large");
    }
    if exponent < 0 {
        check_alloc(vm, scale)?;
    }
    raise_alloc_check(vm, &base, exponent)?;
    match num::raise(&base, exponent, scale) {
        Ok(r) => ret_num(vm, &r, scale),
        Err(e) => pow_err(vm, e, Some(arg_prefix(f, 2))),
    }
}

fn bcsqrt(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bcsqrt";
    let scale = tri!(scale_arg(vm, f, args, 1));
    let n = tri!(str_arg(vm, f, args, 0));
    check_alloc(vm, scale)?;
    match num::sqrt(&n, scale) {
        Some(r) => ret_num(vm, &r, scale),
        None => value_error(vm, f, 1, "must be greater than or equal to 0"),
    }
}

fn bccomp(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bccomp";
    let scale = tri!(scale_arg(vm, f, args, 2));
    let mut nums = Vec::with_capacity(2);
    for i in 0..2 {
        let s = arg(args, i).to_php_string();
        match num::str2num(s.as_bytes(), scale, false).0 {
            Some(n) => nums.push(n),
            None => return value_error(vm, f, i + 1, "is not well-formed"),
        }
    }
    Ok(Value::Long(num::compare(&nums[0], &nums[1], scale) as i64))
}

fn floor_or_ceil(vm: &mut Vm, args: &[Value], f: &str, is_floor: bool) -> R {
    let n = tri!(str_arg(vm, f, args, 0));
    let r = num::floor_or_ceil(&n, is_floor);
    Ok(str_val(num::num2str(&r, 0)))
}

fn bcfloor(vm: &mut Vm, args: &[Value]) -> R {
    floor_or_ceil(vm, args, "bcfloor", true)
}

fn bcceil(vm: &mut Vm, args: &[Value]) -> R {
    floor_or_ceil(vm, args, "bcceil", false)
}

/// php_math_round_mode_from_enum(): the mode of a RoundingMode case, or
/// None (TypeError message) for another value.
pub(crate) fn rounding_mode(v: &Value) -> Result<i64, String> {
    if let Value::Object(o) = v {
        let o = o.borrow();
        if o.class_name.eq_ignore_ascii_case(b"RoundingMode") {
            return Ok(match o.get_property(b"name").to_php_string().as_bytes() {
                b"HalfAwayFromZero" => num::ROUND_HALF_UP,
                b"HalfTowardsZero" => num::ROUND_HALF_DOWN,
                b"HalfEven" => num::ROUND_HALF_EVEN,
                b"HalfOdd" => num::ROUND_HALF_ODD,
                b"PositiveInfinity" => num::ROUND_CEILING,
                b"NegativeInfinity" => num::ROUND_FLOOR,
                b"TowardsZero" => num::ROUND_TOWARD_ZERO,
                b"AwayFromZero" => num::ROUND_AWAY_FROM_ZERO,
                _ => 0,
            });
        }
    }
    Err(Vm::value_type_name(v))
}

fn bcround(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bcround";
    let precision = match arg(args, 1) {
        Value::Undef => 0,
        v => v.to_long(),
    };
    let mode = match arg(args, 2) {
        Value::Undef => num::ROUND_HALF_UP,
        v => match rounding_mode(&v) {
            Ok(m) => m,
            Err(given) => {
                let msg = format!(
                    "{} must be of type RoundingMode, {} given",
                    arg_prefix(f, 3),
                    given
                );
                return throw(vm, b"TypeError", &msg);
            }
        },
    };
    if !(1..=8).contains(&mode) {
        return value_error(vm, f, 3, "is an unsupported rounding mode");
    }
    let n = tri!(str_arg(vm, f, args, 0));
    if let Some(size) = num::round_alloc_size(&n, precision, mode) {
        vm.memory_check(usize::try_from(size).unwrap_or(usize::MAX))?;
    }
    let (r, scale) = num::round(&n, precision, mode);
    ret_num(vm, &r, scale)
}

fn bcscale(vm: &mut Vm, args: &[Value]) -> R {
    let old = default_scale(vm) as i64;
    match arg(args, 0) {
        Value::Undef | Value::Null => {}
        v => {
            let n = v.to_long();
            if !(0..=INT_MAX).contains(&n) {
                return value_error(
                    vm,
                    "bcscale",
                    1,
                    &format!("must be between 0 and {}", INT_MAX),
                );
            }
            // zend_alter_ini_entry("bcmath.scale")
            let s = Value::String(PhpString::from_string(n.to_string()));
            if !vm.ini_originals.contains_key(b"bcmath.scale".as_slice()) {
                let cur = vm.constants.get(b"bcmath.scale".as_slice()).cloned();
                vm.ini_originals.insert(b"bcmath.scale".to_vec(), cur);
            }
            vm.constants.insert(b"bcmath.scale".to_vec(), s);
        }
    }
    Ok(Value::Long(old))
}
