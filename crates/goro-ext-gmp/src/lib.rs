mod mp;
mod mt;

use puremp::Int;

use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::object::NativeData;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// The number of a GMP object (its native data).
struct GmpNum(Int);

static GMP_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    get_properties: Some(gmp_get_properties),
    cast: Some(gmp_cast),
    compare: Some(gmp_compare_handler),
    clone_obj: Some(gmp_clone_obj),
    ..NativeObjectHandlers::EMPTY
};

/// gmp_get_debug_info(): the properties plus `num` (debug purpose only;
/// the number is no property).
fn gmp_get_properties(
    _vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    purpose: u8,
) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let mut arr = PhpArray::new();
    let n = obj_num(obj);
    {
        let ob = obj.borrow();
        for (k, v) in ob.properties.iter() {
            if matches!(v, Value::Undef) || k.starts_with(b"__") {
                continue;
            }
            arr.set(
                Vm::value_to_array_key(Value::String(PhpString::from_vec(k.clone()))),
                v.clone(),
            );
        }
    }
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"num")),
        Value::String(PhpString::from_string(n.to_string())),
    );
    Some(arr)
}

/// gmp_cast_object(): bool / int / float / string.
fn gmp_cast(obj: &Rc<RefCell<PhpObject>>, kind: u8) -> Option<Value> {
    let n = obj_num(obj);
    Some(match kind {
        0 => {
            if n.is_zero() {
                Value::False
            } else {
                Value::True
            }
        }
        1 => Value::Long(mp::get_si(&n)),
        2 => Value::Double(n.to_f64_trunc()),
        _ => Value::String(PhpString::from_string(n.to_string())),
    })
}

/// gmp_compare() for comparisons made outside the operators (sorting,
/// in_array()...), without its diagnostics.
fn gmp_compare_handler(a: &Value, b: &Value) -> Option<i64> {
    fn num(v: &Value) -> Option<Int> {
        match v.deref() {
            v @ Value::Object(_) if is_gmp_object(&v) => get_gmp_value(&v),
            Value::Long(n) => Some(Int::from(n)),
            Value::String(s) => parse_bigint_str_strict(&s.to_string_lossy(), 0).ok(),
            Value::Double(d)
                if !d.is_nan()
                    && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) =>
            {
                Some(Int::from(d as i64))
            }
            _ => None,
        }
    }
    if matches!(a.deref(), Value::Object(_)) && !is_gmp_object(a) {
        return None;
    }
    let bool_of = |v: &Value| match v.deref() {
        Value::True => Some(true),
        Value::False | Value::Null | Value::Undef => Some(false),
        _ => None,
    };
    if let Some(o) = bool_of(b) {
        let me = get_gmp_value(a).is_some_and(|n| !n.is_zero());
        return Some(me.cmp(&o) as i64);
    }
    if let Some(o) = bool_of(a) {
        let me = get_gmp_value(b).is_some_and(|n| !n.is_zero());
        return Some(o.cmp(&me) as i64);
    }
    match (num(a), num(b)) {
        (Some(x), Some(y)) => Some(x.cmp(&y) as i64),
        _ => Some(2),
    }
}

fn gmp_clone_obj(_vm: &mut Vm, src: &Rc<RefCell<PhpObject>>, dst: &Rc<RefCell<PhpObject>>) {
    let n = obj_num(src);
    attach_num(dst, n);
}

fn attach_num(obj: &Rc<RefCell<PhpObject>>, n: Int) {
    obj.borrow_mut().native = Some(NativeData {
        data: Rc::new(GmpNum(n)),
        handlers: &GMP_HANDLERS,
    });
}

/// The number of a GMP object; 0 for one never initialized.
fn obj_num(obj: &Rc<RefCell<PhpObject>>) -> Int {
    let ob = obj.borrow();
    ob.native
        .as_ref()
        .and_then(|n| n.data.downcast_ref::<GmpNum>())
        .map(|g| g.0.clone())
        .unwrap_or(Int::ZERO)
}

/// Register all GMP extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"gmp");
    // Functions
    vm.register_function(b"gmp_init", gmp_init);
    vm.register_function(b"gmp_strval", gmp_strval);
    vm.register_function(b"gmp_intval", gmp_intval);
    vm.register_function(b"gmp_add", gmp_add);
    vm.register_function(b"gmp_sub", gmp_sub);
    vm.register_function(b"gmp_mul", gmp_mul);
    vm.register_function(b"gmp_div_q", gmp_div_q);
    vm.register_function(b"gmp_div_r", gmp_div_r);
    vm.register_function(b"gmp_div_qr", gmp_div_qr);
    vm.register_function(b"gmp_div", gmp_div); // alias of gmp_div_q
    vm.register_function(b"gmp_divexact", gmp_divexact);
    vm.register_function(b"gmp_mod", gmp_mod);
    vm.register_function(b"gmp_neg", gmp_neg);
    vm.register_function(b"gmp_abs", gmp_abs);
    vm.register_function(b"gmp_cmp", gmp_cmp);
    vm.register_function(b"gmp_sign", gmp_sign);
    vm.register_function(b"gmp_pow", gmp_pow);
    vm.register_function(b"gmp_powm", gmp_powm);
    vm.register_function(b"gmp_sqrt", gmp_sqrt);
    vm.register_function(b"gmp_sqrtrem", gmp_sqrtrem);
    vm.register_function(b"gmp_root", gmp_root);
    vm.register_function(b"gmp_rootrem", gmp_rootrem);
    vm.register_function(b"gmp_perfect_power", gmp_perfect_power);
    vm.register_function(b"gmp_gcd", gmp_gcd);
    vm.register_function(b"gmp_gcdext", gmp_gcdext);
    vm.register_function(b"gmp_lcm", gmp_lcm);
    vm.register_function(b"gmp_and", gmp_and);
    vm.register_function(b"gmp_or", gmp_or);
    vm.register_function(b"gmp_xor", gmp_xor);
    vm.register_function(b"gmp_com", gmp_com);
    vm.register_function(b"gmp_fact", gmp_fact);
    vm.register_function(b"gmp_prob_prime", gmp_prob_prime);
    vm.register_function(b"gmp_nextprime", gmp_nextprime);
    vm.register_function(b"gmp_testbit", gmp_testbit);
    vm.register_function(b"gmp_setbit", gmp_setbit);
    vm.register_function(b"gmp_clrbit", gmp_clrbit);
    vm.register_function(b"gmp_popcount", gmp_popcount);
    vm.register_function(b"gmp_hamdist", gmp_hamdist);
    vm.register_function(b"gmp_scan0", gmp_scan0);
    vm.register_function(b"gmp_scan1", gmp_scan1);
    vm.register_function(b"gmp_perfect_square", gmp_perfect_square);
    vm.register_function(b"gmp_invert", gmp_invert);
    vm.register_function(b"gmp_jacobi", gmp_jacobi);
    vm.register_function(b"gmp_legendre", gmp_legendre);
    vm.register_function(b"gmp_kronecker", gmp_kronecker);
    vm.register_function(b"gmp_binomial", gmp_binomial);
    vm.register_function(b"gmp_export", gmp_export_fn);
    vm.register_function(b"gmp_import", gmp_import_fn);
    vm.register_function(b"gmp_random_bits", gmp_random_bits);
    vm.register_function(b"gmp_random_range", gmp_random_range);
    vm.register_function(b"gmp_random_seed", gmp_random_seed);

    // Register operator overloading handlers
    vm.object_do_operation = Some(gmp_do_operation);
    vm.object_do_unary = Some(gmp_do_unary);
    vm.object_compare = Some(gmp_compare);
    vm.object_to_string = Some(gmp_to_string);
    vm.object_to_long = Some(gmp_to_long);
    vm.object_to_double = Some(gmp_to_double);
    vm.object_is_operator_type = Some(is_gmp_object);
    vm.gmp_to_hex = Some(gmp_to_hex_str);
    vm.gmp_from_hex = Some(gmp_set_from_hex_str);
    vm.gmp_from_dec = Some(gmp_set_from_dec_str);
    vm.object_construct_handlers
        .insert(b"gmp".to_vec(), gmp_construct_native);

    // Constants
    vm.constants
        .insert(b"GMP_ROUND_ZERO".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"GMP_ROUND_PLUSINF".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"GMP_ROUND_MINUSINF".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"GMP_MSW_FIRST".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"GMP_LSW_FIRST".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"GMP_LITTLE_ENDIAN".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"GMP_BIG_ENDIAN".to_vec(), Value::Long(8));
    vm.constants
        .insert(b"GMP_NATIVE_ENDIAN".to_vec(), Value::Long(16));
    vm.constants.insert(
        b"GMP_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(b"6.3.0")),
    );

    // Register the GMP class as a final, non-instantiable-via-extends class.
    // The number is native data (no property; `num` shows in debug output).
    let mut ce = goro_core::object::ClassEntry::new(b"GMP".to_vec());
    ce.is_final = true;
    vm.classes.insert(b"gmp".to_vec(), ce);
}

