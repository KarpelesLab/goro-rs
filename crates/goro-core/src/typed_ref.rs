//! Typed references (ZEND_REF_HAS_TYPE_SOURCES).
//!
//! A PHP reference that is held by a typed property remembers that property
//! as one of its "type sources"; every later write through the reference must
//! satisfy (and coerce identically for) all of them. A `Value::Reference` is a
//! plain `Rc<RefCell<Value>>`, so the sources live in a side table keyed by
//! the reference cell's address. The table holds a `Weak` to the cell, which
//! keeps the allocation (and so the address) from being reused while the
//! entry exists.
//!
//! Instance-property sources are validated lazily: a source is live while its
//! owner object is alive and the owner's property slot still holds this very
//! reference. This stands in for ZEND_REF_DEL_TYPE_SOURCE on unset, re-binding
//! and object destruction. Static-property sources have no owner object and
//! are removed explicitly (`typed_ref_unregister`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use super::Vm;
use super::variance::{
    T_ARRAY, T_BOOL, T_DOUBLE, T_FALSE, T_LONG, T_NULL, T_STRING, T_TRUE, TypeView,
};
use crate::object::PhpObject;
use crate::opcode::{OpCode, ParamType};
use crate::value::Value;

/// One typed property holding a reference.
#[derive(Clone)]
pub(crate) struct RefSource {
    /// The object owning the property (dangling for a static property).
    pub owner: Weak<RefCell<PhpObject>>,
    /// A static property: live until unregistered.
    pub is_static: bool,
    /// Lowercase declaring class.
    pub decl: Vec<u8>,
    /// The property's storage key on the owner.
    pub slot: Vec<u8>,
    /// Declaring class (display form) and unmangled property name.
    pub class: String,
    pub prop: String,
    pub ty: ParamType,
}

struct Entry {
    cell: Weak<RefCell<Value>>,
    sources: Vec<RefSource>,
}

thread_local! {
    static TABLE: RefCell<HashMap<usize, Entry>> = RefCell::new(HashMap::new());
    static COUNT: Cell<usize> = const { Cell::new(0) };
    static SWEEP_AT: Cell<usize> = const { Cell::new(64) };
}

#[inline]
fn key(r: &Rc<RefCell<Value>>) -> usize {
    Rc::as_ptr(r) as *const () as usize
}

/// Whether any typed reference exists at all (fast path for writers).
#[inline]
pub(crate) fn any() -> bool {
    COUNT.with(|c| c.get()) != 0
}

fn source_live(r: &Rc<RefCell<Value>>, s: &RefSource) -> bool {
    if s.is_static {
        return true;
    }
    let Some(owner) = s.owner.upgrade() else {
        return false;
    };
    let Ok(ob) = owner.try_borrow() else {
        // Being modified right now: assume the slot still holds it.
        return true;
    };
    ob.properties
        .iter()
        .any(|(k, v)| k == &s.slot && matches!(v, Value::Reference(x) if Rc::ptr_eq(x, r)))
}

/// Record `src` as a type source of `r`.
pub(crate) fn add_source(r: &Rc<RefCell<Value>>, src: RefSource) {
    TABLE.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() >= SWEEP_AT.with(|c| c.get()) {
            t.retain(|_, e| e.cell.strong_count() > 0);
            let n = t.len();
            SWEEP_AT.with(|c| c.set((n * 2).max(64)));
        }
        let e = t.entry(key(r)).or_insert_with(|| Entry {
            cell: Rc::downgrade(r),
            sources: Vec::new(),
        });
        if e.cell.strong_count() == 0 {
            e.cell = Rc::downgrade(r);
            e.sources.clear();
        }
        let dup = e.sources.iter().any(|s| {
            if src.is_static {
                s.is_static && s.decl == src.decl && s.prop == src.prop
            } else {
                !s.is_static && s.owner.ptr_eq(&src.owner) && s.slot == src.slot
            }
        });
        if !dup {
            e.sources.push(src);
        }
        COUNT.with(|c| c.set(t.len()));
    });
}

/// The static property `decl::$prop` (lowercase class) no longer holds `r`.
pub(crate) fn remove_static_source(r: &Rc<RefCell<Value>>, decl: &[u8], prop: &[u8]) {
    if !any() {
        return;
    }
    let k = key(r);
    TABLE.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(e) = t.get_mut(&k) {
            e.sources
                .retain(|s| !(s.is_static && s.decl == decl && s.prop.as_bytes() == prop));
            if e.sources.is_empty() {
                t.remove(&k);
            }
        }
        COUNT.with(|c| c.set(t.len()));
    });
}

