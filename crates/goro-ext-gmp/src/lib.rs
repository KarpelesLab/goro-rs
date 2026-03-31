mod bigint;

use bigint::BigInt;
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
    vm.constants.insert(b"GMP_ROUND_ZERO".to_vec(), Value::Long(0));
    vm.constants.insert(b"GMP_ROUND_PLUSINF".to_vec(), Value::Long(1));
    vm.constants.insert(b"GMP_ROUND_MINUSINF".to_vec(), Value::Long(2));
    vm.constants.insert(b"GMP_MSW_FIRST".to_vec(), Value::Long(1));
    vm.constants.insert(b"GMP_LSW_FIRST".to_vec(), Value::Long(2));
    vm.constants.insert(b"GMP_LITTLE_ENDIAN".to_vec(), Value::Long(4));
    vm.constants.insert(b"GMP_BIG_ENDIAN".to_vec(), Value::Long(8));
    vm.constants.insert(b"GMP_NATIVE_ENDIAN".to_vec(), Value::Long(16));
    vm.constants.insert(
        b"GMP_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(b"6.3.0")),
    );
}

// === Helper functions ===

/// Extract a BigInt from a Value. Accepts GMP objects, integers, and strings.
fn value_to_bigint(val: &Value) -> Option<BigInt> {
    match val {
        Value::Object(obj) => {
            let id = obj.borrow().object_id;
            GMP_VALUES.with(|m| m.borrow().get(&id).cloned())
        }
        Value::Long(n) => Some(BigInt::from_i64(*n)),
        Value::String(s) => {
            let s = s.to_string_lossy();
            let s = s.trim();
            if s.is_empty() {
                return Some(BigInt::from_i64(0));
            }
            BigInt::from_str(s, 10).ok()
        }
        Value::Double(f) => Some(BigInt::from_i64(*f as i64)),
        Value::True => Some(BigInt::from_i64(1)),
        Value::False | Value::Null | Value::Undef => Some(BigInt::from_i64(0)),
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

// === PHP function implementations ===

/// gmp_init(value, base=10) -> GMP
fn gmp_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;

    let n = match val {
        Value::Long(n) => BigInt::from_i64(*n),
        Value::String(s) => {
            let s = s.to_string_lossy();
            let s = s.trim();
            match BigInt::from_str(s, base) {
                Ok(n) => n,
                Err(_) => {
                    return Err(VmError {
                        message: format!("gmp_init(): Unable to convert variable to GMP - string is not an integer"),
                        line: vm.current_line,
                    });
                }
            }
        }
        Value::Double(f) => BigInt::from_i64(*f as i64),
        Value::True => BigInt::from_i64(1),
        Value::False | Value::Null | Value::Undef => BigInt::from_i64(0),
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
    let s = n.to_string_radix(base);
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
    Ok(Value::Long(n.to_i64()))
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
    Ok(bigint_to_gmp_value(vm, a.add(&b)))
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
    Ok(bigint_to_gmp_value(vm, a.sub(&b)))
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
    Ok(bigint_to_gmp_value(vm, a.mul(&b)))
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

    let (q, r) = match a.div_rem(&b) {
        Ok(qr) => qr,
        Err(_) => {
            return Err(VmError {
                message: "gmp_div_q(): Zero operand not allowed".to_string(),
                line: vm.current_line,
            });
        }
    };

    let result = apply_rounding(q, &r, &a, &b, round);
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

    let (q, r) = match a.div_rem(&b) {
        Ok(qr) => qr,
        Err(_) => {
            return Err(VmError {
                message: "gmp_div_r(): Zero operand not allowed".to_string(),
                line: vm.current_line,
            });
        }
    };

    let adjusted_q = apply_rounding(q, &r, &a, &b, round);
    // remainder = a - q*b
    let result = a.sub(&adjusted_q.mul(&b));
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

    let (q, r) = match a.div_rem(&b) {
        Ok(qr) => qr,
        Err(_) => {
            return Err(VmError {
                message: "gmp_div_qr(): Zero operand not allowed".to_string(),
                line: vm.current_line,
            });
        }
    };

    let adjusted_q = apply_rounding(q, &r, &a, &b, round);
    let adjusted_r = a.sub(&adjusted_q.mul(&b));

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

    let (_, mut r) = match a.div_rem(&b) {
        Ok(qr) => qr,
        Err(_) => {
            return Err(VmError {
                message: "gmp_mod(): Zero operand not allowed".to_string(),
                line: vm.current_line,
            });
        }
    };

    // Make remainder non-negative
    if r.signum() < 0 {
        r = r.add(&b.abs());
    }

    Ok(bigint_to_gmp_value(vm, r))
}

/// gmp_neg(a) -> GMP
fn gmp_neg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_neg")),
    };
    Ok(bigint_to_gmp_value(vm, a.neg()))
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
    let result = match BigInt::cmp_bigint(&a, &b) {
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
    Ok(Value::Long(a.signum() as i64))
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

    match base.powmod(&exp, &modulus) {
        Ok(result) => Ok(bigint_to_gmp_value(vm, result)),
        Err(e) => Err(VmError {
            message: format!("gmp_powm(): {}", e),
            line: vm.current_line,
        }),
    }
}

/// gmp_sqrt(a) -> GMP
fn gmp_sqrt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_sqrt")),
    };
    match a.sqrt() {
        Ok(result) => Ok(bigint_to_gmp_value(vm, result)),
        Err(_) => Err(VmError {
            message: "gmp_sqrt(): Number has to be greater than or equal to 0".to_string(),
            line: vm.current_line,
        }),
    }
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
        return Ok(bigint_to_gmp_value(vm, BigInt::from_i64(0)));
    }

    let gcd = a.gcd(&b);
    let (q, _) = a.abs().div_rem(&gcd).unwrap();
    let lcm = q.mul(&b.abs());
    Ok(bigint_to_gmp_value(vm, lcm))
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
    Ok(bigint_to_gmp_value(vm, a.bitand(&b)))
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
    Ok(bigint_to_gmp_value(vm, a.bitor(&b)))
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
    Ok(bigint_to_gmp_value(vm, a.bitxor(&b)))
}