// ============================================================
// Public API for VM integration (operator overloading, casting)
// ============================================================

/// Check if a Value is a GMP object
pub fn is_gmp_object(val: &Value) -> bool {
    match val {
        Value::Object(obj) => {
            let b = obj.borrow();
            b.class_name.eq_ignore_ascii_case(b"GMP")
        }
        Value::Reference(r) => is_gmp_object(&r.borrow()),
        _ => false,
    }
}

/// Get the Int value from a GMP object
pub fn get_gmp_value(val: &Value) -> Option<Int> {
    match val {
        Value::Object(obj) if is_gmp_object(val) => Some(obj_num(obj)),
        Value::Reference(r) => get_gmp_value(&r.borrow()),
        _ => None,
    }
}

/// Encode a ValueError with the magic prefix understood by VM op handler.
fn ve(msg: &str) -> String {
    format!("\x01V\x01{}", msg)
}

/// Encode a DivisionByZeroError with the magic prefix understood by VM op handler.
fn dze(msg: &str) -> String {
    format!("\x01D\x01{}", msg)
}

/// gmp_zend_parse_arg_into_mpz_ex() with `is_operator`: Ok(number), or
/// Err(message) when the operand is not usable (a ValueError message is
/// prefixed; None as message means the conversion threw already).
fn op_arg(vm: &mut Vm, val: &Value) -> Result<Int, Option<String>> {
    match val {
        Value::Reference(r) => {
            let v = r.borrow().clone();
            op_arg(vm, &v)
        }
        // Only GMP objects are accepted.
        Value::Object(_) if is_gmp_object(val) => get_gmp_value(val).ok_or(None),
        Value::Object(_) => Err(None),
        // convert_zstr_to_gmp(..., base 0, arg_num 0)
        Value::String(s) => parse_bigint_str_strict(&s.to_string_lossy(), 0)
            .map_err(|_| Some(ve("Number is not an integer string"))),
        Value::Long(n) => Ok(Int::from(*n)),
        // zend_parse_arg_long_weak(); null is rejected for operators
        Value::Double(d) => {
            let d = *d;
            if d.is_nan() || !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
                return Err(None);
            }
            let l = d as i64;
            if l as f64 != d {
                vm.emit_deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_core::value::format_php_float_shortest_pub(d)
                ));
                if vm.current_exception.is_some() {
                    return Err(Some(String::new()));
                }
            }
            Ok(Int::from(l))
        }
        Value::False => Ok(Int::ZERO),
        Value::True => Ok(Int::ONE),
        _ => Err(None),
    }
}

/// binop_operator_helper()'s operand conversion with its TypeError.
fn op_arg_or_type_error(vm: &mut Vm, val: &Value) -> Result<Int, String> {
    op_arg(vm, val).map_err(|e| {
        e.unwrap_or_else(|| {
            format!(
                "Number must be of type GMP|string|int, {} given",
                value_type_display_for_gmp(val)
            )
        })
    })
}

/// zend_zval_type_name()
fn zval_type_name(val: &Value) -> String {
    match val {
        Value::Reference(r) => zval_type_name(&r.borrow()),
        Value::Object(o) => String::from_utf8_lossy(&o.borrow().class_name).into_owned(),
        Value::True | Value::False => "bool".to_string(),
        other => value_type_name_for_gmp(other).to_string(),
    }
}

/// shift_operator_helper(): `**`, `<<` and `>>`.
fn shift_operator_helper(vm: &mut Vm, op: &str, a: &Value, b: &Value) -> Result<Value, String> {
    let typeof_failure = || {
        format!(
            "Unsupported operand types: {} {} {}",
            zval_type_name(a),
            op,
            zval_type_name(b)
        )
    };
    let shift: i64 = match b.deref() {
        Value::Long(n) => n,
        v if is_gmp_object(&v) => gmp_to_long(&v).unwrap_or(0),
        // zval_get_long()
        Value::Double(d) => goro_core::value::Value::Double(d).to_long(),
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Long(n), _)) => n,
            _ => return Err(ve("Number is not an integer string")),
        },
        _ => return Err(typeof_failure()),
    };
    if shift < 0 {
        return Err(ve(&format!(
            "{} must be greater than or equal to 0",
            if op == "**" { "Exponent" } else { "Shift" }
        )));
    }
    let n = match op_arg(vm, a) {
        Ok(n) => n,
        // A deprecation handler threw: that exception stays.
        Err(_) if vm.current_exception.is_some() => return Err(String::new()),
        Err(_) => return Err(typeof_failure()),
    };
    Ok(bigint_to_gmp_value(
        vm,
        match op {
            "**" => n.pow(shift as u64),
            "<<" => n.mul_2k(shift as u64),
            // mpz_fdiv_q_2exp
            _ => n.div_2k_floor(shift as u64),
        },
    ))
}