/// The live type sources of `r` (empty for an untyped reference).
pub(crate) fn sources(r: &Rc<RefCell<Value>>) -> Vec<RefSource> {
    if !any() {
        return Vec::new();
    }
    let k = key(r);
    let raw: Vec<RefSource> = TABLE.with(|t| match t.borrow().get(&k) {
        Some(e) if e.cell.strong_count() > 0 => e.sources.clone(),
        _ => Vec::new(),
    });
    if raw.is_empty() {
        return raw;
    }
    let live: Vec<RefSource> = raw.iter().filter(|s| source_live(r, s)).cloned().collect();
    if live.len() != raw.len() {
        TABLE.with(|t| {
            let mut t = t.borrow_mut();
            if live.is_empty() {
                t.remove(&k);
            } else if let Some(e) = t.get_mut(&k) {
                e.sources.retain(|s| source_live(r, s));
            }
            COUNT.with(|c| c.set(t.len()));
        });
    }
    live
}

/// Is `r` held by at least one typed property?
#[inline]
pub(crate) fn is_typed(r: &Rc<RefCell<Value>>) -> bool {
    any() && !sources(r).is_empty()
}

fn identical_scalar(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Long(x), Value::Long(y)) => x == y,
        (Value::Double(x), Value::Double(y)) => x.to_bits() == y.to_bits() || x == y,
        (Value::String(x), Value::String(y)) => x.as_bytes() == y.as_bytes(),
        (Value::True, Value::True) | (Value::False, Value::False) => true,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

impl Vm {
    /// ZEND_TRY_ASSIGN_REF_*() of an internal function: store `value`
    /// through the by-reference argument `r`, coerced to the typed
    /// properties the reference is bound to (in the caller's strict_types
    /// mode). On a type mismatch the TypeError is thrown (left in
    /// current_exception) and the reference is unchanged: Err(()).
    #[allow(clippy::result_unit_err)]
    pub fn try_assign_ref(&mut self, r: &Rc<RefCell<Value>>, value: Value) -> Result<(), ()> {
        let mut value = value;
        if is_typed(r) {
            let strict = self.caller_strict_types;
            if let Err(msg) = self.typed_ref_verify(r, &mut value, strict) {
                let line = self.current_line;
                let e = self.create_exception(b"TypeError", &msg, line);
                self.current_exception = Some(e);
                return Err(());
            }
            *r.borrow_mut() = value;
            return Ok(());
        }
        // zval_ptr_safe_dtor(): the reference reads NULL while the old
        // value's destructor runs.
        let old = std::mem::replace(&mut *r.borrow_mut(), Value::Null);
        let releases = matches!(old, Value::Object(_) | Value::Array(_));
        drop(old);
        if releases && self.has_release_work() {
            self.run_pending_destructors_pub();
        }
        *r.borrow_mut() = value;
        Ok(())
    }

    /// The type source a typed property slot contributes (None: untyped).
    pub(crate) fn typed_ref_source_for(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        slot: &[u8],
    ) -> Option<RefSource> {
        let class_lower: Vec<u8> = obj
            .borrow()
            .class_name
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        // A mangled private slot (`\0Class\0name`) names its declaring class.
        let (lookup_class, name): (Vec<u8>, &[u8]) = if slot.first() == Some(&0) {
            let rest = &slot[1..];
            let p = rest.iter().position(|&b| b == 0)?;
            (rest[..p].to_ascii_lowercase(), &rest[p + 1..])
        } else {
            (class_lower, slot)
        };
        let (_, declaring, _, ty, _) = self.find_property_def(&lookup_class, name)?;
        let ty = ty?;
        let decl_lower = declaring.to_ascii_lowercase();
        let class = self
            .class_name_for_display(&decl_lower)
            .map(|n| crate::value::display_class_name(&n))
            .unwrap_or_else(|| crate::value::display_class_name(&declaring));
        Some(RefSource {
            owner: Rc::downgrade(obj),
            is_static: false,
            decl: decl_lower,
            slot: slot.to_vec(),
            class,
            prop: String::from_utf8_lossy(name).into_owned(),
            ty,
        })
    }

    /// Register the typed property `slot` of `obj` as a source of `r`.
    pub(crate) fn typed_ref_register_slot(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        slot: &[u8],
        r: &Rc<RefCell<Value>>,
    ) {
        if let Some(src) = self.typed_ref_source_for(obj, slot) {
            add_source(r, src);
        }
    }

