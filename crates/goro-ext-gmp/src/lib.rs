use num_bigint::BigInt;
use num_bigint::Sign;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Thread-local storage for GMP values keyed by object_id
thread_local! {
    static GMP_VALUES: RefCell<HashMap<u64, BigInt>> = RefCell::new(HashMap::new());
}

/// Register all GMP extension functions and constants
pub fn register(vm: &mut Vm) {
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
    vm.register_function(b"gmp_div", gmp_div_q); // alias
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

/// Get the BigInt value from a GMP object
pub fn get_gmp_value(val: &Value) -> Option<BigInt> {
    match val {
        Value::Object(obj) => {
            let id = obj.borrow().object_id;
            GMP_VALUES.with(|m| m.borrow().get(&id).cloned())
        }
        Value::Reference(r) => get_gmp_value(&r.borrow()),
        _ => None,
    }
}

/// Perform a GMP arithmetic operation and return the result as a GMP Value.
/// `op` is one of: "+", "-", "*", "/", "%", "**", "|", "&", "^", "<<", ">>"
/// Returns None if the operation can't be performed (e.g., invalid types).
pub fn gmp_do_operation(vm: &mut Vm, op: &str, a: &Value, b: &Value) -> Option<Result<Value, String>> {
    let a_is_gmp = is_gmp_object(a);
    let b_is_gmp = is_gmp_object(b);

    if !a_is_gmp && !b_is_gmp {
        return None; // Not a GMP operation
    }

    // Convert both operands to BigInt
    let a_val = if a_is_gmp {
        get_gmp_value(a)
    } else {
        operand_to_bigint(a)
    };

    let b_val = if b_is_gmp {
        get_gmp_value(b)
    } else {
        operand_to_bigint(b)
    };

    let a_bi = match a_val {
        Some(v) => v,
        None => {
            let type_name = value_type_name_for_gmp(if !a_is_gmp { a } else { b });
            return Some(Err(format!(
                "Number must be of type GMP|string|int, {} given",
                type_name
            )));
        }
    };

    let b_bi = match b_val {
        Some(v) => v,
        None => {
            let type_name = value_type_name_for_gmp(if !b_is_gmp { b } else { a });
            return Some(Err(format!(
                "Number must be of type GMP|string|int, {} given",
                type_name
            )));
        }
    };

    let result = match op {
        "+" => Ok(&a_bi + &b_bi),
        "-" => Ok(&a_bi - &b_bi),
        "*" => Ok(&a_bi * &b_bi),
        "/" => {
            if b_bi.is_zero() {
                return Some(Err("Division by zero".to_string()));
            }
            // GMP division truncates toward zero
            Ok(a_bi.div_floor(&b_bi))
        }
        "%" => {
            if b_bi.is_zero() {
                return Some(Err("Modulo by zero".to_string()));
            }
            Ok(&a_bi % &b_bi)
        }
        "**" => {
            let exp = b_bi.to_u32().unwrap_or(0);
            Ok(a_bi.pow(exp))
        }
        "|" => Ok(&a_bi | &b_bi),
        "&" => Ok(&a_bi & &b_bi),
        "^" => Ok(&a_bi ^ &b_bi),
        "<<" => {
            let shift = b_bi.to_i64().unwrap_or(0);
            if shift < 0 {
                return Some(Err("Shift must be greater than or equal to 0".to_string()));
            }
            Ok(&a_bi << shift as u64)
        }
        ">>" => {
            let shift = b_bi.to_i64().unwrap_or(0);
            if shift < 0 {
                return Some(Err("Shift must be greater than or equal to 0".to_string()));
            }
            Ok(&a_bi >> shift as u64)
        }
        _ => return None,
    };

    match result {
        Ok(n) => Some(Ok(bigint_to_gmp_value(vm, n))),
        Err(e) => Some(Err(e)),
    }
}

/// Perform GMP unary operation: "~" (complement), "-" (negation), "+" (identity)
pub fn gmp_do_unary(vm: &mut Vm, op: &str, a: &Value) -> Option<Value> {
    if !is_gmp_object(a) {
        return None;
    }
    let n = get_gmp_value(a)?;
    match op {
        "~" => Some(bigint_to_gmp_value(vm, -(n + BigInt::one()))),
        "-" => Some(bigint_to_gmp_value(vm, -n)),
        "+" => Some(bigint_to_gmp_value(vm, n)),
        _ => None,
    }
}

/// Compare two values where at least one is GMP.
/// Returns Some(ordering) if comparison is possible, None otherwise.
pub fn gmp_compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    let a_gmp = is_gmp_object(a);
    let b_gmp = is_gmp_object(b);
    if !a_gmp && !b_gmp {
        return None;
    }
    let a_bi = if a_gmp { get_gmp_value(a) } else { operand_to_bigint(a) };
    let b_bi = if b_gmp { get_gmp_value(b) } else { operand_to_bigint(b) };
    match (a_bi, b_bi) {
        (Some(a), Some(b)) => Some(a.cmp(&b)),
        _ => None,
    }
}

/// Convert a GMP object to its string representation (for __toString / concatenation)
pub fn gmp_to_string(val: &Value) -> Option<String> {
    if !is_gmp_object(val) {
        return None;
    }
    get_gmp_value(val).map(|n| n.to_str_radix(10))
}

/// Convert a GMP object to i64 (for (int) cast)
pub fn gmp_to_long(val: &Value) -> Option<i64> {
    if !is_gmp_object(val) {
        return None;
    }
    get_gmp_value(val).map(|n| {
        n.to_i64().unwrap_or_else(|| {
            let (sign, bytes) = n.to_bytes_le();
            let mut buf = [0u8; 8];
            let len = bytes.len().min(8);
            buf[..len].copy_from_slice(&bytes[..len]);
            let v = i64::from_le_bytes(buf);
            if sign == Sign::Minus { -v } else { v }
        })
    })
}