/// gmp_do_operation(): the do_operation object handler.
/// `op` is one of: "+", "-", "*", "/", "%", "**", "|", "&", "^", "<<", ">>"
/// Returns None if neither operand is a GMP object.
pub fn gmp_do_operation(
    vm: &mut Vm,
    op: &str,
    a: &Value,
    b: &Value,
) -> Option<Result<Value, String>> {
    if !is_gmp_object(a) && !is_gmp_object(b) {
        return None; // Not a GMP operation
    }
    if matches!(op, "**" | "<<" | ">>") {
        return Some(shift_operator_helper(vm, op, a, b));
    }
    if !matches!(op, "+" | "-" | "*" | "/" | "%" | "|" | "&" | "^") {
        return None;
    }
    let x = match op_arg_or_type_error(vm, a) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    let y = match op_arg_or_type_error(vm, b) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    let r = match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        // mpz_tdiv_q
        "/" => {
            if y.is_zero() {
                return Some(Err(dze("Division by zero")));
            }
            x.div_trunc(&y)
        }
        // mpz_mod: the result is never negative
        "%" => {
            if y.is_zero() {
                return Some(Err(dze("Modulo by zero")));
            }
            x.rem_euclid(&y)
        }
        "|" => x.bitor(&y),
        "&" => x.bitand(&y),
        _ => x.bitxor(&y),
    };
    Some(Ok(bigint_to_gmp_value(vm, r)))
}

/// Perform GMP unary operation: "~" (complement), "-" (negation), "+" (identity)
pub fn gmp_do_unary(vm: &mut Vm, op: &str, a: &Value) -> Option<Value> {
    if !is_gmp_object(a) {
        return None;
    }
    let n = get_gmp_value(a)?;
    match op {
        "~" => Some(bigint_to_gmp_value(vm, -(n + Int::ONE))),
        "-" => Some(bigint_to_gmp_value(vm, -n)),
        "+" => Some(bigint_to_gmp_value(vm, n)),
        _ => None,
    }
}

/// gmp_compare(): None (ZEND_UNCOMPARABLE) after throwing when an operand
/// is not usable.
pub fn gmp_compare(vm: &mut Vm, a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if !is_gmp_object(a) && !is_gmp_object(b) {
        return None;
    }
    // zend_compare(): (object, null) pairs never reach the handler.
    if matches!(b.deref(), Value::Null | Value::Undef) {
        return Some(std::cmp::Ordering::Greater);
    }
    if matches!(a.deref(), Value::Null | Value::Undef) {
        return Some(std::cmp::Ordering::Less);
    }
    // zend_compare() uses op1's handler: another object compares by its own.
    if matches!(a.deref(), Value::Object(_)) && !is_gmp_object(a) {
        return None;
    }
    // ... and compares an object with a bool through the bool cast.
    let as_bool = |v: &Value| match v.deref() {
        Value::True => Some(true),
        Value::False => Some(false),
        _ => None,
    };
    if let Some(other) = as_bool(b) {
        let me = get_gmp_value(a).is_some_and(|n| !n.is_zero());
        return Some(me.cmp(&other));
    }
    if let Some(other) = as_bool(a) {
        let me = get_gmp_value(b).is_some_and(|n| !n.is_zero());
        return Some(other.cmp(&me));
    }
    let get = |vm: &mut Vm, v: &Value| match op_arg_or_type_error(vm, v) {
        Ok(n) => Some(n),
        Err(e) => {
            if vm.current_exception.is_none() && !e.is_empty() {
                let (class, msg) = Vm::classify_object_op_error(&e);
                let exc = vm.create_exception(class, &msg, vm.current_line);
                vm.current_exception = Some(exc);
            }
            None
        }
    };
    let x = get(vm, a)?;
    let y = get(vm, b)?;
    Some(x.cmp(&y))
}

/// Convert a GMP object to its string representation (for __toString / concatenation)
pub fn gmp_to_string(val: &Value) -> Option<String> {
    if !is_gmp_object(val) {
        return None;
    }
    get_gmp_value(val).map(|n| n.to_string())
}

/// Convert a GMP object to i64 (for (int) cast)
pub fn gmp_to_long(val: &Value) -> Option<i64> {
    if !is_gmp_object(val) {
        return None;
    }
    get_gmp_value(val).map(|n| mp::get_si(&n))
}

/// Convert a GMP object to f64 (for (float) cast)
pub fn gmp_to_double(val: &Value) -> Option<f64> {
    if !is_gmp_object(val) {
        return None;
    }
    get_gmp_value(val).map(|n| n.to_f64_trunc())
}

fn value_type_name_for_gmp(val: &Value) -> &'static str {
    match val {
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::True => "true",
        Value::False => "false",
        Value::Null | Value::Undef => "null",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Resource(_) => "resource",
        Value::Reference(r) => value_type_name_for_gmp(&r.borrow()),
        _ => "unknown",
    }
}

/// Like `value_type_name_for_gmp` but returns the actual class name for
/// non-GMP objects (matching PHP's `zend_zval_value_name`). Returns an
/// owned String because the class name is dynamic.
fn value_type_display_for_gmp(val: &Value) -> String {
    match val {
        Value::Object(obj) => {
            let b = obj.borrow();
            String::from_utf8_lossy(&b.class_name).into_owned()
        }
        Value::Reference(r) => value_type_display_for_gmp(&r.borrow()),
        _ => value_type_name_for_gmp(val).to_string(),
    }
}

// ============================================================
// Helper functions
// ============================================================

/// Strict parser used by GMP function arguments. Mirrors GMP's `mpz_set_str`
/// behavior: accepts an optional `-` sign, an optional base prefix
/// (`0x`/`0X`/`0b`/`0B`/`0o`/`0O`/`0` when `base` is 0 or matches), and a
/// run of digits with whitespace allowed anywhere (including embedded).
/// Rejects:
///   * lone prefixes like "0x", "0b"
///   * a leading `+` sign (GMP itself rejects "+123")
///   * empty strings or strings with no digits at all
///   * strings whose digits don't fit the inferred base (e.g. "09" with base 0)
///
/// `base == 0` enables prefix auto-detection (PHP/GMP default).
fn parse_bigint_str_strict(s: &str, base: u32) -> Result<Int, String> {
    // Strip leading whitespace.
    let s_trimmed = s.trim_start();
    if s_trimmed.is_empty() {
        return Err("empty".to_string());
    }
    let bytes = s_trimmed.as_bytes();
    let (negative, start) = match bytes[0] {
        b'-' => (true, 1),
        _ => (false, 0),
    };
    if start >= bytes.len() {
        return Err("only sign".to_string());
    }

    // Auto-detect prefix when base is 0 or compatible.
    let (actual_base, digit_start, allow_octal_zero_prefix) =
        if bytes.len() > start + 1 && bytes[start] == b'0' {
            match bytes[start + 1] {
                b'x' | b'X' if base == 0 || base == 16 => (16u32, start + 2, false),
                b'b' | b'B' if base == 0 || base == 2 => (2u32, start + 2, false),
                b'o' | b'O' if base == 0 || base == 8 => (8u32, start + 2, false),
                _ if base == 0 => (8u32, start + 1, true), // leading "0" -> octal in base 0
                _ => (if base == 0 { 10 } else { base }, start, false),
            }
        } else if base == 0 {
            (10u32, start, false)
        } else {
            (base, start, false)
        };

    // Collect digit characters, skipping whitespace.
    let raw_slice = &bytes[digit_start..];
    let mut digits: Vec<u8> = Vec::with_capacity(raw_slice.len());
    for &ch in raw_slice {
        if (ch as char).is_ascii_whitespace() {
            continue;
        }
        if mp::digit_value(ch, actual_base).is_none() {
            return Err(format!("invalid digit for base {}", actual_base));
        }
        digits.push(ch);
    }

    if digits.is_empty() {
        if allow_octal_zero_prefix {
            return Ok(Int::ZERO);
        }
        return Err("no digits".to_string());
    }

    let n = mp::parse_digits(&digits, actual_base).ok_or_else(|| "parse failed".to_string())?;

    if negative { Ok(-n) } else { Ok(n) }
}