    /// One source's verdict (i_zend_verify_type_assignable_zval followed by
    /// the weak coercion): Ok(None) accepted as is, Ok(Some(v)) accepted
    /// after coercion to v, Err(()) rejected.
    pub(crate) fn typed_ref_check_one(
        &self,
        ty: &ParamType,
        value: &Value,
        strict: bool,
    ) -> Result<Option<Value>, ()> {
        let tv = TypeView::from_param_type(ty);
        let mask = tv.mask;
        let exact = match value {
            Value::Null | Value::Undef => mask & T_NULL != 0,
            Value::Long(_) => mask & T_LONG != 0,
            Value::Double(_) => mask & T_DOUBLE != 0,
            Value::String(_) => mask & T_STRING != 0,
            Value::Array(_) => mask & T_ARRAY != 0,
            Value::True => mask & T_TRUE != 0,
            Value::False => mask & T_FALSE != 0,
            _ => false,
        } || (matches!(
            value,
            Value::Object(_) | Value::Generator(_) | Value::Resource(_)
        ) && self.value_matches_type_mode(value, ty, true));
        if exact {
            return Ok(None);
        }
        let scalar_target = mask & (T_LONG | T_DOUBLE | T_STRING) != 0 || mask & T_BOOL == T_BOOL;
        let may_coerce = if strict {
            mask & T_DOUBLE != 0 && matches!(value, Value::Long(_))
        } else {
            !matches!(value, Value::Null | Value::Undef) && scalar_target
        };
        if !may_coerce {
            return Err(());
        }
        if matches!(value, Value::Object(_)) {
            return Err(());
        }
        let mut c = value.clone();
        Self::coerce_value_to_type(&mut c, ty, strict);
        if self.value_matches_type_mode(&c, ty, true) && !identical_scalar(&c, value) {
            Ok(Some(c))
        } else if self.value_matches_type_mode(&c, ty, true) {
            Ok(None)
        } else {
            Err(())
        }
    }

    /// zend_verify_ref_assignable_zval: check (and coerce) `value` for a write
    /// through the typed reference `r`. Returns the TypeError message on
    /// failure; `value` is left coerced on success.
    pub(crate) fn typed_ref_verify(
        &self,
        r: &Rc<RefCell<Value>>,
        value: &mut Value,
        strict: bool,
    ) -> Result<(), String> {
        let srcs = sources(r);
        if srcs.is_empty() {
            return Ok(());
        }
        self.typed_ref_verify_sources(&srcs, value, strict)
    }

    pub(crate) fn typed_ref_verify_sources(
        &self,
        srcs: &[RefSource],
        value: &mut Value,
        strict: bool,
    ) -> Result<(), String> {
        let v = value.deref();
        let mut first: Option<&RefSource> = None;
        let mut coerced: Option<Value> = None;
        for s in srcs {
            match self.typed_ref_check_one(&s.ty, &v, strict) {
                Err(()) => {
                    return Err(format!(
                        "Cannot assign {} to reference held by property {}::${} of type {}",
                        Self::value_type_name(&v),
                        s.class,
                        s.prop,
                        self.param_type_name(&s.ty)
                    ));
                }
                Ok(Some(c)) => match (&first, &coerced) {
                    (None, _) => {
                        first = Some(s);
                        coerced = Some(c);
                    }
                    (Some(f), None) => return Err(self.typed_ref_conflict(f, s, &v)),
                    (Some(f), Some(prev)) => {
                        if !identical_scalar(prev, &c) {
                            return Err(self.typed_ref_conflict(f, s, &v));
                        }
                    }
                },
                Ok(None) => match (&first, &coerced) {
                    (None, _) => first = Some(s),
                    (Some(f), Some(_)) => return Err(self.typed_ref_conflict(f, s, &v)),
                    _ => {}
                },
            }
        }
        if let Some(c) = coerced {
            *value = c;
        }
        Ok(())
    }