/// Convert a GMP object to f64 (for (float) cast)
pub fn gmp_to_double(val: &Value) -> Option<f64> {
    if !is_gmp_object(val) {
        return None;
    }
    get_gmp_value(val).map(|n| n.to_f64().unwrap_or(0.0))
}

/// Convert a non-GMP operand to BigInt for operator overloading.
/// Only accepts int and int-strings. Rejects float, array, object, etc.
fn operand_to_bigint(val: &Value) -> Option<BigInt> {
    match val {
        Value::Long(n) => Some(BigInt::from(*n)),
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            let trimmed = s_str.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Must be a valid integer string
            parse_bigint_str(trimmed, 10).ok()
        }
        Value::Reference(r) => operand_to_bigint(&r.borrow()),
        _ => None,
    }
}

fn value_type_name_for_gmp(val: &Value) -> &'static str {
    match val {
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::True | Value::False => "bool",
        Value::Null | Value::Undef => "null",
        Value::Array(_) => "array",
        Value::Object(obj) => {
            let name = obj.borrow().class_name.clone();
            if name.eq_ignore_ascii_case(b"stdClass") {
                "stdClass"
            } else {
                // leak a static string - not great but matches PHP behavior
                "object"
            }
        }
        _ => "unknown",
    }
}

// ============================================================
// Helper functions
// ============================================================

/// Parse a string to BigInt, supporting optional sign and base prefixes (0x, 0b, 0o).
fn parse_bigint_str(s: &str, base: u32) -> Result<BigInt, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty string".to_string());
    }

    let bytes = s.as_bytes();
    let (negative, start) = match bytes[0] {
        b'-' => (true, 1),
        b'+' => (false, 1),
        _ => (false, 0),
    };

    if start >= bytes.len() {
        return Err("empty string after sign".to_string());
    }

    // Handle "0x", "0b", "0o" prefixes
    let (actual_base, digit_start) = if base == 0 || base == 16 || base == 2 || base == 8 {
        if bytes.len() > start + 1 && bytes[start] == b'0' {
            match bytes[start + 1] {
                b'x' | b'X' if base == 0 || base == 16 => (16u32, start + 2),
                b'b' | b'B' if base == 0 || base == 2 => (2u32, start + 2),
                b'o' | b'O' if base == 0 || base == 8 => (8u32, start + 2),
                _ if base == 0 => (10u32, start),
                _ => (base, start),
            }
        } else if base == 0 {
            (10u32, start)
        } else {
            (base, start)
        }
    } else {
        (base, start)
    };

    let digit_str = &s[digit_start..];
    if digit_str.is_empty() {
        return Err("no digits after prefix".to_string());
    }

    // Validate all characters are valid for the base
    for &ch in digit_str.as_bytes() {
        let valid = if actual_base <= 10 {
            ch >= b'0' && ch < b'0' + actual_base as u8
        } else if actual_base <= 36 {
            (ch >= b'0' && ch <= b'9')
                || (ch >= b'a' && ch < b'a' + (actual_base - 10) as u8)
                || (ch >= b'A' && ch < b'A' + (actual_base - 10) as u8)
        } else {
            // bases 37..=62: 0-9, a-z (10-35), A-Z (36-61)
            (ch >= b'0' && ch <= b'9')
                || (ch >= b'a' && ch <= b'z')
                || (ch >= b'A' && ch < b'A' + (actual_base - 36) as u8)
        };
        if !valid {
            return Err(format!("Invalid digit for base {}", actual_base));
        }
    }

    let n = if actual_base <= 36 {
        BigInt::parse_bytes(digit_str.as_bytes(), actual_base)
            .ok_or_else(|| format!("Invalid number string: {}", s))?
    } else {
        // Manual parsing for bases 37..=62
        let mut result = BigInt::zero();
        let base_bi = BigInt::from(actual_base);
        for &ch in digit_str.as_bytes() {
            let digit = match ch {
                b'0'..=b'9' => (ch - b'0') as u32,
                b'a'..=b'z' => (ch - b'a' + 10) as u32,
                b'A'..=b'Z' => (ch - b'A' + 36) as u32,
                _ => return Err(format!("Invalid digit for base {}", actual_base)),
            };
            result = result * &base_bi + BigInt::from(digit);
        }
        result
    };

    if negative {
        Ok(-n)
    } else {
        Ok(n)
    }
}

/// Convert a BigInt to a string in the given radix (2..=62).
fn bigint_to_str_radix(n: &BigInt, base: u32) -> String {
    if base <= 36 {
        n.to_str_radix(base)
    } else {
        if n.is_zero() {
            return "0".to_string();
        }
        let negative = n.sign() == Sign::Minus;
        let mut val = n.abs();
        let base_bi = BigInt::from(base);
        let mut digits = Vec::new();
        while !val.is_zero() {
            let (q, r) = val.div_rem(&base_bi);
            let d = r.to_u32().unwrap();
            let ch = match d {
                0..=9 => (b'0' + d as u8) as char,
                10..=35 => (b'a' + (d - 10) as u8) as char,
                36..=61 => (b'A' + (d - 36) as u8) as char,
                _ => unreachable!(),
            };
            digits.push(ch);
            val = q;
        }
        digits.reverse();
        let s: String = digits.into_iter().collect();
        if negative {
            format!("-{}", s)
        } else {
            s
        }
    }
}

