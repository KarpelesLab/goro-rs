//! SplDoublyLinkedList / SplQueue / SplStack (php-src ext/spl/spl_dllist.c).
//!
//! Elements live in `__spl_array` in list order, each keyed by a stable
//! node id (allocated from `__spl_dll_nid`, never reused), so traversal
//! pointers survive insertions/removals like php-src's linked list nodes
//! and every end operation is O(1) (amortized; see PhpArray::push_front).
//! The object's own traversal state is `__spl_dll_cur` (node id or Null) /
//! `__spl_dll_tpos`; foreach uses an independent InternalIterator.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{mangle_private, obj_class_lower, storage_arr, val_arr};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

const IT_DELETE: i64 = 1;
const IT_LIFO: i64 = 2;
const IT_FIX: i64 = 4;
const IT_MASK: i64 = 3;

fn prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Value {
    obj.borrow().get_property(name)
}

fn set_prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8], v: Value) {
    obj.borrow_mut().set_property_slice(name, v);
}

/// The element storage, for reading (None: no storage yet).
fn list(obj: &Rc<RefCell<PhpObject>>) -> Option<Rc<RefCell<PhpArray>>> {
    match prop(obj, b"__spl_array") {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// The element storage, unshared, for in-place updates. Storage filled
/// elsewhere (keys that are not node ids) is re-keyed first.
fn list_mut(obj: &Rc<RefCell<PhpObject>>) -> Rc<RefCell<PhpArray>> {
    let a = storage_arr(obj, b"__spl_array");
    if !obj.borrow().has_property(b"__spl_dll_nid") && !a.borrow().is_empty() {
        let vals: Vec<Value> = a.borrow().values().cloned().collect();
        let mut b = PhpArray::new();
        for v in vals {
            b.set(ArrayKey::Int(new_id(obj)), v);
        }
        *a.borrow_mut() = b;
    }
    a
}

fn len(obj: &Rc<RefCell<PhpObject>>) -> usize {
    list(obj).map(|a| a.borrow().len()).unwrap_or(0)
}

/// Node id of the element at list position `i`.
fn id_at(a: &Rc<RefCell<PhpArray>>, i: usize) -> Option<i64> {
    match a.borrow().entry_at(i) {
        Some((ArrayKey::Int(n), _)) => Some(*n),
        _ => None,
    }
}

fn value_at(obj: &Rc<RefCell<PhpObject>>, i: usize) -> Option<Value> {
    let a = list(obj)?;
    let ab = a.borrow();
    ab.entry_at(i).map(|(_, v)| v.clone())
}

/// List position of node `id` (None once the node was removed).
fn pos_of(a: &Rc<RefCell<PhpArray>>, id: i64) -> Option<usize> {
    a.borrow().position_of(&ArrayKey::Int(id))
}

fn values(obj: &Rc<RefCell<PhpObject>>) -> Vec<Value> {
    match list(obj) {
        Some(a) => a.borrow().values().cloned().collect(),
        None => Vec::new(),
    }
}

fn push_back(obj: &Rc<RefCell<PhpObject>>, v: Value) {
    let a = list_mut(obj);
    let id = new_id(obj);
    a.borrow_mut().set(ArrayKey::Int(id), v);
}

/// Remove the first (or last) element, handing it to the caller.
fn take_end(obj: &Rc<RefCell<PhpObject>>, last: bool) -> Option<Value> {
    let a = list_mut(obj);
    let mut ab = a.borrow_mut();
    if last {
        ab.pop()
    } else {
        let k = ab.entry_at(0).map(|(k, _)| k.clone())?;
        ab.take(&k)
    }
}

fn new_id(obj: &Rc<RefCell<PhpObject>>) -> i64 {
    let n = match prop(obj, b"__spl_dll_nid") {
        Value::Long(n) => n,
        _ => 1,
    };
    set_prop(obj, b"__spl_dll_nid", Value::Long(n + 1));
    n
}

pub(crate) fn dll_flags(vm: &Vm, obj: &Rc<RefCell<PhpObject>>) -> i64 {
    match prop(obj, b"__spl_iter_mode") {
        Value::Long(f) => f,
        _ => {
            let cl = obj_class_lower(obj);
            match vm.spl_find_base(&cl, &[b"splstack", b"splqueue", b"spldoublylinkedlist"]) {
                Some(b"splstack") => IT_FIX | IT_LIFO,
                Some(b"splqueue") => IT_FIX,
                _ => 0,
            }
        }
    }
}

/// A traversal cursor: (current node id, position).
#[derive(Clone, Copy)]
pub(crate) struct Cursor {
    pub cur: Option<i64>,
    pub pos: i64,
}

impl Vm {
    fn dll_rewind_cursor(obj: &Rc<RefCell<PhpObject>>, flags: i64) -> Cursor {
        let a = match list(obj) {
            Some(a) => a,
            None => return Cursor { cur: None, pos: 0 },
        };
        let n = a.borrow().len();
        if flags & IT_LIFO != 0 {
            Cursor {
                cur: if n > 0 { id_at(&a, n - 1) } else { None },
                pos: n as i64 - 1,
            }
        } else {
            Cursor {
                cur: id_at(&a, 0),
                pos: 0,
            }
        }
    }

    /// spl_dllist_it_helper_move_forward()
    fn dll_move_forward(&mut self, obj: &Rc<RefCell<PhpObject>>, c: Cursor, flags: i64) -> Cursor {
        let cur = match c.cur {
            Some(id) => id,
            None => return c,
        };
        let a = list_mut(obj);
        let lifo = flags & IT_LIFO != 0;
        let next = match pos_of(&a, cur) {
            Some(i) => {
                if lifo {
                    if i > 0 { id_at(&a, i - 1) } else { None }
                } else {
                    id_at(&a, i + 1)
                }
            }
            None => None, // node was removed: its links are cut
        };
        let mut pos = c.pos;
        if lifo {
            pos -= 1;
            if flags & IT_DELETE != 0 {
                let v = take_end(obj, true);
                drop(v);
            }
        } else if flags & IT_DELETE != 0 {
            let v = take_end(obj, false);
            drop(v);
        } else {
            pos += 1;
        }
        Cursor { cur: next, pos }
    }

    fn dll_cursor_value(obj: &Rc<RefCell<PhpObject>>, c: Cursor) -> Option<Value> {
        let id = c.cur?;
        let a = list(obj)?;
        let ab = a.borrow();
        ab.get(&ArrayKey::Int(id)).cloned()
    }

    fn dll_obj_cursor(obj: &Rc<RefCell<PhpObject>>) -> Cursor {
        let cur = match prop(obj, b"__spl_dll_cur") {
            Value::Long(n) => Some(n),
            _ => None,
        };
        let pos = match prop(obj, b"__spl_dll_tpos") {
            Value::Long(n) => n,
            _ => 0,
        };
        Cursor { cur, pos }
    }

    fn dll_set_obj_cursor(obj: &Rc<RefCell<PhpObject>>, c: Cursor) {
        set_prop(
            obj,
            b"__spl_dll_cur",
            match c.cur {
                Some(n) => Value::Long(n),
                None => Value::Null,
            },
        );
        set_prop(obj, b"__spl_dll_tpos", Value::Long(c.pos));
    }

    /// spl_ptr_llist_offset(): element index for a user offset.
    fn dll_offset_index(len: usize, index: i64, backward: bool) -> Option<usize> {
        if index < 0 || index as usize >= len {
            return None;
        }
        Some(if backward {
            len - 1 - index as usize
        } else {
            index as usize
        })
    }

    pub(crate) fn dll_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        let mut out = self.spl_std_props(obj);
        let flags = dll_flags(self, obj);
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(
                b"SplDoublyLinkedList",
                b"flags",
            ))),
            Value::Long(flags),
        );
        let mut a = PhpArray::new();
        for v in values(obj) {
            a.push(v);
        }
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(
                b"SplDoublyLinkedList",
                b"dllist",
            ))),
            val_arr(a),
        );
        out
    }

    fn dll_index_arg(&mut self, func: &str, v: Option<&Value>) -> Option<i64> {
        let v = v.cloned().unwrap_or(Value::Null);
        self.spl_arg_long(func, 1, "index", &v)
    }

    fn dll_out_of_range(&mut self, func: &str) -> Option<Value> {
        let msg = format!("{}(): Argument #1 ($index) is out of range", func);
        self.spl_throw(b"OutOfRangeException", &msg)
    }

    /// Dispatch an SplDoublyLinkedList method. `args` excludes $this; None on
    /// the no-argument path.
    pub(crate) fn spl_dll_call(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        // Serializable::unserialize(): one var_hash for all the pieces.
        if method == b"unserialize" {
            if let Some(h) = self.unserialize_scope_hook {
                h(self, true);
                let r = self.spl_dll_call_impl(method, obj, args);
                h(self, false);
                return r;
            }
        }
        self.spl_dll_call_impl(method, obj, args)
    }

    fn spl_dll_call_impl(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        let needs_args = matches!(
            method,
            b"push"
                | b"unshift"
                | b"enqueue"
                | b"setiteratormode"
                | b"offsetexists"
                | b"offsetget"
                | b"offsetset"
                | b"offsetunset"
                | b"add"
                | b"unserialize"
                | b"__unserialize"
        );
        if needs_args && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        let r = match method {
            b"push" | b"enqueue" => {
                let v = a.first().map_or(Value::Null, Value::deref_for_storage);
                push_back(obj, v);
                Value::Null
            }
            b"unshift" => {
                let v = a.first().map_or(Value::Null, Value::deref_for_storage);
                let l = list_mut(obj);
                let id = new_id(obj);
                l.borrow_mut().push_front(ArrayKey::Int(id), v);
                Value::Null
            }
            b"pop" => {
                match if len(obj) > 0 {
                    take_end(obj, true)
                } else {
                    None
                } {
                    Some(v) => v,
                    None => {
                        return self.spl_throw(
                            b"RuntimeException",
                            "Can't pop from an empty datastructure",
                        );
                    }
                }
            }
            b"shift" | b"dequeue" => {
                if len(obj) == 0 {
                    return self.spl_throw(
                        b"RuntimeException",
                        "Can't shift from an empty datastructure",
                    );
                }
                take_end(obj, false).unwrap_or(Value::Null)
            }
            b"top" => {
                let n = len(obj);
                match if n > 0 { value_at(obj, n - 1) } else { None } {
                    Some(v) => v,
                    None => {
                        return self.spl_throw(
                            b"RuntimeException",
                            "Can't peek at an empty datastructure",
                        );
                    }
                }
            }
            b"bottom" => match value_at(obj, 0) {
                Some(v) => v,
                None => {
                    return self
                        .spl_throw(b"RuntimeException", "Can't peek at an empty datastructure");
                }
            },
            b"count" => Value::Long(len(obj) as i64),
            b"isempty" => {
                // spl_dllist_object_count_elements: honours a count() override
                let cl = obj_class_lower(obj);
                let user_count = self
                    .classes
                    .get(&cl)
                    .map(|c| c.get_method(b"count").is_some())
                    .unwrap_or(false);
                let n = if user_count {
                    self.call_object_method(&Value::Object(obj.clone()), b"count", &[])
                        .map(|v| v.to_long())
                        .unwrap_or(0)
                } else {
                    len(obj) as i64
                };
                if n == 0 { Value::True } else { Value::False }
            }
            b"setiteratormode" => {
                let v = match a.first() {
                    Some(v) => match self.spl_arg_long(
                        "SplDoublyLinkedList::setIteratorMode",
                        1,
                        "mode",
                        v,
                    ) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                let flags = dll_flags(self, obj);
                if flags & IT_FIX != 0 && (flags & IT_LIFO) != (v & IT_LIFO) {
                    return self.spl_throw(
                        b"RuntimeException",
                        "Iterators' LIFO/FIFO modes for SplStack/SplQueue objects are frozen",
                    );
                }
                let nf = (v & IT_MASK) | (flags & IT_FIX);
                set_prop(obj, b"__spl_iter_mode", Value::Long(nf));
                Value::Long(nf)
            }
            b"getiteratormode" => Value::Long(dll_flags(self, obj)),
            b"offsetexists" => {
                let i = match self.dll_index_arg("SplDoublyLinkedList::offsetExists", a.first()) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let n = len(obj) as i64;
                if i >= 0 && i < n {
                    Value::True
                } else {
                    Value::False
                }
            }
            b"offsetget" => {
                let func = "SplDoublyLinkedList::offsetGet";
                let i = match self.dll_index_arg(func, a.first()) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                match Self::dll_offset_index(len(obj), i, lifo).and_then(|idx| value_at(obj, idx)) {
                    Some(v) => v,
                    None => return self.dll_out_of_range(func),
                }
            }
            b"offsetset" => {
                let func = "SplDoublyLinkedList::offsetSet";
                let v = a.get(1).map_or(Value::Null, Value::deref_for_storage);
                let idx_v = a.first().cloned().unwrap_or(Value::Null).deref();
                if matches!(idx_v, Value::Null | Value::Undef) {
                    push_back(obj, v);
                    return Some(Value::Null);
                }
                let i = match self.spl_arg_long(func, 1, "index", &idx_v) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                match Self::dll_offset_index(len(obj), i, lifo) {
                    Some(idx) => {
                        let l = list_mut(obj);
                        let old = l
                            .borrow_mut()
                            .value_at_mut(idx)
                            .map(|slot| std::mem::replace(slot, v));
                        if let Some(o) = &old {
                            crate::gc::on_child_released(o);
                        }
                        drop(old);
                        Value::Null
                    }
                    None => return self.dll_out_of_range(func),
                }
            }
            b"offsetunset" => {
                let func = "SplDoublyLinkedList::offsetUnset";
                let i = match self.dll_index_arg(func, a.first()) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                match Self::dll_offset_index(len(obj), i, lifo) {
                    Some(idx) => {
                        let l = list_mut(obj);
                        let removed_id = id_at(&l, idx).unwrap_or(0);
                        let old = l.borrow_mut().remove(&ArrayKey::Int(removed_id));
                        let c = Self::dll_obj_cursor(obj);
                        if c.cur == Some(removed_id) {
                            Self::dll_set_obj_cursor(
                                obj,
                                Cursor {
                                    cur: None,
                                    pos: c.pos,
                                },
                            );
                        }
                        drop(old);
                        Value::Null
                    }
                    None => return self.dll_out_of_range(func),
                }
            }
            b"add" => {
                let func = "SplDoublyLinkedList::add";
                let i = match self.dll_index_arg(func, a.first()) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let v = a.get(1).map_or(Value::Null, Value::deref_for_storage);
                let n = len(obj);
                if i < 0 || i as usize > n {
                    return self.dll_out_of_range(func);
                }
                if i as usize == n {
                    push_back(obj, v);
                } else {
                    let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                    let at = Self::dll_offset_index(n, i, lifo).unwrap_or(0);
                    let l = list_mut(obj);
                    let id = new_id(obj);
                    l.borrow_mut().insert_at(at, ArrayKey::Int(id), v);
                }
                Value::Null
            }
            b"rewind" => {
                let c = Self::dll_rewind_cursor(obj, dll_flags(self, obj));
                Self::dll_set_obj_cursor(obj, c);
                Value::Null
            }
            b"valid" => {
                let c = Self::dll_obj_cursor(obj);
                if c.cur.is_some() && Self::dll_cursor_value(obj, c).is_some() {
                    Value::True
                } else {
                    Value::False
                }
            }
            b"current" => {
                Self::dll_cursor_value(obj, Self::dll_obj_cursor(obj)).unwrap_or(Value::Null)
            }
            b"key" => Value::Long(Self::dll_obj_cursor(obj).pos),
            b"next" => {
                let c = Self::dll_obj_cursor(obj);
                let f = dll_flags(self, obj);
                let n = self.dll_move_forward(obj, c, f);
                Self::dll_set_obj_cursor(obj, n);
                Value::Null
            }
            b"prev" => {
                let c = Self::dll_obj_cursor(obj);
                let f = dll_flags(self, obj) ^ IT_LIFO;
                let n = self.dll_move_forward(obj, c, f);
                Self::dll_set_obj_cursor(obj, n);
                Value::Null
            }
            b"__debuginfo" => val_arr(self.dll_debug_info(obj)),
            b"serialize" => {
                let mut out: Vec<u8> = Vec::new();
                // (one var_hash for all the pieces, the flags included)
                let scope = self.serialize_scope_hook;
                if let Some(h) = scope {
                    h(true);
                }
                let f = self
                    .spl_call_builtin(b"serialize", &[Value::Long(dll_flags(self, obj))])
                    .unwrap_or(Value::Null);
                out.extend_from_slice(f.to_php_string().as_bytes());
                // Walk the live list node by node: serializing an element may
                // modify the list (only nodes still linked are visited).
                let mut cur = list(obj).and_then(|a| id_at(&a, 0));
                while let Some(id) = cur {
                    let v =
                        match list(obj).and_then(|a| a.borrow().get(&ArrayKey::Int(id)).cloned()) {
                            Some(v) => v,
                            None => break,
                        };
                    out.push(b':');
                    let s = self
                        .spl_call_builtin(b"serialize", &[v])
                        .unwrap_or(Value::Null);
                    out.extend_from_slice(s.to_php_string().as_bytes());
                    if self.current_exception.is_some() {
                        break;
                    }
                    cur = list(obj).and_then(|a| pos_of(&a, id).and_then(|p| id_at(&a, p + 1)));
                }
                if let Some(h) = scope {
                    h(false);
                }
                Value::String(PhpString::from_vec(out))
            }
            b"unserialize" => {
                let data = match a.first() {
                    Some(v) => {
                        match self.spl_arg_string("SplDoublyLinkedList::unserialize", 1, "data", v)
                        {
                            Some(s) => s.as_bytes().to_vec(),
                            None => return Some(Value::Null),
                        }
                    }
                    None => Vec::new(),
                };
                if data.is_empty() {
                    return Some(Value::Null);
                }
                set_prop(obj, b"__spl_array", val_arr(PhpArray::new()));
                let mut p = 0usize;
                let flags = self.spl_unserialize_at(&data, &mut p);
                match flags {
                    Some(Value::Long(f)) => set_prop(obj, b"__spl_iter_mode", Value::Long(f)),
                    _ => {
                        let msg = format!("Error at offset {} of {} bytes", p, data.len());
                        return self.spl_throw(b"UnexpectedValueException", &msg);
                    }
                }
                while p < data.len() && data[p] == b':' {
                    p += 1;
                    match self.spl_unserialize_at(&data, &mut p) {
                        Some(v) => push_back(obj, v),
                        None => {
                            let msg = format!("Error at offset {} of {} bytes", p, data.len());
                            return self.spl_throw(b"UnexpectedValueException", &msg);
                        }
                    }
                }
                if p != data.len() {
                    let msg = format!("Error at offset {} of {} bytes", p, data.len());
                    return self.spl_throw(b"UnexpectedValueException", &msg);
                }
                Value::Null
            }
            b"__serialize" => {
                let mut out = PhpArray::new();
                out.push(Value::Long(dll_flags(self, obj)));
                let mut el = PhpArray::new();
                for v in values(obj) {
                    el.push(v);
                }
                out.push(val_arr(el));
                out.push(val_arr(self.spl_std_props(obj)));
                val_arr(out)
            }
            b"__unserialize" => {
                let data = match a.first().map(|v| v.deref()) {
                    Some(Value::Array(d)) => d.borrow().clone(),
                    _ => PhpArray::new(),
                };
                let flags = data.get(&ArrayKey::Int(0)).cloned();
                let storage = data.get(&ArrayKey::Int(1)).cloned();
                let members = data.get(&ArrayKey::Int(2)).cloned();
                match (flags, storage, members) {
                    (Some(Value::Long(f)), Some(Value::Array(st)), Some(Value::Array(m))) => {
                        set_prop(obj, b"__spl_iter_mode", Value::Long(f));
                        let st: Vec<Value> = st.borrow().values().cloned().collect();
                        for v in st {
                            push_back(obj, v);
                        }
                        let m = m.borrow().clone();
                        self.spl_load_props(obj, &m);
                        Value::Null
                    }
                    _ => {
                        return self.spl_throw(
                            b"UnexpectedValueException",
                            "Incomplete or ill-typed serialization data",
                        );
                    }
                }
            }
            _ => return None,
        };
        Some(r)
    }

    /// Unserialize one value at `pos` (hook into ext/standard).
    pub(crate) fn spl_unserialize_at(&mut self, data: &[u8], pos: &mut usize) -> Option<Value> {
        let f = self.unserialize_value_hook?;
        f(self, data, pos)
    }

    /// Foreach over a DLL whose class does not override the iterator
    /// methods: an independent cursor (spl_dllist_get_iterator).
    /// (The handle foreach takes for the iterator is allocated by foreach.)
    pub(crate) fn dll_new_iterator(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Value {
        self.dll_new_iterator_at(obj, false)
    }

    /// spl_dllist_get_iterator(); `handle`: the iterator object takes an
    /// object handle itself.
    pub fn dll_new_iterator_at(&mut self, obj: &Rc<RefCell<PhpObject>>, handle: bool) -> Value {
        let id = if handle { self.alloc_object_id() } else { 0 };
        let it = Rc::new(RefCell::new(PhpObject::new(
            b"InternalIterator".to_vec(),
            id,
        )));
        let c = Self::dll_obj_cursor(obj);
        let flags = dll_flags(self, obj) & IT_MASK;
        {
            let mut ob = it.borrow_mut();
            ob.set_property_slice(b"__spl_dll_target", Value::Object(obj.clone()));
            ob.set_property_slice(
                b"__spl_dll_cur",
                match c.cur {
                    Some(n) => Value::Long(n),
                    None => Value::Null,
                },
            );
            ob.set_property_slice(b"__spl_dll_tpos", Value::Long(c.pos));
            ob.set_property_slice(b"__spl_iter_mode", Value::Long(flags));
        }
        Value::Object(it)
    }

    /// InternalIterator methods for the DLL kind.
    pub(crate) fn dll_iterator_call(
        &mut self,
        method: &[u8],
        it: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let target = match prop(it, b"__spl_dll_target") {
            Value::Object(o) => o,
            _ => return None,
        };
        let flags = match prop(it, b"__spl_iter_mode") {
            Value::Long(f) => f,
            _ => 0,
        };
        let c = Self::dll_obj_cursor(it);
        let r = match method {
            b"rewind" => {
                let c = Self::dll_rewind_cursor(&target, flags);
                Self::dll_set_obj_cursor(it, c);
                Value::Null
            }
            b"valid" => {
                if c.cur.is_some() && Self::dll_cursor_value(&target, c).is_some() {
                    Value::True
                } else {
                    Value::False
                }
            }
            b"current" => Self::dll_cursor_value(&target, c).unwrap_or(Value::Null),
            b"key" => Value::Long(c.pos),
            b"next" => {
                let n = self.dll_move_forward(&target, c, flags);
                Self::dll_set_obj_cursor(it, n);
                Value::Null
            }
            _ => return None,
        };
        Some(r)
    }

    /// Whether foreach should use the internal DLL iterator for this object.
    pub fn dll_uses_internal_iterator(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        if self.spl_find_base(&cl, &[b"spldoublylinkedlist"]).is_none() {
            return false;
        }
        match self.classes.get(&cl) {
            None => true,
            Some(c) => !["rewind", "valid", "key", "current", "next"]
                .iter()
                .any(|m| c.get_method(m.as_bytes()).is_some()),
        }
    }
}

/// An object's `__spl_array` storage as other code expects to see it: for
/// a doubly linked list (keyed by node id) or an SplObjectStorage (keyed by
/// object hash) a copy keyed by position.
/// An ArrayObject/ArrayIterator or SplObjectStorage, whose compare handler
/// compares the storage (spl_array_compare_objects(),
/// spl_object_storage_compare_objects()); lists, heaps and fixed arrays
/// have none and compare by their real properties only.
pub fn is_spl_array_object(ob: &PhpObject) -> bool {
    ob.has_property(b"__spl_array")
        && (ob.has_property(b"__spl_flags") || ob.has_property(b"__spl_objects"))
}

pub fn spl_storage_view(ob: &PhpObject) -> Value {
    let v = ob.get_property(b"__spl_array");
    match &v {
        Value::Array(a)
            if ob.has_property(b"__spl_dll_nid") || ob.has_property(b"__spl_objects") =>
        {
            let mut l = PhpArray::new();
            for x in a.borrow().values() {
                l.push(x.clone());
            }
            val_arr(l)
        }
        _ => v,
    }
}