    /// zend_verify_prop_assignable_by_ref: may the reference `r` be bound to
    /// the typed property described by `dst`? Coerces the referenced value
    /// when `r` is not typed yet. Returns the TypeError message on failure.
    pub(crate) fn typed_ref_verify_bind(
        &self,
        dst: &RefSource,
        r: &Rc<RefCell<Value>>,
        strict: bool,
    ) -> Result<(), String> {
        let cur = r.borrow().clone();
        let existing = sources(r)
            .into_iter()
            .filter(|s| !(s.owner.ptr_eq(&dst.owner) && s.slot == dst.slot))
            .collect::<Vec<_>>();
        let cannot = || {
            format!(
                "Cannot assign {} to property {}::${} of type {}",
                Self::value_type_name(&cur),
                dst.class,
                dst.prop,
                self.param_type_name(&dst.ty)
            )
        };
        match self.typed_ref_check_one(&dst.ty, &cur, strict) {
            Ok(None) => Ok(()),
            Ok(Some(c)) => {
                if let Some(first) = existing.first() {
                    let tn = match &cur {
                        Value::True | Value::False => "bool".to_string(),
                        v => Self::value_type_name(v),
                    };
                    Err(format!(
                        "Reference with value of type {} held by property {}::${} of type {} is not compatible with property {}::${} of type {}",
                        tn,
                        first.class,
                        first.prop,
                        self.param_type_name(&first.ty),
                        dst.class,
                        dst.prop,
                        self.param_type_name(&dst.ty)
                    ))
                } else {
                    *r.borrow_mut() = c;
                    Ok(())
                }
            }
            Err(()) => Err(cannot()),
        }
    }

    /// Finish a read-modify-write (compound assignment or inc/dec) whose
    /// result `new` was computed from `old`, the value of the typed reference
    /// `r` (zend_binary_assign_op_typed_ref / zend_incdec_typed_ref). `r`
    /// still holds `old`; it is updated unless the result is rejected.
    pub(crate) fn typed_ref_finish_rmw(
        &self,
        r: &Rc<RefCell<Value>>,
        old: Value,
        new: Value,
        opc: OpCode,
        strict: bool,
    ) -> Result<(), String> {
        let mut new = match new {
            Value::Reference(x) => x.borrow().clone(),
            v => v,
        };
        let inc = matches!(opc, OpCode::PreIncrement | OpCode::PostIncrement);
        let dec = matches!(opc, OpCode::PreDecrement | OpCode::PostDecrement);
        if (inc || dec) && matches!(new, Value::Double(_)) && matches!(old, Value::Long(_)) {
            let srcs = sources(r);
            if let Some(s) = srcs
                .iter()
                .find(|s| TypeView::from_param_type(&s.ty).mask & T_DOUBLE == 0)
            {
                *r.borrow_mut() = Value::Long(if inc { i64::MAX } else { i64::MIN });
                return Err(format!(
                    "Cannot {} a reference held by property {}::${} of type {} past its {} value",
                    if inc { "increment" } else { "decrement" },
                    s.class,
                    s.prop,
                    self.param_type_name(&s.ty),
                    if inc { "maximal" } else { "minimal" }
                ));
            }
            *r.borrow_mut() = new;
            return Ok(());
        }
        self.typed_ref_verify(r, &mut new, strict)?;
        *r.borrow_mut() = new;
        Ok(())
    }

    /// A write of `value` into the property slot `slot` of `obj`: when the
    /// slot holds a typed reference, the value must satisfy its sources.
    /// `incdec` is Some(is_increment) when `value` is the result of a ++/--
    /// that overflowed the int range.
    pub(crate) fn typed_ref_slot_write_check(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        slot: &[u8],
        value: &mut Value,
        strict: bool,
        incdec: Option<bool>,
    ) -> Result<(), String> {
        let r = match obj.borrow().properties.iter().find(|(k, _)| k == slot) {
            Some((_, Value::Reference(r))) => r.clone(),
            _ => return Ok(()),
        };
        self.typed_ref_write_check(&r, value, strict, incdec)
    }

    /// A write of `value` through the reference `r` (see
    /// `typed_ref_slot_write_check` for `incdec`).
    pub(crate) fn typed_ref_write_check(
        &self,
        r: &Rc<RefCell<Value>>,
        value: &mut Value,
        strict: bool,
        incdec: Option<bool>,
    ) -> Result<(), String> {
        let srcs = sources(r);
        if srcs.is_empty() {
            return Ok(());
        }
        if let Some(inc) = incdec {
            if let Some(s) = srcs
                .iter()
                .find(|s| TypeView::from_param_type(&s.ty).mask & T_DOUBLE == 0)
            {
                return Err(format!(
                    "Cannot {} a reference held by property {}::${} of type {} past its {} value",
                    if inc { "increment" } else { "decrement" },
                    s.class,
                    s.prop,
                    self.param_type_name(&s.ty),
                    if inc { "maximal" } else { "minimal" }
                ));
            }
            return Ok(());
        }
        self.typed_ref_verify_sources(&srcs, value, strict)
    }

