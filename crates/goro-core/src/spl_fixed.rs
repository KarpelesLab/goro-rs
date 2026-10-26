//! SplFixedArray (php-src ext/spl/spl_fixedarray.c).
//!
//! Storage: `__spl_array` holds exactly `size` elements keyed 0..size-1 and
//! is mutated in place (foreach iterates it live); `__spl_size` mirrors the
//! size for older code paths.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{obj_class_lower, val_arr};
use crate::value::Value;
use crate::vm::Vm;

fn storage(obj: &Rc<RefCell<PhpObject>>) -> Rc<RefCell<PhpArray>> {
    if let Value::Array(a) = obj.borrow().get_property(b"__spl_array") {
        return a;
    }
    let a = Rc::new(RefCell::new(PhpArray::new()));
    // (an object made without its constructor has no storage yet; a caller
    // that is reading it, e.g. var_dump(), just sees it empty)
    let Ok(mut ob) = obj.try_borrow_mut() else {
        return a;
    };
    ob.set_property_slice(b"__spl_array", Value::Array(a.clone()));
    ob.set_property_slice(b"__spl_size", Value::Long(0));
    a
}

fn fa_size(obj: &Rc<RefCell<PhpObject>>) -> i64 {
    storage(obj).borrow().len() as i64
}

fn sync_size(obj: &Rc<RefCell<PhpObject>>) {
    let n = fa_size(obj);
    obj.borrow_mut()
        .set_property_slice(b"__spl_size", Value::Long(n));
}

impl Vm {
    /// Resize in place (spl_fixedarray_resize).
    pub(crate) fn fa_resize(&mut self, obj: &Rc<RefCell<PhpObject>>, size: i64) {
        let st = storage(obj);
        let cur = st.borrow().len() as i64;
        if size == cur {
            return;
        }
        if size > cur
            && (size as u64)
                .checked_mul(16)
                .map(|b| b > isize::MAX as u64)
                .unwrap_or(true)
        {
            // safe_erealloc(elements, size, sizeof(zval), 0)
            let msg = format!(
                "Possible integer overflow in memory allocation ({} * 16 + 0)",
                size as u64
            );
            self.spl_fatal(&msg);
            return;
        }
        // A resize requested by a destructor run by this resize is
        // remembered and performed afterwards (cached_resize).
        const CACHED: &[u8] = b"__spl_cached_resize";
        let resizing = matches!(obj.borrow().get_property(CACHED), Value::Long(_));
        if resizing {
            obj.borrow_mut()
                .set_property(CACHED.to_vec(), Value::Long(size));
            return;
        }
        obj.borrow_mut()
            .set_property(CACHED.to_vec(), Value::Long(size));
        if size > cur {
            let mut a = st.borrow_mut();
            for i in cur..size {
                a.set(ArrayKey::Int(i), Value::Null);
            }
        } else {
            // spl_fixedarray_dtor_range(): the size is set first, then the
            // removed elements are released in order (destructors may run).
            let removed: Vec<Value> = {
                let mut a = st.borrow_mut();
                let mut out = Vec::new();
                for i in size..cur {
                    if let Some(v) = a.remove(&ArrayKey::Int(i)) {
                        out.push(v);
                    }
                }
                a.recalculate_next_int_key();
                out
            };
            sync_size(obj);
            for v in removed {
                drop(v);
                self.run_pending_destructors_pub();
            }
        }
        sync_size(obj);
        let cached = obj.borrow().get_property(CACHED);
        obj.borrow_mut().remove_property(CACHED);
        if let Value::Long(c) = cached {
            if c != size {
                self.fa_resize(obj, c);
            }
        }
    }

    /// spl_offset_convert_to_ulong(): Some(index) or None with an exception.
    fn fa_offset(&mut self, offset: &Value) -> Option<i64> {
        match offset.deref() {
            Value::Long(n) => Some(n),
            Value::String(s) => {
                let t = s.to_string_lossy();
                let canon = t.parse::<i64>().ok().filter(|n| n.to_string() == t);
                match canon {
                    Some(n) => Some(n),
                    None => {
                        self.spl_throw(
                            b"TypeError",
                            "Cannot access offset of type string on SplFixedArray",
                        );
                        None
                    }
                }
            }
            Value::Double(d) => {
                // zend_dval_to_lval_safe()
                let line = self.current_line;
                let l = self.float_to_int_cast(d, line);
                let fits = !(d >= i64::MAX as f64 || d < i64::MIN as f64);
                if l as f64 != d && fits {
                    let msg = format!(
                        "Implicit conversion from float {} to int loses precision",
                        crate::value::format_php_float_repr(d)
                    );
                    self.emit_deprecated(&msg);
                }
                Some(l)
            }
            Value::False => Some(0),
            Value::True => Some(1),
            Value::Resource(r) => {
                let msg = format!(
                    "Resource ID#{} used as offset, casting to integer ({})",
                    r, r
                );
                self.emit_warning(&msg);
                Some(r.id())
            }
            other => {
                let t = match other {
                    Value::Null | Value::Undef => "null".to_string(),
                    Value::Array(_) => "array".to_string(),
                    Value::Object(o) => {
                        crate::value::display_class_name(&o.borrow().class_name).to_string()
                    }
                    _ => Self::value_type_name(&other),
                };
                let msg = format!("Cannot access offset of type {} on SplFixedArray", t);
                self.spl_throw(b"TypeError", &msg);
                None
            }
        }
    }

