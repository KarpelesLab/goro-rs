use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::array::PhpArray;
use crate::string::PhpString;

/// The core value type (equivalent to zval in PHP)
#[derive(Clone)]
pub enum Value {
    Undef,
    Null,
    False,
    True,
    Long(i64),
    Double(f64),
    String(PhpString),
    Array(Rc<RefCell<PhpArray>>),
    // Object, Resource, Reference - to be added later
}

impl Value {
    // ---- Truthiness ----

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undef | Value::Null | Value::False => false,
            Value::True => true,
            Value::Long(n) => *n != 0,
            Value::Double(f) => *f != 0.0,
            Value::String(s) => {
                let data = s.as_bytes();
                !data.is_empty() && data != b"0"
            }
            Value::Array(arr) => !arr.borrow().is_empty(),
        }
    }

    // ---- Type name ----

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Undef => "NULL", // undefined acts as null
            Value::Null => "NULL",
            Value::False | Value::True => "bool",
            Value::Long(_) => "int",
            Value::Double(_) => "float",
            Value::String(_) => "string",
            Value::Array(_) => "array",
        }
    }

    // ---- Conversion helpers ----

    pub fn to_long(&self) -> i64 {
        match self {
            Value::Undef | Value::Null | Value::False => 0,
            Value::True => 1,
            Value::Long(n) => *n,
            Value::Double(f) => *f as i64,
            Value::String(s) => {
                let s = s.to_string_lossy();
                let s = s.trim();
                // PHP-style string to int: parse leading digits
                if s.is_empty() {
                    return 0;
                }
                let mut chars = s.chars();
                let negative = if chars.clone().next() == Some('-') {
                    chars.next();
                    true
                } else if chars.clone().next() == Some('+') {
                    chars.next();
                    false
                } else {
                    false
                };
                let mut result: i64 = 0;
                for ch in chars {
                    if ch.is_ascii_digit() {
                        result = result.wrapping_mul(10).wrapping_add((ch as u8 - b'0') as i64);
                    } else {
                        break;
                    }
                }
                if negative { -result } else { result }
            }
            Value::Array(arr) => {
                if arr.borrow().is_empty() { 0 } else { 1 }
            }
        }
    }

    pub fn to_double(&self) -> f64 {
        match self {
            Value::Undef | Value::Null | Value::False => 0.0,
            Value::True => 1.0,
            Value::Long(n) => *n as f64,
            Value::Double(f) => *f,
            Value::String(s) => {
                let s = s.to_string_lossy();
                let s = s.trim();
                s.parse::<f64>().unwrap_or(0.0)
            }
            Value::Array(arr) => {
                if arr.borrow().is_empty() { 0.0 } else { 1.0 }
            }
        }
    }

    pub fn to_php_string(&self) -> PhpString {
        match self {
            Value::Undef | Value::Null => PhpString::empty(),
            Value::False => PhpString::empty(),
            Value::True => PhpString::from_bytes(b"1"),
            Value::Long(n) => PhpString::from_string(n.to_string()),
            Value::Double(f) => {
                // PHP formats floats without trailing zeros, but always with at least one decimal
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    PhpString::from_string(format!("{}", *f as i64))
                } else {
                    // Use PHP's default precision (14 significant digits)
                    let s = format!("{:.14}", f);
                    // Trim trailing zeros but keep at least one decimal digit
                    let trimmed = s.trim_end_matches('0');
                    let trimmed = if trimmed.ends_with('.') {
                        trimmed
                    } else {
                        trimmed
                    };
                    PhpString::from_string(trimmed.to_string())
                }
            }
            Value::String(s) => s.clone(),
            Value::Array(_) => {
                // PHP emits a notice and returns "Array"
                PhpString::from_bytes(b"Array")
            }
        }
    }

    pub fn to_bool(&self) -> bool {
        self.is_truthy()
    }

    // ---- Arithmetic helpers ----

    pub fn add(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => {
                match a.checked_add(*b) {
                    Some(result) => Value::Long(result),
                    None => Value::Double(*a as f64 + *b as f64),
                }
            }
            (Value::Double(a), Value::Double(b)) => Value::Double(a + b),
            (Value::Long(a), Value::Double(b)) => Value::Double(*a as f64 + b),
            (Value::Double(a), Value::Long(b)) => Value::Double(a + *b as f64),
            _ => {
                // Type juggling: convert to numeric
                let a = self.to_numeric();
                let b = other.to_numeric();
                a.add(&b)
            }
        }
    }

    pub fn sub(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => {
                match a.checked_sub(*b) {
                    Some(result) => Value::Long(result),
                    None => Value::Double(*a as f64 - *b as f64),
                }
            }
            (Value::Double(a), Value::Double(b)) => Value::Double(a - b),
            (Value::Long(a), Value::Double(b)) => Value::Double(*a as f64 - b),
            (Value::Double(a), Value::Long(b)) => Value::Double(a - *b as f64),
            _ => {
                let a = self.to_numeric();
                let b = other.to_numeric();
                a.sub(&b)
            }
        }
    }

    pub fn mul(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => {
                match a.checked_mul(*b) {
                    Some(result) => Value::Long(result),
                    None => Value::Double(*a as f64 * *b as f64),
                }
            }
            (Value::Double(a), Value::Double(b)) => Value::Double(a * b),
            (Value::Long(a), Value::Double(b)) => Value::Double(*a as f64 * b),
            (Value::Double(a), Value::Long(b)) => Value::Double(a * *b as f64),
            _ => {
                let a = self.to_numeric();
                let b = other.to_numeric();
                a.mul(&b)
            }
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value, &'static str> {
        let a = self.to_double();
        let b = other.to_double();
        if b == 0.0 {
            return Err("Division by zero");
        }
        let result = a / b;
        // If both operands were ints and result is a whole number, return int
        if matches!(self, Value::Long(_)) && matches!(other, Value::Long(_)) {
            if result.fract() == 0.0 && result.abs() < i64::MAX as f64 {
                return Ok(Value::Long(result as i64));
            }
        }
        Ok(Value::Double(result))
    }

    pub fn modulo(&self, other: &Value) -> Result<Value, &'static str> {
        let a = self.to_long();
        let b = other.to_long();
        if b == 0 {
            return Err("Division by zero");
        }
        Ok(Value::Long(a % b))
    }

    pub fn pow(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Long(base), Value::Long(exp)) if *exp >= 0 => {
                match (*base as u64).checked_pow(*exp as u32) {
                    Some(result) if result <= i64::MAX as u64 => Value::Long(result as i64),
                    _ => Value::Double((*base as f64).powf(*exp as f64)),
                }
            }
            _ => Value::Double(self.to_double().powf(other.to_double())),
        }
    }

    pub fn concat(&self, other: &Value) -> Value {
        let a = self.to_php_string();
        let b = other.to_php_string();
        let mut result = a.as_bytes().to_vec();
        result.extend_from_slice(b.as_bytes());
        Value::String(PhpString::from_vec(result))
    }

    pub fn negate(&self) -> Value {
        match self {
            Value::Long(n) => Value::Long(-n),
            Value::Double(f) => Value::Double(-f),
            _ => {
                let n = self.to_numeric();
                n.negate()
            }
        }
    }

    /// Convert to numeric type (int or float), PHP-style
    fn to_numeric(&self) -> Value {
        match self {
            Value::Long(_) | Value::Double(_) => self.clone(),
            Value::True => Value::Long(1),
            Value::False | Value::Null | Value::Undef => Value::Long(0),
            Value::String(s) => {
                let s_str = s.to_string_lossy();
                let trimmed = s_str.trim();
                if trimmed.is_empty() {
                    return Value::Long(0);
                }
                // Try int first
                if let Ok(n) = trimmed.parse::<i64>() {
                    return Value::Long(n);
                }
                // Try float
                if let Ok(f) = trimmed.parse::<f64>() {
                    return Value::Double(f);
                }
                // Leading numeric portion
                Value::Long(self.to_long())
            }
            Value::Array(_) => Value::Long(if self.is_truthy() { 1 } else { 0 }),
        }
    }

    // ---- Comparison ----

    /// PHP == comparison
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null | Value::Undef, Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::False) | (Value::False, Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::String(s)) | (Value::String(s), Value::Null | Value::Undef) => s.as_bytes().is_empty(),
            (Value::Null | Value::Undef, Value::Long(0)) | (Value::Long(0), Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, _) | (_, Value::Null | Value::Undef) => false,
            (Value::True, other) | (other, Value::True) => other.is_truthy(),
            (Value::False, other) | (other, Value::False) => !other.is_truthy(),
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::Long(a), Value::Double(b)) | (Value::Double(b), Value::Long(a)) => *a as f64 == *b,
            (Value::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
            (Value::String(_), Value::Long(_)) | (Value::Long(_), Value::String(_)) => {
                self.to_double() == other.to_double()
            }
            _ => false,
        }
    }

    /// PHP === comparison
    pub fn identical(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::True, Value::True) | (Value::False, Value::False) => true,
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
            _ => false,
        }
    }

    /// PHP <=> comparison, returns -1, 0, or 1
    pub fn compare(&self, other: &Value) -> i64 {
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => {
                if a < b { -1 } else if a > b { 1 } else { 0 }
            }
            (Value::Double(a), Value::Double(b)) => {
                if a < b { -1 } else if a > b { 1 } else { 0 }
            }
            (Value::Long(a), Value::Double(b)) => {
                let a = *a as f64;
                if a < *b { -1 } else if a > *b { 1 } else { 0 }
            }
            (Value::Double(a), Value::Long(b)) => {
                let b = *b as f64;
                if *a < b { -1 } else if *a > b { 1 } else { 0 }
            }
            _ => {
                let a = self.to_double();
                let b = other.to_double();
                if a < b { -1 } else if a > b { 1 } else { 0 }
            }
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undef => write!(f, "UNDEF"),
            Value::Null => write!(f, "NULL"),
            Value::False => write!(f, "false"),
            Value::True => write!(f, "true"),
            Value::Long(n) => write!(f, "int({})", n),
            Value::Double(n) => write!(f, "float({})", n),
            Value::String(s) => write!(f, "string({:?})", s.to_string_lossy()),
            Value::Array(arr) => write!(f, "array({})", arr.borrow().len()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undef | Value::Null | Value::False => Ok(()),
            Value::True => write!(f, "1"),
            Value::Long(n) => write!(f, "{}", n),
            Value::Double(d) => {
                if d.fract() == 0.0 && d.abs() < 1e15 {
                    write!(f, "{}", *d as i64)
                } else {
                    write!(f, "{}", d)
                }
            }
            Value::String(s) => {
                // Write raw bytes
                f.write_str(&s.to_string_lossy())
            }
            Value::Array(_) => write!(f, "Array"),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Null
    }
}
