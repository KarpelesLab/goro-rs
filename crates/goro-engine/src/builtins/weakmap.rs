//! WeakMap (PHP manual: WeakMap; RFC "WeakMaps"): object keys held weakly;
//! an entry goes away with its key object. Messages and the debug layout
//! from php85 output.

use goro_runtime::{Key, ObjRef, PhpArray, PhpRef, Value, WeakObj};

use crate::class::{NativeHooks, cflags};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "Core",
    classes: &[(
        "weakmap",
        "<?php #[NotSerializable] final class WeakMap implements ArrayAccess, Countable, IteratorAggregate {
            public function offsetGet($object): mixed {}
            public function offsetSet($object, mixed $value): void {}
            public function offsetExists($object): bool {}
            public function offsetUnset($object): void {}
            public function count(): int {}
            public function getIterator(): Iterator { return self::__h_gen($this->__h_entries()); }
            private function __h_entries(): array {}
            private static function __h_gen(array $e): Iterator { foreach ($e as [$k, $v]) { yield $k => $v; } }
        }",
    )],
    methods: &[
        ("weakmap::offsetget", wm_get),
        ("weakmap::offsetset", wm_set),
        ("weakmap::offsetexists", wm_exists),
        ("weakmap::offsetunset", wm_unset),
        ("weakmap::count", wm_count),
        ("weakmap::__h_entries", wm_entries),
    ],
    hooks: &[("weakmap", hooks)],
    listed: false,
    ..Module::EMPTY
};

pub struct WmData {
    /// Insertion-ordered (key, value cell); dead keys are pruned on use.
    entries: Vec<(WeakObj, u32, PhpRef)>,
}

impl goro_runtime::NativeData for WmData {
    // Values are ephemerons of their keys, not traced from the map.
    fn weak_key_dropped(&mut self, key_addr: usize) -> Vec<Value> {
        let mut out = Vec::new();
        self.entries.retain(|(w, _, v)| {
            if w.addr() == key_addr {
                out.push(Value::Ref(v.clone()));
                false
            } else {
                true
            }
        });
        out
    }
    fn ephemerons(&self, key: &ObjRef, visit: &mut dyn FnMut(&Value)) {
        for (w, h, v) in &self.entries {
            if *h == key.handle() && w.upgrade().is_some_and(|k| ObjRef::ptr_eq(&k, key)) {
                visit(&Value::Ref(v.clone()));
            }
        }
    }
    fn clear(&mut self) {
        self.entries.clear();
    }
}

fn with<T>(o: &ObjRef, f: impl FnOnce(&mut WmData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<WmData>().map(f)
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags
                .set(cls.flags.get() | cflags::NO_DYNAMIC | cflags::NOT_SERIALIZABLE);
            *o.obj().native.borrow_mut() = Some(Box::new(WmData {
                entries: Vec::new(),
            }));
        }),
        clone: Some(|_, from, to| {
            let entries = with(from, |d| {
                d.entries
                    .iter()
                    .map(|(w, h, v)| (w.clone(), *h, PhpRef::new(v.get())))
                    .collect()
            })
            .unwrap_or_default();
            *to.obj().native.borrow_mut() = Some(Box::new(WmData { entries }));
        }),
        debug_props: Some(|_, o| {
            let mut out = PhpArray::new();
            for (k, v) in live(o) {
                let mut e = PhpArray::new();
                e.insert(Key::from("key"), Value::Object(k));
                e.insert(Key::from("value"), v.get());
                let _ = out.push(Value::Array(e));
            }
            Some(out)
        }),
        props_for: Some(|_, _| Some(PhpArray::new())),
        count: Some(|_, o| Ok(live(o).len() as i64)),
        ..Default::default()
    }
}

/// The live entries (pruning dead ones).
fn live(o: &ObjRef) -> Vec<(ObjRef, PhpRef)> {
    let dead: Vec<PhpRef>;
    let out = with(o, |d| {
        let mut out = Vec::new();
        let mut gone = Vec::new();
        d.entries.retain(|(w, h, v)| match w.upgrade() {
            Some(k) if k.handle() == *h => {
                out.push((k, v.clone()));
                true
            }
            _ => {
                gone.push(v.clone());
                false
            }
        });
        (out, gone)
    });
    let (out, gone) = out.unwrap_or_default();
    dead = gone;
    drop(dead);
    out
}

fn key_obj(vm: &mut Vm, a: &Args) -> R<ObjRef> {
    match a.val(0) {
        Value::Object(o) => Ok(o),
        Value::Null if a.has(0) => vm.throw("Error", "Cannot append to WeakMap"),
        _ => vm.type_error("WeakMap key must be an object"),
    }
}

fn find(o: &ObjRef, k: &ObjRef) -> Option<PhpRef> {
    live(o)
        .into_iter()
        .find(|(x, _)| ObjRef::ptr_eq(x, k))
        .map(|(_, v)| v)
}

fn wm_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let k = key_obj(vm, a)?;
    match find(&o, &k) {
        // A reference to the stored value: nested writes reach it.
        Some(v) => Ok(Value::Ref(v)),
        None => vm.throw(
            "Error",
            &format!(
                "Object {}#{} not contained in WeakMap",
                vm.class_of(&k).name.to_string_lossy(),
                k.handle()
            ),
        ),
    }
}

fn wm_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let k = key_obj(vm, a)?;
    let v = a.val(1).into_deref();
    match find(&o, &k) {
        Some(cell) => {
            let old = cell.set(v);
            drop(old);
        }
        None => {
            with(&o, |d| {
                d.entries.push((k.downgrade(), k.handle(), PhpRef::new(v)))
            });
            goro_runtime::gc::register_weak_key(&k, &o);
        }
    }
    Ok(Value::Null)
}

fn wm_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let k = key_obj(vm, a)?;
    Ok(Value::Bool(
        find(&o, &k).is_some_and(|v| !v.get().is_null()),
    ))
}

fn wm_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let k = key_obj(vm, a)?;
    let removed = with(&o, |d| {
        let mut out = None;
        d.entries.retain(|(w, h, v)| {
            let hit = *h == k.handle() && w.upgrade().is_some_and(|x| ObjRef::ptr_eq(&x, &k));
            if hit {
                out = Some(v.clone());
            }
            !hit
        });
        out
    });
    drop(removed);
    Ok(Value::Null)
}

fn wm_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(live(a.this_obj()).len() as i64))
}

/// The entries as `[key, value]` pairs (getIterator() yields them).
fn wm_entries(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut flat = PhpArray::new();
    for (k, v) in live(a.this_obj()) {
        let _ = flat.push(Value::Array(PhpArray::from_values([
            Value::Object(k),
            v.get(),
        ])));
    }
    Ok(Value::Array(flat))
}