/// Extract a BigInt from a Value for GMP function arguments.
/// Returns Ok(BigInt) on success, Err(error_kind) on failure.
/// error_kind: "type" for TypeError, "value" for ValueError
fn value_to_bigint_checked(val: &Value, func_name: &str, arg_num: u32, arg_name: &str) -> Result<BigInt, GmpArgError> {
    match val {
        Value::Object(obj) => {
            let b = obj.borrow();
            if b.class_name.eq_ignore_ascii_case(b"GMP") {
                let id = b.object_id;
                GMP_VALUES.with(|m| m.borrow().get(&id).cloned())
                    .ok_or(GmpArgError::Type(format!(
                        "{}(): Argument #{} (${}) must be of type GMP|string|int, {} given",
                        func_name, arg_num, arg_name, String::from_utf8_lossy(&b.class_name)
                    )))
            } else {
                Err(GmpArgError::Type(format!(
                    "{}(): Argument #{} (${}) must be of type GMP|string|int, {} given",
                    func_name, arg_num, arg_name, String::from_utf8_lossy(&b.class_name)
                )))
            }
        }
        Value::Long(n) => Ok(BigInt::from(*n)),
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            let trimmed = s_str.trim();
            if trimmed.is_empty() {
                return Err(GmpArgError::Value(format!(
                    "{}(): Argument #{} (${}) is not an integer string",
                    func_name, arg_num, arg_name
                )));
            }
            parse_bigint_str(trimmed, 10).map_err(|_| {
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
            VmError { message: msg, line: vm.current_line }
        }
        GmpArgError::Value(msg) => {
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            VmError { message: msg, line: vm.current_line }
        }
    }
}

/// Helper to get a BigInt arg or throw proper exception
fn get_gmp_arg(vm: &mut Vm, args: &[Value], idx: usize, func_name: &str, arg_name: &str) -> Result<BigInt, VmError> {
    let val = args.get(idx).unwrap_or(&Value::Null);
    value_to_bigint_checked(val, func_name, (idx + 1) as u32, arg_name)
        .map_err(|e| throw_gmp_error(vm, e))
}

/// Create a GMP object from a BigInt and return it as a Value
fn bigint_to_gmp_value(vm: &mut Vm, n: BigInt) -> Value {
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"GMP".to_vec(), obj_id);
    // Set the "num" property for var_dump display
    let num_str = n.to_str_radix(10);
    obj.set_property(b"num".to_vec(), Value::String(PhpString::from_string(num_str)));
    GMP_VALUES.with(|m| m.borrow_mut().insert(obj_id, n));
    Value::Object(Rc::new(RefCell::new(obj)))
}

// ============================================================
// Primality testing (Miller-Rabin)
// ============================================================

fn deterministic_witnesses(n: &BigInt) -> Vec<u64> {
    if *n < BigInt::from(2047u64) {
        vec![2]
    } else if *n < BigInt::from(1_373_653u64) {
        vec![2, 3]
    } else if *n < BigInt::from(9_080_191u64) {
        vec![31, 73]
    } else if *n < BigInt::from(25_326_001u64) {
        vec![2, 3, 5]
    } else if *n < BigInt::from(3_215_031_751u64) {
        vec![2, 3, 5, 7]
    } else if *n < BigInt::from(4_759_123_141u64) {
        vec![2, 7, 61]
    } else if *n < BigInt::from(1_122_004_669_633u64) {
        vec![2, 13, 23, 1662803]
    } else if *n < BigInt::from(2_152_302_898_747u64) {
        vec![2, 3, 5, 7, 11]
    } else if *n < BigInt::from(3_474_749_660_383u64) {
        vec![2, 3, 5, 7, 11, 13]
    } else if *n < BigInt::from(341_550_071_728_321u64) {
        vec![2, 3, 5, 7, 11, 13, 17]
    } else {
        vec![] // Use probabilistic test
    }
}

fn is_probably_prime(n: &BigInt, reps: u32) -> u32 {
    let n = n.abs();
    if n.is_zero() {
        return 0;
    }

    let one = BigInt::one();
    let two = BigInt::from(2);
    let three = BigInt::from(3);

    if n == one {
        return 0;
    }
    if n == two || n == three {
        return 2;
    }

    if (&n & &one).is_zero() {
        return 0;
    }

    let small_primes: &[u32] = &[
        3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83,
        89, 97, 101, 103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173, 179,
        181, 191, 193, 197, 199, 211, 223, 227, 229, 233, 239, 241, 251,
    ];

    for &p in small_primes {
        let pb = BigInt::from(p);
        if n == pb {
            return 2;
        }
        if (&n % &pb).is_zero() {
            return 0;
        }
    }

    if n < BigInt::from(63001u32) {
        return 2;
    }

    let n_minus_1 = &n - &one;
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while (&d & &one).is_zero() {
        d >>= 1;
        r += 1;
    }

    let witnesses = deterministic_witnesses(&n);

    let actual_reps = if witnesses.is_empty() {
        reps
    } else {
        witnesses.len() as u32
    };

    let (_, n_bytes) = n.to_bytes_le();
    let mut prng_state: u64 = 0;
    for (i, &b) in n_bytes.iter().enumerate() {
        prng_state ^= (b as u64).wrapping_mul(i as u64 + 1);
    }
    prng_state = prng_state.wrapping_add(0x9E3779B97F4A7C15);

    for i in 0..actual_reps {
        let a = if !witnesses.is_empty() {
            BigInt::from(witnesses[i as usize])
        } else {
            prng_state ^= prng_state << 13;
            prng_state ^= prng_state >> 7;
            prng_state ^= prng_state << 17;
            let n_i64 = n.to_i64().unwrap_or(i64::MAX);
            let range = (n_i64 as u64).saturating_sub(3) + 1;
            let val = if range > 0 { prng_state % range + 2 } else { 2 };
            BigInt::from(val)
        };

        if a >= n_minus_1 || a < two {
            continue;
        }

        let mut x = a.modpow(&d, &n);

        if x == one || x == n_minus_1 {
            continue;
        }

        let mut composite = true;
        for _ in 1..r {
            x = x.modpow(&two, &n);
            if x == n_minus_1 {
                composite = false;
                break;
            }
        }

        if composite {
            return 0;
        }
    }

    if !witnesses.is_empty() {
        2
    } else {
        1
    }
}

