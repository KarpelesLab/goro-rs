//! The PHP value type.
//!
//! [`Value`] is 16 bytes: a tag and an 8-byte payload. Scalars (null, bool,
//! int, float) are stored inline and never allocate; strings, arrays,
//! objects, resources and references are single refcounted pointers, so
//! cloning a value is at most one non-atomic increment.
//!
//! `Option<Value>` is also 16 bytes; the engine can use `None` for PHP's
//! "undefined" (unset variable slots).

use std::fmt;

use crate::array::PhpArray;
use crate::object::{ObjRef, ResRef};
use crate::reference::PhpRef;
use crate::string::PhpStr;

/// A PHP value.
#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(PhpStr),
    Array(PhpArray),
    Object(ObjRef),
    Resource(ResRef),
    /// A reference cell. Only stored in slots that are references
    /// (variables, array elements, properties); operations on values
    /// dereference first.
    Ref(PhpRef),
}

/// PHP's type of a value, as `gettype`-like discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
    Resource,
}

impl Type {
    /// The type name PHP uses in error messages ("int", "float", "string",
    /// "bool", "array", "null", ...). Objects are named by class instead; see
    /// [`Value::type_name_for_error`].
    pub fn name(self) -> &'static str {
        match self {
            Type::Null => "null",
            Type::Bool => "bool",
            Type::Int => "int",
            Type::Float => "float",
            Type::String => "string",
            Type::Array => "array",
            Type::Object => "object",
            Type::Resource => "resource",
        }
    }
}

impl Value {
    /// A string value from bytes.
    #[inline]
    pub fn str(b: &[u8]) -> Value {
        Value::Str(PhpStr::new(b))
    }

    /// The type, looking through references.
    pub fn type_of(&self) -> Type {
        match self {
            Value::Null => Type::Null,
            Value::Bool(_) => Type::Bool,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Str(_) => Type::String,
            Value::Array(_) => Type::Array,
            Value::Object(_) => Type::Object,
            Value::Resource(_) => Type::Resource,
            Value::Ref(r) => r.borrow().type_of(),
        }
    }

    /// Name used in "Unsupported operand types: X + Y" style messages:
    /// the type name, or the class name for objects.
    pub fn type_name_for_error(&self) -> String {
        match self {
            Value::Object(o) => o.class_name().to_string_lossy(),
            Value::Ref(r) => r.borrow().type_name_for_error(),
            v => v.type_of().name().to_string(),
        }
    }

    /// `get_debug_type()`-style name: "null", "bool", "int", "float",
    /// "string", "array", class name, "resource (type)".
    pub fn debug_type(&self) -> String {
        match self {
            Value::Resource(r) => {
                if r.is_closed() {
                    "resource (closed)".to_string()
                } else {
                    format!("resource ({})", r.res().kind.borrow().to_string_lossy())
                }
            }
            v => v.type_name_for_error(),
        }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    #[inline]
    pub fn is_ref(&self) -> bool {
        matches!(self, Value::Ref(_))
    }

    /// The value with any reference resolved (clones the target).
    #[inline]
    pub fn deref(&self) -> Value {
        match self {
            Value::Ref(r) => r.get(),
            v => v.clone(),
        }
    }

    /// Converts in place: a reference slot becomes a plain copy of its
    /// target.
    #[inline]
    pub fn into_deref(self) -> Value {
        match self {
            Value::Ref(r) => r.get(),
            v => v,
        }
    }

    /// Calls `f` with the dereferenced value, without cloning.
    #[inline]
    pub fn with_deref<R>(&self, f: impl FnOnce(&Value) -> R) -> R {
        match self {
            Value::Ref(r) => f(&r.borrow()),
            v => f(v),
        }
    }

    /// Makes this slot a reference (if it isn't one) and returns the cell,
    /// as `$b = &$a` does to `$a`.
    pub fn make_ref(&mut self) -> PhpRef {
        if let Value::Ref(r) = self {
            return r.clone();
        }
        let r = PhpRef::new(std::mem::take(self));
        *self = Value::Ref(r.clone());
        r
    }

    /// Assigns to this slot with PHP assignment semantics: if the slot is a
    /// reference, the referenced value is replaced. Returns the old value so
    /// the caller controls when it is released.
    pub fn assign(&mut self, v: Value) -> Value {
        let v = v.into_deref();
        match self {
            Value::Ref(r) => r.set(v),
            slot => std::mem::replace(slot, v),
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&PhpStr> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&PhpArray> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut PhpArray> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&ObjRef> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Int(i)
    }
}
impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Value::Int(i as i64)
    }
}
impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}
impl From<PhpStr> for Value {
    fn from(s: PhpStr) -> Self {
        Value::Str(s)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(PhpStr::from(s))
    }
}
impl From<&[u8]> for Value {
    fn from(s: &[u8]) -> Self {
        Value::Str(PhpStr::new(s))
    }
}
impl From<PhpArray> for Value {
    fn from(a: PhpArray) -> Self {
        Value::Array(a)
    }
}
impl From<ObjRef> for Value {
    fn from(o: ObjRef) -> Self {
        Value::Object(o)
    }
}
impl From<PhpRef> for Value {
    fn from(r: PhpRef) -> Self {
        Value::Ref(r)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "bool({b})"),
            Value::Int(i) => write!(f, "int({i})"),
            Value::Float(x) => write!(f, "float({x:?})"),
            Value::Str(s) => write!(f, "string({s:?})"),
            Value::Array(a) => write!(f, "{a:?}"),
            Value::Object(o) => write!(f, "{o:?}"),
            Value::Resource(r) => write!(f, "{r:?}"),
            Value::Ref(r) => write!(f, "{r:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(std::mem::size_of::<Value>(), 16);
        assert_eq!(std::mem::size_of::<Option<Value>>(), 16);
    }

    #[test]
    fn refs() {
        let mut a = Value::Int(1);
        let r = a.make_ref();
        let mut b = Value::Ref(r.clone());
        b.assign(Value::Int(5));
        assert_eq!(a.deref().as_int(), Some(5));
        assert_eq!(r.refcount(), 3);
        drop(b);
        drop(r);
        assert_eq!(a.deref().as_int(), Some(5));
        // Nested reference creation reuses the cell.
        let r1 = a.make_ref();
        let r2 = PhpRef::new(a.clone());
        assert!(PhpRef::ptr_eq(&r1, &r2));
    }
}
