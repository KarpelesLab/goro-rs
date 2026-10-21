//! SplDoublyLinkedList / SplQueue / SplStack (php-src ext/spl/spl_dllist.c).
//!
//! Elements live in `__spl_array` (keys 0..n-1); each element also has a
//! stable node id in the parallel `__spl_dll_ids` array so that traversal
//! pointers survive insertions/removals like php-src's linked list nodes.
//! The object's own traversal state is `__spl_dll_cur` (node id or Null) /
//! `__spl_dll_tpos`; foreach uses an independent InternalIterator.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{mangle_private, obj_class_lower, val_arr};
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
    obj.borrow_mut().set_property(name.to_vec(), v);
}

fn load(obj: &Rc<RefCell<PhpObject>>) -> (Vec<Value>, Vec<i64>) {
    let vals: Vec<Value> = match prop(obj, b"__spl_array") {
        Value::Array(a) => a.borrow().values().cloned().collect(),
        _ => Vec::new(),
    };
    let mut ids: Vec<i64> = match prop(obj, b"__spl_dll_ids") {
        Value::Array(a) => a.borrow().values().map(|v| v.to_long()).collect(),
        _ => Vec::new(),
    };
    if ids.len() != vals.len() {
        // Storage created elsewhere (e.g. old code paths): assign fresh ids.
        let mut nid = match prop(obj, b"__spl_dll_nid") {
            Value::Long(n) => n,
            _ => 1,
        };
        ids = Vec::with_capacity(vals.len());
        for _ in 0..vals.len() {
            ids.push(nid);
            nid += 1;
        }
        set_prop(obj, b"__spl_dll_nid", Value::Long(nid));
    }
    (vals, ids)
}