fn next_prime(n: &BigInt) -> BigInt {
    let mut candidate = n.abs();
    let one = BigInt::one();
    let two = BigInt::from(2);

    if candidate < two {
        return two;
    }

    if (&candidate & &one).is_zero() {
        candidate += &one;
    } else {
        candidate += &two;
    }

    loop {
        if is_probably_prime(&candidate, 25) > 0 {
            return candidate;
        }
        candidate += &two;
    }
}

/// Extended GCD: returns (gcd, x, y) such that a*x + b*y = gcd
fn extended_gcd(a: &BigInt, b: &BigInt) -> (BigInt, BigInt, BigInt) {
    if b.is_zero() {
        return (a.clone(), BigInt::one(), BigInt::zero());
    }
    let (q, r) = a.div_rem(b);
    let (g, x1, y1) = extended_gcd(b, &r);
    let x = y1.clone();
    let y = x1 - &q * &y1;
    (g, x, y)
}

/// Modular inverse: a^(-1) mod m
fn mod_inverse(a: &BigInt, modulus: &BigInt) -> Result<BigInt, String> {
    if modulus.is_zero() {
        return Err("Division by zero".to_string());
    }
    let (g, x, _) = extended_gcd(&a.abs(), &modulus.abs());
    if !g.is_one() {
        return Err("Inverse doesn't exist".to_string());
    }
    let result = if a.sign() == Sign::Minus { -x } else { x };
    let m = modulus.abs();
    let r = result.mod_floor(&m);
    Ok(r)
}

/// Integer square root (floor) using Newton's method
fn isqrt(n: &BigInt) -> BigInt {
    if n.is_zero() {
        return BigInt::zero();
    }
    let two = BigInt::from(2);
    let bits = n.bits();
    let mut x = BigInt::one() << (bits / 2 + 1);
    loop {
        let next = (&x + n / &x) / &two;
        if next >= x {
            return x;
        }
        x = next;
    }
}

/// Integer nth root (floor)
fn iroot(n: &BigInt, k: u32) -> BigInt {
    if n.is_zero() || k == 0 {
        return BigInt::zero();
    }
    if k == 1 {
        return n.clone();
    }
    if k == 2 {
        return isqrt(n);
    }
    let k_bi = BigInt::from(k);
    let k_minus_1 = BigInt::from(k - 1);

    // Initial guess
    let bits = n.bits();
    let mut x = BigInt::one() << ((bits / k as u64) + 1);

    loop {
        // Newton: x_new = ((k-1)*x + n/x^(k-1)) / k
        let xk1 = x.pow(k - 1);
        if xk1.is_zero() {
            break;
        }
        let next = (&k_minus_1 * &x + n / &xk1) / &k_bi;
        if next >= x {
            break;
        }
        x = next;
    }
    // Adjust down if needed
    while x.pow(k) > *n {
        x -= BigInt::one();
    }
    x
}

fn test_bit(n: &BigInt, index: u64) -> bool {
    n.bit(index)
}

fn set_bit(n: &BigInt, index: u64, value: bool) -> BigInt {
    let mask = BigInt::one() << index;
    if value {
        n | &mask
    } else {
        n & &(!mask)
    }
}

fn popcount(n: &BigInt) -> i64 {
    if n.sign() == Sign::Minus {
        return -1;
    }
    if n.is_zero() {
        return 0;
    }
    let (_, bytes) = n.to_bytes_le();
    let mut count: i64 = 0;
    for &b in &bytes {
        count += b.count_ones() as i64;
    }
    count
}

fn is_perfect_square(n: &BigInt) -> bool {
    if n.sign() == Sign::Minus {
        return false;
    }
    if n.is_zero() {
        return true;
    }
    let root = isqrt(n);
    &root * &root == *n
}

/// Jacobi symbol (a/n) for odd n > 0
fn jacobi_symbol(a: &BigInt, n: &BigInt) -> i32 {
    if n.is_one() {
        return 1;
    }
    if a.is_zero() {
        return 0;
    }

    let mut a = a.mod_floor(n);
    let mut n = n.clone();
    let mut result = 1i32;

    while !a.is_zero() {
        // Factor out 2s from a
        while (&a & BigInt::one()).is_zero() {
            a >>= 1;
            let n_mod_8 = (&n % BigInt::from(8)).to_i64().unwrap_or(0);
            if n_mod_8 == 3 || n_mod_8 == 5 {
                result = -result;
            }
        }

        // Apply quadratic reciprocity
        std::mem::swap(&mut a, &mut n);
        let a_mod_4 = (&a % BigInt::from(4)).to_i64().unwrap_or(0);
        let n_mod_4 = (&n % BigInt::from(4)).to_i64().unwrap_or(0);
        if a_mod_4 == 3 && n_mod_4 == 3 {
            result = -result;
        }
        a = a.mod_floor(&n);
    }

    if n.is_one() {
        result
    } else {
        0
    }
}

/// Kronecker symbol (a/n), extension of Jacobi symbol
fn kronecker_symbol(a: &BigInt, n: &BigInt) -> i32 {
    if n.is_zero() {
        if a.abs().is_one() { return 1; } else { return 0; }
    }
    if n.is_one() {
        return 1;
    }
    if *n == BigInt::from(-1) {
        if a.sign() == Sign::Minus { return -1; } else { return 1; }
    }

    // Handle negative n
    let (mut result, n) = if n.sign() == Sign::Minus {
        let r = if a.sign() == Sign::Minus { -1 } else { 1 };
        (r, n.abs())
    } else {
        (1, n.clone())
    };

    // Factor out powers of 2 from n
    let mut n = n;
    let mut v = 0u32;
    while (&n & BigInt::one()).is_zero() {
        n >>= 1;
        v += 1;
    }

    if v > 0 {
        // Handle (a/2) for each factor of 2
        let a_mod_8 = a.mod_floor(&BigInt::from(8)).to_i64().unwrap_or(0);
        if v % 2 != 0 {
            if a_mod_8 == 3 || a_mod_8 == 5 {
                result = -result;
            }
        }
    }

    if n.is_one() {
        return result;
    }

    // Now n is odd > 1, use Jacobi symbol
    result * jacobi_symbol(a, &n)
}

