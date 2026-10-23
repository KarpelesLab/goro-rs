//! SplHeap / SplMinHeap / SplMaxHeap / SplPriorityQueue
//!
//! Elements are kept in binary-heap order (same layout and sift algorithm as
//! php-src's spl_ptr_heap) in `__spl_array` (values) and, for priority
//! queues, `__spl_priorities` (parallel priorities).

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{mangle_private, obj_class_lower, skey, storage_arr, val_arr};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum HeapKind {
    Max,
    Min,
    /// Abstract SplHeap: user must implement compare().
    Abstract,
    PQueue,
}

const HEAP_CORRUPTED: &[u8] = b"__spl_corrupted";
const HEAP_LOCKED: &[u8] = b"__spl_heap_locked";

impl Vm {
    fn heap_kind_of(base: &[u8]) -> Option<HeapKind> {
        match base {
            b"splmaxheap" => Some(HeapKind::Max),
            b"splminheap" => Some(HeapKind::Min),
            b"splheap" => Some(HeapKind::Abstract),
            b"splpriorityqueue" => Some(HeapKind::PQueue),
            _ => None,
        }
    }

    fn heap_load(obj: &Rc<RefCell<PhpObject>>, pq: bool) -> Vec<(Value, Value)> {
        let ob = obj.borrow();
        let data: Vec<Value> = match ob.get_property(b"__spl_array") {
            Value::Array(a) => a.borrow().values().cloned().collect(),
            _ => Vec::new(),
        };
        if pq {
            let pri: Vec<Value> = match ob.get_property(b"__spl_priorities") {
                Value::Array(a) => a.borrow().values().cloned().collect(),
                _ => Vec::new(),
            };
            let mut pri = pri.into_iter();
            data.into_iter()
                .map(|d| (d, pri.next().unwrap_or(Value::Null)))
                .collect()
        } else {
            data.into_iter().map(|d| (d, Value::Null)).collect()
        }
    }

    fn heap_count(obj: &Rc<RefCell<PhpObject>>) -> usize {
        match obj.borrow().get_property(b"__spl_array") {
            Value::Array(a) => a.borrow().len(),
            _ => 0,
        }
    }