    /// zend_verify_ref_array_assignable: may the null held by the typed
    /// reference in `slot` (a CV) be auto-initialized to an array?
    pub(crate) fn typed_ref_auto_init_check(&self, slot: Option<&Value>) -> Result<(), String> {
        let Some(Value::Reference(r)) = slot else {
            return Ok(());
        };
        if !any() {
            return Ok(());
        }
        for s in sources(r) {
            if !super::param_type_accepts_array(&s.ty) {
                return Err(format!(
                    "Cannot auto-initialize an array inside a reference held by property {}::${} of type {}",
                    s.class,
                    s.prop,
                    self.param_type_name(&s.ty)
                ));
            }
        }
        Ok(())
    }

    fn typed_ref_conflict(&self, a: &RefSource, b: &RefSource, v: &Value) -> String {
        format!(
            "Cannot assign {} to reference held by property {}::${} of type {} and property {}::${} of type {}, as this would result in an inconsistent type conversion",
            Self::value_type_name(v),
            a.class,
            a.prop,
            self.param_type_name(&a.ty),
            b.class,
            b.prop,
            self.param_type_name(&b.ty)
        )
    }

    /// zend_std_read_property after __get() for an unset typed property:
    /// the result must satisfy (and is coerced to) the property's type, using
    /// the strictness of __get()'s file. A by-reference result is coerced in
    /// place.
    pub(crate) fn magic_get_typed_check(
        &self,
        class_lower: &[u8],
        prop: &[u8],
        result: &mut Value,
        strict: bool,
    ) -> Result<(), String> {
        let Some((_, declaring, _, Some(ty), _)) = self.find_property_def(class_lower, prop) else {
            return Ok(());
        };
        let verdict = match &*result {
            Value::Reference(r) if !sources(r).is_empty() => {
                // A typed reference must already be exactly assignable.
                let inner = r.borrow().clone();
                match self.typed_ref_check_one(&ty, &inner, strict) {
                    Ok(None) => Ok(None),
                    _ => Err(()),
                }
            }
            v => self.typed_ref_check_one(&ty, &v.deref(), strict),
        };
        match verdict {
            Ok(None) => Ok(()),
            Ok(Some(c)) => {
                match result {
                    Value::Reference(r) => *r.borrow_mut() = c,
                    v => *v = c,
                }
                Ok(())
            }
            Err(()) => {
                let v = result.deref();
                let tn = match &v {
                    Value::True | Value::False => "bool".to_string(),
                    other => Self::value_type_name(other),
                };
                let decl_lower = declaring.to_ascii_lowercase();
                let class = self
                    .class_name_for_display(&decl_lower)
                    .map(|n| crate::value::display_class_name(&n))
                    .unwrap_or_else(|| crate::value::display_class_name(&declaring));
                Err(format!(
                    "Value of type {} returned from {}::__get() must be compatible with unset property {}::${} of type {}",
                    tn,
                    class,
                    class,
                    crate::string::msg_str(prop),
                    self.param_type_name(&ty)
                ))
            }
        }
    }
}

/// Static-property sources and the generic check / post-op entry points used
/// by the static property, array element and CV write paths.
impl Vm {
    fn typed_ref_static_source(&self, decl: &[u8], prop: &[u8], ty: ParamType) -> RefSource {
        let decl = decl.to_ascii_lowercase();
        let class = self
            .classes
            .get(&decl)
            .map(|c| crate::value::display_class_name(&c.name))
            .unwrap_or_else(|| String::from_utf8_lossy(&decl).into_owned());
        RefSource {
            owner: Weak::new(),
            is_static: true,
            decl,
            slot: prop.to_vec(),
            class,
            prop: String::from_utf8_lossy(prop).into_owned(),
            ty,
        }
    }

    /// Record that reference `r` is bound to the typed static property
    /// `decl::$prop`.
    pub(crate) fn typed_ref_register(
        &mut self,
        r: &Rc<RefCell<Value>>,
        decl: &[u8],
        prop: &[u8],
        ty: ParamType,
    ) {
        let src = self.typed_ref_static_source(decl, prop, ty);
        add_source(r, src);
    }

    /// The static property `decl::$prop` no longer holds reference `r`
    /// (instance properties are tracked through their slots).
    pub(crate) fn typed_ref_unregister(
        &mut self,
        r: &Rc<RefCell<Value>>,
        decl: &[u8],
        prop: &[u8],
    ) {
        remove_static_source(r, &decl.to_ascii_lowercase(), prop);
    }

