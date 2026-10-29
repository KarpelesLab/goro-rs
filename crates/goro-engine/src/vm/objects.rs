//! Objects: property access with visibility, magic methods and hooks;
//! static properties; class constants; `new`, `clone`, `instanceof`.
//!
//! PHP manual: Properties, Visibility, Overloading (magic methods),
//! Property hooks, Readonly properties, Class constants, Object cloning.

use crate::class::{DisplayName, TypeName};
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use super::dims::deref_ptr;
use super::types::Check;
use super::{R, Vm, pending_flags};
use crate::bytecode::*;
use crate::class::{Class, ConstState, PropInfo, cflags, mflags};

/// Outcome of looking up a declared property from a scope.
pub enum PropLookup {
    /// Accessible declared property.
    Found(Rc<PropInfo>),
    /// Declared but not visible from the scope.
    Hidden(Rc<PropInfo>),
    /// Not declared (dynamic property).
    Dynamic,
}

/// Inline cache flags for property instructions.
pub(crate) const IC_PLAIN: u32 = 1;
/// A typed property: values whose type is in the mask (bits 8..) are
/// stored without coercion.
pub(crate) const IC_TYPED: u32 = 2;

/// Whether `v` needs no coercion for a property typed with `mask`.
#[inline(always)]
pub(crate) fn exact_type(v: &Value, mask: u32) -> bool {
    match v {
        Value::Int(_) => mask & tmask::INT != 0,
        Value::Float(_) => mask & tmask::FLOAT != 0,
        Value::Str(_) => mask & tmask::STRING != 0,
        Value::Bool(true) => mask & tmask::TRUE != 0,
        Value::Bool(false) => mask & tmask::FALSE != 0,
        Value::Array(_) => mask & tmask::ARRAY != 0,
        Value::Null => mask & tmask::NULL != 0,
        _ => false,
    }
}

impl Vm {
    // ------------------------------------------------------------ sentinels

    /// Whether a slot value marks an uninitialized or unset property.
    #[inline]
    pub fn is_uninit(&self, v: &Value) -> bool {
        match v {
            Value::Resource(r) => goro_runtime::ResRef::ptr_eq(r, &self.uninit),
            Value::Ref(r) => PhpRef::ptr_eq(r, &self.unset_mark),
            _ => false,
        }
    }

    #[inline]
    pub fn is_unset_mark(&self, v: &Value) -> bool {
        matches!(v, Value::Ref(r) if PhpRef::ptr_eq(r, &self.unset_mark))
    }

    pub fn uninit_value(&self) -> Value {
        Value::Resource(self.uninit.clone())
    }

    pub fn unset_value(&self) -> Value {
        Value::Ref(self.unset_mark.clone())
    }

    // ------------------------------------------------------------ lookup

    /// Finds the declared property `name` of `cls` as seen from `scope`.
    pub fn lookup_prop(&self, cls: &Class, name: &[u8], scope: *const Class) -> PropLookup {
        let map = cls.prop_map.borrow();
        let Some(idxs) = map.get(&PhpStr::new(name)) else {
            return PropLookup::Dynamic;
        };
        let props = cls.props.borrow();
        // A private property of the calling scope wins.
        if !scope.is_null() {
            for &i in idxs {
                let p = &props[i as usize];
                if p.flags & mflags::PRIVATE != 0 && std::ptr::eq(p.class, scope) {
                    return PropLookup::Found(p.clone());
                }
            }
        }
        // Otherwise the most derived non-shadowed one.
        let p = &props[idxs[0] as usize];
        if p.flags & mflags::PRIVATE != 0 && !std::ptr::eq(p.class, scope) {
            // A private property of an ancestor isn't visible: it behaves
            // as if undeclared unless it belongs to the object's class.
            if !std::ptr::eq(p.class, cls) {
                // Look for a non-private candidate.
                for &i in &idxs[1..] {
                    let q = &props[i as usize];
                    if q.flags & mflags::PRIVATE == 0 {
                        return self.check_vis(q.clone(), scope);
                    }
                }
                return PropLookup::Dynamic;
            }
            return PropLookup::Hidden(p.clone());
        }
        self.check_vis(p.clone(), scope)
    }

    fn check_vis(&self, p: Rc<PropInfo>, scope: *const Class) -> PropLookup {
        if p.flags & mflags::PUBLIC != 0 {
            return PropLookup::Found(p);
        }
        if scope.is_null() {
            return PropLookup::Hidden(p);
        }
        if p.flags & mflags::PRIVATE != 0 {
            return if std::ptr::eq(p.class, scope) {
                PropLookup::Found(p)
            } else {
                PropLookup::Hidden(p)
            };
        }
        let sc = unsafe { &*scope };
        let dc = unsafe { &*p.class };
        if sc.is_subclass_of(dc) || dc.is_subclass_of(sc) || self.common_root(sc, dc, &p.name) {
            PropLookup::Found(p)
        } else {
            PropLookup::Hidden(p)
        }
    }

    /// Protected members are visible between classes sharing the root
    /// declaration.
    fn common_root(&self, a: &Class, b: &Class, name: &PhpStr) -> bool {
        // Root declaring class of the property in b's chain.
        let mut root: *const Class = b;
        let mut c = b.parent.as_deref();
        while let Some(p) = c {
            if p.prop_map.borrow().contains_key(name) {
                root = p;
            }
            c = p.parent.as_deref();
        }
        a.is_subclass_of(unsafe { &*root })
    }

    /// The slot of a declared property by name (any visibility, the
    /// object class's own first).
    pub fn find_prop_slot_any(&self, cls: &Class, name: &[u8]) -> Option<usize> {
        let map = cls.prop_map.borrow();
        let idxs = map.get(&PhpStr::new(name))?;
        let props = cls.props.borrow();
        Some(props[idxs[0] as usize].slot as usize)
    }

