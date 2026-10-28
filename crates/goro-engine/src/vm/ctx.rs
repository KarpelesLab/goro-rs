//! The engine as the runtime's [`Ctx`]: diagnostics, object conversions
//! and comparisons that need user code.

use goro_runtime::numeric::Number;
use goro_runtime::ops::BinOp;
use goro_runtime::{Ctx, Key, Level, ObjRef, PhpArray, PhpError, PhpResult, PhpStr, Value};

use super::errors::{E_DEPRECATED, E_NOTICE, E_WARNING};
use super::{R, Unwind, Vm};
use crate::bytecode::CastTo;

impl Ctx for Vm {
    fn notice(&mut self, level: Level, message: String) {
        if self.err.deferred.is_some() {
            return;
        }
        let l = match level {
            Level::Warning => E_WARNING,
            Level::Notice => E_NOTICE,
            Level::Deprecated => E_DEPRECATED,
        };
        if let Err(e) = self.raise(l, &message) {
            self.err.deferred = Some(e);
        }
    }

    fn precision(&self) -> i32 {
        self.ini.precision
    }

    fn object_to_string(&mut self, o: &ObjRef) -> PhpResult<PhpStr> {
        match self.obj_to_string(o) {
            Ok(s) => Ok(s),
            Err(e) => {
                if let Unwind::Throw(ex) = &e
                    && self.err.deferred.is_none()
                {
                    // Re-raise through the deferred slot: the runtime only
                    // knows PhpError.
                    let _ = ex;
                }
                self.err.deferred = Some(e);
                Err(PhpError::error(String::new()))
            }
        }
    }

    fn object_to_number(&mut self, o: &ObjRef, want_float: bool) -> Number {
        let cls = self.class_of(o);
        if let Some(h) = cls.native.cast {
            let r = h(
                self,
                o,
                if want_float {
                    CastTo::Float
                } else {
                    CastTo::Int
                },
            );
            match r {
                Some(Ok(Value::Int(i))) => return Number::Int(i),
                Some(Ok(Value::Float(f))) => return Number::Float(f),
                Some(Err(e)) => {
                    self.err.deferred = Some(e);
                    return Number::Int(0);
                }
                _ => {}
            }
        }
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

    fn object_to_array(&mut self, o: &ObjRef) -> PhpArray {
        self.object_props_array(o, true)
    }

    fn object_compare(&mut self, a: &Value, b: &Value) -> Option<PhpResult<i32>> {
        let r = self.compare_objects(a, b);
        match r {
            Ok(Some(c)) => Some(Ok(c)),
            Ok(None) => None,
            Err(e) => {
                self.err.deferred = Some(e);
                Some(Err(PhpError::error(String::new())))
            }
        }
    }

    fn object_binop(&mut self, op: BinOp, a: &Value, b: &Value) -> Option<PhpResult<Value>> {
        let _ = (op, a, b);
        None
    }
}

impl Vm {
    pub fn to_bool(&mut self, v: &Value) -> bool {
        match v {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Object(o) => {
                let cls = self.class_of(o);
                if let Some(h) = cls.native.cast
                    && let Some(Ok(Value::Bool(b))) = h(self, o, CastTo::Bool)
                {
                    return b;
                }
                true
            }
            _ => goro_runtime::convert::to_bool(v, self),
        }
    }

    /// `(string)$v`.
    pub fn to_php_str(&mut self, v: &Value) -> R<PhpStr> {
        match v {
            Value::Str(s) => Ok(s.clone()),
            Value::Object(o) => self.obj_to_string(o),
            Value::Ref(r) => {
                let v = r.get();
                self.to_php_str(&v)
            }
            _ => {
                let r = goro_runtime::convert::to_str(v, self);
                self.check_deferred()?;
                r.map_err(|e| self.php_error(e))
            }
        }
    }

    pub fn append_to_string(&mut self, out: &mut Vec<u8>, v: &Value) -> R<()> {
        match v {
            Value::Str(s) => {
                out.extend_from_slice(s);
                Ok(())
            }
            Value::Object(o) => {
                let s = self.obj_to_string(o)?;
                out.extend_from_slice(&s);
                Ok(())
            }
            Value::Ref(r) => {
                let v = r.get();
                self.append_to_string(out, &v)
            }
            _ => {
                let r = goro_runtime::convert::append_str(out, v, self);
                self.check_deferred()?;
                r.map_err(|e| self.php_error(e))
            }
        }
    }

    /// `__toString()` of an object.
    pub fn obj_to_string(&mut self, o: &ObjRef) -> R<PhpStr> {
        let cls = self.class_of(o);
        let m = cls.magic.borrow().to_string.clone();
        match m {
            Some(f) => {
                let v = self.call_func_this(&f, Some(o.clone()), Vec::new())?;
                match v {
                    Value::Str(s) => Ok(s),
                    _ => self.throw(
                        "Error",
                        &format!(
                            "{}::__toString(): Return value must be of type string, {} returned",
                            cls.name.to_string_lossy(),
                            v.type_name_for_error()
                        ),
                    ),
                }
            }
            None => self.throw(
                "Error",
                &format!(
                    "Object of class {} could not be converted to string",
                    cls.name.to_string_lossy()
                ),
            ),
        }
    }

    pub fn to_int(&mut self, v: &Value) -> R<i64> {
        let r = goro_runtime::convert::to_int(v, self);
        self.check_deferred()?;
        Ok(r)
    }

    pub fn to_float(&mut self, v: &Value) -> R<f64> {
        let r = goro_runtime::convert::to_float(v, self);
        self.check_deferred()?;
        Ok(r)
    }