/// Scan for bit 0 starting from position start
fn scan0(n: &BigInt, start: u64) -> i64 {
    // Find the first 0 bit at or after position start
    for i in start..start + 4096 {
        if !n.bit(i) {
            return i as i64;
        }
    }
    -1
}

/// Scan for bit 1 starting from position start
fn scan1(n: &BigInt, start: u64) -> i64 {
    if n.sign() == Sign::Minus {
        // For negative numbers in two's complement, there are always 1 bits
        for i in start..start + 4096 {
            if n.bit(i) {
                return i as i64;
            }
        }
        return -1;
    }
    if n.is_zero() {
        return -1;
    }
    for i in start..start + 4096 {
        if n.bit(i) {
            return i as i64;
        }
    }
    -1
}

/// Hamming distance (number of different bits)
fn hamdist(a: &BigInt, b: &BigInt) -> i64 {
    let diff = a ^ b;
    popcount(&diff)
}

/// Binomial coefficient C(n, k)
fn binomial(n: &BigInt, k: i64) -> BigInt {
    if k < 0 {
        return BigInt::zero();
    }
    if k == 0 {
        return BigInt::one();
    }

    let n_neg = n.sign() == Sign::Minus;
    if n_neg {
        // C(n, k) = (-1)^k * C(k - n - 1, k) for negative n
        let adjusted = BigInt::from(k) - n - BigInt::one();
        let result = binomial_positive(&adjusted, k as u64);
        if k % 2 != 0 {
            -result
        } else {
            result
        }
    } else {
        binomial_positive(n, k as u64)
    }
}

fn binomial_positive(n: &BigInt, k: u64) -> BigInt {
    if n.to_u64().map_or(false, |nv| k > nv) {
        return BigInt::zero();
    }
    let mut result = BigInt::one();
    for i in 0..k {
        result = result * (n - BigInt::from(i));
        result = result / BigInt::from(i + 1);
    }
    result
}

// ============================================================
// Rounding helper
// ============================================================

fn apply_rounding(q: BigInt, r: &BigInt, b: &BigInt, round: i64) -> BigInt {
    if r.is_zero() {
        return q;
    }
    match round {
        0 => q,
        1 => {
            if (r.sign() == Sign::Plus && b.sign() == Sign::Plus)
                || (r.sign() == Sign::Minus && b.sign() == Sign::Minus)
            {
                q + BigInt::one()
            } else {
                q
            }
        }
        2 => {
            if (r.sign() == Sign::Plus && b.sign() == Sign::Minus)
                || (r.sign() == Sign::Minus && b.sign() == Sign::Plus)
            {
                q - BigInt::one()
            } else {
                q
            }
        }
        _ => q,
    }
}

// ============================================================
// PHP function implementations
// ============================================================