    fn vis_word(p: &PropInfo) -> &'static str {
        if p.flags & mflags::PRIVATE != 0 {
            "private"
        } else {
            "protected"
        }
    }

    fn prop_class_name(p: &PropInfo) -> String {
        unsafe { (*p.class).name.dname() }
    }

    // ------------------------------------------------------------ reads

    /// `FetchProp` on an inline cache hit (a plain property of a known
    /// class), without taking a handle to the object; false when the
    /// general path must run.
    #[inline(always)]
    fn fetch_prop_hit(&mut self, dst: Reg, obj: Reg, name: Reg, cache: u32) -> bool {
        if !is_const(name) {
            return false;
        }
        let c = self.code_ref().caches[cache as usize].get();
        if c.key == 0 || c.b & IC_PLAIN == 0 {
            return false;
        }
        let op: Option<*const ObjRef> = if obj == THIS {
            self.ex
                .frames
                .last()
                .and_then(|f| f.this.as_ref())
                .map(|o| o as *const ObjRef)
        } else {
            match self.peek(obj) {
                Some(Value::Object(o)) => Some(o as *const ObjRef),
                _ => None,
            }
        };
        // SAFETY: the frame or slot holding the handle is not touched
        // until the value is copied out.
        let Some(o) = op else { return false };
        if unsafe { (*o).class_addr() } != c.key {
            return false;
        }
        let v = unsafe { (*o).obj().slots.borrow()[c.a].clone() };
        if self.is_uninit(&v) {
            return false;
        }
        if obj != THIS {
            self.drop_tmp(obj);
        }
        self.put_hot(dst, v.into_deref());
        true
    }

    pub(crate) fn op_fetch_prop(
        &mut self,
        dst: Reg,
        obj: Reg,
        name: Reg,
        cache: u32,
        quiet: bool,
    ) -> R<()> {
        if self.fetch_prop_hit(dst, obj, name, cache) {
            return Ok(());
        }
        let o = if obj == THIS {
            match self.this_obj() {
                Some(o) => o,
                None => return self.throw("Error", "Using $this when not in object context"),
            }
        } else {
            match self.peek(obj) {
                Some(Value::Object(o)) => {
                    let o = o.clone();
                    self.drop_tmp(obj);
                    o
                }
                _ => {
                    let v = if quiet {
                        self.get_quiet(obj)
                    } else {
                        self.get(obj)?
                    };
                    match v {
                        Value::Object(o) => o,
                        other => {
                            let n = self.get(name)?;
                            if !quiet {
                                let t = match &other {
                                    Value::Bool(true) => "true".to_string(),
                                    Value::Bool(false) => "false".to_string(),
                                    v => v.tname(),
                                };
                                let n = self.to_php_str(&n)?;
                                self.warn(&format!(
                                    "Attempt to read property \"{}\" on {}",
                                    n.to_string_lossy(),
                                    t
                                ))?;
                            }
                            self.put(dst, Value::Null);
                            return Ok(());
                        }
                    }
                }
            }
        };
        // Inline cache: plain public property of a known class.
        if is_const(name) {
            let c = self.code_ref().caches[cache as usize].get();
            if c.key != 0 && c.key == self.class_ptr_of(&o) as usize && c.b & IC_PLAIN != 0 {
                let v = o.obj().slots.borrow()[c.a].clone();
                if !self.is_uninit(&v) {
                    self.put(dst, v.into_deref());
                    return Ok(());
                }
            }
        }
        let n = self.get(name)?;
        let n = match n {
            Value::Str(s) => s,
            other => self.to_php_str(&other)?,
        };
        let v = self.read_prop(&o, &n, quiet, is_const(name).then_some(cache))?;
        self.put(dst, v);
        Ok(())
    }

    /// `$o->name` for reading.
    pub fn read_prop(
        &mut self,
        o: &ObjRef,
        name: &PhpStr,
        quiet: bool,
        cache: Option<u32>,
    ) -> R<Value> {
        if o.is_lazy()
            && let Some(t) = self.lazy_route(o, name, super::lazy::LazyAccess::Read)?
        {
            return self.forward_guarded(o, &t, name, |vm| vm.read_prop(&t, name, quiet, None));
        }
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                if let Some(h) = &p.get
                    && !self.in_hook(o, &p)
                {
                    let h = h.clone();
                    return self.call_hook(o, &p, h, Vec::new()).map(|v| v.into_deref());
                }
                if p.flags & mflags::VIRTUAL != 0 && p.get.is_none() {
                    return self.throw(
                        "Error",
                        &format!(
                            "Property {}::${} is write-only",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                let v = o.obj().slots.borrow()[p.slot as usize].clone();
                if self.is_uninit(&v) {
                    let was_unset = self.is_unset_mark(&v);
                    if (was_unset || p.ty.is_none())
                        && self.has_magic_get(&cls)
                        && !self.guarded(o, name, 0)
                    {
                        let v = self.magic_get(o, &cls, name)?;
                        // test Zend/tests/type_declarations/typed_properties_030
                        if let Some(t) = &p.ty {
                            let strict = self.caller_strict();
                            return match self.check_type(&v, t, strict, p.class, p.class)? {
                                crate::vm::types::Check::Ok => Ok(v),
                                crate::vm::types::Check::Coerced(c) => Ok(c),
                                crate::vm::types::Check::Fail => self.type_error(&format!(
                                    "Value of type {} returned from {}::__get() must be compatible with unset property {}::${} of type {}",
                                    Vm::type_name_given(&v),
                                    cls.name.dname(),
                                    Self::prop_class_name(&p),
                                    name.to_string_lossy(),
                                    t.display()
                                )),
                            };
                        }
                        return Ok(v);
                    }
                    // `??`, `??=`: an uninitialized typed property is unset.
                    if p.ty.is_some() && quiet {
                        return Ok(Value::Null);
                    }
                    if p.ty.is_some() {
                        return self.throw(
                            "Error",
                            &format!(
                                "Typed property {}::${} must not be accessed before initialization",
                                Self::prop_class_name(&p),
                                name.to_string_lossy()
                            ),
                        );
                    }
                    if !quiet {
                        self.warn(&format!(
                            "Undefined property: {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ))?;
                    }
                    return Ok(Value::Null);
                }
                if let Some(c) = cache
                    && p.get.is_none()
                    && p.flags & mflags::VIRTUAL == 0
                {
                    self.code_ref().caches[c as usize].set(Cache {
                        key: Rc::as_ptr(&cls) as usize,
                        a: p.slot as usize,
                        b: IC_PLAIN,
                    });
                }
                Ok(v.into_deref())
            }
            PropLookup::Hidden(p) => {
                if self.has_magic_get(&cls) && !self.guarded(o, name, 0) {
                    return self.magic_get(o, &cls, name);
                }
                self.throw(
                    "Error",
                    &format!(
                        "Cannot access {} property {}::${}",
                        Self::vis_word(&p),
                        cls.name.dname(),
                        name.to_string_lossy()
                    ),
                )
            }
            PropLookup::Dynamic => {
                self.static_as_instance(&cls, name)?;
                if let Some(h) = cls.native.prop {
                    // `??` reads only what isset() finds (PHP manual: null
                    // coalescing operator; test ext/simplexml bug72957).
                    if quiet
                        && let Some(r) = h(self, o, name, crate::class::ObjAccess::Isset)
                        && !self.to_bool(&r?)
                    {
                        return Ok(Value::Null);
                    }
                    if let Some(r) = h(self, o, name, crate::class::ObjAccess::Read) {
                        return r;
                    }
                }
                let v = o.obj().props.borrow().get_str(name).cloned();
                match v {
                    Some(v) => Ok(v.into_deref()),
                    None => {
                        if self.has_magic_get(&cls) && !self.guarded(o, name, 0) {
                            return self.magic_get(o, &cls, name);
                        }
                        if let Some(h) = cls.native.dyn_prop
                            && let Some(r) = h(self, o, name, crate::class::DynPropOp::Read)
                        {
                            return r;
                        }
                        if let Some(h) = cls.native.debug_props
                            && let Some(arr) = h(self, o)
                            && let Some(v) = arr.get_str(name)
                        {
                            return Ok(v.deref());
                        }
                        if !quiet && name.starts_with(b"\0") {
                            return self.nul_prop_error();
                        }
                        if !quiet {
                            self.warn(&format!(
                                "Undefined property: {}::${}",
                                cls.name.dname(),
                                name.to_string_lossy()
                            ))?;
                        }
                        Ok(Value::Null)
                    }
                }
            }
        }
    }

    fn has_magic_get(&self, cls: &Class) -> bool {
        cls.magic.borrow().get.is_some()
    }

    /// Recursion guard for magic property methods (kind: 0 get, 1 set,
    /// 2 isset, 3 unset).
    fn guarded(&self, o: &ObjRef, name: &PhpStr, kind: u8) -> bool {
        self.prop_guards
            .iter()
            .any(|(h, n, k)| *h == o.handle() && n == name && *k == kind)
    }

    fn magic_get(&mut self, o: &ObjRef, cls: &Class, name: &PhpStr) -> R<Value> {
        let f = cls.magic.borrow().get.clone().unwrap();
        self.prop_guards.push((o.handle(), name.clone(), 0));
        let r = self.call_func_this(&f, Some(o.clone()), vec![Value::Str(name.clone())]);
        self.prop_guards.pop();
        r
    }

    /// A hooked property fetched for writing (`$o->p[] = 1`, `&$o->p`,
    /// `$o->p->x = 1`): the reference a `&get` hook returns, an object the
    /// property holds, or "Indirect modification of C::$p is not allowed"
    /// (RFC: Property hooks; tests Zend/tests/property_hooks/
    /// indirect_modification, get_by_ref*). `None`: not hooked here.
    pub(crate) fn hooked_fetch_w(
        &mut self,
        o: &ObjRef,
        name: &PhpStr,
        objects_ok: bool,
    ) -> R<Option<Value>> {
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        let PropLookup::Found(p) = self.lookup_prop(&cls, name, scope) else {
            return Ok(None);
        };
        if (p.get.is_none() && p.set.is_none()) || self.in_hook(o, &p) {
            return Ok(None);
        }
        if let Some(g) = &p.get
            && g.code()
                .is_some_and(|c| c.flags & crate::bytecode::fflags::BY_REF != 0)
        {
            let g = g.clone();
            let v = self.call_hook(o, &p, g, Vec::new())?;
            return Ok(Some(match v {
                Value::Ref(r) => Value::Ref(r),
                other => Value::Ref(PhpRef::new(other)),
            }));
        }
        if objects_ok {
            let v = self.read_prop(o, name, false, None)?;
            if let Value::Object(_) = v {
                return Ok(Some(v));
            }
            // Internal computed properties read as plain values: a null
            // one gives "Attempt to assign property on null" (ext/dom,
            // test bug47430.phpt).
            if v.is_null() && p.get.as_ref().is_some_and(|g| g.is_native()) {
                return Ok(Some(v));
            }
        } else if p.get.as_ref().is_some_and(|g| g.is_native()) {
            // ... and objects as themselves (`foreach ($o->list as &$x)`
            // reaches the object's iterator; test
            // ext/dom/tests/modern/token_list/foreach_by_ref.phpt).
            let v = self.read_prop(o, name, false, None)?;
            if let Value::Object(_) = v {
                return Ok(Some(v));
            }
        }
        self.throw(
            "Error",
            &format!(
                "Indirect modification of {}::${} is not allowed",
                cls.name.dname(),
                name.to_string_lossy()
            ),
        )
    }

    /// The object held by a readonly or asymmetric-visibility property.
    fn restricted_object(&mut self, o: &ObjRef, name: &PhpStr) -> Option<Value> {
        let cls = self.class_of(o);
        if let PropLookup::Found(p) = self.lookup_prop(&cls, name, self.scope_ptr())
            && p.flags & (mflags::READONLY | mflags::SET_VISIBILITY) != 0
        {
            let v = o.obj().slots.borrow()[p.slot as usize].clone().into_deref();
            if let Value::Object(_) = v {
                return Some(v);
            }
        }
        None
    }

    /// `$o->p++`, `$o->p .= x` on an initialized readonly property fail
    /// before the operation (test Zend/tests/readonly_props/
    /// readonly_containing_object).
    fn readonly_rmw_check(&mut self, o: &ObjRef, name: &PhpStr) -> R<()> {
        let cls = self.class_of(o);
        if let PropLookup::Found(p) = self.lookup_prop(&cls, name, self.scope_ptr())
            && p.flags & mflags::READONLY != 0
            && !self.is_uninit(&o.obj().slots.borrow()[p.slot as usize])
            && !self.readonly_clone_ok(o, &p)
        {
            return self.throw(
                "Error",
                &format!(
                    "Cannot modify readonly property {}::${}",
                    cls.name.dname(),
                    name.to_string_lossy()
                ),
            );
        }
        Ok(())
    }

    /// `$o->x` naming a static property visible here (tests
    /// Zend/tests/bug30820, bug76860).
    fn static_as_instance(&mut self, cls: &Class, name: &PhpStr) -> R<()> {
        if cls.static_props.borrow().is_empty() {
            return Ok(());
        }
        // __get/__set take the access instead (test Zend/tests/objects/
        // objects_024).
        {
            let m = cls.magic.borrow();
            if m.get.is_some() || m.set.is_some() {
                return Ok(());
            }
        }
        let Some(sp) = cls.find_static(name) else {
            return Ok(());
        };
        if sp.flags & mflags::PRIVATE != 0 && !std::ptr::eq(sp.class, self.scope_ptr()) {
            return Ok(());
        }
        self.notice_msg(&format!(
            "Accessing static property {}::${} as non static",
            cls.name.dname(),
            name.to_string_lossy()
        ))
    }

    fn in_hook(&self, o: &ObjRef, p: &PropInfo) -> bool {
        self.hook_guards
            .iter()
            .any(|(h, n)| *h == o.handle() && *n == p.name)
    }

    fn call_hook(
        &mut self,
        o: &ObjRef,
        p: &PropInfo,
        h: Rc<crate::func::Func>,
        args: Vec<Value>,
    ) -> R<Value> {
        self.hook_guards.push((o.handle(), p.name.clone()));
        // Raw: a `&get` hook's reference is kept for write fetches.
        let c = super::calls::Callee {
            static_class: self.class_ptr_of(o),
            scope: h.scope.get(),
            func: h,
            this: Some(o.clone()),
            closure: None,
            magic_name: None,
        };
        let r = self.call_callee_raw(c, args);
        self.hook_guards.pop();
        r
    }

    // ------------------------------------------------------------ isset

    pub(crate) fn op_isset_prop(&mut self, dst: Reg, obj: Reg, name: Reg, empty: bool) -> R<()> {
        let o = if obj == THIS {
            self.this_obj()
        } else {
            match self.get_quiet(obj) {
                Value::Object(o) => Some(o),
                _ => None,
            }
        };
        let n = self.get(name)?;
        let r = match o {
            None => empty,
            Some(o) => {
                let n = self.to_php_str(&n)?;
                self.prop_isset(&o, &n, empty)?
            }
        };
        self.put(dst, Value::Bool(r));
        Ok(())
    }

    pub fn prop_isset(&mut self, o: &ObjRef, name: &PhpStr, empty: bool) -> R<bool> {
        if o.is_lazy()
            && let Some(t) = self.lazy_route(o, name, super::lazy::LazyAccess::Isset)?
        {
            return self.forward_guarded(o, &t, name, |vm| vm.prop_isset(&t, name, empty));
        }
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        let val: Option<Value> = match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                if let Some(h) = &p.get
                    && !self.in_hook(o, &p)
                {
                    let h = h.clone();
                    let v = self.call_hook(o, &p, h, Vec::new())?;
                    Some(v.into_deref())
                } else if p.flags & mflags::VIRTUAL != 0 && !self.in_hook(o, &p) {
                    return self.throw(
                        "Error",
                        &format!(
                            "Property {}::${} is write-only",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                } else {
                    let v = o.obj().slots.borrow()[p.slot as usize].clone();
                    if self.is_uninit(&v) {
                        if self.is_unset_mark(&v) || p.ty.is_none() {
                            return self.magic_isset(o, &cls, name, empty);
                        }
                        None
                    } else {
                        Some(v.into_deref())
                    }
                }
            }
            PropLookup::Hidden(_) => return self.magic_isset(o, &cls, name, empty),
            PropLookup::Dynamic => {
                if let Some(h) = cls.native.prop {
                    let op = if empty {
                        crate::class::ObjAccess::Empty
                    } else {
                        crate::class::ObjAccess::Isset
                    };
                    if let Some(r) = h(self, o, name, op) {
                        let v = r?;
                        return Ok(self.to_bool(&v));
                    }
                }
                let v = o.obj().props.borrow().get_str(name).cloned();
                match v {
                    Some(v) => Some(v.into_deref()),
                    None => {
                        if let Some(h) = cls.native.debug_props
                            && cls.magic.borrow().isset.is_none()
                            && let Some(arr) = h(self, o)
                            && let Some(v) = arr.get_str(name)
                        {
                            Some(v.deref())
                        } else {
                            return self.magic_isset(o, &cls, name, empty);
                        }
                    }
                }
            }
        };
        Ok(match val {
            None => empty,
            Some(v) => {
                if empty {
                    !self.to_bool(&v)
                } else {
                    !v.is_null()
                }
            }
        })
    }

    fn magic_isset(&mut self, o: &ObjRef, cls: &Class, name: &PhpStr, empty: bool) -> R<bool> {
        let f = cls.magic.borrow().isset.clone();
        let Some(f) = f else { return Ok(empty) };
        if self.guarded(o, name, 2) {
            return Ok(empty);
        }
        self.prop_guards.push((o.handle(), name.clone(), 2));
        let r = self.call_func_this(&f, Some(o.clone()), vec![Value::Str(name.clone())]);
        self.prop_guards.pop();
        let set = self.to_bool(&r?);
        if !empty {
            return Ok(set);
        }
        if !set {
            return Ok(true);
        }
        if self.has_magic_get(cls) && !self.guarded(o, name, 0) {
            let v = self.magic_get(o, cls, name)?;
            return Ok(!self.to_bool(&v));
        }
        Ok(true)
    }

    // ------------------------------------------------------------ writes

    pub(crate) fn op_assign_prop(&mut self, obj: Reg, name: Reg, val: Reg, cache: u32) -> R<()> {
        let o = if obj == THIS {
            match self.this_obj() {
                Some(o) => o,
                None => return self.throw("Error", "Using $this when not in object context"),
            }
        } else {
            match self.peek(obj) {
                Some(Value::Object(o)) => o.clone(),
                _ => {
                    let v = self.get(obj)?;
                    match v {
                        Value::Object(o) => o,
                        other => {
                            let n = self.get(name)?;
                            let n = self.to_php_str(&n)?;
                            self.drop_tmp(val);
                            let t = match &other {
                                Value::Bool(true) => "true".to_string(),
                                Value::Bool(false) => "false".to_string(),
                                v => v.tname(),
                            };
                            return self.throw(
                                "Error",
                                &format!(
                                    "Attempt to assign property \"{}\" on {}",
                                    n.to_string_lossy(),
                                    t
                                ),
                            );
                        }
                    }
                }
            }
        };
        if is_const(name) {
            let c = self.code_ref().caches[cache as usize].get();
            if c.key != 0
                && c.key == self.class_ptr_of(&o) as usize
                && c.b & IC_PLAIN != 0
                && !o.is_lazy()
            {
                let v = self.get_hot(val)?;
                let mut slots = o.obj().slots.borrow_mut();
                let slot = &mut slots[c.a];
                let typed_ok = c.b & IC_TYPED == 0
                    || (exact_type(&v, c.b >> 8) && !matches!(slot, Value::Ref(_)));
                // An uninitialized typed property (not an unset() one,
                // which goes through __set) takes the value directly.
                let fresh =
                    c.b & IC_TYPED != 0 && self.is_uninit(slot) && !self.is_unset_mark(slot);
                if typed_ok && fresh {
                    let old = std::mem::replace(slot, v);
                    drop(slots);
                    drop(old);
                    return Ok(());
                }
                if typed_ok && !self.is_uninit(slot) {
                    let old = slot.assign(v);
                    drop(slots);
                    drop(old);
                    return Ok(());
                }
                drop(slots);
                let n = self.get(name)?;
                let n = self.to_php_str(&n)?;
                return self.write_prop(&o, &n, v, None);
            }
        }
        let n = self.get(name)?;
        let n = match n {
            Value::Str(s) => s,
            other => self.to_php_str(&other)?,
        };
        let v = self.get(val)?;
        self.write_prop(&o, &n, v, is_const(name).then_some(cache))
    }

    /// Fills a property inline cache for `$o->name` read-modify-writes
    /// from the current scope (plain or simply typed, accessible, no hooks,
    /// not readonly).
    pub(crate) fn prop_ic_fill(&mut self, o: &ObjRef, name: &PhpStr, cache: u32) {
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        if let PropLookup::Found(p) = self.lookup_prop(&cls, name, scope) {
            let typed = p.ty.as_ref().map(|t| (t.mask, t.classes.is_empty()));
            if typed.is_none_or(|(m, plain)| plain && m < (1 << 24) && m & tmask::MIXED == 0)
                && p.flags & (mflags::READONLY | mflags::VIRTUAL | mflags::SET_VISIBILITY) == 0
                && p.set.is_none()
                && p.get.is_none()
            {
                let b = match typed {
                    None => IC_PLAIN,
                    Some((m, _)) => IC_PLAIN | IC_TYPED | (m << 8),
                };
                self.code_ref().caches[cache as usize].set(Cache {
                    key: Rc::as_ptr(&cls) as usize,
                    a: p.slot as usize,
                    b,
                });
            }
        }
    }

    /// Records that reference `r` is bound to typed property `p`.
    pub(crate) fn type_ref(&mut self, r: &PhpRef, p: &Rc<PropInfo>, obj: Option<&ObjRef>) {
        if p.ty.is_none() {
            return;
        }
        let e = self.typed_refs.entry(r.addr()).or_default();
        if !r.is_typed() {
            e.clear();
            r.set_typed(true);
        }
        let same = |s: &super::TypedRefSource| {
            Rc::ptr_eq(&s.prop, p)
                && match (&s.obj, obj) {
                    (Some(w), Some(o)) => w.upgrade().is_some_and(|x| ObjRef::ptr_eq(&x, o)),
                    (None, None) => true,
                    _ => false,
                }
        };
        if !e.iter().any(same) {
            e.push(super::TypedRefSource {
                obj: obj.map(|o| o.downgrade()),
                prop: p.clone(),
            });
        }
    }

    /// The live typed properties a reference is bound to.
    pub(crate) fn typed_ref_props(&mut self, r: &PhpRef) -> Vec<Rc<PropInfo>> {
        let Some(srcs) = self.typed_refs.get_mut(&r.addr()) else {
            return Vec::new();
        };
        srcs.retain(|s| match &s.obj {
            None => true,
            Some(w) => match w.upgrade() {
                Some(o) => matches!(o.obj().slots.borrow().get(s.prop.slot as usize),
                    Some(Value::Ref(x)) if PhpRef::ptr_eq(x, r)),
                None => false,
            },
        });
        let out: Vec<Rc<PropInfo>> = srcs.iter().map(|s| s.prop.clone()).collect();
        if out.is_empty() {
            r.set_typed(false);
        }
        out
    }

    /// A value assigned through a typed reference, coerced to the types of
    /// the properties it is bound to (TypeError when it doesn't fit).
    pub(crate) fn typed_ref_value(&mut self, r: &PhpRef, v: Value) -> R<Value> {
        if !r.is_typed() {
            return Ok(v);
        }
        let props = self.typed_ref_props(r);
        let strict = self.caller_strict();
        let mut v = v;
        for p in &props {
            let Some(t) = &p.ty else { continue };
            match self.check_type(&v, t, strict, p.class, p.class)? {
                crate::vm::types::Check::Ok => {}
                crate::vm::types::Check::Coerced(c) => v = c,
                crate::vm::types::Check::Fail => {
                    return self.type_error(&format!(
                        "Cannot assign {} to reference held by property {}::${} of type {}",
                        Vm::type_name_given(&v),
                        unsafe { (*p.class).name.dname() },
                        p.name.dname(),
                        t.display()
                    ));
                }
            }
        }
        Ok(v)
    }

    /// The declared property `name` of `o` from the current scope, if typed.
    pub(crate) fn typed_prop_info(&self, o: &ObjRef, name: &PhpStr) -> Option<Rc<PropInfo>> {
        let cls = self.class_of(o);
        match self.lookup_prop(&cls, name, self.scope_ptr()) {
            PropLookup::Found(p) if p.ty.is_some() => Some(p),
            _ => None,
        }
    }

    /// `$o->name = v`.
    pub fn write_prop(&mut self, o: &ObjRef, name: &PhpStr, v: Value, cache: Option<u32>) -> R<()> {
        if o.is_lazy()
            && let Some(t) = self.lazy_route(o, name, super::lazy::LazyAccess::Write)?
        {
            return self.forward_guarded(o, &t, name, |vm| vm.write_prop(&t, name, v, None));
        }
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                if let Some(h) = &p.set
                    && !self.in_hook(o, &p)
                {
                    let h = h.clone();
                    // Internal classes' computed properties take values as
                    // plain typed properties do ("Cannot assign X to
                    // property C::$p of type T"; ext/dom, test
                    // Document_body_setter_errors.phpt).
                    let v = match &p.ty {
                        Some(t) if h.is_native() => self.coerce_prop(&p, t, v, &cls)?,
                        _ => v,
                    };
                    self.call_hook(o, &p, h, vec![v])?;
                    return Ok(());
                }
                if p.flags & mflags::VIRTUAL != 0 && !self.in_hook(o, &p) {
                    return self.throw(
                        "Error",
                        &format!(
                            "Property {}::${} is read-only",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                let (cur_uninit, was_unset) = {
                    let slots = o.obj().slots.borrow();
                    let s = &slots[p.slot as usize];
                    (self.is_uninit(s), self.is_unset_mark(s))
                };
                // An unset declared property takes __set like a missing one
                // (observed on PHP 8.5; test
                // ext/spl/tests/ArrayObject/arrayObject_setFlags_basic1).
                if was_unset
                    && let Some(f) = cls.magic.borrow().set.clone()
                    && !self.guarded(o, name, 1)
                {
                    return self.magic_set(o, f, name, v);
                }
                if p.flags & mflags::READONLY != 0 && !cur_uninit && !self.readonly_clone_ok(o, &p)
                {
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot modify readonly property {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                self.check_set_visibility(&p, scope, o, name, "modify")?;
                let v = match &p.ty {
                    Some(t) => self.coerce_prop(&p, t, v, &cls)?,
                    None => v,
                };
                // A property holding a typed reference (test Zend/tests/
                // type_declarations/typed_properties_062).
                let tref = match &o.obj().slots.borrow()[p.slot as usize] {
                    Value::Ref(r) if r.is_typed() => Some(r.clone()),
                    _ => None,
                };
                let v = match tref {
                    Some(r) => self.typed_ref_value(&r, v)?,
                    None => v,
                };
                let slots_ptr = o.obj().slots.as_ptr();
                let slot = unsafe { &mut (&mut *slots_ptr)[p.slot as usize] };
                if self.is_uninit(slot) {
                    let old = std::mem::replace(slot, v);
                    drop(old);
                } else {
                    // Typed reference targets keep their type.
                    let old = slot.assign(v);
                    drop(old);
                }
                let typed = p.ty.as_ref().map(|t| (t.mask, t.classes.is_empty()));
                if let Some(c) = cache
                    && typed
                        .is_none_or(|(m, plain)| plain && m < (1 << 24) && m & tmask::MIXED == 0)
                    && p.flags & (mflags::READONLY | mflags::VIRTUAL | mflags::SET_VISIBILITY) == 0
                    && p.set.is_none()
                    && p.get.is_none()
                {
                    let b = match typed {
                        None => IC_PLAIN,
                        Some((m, _)) => IC_PLAIN | IC_TYPED | (m << 8),
                    };
                    self.code_ref().caches[c as usize].set(Cache {
                        key: Rc::as_ptr(&cls) as usize,
                        a: p.slot as usize,
                        b,
                    });
                }
                Ok(())
            }
            PropLookup::Hidden(p) => {
                if let Some(f) = cls.magic.borrow().set.clone()
                    && !self.guarded(o, name, 1)
                {
                    return self.magic_set(o, f, name, v);
                }
                if p.flags & mflags::READONLY != 0 && p.flags & mflags::PRIVATE == 0 {
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot modify protected readonly property {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                self.throw(
                    "Error",
                    &format!(
                        "Cannot access {} property {}::${}",
                        Self::vis_word(&p),
                        cls.name.dname(),
                        name.to_string_lossy()
                    ),
                )
            }
            PropLookup::Dynamic => {
                self.static_as_instance(&cls, name)?;
                if let Some(h) = cls.native.prop
                    && let Some(r) = h(self, o, name, crate::class::ObjAccess::Write(v.clone()))
                {
                    return r.map(|_| ());
                }
                if let Some(h) = cls.native.dyn_prop
                    && let Some(r) = h(self, o, name, crate::class::DynPropOp::Write)
                {
                    return r.map(|_| ());
                }
                if o.obj().props.borrow().get_str(name).is_some() {
                    let mut props = o.obj().props.borrow_mut();
                    let old = props.assign(Key::Str(name.clone()), v);
                    drop(props);
                    drop(old);
                    return Ok(());
                }
                if let Some(f) = cls.magic.borrow().set.clone()
                    && !self.guarded(o, name, 1)
                {
                    return self.magic_set(o, f, name, v);
                }
                self.create_dynamic_prop(o, &cls, name)?;
                let mut props = o.obj().props.borrow_mut();
                let old = props.insert(Key::Str(name.clone()), v);
                drop(props);
                drop(old);
                Ok(())
            }
        }
    }

    fn readonly_clone_ok(&self, o: &ObjRef, _p: &PropInfo) -> bool {
        // Inside __clone, readonly properties may be reinitialized once.
        self.cloning.iter().any(|h| *h == o.handle())
    }

    fn check_set_visibility(
        &mut self,
        p: &PropInfo,
        scope: *const Class,
        _o: &ObjRef,
        name: &PhpStr,
        verb: &str,
    ) -> R<()> {
        let mut sv = p.flags & mflags::SET_VISIBILITY;
        // readonly is implicitly protected(set) (RFC: Asymmetric
        // Visibility v2).
        if sv == 0 && p.flags & mflags::READONLY != 0 && p.flags & mflags::PUBLIC != 0 {
            sv = mflags::PROTECTED_SET;
        }
        if sv == 0 || sv & mflags::PUBLIC_SET != 0 {
            return Ok(());
        }
        let dc = p.class;
        let ok = if sv & mflags::PRIVATE_SET != 0 {
            std::ptr::eq(scope, dc)
        } else {
            !scope.is_null() && {
                let sc = unsafe { &*scope };
                let d = unsafe { &*dc };
                sc.is_subclass_of(d) || d.is_subclass_of(sc)
            }
        };
        if ok {
            return Ok(());
        }
        let word = if sv & mflags::PRIVATE_SET != 0 {
            "private(set)"
        } else {
            "protected(set)"
        };
        let ro = if p.flags & mflags::READONLY != 0 && sv & mflags::PROTECTED_SET != 0 {
            " readonly"
        } else {
            ""
        };
        let cname = unsafe { (*dc).name.dname() };
        self.throw(
            "Error",
            &format!(
                "Cannot {} {}{} property {}::${} from {}",
                verb,
                word,
                ro,
                cname,
                name.to_string_lossy(),
                self.scope_desc(scope)
            ),
        )
    }

    fn magic_set(
        &mut self,
        o: &ObjRef,
        f: Rc<crate::func::Func>,
        name: &PhpStr,
        v: Value,
    ) -> R<()> {
        self.prop_guards.push((o.handle(), name.clone(), 1));
        let r = self.call_func_this(&f, Some(o.clone()), vec![Value::Str(name.clone()), v]);
        self.prop_guards.pop();
        r.map(|_| ())
    }

    fn create_dynamic_prop(&mut self, o: &ObjRef, cls: &Class, name: &PhpStr) -> R<()> {
        let _ = o;
        if name.starts_with(b"\0") {
            return self.nul_prop_error();
        }
        if cls.has_flag(cflags::NO_DYNAMIC) || cls.has_flag(cflags::READONLY) || cls.is_enum() {
            return self.throw(
                "Error",
                &format!(
                    "Cannot create dynamic property {}::${}",
                    cls.name.dname(),
                    name.to_string_lossy()
                ),
            );
        }
        if !self.allows_dynamic(cls) {
            self.deprecated(&format!(
                "Creation of dynamic property {}::${} is deprecated",
                cls.name.dname(),
                name.to_string_lossy()
            ))?;
        }
        Ok(())
    }

    fn allows_dynamic(&self, cls: &Class) -> bool {
        let mut c = Some(cls);
        while let Some(k) = c {
            if k.has_flag(cflags::ALLOW_DYNAMIC) {
                return true;
            }
            c = k.parent.as_deref();
        }
        false
    }

    /// Type check of a property assignment.
    pub(crate) fn coerce_prop(
        &mut self,
        p: &PropInfo,
        t: &TypeHint,
        v: Value,
        cls: &Class,
    ) -> R<Value> {
        let strict = self.caller_strict();
        let v = v.into_deref();
        match self.check_type(&v, t, strict, p.class, cls as *const Class)? {
            Check::Ok => Ok(v),
            Check::Coerced(c) => Ok(c),
            Check::Fail => self.type_error(&format!(
                "Cannot assign {} to property {}::${} of type {}",
                v.tname(),
                Self::prop_class_name(p),
                p.name.dname(),
                t.display()
            )),
        }
    }

    // ------------------------------------------------------------ write-path steps

    /// Final step of a write path on property `name`.
    pub(crate) fn prop_action(
        &mut self,
        o: &ObjRef,
        name: &PhpStr,
        cache: u32,
        action: PathAction,
        value: Option<Value>,
    ) -> R<Value> {
        let _ = cache;
        match action {
            PathAction::Assign(_) => {
                let v = value.unwrap();
                self.write_prop(o, name, v.clone(), None)?;
                // The assignment's value is the coerced one (test Zend/tests/
                // type_declarations/typed_properties_077).
                if !o.is_lazy()
                    && let Some(pi) = self.typed_prop_info(o, name)
                    && pi.get.is_none()
                    && pi.set.is_none()
                    && let Some(cur) = o.obj().slots.borrow().get(pi.slot as usize)
                {
                    return Ok(cur.clone().into_deref());
                }
                Ok(v)
            }
            PathAction::Unset => {
                self.unset_prop(o, name)?;
                Ok(Value::Null)
            }
            PathAction::AssignOp(op, _) => {
                let rhs = value.unwrap();
                // In-place append to an existing plain string property.
                if op == Bin::Concat
                    && let Value::Str(r) = &rhs
                    && let Some(p) = self.existing_plain_prop(o, name)
                {
                    let p = deref_ptr(p);
                    if let Value::Str(s) = unsafe { &mut *p } {
                        s.push_bytes(r);
                        return Ok(unsafe { (*p).clone() });
                    }
                }
                let old = self.read_prop(o, name, false, None)?;
                let new = self.binary_op(op, &old, &rhs)?;
                self.write_prop(o, name, new.clone(), None)?;
                Ok(new)
            }
            PathAction::IncDec(op) => {
                self.readonly_rmw_check(o, name)?;
                let old = self.read_prop(o, name, false, None)?;
                let new = self.incdec_value(op, &old)?;
                // An int property overflowing (tests Zend/tests/
                // type_declarations/typed_properties_019, 038, 075, 097).
                if let (Value::Int(_), Value::Float(_)) = (&old, &new)
                    && let Some(p) = self.typed_prop_info(o, name)
                    && let Some(t) = &p.ty
                    && t.mask & tmask::INT != 0
                    && t.mask & (tmask::FLOAT | tmask::MIXED) == 0
                {
                    let inc = op.is_inc();
                    // Through a reference (test Zend/tests/type_declarations/
                    // typed_properties_097).
                    let is_ref = matches!(
                        o.obj().slots.borrow().get(p.slot as usize),
                        Some(Value::Ref(_))
                    );
                    return self.type_error(&format!(
                        "Cannot {} {}property {}::${} of type {} past its {} value",
                        if inc { "increment" } else { "decrement" },
                        if is_ref { "a reference held by " } else { "" },
                        Self::prop_class_name(&p),
                        name.to_string_lossy(),
                        t.display(),
                        if inc { "maximal" } else { "minimal" }
                    ));
                }
                // An untyped property bound to a typed reference (test
                // Zend/tests/type_declarations/typed_properties_062).
                if let (Value::Int(_), Value::Float(_)) = (&old, &new)
                    && let Some(r) = self.prop_typed_ref(o, name)
                {
                    for p in self.typed_ref_props(&r) {
                        if let Some(t) = &p.ty
                            && t.mask & tmask::INT != 0
                            && t.mask & (tmask::FLOAT | tmask::MIXED) == 0
                        {
                            let inc = op.is_inc();
                            return self.type_error(&format!(
                                "Cannot {} a reference held by property {}::${} of type {} past its {} value",
                                if inc { "increment" } else { "decrement" },
                                Self::prop_class_name(&p),
                                p.name.to_string_lossy(),
                                t.display(),
                                if inc { "maximal" } else { "minimal" }
                            ));
                        }
                    }
                }
                self.write_prop(o, name, new.clone(), None)?;
                Ok(if op.is_post() { old } else { new })
            }
            PathAction::MakeRef | PathAction::FetchW
                if let Some(v) = self.hooked_fetch_w(o, name, false)? =>
            {
                Ok(v)
            }
            PathAction::MakeRef | PathAction::FetchW
                if let Some(v) = self.restricted_object(o, name) =>
            {
                // A reference to the object of a readonly or asymmetric
                // property: allowed, and does not rebind it (test
                // Zend/tests/readonly_props/readonly_containing_object).
                Ok(Value::Ref(PhpRef::new(v)))
            }
            PathAction::MakeRef | PathAction::FetchW => match self.prop_ptr(o, name, false)? {
                Some(p) => {
                    let r = unsafe { (*p).make_ref() };
                    if let Some(pi) = self.typed_prop_info(o, name) {
                        self.type_ref(&r, &pi, Some(o));
                    }
                    Ok(Value::Ref(r))
                }
                None => {
                    let v = self.read_prop(o, name, true, None)?;
                    let cls = self.class_of(o);
                    // Natively overloaded properties hand out a detached
                    // reference silently (observed on PHP 8.5; test
                    // ext/simplexml gh17736).
                    if cls.native.prop.is_none() {
                        self.notice_msg(&format!(
                            "Indirect modification of overloaded property {}::${} has no effect",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ))?;
                    }
                    Ok(Value::Ref(PhpRef::new(v)))
                }
            },
            PathAction::BindRef(_) => {
                let r = match value.unwrap() {
                    Value::Ref(r) => r,
                    other => PhpRef::new(other),
                };
                // A hooked property: its write fetch (a `&get` call or the
                // indirect-modification error) comes first (observed on
                // PHP 8.5; test Zend/tests/property_hooks/bug007).
                match self.hooked_fetch_w(o, name, false) {
                    Ok(None) => {}
                    Ok(Some(_)) => {
                        return self
                            .throw("Error", "Cannot assign by reference to overloaded object");
                    }
                    Err(super::Unwind::Throw(prev)) => {
                        let r: R<Value> =
                            self.throw("Error", "Cannot assign by reference to overloaded object");
                        if let Err(super::Unwind::Throw(ex)) = &r {
                            self.chain_previous(ex, prev);
                        }
                        return r;
                    }
                    Err(e) => return Err(e),
                }
                // Binding a typed property: the referenced value must fit
                // its type, checked before the property is touched (tests
                // Zend/tests/type_declarations/typed_properties_093, 096).
                let typed = self.typed_prop_info(o, name);
                // A readonly one fails its write fetch first (test
                // Zend/tests/readonly_props/readonly_modification).
                if typed
                    .as_ref()
                    .is_some_and(|p| p.flags & mflags::READONLY != 0)
                {
                    self.prop_ptr(o, name, false)?;
                }
                if let Some(pi) = &typed {
                    let cur = r.get();
                    let t = pi.ty.as_ref().unwrap();
                    let strict = self.caller_strict();
                    match self.check_type(&cur, t, strict, pi.class, pi.class)? {
                        crate::vm::types::Check::Ok => {}
                        crate::vm::types::Check::Coerced(c) => {
                            let old = r.set(c);
                            drop(old);
                        }
                        crate::vm::types::Check::Fail => {
                            return self.type_error(&format!(
                                "Cannot assign {} to property {}::${} of type {}",
                                Vm::type_name_given(&cur),
                                unsafe { (*pi.class).name.dname() },
                                name.dname(),
                                t.display()
                            ));
                        }
                    }
                    let cur = r.get();
                    let _ = self.typed_ref_value(&r, cur)?;
                }
                match self.prop_ptr(o, name, false)? {
                    Some(p) => {
                        if let Some(pi) = &typed {
                            self.type_ref(&r, pi, Some(o));
                        }
                        let old = unsafe { std::mem::replace(&mut *p, Value::Ref(r.clone())) };
                        drop(old);
                    }
                    None => {
                        return self
                            .throw("Error", "Cannot assign by reference to overloaded object");
                    }
                }
                Ok(Value::Ref(r))
            }
        }
    }

    /// Pointer to an initialized, accessible property without hooks, type
    /// or readonly constraints (in-place updates).
    /// The typed reference a declared property of `o` holds, if any.
    fn prop_typed_ref(&mut self, o: &ObjRef, name: &PhpStr) -> Option<PhpRef> {
        if o.is_lazy() {
            return None;
        }
        let cls = self.class_of(o);
        match self.lookup_prop(&cls, name, self.scope_ptr()) {
            PropLookup::Found(p) => match o.obj().slots.borrow().get(p.slot as usize) {
                Some(Value::Ref(r)) if r.is_typed() => Some(r.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn existing_plain_prop(&mut self, o: &ObjRef, name: &PhpStr) -> Option<*mut Value> {
        if o.is_lazy() {
            return None;
        }
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                if p.get.is_some()
                    || p.set.is_some()
                    || p.flags & (mflags::READONLY | mflags::VIRTUAL) != 0
                    || p.ty.is_some()
                {
                    return None;
                }
                let slots = o.obj().slots.as_ptr();
                let slot = unsafe { &mut (&mut *slots)[p.slot as usize] };
                if self.is_uninit(slot) || matches!(slot, Value::Ref(r) if r.is_typed()) {
                    return None;
                }
                Some(slot as *mut Value)
            }
            PropLookup::Dynamic => {
                let props = o.obj().props.as_ptr();
                let p = unsafe { (*props).get_mut(KeyRef::Str(name)) }?;
                Some(p as *mut Value)
            }
            PropLookup::Hidden(_) => None,
        }
    }

    /// Pointer to a property's storage for in-place modification (creating
    /// a dynamic property as null). `None` when access goes through magic
    /// methods or hooks.
    fn prop_ptr(&mut self, o: &ObjRef, name: &PhpStr, rmw: bool) -> R<Option<*mut Value>> {
        if o.is_lazy()
            && let Some(t) = self.lazy_route(o, name, super::lazy::LazyAccess::Write)?
        {
            return self.prop_ptr(&t, name, rmw);
        }
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                if (p.get.is_some() || p.set.is_some()) && !self.in_hook(o, &p) {
                    return Ok(None);
                }
                if p.flags & mflags::READONLY != 0 {
                    // Write fetches and references: never (observed on
                    // PHP 8.5; tests Zend/tests/readonly_props/*).
                    if rmw
                        && p.ty.is_some()
                        && self.is_uninit(&o.obj().slots.borrow()[p.slot as usize])
                    {
                        return self.throw(
                            "Error",
                            &format!(
                                "Typed property {}::${} must not be accessed before initialization",
                                Self::prop_class_name(&p),
                                name.to_string_lossy()
                            ),
                        );
                    }
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot indirectly modify readonly property {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                self.check_set_visibility(&p, scope, o, name, "indirectly modify")?;
                let slots = o.obj().slots.as_ptr();
                let slot = unsafe { &mut (&mut *slots)[p.slot as usize] };
                if self.is_uninit(slot) {
                    if p.ty.is_some() {
                        if rmw {
                            return self.throw(
                                "Error",
                                &format!(
                                    "Typed property {}::${} must not be accessed before initialization",
                                    Self::prop_class_name(&p),
                                    name.to_string_lossy()
                                ),
                            );
                        }
                        *slot = Value::Null;
                    } else {
                        if self.has_magic_get(&cls) {
                            return Ok(None);
                        }
                        if rmw {
                            self.warn(&format!(
                                "Undefined property: {}::${}",
                                cls.name.dname(),
                                name.to_string_lossy()
                            ))?;
                        }
                        *slot = Value::Null;
                    }
                }
                Ok(Some(slot as *mut Value))
            }
            PropLookup::Hidden(p) => {
                if self.has_magic_get(&cls) {
                    return Ok(None);
                }
                self.throw(
                    "Error",
                    &format!(
                        "Cannot access {} property {}::${}",
                        Self::vis_word(&p),
                        cls.name.dname(),
                        name.to_string_lossy()
                    ),
                )
            }
            PropLookup::Dynamic => {
                if cls.native.prop.is_some() {
                    return Ok(None);
                }
                if let Some(h) = cls.native.dyn_prop
                    && let Some(r) = h(self, o, name, crate::class::DynPropOp::Write)
                {
                    r?;
                }
                let exists = o.obj().props.borrow().get_str(name).is_some();
                if !exists {
                    if self.has_magic_get(&cls) {
                        return Ok(None);
                    }
                    if rmw {
                        self.warn(&format!(
                            "Undefined property: {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ))?;
                    }
                    self.create_dynamic_prop(o, &cls, name)?;
                }
                let props = o.obj().props.as_ptr();
                let slot = unsafe { (*props).entry(Key::Str(name.clone())) };
                Ok(Some(slot as *mut Value))
            }
        }
    }

    /// `$o->name` fetched for a nested write (`$o->name[] = ...`).
    pub(crate) fn prop_fetch_w(
        &mut self,
        o: &ObjRef,
        name: &PhpStr,
        _cache: u32,
        next: Option<&PathElem>,
        kind: u8,
    ) -> R<Option<super::dims::Place>> {
        if o.is_lazy()
            && let Some(t) = self.lazy_route(o, name, super::lazy::LazyAccess::Write)?
        {
            return self.prop_fetch_w(&t, name, _cache, next, kind);
        }
        let next_is_prop = matches!(next, Some(PathElem::Prop(..)));
        let cls = self.class_of(o);
        if !next_is_prop {
            // Typed properties can only auto-vivify arrays if allowed.
            let scope = self.scope_ptr();
            if let PropLookup::Found(p) = self.lookup_prop(&cls, name, scope)
                && let Some(t) = &p.ty
                && p.flags & mflags::READONLY == 0
                && p.flags & mflags::VIRTUAL == 0
            {
                let cur = o.obj().slots.borrow()[p.slot as usize].clone();
                let cur_null = cur.is_null() || self.is_uninit(&cur);
                if self.is_uninit(&cur) && t.mask & tmask::ARRAY == 0 {
                    return self.throw(
                        "Error",
                        &format!(
                            "Typed property {}::${} must not be accessed before initialization",
                            Self::prop_class_name(&p),
                            name.to_string_lossy()
                        ),
                    );
                }
                if cur_null
                    && t.mask & (tmask::ARRAY | tmask::MIXED | tmask::ITERABLE) == 0
                    && p.get.is_none()
                {
                    return self.type_error(&format!(
                        "Cannot auto-initialize an array inside property {}::${} of type {}",
                        Self::prop_class_name(&p),
                        name.to_string_lossy(),
                        t.display()
                    ));
                }
            }
        }
        if let Some(v) = self.hooked_fetch_w(o, name, true)? {
            return Ok(Some(super::dims::Place::Temp(Box::new(v))));
        }
        // An object in a readonly property may be modified itself.
        if let PropLookup::Found(p) = self.lookup_prop(&cls, name, self.scope_ptr())
            && p.flags & (mflags::READONLY | mflags::SET_VISIBILITY) != 0
        {
            let v = o.obj().slots.borrow()[p.slot as usize].clone();
            // (kind: 0 write, 1 read-write, 2 unset)
            if self.is_uninit(&v) && kind == 2 {
                return Ok(None);
            }
            if next_is_prop && kind == 1 && p.ty.is_some() && self.is_uninit(&v) {
                return self.throw(
                    "Error",
                    &format!(
                        "Typed property {}::${} must not be accessed before initialization",
                        Self::prop_class_name(&p),
                        name.to_string_lossy()
                    ),
                );
            }
            if let Value::Object(_) = v.deref() {
                return Ok(Some(super::dims::Place::Temp(Box::new(v.into_deref()))));
            }
        }
        match self.prop_ptr(o, name, false)? {
            Some(p) => Ok(Some(super::dims::Place::Ptr(p))),
            None => {
                // Through __get or a hook: a temporary (objects are handles,
                // so nested property writes still reach them). Overloaded
                // properties are fetched as such (they may be created).
                let v = match cls.native.prop {
                    Some(h)
                        if matches!(
                            self.lookup_prop(&cls, name, self.scope_ptr()),
                            PropLookup::Dynamic
                        ) =>
                    {
                        match h(self, o, name, crate::class::ObjAccess::Fetch) {
                            Some(r) => r?,
                            None => self.read_prop(o, name, false, None)?,
                        }
                    }
                    _ => self.read_prop(o, name, false, None)?,
                };
                if !matches!(v, Value::Object(_)) && !next_is_prop {
                    self.notice_msg(&format!(
                        "Indirect modification of overloaded property {}::${} has no effect",
                        cls.name.dname(),
                        name.to_string_lossy()
                    ))?;
                }
                Ok(Some(super::dims::Place::Temp(Box::new(v))))
            }
        }
    }

    pub fn unset_prop(&mut self, o: &ObjRef, name: &PhpStr) -> R<()> {
        if o.is_lazy()
            && let Some(t) = self.lazy_route(o, name, super::lazy::LazyAccess::Unset)?
        {
            return self.forward_guarded(o, &t, name, |vm| vm.unset_prop(&t, name));
        }
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                if p.get.is_some() || p.set.is_some() || p.flags & mflags::VIRTUAL != 0 {
                    // Internal classes' computed properties (ext/dom):
                    // "Cannot unset DOMElement::$nodeValue" (observed on
                    // PHP 8.5).
                    let internal = p.get.as_ref().is_some_and(|g| g.is_native());
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot unset {}{}::${}",
                            if internal { "" } else { "hooked property " },
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                // An unset declared property takes __unset like a missing
                // one (observed on PHP 8.5; test ext/spl/tests/bug45622).
                if self.is_unset_mark(&o.obj().slots.borrow()[p.slot as usize])
                    && let Some(f) = cls.magic.borrow().unset.clone()
                    && !self.guarded(o, name, 3)
                {
                    self.prop_guards.push((o.handle(), name.clone(), 3));
                    let r =
                        self.call_func_this(&f, Some(o.clone()), vec![Value::Str(name.clone())]);
                    self.prop_guards.pop();
                    return r.map(|_| ());
                }
                let init = !self.is_uninit(&o.obj().slots.borrow()[p.slot as usize]);
                let ro = p.flags & mflags::READONLY != 0;
                if ro && init && !self.readonly_clone_ok(o, &p) {
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot unset readonly property {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                self.check_set_visibility(&p, scope, o, name, "unset")?;
                if ro && !std::ptr::eq(scope, p.class) {
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot unset readonly property {}::${}",
                            cls.name.dname(),
                            name.to_string_lossy()
                        ),
                    );
                }
                let mark = self.unset_value();
                let mut slots = o.obj().slots.borrow_mut();
                let old = std::mem::replace(&mut slots[p.slot as usize], mark);
                drop(slots);
                drop(old);
                Ok(())
            }
            PropLookup::Hidden(p) => {
                if let Some(f) = cls.magic.borrow().unset.clone()
                    && !self.guarded(o, name, 3)
                {
                    self.prop_guards.push((o.handle(), name.clone(), 3));
                    let r =
                        self.call_func_this(&f, Some(o.clone()), vec![Value::Str(name.clone())]);
                    self.prop_guards.pop();
                    return r.map(|_| ());
                }
                self.throw(
                    "Error",
                    &format!(
                        "Cannot access {} property {}::${}",
                        Self::vis_word(&p),
                        cls.name.dname(),
                        name.to_string_lossy()
                    ),
                )
            }
            PropLookup::Dynamic => {
                if let Some(h) = cls.native.prop
                    && let Some(r) = h(self, o, name, crate::class::ObjAccess::Unset)
                {
                    return r.map(|_| ());
                }
                if let Some(h) = cls.native.dyn_prop
                    && let Some(r) = h(self, o, name, crate::class::DynPropOp::Unset)
                {
                    return r.map(|_| ());
                }
                let exists = o.obj().props.borrow().get_str(name).is_some();
                if exists {
                    let old = o.obj().props.borrow_mut().remove(KeyRef::Str(name));
                    drop(old);
                    return Ok(());
                }
                if let Some(f) = cls.magic.borrow().unset.clone()
                    && !self.guarded(o, name, 3)
                {
                    self.prop_guards.push((o.handle(), name.clone(), 3));
                    let r =
                        self.call_func_this(&f, Some(o.clone()), vec![Value::Str(name.clone())]);
                    self.prop_guards.pop();
                    return r.map(|_| ());
                }
                if name.starts_with(b"\0") {
                    return self.nul_prop_error();
                }
                Ok(())
            }
        }
    }

    /// A dynamic property name starting with NUL (observed on PHP 8.5;
    /// tests Zend/tests/magic_methods/bug52484*).
    fn nul_prop_error<T>(&mut self) -> R<T> {
        self.throw("Error", "Cannot access property starting with \"\\0\"")
    }

    // ------------------------------------------------------------ static properties

    pub(crate) fn static_prop_ref(
        &mut self,
        class: ClassSpec,
        name: Reg,
        write: bool,
    ) -> R<(PhpRef, Rc<crate::class::StaticProp>)> {
        let cls = self.resolve_class_spec(class, true)?;
        let n = self.get(name)?;
        let n = self.to_php_str(&n)?;
        self.find_static_prop_entry_ex(&cls, &n, write)
    }

    pub fn find_static_prop(&mut self, cls: &Rc<Class>, n: &PhpStr) -> R<PhpRef> {
        self.find_static_prop_entry(cls, n).map(|(r, _)| r)
    }

    /// A value bound to a typed static property by reference, checked
    /// against its type ("Cannot assign X to property A::$p of type T").
    pub(crate) fn typed_static_ref_value(
        &mut self,
        sp: &crate::class::StaticProp,
        v: Value,
    ) -> R<Value> {
        let Some(t) = &sp.ty else { return Ok(v) };
        let strict = self.caller_strict();
        match self.check_type(&v, t, strict, sp.class, sp.class)? {
            crate::vm::types::Check::Ok => Ok(v),
            crate::vm::types::Check::Coerced(c) => Ok(c),
            crate::vm::types::Check::Fail => self.type_error(&format!(
                "Cannot assign {} to property {}::${} of type {}",
                Vm::type_name_given(&v),
                unsafe { (*sp.class).name.dname() },
                sp.name.dname(),
                t.display()
            )),
        }
    }

    /// The storage of a static property and its declaration.
    pub fn find_static_prop_entry(
        &mut self,
        cls: &Rc<Class>,
        n: &PhpStr,
    ) -> R<(PhpRef, Rc<crate::class::StaticProp>)> {
        self.find_static_prop_entry_ex(cls, n, false)
    }

    /// `write`: a plain assignment, which an uninitialized typed property
    /// accepts.
    pub fn find_static_prop_entry_ex(
        &mut self,
        cls: &Rc<Class>,
        n: &PhpStr,
        write: bool,
    ) -> R<(PhpRef, Rc<crate::class::StaticProp>)> {
        self.ensure_statics(cls)?;
        if cls.is_trait() && cls.find_static(n).is_some() {
            // PHP 8.1 (test Zend/tests/traits/direct_static_member_access).
            self.deprecated(&format!(
                "Accessing static trait property {}::${} is deprecated, it should only be accessed on a class using the trait",
                cls.name.dname(),
                n.to_string_lossy()
            ))?;
        }
        let Some(sp) = cls.find_static(n) else {
            return self.throw(
                "Error",
                &format!(
                    "Access to undeclared static property {}::${}",
                    cls.name.dname(),
                    n.to_string_lossy()
                ),
            );
        };
        let scope = self.scope_ptr();
        let visible = if sp.flags & mflags::PUBLIC != 0 {
            true
        } else if scope.is_null() {
            false
        } else if sp.flags & mflags::PRIVATE != 0 {
            std::ptr::eq(sp.class, scope)
        } else {
            let sc = unsafe { &*scope };
            let dc = unsafe { &*sp.class };
            sc.is_subclass_of(dc) || dc.is_subclass_of(sc)
        };
        if !visible {
            let w = if sp.flags & mflags::PRIVATE != 0 {
                "private"
            } else {
                "protected"
            };
            return self.throw(
                "Error",
                &format!(
                    "Cannot access {} property {}::${}",
                    w,
                    cls.name.dname(),
                    n.to_string_lossy()
                ),
            );
        }
        if !write && self.is_uninit(&sp.cell.borrow().borrow()) {
            return self.throw(
                "Error",
                &format!(
                    "Typed static property {}::${} must not be accessed before initialization",
                    unsafe { (*sp.class).name.dname() },
                    n.to_string_lossy()
                ),
            );
        }
        // References to typed static properties are typed references.
        if let Some(t) = &sp.ty
            && !sp.cell.borrow().is_typed()
        {
            let pi = Rc::new(PropInfo {
                name: sp.name.clone(),
                slot: 0,
                flags: sp.flags,
                ty: Some(t.clone()),
                class: sp.class,
                get: None,
                set: None,
                doc: None,
                attrs: sp.attrs.clone(),
                default: None,
            });
            self.type_ref(&sp.cell.borrow().clone(), &pi, None);
        }
        let r = sp.cell.borrow().clone();
        Ok((r, sp))
    }

    pub(crate) fn op_isset_static_prop(
        &mut self,
        dst: Reg,
        class: ClassSpec,
        name: Reg,
        empty: bool,
    ) -> R<()> {
        let cls = match class {
            ClassSpec::Named(_) | ClassSpec::Dynamic(_) => {
                match self.resolve_class_spec_quiet(class)? {
                    Some(c) => c,
                    None => {
                        self.put(dst, Value::Bool(empty));
                        return Ok(());
                    }
                }
            }
            _ => self.resolve_class_spec(class, true)?,
        };
        let n = self.get(name)?;
        let n = self.to_php_str(&n)?;
        self.ensure_statics(&cls)?;
        let r = match cls.find_static(&n) {
            None => empty,
            Some(sp) => {
                let scope = self.scope_ptr();
                let visible = sp.flags & mflags::PUBLIC != 0
                    || (!scope.is_null()
                        && (std::ptr::eq(sp.class, scope)
                            || (sp.flags & mflags::PROTECTED != 0
                                && unsafe {
                                    (*scope).is_subclass_of(&*sp.class)
                                        || (*sp.class).is_subclass_of(&*scope)
                                })));
                if !visible {
                    empty
                } else {
                    let v = sp.cell.borrow().get();
                    if self.is_uninit(&v) {
                        empty
                    } else if empty {
                        !self.to_bool(&v)
                    } else {
                        !v.is_null()
                    }
                }
            }
        };
        self.put(dst, Value::Bool(r));
        Ok(())
    }

    // ------------------------------------------------------------ class references

    /// Resolves a class operand (`self`, `parent`, `static`, a name or a
    /// value).
    pub fn resolve_class_spec(&mut self, spec: ClassSpec, autoload: bool) -> R<Rc<Class>> {
        match spec {
            ClassSpec::Named(k) => {
                let name = self.code_ref().consts[k as usize].clone();
                let Value::Str(name) = name else {
                    unreachable!()
                };
                match self.find_class(&name, autoload)? {
                    Some(c) => Ok(c),
                    None => self.throw(
                        "Error",
                        &format!("Class \"{}\" not found", name.to_string_lossy()),
                    ),
                }
            }
            ClassSpec::SelfRef => {
                let p = self.scope_ptr();
                if p.is_null() {
                    return self.throw(
                        "Error",
                        "Cannot access \"self\" when no class scope is active",
                    );
                }
                Ok(self.rc_class(p))
            }
            ClassSpec::Parent => {
                let p = self.scope_ptr();
                if p.is_null() {
                    return self.throw(
                        "Error",
                        "Cannot access \"parent\" when no class scope is active",
                    );
                }
                match unsafe { (*p).parent.clone() } {
                    Some(c) => Ok(c),
                    None => self.throw(
                        "Error",
                        "Cannot use \"parent\" when current class scope has no parent",
                    ),
                }
            }
            ClassSpec::Static => {
                let p = self.static_class_ptr();
                if p.is_null() {
                    return self.throw(
                        "Error",
                        "Cannot access \"static\" when no class scope is active",
                    );
                }
                Ok(self.rc_class(p))
            }
            ClassSpec::Dynamic(r) => {
                let v = self.get(r)?;
                match v {
                    Value::Object(o) => Ok(self.class_of(&o)),
                    Value::Str(s) => {
                        let l = crate::class::lower(&s);
                        match l.as_bytes() {
                            b"self" => {
                                return self.resolve_class_spec(ClassSpec::SelfRef, autoload);
                            }
                            b"parent" => {
                                return self.resolve_class_spec(ClassSpec::Parent, autoload);
                            }
                            b"static" => {
                                return self.resolve_class_spec(ClassSpec::Static, autoload);
                            }
                            _ => {}
                        }
                        match self.find_class(&s, autoload)? {
                            Some(c) => Ok(c),
                            None => self.throw(
                                "Error",
                                &format!("Class \"{}\" not found", s.to_string_lossy()),
                            ),
                        }
                    }
                    // Observed on PHP 8.5 (test Zend/tests/nullsafe_operator/019).
                    _ => self.throw("Error", "Class name must be a valid object or a string"),
                }
            }
        }
    }

    /// `X::class` for special classes ("Cannot use" messages).
    pub fn resolve_class_name_spec(&mut self, spec: ClassSpec) -> R<Rc<Class>> {
        let (p, word) = match spec {
            ClassSpec::SelfRef => (self.scope_ptr(), "self"),
            ClassSpec::Parent => (self.scope_ptr(), "parent"),
            ClassSpec::Static => (self.static_class_ptr(), "static"),
            ClassSpec::Dynamic(r) => {
                // `$x::class` needs an object (observed on PHP 8.5).
                let v = self.get(r)?;
                if let Value::Object(o) = &v {
                    return Ok(self.class_of(o));
                }
                return self.type_error(&format!(
                    "Cannot use \"::class\" on {}",
                    Vm::type_name_given(&v)
                ));
            }
            other => return self.resolve_class_spec(other, false),
        };
        if p.is_null() {
            // Functions outside classes fail at compile time (observed on
            // PHP 8.5; test Zend/tests/class_name/class_name_as_scalar_error_007).
            let in_const = self.code_ref().flags & crate::bytecode::fflags::CONST_EXPR != 0;
            return self.throw(
                "Error",
                &if in_const {
                    format!("Cannot use \"{}\" when no class scope is active", word)
                } else {
                    format!("Cannot use \"{}\" in the global scope", word)
                },
            );
        }
        let c = self.rc_class(p);
        if word == "parent" {
            return match c.parent.clone() {
                Some(pc) => Ok(pc),
                None => self.throw(
                    "Error",
                    "Cannot use \"parent\" when current class scope has no parent",
                ),
            };
        }
        Ok(c)
    }

    fn resolve_class_spec_quiet(&mut self, spec: ClassSpec) -> R<Option<Rc<Class>>> {
        match spec {
            ClassSpec::Named(k) => {
                let name = self.code_ref().consts[k as usize].clone();
                let Value::Str(name) = name else {
                    unreachable!()
                };
                self.find_class(&name, true)
            }
            ClassSpec::Dynamic(r) => {
                let v = self.get(r)?;
                match v {
                    Value::Object(o) => Ok(Some(self.class_of(&o))),
                    Value::Str(s) => self.find_class(&s, true),
                    _ => Ok(None),
                }
            }
            other => self.resolve_class_spec(other, true).map(Some),
        }
    }

    pub(crate) fn op_instanceof(&mut self, dst: Reg, a: Reg, class: ClassSpec) -> R<()> {
        let v = self.get(a)?;
        let r = match &v {
            Value::Object(o) => {
                let oc = self.class_of(o);
                match class {
                    ClassSpec::Named(k) => {
                        let name = self.code_ref().consts[k as usize + 1].clone();
                        let Value::Str(lname) = name else {
                            unreachable!()
                        };
                        self.instance_of_lname(&oc, &lname)
                    }
                    ClassSpec::Dynamic(r) => {
                        let cv = self.get(r)?;
                        match cv {
                            Value::Object(co) => {
                                let cc = self.class_of(&co);
                                oc.is_subclass_of(&cc)
                            }
                            Value::Str(s) => {
                                let l = crate::class::lower(s.strip_prefix(b"\\").unwrap_or(&s));
                                self.instance_of_lname(&oc, &l)
                            }
                            _ => {
                                return self.throw(
                                    "Error",
                                    "Class name must be a valid object or a string",
                                );
                            }
                        }
                    }
                    other => {
                        let c = self.resolve_class_spec(other, false)?;
                        oc.is_subclass_of(&c)
                    }
                }
            }
            _ => {
                if let ClassSpec::Dynamic(r) = class {
                    let cv = self.get(r)?;
                    if !matches!(cv, Value::Object(_) | Value::Str(_)) {
                        return self
                            .throw("Error", "Class name must be a valid object or a string");
                    }
                }
                false
            }
        };
        self.put(dst, Value::Bool(r));
        Ok(())
    }

    pub(crate) fn instance_of_lname(&self, cls: &Class, lname: &[u8]) -> bool {
        let mut c = Some(cls);
        while let Some(k) = c {
            if k.lname.as_bytes() == lname {
                return true;
            }
            c = k.parent.as_deref();
        }
        if cls
            .interfaces
            .borrow()
            .iter()
            .any(|i| i.lname.as_bytes() == lname)
        {
            return true;
        }
        // A class_alias() name (test Zend/tests/class_alias/class_alias_001).
        match self.classes.get(&PhpStr::new(lname)) {
            Some(t) if t.lname.as_bytes() != lname => cls.is_subclass_of(t),
            _ => false,
        }
    }

    // ------------------------------------------------------------ class constants

    pub(crate) fn op_class_const(
        &mut self,
        dst: Reg,
        class: ClassSpec,
        name: Reg,
        cache: u32,
    ) -> R<()> {
        let static_like = matches!(class, ClassSpec::Static | ClassSpec::Dynamic(_));
        if !static_like && is_const(name) {
            let c = self.code_ref().caches[cache as usize].get();
            if c.key == 1 {
                let v = self.code_ref().consts[c.b as usize].clone();
                let _ = v;
            }
        }
        let cls = if self.code_ref().flags & crate::bytecode::fflags::CONST_EXPR != 0 {
            // Its errors show the constant expression's frame too (test
            // Zend/tests/type_declarations/typed_properties_022.phpt).
            let saved = std::mem::replace(&mut self.trace_const_expr, true);
            let r = self.resolve_class_spec(class, true);
            self.trace_const_expr = saved;
            r?
        } else {
            self.resolve_class_spec(class, true)?
        };
        let n = self.get(name)?;
        let n = match n {
            Value::Str(s) => s,
            other => {
                return self.throw(
                    "Error",
                    &format!(
                        "Cannot use value of type {} as class constant name",
                        other.tname()
                    ),
                );
            }
        };
        // test Zend/tests/traits/constant_004
        if cls.is_trait() && matches!(class, ClassSpec::Named(_)) && n.as_bytes() != b"class" {
            return self.throw(
                "Error",
                &format!(
                    "Cannot access trait constant {}::{} directly",
                    cls.name.dname(),
                    n.to_string_lossy()
                ),
            );
        }
        // In constant expressions a missing constant is named as written
        // (observed on PHP 8.5: "Undefined constant self::B"), and errors
        // of class constant lookups show a "[constant expression]" frame.
        let const_expr = self.code_ref().flags & crate::bytecode::fflags::CONST_EXPR != 0;
        if const_expr {
            let saved = std::mem::replace(&mut self.trace_const_expr, true);
            let r = self.class_const_in_expr(class, &cls, &n);
            self.trace_const_expr = saved;
            let v = r?;
            self.put(dst, v);
            return Ok(());
        }
        let v = self.class_constant(&cls, &n)?;
        self.put(dst, v);
        Ok(())
    }

    fn class_const_in_expr(&mut self, class: ClassSpec, cls: &Rc<Class>, n: &PhpStr) -> R<Value> {
        if matches!(class, ClassSpec::SelfRef | ClassSpec::Parent)
            && !n.as_bytes().eq_ignore_ascii_case(b"class")
            && cls.find_const(n).is_none()
        {
            let w = if matches!(class, ClassSpec::SelfRef) {
                "self"
            } else {
                "parent"
            };
            return self.throw(
                "Error",
                &format!("Undefined constant {}::{}", w, n.to_string_lossy()),
            );
        }
        self.class_constant(cls, n)
    }

    /// `Cls::NAME`.
    pub fn class_constant(&mut self, cls: &Rc<Class>, n: &PhpStr) -> R<Value> {
        if n.as_bytes().eq_ignore_ascii_case(b"class") {
            return Ok(Value::Str(cls.name.clone()));
        }
        let Some(c) = cls.find_const(n) else {
            return self.throw(
                "Error",
                &format!(
                    "Undefined constant {}::{}",
                    cls.name.dname(),
                    n.to_string_lossy()
                ),
            );
        };
        // Visibility.
        if c.flags & mflags::PUBLIC == 0 {
            let scope = self.scope_ptr();
            let ok = !scope.is_null()
                && if c.flags & mflags::PRIVATE != 0 {
                    std::ptr::eq(scope, c.class)
                } else {
                    let sc = unsafe { &*scope };
                    let dc = unsafe { &*c.class };
                    sc.is_subclass_of(dc) || dc.is_subclass_of(sc)
                };
            if !ok {
                let w = if c.flags & mflags::PRIVATE != 0 {
                    "private"
                } else {
                    "protected"
                };
                return self.throw(
                    "Error",
                    &format!(
                        "Cannot access {} constant {}::{}",
                        w,
                        cls.name.dname(),
                        n.to_string_lossy()
                    ),
                );
            }
        }
        if c.flags & mflags::DEPRECATED != 0
            && !self.deprecating.contains(&(c.attrs.as_ptr() as usize))
        {
            let detail = self.deprecation_detail(&c.attrs, c.class, false)?;
            let (since, msg) = detail.unwrap_or((None, None));
            let what = if c.flags & mflags::CASE != 0 {
                "Enum case"
            } else {
                "Constant"
            };
            let text = super::include::deprecation_text(
                &format!(
                    "{} {}::{}",
                    what,
                    unsafe { (*c.class).name.dname() },
                    n.to_string_lossy()
                ),
                since,
                msg,
            );
            self.raise(super::errors::E_USER_DEPRECATED, &text)?;
        }
        if c.flags & mflags::CASE != 0 {
            return self.enum_case(cls, n);
        }
        self.eval_class_const(&c)
    }

    pub fn eval_class_const(&mut self, c: &crate::class::ClassConst) -> R<Value> {
        let st = c.value.borrow().clone();
        match st {
            ConstState::Value(v) => Ok(v),
            ConstState::Evaluating => {
                let cn = unsafe { (*c.class).name.dname() };
                self.throw(
                    "Error",
                    &format!(
                        "Cannot declare self-referencing constant {}::{}",
                        cn,
                        c.name.dname()
                    ),
                )
            }
            ConstState::Pending(ce) => {
                *c.value.borrow_mut() = ConstState::Evaluating;
                let r = self.eval_const_expr(&ce, c.class);
                match r {
                    Ok(v) => {
                        if let Some(t) = &c.ty {
                            let strict = true;
                            match self.check_type(&v, t, strict, c.class, c.class)? {
                                Check::Ok => {}
                                Check::Coerced(x) => {
                                    *c.value.borrow_mut() = ConstState::Value(x.clone());
                                    return Ok(x);
                                }
                                Check::Fail => {
                                    *c.value.borrow_mut() = ConstState::Pending(ce);
                                    let cn = unsafe { (*c.class).name.dname() };
                                    return self.type_error(&format!(
                                        "Cannot assign {} to class constant {}::{} of type {}",
                                        v.tname(),
                                        cn,
                                        c.name.dname(),
                                        t.display()
                                    ));
                                }
                            }
                        }
                        *c.value.borrow_mut() = ConstState::Value(v.clone());
                        Ok(v)
                    }
                    Err(e) => {
                        *c.value.borrow_mut() = ConstState::Pending(ce);
                        Err(e)
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ new / clone

    pub(crate) fn op_new(&mut self, dst: Reg, class: ClassSpec, nargs: u32, cache: u32) -> R<()> {
        if let ClassSpec::Named(_) = class {
            // Classes are never undeclared: a resolved name stays valid.
            let c = self.code_ref().caches[cache as usize].get();
            if c.key == 1 {
                let cls = self.rc_class(c.a as *const Class);
                return self.new_object_call(dst, &cls, nargs);
            }
            let cls = self.resolve_class_spec(class, true)?;
            self.code_ref().caches[cache as usize].set(Cache {
                key: 1,
                a: Rc::as_ptr(&cls) as usize,
                b: 0,
            });
            return self.new_object_call(dst, &cls, nargs);
        }
        let cls = self.resolve_class_spec(class, true)?;
        self.new_object_call(dst, &cls, nargs)
    }

    pub(crate) fn op_new_anon(&mut self, dst: Reg, class: u32, nargs: u32) -> R<()> {
        let decl = self.code_ref().classes[class as usize].clone();
        let cls = if decl.declared.get() {
            match self.find_class(&decl.name, false)? {
                Some(c) => c,
                None => self.declare_class(&decl)?,
            }
        } else {
            self.declare_class(&decl)?
        };
        self.new_object_call(dst, &cls, nargs)
    }

    fn new_object_call(&mut self, dst: Reg, cls: &Rc<Class>, nargs: u32) -> R<()> {
        let _ = nargs;
        let o = self.instantiate(cls)?;
        let ctor = cls.magic.borrow().construct.clone();
        self.put(dst, Value::Object(o.clone()));
        match ctor {
            Some(f) => {
                let scope = self.scope_ptr();
                if !self.method_visible(&f, scope) {
                    // tests Zend/tests/closures/closure_instantiate,
                    // generators/errors/generator_instantiate_error
                    match cls.lname.as_bytes() {
                        b"closure" => {
                            return self
                                .throw("Error", "Instantiation of class Closure is not allowed");
                        }
                        b"generator" => {
                            return self.throw("Error", "The \"Generator\" class is reserved for internal use and cannot be manually instantiated");
                        }
                        _ => {}
                    }
                    let vis = if f.flags & mflags::PRIVATE != 0 {
                        "private"
                    } else {
                        "protected"
                    };
                    return self.throw(
                        "Error",
                        &format!(
                            "Call to {} {}::__construct() from {}",
                            vis,
                            cls.name.dname(),
                            self.scope_desc(scope)
                        ),
                    );
                }
                let c = super::calls::Callee {
                    scope: f.scope.get(),
                    func: f,
                    this: Some(o),
                    static_class: Rc::as_ptr(cls),
                    closure: None,
                    magic_name: None,
                };
                self.push_callee(c, pending_flags::NEW);
            }
            None => {
                // Arguments are still evaluated, then discarded.
                let f = self.noop_func();
                let mut c = super::calls::Callee::func(f);
                c.this = None;
                self.push_callee(c, pending_flags::NO_CTOR);
            }
        }
        Ok(())
    }

    /// A native function accepting anything (pending call of `new` without
    /// a constructor).
    fn noop_func(&mut self) -> Rc<crate::func::Func> {
        if let Some(f) = &self.noop {
            return f.clone();
        }
        fn noop(_: &mut Vm, _: &mut crate::func::Args) -> R<Value> {
            Ok(Value::Null)
        }
        let f = Rc::new(crate::func::Func::native(
            "{new}",
            "function noop(mixed ...$args): void",
            noop,
            0,
        ));
        self.noop = Some(f.clone());
        f
    }

    /// Creates an object of `cls` with its default property values.
    pub fn instantiate(&mut self, cls: &Rc<Class>) -> R<ObjRef> {
        if cls.kind != crate::class::ClassKind::Class || cls.has_flag(cflags::ABSTRACT) {
            let what = match cls.kind {
                crate::class::ClassKind::Interface => "interface",
                crate::class::ClassKind::Trait => "trait",
                crate::class::ClassKind::Enum => "enum",
                _ => "abstract class",
            };
            return self.throw(
                "Error",
                &format!("Cannot instantiate {} {}", what, cls.name.dname()),
            );
        }
        self.instantiate_raw(cls)
    }

    pub fn instantiate_raw(&mut self, cls: &Rc<Class>) -> R<ObjRef> {
        let defaults = self.class_defaults(cls)?;
        let info: Rc<dyn goro_runtime::ClassInfo> = cls.clone();
        let o = ObjRef::new(info, defaults);
        // Native initialization of internal classes (inherited).
        let mut c = Some(&**cls);
        while let Some(k) = c {
            if let Some(h) = k.native.create {
                h(self, &o);
                break;
            }
            c = k.parent.as_deref();
        }
        if goro_runtime::gc::should_collect(goro_runtime::gc::DEFAULT_THRESHOLD) && self.gc_enabled
        {
            self.collect_cycles()?;
        }
        Ok(o)
    }

    pub fn new_std_object(&mut self) -> R<ObjRef> {
        let c = self.core.std_class.clone().unwrap();
        self.instantiate_raw(&c)
    }

    /// Default slot values of a class (evaluated once).
    pub fn class_defaults(&mut self, cls: &Rc<Class>) -> R<Vec<Value>> {
        if let Some(d) = cls.defaults.get() {
            return Ok(d.clone());
        }
        let props: Vec<Rc<PropInfo>> = cls.props.borrow().clone();
        let mut v = vec![Value::Null; cls.num_slots.get() as usize];
        for p in &props {
            let val = match &p.default {
                Some(ConstExpr::Value(x)) => x.clone(),
                Some(ConstExpr::Code(_)) => {
                    let ce = p.default.clone().unwrap();
                    let x = self.eval_const_expr(&ce, p.class)?;
                    if let Some(t) = &p.ty {
                        match self.check_type(&x, t, true, p.class, Rc::as_ptr(cls))? {
                            Check::Fail => {
                                return self.type_error(&format!(
                                    "Cannot assign {} to property {}::${} of type {}",
                                    x.tname(),
                                    Self::prop_class_name(p),
                                    p.name.dname(),
                                    t.display()
                                ));
                            }
                            Check::Coerced(c) => c,
                            Check::Ok => x,
                        }
                    } else {
                        x
                    }
                }
                None => {
                    if p.ty.is_some() || p.flags & mflags::VIRTUAL != 0 {
                        self.uninit_value()
                    } else {
                        Value::Null
                    }
                }
            };
            let val = match (&p.ty, val) {
                (Some(t), Value::Int(i))
                    if t.mask & tmask::FLOAT != 0 && t.mask & tmask::INT == 0 =>
                {
                    Value::Float(i as f64)
                }
                (_, x) => x,
            };
            v[p.slot as usize] = val;
        }
        let _ = cls.defaults.set(v.clone());
        Ok(v)
    }

    pub fn clone_value(&mut self, v: Value) -> R<Value> {
        let Value::Object(o) = v else {
            return self.throw("Error", "__clone method called on non-object");
        };
        let n = self.clone_object(&o)?;
        Ok(Value::Object(n))
    }

    pub fn clone_object(&mut self, o: &ObjRef) -> R<ObjRef> {
        let cls = self.class_of(o);
        if o.is_lazy() {
            let t = self.lazy_realize(o)?;
            if !ObjRef::ptr_eq(&t, o) {
                // A proxy's clone proxies a clone of the real instance.
                let inst = self.clone_object(&t)?;
                let info: Rc<dyn goro_runtime::ClassInfo> = cls.clone();
                let n = ObjRef::new(info, o.obj().slots.borrow().clone());
                self.make_initialized_proxy(&n, inst);
                return Ok(n);
            }
        }
        if cls.has_flag(cflags::NOT_CLONEABLE) || cls.is_enum() {
            return self.throw(
                "Error",
                &format!(
                    "Trying to clone an uncloneable object of class {}",
                    cls.name.dname()
                ),
            );
        }
        let cl = cls.magic.borrow().clone.clone();
        if let Some(f) = &cl {
            let scope = self.scope_ptr();
            if !self.method_visible(f, scope) {
                let vis = if f.flags & mflags::PRIVATE != 0 {
                    "private"
                } else {
                    "protected"
                };
                return self.throw(
                    "Error",
                    &format!(
                        "Call to {} method {}::__clone() from {}",
                        vis,
                        cls.name.dname(),
                        self.scope_desc(scope)
                    ),
                );
            }
        }
        let slots: Vec<Value> = o
            .obj()
            .slots
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Ref(r) if r.refcount() == 1 => r.get(),
                v => v.clone(),
            })
            .collect();
        let info: Rc<dyn goro_runtime::ClassInfo> = cls.clone();
        let n = ObjRef::new(info, slots);
        *n.obj().props.borrow_mut() = o.obj().props.borrow().clone();
        // The clone's properties hold the typed references too (test
        // Zend/tests/type_declarations/typed_properties_081).
        let typed: Vec<(PhpRef, Rc<PropInfo>)> = {
            let sl = n.obj().slots.borrow();
            cls.props
                .borrow()
                .iter()
                .filter(|p| p.ty.is_some())
                .filter_map(|p| match sl.get(p.slot as usize) {
                    Some(Value::Ref(r)) if r.is_typed() => Some((r.clone(), p.clone())),
                    _ => None,
                })
                .collect()
        };
        for (r, p) in typed {
            self.type_ref(&r, &p, Some(&n));
        }
        let mut c = Some(&*cls);
        while let Some(k) = c {
            if let Some(h) = k.native.clone {
                h(self, o, &n);
                break;
            }
            c = k.parent.as_deref();
        }
        // A clone hook refusing the copy leaves its error deferred.
        if self.err.defer_depth == 0
            && let Some(e) = self.err.deferred.take()
        {
            return Err(e);
        }
        if let Some(f) = cl {
            self.cloning.push(n.handle());
            let r = self.call_func_this(&f, Some(n.clone()), Vec::new());
            self.cloning.pop();
            r?;
        }
        Ok(n)
    }

    /// `clone($o, ['prop' => v])` (PHP 8.5 "clone with").
    pub fn clone_with(&mut self, v: Value, with: Value) -> R<Value> {
        let Value::Object(o) = v else {
            return self.type_error(&format!(
                "clone(): Argument #1 ($object) must be of type object, {} given",
                v.tname()
            ));
        };
        let n = self.clone_object(&o)?;
        if let Value::Array(a) = with {
            self.cloning.push(n.handle());
            for (k, x) in a.iter() {
                let name = match k {
                    KeyRef::Str(s) => s.clone(),
                    KeyRef::Int(_) => {
                        self.cloning.pop();
                        return self.type_error("Cannot clone with numeric property names");
                    }
                };
                if let Err(e) = self.write_prop(&n, &name, x.deref(), None) {
                    self.cloning.pop();
                    return Err(e);
                }
            }
            self.cloning.pop();
        }
        Ok(Value::Object(n))
    }

    // ------------------------------------------------------------ property tables

    /// The properties as an array: `(array)$o` with mangled names when
    /// `mangle`, or visible ones from the current scope otherwise.
    pub fn object_props_array(&mut self, o: &ObjRef, mangle: bool) -> PhpArray {
        let o = &self.lazy_target(o);
        let cls = self.class_of(o);
        if mangle
            && let Some(h) = cls.native.props_for
            && let Some(a) = h(self, o)
        {
            return a;
        }
        if let Some(h) = cls.native.debug_props
            && mangle
            && let Some(a) = h(self, o)
        {
            return a;
        }
        self.plain_props_array(o, mangle)
    }

    /// The declared and dynamic properties, ignoring the class's
    /// property-table hooks.
    pub fn plain_props_array(&mut self, o: &ObjRef, mangle: bool) -> PhpArray {
        let cls = self.class_of(o);
        let mut out = PhpArray::new();
        let props: Vec<Rc<PropInfo>> = cls.props.borrow().clone();
        let slots = o.obj().slots.borrow();
        for p in &props {
            let v = &slots[p.slot as usize];
            if self.is_uninit(v) || p.flags & (mflags::VIRTUAL | mflags::HIDDEN) != 0 {
                continue;
            }
            let key = if !mangle || p.flags & mflags::PUBLIC != 0 {
                p.name.clone()
            } else if p.flags & mflags::PROTECTED != 0 {
                let mut k = b"\0*\0".to_vec();
                k.extend_from_slice(&p.name);
                PhpStr::from(k)
            } else {
                let mut k = vec![0u8];
                k.extend_from_slice(unsafe { &(*p.class).name });
                k.push(0);
                k.extend_from_slice(&p.name);
                PhpStr::from(k)
            };
            let v = match v {
                Value::Ref(r) if r.refcount() == 1 => r.get(),
                v => v.clone(),
            };
            out.insert(Key::from_php_str(&key), v);
        }
        drop(slots);
        for (k, v) in o.obj().props.borrow().iter() {
            out.insert(k.to_owned(), v.clone());
        }
        out
    }

    /// The property table as an array with numeric names as integer keys,
    /// as `(array)` and get_mangled_object_vars() give it (RFC: Convert
    /// numeric keys in object/array casts).
    pub fn object_props_cast(&mut self, o: &ObjRef) -> PhpArray {
        let a = self.object_props_array(o, true);
        Self::int_keys(a)
    }

    /// get_mangled_object_vars(): the declared and dynamic properties
    /// only, whatever the class's property-table hooks give (observed on
    /// PHP 8.5: ArrayObject, SplFixedArray, DateTime give `[]`; test
    /// ext/spl/tests/SplFixedArray_get_properties_for).
    pub fn object_props_mangled(&mut self, o: &ObjRef) -> PhpArray {
        let o = &self.lazy_target(o);
        let a = self.plain_props_array(o, true);
        Self::int_keys(a)
    }

    fn int_keys(a: PhpArray) -> PhpArray {
        if !a.iter().any(|(k, _)| matches!(k, goro_runtime::KeyRef::Str(s) if matches!(Key::from_php_str(s), Key::Int(_)))) {
            return a;
        }
        let mut out = PhpArray::new();
        for (k, v) in a.iter() {
            let k = match k {
                goro_runtime::KeyRef::Str(s) => Key::from_php_str(s),
                k => k.to_owned(),
            };
            out.insert(k, v.clone());
        }
        out
    }

    /// Properties visible from the current scope (get_object_vars,
    /// foreach): (name, value) pairs.
    pub fn visible_props(&mut self, o: &ObjRef) -> Vec<(Value, Value)> {
        let scope = self.scope_ptr();
        self.visible_props_from(o, scope)
    }

    /// Properties visible from `scope` (null: public ones, as
    /// json_encode() sees them whatever the calling scope; observed on
    /// PHP 8.5).
    pub fn visible_props_from(&mut self, o: &ObjRef, scope: *const Class) -> Vec<(Value, Value)> {
        let o = &self.realize_deferred(o);
        let cls = self.class_of(o);
        let mut out = Vec::new();
        let props: Vec<Rc<PropInfo>> = cls.props.borrow().clone();
        for p in &props {
            if p.flags & mflags::HIDDEN != 0 {
                continue;
            }
            match self.lookup_prop(&cls, &p.name, scope) {
                PropLookup::Found(f) if Rc::ptr_eq(&f, p) => {}
                _ => continue,
            }
            // Through the get hook (RFC: Property hooks; test
            // Zend/tests/property_hooks/dump).
            if let Some(g) = &p.get
                && !self.in_hook(o, p)
            {
                if self.err.deferred.is_some() {
                    break;
                }
                match self.call_hook(o, p, g.clone(), Vec::new()) {
                    Ok(v) => out.push((Value::Str(p.name.clone()), v.into_deref())),
                    Err(e) => {
                        self.err.deferred = Some(e);
                        break;
                    }
                }
                continue;
            }
            let v = o.obj().slots.borrow()[p.slot as usize].clone();
            if self.is_uninit(&v) || p.flags & mflags::VIRTUAL != 0 {
                continue;
            }
            let v = match v {
                Value::Ref(r) if r.refcount() == 1 => r.get(),
                v => v,
            };
            out.push((Value::Str(p.name.clone()), v));
        }
        for (k, v) in o.obj().props.borrow().iter() {
            let key = match k {
                KeyRef::Int(i) => Value::Int(i),
                KeyRef::Str(s) => Value::Str(s.clone()),
            };
            out.push((key, v.clone()));
        }
        out
    }

    /// Names of the properties foreach visits from the current scope,
    /// without reading them (hooks run as the loop reaches them).
    pub fn visible_prop_names(&mut self, o: &ObjRef) -> Vec<Value> {
        let o = &self.realize_deferred(o);
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        let mut out = Vec::new();
        let props: Vec<Rc<PropInfo>> = cls.props.borrow().clone();
        for p in &props {
            if p.flags & mflags::HIDDEN != 0 {
                continue;
            }
            match self.lookup_prop(&cls, &p.name, scope) {
                PropLookup::Found(f) if Rc::ptr_eq(&f, p) => {}
                _ => continue,
            }
            if p.get.is_none() || self.in_hook(o, p) {
                let v = &o.obj().slots.borrow()[p.slot as usize];
                if self.is_uninit(v) || p.flags & mflags::VIRTUAL != 0 {
                    continue;
                }
            }
            out.push(Value::Str(p.name.clone()));
        }
        for (k, _) in o.obj().props.borrow().iter() {
            out.push(match k {
                KeyRef::Int(i) => Value::Int(i),
                KeyRef::Str(s) => Value::Str(s.clone()),
            });
        }
        out
    }

    /// Whether the class has a property with hooks.
    pub(crate) fn has_hooked_props(cls: &Class) -> bool {
        cls.props
            .borrow()
            .iter()
            .any(|p| p.get.is_some() || p.set.is_some())
    }

    /// The property table with mangled names, values read through get
    /// hooks from any scope (var_export(); test Zend/tests/property_hooks/
    /// dump).
    pub fn hooked_props_mangled(&mut self, o: &ObjRef) -> PhpArray {
        let cls = self.class_of(o);
        let mut out = PhpArray::new();
        let props: Vec<Rc<PropInfo>> = cls.props.borrow().clone();
        for p in &props {
            if p.flags & mflags::HIDDEN != 0 {
                continue;
            }
            let key = if p.flags & mflags::PUBLIC != 0 {
                p.name.clone()
            } else if p.flags & mflags::PROTECTED != 0 {
                let mut k = b"\0*\0".to_vec();
                k.extend_from_slice(&p.name);
                PhpStr::from(k)
            } else {
                let mut k = vec![0u8];
                k.extend_from_slice(unsafe { &(*p.class).name });
                k.push(0);
                k.extend_from_slice(&p.name);
                PhpStr::from(k)
            };
            let v = if let Some(g) = &p.get
                && !self.in_hook(o, p)
            {
                if self.err.deferred.is_some() {
                    break;
                }
                match self.call_hook(o, p, g.clone(), Vec::new()) {
                    Ok(v) => v.into_deref(),
                    Err(e) => {
                        self.err.deferred = Some(e);
                        break;
                    }
                }
            } else {
                let v = o.obj().slots.borrow()[p.slot as usize].clone();
                if self.is_uninit(&v) || p.flags & mflags::VIRTUAL != 0 {
                    continue;
                }
                v.into_deref()
            };
            out.insert(Key::from_php_str(&key), v);
        }
        for (k, v) in o.obj().props.borrow().iter() {
            out.insert(k.to_owned(), v.clone());
        }
        out
    }

    /// Object comparison (`==`, `<=>`).
    pub(crate) fn compare_objects(&mut self, a: &Value, b: &Value) -> R<Option<i32>> {
        if let (Value::Object(x), Value::Object(y)) = (a, b) {
            if ObjRef::ptr_eq(x, y) {
                return Ok(Some(0));
            }
            if x.is_lazy() || y.is_lazy() {
                let x = self.lazy_realize(x)?;
                let y = self.lazy_realize(y)?;
                return self.compare_objects(&Value::Object(x), &Value::Object(y));
            }
            let cx = self.class_of(x);
            let cy = self.class_of(y);
            if let Some(h) = cx.native.compare.or(cy.native.compare)
                && let Some(r) = h(self, a, b)
            {
                return r.map(Some);
            }
            // Distinct enum cases are uncomparable (test Zend/tests/enum/
            // comparison).
            if cx.is_enum() || cy.is_enum() {
                return Ok(Some(1));
            }
            if !Rc::ptr_eq(&cx, &cy) {
                return Ok(Some(1));
            }
            if self.compare_depth > 256 {
                return self.throw("Error", "Nesting level too deep - recursive dependency?");
            }
            self.compare_depth += 1;
            let r = (|| {
                let n = cx.num_slots.get() as usize;
                for i in 0..n {
                    let u = x.obj().slots.borrow()[i].clone();
                    let w = y.obj().slots.borrow()[i].clone();
                    let (uu, wu) = (self.is_uninit(&u), self.is_uninit(&w));
                    if uu || wu {
                        if uu && wu {
                            continue;
                        }
                        return Ok(Some(1));
                    }
                    let c = goro_runtime::compare::compare(&u.deref(), &w.deref(), self)
                        .map_err(|e| self.php_error(e))?;
                    if c != 0 {
                        return Ok(Some(c));
                    }
                }
                let px = x.obj().props.borrow().clone();
                let py = y.obj().props.borrow().clone();
                let c = goro_runtime::compare::compare_arrays(&px, &py, self)
                    .map_err(|e| self.php_error(e))?;
                Ok(Some(c))
            })();
            self.compare_depth -= 1;
            return r;
        }
        // Object vs scalar.
        let (o, other, swapped) = match (a, b) {
            (Value::Object(o), x) => (o, x, false),
            (x, Value::Object(o)) => (o, x, true),
            _ => return Ok(None),
        };
        let cls = self.class_of(o);
        if let Some(h) = cls.native.compare
            && let Some(r) = h(self, a, b)
        {
            return r.map(Some);
        }
        match other {
            Value::Str(s) if cls.magic.borrow().to_string.is_some() => {
                let os = self.obj_to_string(o)?;
                let c = goro_runtime::compare::cmp_strings(&os, s);
                Ok(Some(if swapped { -c } else { c }))
            }
            Value::Null => Ok(Some(if swapped { -1 } else { 1 })),
            Value::Bool(bv) => {
                let c = (true as i32) - (*bv as i32);
                Ok(Some(if swapped { -c } else { c }))
            }
            _ => Ok(None),
        }
    }
}
