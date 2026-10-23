//! SplObjectStorage and MultipleIterator (php-src ext/spl/spl_observer.c).
//!
//! Entries are kept in insertion order in two arrays keyed alike by the
//! entry's hash key (object id, or the getHash() string): `__spl_objects`
//! (the objects) and `__spl_array` (the associated data), so lookups are
//! O(1). `__spl_pos` is the iteration position (an insertion-order index),
//! `__spl_index` the key() counter.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{mangle_private, obj_class_lower, skey, storage_arr, val_arr};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

const MIT_NEED_ALL: i64 = 1;
const MIT_KEYS_ASSOC: i64 = 2;

struct Entries {
    objs: Vec<Value>,
    infs: Vec<Value>,
}

fn arr_prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Option<Rc<RefCell<PhpArray>>> {
    match obj.borrow().get_property(name) {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// Snapshot of the entries, in order.
fn load(obj: &Rc<RefCell<PhpObject>>) -> Entries {
    let objs_a = arr_prop(obj, b"__spl_objects");
    let infs_a = arr_prop(obj, b"__spl_array");
    let mut objs = Vec::new();
    let mut infs = Vec::new();
    if let Some(oa) = objs_a {
        let ob = oa.borrow();
        let ib = infs_a.as_ref().map(|a| a.borrow());
        for (k, o) in ob.iter() {
            objs.push(o.clone());
            infs.push(
                ib.as_ref()
                    .and_then(|i| i.get(k).cloned())
                    .unwrap_or(Value::Null),
            );
        }
    }
    Entries { objs, infs }
}

fn count(obj: &Rc<RefCell<PhpObject>>) -> usize {
    arr_prop(obj, b"__spl_objects")
        .map(|a| a.borrow().len())
        .unwrap_or(0)
}

/// Hash key of entry number `pos` (insertion order).
fn key_at(obj: &Rc<RefCell<PhpObject>>, pos: i64) -> Option<ArrayKey> {
    if pos < 0 {
        return None;
    }
    let a = arr_prop(obj, b"__spl_objects")?;
    let ab = a.borrow();
    ab.entry_at(pos as usize).map(|(k, _)| k.clone())
}

fn obj_at(obj: &Rc<RefCell<PhpObject>>, pos: i64) -> Option<Value> {
    if pos < 0 {
        return None;
    }
    let a = arr_prop(obj, b"__spl_objects")?;
    let ab = a.borrow();
    ab.entry_at(pos as usize).map(|(_, v)| v.clone())
}

/// The data of entry number `pos` (Null when there is none).
fn inf_at(obj: &Rc<RefCell<PhpObject>>, pos: i64) -> Value {
    let k = match key_at(obj, pos) {
        Some(k) => k,
        None => return Value::Null,
    };
    arr_prop(obj, b"__spl_array")
        .and_then(|a| a.borrow().get(&k).cloned())
        .unwrap_or(Value::Null)
}

/// Replace the data of the entry keyed `k`; returns the old data.
fn set_inf(obj: &Rc<RefCell<PhpObject>>, k: ArrayKey, inf: Value) -> Option<Value> {
    let infs = storage_arr(obj, b"__spl_array");
    let mut ib = infs.borrow_mut();
    match ib.get_mut(&k) {
        Some(slot) => Some(std::mem::replace(slot, inf)),
        None => {
            ib.set(k, inf);
            None
        }
    }
}

/// Array key standing for a storage hash key (an object id or a string).
fn hkey(k: &Value) -> ArrayKey {
    match k {
        Value::Long(n) => ArrayKey::Int(*n),
        Value::String(s) => match crate::fastpath::canonical_int_key(s.as_bytes()) {
            Some(n) => ArrayKey::Int(n),
            None => ArrayKey::String(s.clone()),
        },
        _ => ArrayKey::Int(0),
    }
}

fn lprop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> i64 {
    match obj.borrow().get_property(name) {
        Value::Long(n) => n,
        _ => 0,
    }
}

fn set_l(obj: &Rc<RefCell<PhpObject>>, name: &[u8], n: i64) {
    obj.borrow_mut().set_property_slice(name, Value::Long(n));
}

fn bv(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

impl Vm {
    fn sos_user_get_hash(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        self.classes
            .get(&cl)
            .map(|c| c.get_method(b"gethash").is_some())
            .unwrap_or(false)
    }

    /// spl_object_storage_get_hash(): None with an exception on failure.
    fn sos_key(&mut self, storage: &Rc<RefCell<PhpObject>>, o: &Value) -> Option<Value> {
        let oid = match o {
            Value::Object(x) => x.borrow().object_id as i64,
            Value::Generator(g) => self.generator_handle(g) as i64,
            _ => return None,
        };
        if !self.sos_user_get_hash(storage) {
            return Some(Value::Long(oid));
        }
        let r = self.call_object_method(
            &Value::Object(storage.clone()),
            b"getHash",
            std::slice::from_ref(o),
        );
        if self.current_exception.is_some() {
            return None;
        }
        match r.map(|v| v.deref()) {
            Some(Value::String(s)) => Some(Value::String(s)),
            Some(other) => {
                let msg = format!(
                    "{}::getHash(): Return value must be of type string, {} returned",
                    crate::value::display_class_name(&storage.borrow().class_name),
                    Self::value_type_name(&other)
                );
                self.spl_throw(b"TypeError", &msg);
                None
            }
            None => None,
        }
    }

    fn sos_attach(&mut self, storage: &Rc<RefCell<PhpObject>>, o: &Value, inf: Value) -> bool {
        let key = match self.sos_key(storage, o) {
            Some(k) => k,
            None => return false,
        };
        let k = hkey(&key);
        let objs = storage_arr(storage, b"__spl_objects");
        let exists = objs.borrow().contains_key(&k);
        if exists {
            let old = set_inf(storage, k, inf);
            if let Some(old) = &old {
                crate::gc::on_child_released(old);
            }
            drop(old);
        } else {
            objs.borrow_mut().set(k.clone(), o.clone());
            set_inf(storage, k, inf);
        }
        true
    }

    fn sos_detach(&mut self, storage: &Rc<RefCell<PhpObject>>, o: &Value) -> bool {
        let key = match self.sos_key(storage, o) {
            Some(k) => k,
            None => return false,
        };
        let k = hkey(&key);
        let objs = storage_arr(storage, b"__spl_objects");
        if !objs.borrow().contains_key(&k) {
            return false;
        }
        let ob = objs.borrow_mut().remove(&k);
        let infs = storage_arr(storage, b"__spl_array");
        let inf = infs.borrow_mut().remove(&k);
        // Release after the entry is gone (destructors see the new state).
        drop(inf);
        drop(ob);
        self.run_pending_destructors_pub();
        true
    }

    fn sos_find(
        &mut self,
        storage: &Rc<RefCell<PhpObject>>,
        o: &Value,
    ) -> Result<Option<usize>, ()> {
        let key = match self.sos_key(storage, o) {
            Some(k) => k,
            None => return Err(()),
        };
        let k = hkey(&key);
        Ok(arr_prop(storage, b"__spl_objects").and_then(|a| a.borrow().position_of(&k)))
    }

    fn sos_reset(storage: &Rc<RefCell<PhpObject>>) {
        set_l(storage, b"__spl_pos", 0);
        set_l(storage, b"__spl_index", 0);
    }

    fn sos_obj_arg(&mut self, func: &str, v: Option<&Value>) -> Option<Value> {
        match v.map(|v| v.deref()) {
            Some(o @ (Value::Object(_) | Value::Generator(_))) => Some(o),
            Some(other) => {
                let msg = format!(
                    "{}(): Argument #1 ($object) must be of type object, {} given",
                    func,
                    Self::value_type_name(&other)
                );
                self.spl_throw(b"TypeError", &msg);
                None
            }
            None => {
                let msg = format!("{}() expects exactly 1 argument, 0 given", func);
                self.spl_throw(b"ArgumentCountError", &msg);
                None
            }
        }
    }

    fn sos_storage_arg(&mut self, func: &str, v: Option<&Value>) -> Option<Rc<RefCell<PhpObject>>> {
        match v.map(|v| v.deref()) {
            Some(Value::Object(o))
                if self
                    .spl_find_base(&obj_class_lower(&o), &[b"splobjectstorage"])
                    .is_some() =>
            {
                Some(o)
            }
            other => {
                let t = other
                    .map(|v| Self::value_type_name(&v))
                    .unwrap_or_else(|| "null".to_string());
                let msg = format!(
                    "{}(): Argument #1 ($storage) must be of type SplObjectStorage, {} given",
                    func, t
                );
                self.spl_throw(b"TypeError", &msg);
                None
            }
        }
    }

    pub(crate) fn sos_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        let mut out = self.spl_std_props(obj);
        let e = load(obj);
        let mut st = PhpArray::new();
        for (o, inf) in e.objs.into_iter().zip(e.infs) {
            let mut entry = PhpArray::new();
            entry.set(skey(b"obj"), o);
            entry.set(skey(b"inf"), inf);
            st.push(val_arr(entry));
        }
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(
                b"SplObjectStorage",
                b"storage",
            ))),
            val_arr(st),
        );
        out
    }

    /// SplObjectStorage methods (`args` excludes $this; None on the no-arg path).
    pub(crate) fn spl_storage_call(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        // Serializable::unserialize(): one var_hash for all the pieces.
        if method == b"unserialize" {
            if let Some(h) = self.unserialize_scope_hook {
                h(self, true);
                let r = self.spl_storage_call_impl(method, obj, args);
                h(self, false);
                return r;
            }
        }
        self.spl_storage_call_impl(method, obj, args)
    }

    fn spl_storage_call_impl(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        let needs_args = matches!(
            method,
            b"attach"
                | b"detach"
                | b"contains"
                | b"addall"
                | b"removeall"
                | b"removeallexcept"
                | b"setinfo"
                | b"seek"
                | b"unserialize"
                | b"offsetexists"
                | b"offsetget"
                | b"offsetset"
                | b"offsetunset"
                | b"gethash"
                | b"__unserialize"
        );
        if needs_args && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        let r = match method {
            b"attach" | b"offsetset" => {
                let func = if method == b"attach" {
                    "SplObjectStorage::attach"
                } else {
                    "SplObjectStorage::offsetSet"
                };
                let o = self.sos_obj_arg(func, a.first())?;
                let inf = a.get(1).map_or(Value::Null, Value::deref_for_storage);
                self.sos_attach(obj, &o, inf);
                Value::Null
            }
            b"detach" | b"offsetunset" => {
                let func = if method == b"detach" {
                    "SplObjectStorage::detach"
                } else {
                    "SplObjectStorage::offsetUnset"
                };
                let o = self.sos_obj_arg(func, a.first())?;
                self.sos_detach(obj, &o);
                Self::sos_reset(obj);
                Value::Null
            }
            b"contains" | b"offsetexists" => {
                let func = if method == b"contains" {
                    "SplObjectStorage::contains"
                } else {
                    "SplObjectStorage::offsetExists"
                };
                let o = self.sos_obj_arg(func, a.first())?;
                match self.sos_find(obj, &o) {
                    Ok(f) => bv(f.is_some()),
                    Err(()) => Value::True,
                }
            }
            b"offsetget" => {
                let o = self.sos_obj_arg("SplObjectStorage::offsetGet", a.first())?;
                match self.sos_find(obj, &o) {
                    Ok(Some(i)) => inf_at(obj, i as i64),
                    Ok(None) => {
                        return self.spl_throw(b"UnexpectedValueException", "Object not found");
                    }
                    Err(()) => Value::Null,
                }
            }
            b"gethash" => {
                let o = self.sos_obj_arg("SplObjectStorage::getHash", a.first())?;
                return self.spl_call_builtin(b"spl_object_hash", &[o]);
            }
            b"addall" => {
                let other = self.sos_storage_arg("SplObjectStorage::addAll", a.first())?;
                let oe = load(&other);
                for (o, inf) in oe.objs.into_iter().zip(oe.infs) {
                    if !self.sos_attach(obj, &o, inf) {
                        break;
                    }
                }
                set_l(obj, b"__spl_index", 0);
                Value::Long(load(obj).objs.len() as i64)
            }
            b"removeall" => {
                let other = self.sos_storage_arg("SplObjectStorage::removeAll", a.first())?;
                let oe = load(&other);
                for o in oe.objs {
                    self.sos_detach(obj, &o);
                    if self.current_exception.is_some() {
                        break;
                    }
                }
                Self::sos_reset(obj);
                Value::Long(load(obj).objs.len() as i64)
            }
            b"removeallexcept" => {
                let other = self.sos_storage_arg("SplObjectStorage::removeAllExcept", a.first())?;
                let mine = load(obj).objs;
                for o in mine {
                    match self.sos_find(&other, &o) {
                        Ok(Some(_)) | Err(()) => {}
                        Ok(None) => {
                            self.sos_detach(obj, &o);
                        }
                    }
                    if self.current_exception.is_some() {
                        break;
                    }
                }
                Self::sos_reset(obj);
                Value::Long(load(obj).objs.len() as i64)
            }
            b"count" => Value::Long(count(obj) as i64),
            b"rewind" => {
                Self::sos_reset(obj);
                Value::Null
            }
            b"valid" => {
                let pos = lprop(obj, b"__spl_pos");
                bv(pos >= 0 && (pos as usize) < count(obj))
            }
            b"key" => Value::Long(lprop(obj, b"__spl_index")),
            b"current" => {
                let pos = lprop(obj, b"__spl_pos");
                match obj_at(obj, pos) {
                    Some(o) => o,
                    None => {
                        return self.spl_throw(
                            b"RuntimeException",
                            "Called current() on invalid iterator",
                        );
                    }
                }
            }
            b"getinfo" => inf_at(obj, lprop(obj, b"__spl_pos")),
            b"setinfo" => {
                if let Some(k) = key_at(obj, lprop(obj, b"__spl_pos")) {
                    let v = a.first().map_or(Value::Null, Value::deref_for_storage);
                    let old = set_inf(obj, k, v);
                    if let Some(old) = &old {
                        crate::gc::on_child_released(old);
                    }
                    drop(old);
                }
                Value::Null
            }
            b"next" => {
                set_l(obj, b"__spl_pos", lprop(obj, b"__spl_pos") + 1);
                set_l(obj, b"__spl_index", lprop(obj, b"__spl_index") + 1);
                Value::Null
            }
            b"seek" => {
                let p = match a.first() {
                    Some(v) => match self.spl_arg_long("SplObjectStorage::seek", 1, "offset", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                let n = count(obj) as i64;
                if p < 0 || p >= n {
                    let msg = format!("Seek position {} is out of range", p);
                    return self.spl_throw(b"OutOfBoundsException", &msg);
                }
                set_l(obj, b"__spl_pos", p);
                set_l(obj, b"__spl_index", p);
                Value::Null
            }
            b"__debuginfo" => val_arr(self.sos_debug_info(obj)),
            b"__serialize" => {
                let e = load(obj);
                let mut st = PhpArray::new();
                for (o, inf) in e.objs.into_iter().zip(e.infs) {
                    st.push(o);
                    st.push(inf);
                }
                let mut out = PhpArray::new();
                out.push(val_arr(st));
                out.push(val_arr(self.spl_std_props(obj)));
                val_arr(out)
            }
            b"__unserialize" => {
                let data = match a.first().map(|v| v.deref()) {
                    Some(Value::Array(d)) => d.borrow().clone(),
                    _ => PhpArray::new(),
                };
                let st = data.get(&ArrayKey::Int(0)).cloned();
                let members = data.get(&ArrayKey::Int(1)).cloned();
                let (st, members) = match (st, members) {
                    (Some(Value::Array(s)), Some(Value::Array(m))) => {
                        (s.borrow().clone(), m.borrow().clone())
                    }
                    _ => {
                        return self.spl_throw(
                            b"UnexpectedValueException",
                            "Incomplete or ill-typed serialization data",
                        );
                    }
                };
                if st.len() % 2 != 0 {
                    return self.spl_throw(b"UnexpectedValueException", "Odd number of elements");
                }
                let vals: Vec<Value> = st.values().cloned().collect();
                for pair in vals.chunks(2) {
                    let k = pair[0].deref();
                    if !matches!(k, Value::Object(_)) {
                        return self.spl_throw(b"UnexpectedValueException", "Non-object key");
                    }
                    if !self.sos_attach(obj, &k, pair[1].deref()) {
                        return Some(Value::Null);
                    }
                }
                self.spl_load_props(obj, &members);
                Value::Null
            }
            b"serialize" => {
                let e = load(obj);
                let mut out: Vec<u8> = b"x:".to_vec();
                // (one var_hash for all the pieces, the count included)
                let scope = self.serialize_scope_hook;
                if let Some(h) = scope {
                    h(true);
                }
                let n = self
                    .spl_call_builtin(b"serialize", &[Value::Long(e.objs.len() as i64)])
                    .unwrap_or(Value::Null);
                out.extend_from_slice(n.to_php_string().as_bytes());
                for (o, inf) in e.objs.into_iter().zip(e.infs) {
                    let so = self
                        .spl_call_builtin(b"serialize", &[o])
                        .unwrap_or(Value::Null);
                    out.extend_from_slice(so.to_php_string().as_bytes());
                    out.push(b',');
                    let si = self
                        .spl_call_builtin(b"serialize", &[inf])
                        .unwrap_or(Value::Null);
                    out.extend_from_slice(si.to_php_string().as_bytes());
                    out.push(b';');
                }
                out.extend_from_slice(b"m:");
                let props = val_arr(self.spl_std_props(obj));
                let sp = self
                    .spl_call_builtin(b"serialize", &[props])
                    .unwrap_or(Value::Null);
                if let Some(h) = scope {
                    h(false);
                }
                out.extend_from_slice(sp.to_php_string().as_bytes());
                Value::String(PhpString::from_vec(out))
            }
            b"unserialize" => {
                let data = match a.first() {
                    Some(v) => {
                        match self.spl_arg_string("SplObjectStorage::unserialize", 1, "data", v) {
                            Some(s) => s.as_bytes().to_vec(),
                            None => return Some(Value::Null),
                        }
                    }
                    None => Vec::new(),
                };
                if data.is_empty() {
                    return Some(Value::Null);
                }
                let fail = |vm: &mut Vm, p: usize, n: usize| -> Option<Value> {
                    let msg = format!("Error at offset {} of {} bytes", p, n);
                    vm.spl_throw(b"UnexpectedValueException", &msg)
                };
                let n = data.len();
                if !(data.len() >= 2 && data[0] == b'x' && data[1] == b':') {
                    return fail(self, if data.first() == Some(&b'x') { 1 } else { 0 }, n);
                }
                let mut p = 2usize;
                let count = match self.spl_unserialize_at(&data, &mut p) {
                    Some(Value::Long(c)) => c,
                    _ => return fail(self, p, n),
                };
                p -= 1; // back to ';'
                if count < 0 {
                    return fail(self, p, n);
                }
                for _ in 0..count {
                    if data.get(p) != Some(&b';') {
                        return fail(self, p, n);
                    }
                    p += 1;
                    if !matches!(data.get(p), Some(b'O') | Some(b'C') | Some(b'r')) {
                        return fail(self, p, n);
                    }
                    let entry = match self.spl_unserialize_at(&data, &mut p) {
                        Some(v) => v,
                        None => return fail(self, p, n),
                    };
                    let mut inf = Value::Null;
                    if data.get(p) == Some(&b',') {
                        p += 1;
                        inf = match self.spl_unserialize_at(&data, &mut p) {
                            Some(v) => v,
                            None => return fail(self, p, n),
                        };
                    }
                    if !matches!(entry, Value::Object(_)) {
                        return fail(self, p, n);
                    }
                    if !self.sos_attach(obj, &entry, inf) {
                        return fail(self, p, n);
                    }
                }
                if data.get(p) != Some(&b';') {
                    return fail(self, p, n);
                }
                p += 1;
                if !(data.get(p) == Some(&b'm') && data.get(p + 1) == Some(&b':')) {
                    return fail(self, p, n);
                }
                p += 2;
                match self.spl_unserialize_at(&data, &mut p) {
                    Some(Value::Array(m)) => {
                        let m = m.borrow().clone();
                        self.spl_load_props(obj, &m);
                    }
                    _ => return fail(self, p, n),
                }
                Value::Null
            }
            _ => return None,
        };
        Some(r)
    }

    /// A sub-iterator's Iterator method (a Generator's own, natively).
    fn mit_sub_call(&mut self, it: &Value, method: &[u8]) -> Option<Value> {
        if !matches!(it, Value::Generator(_)) {
            return self.call_object_method(it, method, &[]);
        }
        Some(match method {
            b"rewind" => {
                self.it_rewind(it);
                Value::Null
            }
            b"next" => {
                self.it_next(it);
                Value::Null
            }
            b"valid" => bv(self.it_valid(it)),
            b"current" => self.it_current(it),
            _ => self.it_key(it),
        })
    }

    /// MultipleIterator methods.
    pub(crate) fn spl_multiple_call(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        let needs_args = matches!(
            method,
            b"setflags"
                | b"attachiterator"
                | b"detachiterator"
                | b"containsiterator"
                | b"__unserialize"
                | b"offsetset"
                | b"offsetget"
                | b"offsetexists"
                | b"offsetunset"
        );
        if needs_args && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        let flags = match obj.borrow().get_property(b"__spl_flags") {
            Value::Long(f) => f,
            _ => MIT_NEED_ALL,
        };
        // Dimension handlers MultipleIterator shares with SplObjectStorage
        // (spl_multiple_iterator_write_dimension & co.); it has no
        // ArrayAccess methods of its own.
        if matches!(
            method,
            b"offsetset" | b"offsetget" | b"offsetexists" | b"offsetunset"
        ) {
            let key = a.first().cloned().unwrap_or(Value::Null).deref();
            let ko = match &key {
                Value::Object(_) => key.clone(),
                _ => {
                    if method == b"offsetexists" {
                        return Some(Value::False);
                    }
                    return self.spl_throw(
                        b"Error",
                        "Cannot use object of type MultipleIterator as array",
                    );
                }
            };
            let r = match method {
                b"offsetset" => {
                    let is_iter = match &ko {
                        Value::Object(o) => {
                            let cl = obj_class_lower(o);
                            self.class_implements_interface(&cl, b"iterator")
                                || crate::vm::is_builtin_subclass(&cl, b"iterator")
                        }
                        _ => false,
                    };
                    if !is_iter {
                        return self.spl_throw(
                            b"TypeError",
                            "Can only attach objects that implement the Iterator interface",
                        );
                    }
                    let inf = a.get(1).cloned().unwrap_or(Value::Null).deref();
                    self.sos_attach(obj, &ko, inf);
                    Value::Null
                }
                b"offsetunset" => {
                    self.sos_detach(obj, &ko);
                    Self::sos_reset(obj);
                    Value::Null
                }
                b"offsetexists" => match self.sos_find(obj, &ko) {
                    Ok(f) => bv(f.is_some()),
                    Err(()) => Value::True,
                },
                _ => match self.sos_find(obj, &ko) {
                    Ok(Some(i)) => inf_at(obj, i as i64),
                    Ok(None) => {
                        return self.spl_throw(b"UnexpectedValueException", "Object not found");
                    }
                    Err(()) => Value::Null,
                },
            };
            return Some(r);
        }
        let r = match method {
            b"getflags" => Value::Long(flags),
            b"setflags" => {
                let f = match a.first() {
                    Some(v) => match self.spl_arg_long("MultipleIterator::setFlags", 1, "flags", v)
                    {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                set_l(obj, b"__spl_flags", f);
                Value::Null
            }
            b"attachiterator" => {
                let it = a.first().cloned().unwrap_or(Value::Null).deref();
                let is_iter = match &it {
                    Value::Object(o) => {
                        let cl = obj_class_lower(o);
                        self.class_implements_interface(&cl, b"iterator")
                            || crate::vm::is_builtin_subclass(&cl, b"iterator")
                    }
                    Value::Generator(_) => true,
                    _ => false,
                };
                if !is_iter {
                    let msg = format!(
                        "MultipleIterator::attachIterator(): Argument #1 ($iterator) must be of type Iterator, {} given",
                        Self::value_type_name(&it)
                    );
                    return self.spl_throw(b"TypeError", &msg);
                }
                let info = a.get(1).cloned().unwrap_or(Value::Null).deref();
                match &info {
                    Value::Null | Value::Long(_) | Value::String(_) => {}
                    other => {
                        let msg = format!(
                            "MultipleIterator::attachIterator(): Argument #2 ($info) must be of type string|int|null, {} given",
                            Self::value_type_name(other)
                        );
                        return self.spl_throw(b"TypeError", &msg);
                    }
                }
                if !matches!(info, Value::Null) {
                    let e = load(obj);
                    if e.infs.iter().any(|x| x.identical(&info)) {
                        return self
                            .spl_throw(b"InvalidArgumentException", "Key duplication error");
                    }
                }
                self.sos_attach(obj, &it, info);
                Value::Null
            }
            b"detachiterator" => {
                let it = a.first().cloned().unwrap_or(Value::Null).deref();
                if matches!(it, Value::Object(_)) {
                    self.sos_detach(obj, &it);
                }
                Self::sos_reset(obj);
                Value::Null
            }
            b"containsiterator" => {
                let it = a.first().cloned().unwrap_or(Value::Null).deref();
                match self.sos_find(obj, &it) {
                    Ok(f) => bv(f.is_some()),
                    Err(()) => Value::False,
                }
            }
            b"countiterators" => Value::Long(load(obj).objs.len() as i64),
            b"rewind" | b"next" => {
                for it in load(obj).objs {
                    if self.current_exception.is_some() {
                        break;
                    }
                    self.mit_sub_call(&it, method);
                }
                Value::Null
            }
            b"valid" => {
                let its = load(obj).objs;
                if its.is_empty() {
                    return Some(Value::False);
                }
                let expect = flags & MIT_NEED_ALL != 0;
                for it in its {
                    if self.current_exception.is_some() {
                        break;
                    }
                    let v = matches!(self.mit_sub_call(&it, b"valid"), Some(Value::True));
                    if v != expect {
                        return Some(bv(!expect));
                    }
                }
                bv(expect)
            }
            b"current" | b"key" => {
                let e = load(obj);
                if e.objs.is_empty() {
                    let msg = format!(
                        "Called {}() on an invalid iterator",
                        crate::string::msg_str(method)
                    );
                    return self.spl_throw(b"RuntimeException", &msg);
                }
                let mut out = PhpArray::new();
                for (it, inf) in e.objs.into_iter().zip(e.infs) {
                    if self.current_exception.is_some() {
                        break;
                    }
                    let valid = matches!(self.mit_sub_call(&it, b"valid"), Some(Value::True));
                    let v = if valid {
                        match self.mit_sub_call(&it, method) {
                            Some(v) => v,
                            None => {
                                if self.current_exception.is_none() {
                                    self.spl_throw(
                                        b"RuntimeException",
                                        "Failed to call sub iterator method",
                                    );
                                }
                                return Some(Value::Null);
                            }
                        }
                    } else if flags & MIT_NEED_ALL != 0 {
                        let msg = format!(
                            "Called {}() with non valid sub iterator",
                            crate::string::msg_str(method)
                        );
                        return self.spl_throw(b"RuntimeException", &msg);
                    } else {
                        Value::Null
                    };
                    if flags & MIT_KEYS_ASSOC != 0 {
                        match inf {
                            Value::Long(n) => out.set(ArrayKey::Int(n), v),
                            Value::String(s) => {
                                out.set(Self::value_to_array_key(Value::String(s)), v)
                            }
                            _ => {
                                return self.spl_throw(
                                    b"InvalidArgumentException",
                                    "Sub-Iterator is associated with NULL",
                                );
                            }
                        }
                    } else {
                        out.push(v);
                    }
                }
                val_arr(out)
            }
            b"__debuginfo" => val_arr(self.sos_debug_info(obj)),
            _ => return None,
        };
        Some(r)
    }

    /// MultipleIterator::__construct(int $flags = MIT_NEED_ALL | MIT_KEYS_NUMERIC)
    pub fn spl_multiple_construct(&mut self, obj: &Rc<RefCell<PhpObject>>, args: &[Value]) -> bool {
        let f = match args.first() {
            Some(v) => match self.spl_arg_long("MultipleIterator::__construct", 1, "flags", v) {
                Some(n) => n,
                None => return false,
            },
            None => MIT_NEED_ALL,
        };
        set_l(obj, b"__spl_flags", f);
        true
    }
}

/// Native constructor handler for MultipleIterator.
pub fn spl_multiple_construct_native(
    vm: &mut Vm,
    args: &[Value],
    _line: u32,
) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    if vm.spl_multiple_construct(&obj, &args[1..]) {
        Ok(())
    } else {
        let msg = match &vm.current_exception {
            Some(Value::Object(e)) => e
                .borrow()
                .get_property(b"message")
                .to_php_string()
                .to_msg_string(),
            _ => String::new(),
        };
        Err(msg)
    }
}