    /// A value assigned through reference `r`: coerced and checked against
    /// every typed property the reference is bound to, except `skip_prop`
    /// (the property being assigned, which checked its own type).
    pub(crate) fn typed_ref_check(
        &mut self,
        r: &Rc<RefCell<Value>>,
        value: &mut Value,
        strict: bool,
        skip_prop: Option<&[u8]>,
    ) -> Result<(), String> {
        let srcs: Vec<RefSource> = sources(r)
            .into_iter()
            .filter(|s| skip_prop != Some(s.prop.as_bytes()))
            .collect();
        if srcs.is_empty() {
            return Ok(());
        }
        if let Value::Reference(inner) = value {
            let v = inner.borrow().clone();
            *value = v;
        }
        // zend_verify_weak_scalar_type_hint(): a Stringable object fits a
        // string type through __toString() (unless some type takes it).
        if !strict && matches!(value, Value::Object(_)) {
            let takes_object = srcs
                .iter()
                .any(|s| self.value_matches_type_mode(value, &s.ty, true));
            let wants_string = srcs
                .iter()
                .any(|s| TypeView::from_param_type(&s.ty).mask & T_STRING != 0);
            let stringable = match &*value {
                Value::Object(o) => {
                    let cl = o.borrow().class_name.to_ascii_lowercase();
                    self.classes
                        .get(&cl)
                        .is_some_and(|c| c.get_method(b"__tostring").is_some())
                }
                _ => false,
            };
            if !takes_object && wants_string && stringable {
                let sv = self.value_to_string(value);
                if self.current_exception.is_some() {
                    return Ok(());
                }
                *value = Value::String(sv);
            }
        }
        self.typed_ref_verify_sources(&srcs, value, strict)
    }

    /// Binding reference `r` to the typed static property `decl::$prop` of
    /// type `ty`: a reference already bound to other typed properties must
    /// hold a value fitting `ty` as is.
    pub(crate) fn typed_ref_bind_error(
        &mut self,
        r: &Rc<RefCell<Value>>,
        decl: &[u8],
        prop: &[u8],
        ty: &ParamType,
    ) -> Option<String> {
        let dst = self.typed_ref_static_source(decl, prop, ty.clone());
        let existing: Vec<RefSource> = sources(r)
            .into_iter()
            .filter(|s| !(s.is_static && s.decl == dst.decl && s.prop == dst.prop))
            .collect();
        let first = existing.first()?;
        let cur = r.borrow().clone();
        match self.typed_ref_check_one(ty, &cur, false) {
            Ok(None) => None,
            Ok(Some(_)) => {
                let tn = match &cur {
                    Value::True | Value::False => "bool".to_string(),
                    v => Self::value_type_name(v),
                };
                Some(format!(
                    "Reference with value of type {} held by property {}::${} of type {} is not compatible with property {}::${} of type {}",
                    tn,
                    first.class,
                    first.prop,
                    self.param_type_name(&first.ty),
                    dst.class,
                    dst.prop,
                    self.param_type_name(ty)
                ))
            }
            Err(()) => Some(format!(
                "Cannot assign {} to property {}::${} of type {}",
                Self::value_type_name(&cur),
                dst.class,
                dst.prop,
                self.param_type_name(ty)
            )),
        }
    }

    /// Before a compound assignment / increment of a CV: the reference it
    /// holds when that reference is bound to typed properties, with its
    /// current value (see typed_ref_after_op).
    pub(crate) fn typed_ref_watch(&self, v: Option<&Value>) -> Option<(Rc<RefCell<Value>>, Value)> {
        let Some(Value::Reference(r)) = v else {
            return None;
        };
        if !is_typed(r) {
            return None;
        }
        Some((r.clone(), r.borrow().clone()))
    }

    /// After the op: verify the new value against the reference's typed
    /// properties, restoring the old value on failure (zend_incdec_typed_ref /
    /// zend_binary_assign_op_typed_ref). `incdec`: Some(true) for ++,
    /// Some(false) for --. Raises the TypeError for the post-op exception
    /// dispatch.
    pub(crate) fn typed_ref_after_op(
        &mut self,
        r: &Rc<RefCell<Value>>,
        old: Value,
        incdec: Option<bool>,
        strict: bool,
        line: u32,
    ) {
        if self.current_exception.is_some() {
            // The op itself failed: the reference keeps what it had.
            return;
        }
        let opc = match incdec {
            Some(true) => OpCode::PreIncrement,
            Some(false) => OpCode::PreDecrement,
            None => OpCode::AssignAdd,
        };
        let new = r.borrow().clone();
        *r.borrow_mut() = old.clone();
        if let Err(msg) = self.typed_ref_finish_rmw(r, old, new, opc, strict) {
            let exc = self.create_exception(b"TypeError", &msg, line);
            self.current_exception = Some(exc);
            self.error_handler_threw = true;
        }
    }
}