/// Extract a Int from a Value for GMP function arguments.
/// Returns Ok(Int) on success, Err(error_kind) on failure.
/// error_kind: "type" for TypeError, "value" for ValueError
fn value_to_bigint_checked(
    val: &Value,
    func_name: &str,
    arg_num: u32,
    arg_name: &str,
) -> Result<Int, GmpArgError> {
    match val {
        Value::Object(obj) => {
            let b = obj.borrow();
            if b.class_name.eq_ignore_ascii_case(b"GMP") {
                drop(b);
                Ok(obj_num(obj))
            } else {
                Err(GmpArgError::Type(format!(
                    "{}(): Argument #{} (${}) must be of type GMP|string|int, {} given",
                    func_name,
                    arg_num,
                    arg_name,
                    String::from_utf8_lossy(&b.class_name)
                )))
            }
        }
        Value::Long(n) => Ok(Int::from(*n)),
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            // GMP function args use base 0 (autodetect: 0x, 0b, 0o, 0 prefix).
            parse_bigint_str_strict(&s_str, 0).map_err(|_| {
                GmpArgError::Value(format!(
                    "{}(): Argument #{} (${}) is not an integer string",
                    func_name, arg_num, arg_name
                ))
            })
        }
        Value::Double(_) => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, float given",
            func_name, arg_num, arg_name
        ))),
        Value::True => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, true given",
            func_name, arg_num, arg_name
        ))),
        Value::False => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, false given",
            func_name, arg_num, arg_name
        ))),
        Value::Null | Value::Undef => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, null given",
            func_name, arg_num, arg_name
        ))),
        Value::Array(_) => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, array given",
            func_name, arg_num, arg_name
        ))),
        Value::Resource(_) => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, resource given",
            func_name, arg_num, arg_name
        ))),
        Value::Reference(r) => value_to_bigint_checked(&r.borrow(), func_name, arg_num, arg_name),
        _ => Err(GmpArgError::Type(format!(
            "{}(): Argument #{} (${}) must be of type GMP|string|int, unknown given",
            func_name, arg_num, arg_name
        ))),
    }
}

#[derive(Debug)]
enum GmpArgError {
    Type(String),
    Value(String),
}

/// Throw the appropriate exception for a GMP argument error
fn throw_gmp_error(vm: &mut Vm, err: GmpArgError) -> VmError {
    match err {
        GmpArgError::Type(msg) => {
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            VmError {
                message: msg,
                line: vm.current_line,
            }
        }
        GmpArgError::Value(msg) => {
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            VmError {
                message: msg,
                line: vm.current_line,
            }
        }
    }
}

/// Helper to get a Int arg or throw proper exception
fn get_gmp_arg(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    func_name: &str,
    arg_name: &str,
) -> Result<Int, VmError> {
    let val = args.get(idx).unwrap_or(&Value::Null);
    value_to_bigint_checked(val, func_name, (idx + 1) as u32, arg_name)
        .map_err(|e| throw_gmp_error(vm, e))
}

/// Create a GMP object from a Int and return it as a Value
fn bigint_to_gmp_value(vm: &mut Vm, n: Int) -> Value {
    let obj_id = vm.alloc_object_id();
    let obj = Rc::new(RefCell::new(PhpObject::new(b"GMP".to_vec(), obj_id)));
    attach_num(&obj, n);
    Value::Object(obj)
}

/// Generate the hex string representation used by GMP serialize.
/// PHP's __serialize emits a lowercase hex string, with leading '-' for negatives.
pub fn gmp_to_hex_str(val: &Value) -> Option<String> {
    let n = get_gmp_value(val)?;
    Some(mp::to_str_radix(&n, 16))
}

/// Store `n` as the value of the GMP object `val` (and its `num` property).
fn set_gmp_value(val: &Value, n: Int) -> bool {
    let obj = match val {
        Value::Object(o) => o.clone(),
        Value::Reference(r) => match &*r.borrow() {
            Value::Object(o) => o.clone(),
            _ => return false,
        },
        _ => return false,
    };
    attach_num(&obj, n);
    true
}

/// Parse a hex string and update the GMP object's stored Int and `num` property.
/// Returns true on success, false if the hex string was unparseable.
pub fn gmp_set_from_hex_str(val: &Value, hex: &str) -> bool {
    match parse_gmp_hex(hex) {
        Some(n) => set_gmp_value(val, n),
        None => false,
    }
}

/// convert_zstr_to_gmp(..., base 10): the legacy serialization's number.
pub fn gmp_set_from_dec_str(val: &Value, dec: &str) -> bool {
    match parse_bigint_str_strict(dec, 10) {
        Ok(n) => set_gmp_value(val, n),
        Err(_) => false,
    }
}

/// Parse a GMP hex string (lowercase or uppercase, optional leading minus).
fn parse_gmp_hex(hex: &str) -> Option<Int> {
    if hex.is_empty() {
        return None;
    }
    let (negative, digits) = match hex.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, hex),
    };
    if digits.is_empty() {
        return None;
    }
    // Validate all are hex digits
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = mp::parse_digits(digits.as_bytes(), 16)?;
    Some(if negative { -n } else { n })
}

// ============================================================
// Rounding helper
// ============================================================

fn apply_rounding(q: Int, r: &Int, b: &Int, round: i64) -> Int {
    if r.is_zero() {
        return q;
    }
    match round {
        0 => q,
        // The remainder is non-zero here: round the truncated quotient up
        // (or down) when the exact quotient is positive (or negative).
        1 if r.is_negative() == b.is_negative() => q + Int::ONE,
        2 if r.is_negative() != b.is_negative() => q - Int::ONE,
        _ => q,
    }
}

// ============================================================
// PHP function implementations
// ============================================================