fn gmp_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base_val = args.get(1).map(|v| v.to_long()).unwrap_or(10);
    let base = base_val as u32;

    // Validate base
    if base_val != 0 && (base_val < 2 || base_val > 62) {
        let msg = "gmp_init(): Argument #2 ($base) must be 0 or between 2 and 62".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let n = match val {
        Value::Long(n) => {
            if base != 10 && base != 0 {
                // If base is specified and val is int, convert int's string repr in that base
                BigInt::from(*n)
            } else {
                BigInt::from(*n)
            }
        }
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            let trimmed = s_str.trim();
            if trimmed.is_empty() {
                let msg = "gmp_init(): Argument #1 ($num) is not an integer string".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            match parse_bigint_str(trimmed, base) {
                Ok(n) => n,
                Err(_) => {
                    let msg = "gmp_init(): Argument #1 ($num) is not an integer string".to_string();
                    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
        }
        Value::Object(obj) => {
            let b = obj.borrow();
            if b.class_name.eq_ignore_ascii_case(b"GMP") {
                let id = b.object_id;
                match GMP_VALUES.with(|m| m.borrow().get(&id).cloned()) {
                    Some(n) => n,
                    None => {
                        let msg = format!("gmp_init(): Argument #1 ($num) must be of type GMP|string|int, {} given", String::from_utf8_lossy(&b.class_name));
                        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: msg, line: vm.current_line });
                    }
                }
            } else {
                let msg = format!("gmp_init(): Argument #1 ($num) must be of type GMP|string|int, {} given", String::from_utf8_lossy(&b.class_name));
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError { message: msg, line: vm.current_line });
            }
        }
        Value::Double(_) => {
            let msg = "gmp_init(): Argument #1 ($num) must be of type GMP|string|int, float given".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        Value::True | Value::False => {
            let t = if matches!(val, Value::True) { "true" } else { "false" };
            let msg = format!("gmp_init(): Argument #1 ($num) must be of type GMP|string|int, {} given", t);
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        Value::Null | Value::Undef => {
            let msg = "gmp_init(): Argument #1 ($num) must be of type GMP|string|int, null given".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        Value::Array(_) => {
            let msg = "gmp_init(): Argument #1 ($num) must be of type GMP|string|int, array given".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            return gmp_init(vm, &[inner, args.get(1).cloned().unwrap_or(Value::Long(10))]);
        }
        _ => {
            let msg = "gmp_init(): Argument #1 ($num) must be of type GMP|string|int, unknown given".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };

    Ok(bigint_to_gmp_value(vm, n))
}

fn gmp_strval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_strval", "num")?;
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10);

    if base < -36 || (base > -2 && base < 2) || base > 62 {
        let msg = "gmp_strval(): Argument #2 ($base) must be between 2 and 62, or -2 and -36".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let s = if base < 0 {
        // Negative base: uppercase output
        bigint_to_str_radix(&a, (-base) as u32).to_uppercase()
    } else {
        bigint_to_str_radix(&a, base as u32)
    };
    Ok(Value::String(PhpString::from_string(s)))
}

fn gmp_intval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_intval", "num")?;
    let result = a.to_i64().unwrap_or_else(|| {
        let (sign, bytes) = a.to_bytes_le();
        let mut buf = [0u8; 8];
        let len = bytes.len().min(8);
        buf[..len].copy_from_slice(&bytes[..len]);
        let v = i64::from_le_bytes(buf);
        if sign == Sign::Minus { -v } else { v }
    });
    Ok(Value::Long(result))
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

fn gmp_div_q(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_div_q", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_div_q", "num2")?;
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if b.is_zero() {
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let (q, r) = a.div_rem(&b);
    let result = apply_rounding(q, &r, &b, round);
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_div_r(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_div_r", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_div_r", "num2")?;
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if b.is_zero() {
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let (q, r) = a.div_rem(&b);
    let adjusted_q = apply_rounding(q, &r, &b, round);
    let result = &a - &(&adjusted_q * &b);
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_div_qr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_div_qr", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_div_qr", "num2")?;
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if b.is_zero() {
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let (q, r) = a.div_rem(&b);
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
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    Ok(bigint_to_gmp_value(vm, &a / &b))
}

fn gmp_mod(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_mod", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_mod", "num2")?;

    if b.is_zero() {
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let r = a.mod_floor(&b.abs());
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
    let result = match a.sign() {
        Sign::Plus => {
            if a.is_zero() { 0 } else { 1 }
        }
        Sign::NoSign => 0,
        Sign::Minus => -1,
    };
    Ok(Value::Long(result))
}

fn gmp_pow(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = get_gmp_arg(vm, args, 0, "gmp_pow", "num")?;
    let exp = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if exp < 0 {
        let msg = "gmp_pow(): Argument #2 ($exponent) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(bigint_to_gmp_value(vm, base.pow(exp as u32)))
}

fn gmp_powm(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = get_gmp_arg(vm, args, 0, "gmp_powm", "num")?;
    let exp = get_gmp_arg(vm, args, 1, "gmp_powm", "exponent")?;
    let modulus = get_gmp_arg(vm, args, 2, "gmp_powm", "modulus")?;

    if modulus.is_zero() {
        let msg = "Division by zero".to_string();
        let exc = vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    if exp.sign() == Sign::Minus {
        match mod_inverse(&base, &modulus) {
            Ok(inv) => {
                let pos_exp = exp.abs();
                let result = inv.modpow(&pos_exp, &modulus.abs());
                Ok(bigint_to_gmp_value(vm, result))
            }
            Err(_) => {
                let msg = "gmp_powm(): Argument #2 ($exponent) must be greater than or equal to 0 when modular inverse doesn't exist".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                Err(VmError { message: msg, line: vm.current_line })
            }
        }
    } else {
        let result = base.modpow(&exp, &modulus.abs());
        Ok(bigint_to_gmp_value(vm, result))
    }
}

fn gmp_sqrt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_sqrt", "num")?;
    if a.sign() == Sign::Minus {
        let msg = "gmp_sqrt(): Number has to be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(bigint_to_gmp_value(vm, isqrt(&a)))
}

fn gmp_sqrtrem(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_sqrtrem", "num")?;
    if a.sign() == Sign::Minus {
        let msg = "gmp_sqrtrem(): Number has to be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let root = isqrt(&a);
    let rem = &a - &root * &root;
    let root_val = bigint_to_gmp_value(vm, root);
    let rem_val = bigint_to_gmp_value(vm, rem);
    let mut arr = PhpArray::new();
    arr.push(root_val);
    arr.push(rem_val);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_root(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_root", "num")?;
    let nth = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if nth <= 0 {
        let msg = "gmp_root(): Argument #2 ($nth) must be positive".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if a.sign() == Sign::Minus && nth % 2 == 0 {
        let msg = "gmp_root(): Can't take even root of negative number".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let neg = a.sign() == Sign::Minus;
    let root = iroot(&a.abs(), nth as u32);
    let result = if neg { -root } else { root };
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_rootrem(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_rootrem", "num")?;
    let nth = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if nth <= 0 {
        let msg = "gmp_rootrem(): Argument #2 ($nth) must be positive".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let neg = a.sign() == Sign::Minus;
    let root = iroot(&a.abs(), nth as u32);
    let root_signed = if neg { -root.clone() } else { root.clone() };
    let rem = &a - root.pow(nth as u32) * (if neg { BigInt::from(-1) } else { BigInt::one() });
    let root_val = bigint_to_gmp_value(vm, root_signed);
    let rem_val = bigint_to_gmp_value(vm, rem);
    let mut arr = PhpArray::new();
    arr.push(root_val);
    arr.push(rem_val);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_perfect_power(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_perfect_power", "num")?;
    let a_abs = a.abs();
    if a_abs <= BigInt::one() {
        return Ok(Value::True);
    }
    // Check if n is a perfect power (n = m^k for some k >= 2)
    for k in 2..=64u32 {
        let root = iroot(&a_abs, k);
        if root <= BigInt::one() {
            break;
        }
        if root.pow(k) == a_abs {
            return Ok(Value::True);
        }
    }
    Ok(Value::False)
}

fn gmp_gcd(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_gcd", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_gcd", "num2")?;
    Ok(bigint_to_gmp_value(vm, a.gcd(&b)))
}

fn gmp_gcdext(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_gcdext", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_gcdext", "num2")?;
    let (g, s, t) = extended_gcd(&a, &b);
    let g_val = bigint_to_gmp_value(vm, g);
    let s_val = bigint_to_gmp_value(vm, s);
    let t_val = bigint_to_gmp_value(vm, t);
    let mut arr = PhpArray::new();
    arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"g")), g_val);
    arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"s")), s_val);
    arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"t")), t_val);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn gmp_lcm(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_lcm", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_lcm", "num2")?;

    if a.is_zero() || b.is_zero() {
        return Ok(bigint_to_gmp_value(vm, BigInt::zero()));
    }

    Ok(bigint_to_gmp_value(vm, a.lcm(&b)))
}

fn gmp_and(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_and", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_and", "num2")?;
    Ok(bigint_to_gmp_value(vm, &a & &b))
}