    pub fn to_key(&mut self, v: &Value, container: &str) -> R<Key> {
        match v {
            Value::Int(i) => Ok(Key::Int(*i)),
            Value::Str(s) => Ok(Key::from_php_str(s)),
            _ => {
                let r = goro_runtime::convert::to_key(v, container, self);
                self.check_deferred()?;
                r.map_err(|e| self.php_error(e))
            }
        }
    }

    /// Casts (`(int)`, `(array)`, ...).
    pub fn cast(&mut self, to: CastTo, v: Value) -> R<Value> {
        Ok(match to {
            CastTo::Int => match v {
                Value::Int(_) => v,
                Value::Float(f) => Value::Int(goro_runtime::convert::float_to_int_cast(f, self)),
                Value::Object(ref o) => {
                    let cls = self.class_of(o);
                    if let Some(h) = cls.native.cast
                        && let Some(r) = h(self, o, CastTo::Int)
                    {
                        return r;
                    }
                    let n = goro_runtime::convert::to_number_cast(&v, self);
                    self.check_deferred()?;
                    match n {
                        Number::Int(i) => Value::Int(i),
                        Number::Float(f) => Value::Int(f as i64),
                    }
                }
                _ => {
                    let i = goro_runtime::convert::to_int(&v, self);
                    self.check_deferred()?;
                    Value::Int(i)
                }
            },
            CastTo::Float => match v {
                Value::Float(_) => v,
                Value::Object(ref o) => {
                    let cls = self.class_of(o);
                    if let Some(h) = cls.native.cast
                        && let Some(r) = h(self, o, CastTo::Float)
                    {
                        return r;
                    }
                    let n = goro_runtime::convert::to_number_cast(&v, self);
                    self.check_deferred()?;
                    Value::Float(n.to_f64())
                }
                _ => {
                    let f = goro_runtime::convert::to_float(&v, self);
                    self.check_deferred()?;
                    Value::Float(f)
                }
            },
            CastTo::String => Value::Str(self.to_php_str(&v)?),
            CastTo::Bool => Value::Bool(self.to_bool(&v)),
            CastTo::Array => match v {
                Value::Array(_) => v,
                Value::Object(ref o) => Value::Array(self.object_props_array(o, true)),
                _ => Value::Array(goro_runtime::convert::to_array(&v, self)),
            },
            CastTo::Object => match v {
                Value::Object(_) => v,
                Value::Array(a) => {
                    let o = self.new_std_object()?;
                    for (k, x) in a.iter() {
                        let k = match k {
                            goro_runtime::KeyRef::Int(i) => Key::Str(PhpStr::from(i.to_string())),
                            goro_runtime::KeyRef::Str(s) => Key::Str(s.clone()),
                        };
                        o.obj().props.borrow_mut().insert(k, x.clone());
                    }
                    Value::Object(o)
                }
                Value::Null => Value::Object(self.new_std_object()?),
                other => {
                    let o = self.new_std_object()?;
                    o.obj()
                        .props
                        .borrow_mut()
                        .insert(Key::Str(PhpStr::intern(b"scalar")), other);
                    Value::Object(o)
                }
            },
            CastTo::Null => Value::Null,
        })
    }

    pub(crate) fn strlen_slow(&mut self, v: Value) -> R<Value> {
        let s = self.coerce_arg_string(&v, "strlen", 1, "string")?;
        Ok(Value::Int(s.len() as i64))
    }

    /// Converts an argument for a `string` parameter of a native function
    /// in coercive mode (used by inlined builtins).
    pub(crate) fn coerce_arg_string(
        &mut self,
        v: &Value,
        func: &str,
        n: usize,
        pname: &str,
    ) -> R<PhpStr> {
        match v {
            Value::Str(s) => Ok(s.clone()),
            Value::Int(_) | Value::Float(_) | Value::Bool(_) if !self.caller_strict() => {
                self.to_php_str(v)
            }
            Value::Null if !self.caller_strict() => {
                self.deprecated(&format!(
                    "{}(): Passing null to parameter #{} (${}) of type string is deprecated",
                    func, n, pname
                ))?;
                Ok(PhpStr::empty())
            }
            Value::Object(o)
                if self.class_of(o).magic.borrow().to_string.is_some() && !self.caller_strict() =>
            {
                self.obj_to_string(o)
            }
            _ => self.type_error(&format!(
                "{}(): Argument #{} (${}) must be of type string, {} given",
                func,
                n,
                pname,
                Vm::type_name_given(v)
            )),
        }
    }
}

impl Vm {
    /// `count()` of a value (arrays, Countable objects).
    pub fn count_value(&mut self, v: &Value, recursive: bool) -> R<i64> {
        match v {
            Value::Array(a) => {
                if !recursive {
                    return Ok(a.len() as i64);
                }
                let mut n = a.len() as i64;
                for (_, x) in a.iter() {
                    if let Value::Array(_) = x.deref() {
                        n += self.count_value(&x.deref(), true)?;
                    }
                }
                Ok(n)
            }
            Value::Object(o) => {
                let cls = self.class_of(o);
                if let Some(h) = cls.native.count {
                    return h(self, o);
                }
                if self
                    .core
                    .countable
                    .as_ref()
                    .is_some_and(|c| cls.is_subclass_of(c))
                {
                    let r = self.call_method(o, b"count", Vec::new())?;
                    return self.to_int(&r);
                }
                self.type_error(&format!(
                    "count(): Argument #1 ($value) must be of type Countable|array, {} given",
                    cls.name.to_string_lossy()
                ))
            }
            Value::Ref(r) => {
                let v = r.get();
                self.count_value(&v, recursive)
            }
            other => self.type_error(&format!(
                "count(): Argument #1 ($value) must be of type Countable|array, {} given",
                other.type_name_for_error()
            )),
        }
    }
}