fn gmp_init_inner(
    vm: &mut Vm,
    val: &Value,
    args1: Option<&Value>,
    func_name: &str,
) -> Result<Value, VmError> {
    // Default base is 0 (autodetect) per PHP/GMP semantics.
    let base_val = args1.map(|v| v.to_long()).unwrap_or(0);

    // Validate base
    if base_val != 0 && !(2..=62).contains(&base_val) {
        let msg = format!(
            "{}(): Argument #2 ($base) must be 0 or between 2 and 62",
            func_name
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let n = match val {
        Value::Long(n) => Int::from(*n),
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            match parse_bigint_str_strict(&s_str, base_val as u32) {
                Ok(n) => n,
                Err(_) => {
                    let msg = format!(
                        "{}(): Argument #1 ($num) is not an integer string",
                        func_name
                    );
                    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
        }
        Value::Object(obj) => {
            // gmp_init signature is string|int, NOT GMP.
            let b = obj.borrow();
            let cls = String::from_utf8_lossy(&b.class_name).into_owned();
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type string|int, {} given",
                func_name, cls
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::Double(_) => {
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type string|int, float given",
                func_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::True | Value::False => {
            let t = if matches!(val, Value::True) {
                "true"
            } else {
                "false"
            };
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type string|int, {} given",
                func_name, t
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::Null | Value::Undef => {
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type string|int, null given",
                func_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::Array(_) => {
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type string|int, array given",
                func_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            return gmp_init_inner(vm, &inner, args1, func_name);
        }
        _ => {
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type string|int, unknown given",
                func_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };

    Ok(bigint_to_gmp_value(vm, n))
}

fn gmp_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    gmp_init_inner(vm, val, args.get(1), "gmp_init")
}

/// `new GMP(...)` native constructor: parses arguments like gmp_init and
/// stores the Int against `$this`'s object id. `args[0]` is `$this`.
pub fn gmp_construct_native(vm: &mut Vm, args: &[Value], line: u32) -> Result<(), String> {
    // args[0] = $this; args[1] = $num (optional, default 0); args[2] = $base (optional)
    let this_val = args.first().cloned().unwrap_or(Value::Null);
    let this_obj = match &this_val {
        Value::Object(o) => o.clone(),
        _ => {
            return Err("GMP::__construct(): missing \\$this".to_string());
        }
    };

    let num_val = args.get(1).cloned().unwrap_or(Value::Long(0));
    let base_val_ref = args.get(2);

    // Compute Int using same rules as gmp_init (but error class name is GMP::__construct).
    let base_val = base_val_ref.map(|v| v.to_long()).unwrap_or(0);
    if base_val != 0 && !(2..=62).contains(&base_val) {
        let msg =
            "GMP::__construct(): Argument #2 ($base) must be 0 or between 2 and 62".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, line);
        vm.current_exception = Some(exc);
        return Err(msg);
    }

    let n = match &num_val {
        Value::Long(n) => Int::from(*n),
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            match parse_bigint_str_strict(&s_str, base_val as u32) {
                Ok(n) => n,
                Err(_) => {
                    let msg = "GMP::__construct(): Argument #1 ($num) is not an integer string"
                        .to_string();
                    let exc = vm.create_exception(b"ValueError", &msg, line);
                    vm.current_exception = Some(exc);
                    return Err(msg);
                }
            }
        }
        Value::Null | Value::Undef => Int::ZERO,
        Value::Object(obj) => {
            let cls = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
            let msg = format!(
                "GMP::__construct(): Argument #1 ($num) must be of type string|int, {} given",
                cls
            );
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Err(msg);
        }
        Value::Double(_) => {
            let msg =
                "GMP::__construct(): Argument #1 ($num) must be of type string|int, float given"
                    .to_string();
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Err(msg);
        }
        Value::Array(_) => {
            let msg =
                "GMP::__construct(): Argument #1 ($num) must be of type string|int, array given"
                    .to_string();
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Err(msg);
        }
        Value::True | Value::False => {
            let t = if matches!(num_val, Value::True) {
                "true"
            } else {
                "false"
            };
            let msg = format!(
                "GMP::__construct(): Argument #1 ($num) must be of type string|int, {} given",
                t
            );
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Err(msg);
        }
        _ => {
            let msg =
                "GMP::__construct(): Argument #1 ($num) must be of type string|int".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Err(msg);
        }
    };

    attach_num(&this_obj, n);
    Ok(())
}

fn gmp_strval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_strval", "num")?;
    let base = int_arg_or(vm, args, 1, "gmp_strval", "base", 10)?;

    if base < -36 || (base > -2 && base < 2) || base > 62 {
        let msg =
            "gmp_strval(): Argument #2 ($base) must be between 2 and 62, or -2 and -36".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let s = if base < 0 {
        // Negative base: uppercase output
        mp::to_str_radix(&a, (-base) as u32).to_uppercase()
    } else {
        mp::to_str_radix(&a, base as u32)
    };
    Ok(Value::String(PhpString::from_string(s)))
}

fn gmp_intval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_intval", "num")?;
    Ok(Value::Long(mp::get_si(&a)))
}

fn gmp_add(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_add", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_add", "num2")?;
    Ok(bigint_to_gmp_value(vm, &a + &b))
}

fn gmp_sub(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_sub", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_sub", "num2")?;
    Ok(bigint_to_gmp_value(vm, &a - &b))
}

fn gmp_mul(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_mul", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_mul", "num2")?;
    Ok(bigint_to_gmp_value(vm, &a * &b))
}

/// Validate the rounding mode for gmp_div_q/r/qr. Returns Err with message if
/// the value is not 0/1/2.
fn validate_rounding_mode(vm: &mut Vm, round: i64, func_name: &str) -> Result<(), VmError> {
    if !(0..=2).contains(&round) {
        let msg = format!(
            "{}(): Argument #3 ($rounding_mode) must be one of GMP_ROUND_ZERO, GMP_ROUND_PLUSINF, or GMP_ROUND_MINUSINF",
            func_name
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(())
}

fn throw_div_by_zero(
    vm: &mut Vm,
    func_name: &str,
    arg_num: u32,
    arg_name: &str,
    op_word: &str,
) -> VmError {
    let msg = format!(
        "{}(): Argument #{} (${}) {}",
        func_name, arg_num, arg_name, op_word
    );
    let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

fn gmp_div_q(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    gmp_div_q_with_name(vm, args, "gmp_div_q")
}

fn gmp_div(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    gmp_div_q_with_name(vm, args, "gmp_div")
}

fn gmp_div_q_with_name(vm: &mut Vm, args: &[Value], func_name: &str) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, func_name, "num1")?;
    let b = get_gmp_arg(vm, args, 1, func_name, "num2")?;
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    validate_rounding_mode(vm, round, func_name)?;

    if b.is_zero() {
        return Err(throw_div_by_zero(
            vm,
            func_name,
            2,
            "num2",
            "Division by zero",
        ));
    }

    let (q, r) = a.div_rem_trunc(&b);
    let result = apply_rounding(q, &r, &b, round);
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_div_r(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_div_r", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_div_r", "num2")?;
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    validate_rounding_mode(vm, round, "gmp_div_r")?;

    if b.is_zero() {
        return Err(throw_div_by_zero(
            vm,
            "gmp_div_r",
            2,
            "num2",
            "Division by zero",
        ));
    }

    let (q, r) = a.div_rem_trunc(&b);
    let adjusted_q = apply_rounding(q, &r, &b, round);
    let result = &a - &(&adjusted_q * &b);
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_div_qr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_div_qr", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_div_qr", "num2")?;
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    validate_rounding_mode(vm, round, "gmp_div_qr")?;

    if b.is_zero() {
        return Err(throw_div_by_zero(
            vm,
            "gmp_div_qr",
            2,
            "num2",
            "Division by zero",
        ));
    }

    let (q, r) = a.div_rem_trunc(&b);
    let adjusted_q = apply_rounding(q, &r, &b, round);
    let adjusted_r = &a - &(&adjusted_q * &b);

    let q_val = bigint_to_gmp_value(vm, adjusted_q);
    let r_val = bigint_to_gmp_value(vm, adjusted_r);

    let mut arr = PhpArray::new();
    arr.push(q_val);
    arr.push(r_val);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_divexact(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_divexact", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_divexact", "num2")?;

    if b.is_zero() {
        return Err(throw_div_by_zero(
            vm,
            "gmp_divexact",
            2,
            "num2",
            "Division by zero",
        ));
    }

    // mpz_divexact (only meaningful when b divides a).
    Ok(bigint_to_gmp_value(vm, a.div_trunc(&b)))
}

fn gmp_mod(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_mod", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_mod", "num2")?;

    if b.is_zero() {
        return Err(throw_div_by_zero(
            vm,
            "gmp_mod",
            2,
            "num2",
            "Modulo by zero",
        ));
    }

    let r = a.rem_euclid(&b);
    Ok(bigint_to_gmp_value(vm, r))
}

