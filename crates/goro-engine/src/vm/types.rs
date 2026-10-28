//! Type declarations: checks and coercions (PHP manual: Type declarations,
//! "Coercive typing with union types"; observed on PHP 8.5).

use crate::class::{DisplayName, TypeName};
use goro_runtime::numeric::{Numericness, parse_numeric};
use goro_runtime::{PhpStr, Value};

use super::{R, Vm};
use crate::bytecode::{TypeHint, tmask};
use crate::class::Class;

/// Result of checking a value against a type.
pub enum Check {
    Ok,
    /// Coerced to a new value.
    Coerced(Value),
    Fail,
}

impl Vm {
    /// Checks `v` against `ty`; coerces scalars when `strict` is false.
    /// `internal` selects native-function rules (null is coerced with a
    /// deprecation). `self_class` resolves `self`/`static`/`parent`.
    pub fn check_type(
        &mut self,
        v: &Value,
        ty: &TypeHint,
        strict: bool,
        self_class: *const Class,
        static_class: *const Class,
    ) -> R<Check> {
        let m = ty.mask;
        if m & tmask::MIXED != 0 {
            return Ok(Check::Ok);
        }
        let v = match v {
            Value::Ref(r) => &r.get(),
            v => v,
        };
        // Exact matches.
        match v {
            Value::Null if m & tmask::NULL != 0 => return Ok(Check::Ok),
            Value::Bool(true) if m & tmask::TRUE != 0 => return Ok(Check::Ok),
            Value::Bool(false) if m & tmask::FALSE != 0 => return Ok(Check::Ok),
            Value::Int(_) if m & tmask::INT != 0 => return Ok(Check::Ok),
            Value::Float(_) if m & tmask::FLOAT != 0 => return Ok(Check::Ok),
            Value::Str(_) if m & tmask::STRING != 0 => return Ok(Check::Ok),
            Value::Array(_) if m & (tmask::ARRAY | tmask::ITERABLE) != 0 => return Ok(Check::Ok),
            Value::Resource(_) if m & tmask::RESOURCE != 0 => return Ok(Check::Ok),
            Value::Object(o) => {
                if m & tmask::OBJECT != 0 {
                    return Ok(Check::Ok);
                }
                let cls = self.class_of(o);
                if m & tmask::ITERABLE != 0 && self.is_traversable(&cls) {
                    return Ok(Check::Ok);
                }
                if m & tmask::STATIC != 0
                    && !static_class.is_null()
                    && cls.is_subclass_of(unsafe { &*static_class })
                {
                    return Ok(Check::Ok);
                }
                for term in &ty.classes {
                    let mut all = true;
                    for name in term {
                        if !self.instance_of_name(&cls, name, self_class)? {
                            all = false;
                            break;
                        }
                    }
                    if all {
                        return Ok(Check::Ok);
                    }
                }
                if m & tmask::CALLABLE != 0 && self.is_callable_value(v, false) {
                    return Ok(Check::Ok);
                }
            }
            _ => {}
        }
        if m & tmask::CALLABLE != 0
            && matches!(v, Value::Str(_) | Value::Array(_))
            && self.is_callable_value(v, false)
        {
            return Ok(Check::Ok);
        }
        // int → float is allowed even in strict mode.
        if let Value::Int(i) = v
            && m & tmask::FLOAT != 0
        {
            return Ok(Check::Coerced(Value::Float(*i as f64)));
        }
        if strict {
            return Ok(Check::Fail);
        }
        self.coerce_scalar(v, m)
    }