/// gmp_com(a) -> GMP (one's complement)
fn gmp_com(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_com")),
    };
    Ok(bigint_to_gmp_value(vm, a.bitnot()))
}

/// gmp_fact(n) -> GMP
fn gmp_fact(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let n = match value_to_bigint(val) {
        Some(n) => n.to_i64(),
        None => return Err(gmp_error(vm, "gmp_fact")),
    };
    if n < 0 {
        return Err(VmError {
            message: "gmp_fact(): Number has to be greater than or equal to 0".to_string(),
            line: vm.current_line,
        });
    }
    Ok(bigint_to_gmp_value(vm, BigInt::factorial(n as u32)))
}

/// gmp_prob_prime(n, reps=10) -> int (0, 1, or 2)
fn gmp_prob_prime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_prob_prime")),
    };
    let reps = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;
    Ok(Value::Long(n.is_probably_prime(reps) as i64))
}

/// gmp_nextprime(n) -> GMP
fn gmp_nextprime(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let n = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_nextprime")),
    };
    Ok(bigint_to_gmp_value(vm, n.next_prime()))
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
    Ok(if a.test_bit(index as usize) {
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
            let new_val = n.set_bit(index as usize, bit_val);
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
    Ok(Value::Long(a.popcount()))
}

/// gmp_perfect_square(a) -> bool
fn gmp_perfect_square(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = match value_to_bigint(args.first().unwrap_or(&Value::Null)) {
        Some(n) => n,
        None => return Err(gmp_error(vm, "gmp_perfect_square")),
    };
    Ok(if a.is_perfect_square() {
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

    match a.mod_inverse(&b) {
        Ok(result) => Ok(bigint_to_gmp_value(vm, result)),
        Err(_) => Ok(Value::False),
    }
}

// === Helper for rounding modes ===

/// Apply rounding mode to quotient.
/// round=0 (GMP_ROUND_ZERO): truncate toward zero (default)
/// round=1 (GMP_ROUND_PLUSINF): round toward +infinity
/// round=2 (GMP_ROUND_MINUSINF): round toward -infinity
fn apply_rounding(q: BigInt, r: &BigInt, _a: &BigInt, b: &BigInt, round: i64) -> BigInt {
    if r.is_zero() {
        return q;
    }
    match round {
        0 => q, // Truncate toward zero (C-style division, which is what div_rem gives us)
        1 => {
            // Round toward +infinity (ceiling)
            // If remainder has same sign as divisor, we need to round up
            if (r.signum() > 0 && b.signum() > 0) || (r.signum() < 0 && b.signum() < 0) {
                q.add(&BigInt::from_i64(1))
            } else {
                q
            }
        }
        2 => {
            // Round toward -infinity (floor)
            // If remainder has different sign from divisor, we need to round down
            if (r.signum() > 0 && b.signum() < 0) || (r.signum() < 0 && b.signum() > 0) {
                q.sub(&BigInt::from_i64(1))
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
        message: format!("{}(): Unable to convert variable to GMP - string is not an integer", func_name),
        line: vm.current_line,
    }
}