    fn heap_flag(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> bool {
        matches!(obj.borrow().get_property(name), Value::True)
    }

    fn heap_set_flag(obj: &Rc<RefCell<PhpObject>>, name: &[u8], on: bool) {
        obj.borrow_mut()
            .set_property_slice(name, if on { Value::True } else { Value::False });
    }

    /// The heap's get_iterator handlers (foreach, iterator_to_array(),
    /// IteratorIterator...): reading the current element or moving forward
    /// over a corrupted heap throws, unlike the current()/next() methods.
    /// Returns false with the exception set.
    pub(crate) fn heap_iter_check(&mut self, obj: &Rc<RefCell<PhpObject>>, method: &[u8]) -> bool {
        if !(method.eq_ignore_ascii_case(b"current") || method.eq_ignore_ascii_case(b"next"))
            || !Self::heap_flag(obj, HEAP_CORRUPTED)
        {
            return true;
        }
        let cl = obj_class_lower(obj);
        if self
            .spl_find_base(&cl, &[b"splpriorityqueue", b"splheap"])
            .is_none()
        {
            return true;
        }
        // A class overriding an Iterator method iterates through
        // zend_user_it_get_iterator() (the methods).
        let overridden = self.classes.get(&cl).is_some_and(|c| {
            [&b"rewind"[..], b"valid", b"current", b"key", b"next"]
                .iter()
                .any(|m| c.get_method(m).is_some())
        });
        if overridden {
            return true;
        }
        self.spl_throw(
            b"RuntimeException",
            "Heap is corrupted, heap properties are no longer ensured.",
        );
        false
    }

    /// Whether the object's class overrides compare() in userland.
    fn heap_user_compare(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        self.classes
            .get(&cl)
            .map(|c| c.get_method(b"compare").is_some())
            .unwrap_or(false)
    }

    /// Compare two heap elements, given as (data, priority) pairs.
    fn heap_cmp(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        kind: HeapKind,
        user_cmp: bool,
        a: &(Value, Value),
        b: &(Value, Value),
    ) -> i64 {
        if self.current_exception.is_some() {
            return 0;
        }
        let (x, y) = if kind == HeapKind::PQueue {
            (&a.1, &b.1)
        } else {
            (&a.0, &b.0)
        };
        if user_cmp {
            let this = Value::Object(obj.clone());
            let r = self.spl_call_method(&this, b"compare", &[x.clone(), y.clone()]);
            if self.current_exception.is_some() {
                return 0;
            }
            return r.map(|v| v.to_long()).unwrap_or(0).signum();
        }
        match kind {
            HeapKind::Min => self.spl_compare_values(y, x),
            _ => self.spl_compare_values(x, y),
        }
    }

    fn heap_check(&mut self, obj: &Rc<RefCell<PhpObject>>, write: bool) -> bool {
        if Self::heap_flag(obj, HEAP_CORRUPTED) {
            self.spl_throw(
                b"RuntimeException",
                "Heap is corrupted, heap properties are no longer ensured.",
            );
            return false;
        }
        if write && Self::heap_flag(obj, HEAP_LOCKED) {
            self.spl_throw(
                b"RuntimeException",
                "Heap cannot be changed when it is already being modified.",
            );
            return false;
        }
        true
    }

    /// The heap's storage arrays (values, and priorities for a priority
    /// queue), unshared for in-place updates. The priorities are kept
    /// exactly as long as the values.
    fn heap_arrays(
        obj: &Rc<RefCell<PhpObject>>,
        pq: bool,
    ) -> (Rc<RefCell<PhpArray>>, Option<Rc<RefCell<PhpArray>>>) {
        let data = storage_arr(obj, b"__spl_array");
        if !pq {
            return (data, None);
        }
        let pri = storage_arr(obj, b"__spl_priorities");
        let n = data.borrow().len();
        let m = pri.borrow().len();
        if m != n {
            let mut pb = pri.borrow_mut();
            if m > n {
                for _ in n..m {
                    pb.pop();
                }
            } else {
                for _ in m..n {
                    pb.push(Value::Null);
                }
            }
        }
        (data, Some(pri))
    }

    /// Element `i` of the heap as a (data, priority) pair.
    fn heap_elem(
        data: &Rc<RefCell<PhpArray>>,
        pri: &Option<Rc<RefCell<PhpArray>>>,
        i: usize,
    ) -> (Value, Value) {
        let d = data
            .borrow()
            .entry_at(i)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null);
        let p = match pri {
            Some(p) => p
                .borrow()
                .entry_at(i)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Null),
            None => Value::Null,
        };
        (d, p)
    }

    /// Move element `from` to slot `to` (the source slot is left Null).
    fn heap_move(
        data: &Rc<RefCell<PhpArray>>,
        pri: &Option<Rc<RefCell<PhpArray>>>,
        from: usize,
        to: usize,
    ) {
        for a in std::iter::once(data).chain(pri.iter()) {
            let mut ab = a.borrow_mut();
            let v = ab
                .value_at_mut(from)
                .map(std::mem::take)
                .unwrap_or(Value::Null);
            if let Some(slot) = ab.value_at_mut(to) {
                *slot = v;
            }
        }
    }

    fn heap_put(
        data: &Rc<RefCell<PhpArray>>,
        pri: &Option<Rc<RefCell<PhpArray>>>,
        at: usize,
        elem: (Value, Value),
    ) {
        if let Some(slot) = data.borrow_mut().value_at_mut(at) {
            *slot = elem.0;
        }
        if let Some(p) = pri {
            if let Some(slot) = p.borrow_mut().value_at_mut(at) {
                *slot = elem.1;
            }
        }
    }

    /// The top element as a (data, priority) pair.
    fn heap_top(obj: &Rc<RefCell<PhpObject>>, pq: bool) -> Option<(Value, Value)> {
        let ob = obj.borrow();
        let d = match ob.get_property(b"__spl_array") {
            Value::Array(a) => a.borrow().entry_at(0).map(|(_, v)| v.clone())?,
            _ => return None,
        };
        let p = if pq {
            match ob.get_property(b"__spl_priorities") {
                Value::Array(a) => a
                    .borrow()
                    .entry_at(0)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            }
        } else {
            Value::Null
        };
        Some((d, p))
    }

    fn heap_insert(&mut self, obj: &Rc<RefCell<PhpObject>>, kind: HeapKind, elem: (Value, Value)) {
        let pq = kind == HeapKind::PQueue;
        let (data, pri) = Self::heap_arrays(obj, pq);
        let user_cmp = self.heap_user_compare(obj);
        let had_exc = self.current_exception.is_some();
        Self::heap_set_flag(obj, HEAP_LOCKED, true);
        // Sift up. The storage is only updated once the slot is known (a
        // user compare() sees the heap as it was before the insertion).
        let mut pos = data.borrow().len();
        let mut path: Vec<usize> = Vec::new();
        while pos > 0 {
            let parent = (pos - 1) / 2;
            let pe = Self::heap_elem(&data, &pri, parent);
            if self.heap_cmp(obj, kind, user_cmp, &pe, &elem) < 0 {
                path.push(parent);
                pos = parent;
            } else {
                break;
            }
        }
        Self::heap_set_flag(obj, HEAP_LOCKED, false);
        if !had_exc && self.current_exception.is_some() {
            Self::heap_set_flag(obj, HEAP_CORRUPTED, true);
        }
        let mut cur = data.borrow().len();
        data.borrow_mut().push(Value::Null);
        if let Some(p) = &pri {
            p.borrow_mut().push(Value::Null);
        }
        for p in path {
            Self::heap_move(&data, &pri, p, cur);
            cur = p;
        }
        Self::heap_put(&data, &pri, cur, elem);
    }

    /// Remove the top element; returns it as (data, priority), None if empty.
    fn heap_delete_top(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        kind: HeapKind,
    ) -> Option<(Value, Value)> {
        let pq = kind == HeapKind::PQueue;
        let (data, pri) = Self::heap_arrays(obj, pq);
        let n = data.borrow().len();
        if n == 0 {
            return None;
        }
        let user_cmp = self.heap_user_compare(obj);
        let had_exc = self.current_exception.is_some();
        Self::heap_set_flag(obj, HEAP_LOCKED, true);
        let limit = (n - 1) / 2;
        let top = Self::heap_elem(&data, &pri, 0);
        let count = n - 1;
        let bottom = Self::heap_elem(&data, &pri, count);
        // Sift down (the storage is updated afterwards, see heap_insert).
        let mut parent = 0usize;
        let mut path: Vec<usize> = Vec::new();
        while parent < limit {
            let mut child = parent * 2 + 1;
            if child != count {
                let c1 = Self::heap_elem(&data, &pri, child + 1);
                let c0 = Self::heap_elem(&data, &pri, child);
                if self.heap_cmp(obj, kind, user_cmp, &c1, &c0) > 0 {
                    child += 1;
                }
            }
            let ce = Self::heap_elem(&data, &pri, child);
            if self.heap_cmp(obj, kind, user_cmp, &bottom, &ce) < 0 {
                path.push(child);
                parent = child;
            } else {
                break;
            }
        }
        Self::heap_set_flag(obj, HEAP_LOCKED, false);
        if !had_exc && self.current_exception.is_some() {
            Self::heap_set_flag(obj, HEAP_CORRUPTED, true);
        }
        let mut cur = 0usize;
        for c in path {
            Self::heap_move(&data, &pri, c, cur);
            cur = c;
        }
        if cur != count {
            Self::heap_put(&data, &pri, cur, bottom);
        } else {
            drop(bottom);
        }
        let last = data.borrow_mut().pop();
        let last_p = pri.as_ref().and_then(|p| p.borrow_mut().pop());
        drop((last, last_p));
        Some(top)
    }

    fn pq_extract_flags(obj: &Rc<RefCell<PhpObject>>) -> i64 {
        match obj.borrow().get_property(b"__spl_extract_flags") {
            Value::Long(f) => f,
            _ => 1,
        }
    }

    fn pq_extract_value(elem: (Value, Value), flags: i64) -> Value {
        if flags & 3 == 3 {
            let mut a = PhpArray::new();
            a.set(skey(b"data"), elem.0);
            a.set(skey(b"priority"), elem.1);
            val_arr(a)
        } else if flags & 1 != 0 {
            elem.0
        } else {
            elem.1
        }
    }

    pub(crate) fn heap_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>, pq: bool) -> PhpArray {
        let mut out = self.spl_std_props(obj);
        let cls: &[u8] = if pq { b"SplPriorityQueue" } else { b"SplHeap" };
        let flags = if pq { Self::pq_extract_flags(obj) } else { 0 };
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(cls, b"flags"))),
            Value::Long(flags),
        );
        let corrupted = Self::heap_flag(obj, HEAP_CORRUPTED);
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(cls, b"isCorrupted"))),
            if corrupted { Value::True } else { Value::False },
        );
        let mut heap = PhpArray::new();
        for e in Self::heap_load(obj, pq) {
            heap.push(if pq {
                Self::pq_extract_value(e, 3)
            } else {
                e.0
            });
        }
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(cls, b"heap"))),
            val_arr(heap),
        );
        out
    }

    /// Dispatch a heap / priority-queue method. `args` (without $this) is
    /// None on the no-argument dispatch path: methods that need arguments
    /// then return None so the call is retried on the args path.
    pub(crate) fn spl_heap_call(
        &mut self,
        base: &[u8],
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        let kind = Self::heap_kind_of(base)?;
        let pq = kind == HeapKind::PQueue;
        match method {
            b"count" => Some(Value::Long(Self::heap_count(obj) as i64)),
            b"isempty" => Some(if Self::heap_count(obj) == 0 {
                Value::True
            } else {
                Value::False
            }),
            b"key" => Some(Value::Long(Self::heap_count(obj) as i64 - 1)),
            b"valid" => Some(if Self::heap_count(obj) != 0 {
                Value::True
            } else {
                Value::False
            }),
            b"rewind" => Some(Value::Null),
            b"recoverfromcorruption" => {
                Self::heap_set_flag(obj, HEAP_CORRUPTED, false);
                Some(Value::True)
            }
            b"iscorrupted" => Some(if Self::heap_flag(obj, HEAP_CORRUPTED) {
                Value::True
            } else {
                Value::False
            }),
            b"next" => {
                self.heap_delete_top(obj, kind);
                Some(Value::Null)
            }
            b"current" => match Self::heap_top(obj, pq) {
                None => Some(Value::Null),
                Some(e) => Some(if pq {
                    Self::pq_extract_value(e, Self::pq_extract_flags(obj))
                } else {
                    e.0
                }),
            },
            b"top" => {
                if !self.heap_check(obj, false) {
                    return Some(Value::Null);
                }
                match Self::heap_top(obj, pq) {
                    None => self.spl_throw(b"RuntimeException", "Can't peek at an empty heap"),
                    Some(e) => Some(if pq {
                        Self::pq_extract_value(e, Self::pq_extract_flags(obj))
                    } else {
                        e.0
                    }),
                }
            }
            b"extract" => {
                if !self.heap_check(obj, true) {
                    return Some(Value::Null);
                }
                match self.heap_delete_top(obj, kind) {
                    None => self.spl_throw(b"RuntimeException", "Can't extract from an empty heap"),
                    Some(e) => Some(if pq {
                        Self::pq_extract_value(e, Self::pq_extract_flags(obj))
                    } else {
                        e.0
                    }),
                }
            }
            b"insert" => {
                let args = args?;
                if !self.heap_check(obj, true) {
                    return Some(Value::Null);
                }
                let d = args.first().map_or(Value::Null, Value::deref_for_storage);
                let p = if pq {
                    args.get(1).map_or(Value::Null, Value::deref_for_storage)
                } else {
                    Value::Null
                };
                self.heap_insert(obj, kind, (d, p));
                Some(Value::True)
            }
            b"compare" => {
                let args = args?;
                let a = args.first().cloned().unwrap_or(Value::Null);
                let b = args.get(1).cloned().unwrap_or(Value::Null);
                let r = match kind {
                    HeapKind::Min => self.spl_compare_values(&b, &a),
                    _ => self.spl_compare_values(&a, &b),
                };
                Some(Value::Long(r))
            }
            b"setextractflags" if pq => {
                let args = args?;
                let v = args.first().map(|v| v.to_long()).unwrap_or(0) & 3;
                if v == 0 {
                    return self.spl_throw(
                        b"RuntimeException",
                        "Must specify at least one extract flag",
                    );
                }
                obj.borrow_mut()
                    .set_property_slice(b"__spl_extract_flags", Value::Long(v));
                Some(Value::Long(v))
            }
            b"getextractflags" if pq => Some(Value::Long(Self::pq_extract_flags(obj))),
            b"__debuginfo" => Some(val_arr(self.heap_debug_info(obj, pq))),
            b"__serialize" => {
                if !self.heap_check(obj, false) {
                    return Some(Value::Null);
                }
                if Self::heap_flag(obj, HEAP_LOCKED) {
                    return self.spl_throw(
                        b"RuntimeException",
                        "Cannot serialize heap while it is being modified.",
                    );
                }
                let mut result = PhpArray::new();
                result.push(val_arr(self.spl_std_props(obj)));
                let mut state = PhpArray::new();
                state.set(
                    skey(b"flags"),
                    Value::Long(if pq { Self::pq_extract_flags(obj) } else { 0 }),
                );
                let mut elems = PhpArray::new();
                for e in Self::heap_load(obj, pq) {
                    elems.push(if pq {
                        Self::pq_extract_value(e, 3)
                    } else {
                        e.0
                    });
                }
                state.set(skey(b"heap_elements"), val_arr(elems));
                result.push(val_arr(state));
                Some(val_arr(result))
            }
            b"__unserialize" => {
                let args = args?;
                if !pq && !self.heap_check(obj, true) {
                    return Some(Value::Null);
                }
                let class_disp =
                    crate::value::display_class_name(&obj.borrow().class_name).to_string();
                let invalid = format!("Invalid serialization data for {} object", class_disp);
                let data = match args.first().cloned().map(|v| v.deref()) {
                    Some(Value::Array(a)) => a.borrow().clone(),
                    _ => PhpArray::new(),
                };
                if data.len() != 2 {
                    return self.spl_throw(b"Exception", &invalid);
                }
                let props = match data.get(&ArrayKey::Int(0)) {
                    Some(Value::Array(p)) => p.borrow().clone(),
                    _ => return self.spl_throw(b"Exception", &invalid),
                };
                self.spl_load_props(obj, &props);
                let state = match data.get(&ArrayKey::Int(1)) {
                    Some(Value::Array(s)) => s.borrow().clone(),
                    _ => return self.spl_throw(b"Exception", &invalid),
                };
                let flags = match state.get(&skey(b"flags")) {
                    Some(Value::Long(f)) => *f,
                    _ => return self.spl_throw(b"Exception", &invalid),
                };
                if pq {
                    let f = flags & 3;
                    if f == 0 {
                        return self.spl_throw(b"Exception", &invalid);
                    }
                    obj.borrow_mut()
                        .set_property_slice(b"__spl_extract_flags", Value::Long(f));
                } else if flags != 0 {
                    return self.spl_throw(b"Exception", &invalid);
                }
                let elems = match state.get(&skey(b"heap_elements")) {
                    Some(Value::Array(e)) => e.borrow().clone(),
                    _ => return self.spl_throw(b"Exception", &invalid),
                };
                for (_, v) in elems.iter() {
                    let e = if pq {
                        let pair = match v {
                            Value::Array(p) if p.borrow().len() == 2 => p.borrow().clone(),
                            _ => return self.spl_throw(b"Exception", &invalid),
                        };
                        match (
                            pair.get(&skey(b"data")).cloned(),
                            pair.get(&skey(b"priority")).cloned(),
                        ) {
                            (Some(d), Some(p)) => (d, p),
                            _ => return self.spl_throw(b"Exception", &invalid),
                        }
                    } else {
                        (v.clone(), Value::Null)
                    };
                    self.heap_insert(obj, kind, e);
                    if self.current_exception.is_some() {
                        self.current_exception = None;
                        return self.spl_throw(b"Exception", &invalid);
                    }
                }
                if !self.heap_check(obj, false) {
                    return Some(Value::Null);
                }
                Some(Value::Null)
            }
            _ => None,
        }
    }
}
