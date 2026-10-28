//! Object and resource handles, and deterministic destruction.
//!
//! The object *system* (classes, methods, property visibility) belongs to
//! the engine. This module provides the value-level part: a refcounted
//! object handle with an object id, storage for properties, a slot for
//! native (internal-class) data, and the bookkeeping PHP needs to run
//! destructors at the right moment.
//!
//! # Destructors
//!
//! PHP manual (Constructors and Destructors): "The destructor method will be
//! called as soon as there are no other references to a particular object".
//! Running a destructor needs the VM, which a `Drop` impl cannot reach, so
//! when the last [`ObjRef`] to an object whose class has a destructor is
//! dropped, the drop *keeps the object alive* by moving it into a
//! thread-local pending-destructor queue (and marks the destructor as
//! called, so it runs once). The engine drains the queue with
//! [`pop_pending_destructor`] at its next safe point, typically right after
//! the instruction that released the value; [`has_pending_destructors`] is a
//! cheap check for that. The destructor receives the object as `$this` and
//! may store it somewhere (resurrection); when the engine drops the
//! `ObjRef` afterwards the object is freed for real, if nothing else holds
//! it.
//!
//! Objects without a destructor are freed immediately by the last drop,
//! recursively freeing their properties.
//!
//! # Cycles
//!
//! Reference counting alone leaks cycles. Dropping an `ObjRef`, a
//! [`crate::PhpRef`] or a cycle-capable array while other
//! references remain records the target as a *possible root*, and
//! [`crate::gc::collect_cycles`] finds and frees garbage
//! cycles from those roots. See [`crate::gc`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::rc::{Rc, Weak};

use crate::array::PhpArray;
use crate::gc;
use crate::string::PhpStr;
use crate::value::Value;

/// What the runtime needs to know about an object's class. The engine's
/// class type implements this; the engine downcasts with
/// `(&*obj.class() as &dyn Any).downcast_ref::<MyClass>()`.
pub trait ClassInfo: Any {
    /// The class name as written in its declaration.
    fn name(&self) -> &PhpStr;
    /// Whether instances have a destructor (`__destruct`). Read once, when
    /// the object is created.
    fn has_destructor(&self) -> bool {
        false
    }
}

/// Native data attached to objects of internal classes.
pub trait NativeData: Any {
    /// Visit every PHP value this data keeps alive, for the cycle collector.
    fn trace(&self, _visit: &mut dyn FnMut(&Value)) {}
    /// Drop every PHP value this data holds (the collector breaks garbage
    /// cycles this way).
    fn clear(&mut self) {}
    /// Weak-keyed maps (WeakMap): the values stored for `key`. The
    /// collector treats them as referenced by the key (ephemerons), not by
    /// the map; `trace` must not report them.
    fn ephemerons(&self, _key: &ObjRef, _visit: &mut dyn FnMut(&Value)) {}
    /// A key object (by address) of this weak map went away: its entries
    /// are removed (their values returned, to be dropped by the caller).
    fn weak_key_dropped(&mut self, _key_addr: usize) -> Vec<Value> {
        Vec::new()
    }
}

pub(crate) const F_HAS_DTOR: u32 = 1;
pub(crate) const F_DTOR_CALLED: u32 = 2;
pub(crate) const F_GC_BUFFERED: u32 = 4;
pub(crate) const F_GC_CLEARED: u32 = 8;
/// A lazy object (uninitialized, or an initialized lazy proxy).
pub(crate) const F_LAZY: u32 = 16;
/// The object is (or was) a key of a weak map (see [`crate::gc`]).
pub(crate) const F_WEAK_KEY: u32 = 32;

/// An object's storage.
pub struct Object {
    handle: u32,
    pub(crate) flags: Cell<u32>,
    /// Root-buffer entry (when buffered).
    pub(crate) gc_idx: Cell<u32>,
    class: Rc<dyn ClassInfo>,
    /// Declared property slots, laid out by the engine's class.
    pub slots: RefCell<Vec<Value>>,
    /// The property table for dynamic properties (and anything else the
    /// engine keeps as an ordered map).
    pub props: RefCell<PhpArray>,
    /// Native data of internal classes.
    pub native: RefCell<Option<Box<dyn NativeData>>>,
}

/// A handle to an object. 8 bytes. Cloning shares the object (PHP objects
/// are handles: "objects are passed by handle").
pub struct ObjRef(pub(crate) Rc<Object>);

/// A weak handle (for `WeakReference`, `WeakMap`).
#[derive(Clone)]
pub struct WeakObj(Weak<Object>);

impl WeakObj {
    pub fn upgrade(&self) -> Option<ObjRef> {
        self.0.upgrade().map(ObjRef)
    }