    fn fa_bounds(&mut self, obj: &Rc<RefCell<PhpObject>>, idx: i64) -> bool {
        if idx < 0 || idx >= fa_size(obj) {
            self.spl_throw(b"OutOfBoundsException", "Index invalid or out of range");
            return false;
        }
        true
    }

    /// Properties for var_dump / (array): elements, then real properties.
    pub(crate) fn fa_debug_info(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        with_elements: bool,
    ) -> PhpArray {
        let mut out = PhpArray::new();
        if with_elements {
            for (_, v) in storage(obj).borrow().iter() {
                out.push(v.clone());
            }
        }
        for (k, v) in self.spl_std_props(obj).iter() {
            out.set(k.clone(), v.clone());
        }
        out
    }

    /// get_properties_for(ARRAY_CAST): elements, then the property table
    /// (numeric property names become integer keys and may overwrite).
    pub(crate) fn fa_properties_for_cast(&self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        let mut out = PhpArray::new();
        for (k, v) in storage(obj).borrow().iter() {
            out.set(k.clone(), v.clone());
        }
        let props = Vm::proptable_to_symtable(self.object_props_mangled(obj));
        for (k, v) in props.borrow().iter() {
            out.set(k.clone(), v.clone());
        }
        out
    }

    /// SplFixedArray::fromArray(array $array, bool $preserveKeys = true)
    pub(crate) fn fa_from_array(&mut self, args: &[Value]) -> Value {
        let func = "SplFixedArray::fromArray";
        let data = match args.first().map(|v| v.deref()) {
            Some(Value::Array(a)) => a.borrow().clone(),
            Some(other) => {
                let msg = format!(
                    "{}(): Argument #1 ($array) must be of type array, {} given",
                    func,
                    Self::value_type_name(&other)
                );
                self.spl_throw(b"TypeError", &msg);
                return Value::Null;
            }
            None => {
                self.spl_throw(
                    b"ArgumentCountError",
                    "SplFixedArray::fromArray() expects at least 1 argument, 0 given",
                );
                return Value::Null;
            }
        };
        let preserve = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
        let mut out = PhpArray::new();
        if !data.is_empty() && preserve {
            let mut max: i64 = -1;
            for (k, _) in data.iter() {
                match k {
                    ArrayKey::Int(n) if *n >= 0 => {
                        if *n > max {
                            max = *n;
                        }
                    }
                    _ => {
                        self.spl_throw(
                            b"InvalidArgumentException",
                            "array must contain only positive integer keys",
                        );
                        return Value::Null;
                    }
                }
            }
            if max == i64::MAX {
                self.spl_throw(b"InvalidArgumentException", "integer overflow detected");
                return Value::Null;
            }
            for i in 0..=max {
                out.set(ArrayKey::Int(i), Value::Null);
            }
            for (k, v) in data.iter() {
                out.set(k.clone(), v.deref());
            }
        } else {
            for (_, v) in data.iter() {
                out.push(v.deref());
            }
        }
        let obj = self.spl_instantiate(b"SplFixedArray");
        {
            let n = out.len() as i64;
            let mut ob = obj.borrow_mut();
            ob.set_property_slice(b"__spl_array", val_arr(out));
            ob.set_property_slice(b"__spl_size", Value::Long(n));
        }
        Value::Object(obj)
    }

