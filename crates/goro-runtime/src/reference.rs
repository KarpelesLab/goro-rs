//! PHP references (`&`).
//!
//! PHP manual (References Explained): "References in PHP are a means to
//! access the same variable content by different names." A [`PhpRef`] is a
//! shared, mutable cell holding one value. A variable, array element or
//! property that "is a reference" holds a `Value::Ref` pointing at the cell;
//! every name bound with `=&` holds a clone of the same `PhpRef`.
//!
//! The reference count of the cell is the number of slots bound to it. When
//! it drops to 1 the binding is no longer observable as a reference: PHP's
//! `var_dump` stops showing `&`, and copying an array dereferences such
//! elements (see [`crate::array`]).

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::fmt;
use std::rc::{Rc, Weak};

use crate::gc;
use crate::value::Value;

pub(crate) struct RefInner {
    pub(crate) val: RefCell<Value>,
    pub(crate) flags: Cell<u8>,
}

/// A shared reference cell. 8 bytes.
pub struct PhpRef(pub(crate) Rc<RefInner>);

impl PhpRef {
    /// A new reference cell holding `v`. If `v` is itself a reference, the
    /// existing cell is returned instead (references don't nest).
    pub fn new(v: Value) -> PhpRef {
        match v {
            Value::Ref(r) => r,
            v => PhpRef(Rc::new(RefInner {
                val: RefCell::new(v),
                flags: Cell::new(0),
            })),
        }
    }

    /// A clone of the referenced value.
    #[inline]
    pub fn get(&self) -> Value {
        self.0.val.borrow().clone()
    }

    /// Replaces the referenced value, returning the old one. The caller
    /// should drop the old value outside any borrow it holds.
    #[inline]
    pub fn set(&self, v: Value) -> Value {
        debug_assert!(!matches!(v, Value::Ref(_)), "nested reference");
        std::mem::replace(&mut *self.0.val.borrow_mut(), v)
    }

    /// Shared borrow of the referenced value.
    #[inline]
    pub fn borrow(&self) -> Ref<'_, Value> {
        self.0.val.borrow()
    }

    /// Mutable borrow of the referenced value. Don't hold it across calls
    /// that may reach user code.
    #[inline]
    pub fn borrow_mut(&self) -> RefMut<'_, Value> {
        self.0.val.borrow_mut()
    }

    /// Number of slots bound to this reference.
    #[inline]
    pub fn refcount(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    /// Whether two references are the same cell.
    #[inline]
    pub fn ptr_eq(a: &PhpRef, b: &PhpRef) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// An identity usable as a map key while the reference is alive.
    pub fn addr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }

    pub(crate) fn downgrade(&self) -> Weak<RefInner> {
        Rc::downgrade(&self.0)
    }
}

impl Clone for PhpRef {
    #[inline]
    fn clone(&self) -> PhpRef {
        PhpRef(Rc::clone(&self.0))
    }
}

impl Drop for PhpRef {
    #[inline]
    fn drop(&mut self) {
        // A reference that survives this drop may now only be reachable from
        // a cycle: record it as a possible root (PHP manual, Garbage
        // Collection: "possible roots" buffer).
        if Rc::strong_count(&self.0) > 1 {
            gc::possible_root_ref(self);
        }
    }
}

impl fmt::Debug for PhpRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.val.try_borrow() {
            Ok(v) => write!(f, "&{:?}", *v),
            Err(_) => write!(f, "&<borrowed>"),
        }
    }
}