    /// The object's address (stable while any handle exists).
    pub fn addr(&self) -> usize {
        self.0.as_ptr() as usize
    }
}

struct ObjectStore {
    /// Live objects by handle (index 0 unused: PHP object ids start at 1).
    slots: Vec<Option<Weak<Object>>>,
    /// Freed handles, reused most-recently-freed first.
    free: Vec<u32>,
}

thread_local! {
    static STORE: RefCell<ObjectStore> = const {
        RefCell::new(ObjectStore { slots: Vec::new(), free: Vec::new() })
    };
    static PENDING: RefCell<VecDeque<ObjRef>> = const { RefCell::new(VecDeque::new()) };
    static PENDING_LEN: Cell<usize> = const { Cell::new(0) };
}

/// Pending destructors on any thread: the per-instruction check is one
/// relaxed load (the thread-local queue is only looked at when non-zero).
static PENDING_ANY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl ObjRef {
    /// Creates an object of `class` with `slots` as its declared properties.
    pub fn new(class: Rc<dyn ClassInfo>, slots: Vec<Value>) -> ObjRef {
        let flags = if class.has_destructor() {
            F_HAS_DTOR
        } else {
            0
        };
        let handle = STORE.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(h) = s.free.pop() {
                h
            } else {
                if s.slots.is_empty() {
                    s.slots.push(None);
                }
                s.slots.push(None);
                (s.slots.len() - 1) as u32
            }
        });
        let obj = Rc::new(Object {
            handle,
            flags: Cell::new(flags),
            gc_idx: Cell::new(0),
            class,
            slots: RefCell::new(slots),
            props: RefCell::new(PhpArray::new()),
            native: RefCell::new(None),
        });
        STORE.with(|s| s.borrow_mut().slots[handle as usize] = Some(Rc::downgrade(&obj)));
        ObjRef(obj)
    }

    /// The object id (`spl_object_id`). Ids of freed objects are reused.
    #[inline]
    pub fn handle(&self) -> u32 {
        self.0.handle
    }

    #[inline]
    pub fn class(&self) -> &Rc<dyn ClassInfo> {
        &self.0.class
    }

    #[inline]
    pub fn class_name(&self) -> &PhpStr {
        self.0.class.name()
    }

    #[inline]
    pub fn obj(&self) -> &Object {
        &self.0
    }

    #[inline]
    pub fn ptr_eq(a: &ObjRef, b: &ObjRef) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// Number of handles to this object.
    #[inline]
    pub fn refcount(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    pub fn downgrade(&self) -> WeakObj {
        WeakObj(Rc::downgrade(&self.0))
    }

    /// Whether the destructor has been scheduled or run.
    pub fn destructor_called(&self) -> bool {
        self.0.flags.get() & F_DTOR_CALLED != 0
    }

    /// Marks the destructor as called (e.g. when the engine runs it at
    /// shutdown or the user calls it explicitly via the engine's logic).
    /// Returns false if it was already marked.
    pub fn mark_destructor_called(&self) -> bool {
        let f = self.0.flags.get();
        self.0.flags.set(f | F_DTOR_CALLED);
        f & F_DTOR_CALLED == 0
    }

    /// Allows the destructor to run again (a lazy object that got
    /// initialized).
    pub fn clear_destructor_called(&self) {
        let f = self.0.flags.get();
        self.0.flags.set(f & !F_DTOR_CALLED);
    }

    /// Whether the object is lazy (the engine keeps its state).
    #[inline]
    pub fn is_lazy(&self) -> bool {
        self.0.flags.get() & F_LAZY != 0
    }

    pub fn set_lazy(&self, on: bool) {
        let f = self.0.flags.get();
        self.0.flags.set(if on { f | F_LAZY } else { f & !F_LAZY });
    }

    /// Whether the class has a destructor that has not been called yet.
    pub fn needs_destructor(&self) -> bool {
        self.0.flags.get() & (F_HAS_DTOR | F_DTOR_CALLED) == F_HAS_DTOR
    }

    /// Whether the cycle collector has cleared this object's properties.
    pub fn gc_cleared(&self) -> bool {
        self.0.flags.get() & F_GC_CLEARED != 0
    }
}

impl Clone for ObjRef {
    #[inline]
    fn clone(&self) -> ObjRef {
        ObjRef(Rc::clone(&self.0))
    }
}