fn store(obj: &Rc<RefCell<PhpObject>>, vals: Vec<Value>, ids: Vec<i64>) {
    let mut a = PhpArray::new();
    for v in vals {
        a.push(v);
    }
    let mut b = PhpArray::new();
    for i in ids {
        b.push(Value::Long(i));
    }
    let mut ob = obj.borrow_mut();
    ob.set_property(b"__spl_array".to_vec(), val_arr(a));
    ob.set_property(b"__spl_dll_ids".to_vec(), val_arr(b));
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
        let (_, ids) = load(obj);
        if flags & IT_LIFO != 0 {
            Cursor { cur: ids.last().copied(), pos: ids.len() as i64 - 1 }
        } else {
            Cursor { cur: ids.first().copied(), pos: 0 }
        }
    }

    /// spl_dllist_it_helper_move_forward()
    fn dll_move_forward(&mut self, obj: &Rc<RefCell<PhpObject>>, c: Cursor, flags: i64) -> Cursor {
        let cur = match c.cur {
            Some(id) => id,
            None => return c,
        };
        let (mut vals, mut ids) = load(obj);
        let idx = ids.iter().position(|&i| i == cur);
        let lifo = flags & IT_LIFO != 0;
        let next = match idx {
            Some(i) => {
                if lifo {
                    if i > 0 { Some(ids[i - 1]) } else { None }
                } else {
                    ids.get(i + 1).copied()
                }
            }
            None => None, // node was removed: its links are cut
        };
        let mut pos = c.pos;
        if lifo {
            pos -= 1;
            if flags & IT_DELETE != 0 && !vals.is_empty() {
                let v = vals.pop();
                ids.pop();
                store(obj, vals, ids);
                drop(v);
            }
        } else if flags & IT_DELETE != 0 {
            if !vals.is_empty() {
                let v = vals.remove(0);
                ids.remove(0);
                store(obj, vals, ids);
                drop(v);
            }
        } else {
            pos += 1;
        }
        Cursor { cur: next, pos }
    }

    fn dll_cursor_value(obj: &Rc<RefCell<PhpObject>>, c: Cursor) -> Option<Value> {
        let id = c.cur?;
        let (vals, ids) = load(obj);
        let i = ids.iter().position(|&x| x == id)?;
        vals.get(i).cloned()
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
        set_prop(obj, b"__spl_dll_cur", match c.cur { Some(n) => Value::Long(n), None => Value::Null });
        set_prop(obj, b"__spl_dll_tpos", Value::Long(c.pos));
    }

    /// spl_ptr_llist_offset(): element index for a user offset.
    fn dll_offset_index(len: usize, index: i64, backward: bool) -> Option<usize> {
        if index < 0 || index as usize >= len {
            return None;
        }
        Some(if backward { len - 1 - index as usize } else { index as usize })
    }

    pub(crate) fn dll_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        let mut out = self.spl_std_props(obj);
        let flags = dll_flags(self, obj);
        out.set(ArrayKey::String(PhpString::from_vec(mangle_private(b"SplDoublyLinkedList", b"flags"))), Value::Long(flags));
        let (vals, _) = load(obj);
        let mut a = PhpArray::new();
        for v in vals {
            a.push(v);
        }
        out.set(ArrayKey::String(PhpString::from_vec(mangle_private(b"SplDoublyLinkedList", b"dllist"))), val_arr(a));
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
    pub(crate) fn spl_dll_call(&mut self, method: &[u8], obj: &Rc<RefCell<PhpObject>>, args: Option<&[Value]>) -> Option<Value> {
        let needs_args = matches!(
            method,
            b"push" | b"unshift" | b"enqueue" | b"setiteratormode" | b"offsetexists" | b"offsetget" | b"offsetset"
                | b"offsetunset" | b"add" | b"unserialize" | b"__unserialize"
        );
        if needs_args && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        let r = match method {
            b"push" | b"enqueue" => {
                let v = a.first().cloned().unwrap_or(Value::Null).deref();
                let (mut vals, mut ids) = load(obj);
                vals.push(v);
                ids.push(new_id(obj));
                store(obj, vals, ids);
                Value::Null
            }
            b"unshift" => {
                let v = a.first().cloned().unwrap_or(Value::Null).deref();
                let (mut vals, mut ids) = load(obj);
                vals.insert(0, v);
                ids.insert(0, new_id(obj));
                store(obj, vals, ids);
                Value::Null
            }
            b"pop" => {
                let (mut vals, mut ids) = load(obj);
                match vals.pop() {
                    Some(v) => {
                        ids.pop();
                        store(obj, vals, ids);
                        v
                    }
                    None => return self.spl_throw(b"RuntimeException", "Can't pop from an empty datastructure"),
                }
            }
            b"shift" | b"dequeue" => {
                let (mut vals, mut ids) = load(obj);
                if vals.is_empty() {
                    return self.spl_throw(b"RuntimeException", "Can't shift from an empty datastructure");
                }
                let v = vals.remove(0);
                ids.remove(0);
                store(obj, vals, ids);
                v
            }
            b"top" => {
                let (vals, _) = load(obj);
                match vals.last() {
                    Some(v) => v.clone(),
                    None => return self.spl_throw(b"RuntimeException", "Can't peek at an empty datastructure"),
                }
            }
            b"bottom" => {
                let (vals, _) = load(obj);
                match vals.first() {
                    Some(v) => v.clone(),
                    None => return self.spl_throw(b"RuntimeException", "Can't peek at an empty datastructure"),
                }
            }
            b"count" => Value::Long(load(obj).0.len() as i64),
            b"isempty" => {
                // spl_dllist_object_count_elements: honours a count() override
                let cl = obj_class_lower(obj);
                let user_count = self.classes.get(&cl).map(|c| c.get_method(b"count").is_some()).unwrap_or(false);
                let n = if user_count {
                    self.call_object_method(&Value::Object(obj.clone()), b"count", &[]).map(|v| v.to_long()).unwrap_or(0)
                } else {
                    load(obj).0.len() as i64
                };
                if n == 0 { Value::True } else { Value::False }
            }
            b"setiteratormode" => {
                let v = match a.first() {
                    Some(v) => match self.spl_arg_long("SplDoublyLinkedList::setIteratorMode", 1, "mode", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                let flags = dll_flags(self, obj);
                if flags & IT_FIX != 0 && (flags & IT_LIFO) != (v & IT_LIFO) {
                    return self.spl_throw(b"RuntimeException", "Iterators' LIFO/FIFO modes for SplStack/SplQueue objects are frozen");
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
                let n = load(obj).0.len() as i64;
                if i >= 0 && i < n { Value::True } else { Value::False }
            }
            b"offsetget" => {
                let func = "SplDoublyLinkedList::offsetGet";
                let i = match self.dll_index_arg(func, a.first()) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let (vals, _) = load(obj);
                let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                match Self::dll_offset_index(vals.len(), i, lifo) {
                    Some(idx) => vals[idx].clone(),
                    None => return self.dll_out_of_range(func),
                }
            }
            b"offsetset" => {
                let func = "SplDoublyLinkedList::offsetSet";
                let v = a.get(1).cloned().unwrap_or(Value::Null).deref();
                let idx_v = a.first().cloned().unwrap_or(Value::Null).deref();
                if matches!(idx_v, Value::Null | Value::Undef) {
                    let (mut vals, mut ids) = load(obj);
                    vals.push(v);
                    ids.push(new_id(obj));
                    store(obj, vals, ids);
                    return Some(Value::Null);
                }
                let i = match self.spl_arg_long(func, 1, "index", &idx_v) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let (mut vals, ids) = load(obj);
                let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                match Self::dll_offset_index(vals.len(), i, lifo) {
                    Some(idx) => {
                        let old = std::mem::replace(&mut vals[idx], v);
                        store(obj, vals, ids);
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
                let (mut vals, mut ids) = load(obj);
                let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                match Self::dll_offset_index(vals.len(), i, lifo) {
                    Some(idx) => {
                        let removed_id = ids.remove(idx);
                        let old = vals.remove(idx);
                        store(obj, vals, ids);
                        let c = Self::dll_obj_cursor(obj);
                        if c.cur == Some(removed_id) {
                            Self::dll_set_obj_cursor(obj, Cursor { cur: None, pos: c.pos });
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
                let v = a.get(1).cloned().unwrap_or(Value::Null).deref();
                let (mut vals, mut ids) = load(obj);
                if i < 0 || i as usize > vals.len() {
                    return self.dll_out_of_range(func);
                }
                let id = new_id(obj);
                if i as usize == vals.len() {
                    vals.push(v);
                    ids.push(id);
                } else {
                    let lifo = dll_flags(self, obj) & IT_LIFO != 0;
                    let at = Self::dll_offset_index(vals.len(), i, lifo).unwrap_or(0);
                    vals.insert(at, v);
                    ids.insert(at, id);
                }
                store(obj, vals, ids);
                Value::Null
            }
            b"rewind" => {
                let c = Self::dll_rewind_cursor(obj, dll_flags(self, obj));
                Self::dll_set_obj_cursor(obj, c);
                Value::Null
            }
            b"valid" => {
                let c = Self::dll_obj_cursor(obj);
                if c.cur.is_some() && Self::dll_cursor_value(obj, c).is_some() { Value::True } else { Value::False }
            }
            b"current" => Self::dll_cursor_value(obj, Self::dll_obj_cursor(obj)).unwrap_or(Value::Null),
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
                let f = self.spl_call_builtin(b"serialize", &[Value::Long(dll_flags(self, obj))]).unwrap_or(Value::Null);
                out.extend_from_slice(f.to_php_string().as_bytes());
                // Walk the live list node by node: serializing an element may
                // modify the list (only nodes still linked are visited).
                let mut cur = load(obj).1.first().copied();
                while let Some(id) = cur {
                    let (vals, ids) = load(obj);
                    let pos = match ids.iter().position(|&x| x == id) {
                        Some(p) => p,
                        None => break,
                    };
                    out.push(b':');
                    let s = self.spl_call_builtin(b"serialize", &[vals[pos].clone()]).unwrap_or(Value::Null);
                    out.extend_from_slice(s.to_php_string().as_bytes());
                    if self.current_exception.is_some() {
                        break;
                    }
                    let ids = load(obj).1;
                    cur = ids.iter().position(|&x| x == id).and_then(|p| ids.get(p + 1).copied());
                }
                Value::String(PhpString::from_vec(out))
            }
            b"unserialize" => {
                let data = match a.first() {
                    Some(v) => match self.spl_arg_string("SplDoublyLinkedList::unserialize", 1, "data", v) {
                        Some(s) => s.as_bytes().to_vec(),
                        None => return Some(Value::Null),
                    },
                    None => Vec::new(),
                };
                if data.is_empty() {
                    return Some(Value::Null);
                }
                store(obj, Vec::new(), Vec::new());
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
                        Some(v) => {
                            let (mut vals, mut ids) = load(obj);
                            vals.push(v);
                            ids.push(new_id(obj));
                            store(obj, vals, ids);
                        }
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
                for v in load(obj).0 {
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
                        let (mut vals, mut ids) = load(obj);
                        for (_, v) in st.borrow().iter() {
                            vals.push(v.clone());
                            ids.push(new_id(obj));
                        }
                        store(obj, vals, ids);
                        let m = m.borrow().clone();
                        self.spl_load_props(obj, &m);
                        Value::Null
                    }
                    _ => return self.spl_throw(b"UnexpectedValueException", "Incomplete or ill-typed serialization data"),
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
    pub(crate) fn dll_new_iterator(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Value {
        let it = self.spl_instantiate(b"InternalIterator");
        let c = Self::dll_obj_cursor(obj);
        let flags = dll_flags(self, obj) & IT_MASK;
        {
            let mut ob = it.borrow_mut();
            ob.set_property(b"__spl_dll_target".to_vec(), Value::Object(obj.clone()));
            ob.set_property(b"__spl_dll_cur".to_vec(), match c.cur { Some(n) => Value::Long(n), None => Value::Null });
            ob.set_property(b"__spl_dll_tpos".to_vec(), Value::Long(c.pos));
            ob.set_property(b"__spl_iter_mode".to_vec(), Value::Long(flags));
        }
        Value::Object(it)
    }

    /// InternalIterator methods for the DLL kind.
    pub(crate) fn dll_iterator_call(&mut self, method: &[u8], it: &Rc<RefCell<PhpObject>>) -> Option<Value> {
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
                if c.cur.is_some() && Self::dll_cursor_value(&target, c).is_some() { Value::True } else { Value::False }
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
    pub(crate) fn dll_uses_internal_iterator(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
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
