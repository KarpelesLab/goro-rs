//! Weak targets: the objects a WeakReference refers to or a WeakMap is keyed
//! by. A Generator is an object in PHP but its own Value variant here, so
//! both kinds are held behind one enum.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::generator::PhpGenerator;
use crate::object::PhpObject;
use crate::value::Value;
use crate::vm::Vm;

#[derive(Clone, Debug)]
pub enum WeakTarget {
    Object(Weak<RefCell<PhpObject>>),
    Generator(Weak<RefCell<PhpGenerator>>),
}

impl WeakTarget {
    /// The target as a value again, while it is alive.
    pub fn upgrade(&self) -> Option<Value> {
        match self {
            WeakTarget::Object(w) => w.upgrade().map(Value::Object),
            WeakTarget::Generator(w) => w.upgrade().map(Value::Generator),
        }
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        match self {
            WeakTarget::Object(w) => w.strong_count() > 0,
            WeakTarget::Generator(w) => w.strong_count() > 0,
        }
    }

    /// Address of the target's allocation (what the cycle collector keys
    /// its nodes by).
    #[inline]
    pub fn as_ptr(&self) -> usize {
        match self {
            WeakTarget::Object(w) => w.as_ptr() as *const u8 as usize,
            WeakTarget::Generator(w) => w.as_ptr() as *const u8 as usize,
        }
    }

    /// Flag the target as a WeakMap key, so its release purges the entries
    /// keyed by it.
    pub fn mark_key(&self) {
        match self {
            WeakTarget::Object(w) => {
                if let Some(o) = w.upgrade() {
                    if let Ok(b) = o.try_borrow() {
                        b.weak_key.set(true);
                    }
                }
            }
            WeakTarget::Generator(w) => {
                if let Some(g) = w.upgrade() {
                    // (a running generator is borrowed by its executor; its
                    // entries are then dropped by the next purge instead)
                    if let Ok(b) = g.try_borrow() {
                        b.weak_key.set(true);
                    }
                }
            }
        }
    }
}

impl Vm {
    /// The handle and a weak pointer of `v` when it is an object (Generator
    /// included): a valid WeakMap key / WeakReference target.
    pub fn weak_target_of(&self, v: &Value) -> Option<(u64, WeakTarget)> {
        match v {
            Value::Object(o) => Some((o.borrow().object_id, WeakTarget::Object(Rc::downgrade(o)))),
            Value::Generator(g) => Some((
                self.generator_handle(g),
                WeakTarget::Generator(Rc::downgrade(g)),
            )),
            Value::Reference(r) => {
                let inner = r.borrow().clone();
                self.weak_target_of(&inner)
            }
            _ => None,
        }
    }

    /// "Object Foo#3 not contained in WeakMap" (an Error) for key `key`.
    #[inline(never)]
    pub(crate) fn weakmap_missing_msg(&self, key: &Value, key_oid: u64) -> String {
        let class = match key.deref() {
            Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name),
            _ => "Generator".to_string(),
        };
        format!("Object {}#{} not contained in WeakMap", class, key_oid)
    }

    /// The value stored under the live key `key_oid` (dereferenced).
    pub(crate) fn weakmap_get(&self, wm_id: u64, key_oid: u64) -> Option<Value> {
        self.weak_map_entries.get(&wm_id).and_then(|entries| {
            entries
                .iter()
                .find(|(oid, weak, _)| *oid == key_oid && weak.is_alive())
                .map(|(_, _, v)| v.deref())
        })
    }

    /// offsetSet(): store `val` under the key.
    pub(crate) fn weakmap_set(&mut self, wm_id: u64, key_oid: u64, weak: WeakTarget, val: Value) {
        weak.mark_key();
        let entries = self.weak_map_entries.entry(wm_id).or_default();
        if let Some(entry) = entries.iter_mut().find(|(oid, _, _)| *oid == key_oid) {
            entry.1 = weak;
            entry.2 = val;
        } else {
            entries.push((key_oid, weak, val));
        }
    }

    /// offsetExists(): the key is present with a non-null value.
    pub(crate) fn weakmap_exists(&self, wm_id: u64, key_oid: u64) -> bool {
        self.weak_map_entries.get(&wm_id).is_some_and(|entries| {
            entries.iter().any(|(oid, weak, val)| {
                *oid == key_oid && weak.is_alive() && !matches!(val.deref(), Value::Null)
            })
        })
    }

    /// offsetUnset()
    pub(crate) fn weakmap_unset(&mut self, wm_id: u64, key_oid: u64) {
        if let Some(entries) = self.weak_map_entries.get_mut(&wm_id) {
            entries.retain(|(oid, _, _)| *oid != key_oid);
        }
    }

    /// count(): the live entries.
    pub fn weakmap_count(&self, wm_id: u64) -> usize {
        self.weak_map_entries.get(&wm_id).map_or(0, |entries| {
            entries.iter().filter(|(_, w, _)| w.is_alive()).count()
        })
    }

    /// The live (key, value) pairs, in insertion order.
    pub fn weakmap_live_entries(&self, wm_id: u64) -> Vec<(Value, Value)> {
        self.weak_map_entries
            .get(&wm_id)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|(_, w, v)| w.upgrade().map(|k| (k, v.clone())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The target of the WeakReference `weak_id`, or NULL once it died.
    pub fn weakref_target(&self, weak_id: u64) -> Value {
        self.weak_refs
            .get(&weak_id)
            .and_then(|w| w.upgrade())
            .unwrap_or(Value::Null)
    }
}