fn gmp_neg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_neg", "num")?;
    Ok(bigint_to_gmp_value(vm, -a))
}

fn gmp_abs(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_abs", "num")?;
    Ok(bigint_to_gmp_value(vm, a.abs()))
}

fn gmp_cmp(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_cmp", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_cmp", "num2")?;
    let result = match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    Ok(Value::Long(result))
}

fn gmp_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_sign", "num")?;
    Ok(Value::Long(a.signum() as i64))
}

/// Coerce a Value into an i64 for integer-type GMP arguments, throwing TypeError
/// if the value is not int/string/bool/float.
fn get_int_arg(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    func_name: &str,
    arg_name: &str,
) -> Result<i64, VmError> {
    let val = args.get(idx).cloned().unwrap_or(Value::Null);
    let v = match &val {
        Value::Reference(r) => r.borrow().clone(),
        _ => val.clone(),
    };
    match v {
        Value::Long(n) => Ok(n),
        Value::True => Ok(1),
        Value::False => Ok(0),
        Value::Null | Value::Undef => Ok(0),
        Value::Double(d) => Ok(d as i64),
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            // Accept simple integer string
            let trimmed = s_str.trim();
            match trimmed.parse::<i64>() {
                Ok(n) => Ok(n),
                Err(_) => {
                    let msg = format!(
                        "{}(): Argument #{} (${}) must be of type int, string given",
                        func_name,
                        idx + 1,
                        arg_name
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    })
                }
            }
        }
        Value::Array(_) => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type int, array given",
                func_name,
                idx + 1,
                arg_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        }
        Value::Object(obj) => {
            let cls = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type int, {} given",
                func_name,
                idx + 1,
                arg_name,
                cls
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        }
        _ => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type int",
                func_name,
                idx + 1,
                arg_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        }
    }
}

fn gmp_pow(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = get_gmp_arg(vm, args, 0, "gmp_pow", "num")?;
    let exp = get_int_arg(vm, args, 1, "gmp_pow", "exponent")?;
    if exp < 0 {
        let msg =
            "gmp_pow(): Argument #2 ($exponent) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, base.pow(exp as u64)))
}

fn gmp_powm(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = get_gmp_arg(vm, args, 0, "gmp_powm", "num")?;
    let exp = get_gmp_arg(vm, args, 1, "gmp_powm", "exponent")?;
    let modulus = get_gmp_arg(vm, args, 2, "gmp_powm", "modulus")?;

    if exp.is_negative() {
        let msg =
            "gmp_powm(): Argument #2 ($exponent) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    if modulus.is_zero() {
        let msg = "Modulo by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let result = base.modpow(&exp, &modulus);
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_sqrt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_sqrt", "num")?;
    if a.is_negative() {
        let msg = "gmp_sqrt(): Argument #1 ($num) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, a.isqrt()))
}

fn gmp_sqrtrem(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_sqrtrem", "num")?;
    if a.is_negative() {
        let msg =
            "gmp_sqrtrem(): Argument #1 ($num) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let (root, rem) = a.isqrt_rem();
    let root_val = bigint_to_gmp_value(vm, root);
    let rem_val = bigint_to_gmp_value(vm, rem);
    let mut arr = PhpArray::new();
    arr.push(root_val);
    arr.push(rem_val);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_root(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_root", "num")?;
    let nth = get_int_arg(vm, args, 1, "gmp_root", "nth")?;
    if nth <= 0 {
        let msg = "gmp_root(): Argument #2 ($nth) must be greater than 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    if a.is_negative() && nth % 2 == 0 {
        let msg = "gmp_root(): Argument #2 ($nth) must be odd if argument #1 ($a) is negative"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, a.root(nth as u64)))
}

fn gmp_rootrem(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_rootrem", "num")?;
    let nth = get_int_arg(vm, args, 1, "gmp_rootrem", "nth")?;
    if nth <= 0 {
        let msg =
            "gmp_rootrem(): Argument #2 ($nth) must be greater than or equal to 1".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    if a.is_negative() && nth % 2 == 0 {
        let msg = "gmp_rootrem(): Argument #2 ($nth) must be odd if argument #1 ($a) is negative"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let (root, rem) = a.root_rem(nth as u64);
    let root_val = bigint_to_gmp_value(vm, root);
    let rem_val = bigint_to_gmp_value(vm, rem);
    let mut arr = PhpArray::new();
    arr.push(root_val);
    arr.push(rem_val);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_perfect_power(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_perfect_power", "num")?;
    Ok(bool_value(a.is_perfect_power()))
}

fn gmp_gcd(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_gcd", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_gcd", "num2")?;
    Ok(bigint_to_gmp_value(vm, a.gcd(&b)))
}

fn gmp_gcdext(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_gcdext", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_gcdext", "num2")?;
    let (g, s, t) = a.extended_gcd(&b);
    let g_val = bigint_to_gmp_value(vm, g);
    let s_val = bigint_to_gmp_value(vm, s);
    let t_val = bigint_to_gmp_value(vm, t);
    let mut arr = PhpArray::new();
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"g")),
        g_val,
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"s")),
        s_val,
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"t")),
        t_val,
    );
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_lcm(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_lcm", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_lcm", "num2")?;

    if a.is_zero() || b.is_zero() {
        return Ok(bigint_to_gmp_value(vm, Int::ZERO));
    }

    Ok(bigint_to_gmp_value(vm, a.lcm(&b)))
}

fn gmp_and(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_and", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_and", "num2")?;
    Ok(bigint_to_gmp_value(vm, a.bitand(&b)))
}