fn gmp_or(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_or", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_or", "num2")?;
    Ok(bigint_to_gmp_value(vm, &a | &b))
}

fn gmp_xor(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_xor", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_xor", "num2")?;
    Ok(bigint_to_gmp_value(vm, &a ^ &b))
}

fn gmp_com(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_com", "num")?;
    Ok(bigint_to_gmp_value(vm, -(a + BigInt::one())))
}

fn gmp_fact(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_fact", "num")?;
    let n = a.to_i64().unwrap_or(-1);
    if n < 0 {
        let msg = "gmp_fact(): Argument #1 ($num) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let result = (1..=n as u32).fold(BigInt::one(), |acc, i| acc * BigInt::from(i));
    Ok(bigint_to_gmp_value(vm, result))
}

fn gmp_prob_prime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = get_gmp_arg(vm, args, 0, "gmp_prob_prime", "num")?;
    let reps = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;
    Ok(Value::Long(is_probably_prime(&n, reps) as i64))
}

fn gmp_nextprime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = get_gmp_arg(vm, args, 0, "gmp_nextprime", "num")?;
    Ok(bigint_to_gmp_value(vm, next_prime(&n)))
}

fn gmp_testbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_testbit", "num")?;
    let index = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if index < 0 {
        let msg = "gmp_testbit(): Argument #2 ($index) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(if test_bit(&a, index as u64) {
        Value::True
    } else {
        Value::False
    })
}

fn gmp_setbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let index = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bit_val = args.get(2).map(|v| v.is_truthy()).unwrap_or(true);

    if index < 0 {
        let msg = "gmp_setbit(): Argument #2 ($index) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let obj_id = match val {
        Value::Object(obj) => obj.borrow().object_id,
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::Object(obj) => obj.borrow().object_id,
                _ => {
                    let msg = "gmp_setbit(): Argument #1 ($num) must be of type GMP".to_string();
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
        }
        _ => {
            let msg = "gmp_setbit(): Argument #1 ($num) must be of type GMP".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };

    GMP_VALUES.with(|m| {
        let mut map = m.borrow_mut();
        if let Some(n) = map.get(&obj_id) {
            let new_val = set_bit(n, index as u64, bit_val);
            // Update the num property on the object
            let num_str = new_val.to_str_radix(10);
            map.insert(obj_id, new_val);
            // Try to update num property
            if let Value::Object(obj) = val {
                obj.borrow_mut().set_property(b"num".to_vec(), Value::String(PhpString::from_string(num_str)));
            }
        }
    });

    Ok(Value::Null)
}

fn gmp_clrbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let index = args.get(1).map(|v| v.to_long()).unwrap_or(0);

    if index < 0 {
        let msg = "gmp_clrbit(): Argument #2 ($index) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let obj_id = match val {
        Value::Object(obj) => obj.borrow().object_id,
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::Object(obj) => obj.borrow().object_id,
                _ => {
                    let msg = "gmp_clrbit(): Argument #1 ($num) must be of type GMP".to_string();
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: msg, line: vm.current_line });
                }
            }
        }
        _ => {
            let msg = "gmp_clrbit(): Argument #1 ($num) must be of type GMP".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    };

    GMP_VALUES.with(|m| {
        let mut map = m.borrow_mut();
        if let Some(n) = map.get(&obj_id) {
            let new_val = set_bit(n, index as u64, false);
            let num_str = new_val.to_str_radix(10);
            map.insert(obj_id, new_val);
            if let Value::Object(obj) = val {
                obj.borrow_mut().set_property(b"num".to_vec(), Value::String(PhpString::from_string(num_str)));
            }
        }
    });

    Ok(Value::Null)
}

fn gmp_popcount(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_popcount", "num")?;
    Ok(Value::Long(popcount(&a)))
}

fn gmp_hamdist(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_hamdist", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_hamdist", "num2")?;
    Ok(Value::Long(hamdist(&a, &b)))
}

fn gmp_scan0(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_scan0", "num")?;
    let start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if start < 0 {
        let msg = "gmp_scan0(): Argument #2 ($start) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(Value::Long(scan0(&a, start as u64)))
}

fn gmp_scan1(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_scan1", "num")?;
    let start = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if start < 0 {
        let msg = "gmp_scan1(): Argument #2 ($start) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(Value::Long(scan1(&a, start as u64)))
}

fn gmp_perfect_square(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_perfect_square", "num")?;
    Ok(if is_perfect_square(&a) {
        Value::True
    } else {
        Value::False
    })
}

fn gmp_invert(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_invert", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_invert", "num2")?;

    match mod_inverse(&a, &b) {
        Ok(result) => Ok(bigint_to_gmp_value(vm, result)),
        Err(_) => Ok(Value::False),
    }
}

fn gmp_jacobi(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_jacobi", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_jacobi", "num2")?;
    Ok(Value::Long(jacobi_symbol(&a, &b) as i64))
}

fn gmp_legendre(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_legendre", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_legendre", "num2")?;
    Ok(Value::Long(jacobi_symbol(&a, &b) as i64))
}

fn gmp_kronecker(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_kronecker", "num1")?;
    let b = get_gmp_arg(vm, args, 1, "gmp_kronecker", "num2")?;
    Ok(Value::Long(kronecker_symbol(&a, &b) as i64))
}

