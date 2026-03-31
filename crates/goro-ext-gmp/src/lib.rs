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
    vm.register_function(b"gmp_mod", gmp_mod);
    vm.register_function(b"gmp_neg", gmp_neg);
    vm.register_function(b"gmp_abs", gmp_abs);
    vm.register_function(b"gmp_cmp", gmp_cmp);
    vm.register_function(b"gmp_sign", gmp_sign);
    vm.register_function(b"gmp_pow", gmp_pow);
    vm.register_function(b"gmp_powm", gmp_powm);
    vm.register_function(b"gmp_sqrt", gmp_sqrt);
    vm.register_function(b"gmp_gcd", gmp_gcd);
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
    vm.register_function(b"gmp_popcount", gmp_popcount);
    vm.register_function(b"gmp_perfect_square", gmp_perfect_square);
    vm.register_function(b"gmp_invert", gmp_invert);

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

// === Helper functions ===

/// Parse a string to BigInt, supporting optional sign and base prefixes (0x, 0b, 0o).
/// For bases <= 36, digits are case-insensitive (handled by num-bigint).
/// For bases 37..=62, lowercase a-z = 10-35, uppercase A-Z = 36-61.
fn parse_bigint_str(s: &str, base: u32) -> Result<BigInt, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(BigInt::zero());
    }

    let bytes = s.as_bytes();
    let (negative, start) = match bytes[0] {
        b'-' => (true, 1),
        b'+' => (false, 1),
        _ => (false, 0),
    };

    if start >= bytes.len() {
        return Ok(BigInt::zero());
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
        return Ok(BigInt::zero());
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
                _ => {
                    return Err(format!(
                        "Invalid digit for base {}: {}",
                        actual_base, ch as char
                    ))
                }
            };
            if digit >= actual_base {
                return Err(format!(
                    "Digit {} out of range for base {}",
                    digit, actual_base
                ));
            }
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
/// For bases <= 36, uses num-bigint's built-in to_str_radix.
/// For bases 37..=62, uses manual conversion with lowercase a-z = 10-35, uppercase A-Z = 36-61.
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

/// Extract a BigInt from a Value. Accepts GMP objects, integers, and strings.
fn value_to_bigint(val: &Value) -> Option<BigInt> {
    match val {
        Value::Object(obj) => {
            let id = obj.borrow().object_id;
            GMP_VALUES.with(|m| m.borrow().get(&id).cloned())
        }
        Value::Long(n) => Some(BigInt::from(*n)),
        Value::String(s) => {
            let s = s.to_string_lossy();
            let s = s.trim();
            if s.is_empty() {
                return Some(BigInt::zero());
            }
            parse_bigint_str(s, 10).ok()
        }
        Value::Double(f) => Some(BigInt::from(*f as i64)),
        Value::True => Some(BigInt::one()),
        Value::False | Value::Null | Value::Undef => Some(BigInt::zero()),
        Value::Reference(r) => value_to_bigint(&r.borrow()),
        _ => None,
    }
}

/// Create a GMP object from a BigInt and return it as a Value
fn bigint_to_gmp_value(vm: &mut Vm, n: BigInt) -> Value {
    let obj_id = vm.next_object_id();
    let obj = PhpObject::new(b"GMP".to_vec(), obj_id);
    GMP_VALUES.with(|m| m.borrow_mut().insert(obj_id, n));
    Value::Object(Rc::new(RefCell::new(obj)))
}

// === Primality testing (Miller-Rabin) ===

/// Deterministic witnesses for small numbers
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

/// Miller-Rabin primality test.
/// Returns 0 = not prime, 1 = probably prime, 2 = definitely prime (for small values).
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

    // Check if even
    if (&n & &one).is_zero() {
        return 0;
    }

    // Check small primes for divisibility
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

    // If n < 63001 (= 251^2) and we tested all primes up to 251, it's definitely prime.
    if n < BigInt::from(63001u32) {
        return 2;
    }

    // Write n-1 = 2^r * d
    let n_minus_1 = &n - &one;
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while (&d & &one).is_zero() {
        d >>= 1;
        r += 1;
    }

    // Deterministic witnesses for small numbers
    let witnesses = deterministic_witnesses(&n);

    let actual_reps = if witnesses.is_empty() {
        reps
    } else {
        witnesses.len() as u32
    };

    // Use a simple PRNG seeded from the number for non-deterministic witnesses
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
            // Generate pseudo-random witness in range [2, n-2]
            prng_state ^= prng_state << 13;
            prng_state ^= prng_state >> 7;
            prng_state ^= prng_state << 17;
            let n_i64 = n.to_i64().unwrap_or(i64::MAX);
            let range = (n_i64 as u64).saturating_sub(3) + 1;
            let val = if range > 0 { prng_state % range + 2 } else { 2 };
            BigInt::from(val)
        };

        // Make sure a is in range [2, n-2]
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