impl Drop for ObjRef {
    #[inline]
    fn drop(&mut self) {
        if Rc::strong_count(&self.0) == 1 {
            let f = self.0.flags.get();
            if f & (F_HAS_DTOR | F_DTOR_CALLED) == F_HAS_DTOR {
                self.0.flags.set(f | F_DTOR_CALLED);
                let keep = ObjRef(Rc::clone(&self.0));
                let _ = PENDING.try_with(|q| {
                    q.borrow_mut().push_back(keep);
                    PENDING_LEN.with(|n| n.set(n.get() + 1));
                    PENDING_ANY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                });
            }
        } else {
            gc::possible_root_obj(self);
        }
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        // The contents go first, then the handle is reused first (PHP's
        // object store frees properties before putting the handle on its
        // free list; observed on php85: `$c = new C; unset($c);` with C
        // holding a B, the next object gets C's handle).
        drop(std::mem::take(self.slots.get_mut()));
        drop(std::mem::take(self.props.get_mut()));
        drop(self.native.get_mut().take());
        if self.flags.get() & F_WEAK_KEY != 0 {
            crate::gc::forget_weak_key(self as *const Object as usize);
        }
        if self.flags.get() & F_GC_BUFFERED != 0 {
            crate::gc::remove_root(self.gc_idx.get(), self as *const Object as usize);
        }
        let h = self.handle as usize;
        let _ = STORE.try_with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                if let Some(slot) = s.slots.get_mut(h) {
                    *slot = None;
                }
                s.free.push(h as u32);
            }
        });
    }
}

impl fmt::Debug for ObjRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "object({})#{}",
            self.class_name().to_string_lossy(),
            self.handle()
        )
    }
}

/// Whether objects are waiting for their destructor to run.
#[inline]
pub fn has_pending_destructors() -> bool {
    PENDING_ANY.load(std::sync::atomic::Ordering::Relaxed) != 0
        && PENDING_LEN.with(|n| n.get() != 0)
}

/// Takes the next object whose destructor must run now (oldest first). The
/// engine calls `__destruct` on it, then drops the handle.
pub fn pop_pending_destructor() -> Option<ObjRef> {
    PENDING.with(|q| {
        let o = q.borrow_mut().pop_front();
        if o.is_some() {
            PENDING_LEN.with(|n| n.set(n.get() - 1));
            PENDING_ANY.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
        o
    })
}

/// All live objects in handle order (for shutdown destructor calls and
/// debugging).
pub fn live_objects() -> Vec<ObjRef> {
    STORE.with(|s| {
        s.borrow()
            .slots
            .iter()
            .filter_map(|w| w.as_ref().and_then(|w| w.upgrade()).map(ObjRef))
            .collect()
    })
}

/// Number of live objects.
pub fn live_object_count() -> usize {
    STORE.with(|s| {
        s.borrow()
            .slots
            .iter()
            .filter(|w| w.as_ref().is_some_and(|w| w.strong_count() > 0))
            .count()
    })
}

// ---------------------------------------------------------------- resources

/// A resource's storage.
pub struct Resource {
    id: i64,
    /// The type name (`get_resource_type`); "Unknown" once closed.
    pub kind: RefCell<PhpStr>,
    /// The native handle; `None` once closed.
    pub data: RefCell<Option<Box<dyn Any>>>,
}

/// A handle to a resource. 8 bytes.
#[derive(Clone)]
pub struct ResRef(pub(crate) Rc<Resource>);

thread_local! {
    static NEXT_RES: Cell<i64> = const { Cell::new(1) };
}

impl ResRef {
    /// Creates a resource with the next resource id (ids are not reused).
    pub fn new(kind: PhpStr, data: Box<dyn Any>) -> ResRef {
        let id = NEXT_RES.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        ResRef(Rc::new(Resource {
            id,
            kind: RefCell::new(kind),
            data: RefCell::new(Some(data)),
        }))
    }

    /// The resource id (`(int)$res`, "Resource id #N").
    #[inline]
    pub fn id(&self) -> i64 {
        self.0.id
    }

    pub fn res(&self) -> &Resource {
        &self.0
    }

    pub fn ptr_eq(a: &ResRef, b: &ResRef) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// Closes the resource: drops the native data, type becomes "Unknown".
    pub fn close(&self) -> Option<Box<dyn Any>> {
        *self.0.kind.borrow_mut() = PhpStr::intern(b"Unknown");
        self.0.data.borrow_mut().take()
    }

    pub fn is_closed(&self) -> bool {
        self.0.data.borrow().is_none()
    }

    /// A handle that does not keep the resource alive.
    pub fn downgrade(&self) -> WeakRes {
        WeakRes(Rc::downgrade(&self.0))
    }
}

/// A weak handle to a resource.
#[derive(Clone)]
pub struct WeakRes(std::rc::Weak<Resource>);

impl WeakRes {
    pub fn upgrade(&self) -> Option<ResRef> {
        self.0.upgrade().map(ResRef)
    }

    pub fn is_dead(&self) -> bool {
        self.0.strong_count() == 0
    }
}

impl fmt::Debug for ResRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "resource({}) of type ({})",
            self.id(),
            self.0.kind.borrow().to_string_lossy()
        )
    }
}
