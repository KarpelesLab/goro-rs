use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::array::PhpArray;
use crate::generator::PhpGenerator;
use crate::object::PhpObject;
use crate::string::PhpString;

thread_local! {
    /// PHP `precision` ini setting. Default 14. -1 means shortest roundtrip.
    static PHP_PRECISION: Cell<i32> = const { Cell::new(14) };
    /// PHP `serialize_precision` ini setting. Default -1 (shortest roundtrip).
    static PHP_SERIALIZE_PRECISION: Cell<i32> = const { Cell::new(-1) };
    /// Current memory usage in bytes (approximate tracking)
    static MEMORY_USAGE: Cell<usize> = const { Cell::new(0) };
    /// Memory limit in bytes (default 128MB, 0 = unlimited)
    static MEMORY_LIMIT: Cell<usize> = const { Cell::new(128 * 1024 * 1024) };
}

/// Track a memory allocation. Returns false if over limit.
pub fn memory_alloc(bytes: usize) -> bool {
    MEMORY_USAGE.with(|m| {
        let current = m.get();
        let limit = MEMORY_LIMIT.with(|l| l.get());
        let new_total = current.saturating_add(bytes);
        if limit > 0 && new_total > limit {
            return false;
        }
        m.set(new_total);
        true
    })
}

/// Track a memory deallocation.
pub fn memory_free(bytes: usize) {
    MEMORY_USAGE.with(|m| {
        let current = m.get();
        m.set(current.saturating_sub(bytes));
    });
}

/// Get current memory usage.
pub fn memory_get_usage() -> usize {
    MEMORY_USAGE.with(|m| m.get())
}

/// Get memory limit.
pub fn memory_get_limit() -> usize {
    MEMORY_LIMIT.with(|l| l.get())
}

/// Set memory limit. 0 = unlimited, -1 = unlimited.
pub fn set_memory_limit(bytes: i64) {
    let limit = if bytes <= 0 { 0 } else { bytes as usize };
    MEMORY_LIMIT.with(|l| l.set(limit));
}

/// Reset memory tracking (for test isolation).
pub fn memory_reset() {
    MEMORY_USAGE.with(|m| m.set(0));
}

/// Set the PHP `precision` ini value (used by float-to-string conversion).
pub fn set_php_precision(p: i32) {
    PHP_PRECISION.with(|c| c.set(p));
}

/// Get the current PHP `precision` ini value.
pub fn get_php_precision() -> i32 {
    PHP_PRECISION.with(|c| c.get())
}

/// Set the PHP `serialize_precision` ini value (used by var_dump, json_encode, etc.).
pub fn set_php_serialize_precision(p: i32) {
    PHP_SERIALIZE_PRECISION.with(|c| c.set(p));
}

/// Get the current PHP `serialize_precision` ini value.
pub fn get_php_serialize_precision() -> i32 {
    PHP_SERIALIZE_PRECISION.with(|c| c.get())
}

/// The core value type (equivalent to zval in PHP)
#[derive(Clone, Default)]
pub enum Value {
    Undef,
    #[default]
    Null,
    False,
    True,
    Long(i64),
    Double(f64),
    String(PhpString),
    Array(Rc<RefCell<PhpArray>>),
    Object(Rc<RefCell<PhpObject>>),
    /// A PHP reference (&$var) - both variables share the same Rc<RefCell<Value>>
    Reference(Rc<RefCell<Value>>),
    /// A PHP Generator object
    Generator(Rc<RefCell<PhpGenerator>>),
}

impl Value {
    /// Dereference a value: if it's a Reference, return the inner value; otherwise return self.
    /// This is used by read_operand so existing code transparently reads through references.
    pub fn deref(&self) -> Value {
        match self {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        }
    }