/// Find the next prime after n
fn next_prime(n: &BigInt) -> BigInt {
    let mut candidate = n.abs();
    let one = BigInt::one();
    let two = BigInt::from(2);

    if candidate < two {
        return two;
    }

    // Make candidate odd and greater than n
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
    // Adjust sign
    let result = if a.sign() == Sign::Minus { -x } else { x };
    // Make positive
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
    // Initial guess: 2^(bit_length/2 + 1)
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

/// Test if a specific bit is set (handles negative numbers in two's complement)
fn test_bit(n: &BigInt, index: u64) -> bool {
    n.bit(index)
}

/// Set or clear a specific bit (handles negative numbers in two's complement)
fn set_bit(n: &BigInt, index: u64, value: bool) -> BigInt {
    let mask = BigInt::one() << index;
    if value {
        n | &mask
    } else {
        n & &(!mask)
    }
}

/// Population count for non-negative BigInts.
/// Returns -1 for negative numbers (PHP convention).
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

/// Check if n is a perfect square
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

// === PHP function implementations ===

/// gmp_init(value, base=10) -> GMP
fn gmp_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;

    let n = match val {
        Value::Long(n) => BigInt::from(*n),
        Value::String(s) => {
            let s = s.to_string_lossy();
            let s = s.trim();
            match parse_bigint_str(s, base) {
                Ok(n) => n,
                Err(_) => {
                    return Err(VmError {
                        message: "gmp_init(): Unable to convert variable to GMP - string is not an integer".to_string(),
                        line: vm.current_line,
                    });
                }
            }
        }
        Value::Double(f) => BigInt::from(*f as i64),
        Value::True => BigInt::one(),
        Value::False | Value::Null | Value::Undef => BigInt::zero(),
        Value::Object(obj) => {
            let id = obj.borrow().object_id;
            match GMP_VALUES.with(|m| m.borrow().get(&id).cloned()) {
                Some(n) => n,
                None => {
                    return Err(VmError {
                        message: "gmp_init(): Unable to convert variable to GMP".to_string(),
                        line: vm.current_line,
                    });
                }
            }
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            return gmp_init(vm, &[inner]);
        }
        _ => {
            return Err(VmError {
                message: "gmp_init(): Unable to convert variable to GMP".to_string(),
                line: vm.current_line,
            });
        }
    };

    Ok(bigint_to_gmp_value(vm, n))
}

/// gmp_strval(gmp, base=10) -> string
fn gmp_strval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;

    let n = match value_to_bigint(val) {
        Some(n) => n,
        None => {
            return Err(VmError {
                message: "gmp_strval(): Unable to convert variable to GMP - string is not an integer".to_string(),
                line: vm.current_line,
            });
        }
    };

    let base = if base < 2 || base > 62 { 10 } else { base };
    let s = bigint_to_str_radix(&n, base);
    Ok(Value::String(PhpString::from_string(s)))
}

/// gmp_intval(gmp) -> int
fn gmp_intval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let n = match value_to_bigint(val) {
        Some(n) => n,
        None => {
            return Err(VmError {
                message: "gmp_intval(): Unable to convert variable to GMP".to_string(),
                line: vm.current_line,
            });
        }
    };
    // Truncate to i64 (wrapping behavior for large values)
    let result = n.to_i64().unwrap_or_else(|| {
        let (sign, bytes) = n.to_bytes_le();
        let mut buf = [0u8; 8];
        let len = bytes.len().min(8);
        buf[..len].copy_from_slice(&bytes[..len]);
        let v = i64::from_le_bytes(buf);
        if sign == Sign::Minus {
            -v
        } else {
            v
        }
    });
    Ok(Value::Long(result))
}

/// gmp_add(a, b) -> GMP
fn gmp_add(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_add")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_add")),
    };
    Ok(bigint_to_gmp_value(vm, &a + &b))
}

/// gmp_sub(a, b) -> GMP
fn gmp_sub(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_sub")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_sub")),
    };
    Ok(bigint_to_gmp_value(vm, &a - &b))
}

/// gmp_mul(a, b) -> GMP
fn gmp_mul(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_mul")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_mul")),
    };
    Ok(bigint_to_gmp_value(vm, &a * &b))
}

/// gmp_div_q(a, b, round=GMP_ROUND_ZERO) -> GMP
fn gmp_div_q(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_div_q")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_div_q")),
    };
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if b.is_zero() {
        return Err(VmError {
            message: "gmp_div_q(): Zero operand not allowed".to_string(),
            line: vm.current_line,
        });
    }

    let (q, r) = a.div_rem(&b);
    let result = apply_rounding(q, &r, &b, round);
    Ok(bigint_to_gmp_value(vm, result))
}

