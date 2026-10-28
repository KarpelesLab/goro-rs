//! Lazy objects (RFC "Lazy Objects", PHP 8.4): ghosts initialized in place
//! on first access to a lazy property, and proxies forwarding to a real
//! instance made by a factory. The state lives in the object's native
//! data; the object's lazy flag makes the property paths check it.

use std::rc::Rc;

use goro_runtime::{ObjRef, PhpArray, Value};

use super::objects::PropLookup;
use super::{R, Vm};
use crate::class::{Class, DisplayName, TypeName, mflags};

/// ReflectionClass::SKIP_INITIALIZATION_ON_SERIALIZE
pub const SKIP_INITIALIZATION_ON_SERIALIZE: i64 = 8;
/// ReflectionClass::SKIP_DESTRUCTOR
pub const SKIP_DESTRUCTOR: i64 = 16;

pub struct LazyData {
    pub proxy: bool,
    /// The initializer (ghost) or factory (proxy).
    pub init: Value,
    pub options: i64,
    /// An initialized proxy's real instance.
    pub instance: Option<ObjRef>,
    pub initializing: bool,
    /// Per slot: still lazy (not initialized nor skipped).
    pub lazy_slots: Vec<bool>,
}

impl goro_runtime::NativeData for LazyData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&self.init);
        if let Some(i) = &self.instance {
            visit(&Value::Object(i.clone()));
        }
    }
    fn clear(&mut self) {
        self.init = Value::Null;
        self.instance = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LazyAccess {
    Read,
    Write,
    Isset,
    Unset,
}

pub fn with_lazy<T>(o: &ObjRef, f: impl FnOnce(&mut LazyData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<LazyData>().map(f)
}

impl Vm {
    /// An uninitialized lazy object.
    pub fn is_lazy_uninit(&self, o: &ObjRef) -> bool {
        o.is_lazy() && with_lazy(o, |d| d.instance.is_none()).unwrap_or(false)
    }

    /// Makes `o` lazy: every declared property becomes lazy, dynamic
    /// properties go away.
    pub fn make_lazy(&mut self, o: &ObjRef, proxy: bool, init: Value, options: i64) {
        let cls = self.class_of(o);
        let n = cls.num_slots.get() as usize;
        let mut lazy_slots = vec![false; n];
        let uninit = self.uninit_value();
        {
            let props = cls.props.borrow();
            let mut slots = o.obj().slots.borrow_mut();
            for p in props.iter() {
                if p.flags & mflags::VIRTUAL != 0 {
                    continue;
                }
                let i = p.slot as usize;
                if i < slots.len() {
                    let old = std::mem::replace(&mut slots[i], uninit.clone());
                    drop(old);
                    lazy_slots[i] = true;
                }
            }
        }
        let old = std::mem::take(&mut *o.obj().props.borrow_mut());
        drop(old);
        *o.obj().native.borrow_mut() = Some(Box::new(LazyData {
            proxy,
            init,
            options,
            instance: None,
            initializing: false,
            lazy_slots,
        }));
        o.set_lazy(true);
        // Not destructed unless initialized.
        o.mark_destructor_called();
    }

    /// The object to work on for an access to `name`: `None` to go on with
    /// `o` itself, `Some(target)` after initializing (ghost: `o` again;
    /// proxy: its instance).
    pub fn lazy_route(&mut self, o: &ObjRef, name: &[u8], kind: LazyAccess) -> R<Option<ObjRef>> {
        let Some((inst, initializing)) = with_lazy(o, |d| (d.instance.clone(), d.initializing)) else {
            return Ok(None);
        };
        let cls = self.class_of(o);
        let scope = self.scope_ptr();
        if let Some(i) = inst {
            // An initialized proxy: the instance's properties, except
            // that a missing dynamic property may go to the proxy's own
            // magic method (GH-18038).
            if let PropLookup::Dynamic = self.lookup_prop(&cls, name, scope) {
                let n = goro_runtime::PhpStr::new(name);
                if i.obj().props.borrow().get_str(&n).is_none() && self.magic_unguarded(o, &cls, name, kind) {
                    return Ok(None);
                }
            }
            return Ok(Some(i));
        }
        if initializing {
            return Ok(None);
        }
        let needs_init = match self.lookup_prop(&cls, name, scope) {
            PropLookup::Found(p) => {
                let hooked = match kind {
                    LazyAccess::Read | LazyAccess::Isset => p.get.is_some(),
                    LazyAccess::Write => p.set.is_some(),
                    LazyAccess::Unset => p.get.is_some() || p.set.is_some(),
                };
                if (hooked && !self.in_hook_name(o, &p.name)) || p.flags & mflags::VIRTUAL != 0 {
                    false
                } else {
                    with_lazy(o, |d| d.lazy_slots.get(p.slot as usize).copied().unwrap_or(false))
                        .unwrap_or(false)
                }
            }
            PropLookup::Hidden(_) => false,
            PropLookup::Dynamic => {
                let n = goro_runtime::PhpStr::new(name);
                if o.obj().props.borrow().get_str(&n).is_some() {
                    false
                } else {
                    !self.magic_unguarded(o, &cls, name, kind)
                }
            }
        };
        if !needs_init {
            return Ok(None);
        }
        Ok(Some(self.lazy_init(o)?))
    }

    /// Before enumerating the properties of `o` (casts, foreach, compare,
    /// serialization...): initializes it; the object holding the
    /// properties (an initialized proxy's instance).
    pub fn lazy_realize(&mut self, o: &ObjRef) -> R<ObjRef> {
        if !o.is_lazy() {
            return Ok(o.clone());
        }
        let Some((inst, initializing)) = with_lazy(o, |d| (d.instance.clone(), d.initializing)) else {
            return Ok(o.clone());
        };
        if let Some(i) = inst {
            return Ok(i);
        }
        if initializing {
            return Ok(o.clone());
        }
        self.lazy_init(o)
    }

    /// Initializes a lazy object; the object holding its state afterwards.
    pub fn lazy_init(&mut self, o: &ObjRef) -> R<ObjRef> {
        let Some((proxy, init, inst)) = with_lazy(o, |d| (d.proxy, d.init.clone(), d.instance.clone())) else {
            return Ok(o.clone());
        };
        if let Some(i) = inst {
            return Ok(i);
        }
        let cls = self.class_of(o);
        if proxy {
            with_lazy(o, |d| d.initializing = true);
            let r = self.call_value(&init, vec![Value::Object(o.clone())]);
            with_lazy(o, |d| d.initializing = false);
            let r = r?;
            let inst = match r {
                Value::Object(i) => i,
                other => {
                    return self.type_error(&format!(
                        "Lazy proxy factory must return an instance of a class compatible with {}, {} returned",
                        cls.name.dname(),
                        other.tname()
                    ));
                }
            };
            if inst.is_lazy() && self.is_lazy_uninit(&inst) {
                return self.throw("Error", "Lazy proxy factory must return a non-lazy object");
            }
            let ic = self.class_of(&inst);
            if !proxy_compatible(&cls, &ic) {
                return self.type_error(&format!(
                    "The real instance class {} is not compatible with the proxy class {}. The proxy must be a instance of the same class as the real instance, or a sub-class with no additional properties, and no overrides of the __destructor or __clone methods.",
                    ic.name.dname(),
                    cls.name.dname()
                ));
            }
            // The proxy's own state is dropped.
            let uninit = self.uninit_value();
            {
                let mut slots = o.obj().slots.borrow_mut();
                for s in slots.iter_mut() {
                    let old = std::mem::replace(s, uninit.clone());
                    drop(old);
                }
            }
            let old = std::mem::take(&mut *o.obj().props.borrow_mut());
            drop(old);
            with_lazy(o, |d| {
                d.instance = Some(inst.clone());
                d.init = Value::Null;
                for b in d.lazy_slots.iter_mut() {
                    *b = false;
                }
            });
            return Ok(inst);
        }
        // Ghost: defaults first, then the initializer; reverted on errors.
        let saved_slots: Vec<Value> = o.obj().slots.borrow().clone();
        let saved_props: PhpArray = o.obj().props.borrow().clone();
        let defaults = self.class_defaults(&cls)?;
        let lazy_slots = with_lazy(o, |d| d.lazy_slots.clone()).unwrap_or_default();
        {
            let mut slots = o.obj().slots.borrow_mut();
            for (i, lazy) in lazy_slots.iter().enumerate() {
                if *lazy && i < slots.len() && i < defaults.len() {
                    slots[i] = defaults[i].clone();
                }
            }
        }
        with_lazy(o, |d| d.initializing = true);
        // Not lazy while the initializer runs.
        o.set_lazy(false);
        let r = self.call_value(&init, vec![Value::Object(o.clone())]);
        let r = match r {
            Ok(Value::Null) => Ok(()),
            Ok(_) => self.type_error("Lazy object initializer must return NULL or no value"),
            Err(e) => Err(e),
        };
        match r {
            Ok(()) => {
                *o.obj().native.borrow_mut() = None;
                o.set_lazy(false);
                o.clear_destructor_called();
                Ok(o.clone())
            }
            Err(e) => {
                let old_slots = std::mem::replace(&mut *o.obj().slots.borrow_mut(), saved_slots);
                let old_props = std::mem::replace(&mut *o.obj().props.borrow_mut(), saved_props);
                drop(old_slots);
                drop(old_props);
                with_lazy(o, |d| d.initializing = false);
                o.set_lazy(true);
                Err(e)
            }
        }
    }

    /// Marks a lazy property as initialized (skipLazyInitialization,
    /// setRawValueWithoutLazyInitialization); the object stops being lazy
    /// when none is left.
    pub fn lazy_clear_slot(&mut self, o: &ObjRef, slot: usize) {
        let none_left = with_lazy(o, |d| {
            if let Some(b) = d.lazy_slots.get_mut(slot) {
                *b = false;
            }
            d.instance.is_none() && d.lazy_slots.iter().all(|b| !b)
        })
        .unwrap_or(false);
        if none_left {
            *o.obj().native.borrow_mut() = None;
            o.set_lazy(false);
            o.clear_destructor_called();
        }
    }

    /// Makes `n` an initialized lazy proxy of `inst` (clones of proxies).
    pub fn make_initialized_proxy(&mut self, n: &ObjRef, inst: ObjRef) {
        let uninit = self.uninit_value();
        let len = {
            let mut slots = n.obj().slots.borrow_mut();
            for s in slots.iter_mut() {
                *s = uninit.clone();
            }
            slots.len()
        };
        *n.obj().native.borrow_mut() = Some(Box::new(LazyData {
            proxy: true,
            init: Value::Null,
            options: 0,
            instance: Some(inst),
            initializing: false,
            lazy_slots: vec![false; len],
        }));
        n.set_lazy(true);
    }

    /// Initializes a lazy object before its properties are enumerated; an
    /// error is deferred to the end of the current builtin.
    pub fn realize_deferred(&mut self, o: &ObjRef) -> ObjRef {
        if !o.is_lazy() {
            return o.clone();
        }
        match self.lazy_realize(o) {
            Ok(t) => t,
            Err(e) => {
                if self.err.deferred.is_none() {
                    self.err.deferred = Some(e);
                }
                o.clone()
            }
        }
    }

    /// The object holding the properties of `o` without initializing it
    /// (an initialized proxy's instance).
    pub fn lazy_target(&self, o: &ObjRef) -> ObjRef {
        if o.is_lazy()
            && let Some(Some(i)) = with_lazy(o, |d| d.instance.clone())
        {
            return i;
        }
        o.clone()
    }

    /// Whether the slot of `o` is still lazy.
    pub fn lazy_slot(&self, o: &ObjRef, slot: usize) -> bool {
        o.is_lazy()
            && with_lazy(o, |d| d.instance.is_none() && d.lazy_slots.get(slot).copied().unwrap_or(false))
                .unwrap_or(false)
    }

    /// The class has the magic method for `kind`, not running for `name`.
    fn magic_unguarded(&self, o: &ObjRef, cls: &Class, name: &[u8], kind: LazyAccess) -> bool {
        let m = cls.magic.borrow();
        let (f, g) = match kind {
            LazyAccess::Read => (m.get.is_some(), 0),
            LazyAccess::Write => (m.set.is_some(), 1),
            LazyAccess::Isset => (m.isset.is_some(), 2),
            LazyAccess::Unset => (m.unset.is_some(), 3),
        };
        f && !self
            .prop_guards
            .iter()
            .any(|(h, nm, k)| *h == o.handle() && nm.as_bytes() == name && *k == g)
    }

    /// Runs an access forwarded from a proxy to its instance with the
    /// proxy's magic-method guards for `name` (GH-18038).
    pub fn forward_guarded<T>(
        &mut self,
        from: &ObjRef,
        to: &ObjRef,
        name: &[u8],
        f: impl FnOnce(&mut Vm) -> R<T>,
    ) -> R<T> {
        if ObjRef::ptr_eq(from, to) {
            return f(self);
        }
        let kinds: Vec<u8> = self
            .prop_guards
            .iter()
            .filter(|(h, n, _)| *h == from.handle() && n.as_bytes() == name)
            .map(|g| g.2)
            .collect();
        let n0 = self.prop_guards.len();
        for k in kinds {
            self.prop_guards.push((to.handle(), goro_runtime::PhpStr::new(name), k));
        }
        let r = f(self);
        self.prop_guards.truncate(n0);
        r
    }

    fn in_hook_name(&self, o: &ObjRef, name: &[u8]) -> bool {
        self.hook_guards.iter().any(|(h, n)| *h == o.handle() && n.as_bytes() == name)
    }
}

/// A real instance's class fits a proxy of `proxy`: the same class, or a
/// parent of it when the proxy adds no properties nor __destruct/__clone.
fn proxy_compatible(proxy: &Rc<Class>, real: &Rc<Class>) -> bool {
    if Rc::ptr_eq(proxy, real) {
        return true;
    }
    if !proxy.is_subclass_of(real) {
        return false;
    }
    if proxy.num_slots.get() != real.num_slots.get() {
        return false;
    }
    let pm = proxy.magic.borrow();
    let rm = real.magic.borrow();
    let same = |a: &Option<Rc<crate::func::Func>>, b: &Option<Rc<crate::func::Func>>| match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    };
    same(&pm.destruct, &rm.destruct) && same(&pm.clone, &rm.clone)
}