fn gmp_or(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_or", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_or", "num2")?;
    Ok(bigint_to_gmp_value(vm, a.bitor(&b)))
}

fn gmp_xor(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_xor", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_xor", "num2")?;
    Ok(bigint_to_gmp_value(vm, a.bitxor(&b)))
}

fn gmp_com(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_com", "num")?;
    Ok(bigint_to_gmp_value(vm, -(a + Int::ONE)))
}

fn gmp_fact(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_fact", "num")?;
    if a.is_negative() {
        let msg = "gmp_fact(): Argument #1 ($num) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    // mpz_fac_ui(mpz_get_ui(n))
    Ok(bigint_to_gmp_value(vm, Int::factorial(mp::get_ui(&a))))
}

fn gmp_prob_prime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = get_gmp_arg(vm, args, 0, "gmp_prob_prime", "num")?;
    // The repetition count only adds random-base Miller-Rabin rounds after
    // Baillie-PSW (see mp::probab_prime), which cannot change the verdict.
    let _reps = args.get(1).map(|v| v.to_long()).unwrap_or(10);
    Ok(Value::Long(mp::probab_prime(&n)))
}

fn gmp_nextprime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = get_gmp_arg(vm, args, 0, "gmp_nextprime", "num")?;
    Ok(bigint_to_gmp_value(vm, n.next_prime()))
}