/// gmp_div_r(a, b, round=GMP_ROUND_ZERO) -> GMP
fn gmp_div_r(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_div_r")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_div_r")),
    };
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if b.is_zero() {
        return Err(VmError {
            message: "gmp_div_r(): Zero operand not allowed".to_string(),
            line: vm.current_line,
        });
    }

    let (q, r) = a.div_rem(&b);
    let adjusted_q = apply_rounding(q, &r, &b, round);
    // remainder = a - q*b
    let result = &a - &(&adjusted_q * &b);
    Ok(bigint_to_gmp_value(vm, result))
}

/// gmp_div_qr(a, b, round=GMP_ROUND_ZERO) -> [GMP, GMP]
fn gmp_div_qr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_div_qr")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_div_qr")),
    };
    let round = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if b.is_zero() {
        return Err(VmError {
            message: "gmp_div_qr(): Zero operand not allowed".to_string(),
            line: vm.current_line,
        });
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

/// gmp_mod(a, b) -> GMP (always non-negative remainder)
fn gmp_mod(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_mod")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_mod")),
    };

    if b.is_zero() {
        return Err(VmError {
            message: "gmp_mod(): Zero operand not allowed".to_string(),
            line: vm.current_line,
        });
    }

    let r = a.mod_floor(&b.abs());
    Ok(bigint_to_gmp_value(vm, r))
}

/// gmp_neg(a) -> GMP
fn gmp_neg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_neg")),
    };
    Ok(bigint_to_gmp_value(vm, -a))
}

/// gmp_abs(a) -> GMP
fn gmp_abs(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_abs")),
    };
    Ok(bigint_to_gmp_value(vm, a.abs()))
}

/// gmp_cmp(a, b) -> int (-1, 0, 1)
fn gmp_cmp(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_cmp")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_cmp")),
    };
    let result = match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    Ok(Value::Long(result))
}

/// gmp_sign(a) -> int (-1, 0, 1)
fn gmp_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_sign")),
    };
    let result = match a.sign() {
        Sign::Plus => 1,
        Sign::NoSign => 0,
        Sign::Minus => -1,
    };
    Ok(Value::Long(result))
}

/// gmp_pow(base, exp) -> GMP
fn gmp_pow(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_pow")),
    };
    let exp = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if exp < 0 {
        return Err(VmError {
            message: "gmp_pow(): Negative exponent not supported".to_string(),
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, base.pow(exp as u32)))
}

/// gmp_powm(base, exp, mod) -> GMP
fn gmp_powm(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_powm")),
    };
    let exp = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_powm")),
    };
    let modulus = match value_to_bigint(args.get(2).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_powm")),
    };

    if modulus.is_zero() {
        return Err(VmError {
            message: "gmp_powm(): Zero modulus not allowed".to_string(),
            line: vm.current_line,
        });
    }

    // For negative exponents, compute modular inverse first
    if exp.sign() == Sign::Minus {
        match mod_inverse(&base, &modulus) {
            Ok(inv) => {
                let pos_exp = exp.abs();
                let result = inv.modpow(&pos_exp, &modulus.abs());
                Ok(bigint_to_gmp_value(vm, result))
            }
            Err(e) => Err(VmError {
                message: format!("gmp_powm(): {}", e),
                line: vm.current_line,
            }),
        }
    } else {
        let result = base.modpow(&exp, &modulus.abs());
        Ok(bigint_to_gmp_value(vm, result))
    }
}

/// gmp_sqrt(a) -> GMP
fn gmp_sqrt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_sqrt")),
    };
    if a.sign() == Sign::Minus {
        return Err(VmError {
            message: "gmp_sqrt(): Number has to be greater than or equal to 0".to_string(),
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, isqrt(&a)))
}

/// gmp_gcd(a, b) -> GMP
fn gmp_gcd(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_gcd")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_gcd")),
    };
    Ok(bigint_to_gmp_value(vm, a.gcd(&b)))
}

/// gmp_lcm(a, b) -> GMP
fn gmp_lcm(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_lcm")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_lcm")),
    };

    if a.is_zero() || b.is_zero() {
        return Ok(bigint_to_gmp_value(vm, BigInt::zero()));
    }

    Ok(bigint_to_gmp_value(vm, a.lcm(&b)))
}

/// gmp_and(a, b) -> GMP
fn gmp_and(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_and")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_and")),
    };
    Ok(bigint_to_gmp_value(vm, &a & &b))
}

/// gmp_or(a, b) -> GMP
fn gmp_or(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_or")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_or")),
    };
    Ok(bigint_to_gmp_value(vm, &a | &b))
}

/// gmp_xor(a, b) -> GMP
fn gmp_xor(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_xor")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_xor")),
    };
    Ok(bigint_to_gmp_value(vm, &a ^ &b))
}