    /// Returns true if this value is a Reference
    pub fn is_reference(&self) -> bool {
        matches!(self, Value::Reference(_))
    }

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
            Value::Object(_) | Value::Generator(_) => true,
            Value::Reference(r) => r.borrow().is_truthy(),
        }
    }

    // ---- Type name ----

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Undef => "NULL",
            Value::Null => "NULL",
            Value::False | Value::True => "bool",
            Value::Long(_) => "int",
            Value::Double(_) => "float",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) | Value::Generator(_) => "object",
            Value::Reference(r) => r.borrow().type_name(),
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
                        result = result
                            .wrapping_mul(10)
                            .wrapping_add((ch as u8 - b'0') as i64);
                    } else {
                        break;
                    }
                }
                if negative { -result } else { result }
            }
            Value::Array(arr) => {
                if arr.borrow().is_empty() {
                    0
                } else {
                    1
                }
            }
            Value::Object(_) | Value::Generator(_) => 1,
            Value::Reference(r) => r.borrow().to_long(),
        }
    }

    pub fn to_double(&self) -> f64 {
        match self {
            Value::Undef | Value::Null | Value::False => 0.0,
            Value::True => 1.0,
            Value::Long(n) => *n as f64,
            Value::Double(f) => *f,
            Value::String(s) => {
                let str = s.to_string_lossy();
                let str = str.trim();
                // PHP parses leading numeric portion
                parse_leading_float(str)
            }
            Value::Array(arr) => {
                if arr.borrow().is_empty() {
                    0.0
                } else {
                    1.0
                }
            }
            Value::Object(_) | Value::Generator(_) => 1.0,
            Value::Reference(r) => r.borrow().to_double(),
        }
    }

    pub fn to_php_string(&self) -> PhpString {
        match self {
            Value::Undef | Value::Null => PhpString::empty(),
            Value::False => PhpString::empty(),
            Value::True => PhpString::from_bytes(b"1"),
            Value::Long(n) => PhpString::from_string(n.to_string()),
            Value::Double(f) => PhpString::from_string(format_php_float(*f)),
            Value::String(s) => s.clone(),
            Value::Array(_) => PhpString::from_bytes(b"Array"),
            Value::Object(obj) => {
                let obj = obj.borrow();
                // PHP tries __toString magic method; for now return class name
                PhpString::from_vec(obj.class_name.clone())
            }
            Value::Generator(_) => PhpString::from_bytes(b"Generator"),
            Value::Reference(r) => r.borrow().to_php_string(),
        }
    }

    pub fn to_bool(&self) -> bool {
        self.is_truthy()
    }

    // ---- Arithmetic helpers ----

    pub fn add(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().add(other);
        }
        if let Value::Reference(r) = other {
            return self.add(&r.borrow());
        }
        match (self, other) {
            // Array + Array = array union
            (Value::Array(a), Value::Array(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                let mut result = crate::array::PhpArray::new();
                // Copy all entries from a
                for (key, val) in a.iter() {
                    result.set(key.clone(), val.clone());
                }
                // Add entries from b that don't exist in a
                for (key, val) in b.iter() {
                    if !result.contains_key(key) {
                        result.set(key.clone(), val.clone());
                    }
                }
                Value::Array(std::rc::Rc::new(std::cell::RefCell::new(result)))
            }
            (Value::Long(a), Value::Long(b)) => match a.checked_add(*b) {
                Some(result) => Value::Long(result),
                None => Value::Double(*a as f64 + *b as f64),
            },
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
        if let Value::Reference(r) = self {
            return r.borrow().sub(other);
        }
        if let Value::Reference(r) = other {
            return self.sub(&r.borrow());
        }
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => match a.checked_sub(*b) {
                Some(result) => Value::Long(result),
                None => Value::Double(*a as f64 - *b as f64),
            },
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
        if let Value::Reference(r) = self {
            return r.borrow().mul(other);
        }
        if let Value::Reference(r) = other {
            return self.mul(&r.borrow());
        }
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => match a.checked_mul(*b) {
                Some(result) => Value::Long(result),
                None => Value::Double(*a as f64 * *b as f64),
            },
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
        if let Value::Reference(r) = self {
            return r.borrow().div(other);
        }
        if let Value::Reference(r) = other {
            return self.div(&r.borrow());
        }
        let a = self.to_double();
        let b = other.to_double();
        if b == 0.0 {
            return Err("Division by zero");
        }
        let result = a / b;
        // If both operands were ints and result is a whole number, return int
        if matches!(self, Value::Long(_))
            && matches!(other, Value::Long(_))
            && result.fract() == 0.0
            && result.abs() < i64::MAX as f64
        {
            return Ok(Value::Long(result as i64));
        }
        Ok(Value::Double(result))
    }

    pub fn modulo(&self, other: &Value) -> Result<Value, &'static str> {
        if let Value::Reference(r) = self {
            return r.borrow().modulo(other);
        }
        if let Value::Reference(r) = other {
            return self.modulo(&r.borrow());
        }
        let a = self.to_long();
        let b = other.to_long();
        if b == 0 {
            return Err("Modulo by zero");
        }
        // Handle overflow case: i64::MIN % -1 panics in Rust but PHP returns 0
        if a == i64::MIN && b == -1 {
            Ok(Value::Long(0))
        } else {
            Ok(Value::Long(a % b))
        }
    }

    pub fn pow(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().pow(other);
        }
        if let Value::Reference(r) = other {
            return self.pow(&r.borrow());
        }
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
        if let Value::Reference(r) = self {
            return r.borrow().concat(other);
        }
        if let Value::Reference(r) = other {
            return self.concat(&r.borrow());
        }
        let a = self.to_php_string();
        let b = other.to_php_string();
        let mut result = a.as_bytes().to_vec();
        result.extend_from_slice(b.as_bytes());
        Value::String(PhpString::from_vec(result))
    }

    pub fn negate(&self) -> Value {
        match self {
            Value::Reference(r) => r.borrow().negate(),
            Value::Long(n) => {
                match n.checked_neg() {
                    Some(neg) => Value::Long(neg),
                    None => Value::Double(-(*n as f64)),
                }
            }
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
            Value::Array(_) | Value::Object(_) | Value::Generator(_) => {
                Value::Long(if self.is_truthy() { 1 } else { 0 })
            }
            Value::Reference(r) => r.borrow().to_numeric(),
        }
    }

    // ---- Comparison ----

    /// PHP == comparison
    pub fn equals(&self, other: &Value) -> bool {
        if let Value::Reference(r) = self {
            return r.borrow().equals(other);
        }
        if let Value::Reference(r) = other {
            return self.equals(&r.borrow());
        }
        match (self, other) {
            (Value::Null | Value::Undef, Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::False)
            | (Value::False, Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::String(s))
            | (Value::String(s), Value::Null | Value::Undef) => s.as_bytes().is_empty(),
            (Value::Null | Value::Undef, Value::Long(0))
            | (Value::Long(0), Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::Double(f))
            | (Value::Double(f), Value::Null | Value::Undef) => *f == 0.0,
            (Value::Null | Value::Undef, Value::Array(a))
            | (Value::Array(a), Value::Null | Value::Undef) => a.borrow().len() == 0,
            (Value::Null | Value::Undef, _) | (_, Value::Null | Value::Undef) => false,
            (Value::True, other) | (other, Value::True) => other.is_truthy(),
            (Value::False, other) | (other, Value::False) => !other.is_truthy(),
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::Long(a), Value::Double(b)) | (Value::Double(b), Value::Long(a)) => {
                *a as f64 == *b
            }
            (Value::String(a), Value::String(b)) => {
                // PHP 8: if both strings are numeric, compare as numbers
                // First try comparing as integers (handles large numbers that lose f64 precision)
                let a_bytes = a.as_bytes();
                let b_bytes = b.as_bytes();
                let a_str = std::str::from_utf8(a_bytes).unwrap_or("");
                let b_str = std::str::from_utf8(b_bytes).unwrap_or("");
                let a_trimmed = a_str.trim();
                let b_trimmed = b_str.trim();

                // Check if both are pure integer strings
                let a_is_int = is_integer_string(a_trimmed);
                let b_is_int = is_integer_string(b_trimmed);

                if a_is_int && b_is_int {
                    // Try parsing as i64 first
                    match (a_trimmed.parse::<i64>(), b_trimmed.parse::<i64>()) {
                        (Ok(na), Ok(nb)) => na == nb,
                        _ => {
                            // Too large for i64 - compare the numeric value as strings
                            // by normalizing (strip leading zeros, compare)
                            compare_integer_strings(a_trimmed, b_trimmed) == 0
                        }
                    }
                } else if let (Some(na), Some(nb)) = (
                    parse_numeric_string(a_bytes),
                    parse_numeric_string(b_bytes),
                ) {
                    na == nb
                } else {
                    a_bytes == b_bytes
                }
            }
            (Value::String(s), Value::Long(n)) | (Value::Long(n), Value::String(s)) => {
                // PHP 8: if string is numeric, compare numerically; otherwise false
                let s_bytes = s.as_bytes();
                if let Some(num) = parse_numeric_string(s_bytes) {
                    num == *n as f64
                } else {
                    false
                }
            }
            (Value::String(s), Value::Double(f)) | (Value::Double(f), Value::String(s)) => {
                let s_bytes = s.as_bytes();
                if let Some(num) = parse_numeric_string(s_bytes) {
                    num == *f
                } else {
                    false
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                if a.len() != b.len() {
                    return false;
                }
                for (key, a_val) in a.iter() {
                    match b.get(key) {
                        Some(b_val) => {
                            if !a_val.equals(b_val) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            (Value::Object(a), Value::Object(b)) => {
                // Same object reference
                if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
                    return true;
                }
                // Depth guard to prevent stack overflow on self-referencing objects
                thread_local! { static CMP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
                let depth = CMP_DEPTH.with(|d| d.get());
                if depth > 20 {
                    return true; // Assume equal at depth limit
                }
                CMP_DEPTH.with(|d| d.set(depth + 1));
                let result = {
                    let a = a.borrow();
                    let b = b.borrow();
                    if a.class_name.eq_ignore_ascii_case(&b.class_name)
                        && a.properties.len() == b.properties.len()
                    {
                        let mut eq = true;
                        for (name, a_val) in &a.properties {
                            let b_val = b.get_property(name);
                            if !a_val.equals(&b_val) {
                                eq = false;
                                break;
                            }
                        }
                        eq
                    } else {
                        false
                    }
                };
                CMP_DEPTH.with(|d| d.set(depth));
                result
            }
            // Object vs scalar: convert object to int(1) for comparison
            (Value::Object(_), Value::Long(n)) | (Value::Long(n), Value::Object(_)) => {
                *n == 1
            }
            (Value::Object(_), Value::Double(f)) | (Value::Double(f), Value::Object(_)) => {
                *f == 1.0
            }
            (Value::Object(_), Value::String(s)) | (Value::String(s), Value::Object(_)) => {
                // Object compared to string: not equal in PHP 8
                false
            }
            _ => false,
        }
    }

    /// PHP == comparison with object-to-scalar casting
    /// This is the same as equals() but handles the object conversion case
    /// (used by VM which also emits the Notice)
    pub fn equals_with_object_cast(&self, other: &Value) -> bool {
        self.equals(other)
    }

    /// PHP === comparison
    pub fn identical(&self, other: &Value) -> bool {
        if let Value::Reference(r) = self {
            return r.borrow().identical(other);
        }
        if let Value::Reference(r) = other {
            return self.identical(&r.borrow());
        }
        match (self, other) {
            (Value::Undef, Value::Undef) => true,
            (Value::Null, Value::Null) => true,
            (Value::True, Value::True) | (Value::False, Value::False) => true,
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
            (Value::Array(a), Value::Array(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                if a.len() != b.len() {
                    return false;
                }
                for (key, a_val) in a.iter() {
                    match b.get(key) {
                        Some(b_val) => {
                            if !a_val.identical(b_val) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            (Value::Object(a), Value::Object(b)) => {
                // === for objects means same instance
                std::ptr::eq(a.as_ptr(), b.as_ptr())
            }
            _ => false,
        }
    }

    /// PHP <=> comparison, returns -1, 0, or 1
    pub fn compare(&self, other: &Value) -> i64 {
        if let Value::Reference(r) = self {
            return r.borrow().compare(other);
        }
        if let Value::Reference(r) = other {
            return self.compare(&r.borrow());
        }
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => {
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Double(a), Value::Double(b)) => {
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Long(a), Value::Double(b)) => {
                let a = *a as f64;
                if a < *b {
                    -1
                } else if a > *b {
                    1
                } else {
                    0
                }
            }
            (Value::Double(a), Value::Long(b)) => {
                let b = *b as f64;
                if *a < b {
                    -1
                } else if *a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                // Compare by size first
                if a.len() != b.len() {
                    return if a.len() < b.len() { -1 } else { 1 };
                }
                // Compare element by element
                for (key, a_val) in a.iter() {
                    if let Some(b_val) = b.get(key) {
                        let cmp = a_val.compare(b_val);
                        if cmp != 0 {
                            return cmp;
                        }
                    } else {
                        return 1; // key exists in a but not b
                    }
                }
                0
            }
            (Value::Null | Value::Undef, Value::Null | Value::Undef) => 0,
            (Value::Null | Value::Undef, Value::False) => 0,
            (Value::False, Value::Null | Value::Undef) => 0,
            (Value::Null | Value::Undef, Value::True) => -1,
            (Value::True, Value::Null | Value::Undef) => 1,
            (Value::Null | Value::Undef, Value::Long(n)) => {
                if 0 < *n {
                    -1
                } else if 0 > *n {
                    1
                } else {
                    0
                }
            }
            (Value::Long(n), Value::Null | Value::Undef) => {
                if *n < 0 {
                    -1
                } else if *n > 0 {
                    1
                } else {
                    0
                }
            }
            (Value::Null | Value::Undef, Value::Double(f)) => {
                if 0.0 < *f {
                    -1
                } else if 0.0 > *f {
                    1
                } else {
                    0
                }
            }
            (Value::Double(f), Value::Null | Value::Undef) => {
                if *f < 0.0 {
                    -1
                } else if *f > 0.0 {
                    1
                } else {
                    0
                }
            }
            (Value::Null | Value::Undef, Value::String(s)) => {
                if s.is_empty() {
                    0
                } else {
                    -1
                }
            }
            (Value::String(s), Value::Null | Value::Undef) => {
                if s.is_empty() {
                    0
                } else {
                    1
                }
            }
            (Value::Null | Value::Undef, Value::Array(a)) => {
                if a.borrow().len() == 0 { 0 } else { -1 }
            }
            (Value::Array(a), Value::Null | Value::Undef) => {
                if a.borrow().len() == 0 { 0 } else { 1 }
            }
            (Value::Null | Value::Undef, _) => -1,
            (_, Value::Null | Value::Undef) => 1,
            // Bool comparisons: convert other side to bool
            (Value::True, Value::True) | (Value::False, Value::False) => 0,
            (Value::True, Value::False) => 1,
            (Value::False, Value::True) => -1,
            (Value::True, other) => {
                if other.is_truthy() {
                    0
                } else {
                    1
                }
            }
            (Value::False, other) => {
                if other.is_truthy() {
                    -1
                } else {
                    0
                }
            }
            (other, Value::True) => {
                if other.is_truthy() {
                    0
                } else {
                    -1
                }
            }
            (other, Value::False) => {
                if other.is_truthy() {
                    1
                } else {
                    0
                }
            }
            (Value::String(a), Value::String(b)) => {
                // PHP 8: if both strings are numeric, compare as numbers
                let a_str = std::str::from_utf8(a.as_bytes()).unwrap_or("");
                let b_str = std::str::from_utf8(b.as_bytes()).unwrap_or("");
                let a_trimmed = a_str.trim();
                let b_trimmed = b_str.trim();
                let a_is_int = is_integer_string(a_trimmed);
                let b_is_int = is_integer_string(b_trimmed);

                if a_is_int && b_is_int {
                    match (a_trimmed.parse::<i64>(), b_trimmed.parse::<i64>()) {
                        (Ok(na), Ok(nb)) => {
                            if na < nb { -1 } else if na > nb { 1 } else { 0 }
                        }
                        _ => compare_integer_strings(a_trimmed, b_trimmed),
                    }
                } else if let (Some(na), Some(nb)) = (
                    parse_numeric_string(a.as_bytes()),
                    parse_numeric_string(b.as_bytes()),
                ) {
                    if na < nb {
                        -1
                    } else if na > nb {
                        1
                    } else {
                        0
                    }
                } else {
                    match a.as_bytes().cmp(b.as_bytes()) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    }
                }
            }
            // PHP 8: int vs string - compare numerically if string is numeric, else string > int
            (Value::Long(n), Value::String(s)) | (Value::String(s), Value::Long(n)) => {
                let is_left_long = matches!(self, Value::Long(_));
                if let Some(num) = parse_numeric_string(s.as_bytes()) {
                    let n_f = *n as f64;
                    if is_left_long {
                        if n_f < num { -1 } else if n_f > num { 1 } else { 0 }
                    } else {
                        if num < n_f { -1 } else if num > n_f { 1 } else { 0 }
                    }
                } else {
                    // PHP 8: non-numeric string always > int in comparison
                    // Actually: cast both to their natural comparison type
                    // In PHP 8, comparing int with non-numeric string:
                    // the int is cast to string and then compared as strings
                    let n_str = format!("{}", n);
                    let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                    if is_left_long {
                        match n_str.as_bytes().cmp(s_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    } else {
                        match s_str.as_bytes().cmp(n_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    }
                }
            }
            // PHP 8: float vs string - compare numerically if string is numeric, else cast float to string
            (Value::Double(f), Value::String(s)) | (Value::String(s), Value::Double(f)) => {
                let is_left_double = matches!(self, Value::Double(_));
                if let Some(num) = parse_numeric_string(s.as_bytes()) {
                    if is_left_double {
                        if *f < num { -1 } else if *f > num { 1 } else { 0 }
                    } else {
                        if num < *f { -1 } else if num > *f { 1 } else { 0 }
                    }
                } else {
                    // PHP 8: non-numeric string - cast float to string and compare as strings
                    let f_str = format_php_float(*f);
                    let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                    if is_left_double {
                        match f_str.as_bytes().cmp(s_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    } else {
                        match s_str.as_bytes().cmp(f_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    }
                }
            }
            // Object vs Object comparison
            (Value::Object(a_obj), Value::Object(b_obj)) => {
                let a_borrow = a_obj.borrow();
                let b_borrow = b_obj.borrow();
                // Enum cases: only equal if same class and same case name
                let a_is_enum = a_borrow.has_property(b"__enum_case");
                let b_is_enum = b_borrow.has_property(b"__enum_case");
                if a_is_enum || b_is_enum {
                    // Enums are not orderable - return incomparable (i64::MIN)
                    // They can only be compared for equality via == / ===
                    if a_is_enum && b_is_enum {
                        let a_class = &a_borrow.class_name;
                        let b_class = &b_borrow.class_name;
                        let a_case = a_borrow.get_property(b"name");
                        let b_case = b_borrow.get_property(b"name");
                        if a_class.eq_ignore_ascii_case(b_class) {
                            if a_case.compare(&b_case) == 0 {
                                return 0; // Same enum case
                            }
                        }
                    }
                    return i64::MIN; // Not comparable
                }
                // Regular objects: compare by class name and properties
                if !a_borrow.class_name.eq_ignore_ascii_case(&b_borrow.class_name) {
                    // Different classes - not comparable in PHP 8 for ordered comparison
                    // but == returns false (handled elsewhere), <=> should return 1
                    return 1;
                }
                // Same class: compare properties
                if a_borrow.properties.len() != b_borrow.properties.len() {
                    return if a_borrow.properties.len() < b_borrow.properties.len() { -1 } else { 1 };
                }
                // Compare each property value
                for (key, a_val) in &a_borrow.properties {
                    let b_val = b_borrow.get_property(key);
                    let cmp = a_val.compare(&b_val);
                    if cmp != 0 {
                        return cmp;
                    }
                }
                0
            }
            // Array vs non-array (non-null): arrays are always greater than scalars in PHP
            (Value::Array(_), _) => 1,
            (_, Value::Array(_)) => -1,
            // Object vs non-object: objects are generally greater except vs bool
            (Value::Object(_), _) | (_, Value::Object(_)) => {
                // For non-bool comparisons, PHP 8 produces a notice/warning
                // and converts object to int (1). But for enums this is different.
                let a = self.to_double();
                let b = other.to_double();
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            _ => {
                let a = self.to_double();
                let b = other.to_double();
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// Format a float the way PHP does (respecting `precision` ini setting).
/// With precision=14 (default), uses 14 significant digits.
/// With precision=-1, uses shortest roundtrip representation.
pub fn format_php_float(f: f64) -> String {
    let precision = get_php_precision();
    if precision < 0 {
        return format_php_float_shortest(f);
    }
    format_php_float_with_precision(f, precision as usize)
}

/// Format a float using shortest-roundtrip representation (like precision=-1).
fn format_php_float_shortest(f: f64) -> String {
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    let abs = f.abs();
    // Use scientific notation for very large/small numbers
    if abs != 0.0 && (abs >= 1e15 || abs < 1e-4) {
        // Find shortest roundtrip in scientific notation (start from 1 to ensure decimal point)
        for prec in 1..20 {
            let s = format!("{:.prec$e}", f, prec = prec);
            if let Ok(parsed) = s.parse::<f64>() {
                if parsed == f {
                    // Convert to PHP format: uppercase E, explicit +
                    if let Some(pos) = s.find('e') {
                        let mantissa = &s[..pos];
                        let exp: i32 = s[pos + 1..].parse().unwrap_or(0);
                        let exp_str = if exp >= 0 {
                            format!("E+{}", exp)
                        } else {
                            format!("E{}", exp)
                        };
                        return format!("{}{}", mantissa, exp_str);
                    }
                }
            }
        }
        return format!("{}", f);
    }

    // Try increasing precision until roundtrip works
    for prec in 0..20 {
        let s = format!("{:.prec$}", f, prec = prec);
        if let Ok(parsed) = s.parse::<f64>() {
            if parsed == f {
                return s;
            }
        }
    }
    format!("{}", f)
}

/// Public wrapper for format_php_float_with_precision
pub fn format_php_float_with_precision_pub(f: f64, sig_digits: usize) -> String {
    format_php_float_with_precision(f, sig_digits)
}

/// Format a float with a specific number of significant digits.
fn format_php_float_with_precision(f: f64, sig_digits: usize) -> String {
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    // PHP uses G format with `sig_digits` significant digits
    // This is equivalent to: sprintf("%.{sig_digits}G", f) but with some PHP-specific quirks
    let sig_digits = if sig_digits == 0 { 1 } else { sig_digits };
    // Rust's {:.Ne} gives N decimal places after leading digit = N+1 significant digits.
    // We want sig_digits significant digits total, so use sig_digits - 1.
    let fmt_width = if sig_digits > 0 { sig_digits - 1 } else { 0 };
    let s = format!("{:.width$e}", f, width = fmt_width);

    // Parse the scientific notation
    let parts: Vec<&str> = s.split('e').collect();
    if parts.len() != 2 {
        return format!("{}", f);
    }

    let mantissa = parts[0];
    let mut exp: i32 = parts[1].parse().unwrap_or(0);

    // Get the significant digits (strip sign and decimal point)
    let negative = mantissa.starts_with('-');
    let digits_str = mantissa.trim_start_matches('-').replace('.', "");

    // Trim to sig_digits significant digits
    let mut digits_vec: Vec<u8> = digits_str.bytes().take(sig_digits).collect();

    // Check if we need to round (next digit)
    if digits_str.len() > sig_digits {
        let next_digit = digits_str.as_bytes()[sig_digits] - b'0';
        if next_digit >= 5 {
            // Round up
            let mut carry = true;
            for d in digits_vec.iter_mut().rev() {
                if carry {
                    if *d == b'9' {
                        *d = b'0';
                    } else {
                        *d += 1;
                        carry = false;
                    }
                }
            }
            if carry {
                digits_vec.insert(0, b'1');
                // Adjust exponent since we added a leading digit
                exp += 1;
            }
        }
    }

    // Remove trailing zeros from significant digits
    while digits_vec.len() > 1 && *digits_vec.last().unwrap() == b'0' {
        digits_vec.pop();
    }

    let sig_count = digits_vec.len();
    let sig_str: String = digits_vec.iter().map(|&b| b as char).collect();

    // Position of decimal point: exp + 1 digits before decimal point
    let decimal_pos = exp + 1;

    let result = if decimal_pos <= 0 {
        // 0.00...digits  (e.g., 0.001234)
        let mut s = String::from("0.");
        for _ in 0..(-decimal_pos) {
            s.push('0');
        }
        s.push_str(&sig_str);
        s
    } else if decimal_pos as usize >= sig_count {
        // All digits before decimal point, no fractional part
        let mut s = sig_str.clone();
        for _ in 0..(decimal_pos as usize - sig_count) {
            s.push('0');
        }
        s
    } else {
        // Some digits before, some after decimal point
        let dp = decimal_pos as usize;
        let mut s = sig_str[..dp].to_string();
        s.push('.');
        s.push_str(&sig_str[dp..]);
        s
    };

    // Use scientific notation for very large/small numbers (like PHP)
    if exp >= 15 || exp <= -5 {
        // PHP uses uppercase E notation, always with at least one decimal digit
        let mantissa_part = if sig_count > 1 {
            format!("{}.{}", &sig_str[..1], &sig_str[1..])
        } else {
            format!("{}.0", sig_str)
        };
        let exp_str = if exp >= 0 {
            format!("E+{}", exp)
        } else {
            format!("E{}", exp)
        };
        let formatted = if negative {
            format!("-{}{}", mantissa_part, exp_str)
        } else {
            format!("{}{}", mantissa_part, exp_str)
        };
        return formatted;
    }

    if negative {
        format!("-{}", result)
    } else {
        result
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
            Value::Object(obj) => {
                let obj = obj.borrow();
                write!(f, "object({})", String::from_utf8_lossy(&obj.class_name))
            }
            Value::Generator(_) => write!(f, "object(Generator)"),
            Value::Reference(r) => {
                write!(f, "&{:?}", *r.borrow())
            }
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
                write!(f, "{}", format_php_float(*d))
            }
            Value::String(s) => f.write_str(&s.to_string_lossy()),
            Value::Array(_) => write!(f, "Array"),
            Value::Object(_) | Value::Generator(_) => write!(f, "Object"),
            Value::Reference(r) => {
                write!(f, "{}", *r.borrow())
            }
        }
    }
}

/// Parse a PHP numeric string, returning its value as f64 if it's numeric.
/// Handles integers, floats, and scientific notation with optional leading whitespace and sign.
/// Returns None for non-numeric strings.
/// Parse the leading numeric portion of a string as a float.
/// If the string doesn't start with a number, returns 0.0.
fn parse_leading_float(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    // Check for INF/NAN
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("inf") || trimmed.eq_ignore_ascii_case("infinity") {
        return f64::INFINITY;
    }
    if trimmed.eq_ignore_ascii_case("-inf") || trimmed.eq_ignore_ascii_case("-infinity") {
        return f64::NEG_INFINITY;
    }
    if trimmed.eq_ignore_ascii_case("nan") {
        return f64::NAN;
    }
    // Find the longest valid prefix that's a number
    let bytes = s.as_bytes();
    let mut i = 0;
    // Optional sign
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let start = i;
    // Digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    // Optional decimal
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start {
        return 0.0;
    }
    // Optional exponent
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let save = i;
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        } else {
            i = save; // Not a valid exponent, backtrack
        }
    }
    s[..i].parse::<f64>().unwrap_or(0.0)
}

/// Parse a PHP numeric string, returning its value as f64 if it's numeric.
pub fn parse_numeric_string(s: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(s).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Check for INF/NAN
    if s.eq_ignore_ascii_case("inf") || s.eq_ignore_ascii_case("infinity") {
        return Some(f64::INFINITY);
    }
    if s.eq_ignore_ascii_case("-inf") || s.eq_ignore_ascii_case("-infinity") {
        return Some(f64::NEG_INFINITY);
    }
    if s.eq_ignore_ascii_case("nan") {
        return Some(f64::NAN);
    }
    // Try parsing as a number (handles int, float, scientific notation)
    let mut chars = s.chars().peekable();

    // Optional sign
    if matches!(chars.peek(), Some('+') | Some('-')) {
        chars.next();
    }

    // Must start with digit or '.'
    let has_leading_digit = matches!(chars.peek(), Some('0'..='9'));
    let mut has_dot = false;
    let mut has_digits = false;

    // Integer/decimal part
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            has_digits = true;
            chars.next();
        } else if c == '.' && !has_dot {
            has_dot = true;
            chars.next();
        } else {
            break;
        }
    }

    if !has_digits && !has_leading_digit {
        return None;
    }

    // Optional exponent
    if matches!(chars.peek(), Some('e') | Some('E')) {
        chars.next();
        if matches!(chars.peek(), Some('+') | Some('-')) {
            chars.next();
        }
        if !matches!(chars.peek(), Some('0'..='9')) {
            return None;
        }
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                chars.next();
            } else {
                break;
            }
        }
    }

    // Must have consumed everything (trailing whitespace was already trimmed)
    if chars.peek().is_some() {
        return None;
    }

    s.parse::<f64>().ok()
}

/// Check if a trimmed string is a pure integer (digits only, with optional leading sign)
fn is_integer_string(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let s = if s.starts_with('+') || s.starts_with('-') {
        &s[1..]
    } else {
        s
    };
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Compare two integer strings numerically (handling arbitrarily large numbers)
fn compare_integer_strings(a: &str, b: &str) -> i64 {
    let a_neg = a.starts_with('-');
    let b_neg = b.starts_with('-');
    if a_neg != b_neg {
        return if a_neg { -1 } else { 1 };
    }
    let a_digits = a.trim_start_matches(|c: char| c == '-' || c == '+').trim_start_matches('0');
    let b_digits = b.trim_start_matches(|c: char| c == '-' || c == '+').trim_start_matches('0');
    let cmp = if a_digits.len() != b_digits.len() {
        if a_digits.len() < b_digits.len() { -1 } else { 1 }
    } else {
        match a_digits.cmp(b_digits) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    };
    if a_neg { -cmp } else { cmp }
}