/// "Cannot assign float to reference held by P" for an int overflowing
/// ++/-- becomes "Cannot increment a reference held by P past its maximal
/// value".
pub(crate) fn incdec_msg(msg: &str, inc: bool) -> String {
    let rest = msg
        .split_once(" to reference held by ")
        .map(|(_, r)| r)
        .unwrap_or("");
    format!(
        "Cannot {} a reference held by {} past its {} value",
        if inc { "increment" } else { "decrement" },
        rest,
        if inc { "maximal" } else { "minimal" }
    )
}

/// What the UNDEF result of a failed compound assignment becomes in a slot
/// of type `ty`: zend_binary_op() leaves an UNDEF copy that
/// zend_verify_property_type() coerces like a weak scalar (int, float,
/// string, then bool are tried in turn); None when it is rejected (strict
/// mode, no scalar type), which leaves the slot as it was.
pub(crate) fn failed_op_value(ty: &ParamType, strict: bool) -> Option<Value> {
    if strict {
        return None;
    }
    let mask = TypeView::from_param_type(ty).mask;
    if mask & T_LONG != 0 {
        Some(Value::Long(0))
    } else if mask & T_DOUBLE != 0 {
        Some(Value::Double(0.0))
    } else if mask & T_STRING != 0 {
        Some(Value::String(crate::string::PhpString::empty()))
    } else if mask & T_BOOL == T_BOOL {
        Some(Value::False)
    } else {
        None
    }
}

impl Vm {
    /// A failed compound assignment through the typed reference `r`
    /// (zend_binary_assign_op_typed_ref): the coerced UNDEF result is
    /// stored when every type source accepts it alike.
    fn typed_ref_failed_op(&self, r: &Rc<RefCell<Value>>, strict: bool) {
        let mut out: Option<Value> = None;
        for s in sources(r) {
            let Some(v) = failed_op_value(&s.ty, strict) else {
                return;
            };
            match &out {
                Some(prev) if !identical_scalar(prev, &v) => return,
                _ => out = Some(v),
            }
        }
        if let Some(v) = out {
            *r.borrow_mut() = v;
        }
    }

    /// The binary operation (or CV compound assignment) `op` threw. When it
    /// is the middle of a compound assignment on a typed slot, the slot
    /// takes the coerced UNDEF result like php-src's in-place assign-op
    /// handlers leave it (zend_binary_assign_op_typed_prop / _typed_ref);
    /// an undefined variable operand is left NULL (BP_VAR_RW fetch).
    /// Record that the binary / assign-op opcode `op` threw: the catch
    /// block (or the frame's exit) it lands in runs compound_op_failed().
    #[cold]
    #[inline(never)]
    pub(crate) fn compound_fail_note(&mut self, op: &crate::opcode::Op) {
        self.compound_fail_at = op as *const crate::opcode::Op as usize;
        self.compound_fail_exc = match &self.current_exception {
            Some(Value::Object(o)) => Rc::as_ptr(o) as usize,
            _ => 0,
        };
    }

    /// compound_op_failed() for the op recorded in `compound_fail_at`,
    /// when it is one of `op_array`'s.
    #[inline(never)]
    pub(crate) fn compound_op_failed_at(
        &mut self,
        op_array: &crate::opcode::OpArray,
        cvs: &mut [Value],
        tmps: &mut [Value],
        static_cv_keys: &crate::fxhash::FxHashMap<u32, Vec<u8>>,
    ) {
        let at = std::mem::take(&mut self.compound_fail_at);
        let exc = std::mem::take(&mut self.compound_fail_exc);
        // (only for the exception that op threw)
        let cur = match &self.current_exception {
            Some(Value::Object(o)) => Rc::as_ptr(o) as usize,
            _ => 0,
        };
        if exc != cur {
            return;
        }
        let sz = std::mem::size_of::<crate::opcode::Op>().max(1);
        let base = op_array.ops.as_ptr() as usize;
        if at < base || (at - base) % sz != 0 {
            return;
        }
        if let Some(op) = op_array.ops.get((at - base) / sz) {
            self.compound_op_failed(op, op_array, cvs, tmps, static_cv_keys);
        }
    }