/// gmp_com(a) -> GMP (one's complement, i.e., ~n = -(n+1))
fn gmp_com(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_com")),
    };
    // ~n = -(n+1)
    Ok(bigint_to_gmp_value(vm, -(a + BigInt::one())))
}

/// gmp_fact(n) -> GMP
fn gmp_fact(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let n = match value_to_bigint(val) {
        Some(n) => n.to_i64().unwrap_or(-1),
        None => return Err(gmp_error(vm, "gmp_fact")),
    };
    if n < 0 {
        return Err(VmError {
            message: "gmp_fact(): Number has to be greater than or equal to 0".to_string(),
            line: vm.current_line,
        });
    }
    let result = (1..=n as u32).fold(BigInt::one(), |acc, i| acc * BigInt::from(i));
    Ok(bigint_to_gmp_value(vm, result))
}

/// gmp_prob_prime(n, reps=10) -> int (0, 1, or 2)
fn gmp_prob_prime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_prob_prime")),
    };
    let reps = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;
    Ok(Value::Long(is_probably_prime(&n, reps) as i64))
}

/// gmp_nextprime(n) -> GMP
fn gmp_nextprime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_nextprime")),
    };
    Ok(bigint_to_gmp_value(vm, next_prime(&n)))
}

/// gmp_testbit(a, index) -> bool
fn gmp_testbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_testbit")),
    };
    let index = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if index < 0 {
        return Err(VmError {
            message: "gmp_testbit(): Bit index must be greater than or equal to 0".to_string(),
            line: vm.current_line,
        });
    }
    Ok(if test_bit(&a, index as u64) {
        Value::True
    } else {
        Value::False
    })
}

/// gmp_setbit(&a, index, value=true) -> void
/// Note: In PHP, gmp_setbit modifies the GMP object in place.
/// We handle this by updating the stored BigInt.
fn gmp_setbit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let index = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let bit_val = args.get(2).map(|v| v.is_truthy()).unwrap_or(true);

    if index < 0 {
        return Err(VmError {
            message: "gmp_setbit(): Bit index must be greater than or equal to 0".to_string(),
            line: vm.current_line,
        });
    }

    // Get the object ID from the value
    let obj_id = match val {
        Value::Object(obj) => obj.borrow().object_id,
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::Object(obj) => obj.borrow().object_id,
                _ => {
                    return Err(gmp_error(vm, "gmp_setbit"));
                }
            }
        }
        _ => {
            return Err(gmp_error(vm, "gmp_setbit"));
        }
    };

    GMP_VALUES.with(|m| {
        let mut map = m.borrow_mut();
        if let Some(n) = map.get(&obj_id) {
            let new_val = set_bit(n, index as u64, bit_val);
            map.insert(obj_id, new_val);
        }
    });

    Ok(Value::Null)
}

/// gmp_popcount(a) -> int
fn gmp_popcount(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_popcount")),
    };
    Ok(Value::Long(popcount(&a)))
}

/// gmp_perfect_square(a) -> bool
fn gmp_perfect_square(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_perfect_square")),
    };
    Ok(if is_perfect_square(&a) {
        Value::True
    } else {
        Value::False
    })
}

/// gmp_invert(a, b) -> GMP|false
fn gmp_invert(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_invert")),
    };
    let b = match value_to_bigint(args.get(1).unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_invert")),
    };

    match mod_inverse(&a, &b) {
        Ok(result) => Ok(bigint_to_gmp_value(vm, result)),
        Err(_) => Ok(Value::False),
    }
}

// === Helper for rounding modes ===

/// Apply rounding mode to quotient.
/// round=0 (GMP_ROUND_ZERO): truncate toward zero (default)
/// round=1 (GMP_ROUND_PLUSINF): round toward +infinity
/// round=2 (GMP_ROUND_MINUSINF): round toward -infinity
fn apply_rounding(q: BigInt, r: &BigInt, b: &BigInt, round: i64) -> BigInt {
    if r.is_zero() {
        return q;
    }
    match round {
        0 => q, // Truncate toward zero (C-style division, which is what div_rem gives us)
        1 => {
            // Round toward +infinity (ceiling)
            // If remainder has same sign as divisor, we need to round up
            if (r.sign() == Sign::Plus && b.sign() == Sign::Plus)
                || (r.sign() == Sign::Minus && b.sign() == Sign::Minus)
            {
                q + BigInt::one()
            } else {
                q
            }
        }
        2 => {
            // Round toward -infinity (floor)
            // If remainder has different sign from divisor, we need to round down
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

/// Create a generic GMP error
fn gmp_error(vm: &Vm, func_name: &str) -> VmError {
    VmError {
        message: format!(
            "{}(): Unable to convert variable to GMP - string is not an integer",
            func_name
        ),
        line: vm.current_line,
    }
}
