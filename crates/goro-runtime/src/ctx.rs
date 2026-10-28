//! The interface between runtime operations and the engine.

use crate::array::PhpArray;
use crate::error::{Level, Notice, PhpError, PhpResult};
use crate::numeric::Number;
use crate::object::ObjRef;
use crate::ops::BinOp;
use crate::string::PhpStr;
use crate::value::Value;

/// Engine services needed by conversions and operators: reporting
/// diagnostics, ini settings, and object behaviour that needs user code.
///
/// Every method except [`Ctx::notice`] has a default suitable for plain
/// objects without magic methods, so tests can use [`Diagnostics`].
pub trait Ctx {
    /// Reports a warning/deprecation/notice, in the order PHP raises them.
    fn notice(&mut self, level: Level, message: String);

    /// ini `precision` (used by string conversion of floats; default 14).
    fn precision(&self) -> i32 {
        14
    }

    /// String conversion of an object (`__toString`). Default: PHP's error
    /// for objects without it.
    fn object_to_string(&mut self, o: &ObjRef) -> PhpResult<PhpStr> {
        Err(PhpError::error(format!(
            "Object of class {} could not be converted to string",
            o.class_name().to_string_lossy()
        )))
    }

    /// Numeric conversion of an object for casts (`(int)$o`, `(float)$o`).
    /// Default: PHP's warning and 1.
    fn object_to_number(&mut self, o: &ObjRef, want_float: bool) -> Number {
        self.notice(
            Level::Warning,
            format!(
                "Object of class {} could not be converted to {}",
                o.class_name().to_string_lossy(),
                if want_float { "float" } else { "int" }
            ),
        );
        if want_float {
            Number::Float(1.0)
        } else {
            Number::Int(1)
        }
    }

    /// `(array)$o`. Default: a copy of the property table.
    fn object_to_array(&mut self, o: &ObjRef) -> PhpArray {
        o.obj().props.borrow().clone()
    }

    /// `$a <=> $b` where at least one operand is an object. Return -1, 0 or
    /// 1 (1 also for "uncomparable"). Default: equal only to itself.
    fn object_compare(&mut self, a: &Value, b: &Value) -> PhpResult<i32> {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) if ObjRef::ptr_eq(x, y) => Ok(0),
            _ => Ok(1),
        }
    }

    /// `$a == $b` where at least one operand is an object.
    fn object_loose_eq(&mut self, a: &Value, b: &Value) -> PhpResult<bool> {
        Ok(self.object_compare(a, b)? == 0)
    }

    /// Operator overloading for internal classes (GMP and friends). `None`
    /// means "not overloaded": the operator raises PHP's usual TypeError.
    fn object_binop(&mut self, _op: BinOp, _a: &Value, _b: &Value) -> Option<PhpResult<Value>> {
        None
    }
}

/// A [`Ctx`] that records diagnostics in a vector and uses default object
/// behaviour.
#[derive(Default, Debug)]
pub struct Diagnostics {
    pub notices: Vec<Notice>,
    pub precision: Option<i32>,
}

impl Diagnostics {
    pub fn new() -> Diagnostics {
        Diagnostics::default()
    }
    /// Takes the recorded diagnostics.
    pub fn take(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }
}

impl Ctx for Diagnostics {
    fn notice(&mut self, level: Level, message: String) {
        self.notices.push(Notice { level, message });
    }
    fn precision(&self) -> i32 {
        self.precision.unwrap_or(14)
    }
}