    /// Coercive-mode scalar juggling: int, then float, then string, then
    /// bool, as far as the union allows.
    fn coerce_scalar(&mut self, v: &Value, m: u32) -> R<Check> {
        let wants_int = m & tmask::INT != 0;
        let wants_float = m & tmask::FLOAT != 0;
        let wants_string = m & tmask::STRING != 0;
        let wants_bool = m & tmask::BOOL == tmask::BOOL;
        match v {
            Value::Null | Value::Array(_) | Value::Resource(_) => Ok(Check::Fail),
            Value::Object(o) => {
                if wants_string && self.class_of(o).magic.borrow().to_string.is_some() {
                    let s = self.obj_to_string(o)?;
                    return Ok(Check::Coerced(Value::Str(s)));
                }
                Ok(Check::Fail)
            }
            Value::Float(f) => {
                if wants_int {
                    if f.is_finite() && *f >= -9.2233720368547758e18 && *f < 9.2233720368547758e18 {
                        if f.fract() != 0.0 {
                            if wants_string && false {
                                // string preferred? (no: int first)
                            }
                            self.deprecated(&format!(
                                "Implicit conversion from float {} to int loses precision",
                                goro_runtime::convert::float_repr(*f)
                            ))?;
                        }
                        return Ok(Check::Coerced(Value::Int(*f as i64)));
                    }
                    if !wants_string && !wants_bool {
                        return Ok(Check::Fail);
                    }
                }
                if wants_string {
                    let s = goro_runtime::convert::float_to_string(*f, self.ini.precision);
                    return Ok(Check::Coerced(Value::Str(s)));
                }
                if wants_bool {
                    return Ok(Check::Coerced(Value::Bool(*f != 0.0)));
                }
                Ok(Check::Fail)
            }
            Value::Int(i) => {
                if wants_float {
                    return Ok(Check::Coerced(Value::Float(*i as f64)));
                }
                if wants_string {
                    return Ok(Check::Coerced(Value::Str(
                        goro_runtime::convert::int_to_string(*i),
                    )));
                }
                if wants_bool {
                    return Ok(Check::Coerced(Value::Bool(*i != 0)));
                }
                Ok(Check::Fail)
            }
            Value::Str(s) => {
                if wants_int || wants_float {
                    let p = parse_numeric(s);
                    if p.kind == Numericness::Numeric {
                        match p.num {
                            goro_runtime::Number::Int(n) => {
                                if wants_int {
                                    return Ok(Check::Coerced(Value::Int(n)));
                                }
                                return Ok(Check::Coerced(Value::Float(n as f64)));
                            }
                            goro_runtime::Number::Float(f) => {
                                if wants_float {
                                    return Ok(Check::Coerced(Value::Float(f)));
                                }
                                // int only: integral float strings convert;
                                // others with a deprecation.
                                if f.is_finite()
                                    && f >= -9.2233720368547758e18
                                    && f < 9.2233720368547758e18
                                {
                                    if f.fract() != 0.0 {
                                        self.deprecated(&format!(
                                            "Implicit conversion from float-string \"{}\" to int loses precision",
                                            s.to_string_lossy()
                                        ))?;
                                    }
                                    return Ok(Check::Coerced(Value::Int(f as i64)));
                                }
                                if !wants_bool {
                                    return Ok(Check::Fail);
                                }
                            }
                        }
                    }
                }
                if wants_bool {
                    return Ok(Check::Coerced(Value::Bool(
                        goro_runtime::convert::to_bool_quiet(v),
                    )));
                }
                Ok(Check::Fail)
            }
            Value::Bool(b) => {
                if wants_int {
                    return Ok(Check::Coerced(Value::Int(*b as i64)));
                }
                if wants_float {
                    return Ok(Check::Coerced(Value::Float(*b as i64 as f64)));
                }
                if wants_string {
                    return Ok(Check::Coerced(Value::Str(if *b {
                        PhpStr::from_byte(b'1')
                    } else {
                        PhpStr::empty()
                    })));
                }
                Ok(Check::Fail)
            }
            Value::Ref(r) => {
                let v = r.get();
                self.coerce_scalar(&v, m)
            }
        }
    }

    /// Whether a class matches a type name (with `self`/`parent`).
    pub fn instance_of_name(
        &mut self,
        cls: &Class,
        name: &PhpStr,
        self_class: *const Class,
    ) -> R<bool> {
        // Case-insensitive name compare against lowercase class names (no
        // allocation on this hot path: every class-typed parameter).
        let is = |l: &[u8]| l.len() == name.len() && name.iter().zip(l).all(|(a, b)| a.to_ascii_lowercase() == *b);
        if is(b"self") && !self_class.is_null() {
            return Ok(cls.is_subclass_of(unsafe { &*self_class }));
        }
        if is(b"parent") && !self_class.is_null() {
            if let Some(p) = unsafe { &*self_class }.parent.clone() {
                return Ok(cls.is_subclass_of(&p));
            }
            return Ok(false);
        }
        // Fast check by name along the chain (no autoload needed: if the
        // class isn't loaded, nothing is an instance of it).
        let mut c = Some(cls);
        while let Some(k) = c {
            if is(k.lname.as_bytes()) {
                return Ok(true);
            }
            c = k.parent.as_deref();
        }
        for i in cls.interfaces.borrow().iter() {
            if is(i.lname.as_bytes()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn is_traversable(&self, cls: &Class) -> bool {
        match &self.core.traversable {
            Some(t) => cls.is_subclass_of(t),
            None => false,
        }
    }

    /// The type of a value as PHP names it in argument type errors
    /// ("int", "string", "Foo", "null", and "true"/"false" for booleans:
    /// observed on PHP 8.5, "must be of type int, true given").
    pub fn type_name_given(v: &Value) -> String {
        match v.deref() {
            Value::Bool(true) => "true".to_string(),
            Value::Bool(false) => "false".to_string(),
            v => v.tname(),
        }
    }
}