fn gmp_binomial(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = get_gmp_arg(vm, args, 0, "gmp_binomial", "n")?;
    let k = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if k < 0 {
        let msg = "gmp_binomial(): Argument #2 ($k) must be greater than or equal to 0".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(bigint_to_gmp_value(vm, binomial(&n, k)))
}

fn gmp_export_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = get_gmp_arg(vm, args, 0, "gmp_export", "num")?;
    let word_size = args.get(1).map(|v| v.to_long()).unwrap_or(1) as usize;
    let options = args.get(2).map(|v| v.to_long()).unwrap_or(1 | 8); // GMP_MSW_FIRST | GMP_BIG_ENDIAN

    if word_size == 0 {
        let msg = "gmp_export(): Argument #2 ($word_size) must be greater than or equal to 1".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    if a.is_zero() {
        return Ok(Value::String(PhpString::from_vec(vec![0u8; word_size])));
    }

    let (_, bytes) = a.abs().to_bytes_be();

    let msw_first = (options & 1) != 0; // GMP_MSW_FIRST
    let big_endian = (options & 8) != 0; // GMP_BIG_ENDIAN
    let little_endian = (options & 4) != 0; // GMP_LITTLE_ENDIAN

    // Pad bytes to multiple of word_size
    let padded_len = ((bytes.len() + word_size - 1) / word_size) * word_size;
    let mut padded = vec![0u8; padded_len];
    let offset = padded_len - bytes.len();
    padded[offset..].copy_from_slice(&bytes);

    // Process words
    let num_words = padded_len / word_size;
    let mut words: Vec<Vec<u8>> = Vec::with_capacity(num_words);
    for i in 0..num_words {
        let start = i * word_size;
        let end = start + word_size;
        let mut word = padded[start..end].to_vec();
        // Handle endianness within each word
        if little_endian {
            word.reverse();
        } else if !big_endian {
            // native endian - assume little endian on x86
            word.reverse();
        }
        words.push(word);
    }

    // Handle word order
    if !msw_first {
        words.reverse();
    }

    let result: Vec<u8> = words.into_iter().flatten().collect();
    Ok(Value::String(PhpString::from_vec(result)))
}

fn gmp_import_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let word_size = args.get(1).map(|v| v.to_long()).unwrap_or(1) as usize;
    let options = args.get(2).map(|v| v.to_long()).unwrap_or(1 | 8);

    if word_size == 0 {
        let msg = "gmp_import(): Argument #2 ($word_size) must be greater than or equal to 1".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Ok(bigint_to_gmp_value(vm, BigInt::zero()));
    }

    if bytes.len() % word_size != 0 {
        let msg = "gmp_import(): Argument #1 ($data) must be a multiple of word_size".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let msw_first = (options & 1) != 0;
    let big_endian = (options & 8) != 0;
    let little_endian = (options & 4) != 0;

    let num_words = bytes.len() / word_size;
    let mut words: Vec<Vec<u8>> = Vec::with_capacity(num_words);
    for i in 0..num_words {
        let start = i * word_size;
        let end = start + word_size;
        let mut word = bytes[start..end].to_vec();
        if little_endian {
            word.reverse();
        } else if !big_endian {
            word.reverse();
        }
        words.push(word);
    }

    if !msw_first {
        words.reverse();
    }

    let combined: Vec<u8> = words.into_iter().flatten().collect();
    let n = BigInt::from_bytes_be(Sign::Plus, &combined);
    Ok(bigint_to_gmp_value(vm, n))
}

// Simple PRNG for random functions
thread_local! {
    static RNG_STATE: RefCell<u64> = RefCell::new(0);
}

fn gmp_random_seed(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let seed = args.first().map(|v| v.to_long()).unwrap_or(0) as u64;
    RNG_STATE.with(|s| *s.borrow_mut() = seed);
    Ok(Value::Null)
}

fn next_random() -> u64 {
    RNG_STATE.with(|s| {
        let mut state = s.borrow_mut();
        // xorshift64
        *state ^= (*state) << 13;
        *state ^= (*state) >> 7;
        *state ^= (*state) << 17;
        if *state == 0 {
            *state = 1; // avoid stuck at 0
        }
        *state
    })
}

fn gmp_random_bits(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bits = args.first().map(|v| v.to_long()).unwrap_or(0);
    if bits < 1 {
        let msg = "gmp_random_bits(): Argument #1 ($bits) must be greater than or equal to 1".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    // Generate random bytes
    let num_bytes = ((bits as usize) + 7) / 8;
    let mut bytes = Vec::with_capacity(num_bytes);
    for _ in 0..num_bytes {
        bytes.push((next_random() & 0xFF) as u8);
    }

    let mut n = BigInt::from_bytes_be(Sign::Plus, &bytes);
    // Mask off excess bits
    let excess = num_bytes * 8 - bits as usize;
    if excess > 0 {
        let mask = (BigInt::one() << bits as u64) - BigInt::one();
        n = n & mask;
    }

    Ok(bigint_to_gmp_value(vm, n))
}

fn gmp_random_range(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let min = get_gmp_arg(vm, args, 0, "gmp_random_range", "min")?;
    let max = get_gmp_arg(vm, args, 1, "gmp_random_range", "max")?;

    if min > max {
        let msg = "gmp_random_range(): Argument #1 ($min) must be less than or equal to argument #2 ($max)".to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    let range = &max - &min + BigInt::one();
    if range.is_one() {
        return Ok(bigint_to_gmp_value(vm, min));
    }

    let bits = range.bits();
    let num_bytes = ((bits as usize) + 7) / 8;
    let mut bytes = Vec::with_capacity(num_bytes);
    for _ in 0..num_bytes {
        bytes.push((next_random() & 0xFF) as u8);
    }
    let r = BigInt::from_bytes_be(Sign::Plus, &bytes);
    let r = r.mod_floor(&range);
    let result = &min + &r;
    Ok(bigint_to_gmp_value(vm, result))
}