    #[inline(never)]
    pub(crate) fn compound_op_failed(
        &mut self,
        op: &crate::opcode::Op,
        op_array: &crate::opcode::OpArray,
        cvs: &mut [Value],
        tmps: &mut [Value],
        static_cv_keys: &crate::fxhash::FxHashMap<u32, Vec<u8>>,
    ) {
        use crate::opcode::OperandType;
        let strict = op_array.strict_types;
        let ops = &op_array.ops;
        let sz = std::mem::size_of::<crate::opcode::Op>().max(1);
        let idx =
            (op as *const crate::opcode::Op as usize).wrapping_sub(ops.as_ptr() as usize) / sz;
        if !ops.get(idx).is_some_and(|o| std::ptr::eq(o, op)) {
            return;
        }
        match op.opcode {
            OpCode::AssignAdd
            | OpCode::AssignSub
            | OpCode::AssignMul
            | OpCode::AssignDiv
            | OpCode::AssignMod
            | OpCode::AssignPow
            | OpCode::AssignConcat
            | OpCode::AssignBitwiseAnd
            | OpCode::AssignBitwiseOr
            | OpCode::AssignBitwiseXor
            | OpCode::AssignShiftLeft
            | OpCode::AssignShiftRight => {
                let OperandType::Cv(ci) = op.op1 else {
                    return;
                };
                match cvs.get(ci as usize) {
                    Some(Value::Undef) => {
                        self.write_operand(&op.op1, Value::Null, cvs, tmps, static_cv_keys)
                    }
                    Some(Value::Reference(r)) => {
                        let r = r.clone();
                        if matches!(*r.borrow(), Value::Undef) {
                            *r.borrow_mut() = Value::Null;
                        } else if is_typed(&r)
                            && !(op.opcode == OpCode::AssignConcat
                                && matches!(*r.borrow(), Value::String(_)))
                        {
                            // (a string is concatenated in place)
                            self.typed_ref_failed_op(&r, strict);
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                let (Some(prev), Some(next)) = (
                    idx.checked_sub(1).and_then(|i| ops.get(i)),
                    ops.get(idx + 1),
                ) else {
                    return;
                };
                if !matches!(op.result, OperandType::Tmp(_))
                    || prev.result != op.op1
                    || next.op2 != op.result
                {
                    return;
                }
                // A string is concatenated in place: it keeps its value.
                if op.opcode == OpCode::Concat
                    && matches!(
                        self.read_operand(&op.op1, cvs, tmps, &op_array.literals)
                            .deref(),
                        Value::String(_)
                    )
                {
                    return;
                }
                let lits = &op_array.literals;
                match (prev.opcode, next.opcode) {
                    (OpCode::PropertyGetRW, OpCode::PropertySet) => {
                        let Value::Object(o) =
                            self.read_operand(&next.op1, cvs, tmps, lits).deref()
                        else {
                            return;
                        };
                        let name = self
                            .read_operand(&next.result, cvs, tmps, lits)
                            .to_php_string();
                        let key = self
                            .prop_access_key(&o, name.as_bytes())
                            .unwrap_or_else(|| name.as_bytes().to_vec());
                        let cur = o
                            .borrow()
                            .properties
                            .iter()
                            .find(|(k, _)| *k == key)
                            .map(|(_, v)| v.clone());
                        match cur {
                            Some(Value::Reference(r)) if is_typed(&r) => {
                                self.typed_ref_failed_op(&r, strict)
                            }
                            Some(_) => {
                                if let Some(v) = self
                                    .typed_ref_source_for(&o, &key)
                                    .and_then(|s| failed_op_value(&s.ty, strict))
                                {
                                    o.borrow_mut().set_property_slice(&key, v);
                                }
                            }
                            None => {}
                        }
                    }
                    (OpCode::StaticPropFetchRW, OpCode::StaticPropSet) => {
                        let class_v = self.read_operand(&next.op1, cvs, tmps, lits);
                        let name = self
                            .read_operand(&next.result, cvs, tmps, lits)
                            .to_php_string();
                        self.static_prop_failed_op(&class_v, name.as_bytes(), strict);
                    }
                    (OpCode::ArrayGetRW, OpCode::ArraySet) => {
                        let Value::Array(a) = self.read_operand(&next.op1, cvs, tmps, lits).deref()
                        else {
                            return;
                        };
                        let k = Vm::value_to_array_key(self.read_operand(
                            &next.result,
                            cvs,
                            tmps,
                            lits,
                        ));
                        let el = a.borrow().get(&k).cloned();
                        if let Some(Value::Reference(r)) = el {
                            if is_typed(&r) {
                                self.typed_ref_failed_op(&r, strict);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