    /// Dispatch an SplFixedArray method (`args` without $this; None on the
    /// no-argument path).
    pub(crate) fn spl_fixed_call(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        let needs_args = matches!(
            method,
            b"offsetget"
                | b"offsetset"
                | b"offsetexists"
                | b"offsetunset"
                | b"setsize"
                | b"__unserialize"
        );
        if needs_args && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        let r = match method {
            b"count" | b"getsize" => Value::Long(fa_size(obj)),
            b"toarray" | b"jsonserialize" => {
                let mut out = PhpArray::new();
                for (_, v) in storage(obj).borrow().iter() {
                    out.push(v.clone());
                }
                val_arr(out)
            }
            b"setsize" => {
                let n = match a.first() {
                    Some(v) => match self.spl_arg_long("SplFixedArray::setSize", 1, "size", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                if n < 0 {
                    return self.spl_throw(b"ValueError", "SplFixedArray::setSize(): Argument #1 ($size) must be greater than or equal to 0");
                }
                self.fa_resize(obj, n);
                Value::True
            }
            b"offsetexists" => {
                let idx = match self.fa_offset(a.first().unwrap_or(&Value::Null)) {
                    Some(i) => i,
                    None => return Some(Value::Null),
                };
                if idx < 0 || idx >= fa_size(obj) {
                    return Some(Value::False);
                }
                let v = storage(obj)
                    .borrow()
                    .get(&ArrayKey::Int(idx))
                    .cloned()
                    .unwrap_or(Value::Null);
                if matches!(v.deref(), Value::Null) {
                    Value::False
                } else {
                    Value::True
                }
            }
            b"offsetget" => {
                let idx = match self.fa_offset(a.first().unwrap_or(&Value::Null)) {
                    Some(i) => i,
                    None => return Some(Value::Null),
                };
                if !self.fa_bounds(obj, idx) {
                    return Some(Value::Null);
                }
                storage(obj)
                    .borrow()
                    .get(&ArrayKey::Int(idx))
                    .cloned()
                    .unwrap_or(Value::Null)
            }
            b"offsetset" => {
                let idx = match self.fa_offset(a.first().unwrap_or(&Value::Null)) {
                    Some(i) => i,
                    None => return Some(Value::Null),
                };
                if !self.fa_bounds(obj, idx) {
                    return Some(Value::Null);
                }
                let v = a.get(1).map_or(Value::Null, Value::deref_for_storage);
                let old = storage(obj)
                    .borrow_mut()
                    .get_mut(&ArrayKey::Int(idx))
                    .map(|slot| std::mem::replace(slot, v));
                drop(old);
                Value::Null
            }
            b"offsetunset" => {
                let idx = match self.fa_offset(a.first().unwrap_or(&Value::Null)) {
                    Some(i) => i,
                    None => return Some(Value::Null),
                };
                if !self.fa_bounds(obj, idx) {
                    return Some(Value::Null);
                }
                let old = storage(obj)
                    .borrow_mut()
                    .get_mut(&ArrayKey::Int(idx))
                    .map(|slot| std::mem::replace(slot, Value::Null));
                drop(old);
                Value::Null
            }
            b"getiterator" => self.fa_new_iterator(obj),
            b"fromarray" => self.fa_from_array(a),
            b"__wakeup" => {
                if fa_size(obj) == 0 {
                    let props = self.spl_std_props(obj);
                    let st = storage(obj);
                    for (_, v) in props.iter() {
                        st.borrow_mut().push(v.clone());
                    }
                    let keep: Vec<Vec<u8>> = obj
                        .borrow()
                        .properties
                        .iter()
                        .filter(|(k, _)| k.starts_with(b"__"))
                        .map(|(k, _)| k.clone())
                        .collect();
                    obj.borrow_mut()
                        .properties
                        .retain(|(k, _)| keep.contains(k));
                    sync_size(obj);
                }
                Value::Null
            }
            b"__serialize" => {
                let mut out = PhpArray::new();
                for (_, v) in storage(obj).borrow().iter() {
                    out.push(v.clone());
                }
                for (k, v) in self.spl_std_props(obj).iter() {
                    if let ArrayKey::String(_) = k {
                        out.set(k.clone(), v.clone());
                    }
                }
                val_arr(out)
            }
            b"__unserialize" => {
                let data = match a.first().map(|v| v.deref()) {
                    Some(Value::Array(d)) => d.borrow().clone(),
                    _ => PhpArray::new(),
                };
                if fa_size(obj) == 0 {
                    let st = storage(obj);
                    let mut members = PhpArray::new();
                    for (k, v) in data.iter() {
                        match k {
                            ArrayKey::Int(_) => st.borrow_mut().push(v.deref()),
                            ArrayKey::String(_) => members.set(k.clone(), v.clone()),
                        }
                    }
                    sync_size(obj);
                    self.spl_load_props(obj, &members);
                }
                Value::Null
            }
            b"__debuginfo" => val_arr(self.fa_debug_info(obj, true)),
            _ => return None,
        };
        Some(r)
    }

    /// SplFixedArray::getIterator(): an InternalIterator over the live array.
    /// (zend_create_internal_iterator_zval(): the zend_object_iterator it
    /// wraps holds a handle of its own, taken first.)
    fn fa_new_iterator(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Value {
        let h = self.internal_iterator_handle();
        let it = self.spl_instantiate(b"InternalIterator");
        {
            let mut ob = it.borrow_mut();
            ob.set_property_slice(b"__spl_fa_h", h);
            ob.set_property_slice(b"__spl_fa_target", Value::Object(obj.clone()));
            ob.set_property_slice(b"__spl_pos", Value::Long(0));
        }
        Value::Object(it)
    }

    /// InternalIterator methods (only the SplFixedArray kind is produced).
    pub(crate) fn spl_internal_iterator_call(
        &mut self,
        method: &[u8],
        it: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let wm = it.borrow().get_property(b"__spl_wm_target");
        if let Value::Object(map) = wm {
            return self.weakmap_iterator_call(method, it, &map);
        }
        let target = match it.borrow().get_property(b"__spl_fa_target") {
            Value::Object(o) => o,
            _ => return None,
        };
        let pos = match it.borrow().get_property(b"__spl_pos") {
            Value::Long(n) => n,
            _ => 0,
        };
        let r = match method {
            b"rewind" => {
                it.borrow_mut()
                    .set_property_slice(b"__spl_pos", Value::Long(0));
                Value::Null
            }
            b"valid" => {
                if pos >= 0 && pos < fa_size(&target) {
                    Value::True
                } else {
                    Value::False
                }
            }
            b"key" => Value::Long(pos),
            b"current" => {
                if pos < 0 || pos >= fa_size(&target) {
                    self.spl_throw(b"OutOfBoundsException", "Index invalid or out of range");
                    return Some(Value::Null);
                }
                storage(&target)
                    .borrow()
                    .get(&ArrayKey::Int(pos))
                    .cloned()
                    .unwrap_or(Value::Null)
            }
            b"next" => {
                it.borrow_mut()
                    .set_property_slice(b"__spl_pos", Value::Long(pos + 1));
                Value::Null
            }
            _ => return None,
        };
        Some(r)
    }

    /// The InternalIterator of WeakMap::getIterator(): the position among
    /// the map's live entries.
    fn weakmap_iterator_call(
        &mut self,
        method: &[u8],
        it: &Rc<RefCell<PhpObject>>,
        map: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let pos = match it.borrow().get_property(b"__spl_pos") {
            Value::Long(n) => n.max(0) as usize,
            _ => 0,
        };
        let map_id = map.borrow().object_id;
        let entry = |vm: &Self| -> Option<(Value, Value)> {
            vm.weak_map_entries.get(&map_id).and_then(|es| {
                es.iter()
                    .filter_map(|(_, w, v)| w.upgrade().map(|k| (Value::Object(k), v.clone())))
                    .nth(pos)
            })
        };
        Some(match method {
            b"rewind" => {
                it.borrow_mut()
                    .set_property(b"__spl_pos".to_vec(), Value::Long(0));
                Value::Null
            }
            b"valid" => {
                if entry(self).is_some() {
                    Value::True
                } else {
                    Value::False
                }
            }
            b"key" => entry(self).map(|(k, _)| k).unwrap_or(Value::Null),
            b"current" => entry(self).map(|(_, v)| v).unwrap_or(Value::Null),
            b"next" => {
                it.borrow_mut()
                    .set_property(b"__spl_pos".to_vec(), Value::Long(pos as i64 + 1));
                Value::Null
            }
            _ => return None,
        })
    }

    /// SplFixedArray::__construct(int $size = 0)
    pub fn spl_fixed_construct(&mut self, obj: &Rc<RefCell<PhpObject>>, args: &[Value]) -> bool {
        let func = "SplFixedArray::__construct";
        if args.len() > 1 {
            let msg = format!(
                "{}() expects at most 1 argument, {} given",
                func,
                args.len()
            );
            self.spl_throw(b"ArgumentCountError", &msg);
            return false;
        }
        let size = match args.first() {
            Some(v) => match self.spl_arg_long(func, 1, "size", v) {
                Some(n) => n,
                None => return false,
            },
            None => 0,
        };
        if size < 0 {
            let msg = format!(
                "{}(): Argument #1 ($size) must be greater than or equal to 0",
                func
            );
            self.spl_throw(b"ValueError", &msg);
            return false;
        }
        if fa_size(obj) != 0 {
            return true;
        }
        self.fa_resize(obj, size);
        true
    }

    /// Whether foreach over an SplFixedArray-based object should use the
    /// user's getIterator() override.
    pub(crate) fn fa_user_get_iterator(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        self.classes
            .get(&cl)
            .map(|c| c.get_method(b"getiterator").is_some())
            .unwrap_or(false)
    }
}

/// Native constructor handler for SplFixedArray.
pub fn spl_fixed_construct_native(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    vm.spl_push_ctor_frame(&obj, &args[1..]);
    let ok = vm.spl_fixed_construct(&obj, &args[1..]);
    vm.call_stack.pop();
    if ok {
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