fn gmp_testbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_testbit", "num")?;
    let index = get_int_arg(vm, args, 1, "gmp_testbit", "index")?;
    let max_index: i64 = (i32::MAX as i64).saturating_mul(8);
    if index < 0 || index >= max_index {
        let msg = format!(
            "gmp_testbit(): Argument #2 ($index) must be between 0 and {} * {}",
            i32::MAX,
            8
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(if a.test_bit(index as u64) {
        Value::True
    } else {
        Value::False
    })
}

fn gmp_setbit_or_clrbit(
    vm: &mut Vm,
    args: &[Value],
    func_name: &str,
    bit_val: bool,
) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let index = args.get(1).map(|v| v.to_long()).unwrap_or(0);

    // Resolve target object id (must be GMP)
    let (obj_id, target_obj) = match val {
        Value::Object(obj) => {
            let class_ok = obj.borrow().class_name.eq_ignore_ascii_case(b"GMP");
            if !class_ok {
                let cls = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
                let msg = format!(
                    "{}(): Argument #1 ($num) must be of type GMP, {} given",
                    func_name, cls
                );
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            (obj.borrow().object_id, Some(obj.clone()))
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            match inner {
                Value::Object(obj) => {
                    let class_ok = obj.borrow().class_name.eq_ignore_ascii_case(b"GMP");
                    if !class_ok {
                        let cls = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
                        let msg = format!(
                            "{}(): Argument #1 ($num) must be of type GMP, {} given",
                            func_name, cls
                        );
                        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError {
                            message: msg,
                            line: vm.current_line,
                        });
                    }
                    (obj.borrow().object_id, Some(obj.clone()))
                }
                _ => {
                    let type_name = value_type_name_for_gmp(&inner);
                    let msg = format!(
                        "{}(): Argument #1 ($num) must be of type GMP, {} given",
                        func_name, type_name
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError {
                        message: msg,
                        line: vm.current_line,
                    });
                }
            }
        }
        other => {
            let type_name = value_type_name_for_gmp(other);
            let msg = format!(
                "{}(): Argument #1 ($num) must be of type GMP, {} given",
                func_name, type_name
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };

    // Range check: 0 <= index < INT_MAX * CHAR_BIT
    let max_index: i64 = (i32::MAX as i64).saturating_mul(8);
    if index < 0 || index >= max_index {
        let msg = format!(
            "{}(): Argument #2 ($index) must be between 0 and {} * {}",
            func_name,
            i32::MAX,
            8
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let _ = obj_id;
    if let Some(obj) = target_obj {
        let n = obj_num(&obj);
        attach_num(&obj, mp::set_bit(&n, index as u64, bit_val));
    }

    Ok(Value::Null)
}

fn gmp_setbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bit_val = args.get(2).map(|v| v.is_truthy()).unwrap_or(true);
    gmp_setbit_or_clrbit(vm, args, "gmp_setbit", bit_val)
}

fn gmp_clrbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    gmp_setbit_or_clrbit(vm, args, "gmp_clrbit", false)
}

fn gmp_popcount(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_popcount", "num")?;
    Ok(Value::Long(mp::popcount(&a)))
}

fn gmp_hamdist(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_hamdist", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_hamdist", "num2")?;
    Ok(Value::Long(mp::hamdist(&a, &b)))
}

fn gmp_scan0(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_scan0", "num1")?;
    let start = get_int_arg(vm, args, 1, "gmp_scan0", "start")?;
    let max_index: i64 = (i32::MAX as i64).saturating_mul(8);
    if start < 0 || start >= max_index {
        let msg = format!(
            "gmp_scan0(): Argument #2 ($start) must be between 0 and {} * {}",
            i32::MAX,
            8
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(Value::Long(mp::scan(&a, start as u64, false)))
}

fn gmp_scan1(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_scan1", "num1")?;
    let start = get_int_arg(vm, args, 1, "gmp_scan1", "start")?;
    let max_index: i64 = (i32::MAX as i64).saturating_mul(8);
    if start < 0 || start >= max_index {
        let msg = format!(
            "gmp_scan1(): Argument #2 ($start) must be between 0 and {} * {}",
            i32::MAX,
            8
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(Value::Long(mp::scan(&a, start as u64, true)))
}

fn gmp_perfect_square(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_perfect_square", "num")?;
    Ok(bool_value(a.is_perfect_square()))
}

fn gmp_invert(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_invert", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_invert", "num2")?;

    if b.is_zero() {
        // gmp_invert uses the bare "Division by zero" message (no fn prefix).
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    match a.modinv(&b) {
        Some(result) => Ok(bigint_to_gmp_value(vm, result)),
        None => Ok(Value::False),
    }
}

fn gmp_jacobi(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_jacobi", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_jacobi", "num2")?;
    Ok(Value::Long(a.kronecker(&b) as i64))
}

fn gmp_legendre(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_legendre", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_legendre", "num2")?;
    Ok(Value::Long(a.kronecker(&b) as i64))
}

fn gmp_kronecker(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_kronecker", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_kronecker", "num2")?;
    Ok(Value::Long(a.kronecker(&b) as i64))
}

fn gmp_binomial(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = get_gmp_arg(vm, args, 0, "gmp_binomial", "n")?;
    let k = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if k < 0 {
        let msg = "gmp_binomial(): Argument #2 ($k) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, Int::binomial_int(&n, k as u64)))
}

/// Validate gmp_import/gmp_export options. Returns (order, endian, error).
/// order: +1 MSW_FIRST, -1 LSW_FIRST
/// endian: +1 big, -1 little, 0 native
fn validate_import_export_options(
    func_name: &str,
    word_size: i64,
    options: i64,
) -> Result<(i32, i32), (String, &'static [u8])> {
    if word_size < 1 {
        return Err((
            format!(
                "{}(): Argument #2 ($word_size) must be greater than or equal to 1",
                func_name
            ),
            b"ValueError",
        ));
    }

    const GMP_MSW_FIRST: i64 = 1 << 0;
    const GMP_LSW_FIRST: i64 = 1 << 1;
    const GMP_LITTLE_ENDIAN: i64 = 1 << 2;
    const GMP_BIG_ENDIAN: i64 = 1 << 3;
    const GMP_NATIVE_ENDIAN: i64 = 1 << 4;

    let order = match options & (GMP_MSW_FIRST | GMP_LSW_FIRST) {
        GMP_LSW_FIRST => -1,
        GMP_MSW_FIRST => 1,
        0 => 1, // default MSW_FIRST
        _ => {
            return Err((
                format!(
                    "{}(): Argument #3 ($flags) cannot use multiple word order options",
                    func_name
                ),
                b"ValueError",
            ));
        }
    };

    let endian = match options & (GMP_LITTLE_ENDIAN | GMP_BIG_ENDIAN | GMP_NATIVE_ENDIAN) {
        GMP_LITTLE_ENDIAN => -1,
        GMP_BIG_ENDIAN => 1,
        GMP_NATIVE_ENDIAN => 0,
        0 => 0, // default native
        _ => {
            return Err((
                format!(
                    "{}(): Argument #3 ($flags) cannot use multiple endian options",
                    func_name
                ),
                b"ValueError",
            ));
        }
    };

    Ok((order, endian))
}

fn gmp_export_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_export", "num")?;
    let word_size = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    let options = args.get(2).map(|v| v.to_long()).unwrap_or(1 | 16); // GMP_MSW_FIRST | GMP_NATIVE_ENDIAN

    let (order, endian) = match validate_import_export_options("gmp_export", word_size, options) {
        Ok(v) => v,
        Err((msg, cls)) => {
            let exc = vm.create_exception(cls, &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let word_size_usize = word_size as usize;

    if a.is_zero() {
        // GMP returns empty string for 0.
        return Ok(Value::String(PhpString::from_vec(Vec::new())));
    }

    let bytes = a.to_bytes_be();

    // Guard against malicious/huge word_size that would overflow when padding.
    // PHP's gmp_export rejects this with a ValueError.
    if word_size_usize > 1_000_000_000 {
        let msg = "gmp_export(): Argument #2 ($word_size) is too large for argument #1 ($num)"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let padded_len = match bytes
        .len()
        .checked_add(word_size_usize.saturating_sub(1))
        .and_then(|n| n.checked_div(word_size_usize.max(1)))
        .and_then(|n| n.checked_mul(word_size_usize))
    {
        Some(n) if n <= 1_000_000_000 => n,
        _ => {
            let msg = "gmp_export(): Argument #2 ($word_size) is too large for argument #1 ($num)"
                .to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let word_size = word_size_usize;
    let mut padded = vec![0u8; padded_len];
    let offset = padded_len - bytes.len();
    padded[offset..].copy_from_slice(&bytes);

    // Split into words (each word's bytes start out in big-endian order).
    let num_words = padded_len / word_size;
    let mut words: Vec<Vec<u8>> = Vec::with_capacity(num_words);
    for i in 0..num_words {
        let start = i * word_size;
        let end = start + word_size;
        let mut word = padded[start..end].to_vec();
        match endian {
            1 => { /* big endian: leave as-is */ }
            -1 => word.reverse(), // little endian
            _ => {
                // native: assume little endian on x86_64
                if cfg!(target_endian = "little") {
                    word.reverse();
                }
            }
        }
        words.push(word);
    }

    if order == -1 {
        words.reverse();
    }

    let result: Vec<u8> = words.into_iter().flatten().collect();
    Ok(Value::String(PhpString::from_vec(result)))
}

fn gmp_import_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let word_size = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    let options = args.get(2).map(|v| v.to_long()).unwrap_or(1 | 16);

    let (order, endian) = match validate_import_export_options("gmp_import", word_size, options) {
        Ok(v) => v,
        Err((msg, cls)) => {
            let exc = vm.create_exception(cls, &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    };
    let word_size = word_size as usize;

    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Ok(bigint_to_gmp_value(vm, Int::ZERO));
    }

    if !bytes.len().is_multiple_of(word_size) {
        let msg =
            "gmp_import(): Argument #1 ($data) must be a multiple of argument #2 ($word_size)"
                .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let num_words = bytes.len() / word_size;
    let mut words: Vec<Vec<u8>> = Vec::with_capacity(num_words);
    for i in 0..num_words {
        let start = i * word_size;
        let end = start + word_size;
        let mut word = bytes[start..end].to_vec();
        match endian {
            1 => { /* big endian: leave as-is */ }
            -1 => word.reverse(),
            _ => {
                if cfg!(target_endian = "little") {
                    word.reverse();
                }
            }
        }
        words.push(word);
    }

    if order == -1 {
        words.reverse();
    }

    let combined: Vec<u8> = words.into_iter().flatten().collect();
    let n = Int::from_bytes_be(&combined);
    Ok(bigint_to_gmp_value(vm, n))
}

// The gmp_randinit_mt() state shared by the gmp_random_* functions;
// seeded from system entropy on first use (gmp_init_random()).
thread_local! {
    static RAND_STATE: RefCell<Option<mt::GmpMt>> = const { RefCell::new(None) };
}

fn with_rand_state<R>(f: impl FnOnce(&mut mt::GmpMt) -> R) -> R {
    RAND_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = s.get_or_insert_with(|| {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
            );
            mt::GmpMt::with_seed(&Int::from(h.finish()))
        });
        f(state)
    })
}

fn gmp_random_seed(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let seed = get_gmp_arg(vm, args, 0, "gmp_random_seed", "seed")?;
    let state = mt::GmpMt::with_seed(&seed);
    RAND_STATE.with(|s| *s.borrow_mut() = Some(state));
    Ok(Value::Null)
}

fn gmp_random_bits(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bits = get_int_arg(vm, args, 0, "gmp_random_bits", "bits")?;
    // php-src: INT_MAX on 64-bit platforms.
    let max_bits: i64 = i32::MAX as i64;
    if bits < 1 || bits > max_bits {
        let msg = format!(
            "gmp_random_bits(): Argument #1 ($bits) must be between 1 and {}",
            max_bits
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let n = with_rand_state(|st| st.urandomb(bits as u64));
    Ok(bigint_to_gmp_value(vm, n))
}

fn gmp_random_range(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let min = get_gmp_arg(vm, args, 0, "gmp_random_range", "min")?;
    let max = get_gmp_arg(vm, args, 1, "gmp_random_range", "max")?;

    if min >= max {
        let msg = "gmp_random_range(): Argument #1 ($min) must be less than argument #2 ($maximum)"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    let range = &max - &min + Int::ONE;
    let r = with_rand_state(|st| st.urandomm(&range));
    Ok(bigint_to_gmp_value(vm, &min + &r))
}

fn bool_value(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}
