//! ArrayObject / ArrayIterator / RecursiveArrayIterator
//! (php-src ext/spl/spl_array.c).
//!
//! Hidden state:
//!   __spl_array      storage: an array, a wrapped object, or another
//!                    ArrayObject/ArrayIterator (USE_OTHER)
//!   __spl_flags      ar_flags (public flags plus IS_SELF/USE_OTHER)
//!   __spl_iterator_class  ArrayObject::getIterator() class
//!   __spl_pos / __spl_pos_key   iteration position (index + key, so the
//!                    position survives insertions/removals)
//!   __spl_sorting    > 0 while a sort is running

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{mangle_private, obj_class_lower, val_arr};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

pub(crate) const STD_PROP_LIST: i64 = 1;
pub(crate) const ARRAY_AS_PROPS: i64 = 2;
pub(crate) const CHILD_ARRAYS_ONLY: i64 = 4;
pub(crate) const IS_SELF: i64 = 0x0100_0000;
pub(crate) const USE_OTHER: i64 = 0x0200_0000;
const INT_MASK: i64 = 0xFFFF_0000;
/// `~SPL_ARRAY_INT_MASK`: the complement of an unsigned int constant,
/// which only keeps the low 16 bits of a zend_long.
const NOT_INT_MASK: i64 = 0xFFFF;
const CLONE_MASK: i64 = 0x0100_FFFF;

fn prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Value {
    obj.borrow().get_property(name)
}

fn set_prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8], v: Value) {
    obj.borrow_mut().set_property_slice(name, v);
}

pub(crate) fn ao_flags(obj: &Rc<RefCell<PhpObject>>) -> i64 {
    match prop(obj, b"__spl_flags") {
        Value::Long(f) => f,
        _ => 0,
    }
}

/// Names of hidden engine-internal properties (never part of a table).
pub(crate) fn is_hidden_prop(name: &[u8]) -> bool {
    name.starts_with(b"__spl_")
        || name.starts_with(b"__reflection_")
        || name.starts_with(b"__enum_")
        || name.starts_with(b"__ctor_")
        || name.starts_with(b"__clone_")
        || name.starts_with(b"__destructed")
        || name.starts_with(b"__fiber_")
        || name.starts_with(b"__timestamp")
        || name.starts_with(b"__lazy_")
        || name.starts_with(b"__weak_ref_id")
        || name.starts_with(b"__sxml_")
        || name.starts_with(b"__hash_")
        || name.starts_with(b"__hmac_")
        || name.starts_with(b"__pkey_id")
        || name.starts_with(b"__cert_id")
        || name.starts_with(b"__csr_id")
        || name.starts_with(b"__xml_")
        || name.starts_with(b"__regex_")
        || name.starts_with(b"__sensitive_value")
}

/// The hash table an ArrayObject operates on.
#[derive(Clone)]
pub(crate) enum Table {
    Arr(Rc<RefCell<PhpArray>>),
    Obj(Rc<RefCell<PhpObject>>),
}

fn key_name(k: &ArrayKey) -> Vec<u8> {
    match k {
        ArrayKey::Int(n) => n.to_string().into_bytes(),
        ArrayKey::String(s) => s.as_bytes().to_vec(),
    }
}

impl Vm {
    /// Whether objects of the class have a get_properties handler of their
    /// own (not zend_std_get_properties): spl_array rejects them.
    #[inline(never)]
    pub(crate) fn has_overloaded_properties(&self, class_lower: &[u8]) -> bool {
        self.spl_find_base(
            class_lower,
            &[
                b"splfixedarray",
                b"simplexmlelement",
                b"bcmath\\number",
                b"dateinterval",
                b"datetime",
                b"datetimeimmutable",
                b"datetimezone",
                b"dateperiod",
                // (get_properties_for or a get_properties handler)
                b"weakmap",
                b"sensitiveparametervalue",
                b"pdorow",
                b"ziparchive",
            ],
        )
        .is_some()
    }

    /// spl_array_get_hash_table()
    pub(crate) fn ao_table(&self, obj: &Rc<RefCell<PhpObject>>) -> Table {
        let mut cur = obj.clone();
        for _ in 0..32 {
            let flags = ao_flags(&cur);
            if flags & IS_SELF != 0 {
                return Table::Obj(cur);
            }
            match prop(&cur, b"__spl_array") {
                Value::Array(a) => return Table::Arr(a),
                Value::Object(o) => {
                    if flags & USE_OTHER != 0 {
                        cur = o;
                        continue;
                    }
                    // (an initialized lazy proxy's properties are its
                    // instance's)
                    return Table::Obj(self.proxy_redirect(&o));
                }
                _ => {
                    let a = Rc::new(RefCell::new(PhpArray::new()));
                    set_prop(&cur, b"__spl_array", Value::Array(a.clone()));
                    return Table::Arr(a);
                }
            }
        }
        Table::Arr(Rc::new(RefCell::new(PhpArray::new())))
    }

    /// spl_array_is_object()
    pub(crate) fn ao_is_object(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        matches!(self.ao_table(obj), Table::Obj(_))
    }

    /// Whether a property of `o` is visible through an ArrayObject table
    /// (public or dynamic, initialized).
    fn ao_prop_visible(&self, o: &Rc<RefCell<PhpObject>>, name: &[u8], v: &Value) -> bool {
        if is_hidden_prop(name) || matches!(v, Value::Undef) || name.first() == Some(&0) {
            return false;
        }
        let cl = obj_class_lower(o);
        match self.find_instance_prop_def(&cl, name) {
            Some((vis, _)) => vis == crate::object::Visibility::Public,
            None => true,
        }
    }

    pub(crate) fn table_entries(&self, t: &Table) -> Vec<(ArrayKey, Value)> {
        match t {
            Table::Arr(a) => a
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            Table::Obj(o) => {
                let props: Vec<(Vec<u8>, Value)> = o.borrow().properties.to_vec();
                props
                    .into_iter()
                    .filter(|(n, v)| self.ao_prop_visible(o, n, v))
                    .map(|(n, v)| (ArrayKey::String(PhpString::from_vec(n)), v))
                    .collect()
            }
        }
    }

    fn table_get(&self, t: &Table, k: &ArrayKey) -> Option<Value> {
        match t {
            Table::Arr(a) => a.borrow().get(k).cloned(),
            Table::Obj(o) => {
                let name = key_name(k);
                let ob = o.borrow();
                let found = ob
                    .properties
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.clone());
                drop(ob);
                match found {
                    Some(v) if self.ao_prop_visible(o, &name, &v) => Some(v),
                    _ => None,
                }
            }
        }
    }

    fn table_set(&self, t: &Table, k: ArrayKey, v: Value) {
        match t {
            Table::Arr(a) => {
                let mut ab = a.borrow_mut();
                // Write through references stored in the table.
                if let Some(Value::Reference(r)) = ab.get(&k) {
                    let r = r.clone();
                    drop(ab);
                    *r.borrow_mut() = v.deref();
                    return;
                }
                ab.set(k, v);
            }
            Table::Obj(o) => {
                let name = key_name(&k);
                let mut ob = o.borrow_mut();
                for (n, slot) in ob.properties.iter_mut() {
                    if *n == name {
                        if let Value::Reference(r) = slot {
                            *r.borrow_mut() = v.deref();
                        } else {
                            *slot = v;
                        }
                        return;
                    }
                }
                ob.properties.push((name, v));
            }
        }
    }

    fn table_remove(&self, t: &Table, k: &ArrayKey) -> Option<Value> {
        match t {
            Table::Arr(a) => a.borrow_mut().remove(k),
            Table::Obj(o) => {
                let name = key_name(k);
                let mut ob = o.borrow_mut();
                let pos = ob.properties.iter().position(|(n, _)| *n == name)?;
                Some(ob.properties.remove(pos).1)
            }
        }
    }

    fn table_count(&self, t: &Table) -> usize {
        match t {
            Table::Arr(a) => a.borrow().len(),
            Table::Obj(_) => self.table_entries(t).len(),
        }
    }

    /// zend_array_dup() of the table: an object's whole property table
    /// (mangled names of protected/private properties included).
    fn table_dup(&self, t: &Table) -> PhpArray {
        match t {
            Table::Arr(a) => a.borrow().clone(),
            Table::Obj(o) => self.object_props_mangled(o).borrow().clone(),
        }
    }

    fn table_copy(&self, t: &Table) -> PhpArray {
        match t {
            Table::Arr(a) => a.borrow().clone(),
            Table::Obj(_) => {
                let mut out = PhpArray::new();
                for (k, v) in self.table_entries(t) {
                    out.set(k, v);
                }
                out
            }
        }
    }

    /// Display name used in error messages (the object's class).
    fn ao_class_disp(obj: &Rc<RefCell<PhpObject>>) -> String {
        crate::value::display_class_name(&obj.borrow().class_name).to_string()
    }

    /// get_hash_key(): Ok(key) or Err(type name) for illegal offsets.
    fn ao_hash_key(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        offset: &Value,
    ) -> Result<ArrayKey, String> {
        let k = match offset.deref() {
            Value::Null | Value::Undef => {
                self.emit_deprecated(
                    "Using null as an array offset is deprecated, use an empty string instead",
                );
                ArrayKey::String(PhpString::empty())
            }
            Value::String(s) => Self::value_to_array_key(Value::String(s)),
            Value::Resource(r) => {
                let msg = format!(
                    "Resource ID#{} used as offset, casting to integer ({})",
                    r, r
                );
                self.emit_warning(&msg);
                ArrayKey::Int(r.id())
            }
            Value::Double(d) => ArrayKey::Int(self.dval_to_lval_safe(d)),
            Value::False => ArrayKey::Int(0),
            Value::True => ArrayKey::Int(1),
            Value::Long(n) => ArrayKey::Int(n),
            Value::Array(_) => return Err("array".to_string()),
            Value::Object(o) => {
                return Err(crate::value::display_class_name(&o.borrow().class_name).to_string());
            }
            other => return Err(Self::value_type_name(&other)),
        };
        // Object tables only have string keys.
        if self.ao_is_object(obj) {
            if let ArrayKey::Int(n) = k {
                return Ok(ArrayKey::String(PhpString::from_string(n.to_string())));
            }
        }
        Ok(k)
    }

    fn ao_illegal_offset(&mut self, obj: &Rc<RefCell<PhpObject>>, tname: &str, kind: u8) {
        let cls = Self::ao_class_disp(obj);
        let msg = match kind {
            b'i' => format!("Cannot access offset of type {} in isset or empty", tname),
            b'u' => format!("Cannot unset offset of type {} on {}", tname, cls),
            _ => format!("Cannot access offset of type {} on {}", tname, cls),
        };
        self.spl_throw(b"TypeError", &msg);
    }

    fn ao_sorting(obj: &Rc<RefCell<PhpObject>>) -> bool {
        matches!(prop(obj, b"__spl_sorting"), Value::Long(n) if n > 0)
    }

    fn ao_sort_check(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        // The sort lock lives on the object whose table is being sorted.
        if Self::ao_sorting(obj) {
            self.spl_throw(
                b"Error",
                "Modification of ArrayObject during sorting is prohibited",
            );
            return false;
        }
        true
    }

    fn ao_undefined_key(&mut self, k: &ArrayKey) {
        let msg = match k {
            ArrayKey::Int(n) => format!("Undefined array key {}", n),
            ArrayKey::String(s) => format!("Undefined array key \"{}\"", s.to_msg_string()),
        };
        self.emit_warning(&msg);
    }

    /// ArrayObject::offsetGet() (read dimension, BP_VAR_R).
    pub(crate) fn ao_offset_get(&mut self, obj: &Rc<RefCell<PhpObject>>, offset: &Value) -> Value {
        let k = match self.ao_hash_key(obj, offset) {
            Ok(k) => k,
            Err(t) => {
                self.ao_illegal_offset(obj, &t, b'r');
                return Value::Null;
            }
        };
        let t = self.ao_table(obj);
        match self.table_get(&t, &k) {
            Some(v) => v.deref(),
            None => {
                self.ao_undefined_key(&k);
                Value::Null
            }
        }
    }

    /// spl_array_read_dimension() in a write context (BP_VAR_W / RW / UNSET,
    /// `mode` b'w' / b'r' / b'u'): the element a nested write goes into,
    /// vivified to an array (W/RW) and separated so writes reach the storage.
    /// `None` stands for &EG(uninitialized_zval).
    pub(crate) fn ao_fetch_dim_w(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        offset: Option<&Value>,
        mode: u8,
    ) -> Option<Value> {
        let offset = offset?;
        if mode != b'u' && !self.ao_sort_check(obj) {
            return Some(Value::Null);
        }
        let k = match self.ao_hash_key(obj, offset) {
            Ok(k) => k,
            Err(t) => {
                self.ao_illegal_offset(obj, &t, if mode == b'u' { b'u' } else { b'w' });
                return if mode == b'u' {
                    None
                } else {
                    Some(Value::Null)
                };
            }
        };
        let t = self.ao_table(obj);
        let cur = match self.table_get(&t, &k) {
            None | Some(Value::Undef) => {
                if mode == b'u' {
                    return None;
                }
                if mode == b'r' {
                    self.ao_undefined_key(&k);
                }
                Value::Null
            }
            Some(v) => v.deref(),
        };
        match cur {
            Value::Null if mode != b'u' => {
                let arr = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
                self.table_set(&t, k, arr.clone());
                Some(arr)
            }
            Value::Array(rc) if Rc::strong_count(&rc) > 2 => {
                let sep = Value::Array(Rc::new(RefCell::new(rc.borrow().clone())));
                self.table_set(&t, k, sep.clone());
                Some(sep)
            }
            other => Some(other),
        }
    }

    /// `$ao[k] = &$v` on an ArrayObject / ArrayIterator without a user
    /// offsetGet(): the reference is bound into the storage
    /// (spl_array_get_dimension_ptr()). None when `obj` is not one;
    /// Some(false) with an exception pending.
    pub(crate) fn ao_assign_ref(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        offset: Option<&Value>,
        r: &Value,
    ) -> Option<bool> {
        let cl = obj_class_lower(obj);
        self.spl_find_base(&cl, &[b"arrayobject", b"arrayiterator"])?;
        if self.ao_user_has(obj, b"offsetget") {
            return None;
        }
        if !matches!(r, Value::Reference(_)) {
            return None;
        }
        if !self.ao_sort_check(obj) {
            return Some(false);
        }
        let t = self.ao_table(obj);
        let k = match offset.map(|o| o.deref()) {
            None | Some(Value::Null) => None,
            Some(o) => match self.ao_hash_key(obj, &o) {
                Ok(k) => Some(k),
                Err(tn) => {
                    self.ao_illegal_offset(obj, &tn, b'w');
                    return Some(false);
                }
            },
        };
        match &t {
            Table::Arr(a) => {
                let mut ab = a.borrow_mut();
                match k {
                    Some(k) => ab.set(k, r.clone()),
                    None => ab.push(r.clone()),
                }
            }
            Table::Obj(o) => {
                let name = key_name(&k?);
                let mut ob = o.borrow_mut();
                match ob.properties.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, slot)) => *slot = r.clone(),
                    None => ob.properties.push((name, r.clone())),
                }
            }
        }
        Some(self.current_exception.is_none())
    }

    /// spl_array_get_dimension_ptr(BP_VAR_W) for `&$ao[k]`: the storage
    /// slot made a reference. `None` when an exception is pending.
    pub(crate) fn ao_fetch_dim_ref(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        offset: &Value,
    ) -> Option<Value> {
        if !self.ao_sort_check(obj) {
            return None;
        }
        let k = match self.ao_hash_key(obj, offset) {
            Ok(k) => k,
            Err(t) => {
                self.ao_illegal_offset(obj, &t, b'w');
                return None;
            }
        };
        let t = self.ao_table(obj);
        if let Some(Value::Reference(r)) = self.table_get(&t, &k) {
            return Some(Value::Reference(r));
        }
        let cur = match self.table_get(&t, &k) {
            None | Some(Value::Undef) => Value::Null,
            Some(v) => v,
        };
        let r = Rc::new(RefCell::new(cur));
        match &t {
            Table::Arr(a) => a.borrow_mut().set(k, Value::Reference(r.clone())),
            Table::Obj(o) => {
                let name = key_name(&k);
                let mut ob = o.borrow_mut();
                match ob.properties.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, slot)) => *slot = Value::Reference(r.clone()),
                    None => ob.properties.push((name, Value::Reference(r.clone()))),
                }
            }
        }
        Some(Value::Reference(r))
    }

    pub(crate) fn ao_offset_set(&mut self, obj: &Rc<RefCell<PhpObject>>, offset: &Value, v: Value) {
        if !self.ao_sort_check(obj) {
            return;
        }
        let t = self.ao_table(obj);
        let v = v.deref();
        if matches!(offset.deref(), Value::Null | Value::Undef) {
            match &t {
                Table::Arr(a) => {
                    if let Err(msg) = a.borrow_mut().try_push(v) {
                        self.spl_throw(b"Error", msg);
                    }
                }
                Table::Obj(_) => {
                    // zend_hash_next_index_insert on a property table
                    let n = self.table_entries(&t).len();
                    self.table_set(
                        &t,
                        ArrayKey::String(PhpString::from_string(n.to_string())),
                        v,
                    );
                }
            }
            return;
        }
        let k = match self.ao_hash_key(obj, offset) {
            Ok(k) => k,
            Err(tn) => {
                self.ao_illegal_offset(obj, &tn, b'w');
                return;
            }
        };
        self.table_set(&t, k, v);
    }

    pub(crate) fn ao_offset_unset(&mut self, obj: &Rc<RefCell<PhpObject>>, offset: &Value) {
        if !self.ao_sort_check(obj) {
            return;
        }
        let k = match self.ao_hash_key(obj, offset) {
            Ok(k) => k,
            Err(tn) => {
                self.ao_illegal_offset(obj, &tn, b'u');
                return;
            }
        };
        let t = self.ao_table(obj);
        let old = self.table_remove(&t, &k);
        drop(old);
    }

    /// Whether the object's (user) class overrides an ArrayAccess method.
    pub(crate) fn ao_user_has(&self, obj: &Rc<RefCell<PhpObject>>, m: &[u8]) -> bool {
        let cl = obj_class_lower(obj);
        self.classes
            .get(&cl)
            .map(|c| c.get_method(m).is_some())
            .unwrap_or(false)
    }

    /// Whether the class's read_dimension handler (ArrayObject's,
    /// SplFixedArray's) calls an overriding offsetGet(), handing out
    /// uninitialized_zval rather than NULL when it throws: `$obj[k] op= v`
    /// is then not zend_use_object_as_array().
    #[inline(never)]
    pub(crate) fn spl_read_dim_never_null(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        class_lower: &[u8],
    ) -> bool {
        self.spl_find_base(
            class_lower,
            &[b"arrayobject", b"arrayiterator", b"splfixedarray"],
        )
        .is_some()
            && self.ao_user_has(obj, b"offsetget")
    }

    /// The read of `$obj[] op= v` by an SPL read_dimension handler: Some
    /// when it does not call offsetGet(null) (an exception may be set).
    /// ArrayObject / ArrayIterator find no element, or pass no argument to
    /// an overriding offsetGet(); SplFixedArray does not support `[]`.
    #[inline(never)]
    pub(crate) fn spl_dim_read_no_offset(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        class_lower: &[u8],
    ) -> Option<Value> {
        let user_get = self.ao_user_has(obj, b"offsetget");
        if self
            .spl_find_base(class_lower, &[b"arrayobject", b"arrayiterator"])
            .is_some()
        {
            if !user_get {
                // spl_array_get_dimension_ptr(): uninitialized_zval
                return Some(Value::Null);
            }
            let m = self
                .classes
                .get(class_lower)
                .and_then(|c| c.get_method(b"offsetget"))
                .map(|m| {
                    (
                        m.declaring_class.clone(),
                        m.op_array
                            .cv_names
                            .iter()
                            .find(|n| n.as_slice() != b"this")
                            .cloned()
                            .unwrap_or_default(),
                    )
                });
            let (decl, pname) = m?;
            let decl = self
                .class_name_for_display(&decl.to_ascii_lowercase())
                .unwrap_or(decl);
            let msg = format!(
                "{}::offsetGet(): Argument #1 (${}) not passed",
                crate::value::display_class_name(&decl),
                crate::string::msg_str(&pname)
            );
            self.spl_throw(b"ArgumentCountError", &msg);
            return Some(Value::Null);
        }
        if !user_get
            && self
                .spl_find_base(class_lower, &[b"splfixedarray"])
                .is_some()
        {
            self.spl_throw(b"Error", "[] operator not supported for SplFixedArray");
            return Some(Value::Null);
        }
        None
    }

    /// ARRAY_AS_PROPS routes a property access to the storage when no real
    /// property of that name exists (zend_std_has_property(EXISTS)).
    pub(crate) fn ao_prop_as_dim(&self, obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> bool {
        if name.starts_with(b"__spl_") {
            return false;
        }
        let cl = obj_class_lower(obj);
        if self
            .spl_find_base(&cl, &[b"arrayobject", b"arrayiterator"])
            .is_none()
        {
            return false;
        }
        if ao_flags(obj) & ARRAY_AS_PROPS == 0 {
            return false;
        }
        if !obj.borrow().has_property(name) {
            return true;
        }
        // zend_std_has_property(): a property the calling scope cannot see
        // does not exist.
        let Some((vis, decl, ..)) = self.find_property_def(&cl, name) else {
            return false;
        };
        let scope = self
            .current_class_scope()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let decl = decl.to_ascii_lowercase();
        match vis {
            crate::object::Visibility::Public => false,
            crate::object::Visibility::Private => scope != decl,
            crate::object::Visibility::Protected => {
                scope.is_empty()
                    || !(scope == decl
                        || self.class_extends(&scope, &decl)
                        || self.class_extends(&decl, &scope))
            }
        }
    }

    /// spl_array_has_dimension_ex(check_inherited=true, check_empty=0): isset().
    pub(crate) fn ao_has_dimension(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        offset: &Value,
    ) -> bool {
        if self.ao_user_has(obj, b"offsetexists") {
            let ov = Value::Object(obj.clone());
            let r = self
                .call_object_method(&ov, b"offsetExists", std::slice::from_ref(offset))
                .unwrap_or(Value::False);
            return self.current_exception.is_none() && r.is_truthy();
        }
        self.ao_offset_exists(obj, offset, 0)
    }

    /// First half of empty() (check_empty=1): whether the following fetch
    /// must run. With a user offsetGet() and no offsetExists(), any stored
    /// entry (even null) is read through offsetGet().
    #[inline(never)]
    pub(crate) fn ao_empty_probe(&mut self, obj: &Rc<RefCell<PhpObject>>, offset: &Value) -> bool {
        if !self.ao_user_has(obj, b"offsetexists") && self.ao_user_has(obj, b"offsetget") {
            return self.ao_offset_exists(obj, offset, 2);
        }
        if !self.ao_user_has(obj, b"offsetexists") {
            // (check_empty=1: the stored value's truthiness)
            return self.ao_offset_exists(obj, offset, 1);
        }
        self.ao_has_dimension(obj, offset)
    }

    /// An object table's slot of a declared public property:
    /// spl_array_has_dimension_ex() sees the IS_INDIRECT zval itself, which
    /// is "set" (even uninitialized or unset) and never true (empty()).
    fn ao_declared_slot(&self, t: &Table, k: &ArrayKey) -> bool {
        let (Table::Obj(o), ArrayKey::String(name)) = (t, k) else {
            return false;
        };
        let name = name.as_bytes();
        if is_hidden_prop(name) || name.first() == Some(&0) {
            return false;
        }
        let cl = obj_class_lower(o);
        matches!(
            self.find_instance_prop_def(&cl, name),
            Some((crate::object::Visibility::Public, _))
        )
    }

    /// has_property(ZEND_PROPERTY_EXISTS) for property_exists(): Some(result)
    /// when ARRAY_AS_PROPS routes the name to the storage.
    pub fn spl_property_exists_hook(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        name: &[u8],
    ) -> Option<bool> {
        if !self.ao_prop_as_dim(obj, name) {
            return None;
        }
        let key = Value::String(PhpString::from_bytes(name));
        if self.ao_user_has(obj, b"offsetexists") {
            let ov = Value::Object(obj.clone());
            let r = self
                .call_object_method(&ov, b"offsetExists", std::slice::from_ref(&key))
                .unwrap_or(Value::False);
            if self.current_exception.is_some() || !r.is_truthy() {
                return Some(false);
            }
        }
        Some(self.ao_offset_exists(obj, &key, 2))
    }

    /// spl_array_unset_dimension_ex(check_inherited=true).
    pub(crate) fn ao_unset_dimension(&mut self, obj: &Rc<RefCell<PhpObject>>, offset: &Value) {
        if self.ao_user_has(obj, b"offsetunset") {
            let ov = Value::Object(obj.clone());
            self.call_object_method(&ov, b"offsetUnset", std::slice::from_ref(offset));
            return;
        }
        self.ao_offset_unset(obj, offset)
    }

    /// spl_array_has_dimension_ex(check_inherited=false): check_empty 0/1/2.
    pub(crate) fn ao_offset_exists(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        offset: &Value,
        check_empty: u8,
    ) -> bool {
        // isset()/empty() are quiet fetches, but converting the offset
        // still reports its diagnostics.
        let saved = self.unsilence_begin();
        let k = self.ao_hash_key(obj, offset);
        self.unsilence_end(saved);
        let k = match k {
            Ok(k) => k,
            Err(tn) => {
                self.ao_illegal_offset(obj, &tn, b'i');
                return false;
            }
        };
        let t = self.ao_table(obj);
        if self.ao_declared_slot(&t, &k) {
            return check_empty != 1;
        }
        match self.table_get(&t, &k) {
            None => false,
            Some(v) => {
                if check_empty == 2 {
                    true
                } else if check_empty == 1 {
                    v.deref().is_truthy()
                } else {
                    !matches!(v.deref(), Value::Null)
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Iteration
    // ------------------------------------------------------------------

    /// The object holding the table an ArrayObject / ArrayIterator works
    /// on (the end of its USE_OTHER chain).
    fn ao_table_owner(&self, obj: &Rc<RefCell<PhpObject>>) -> Rc<RefCell<PhpObject>> {
        let mut cur = obj.clone();
        for _ in 0..32 {
            if ao_flags(&cur) & (IS_SELF | USE_OTHER) != USE_OTHER {
                break;
            }
            match prop(&cur, b"__spl_array") {
                Value::Object(o) => cur = o,
                _ => break,
            }
        }
        cur
    }

    /// How many times the table was sorted through an ArrayObject /
    /// ArrayIterator method (sorting keeps the bucket index of every hash
    /// iterator of the table, so their keys are stale after it).
    fn ao_sort_epoch(&self, obj: &Rc<RefCell<PhpObject>>) -> i64 {
        match prop(&self.ao_table_owner(obj), b"__spl_sort_epoch") {
            Value::Long(n) => n,
            _ => 0,
        }
    }

    /// Current (index, entry) of the iterator, resolving a stale index by key.
    pub(crate) fn ao_current(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<(usize, ArrayKey, Value)> {
        let t = self.ao_table(obj);
        let (mut idx, mut pos_key, pos_epoch) = {
            let ob = obj.borrow();
            let idx = match ob.get_property(b"__spl_pos") {
                Value::Long(n) if n >= 0 => n as usize,
                _ => 0,
            };
            let key = match ob.get_property(b"__spl_pos_key") {
                k @ (Value::Long(_) | Value::String(_)) => Some(Self::value_to_array_key(k)),
                _ => None,
            };
            let epoch = match ob.get_property(b"__spl_pos_epoch") {
                Value::Long(n) => n,
                _ => 0,
            };
            (idx, key, epoch)
        };
        if pos_key.is_some() {
            let epoch = self.ao_sort_epoch(obj);
            if epoch != pos_epoch {
                // Sorted since the position was taken: the index stands.
                pos_key = None;
                let (_, k) = self.table_len_key_at(&t, idx);
                self.ao_set_pos(obj, idx, k.as_ref());
            }
        }
        match &t {
            Table::Arr(a) => {
                let ab = a.borrow();
                if let Some(key) = pos_key {
                    if ab.entry_at(idx).map(|(ek, _)| *ek != key).unwrap_or(true) {
                        if let Some(i) = ab.position_of(&key) {
                            idx = i;
                        }
                    }
                }
                ab.entry_at(idx).map(|(k, v)| (idx, k.clone(), v.clone()))
            }
            Table::Obj(_) => {
                let entries = self.table_entries(&t);
                if let Some(key) = pos_key {
                    if entries.get(idx).map(|(ek, _)| *ek != key).unwrap_or(true) {
                        if let Some(i) = entries.iter().position(|(ek, _)| *ek == key) {
                            idx = i;
                        }
                    }
                }
                entries.into_iter().nth(idx).map(|(k, v)| (idx, k, v))
            }
        }
    }

    /// Store the iterator position: the index plus the key found there (so
    /// the position survives insertions/removals); no key past the end.
    fn ao_set_pos(&self, obj: &Rc<RefCell<PhpObject>>, idx: usize, key: Option<&ArrayKey>) {
        let epoch = self.ao_sort_epoch(obj);
        let mut ob = obj.borrow_mut();
        if epoch != 0 || ob.has_property(b"__spl_pos_epoch") {
            ob.set_property_slice(b"__spl_pos_epoch", Value::Long(epoch));
        }
        ob.set_property_slice(b"__spl_pos", Value::Long(idx as i64));
        ob.set_property_slice(
            b"__spl_pos_key",
            match key {
                Some(k) => Self::key_value(k),
                None => Value::Null,
            },
        );
    }

    /// Number of entries of a table and the key at `idx`.
    fn table_len_key_at(&self, t: &Table, idx: usize) -> (usize, Option<ArrayKey>) {
        match t {
            Table::Arr(a) => {
                let ab = a.borrow();
                (ab.len(), ab.entry_at(idx).map(|(k, _)| k.clone()))
            }
            Table::Obj(_) => {
                let entries = self.table_entries(t);
                let k = entries.get(idx).map(|(k, _)| k.clone());
                (entries.len(), k)
            }
        }
    }

    pub(crate) fn ao_rewind(&self, obj: &Rc<RefCell<PhpObject>>) {
        let t = self.ao_table(obj);
        let (_, k) = self.table_len_key_at(&t, 0);
        self.ao_set_pos(obj, 0, k.as_ref());
    }

    pub(crate) fn ao_next(&self, obj: &Rc<RefCell<PhpObject>>) {
        let t = self.ao_table(obj);
        // zend_hash_move_forward_ex() does not move an exhausted position,
        // so elements appended later become current.
        let next = match self.ao_current(obj) {
            Some((i, _, _)) => i + 1,
            None => {
                let len = self.table_len_key_at(&t, 0).0;
                match prop(obj, b"__spl_pos") {
                    Value::Long(n) if n >= 0 => (n as usize).min(len),
                    _ => len,
                }
            }
        };
        let (_, k) = self.table_len_key_at(&t, next);
        self.ao_set_pos(obj, next, k.as_ref());
    }

    pub(crate) fn ao_valid(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        self.ao_current(obj).is_some()
    }

    fn key_value(k: &ArrayKey) -> Value {
        match k {
            ArrayKey::Int(n) => Value::Long(*n),
            ArrayKey::String(s) => Value::String(s.clone()),
        }
    }

    // ------------------------------------------------------------------
    // Construction / storage
    // ------------------------------------------------------------------

    fn ao_base_name(&self, obj: &Rc<RefCell<PhpObject>>) -> &'static str {
        let cl = obj_class_lower(obj);
        if self.spl_find_base(&cl, &[b"arrayiterator", b"arrayobject"]) == Some(b"arrayiterator") {
            "ArrayIterator"
        } else {
            "ArrayObject"
        }
    }

    /// spl_array_set_array(). `func` is the active function for messages.
    pub(crate) fn ao_set_array(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        array: &Value,
        mut ar_flags: i64,
        just_array: bool,
        func: &str,
    ) -> bool {
        match array.deref() {
            Value::Array(a) => {
                let copy = a.borrow().clone();
                set_prop(obj, b"__spl_array", val_arr(copy));
            }
            Value::Object(o) => {
                let base = self.ao_base_name(obj);
                let msg = format!(
                    "{}(): Using an object as a backing array for {} is deprecated, as it allows violating class constraints and invariants",
                    func, base
                );
                self.emit_deprecated(&msg);
                if self.current_exception.is_some() {
                    return false;
                }
                let ocl = obj_class_lower(&o);
                let other_is_ao = self
                    .spl_find_base(&ocl, &[b"arrayobject", b"arrayiterator"])
                    .is_some();
                if other_is_ao {
                    if just_array {
                        ar_flags = ao_flags(&o) & NOT_INT_MASK;
                    }
                    if Rc::ptr_eq(&o, obj) {
                        ar_flags |= IS_SELF;
                        set_prop(obj, b"__spl_array", Value::Null);
                    } else {
                        ar_flags |= USE_OTHER;
                        set_prop(obj, b"__spl_array", Value::Object(o.clone()));
                    }
                } else {
                    let cls_disp = Self::ao_class_disp(obj);
                    let is_enum = self.classes.get(&ocl).map(|c| c.is_enum).unwrap_or(false);
                    if is_enum {
                        let m = format!("Enums are not compatible with {}", cls_disp);
                        self.spl_throw(b"InvalidArgumentException", &m);
                        return false;
                    }
                    let overloaded = self.has_overloaded_properties(&ocl);
                    if overloaded {
                        let m = format!(
                            "Overloaded object of type {} is not compatible with {}",
                            crate::value::display_class_name(&o.borrow().class_name),
                            cls_disp
                        );
                        self.spl_throw(b"InvalidArgumentException", &m);
                        return false;
                    }
                    set_prop(obj, b"__spl_array", Value::Object(o.clone()));
                }
            }
            g @ Value::Generator(_) => {
                // (a Generator has no properties: the storage is empty)
                let base = self.ao_base_name(obj);
                let msg = format!(
                    "{}(): Using an object as a backing array for {} is deprecated, as it allows violating class constraints and invariants",
                    func, base
                );
                self.emit_deprecated(&msg);
                if self.current_exception.is_some() {
                    return false;
                }
                set_prop(obj, b"__spl_array", g);
            }
            _ => {}
        }
        let flags = (ao_flags(obj) & !IS_SELF & !USE_OTHER) | ar_flags;
        set_prop(obj, b"__spl_flags", Value::Long(flags));
        set_prop(obj, b"__spl_pos", Value::Long(0));
        set_prop(obj, b"__spl_pos_key", Value::Null);
        true
    }

    /// ArrayObject/ArrayIterator constructor. `args` excludes $this.
    pub fn ao_construct(&mut self, obj: &Rc<RefCell<PhpObject>>, args: &[Value]) -> bool {
        let base = self.ao_base_name(obj);
        let func = format!("{}::__construct", base);
        if !matches!(
            prop(obj, b"__spl_array"),
            Value::Array(_) | Value::Object(_)
        ) {
            set_prop(obj, b"__spl_array", val_arr(PhpArray::new()));
        }
        let max = if base == "ArrayObject" { 3 } else { 2 };
        if args.len() > max {
            let msg = format!(
                "{}() expects at most {} arguments, {} given",
                func,
                max,
                args.len()
            );
            self.spl_throw(b"ArgumentCountError", &msg);
            return false;
        }
        if args.is_empty() {
            return true;
        }
        let array = args[0].deref();
        if !matches!(
            array,
            Value::Array(_) | Value::Object(_) | Value::Generator(_)
        ) {
            let msg = format!(
                "{}(): Argument #1 ($array) must be of type array, {} given",
                func,
                Self::value_type_name(&array)
            );
            self.spl_throw(b"TypeError", &msg);
            return false;
        }
        let flags = match args.get(1) {
            Some(v) => match self.spl_arg_long(&func, 2, "flags", v) {
                Some(n) => n,
                None => return false,
            },
            None => 0,
        };
        if base == "ArrayObject" {
            if let Some(cv) = args.get(2) {
                match self.ao_iterator_class_arg(&func, 3, "iteratorClass", cv) {
                    Some(c) => set_prop(obj, b"__spl_iterator_class", Value::String(c)),
                    None => return false,
                }
            }
        }
        self.ao_set_array(obj, &array, flags & NOT_INT_MASK, args.len() == 1, &func)
    }

    /// Z_PARAM_CLASS with ArrayIterator as base.
    fn ao_iterator_class_arg(
        &mut self,
        func: &str,
        argn: usize,
        name: &str,
        v: &Value,
    ) -> Option<PhpString> {
        // zend_parse_arg_class(): try_convert_to_string() (an object that
        // cannot be converted throws, an array warns), then the class lookup
        let s = match v.deref() {
            Value::String(s) => s,
            o @ Value::Object(_) => match self.value_to_string_checked(&o) {
                Ok(conv) if self.current_exception.is_none() => conv,
                _ => return None,
            },
            Value::Array(_) => {
                self.emit_warning("Array to string conversion");
                if self.current_exception.is_some() {
                    return None;
                }
                PhpString::from_bytes(b"Array")
            }
            other => other.to_php_string(),
        };
        let cl: Vec<u8> = s
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        if !self.classes.contains_key(&cl) && !self.is_known_builtin_class(&cl) {
            self.try_autoload_class(s.as_bytes());
        }
        let ok = (self.classes.contains_key(&cl) || self.is_known_builtin_class(&cl))
            && self.spl_find_base(&cl, &[b"arrayiterator"]).is_some();
        if !ok {
            let msg = format!(
                "{}(): Argument #{} (${}) must be a class name derived from ArrayIterator, {} given",
                func,
                argn,
                name,
                s.to_msg_string()
            );
            self.spl_throw(b"TypeError", &msg);
            return None;
        }
        let canonical = self
            .classes
            .get(&cl)
            .map(|c| PhpString::from_vec(c.name.clone()))
            .unwrap_or_else(|| PhpString::from_string(self.builtin_canonical_name(&cl)));
        Some(canonical)
    }

    /// Debug info: properties plus the private storage entry.
    pub(crate) fn ao_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        let mut out = self.spl_std_props(obj);
        if ao_flags(obj) & IS_SELF != 0 {
            return out;
        }
        let storage = match prop(obj, b"__spl_array") {
            v @ (Value::Array(_) | Value::Object(_) | Value::Generator(_)) => v,
            _ => val_arr(PhpArray::new()),
        };
        let base = self.ao_base_name(obj);
        out.set(
            ArrayKey::String(PhpString::from_vec(mangle_private(
                base.as_bytes(),
                b"storage",
            ))),
            storage,
        );
        out
    }

    /// get_properties_for(ARRAY_CAST / VAR_EXPORT / JSON).
    pub(crate) fn ao_properties_for_cast(&self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        if ao_flags(obj) & STD_PROP_LIST != 0 {
            return self.spl_std_props(obj);
        }
        let t = self.ao_table(obj);
        let copy = self.table_dup(&t);
        // proptable -> symtable for object tables
        let mut out = PhpArray::new();
        for (k, v) in copy.iter() {
            out.set(Self::value_to_array_key(Self::key_value(k)), v.clone());
        }
        out
    }

    /// Storage and flags for a clone (spl_array_object_new_ex clone_orig).
    pub(crate) fn ao_clone_storage(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        is_array_object: bool,
    ) -> (Value, i64) {
        let flags = ao_flags(obj) & CLONE_MASK;
        if flags & IS_SELF != 0 {
            return (Value::Null, flags);
        }
        if is_array_object {
            let t = self.ao_table(obj);
            (val_arr(self.table_dup(&t)), flags)
        } else {
            (Value::Object(obj.clone()), flags | USE_OTHER)
        }
    }

    fn ao_new_iterator(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Value {
        let class = match prop(obj, b"__spl_iterator_class") {
            Value::String(s) if !s.as_bytes().is_empty() => s.as_bytes().to_vec(),
            _ => b"ArrayIterator".to_vec(),
        };
        let it = self.spl_instantiate(&class);
        let flags = (ao_flags(obj) & CLONE_MASK) | USE_OTHER;
        {
            let mut ob = it.borrow_mut();
            ob.set_property_slice(b"__spl_array", Value::Object(obj.clone()));
            ob.set_property_slice(b"__spl_flags", Value::Long(flags & !IS_SELF));
            ob.set_property_slice(b"__spl_pos", Value::Long(0));
        }
        if ao_flags(obj) & IS_SELF != 0 {
            // iterator over the object's own properties
            it.borrow_mut()
                .set_property_slice(b"__spl_flags", Value::Long(flags));
        }
        Value::Object(it)
    }

    fn ao_sort(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        fname: &[u8],
        extra: Option<Value>,
    ) -> Option<Value> {
        let fname_s = String::from_utf8_lossy(fname).to_string();
        if self.disabled_functions_contains(fname) || !self.functions.contains_key(fname) {
            let msg = format!(
                "Cannot call method {} when function {} is disabled",
                fname_s, fname_s
            );
            return self.spl_throw(b"Error", &msg);
        }
        let t = self.ao_table(obj);
        let phys = self.ao_physical_pos(obj, &t);
        let copy = self.table_copy(&t);
        // Sort builtins sort their array argument in place.
        let sorted_rc = Rc::new(RefCell::new(copy));
        let mut args = vec![Value::Array(sorted_rc.clone())];
        if let Some(e) = extra {
            args.push(e);
        }
        let cur = match prop(obj, b"__spl_sorting") {
            Value::Long(n) => n,
            _ => 0,
        };
        set_prop(obj, b"__spl_sorting", Value::Long(cur + 1));
        // (zend_call_function(): the sort function's frame, called from
        // internal code)
        self.push_builtin_frame(fname, &args, 0);
        let res = self.spl_call_builtin(fname, &args);
        self.pop_builtin_frame();
        set_prop(obj, b"__spl_sorting", Value::Long(cur));
        // Write the sorted table back.
        {
            let sorted = sorted_rc.borrow().clone();
            match &t {
                Table::Arr(a) => {
                    *a.borrow_mut() = sorted;
                }
                Table::Obj(o) => {
                    let hidden: Vec<(Vec<u8>, Value)> = o
                        .borrow()
                        .properties
                        .iter()
                        .filter(|(n, v)| !self.ao_prop_visible(o, n, v))
                        .cloned()
                        .collect();
                    let mut props = hidden;
                    for (k, v) in sorted.iter() {
                        props.push((key_name(k), v.clone()));
                    }
                    // The property table now has its own order (see
                    // Vm::object_prop_table).
                    if !props.iter().any(|(k, _)| k == crate::proptable::OWN_ORDER) {
                        props.push((crate::proptable::OWN_ORDER.to_vec(), Value::True));
                    }
                    o.borrow_mut().properties = props.into();
                }
            }
        }
        // Every hash iterator of the table keeps its bucket index.
        {
            let owner = self.ao_table_owner(obj);
            let epoch = match prop(&owner, b"__spl_sort_epoch") {
                Value::Long(n) => n,
                _ => 0,
            };
            set_prop(&owner, b"__spl_sort_epoch", Value::Long(epoch + 1));
        }
        // The sorting iterator keeps its bucket index over the sorted (and
        // compacted) table.
        if let Some(p) = phys {
            let t = self.ao_table(obj);
            let (_, k) = self.table_len_key_at(&t, p);
            self.ao_set_pos(obj, p, k.as_ref());
        }
        Some(res.unwrap_or(Value::True))
    }

    /// The bucket index (HashPosition) of the iterator position, None while
    /// no position was taken yet: a packed table is indexed by key, holes
    /// included.
    fn ao_physical_pos(&self, obj: &Rc<RefCell<PhpObject>>, t: &Table) -> Option<usize> {
        let (pos, has_key) = {
            let ob = obj.borrow();
            (
                match ob.get_property(b"__spl_pos") {
                    Value::Long(n) if n >= 0 => n as usize,
                    _ => 0,
                },
                matches!(
                    ob.get_property(b"__spl_pos_key"),
                    Value::Long(_) | Value::String(_)
                ),
            )
        };
        if !has_key && pos == 0 {
            return None;
        }
        let packed = match t {
            Table::Arr(a) => a.borrow().looks_packed(),
            Table::Obj(_) => false,
        };
        match self.ao_current(obj) {
            Some((_, ArrayKey::Int(n), _)) if packed => Some(n as usize),
            Some((i, _, _)) => Some(i),
            None if packed => match t {
                Table::Arr(a) => Some(a.borrow().next_free_key().max(0) as usize),
                Table::Obj(_) => Some(pos),
            },
            None => Some(pos),
        }
    }

    fn disabled_functions_contains(&self, f: &[u8]) -> bool {
        self.disabled_functions.contains(f)
    }

    /// Dispatch an ArrayObject/ArrayIterator method declared by `class`.
    pub(crate) fn spl_array_call(
        &mut self,
        class: &[u8],
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        // A lazy object being wrapped is initialized when its property
        // table is first used (zend_std_get_properties_ex()).
        if !matches!(
            method,
            b"__construct" | b"getflags" | b"setflags" | b"getiteratorclass" | b"setiteratorclass"
        ) {
            if let Table::Obj(o) = self.ao_table(obj) {
                if o.borrow().has_property(b"__lazy_state") {
                    self.maybe_run_lazy_init(&o);
                    if self.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                }
            }
        }
        // Serializable::unserialize(): one var_hash for all the pieces.
        if method == b"unserialize" {
            if let Some(h) = self.unserialize_scope_hook {
                h(self, true);
                let r = self.spl_array_call_impl(class, method, obj, args);
                h(self, false);
                return r;
            }
        }
        self.spl_array_call_impl(class, method, obj, args)
    }

    fn spl_array_call_impl(
        &mut self,
        class: &[u8],
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        // RecursiveArrayIterator inherits ArrayIterator's methods (parent::
        // calls name the immediate parent).
        let class: &[u8] = if class == b"recursivearrayiterator"
            && crate::spl_stubs::spl_stub_method(class, method).is_none()
        {
            b"arrayiterator"
        } else {
            class
        };
        let declared = crate::spl_stubs::spl_stub_method(class, method).is_some();
        if !declared || method == b"__construct" {
            return None;
        }
        let stub = crate::spl_stubs::spl_stub_method(class, method)?;
        if stub.required > 0 && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        let is_ao = class == b"arrayobject";
        let cls_name = if is_ao {
            "ArrayObject"
        } else if class == b"arrayiterator" {
            "ArrayIterator"
        } else {
            "RecursiveArrayIterator"
        };
        let r = match method {
            b"offsetexists" => {
                let v = self.ao_offset_exists(obj, a.first().unwrap_or(&Value::Null), 2);
                if v { Value::True } else { Value::False }
            }
            b"offsetget" => self.ao_offset_get(obj, a.first().unwrap_or(&Value::Null)),
            b"offsetset" => {
                let k = a.first().cloned().unwrap_or(Value::Null);
                let v = a.get(1).map_or(Value::Null, Value::deref_for_storage);
                self.ao_offset_set(obj, &k, v);
                Value::Null
            }
            b"offsetunset" => {
                self.ao_offset_unset(obj, a.first().unwrap_or(&Value::Null));
                Value::Null
            }
            b"append" => {
                if self.ao_is_object(obj) {
                    let msg = format!(
                        "Cannot append properties to objects, use {}::offsetSet() instead",
                        Self::ao_class_disp(obj)
                    );
                    return self.spl_throw(b"Error", &msg);
                }
                let v = a.first().map_or(Value::Null, Value::deref_for_storage);
                // spl_array_write_dimension(): an overridden offsetSet()
                // is called.
                if self.ao_user_has(obj, b"offsetset") {
                    let this = Value::Object(obj.clone());
                    self.spl_call_method(&this, b"offsetSet", &[Value::Null, v]);
                } else {
                    self.ao_offset_set(obj, &Value::Null, v);
                }
                Value::Null
            }
            b"getarraycopy" => {
                let t = self.ao_table(obj);
                val_arr(self.table_dup(&t))
            }
            b"count" => {
                let t = self.ao_table(obj);
                Value::Long(self.table_count(&t) as i64)
            }
            b"getflags" => Value::Long(ao_flags(obj) & NOT_INT_MASK),
            b"setflags" => {
                let f = match a.first() {
                    Some(v) => {
                        match self.spl_arg_long(&format!("{}::setFlags", cls_name), 1, "flags", v) {
                            Some(n) => n,
                            None => return Some(Value::Null),
                        }
                    }
                    None => 0,
                };
                let nf = (ao_flags(obj) & INT_MASK) | (f & NOT_INT_MASK);
                set_prop(obj, b"__spl_flags", Value::Long(nf));
                Value::Null
            }
            b"asort" | b"ksort" => {
                let fl = match a.first() {
                    Some(v) => match self.spl_arg_long(
                        &format!("{}::{}", cls_name, String::from_utf8_lossy(method)),
                        1,
                        "flags",
                        v,
                    ) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                return self.ao_sort(obj, method, Some(Value::Long(fl)));
            }
            b"uasort" | b"uksort" => {
                let cb = a.first().cloned().unwrap_or(Value::Null);
                // (the sort function's zpp: its argument #2)
                if let Some(e) = self.callable_error(&cb) {
                    if self.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                    let msg = format!(
                        "{}(): Argument #2 ($callback) must be a valid callback, {}",
                        String::from_utf8_lossy(method),
                        e
                    );
                    return self.spl_throw(b"TypeError", &msg);
                }
                return self.ao_sort(obj, method, Some(cb));
            }
            b"natsort" | b"natcasesort" => return self.ao_sort(obj, method, None),
            b"getiterator" => self.ao_new_iterator(obj),
            b"exchangearray" => {
                let array = a.first().cloned().unwrap_or(Value::Null).deref();
                if !matches!(array, Value::Array(_) | Value::Object(_)) {
                    let msg = format!(
                        "ArrayObject::exchangeArray(): Argument #1 ($array) must be of type array, {} given",
                        Self::value_type_name(&array)
                    );
                    return self.spl_throw(b"TypeError", &msg);
                }
                if !self.ao_sort_check(obj) {
                    return Some(Value::Null);
                }
                let t = self.ao_table(obj);
                let old = val_arr(self.table_dup(&t));
                self.ao_set_array(obj, &array, 0, true, "ArrayObject::exchangeArray");
                old
            }
            b"setiteratorclass" => {
                let cv = a.first().cloned().unwrap_or(Value::Null);
                match self.ao_iterator_class_arg(
                    "ArrayObject::setIteratorClass",
                    1,
                    "iteratorClass",
                    &cv,
                ) {
                    Some(c) => set_prop(obj, b"__spl_iterator_class", Value::String(c)),
                    None => return Some(Value::Null),
                }
                Value::Null
            }
            b"getiteratorclass" => match prop(obj, b"__spl_iterator_class") {
                Value::String(s) if !s.as_bytes().is_empty() => Value::String(s),
                _ => Value::String(PhpString::from_bytes(b"ArrayIterator")),
            },
            b"__debuginfo" => val_arr(self.ao_debug_info(obj)),
            b"__serialize" => {
                let mut out = PhpArray::new();
                let flags = ao_flags(obj);
                out.push(Value::Long(flags & CLONE_MASK));
                if flags & IS_SELF != 0 {
                    out.push(Value::Null);
                } else {
                    out.push(match prop(obj, b"__spl_array") {
                        v @ (Value::Array(_) | Value::Object(_)) => v,
                        _ => val_arr(PhpArray::new()),
                    });
                }
                let props = self.spl_std_props(obj);
                let mut sym = PhpArray::new();
                for (k, v) in props.iter() {
                    sym.set(Self::value_to_array_key(Self::key_value(k)), v.clone());
                }
                out.push(val_arr(sym));
                if is_ao
                    || self
                        .spl_find_base(&obj_class_lower(obj), &[b"arrayobject"])
                        .is_some()
                {
                    match prop(obj, b"__spl_iterator_class") {
                        Value::String(s)
                            if !s.as_bytes().is_empty()
                                && !s.as_bytes().eq_ignore_ascii_case(b"arrayiterator") =>
                        {
                            out.push(Value::String(s))
                        }
                        _ => out.push(Value::Null),
                    }
                } else {
                    out.push(Value::Null);
                }
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
                let iter_class = data.get(&ArrayKey::Int(3)).cloned();
                let (flags, storage, members) = match (flags, storage, members) {
                    (Some(Value::Long(f)), Some(s), Some(Value::Array(m)))
                        if matches!(
                            iter_class,
                            None | Some(Value::Null) | Some(Value::String(_))
                        ) =>
                    {
                        (f, s, m.borrow().clone())
                    }
                    _ => {
                        return self.spl_throw(
                            b"UnexpectedValueException",
                            "Incomplete or ill-typed serialization data",
                        );
                    }
                };
                let nf = (ao_flags(obj) & !CLONE_MASK) | (flags & CLONE_MASK);
                set_prop(obj, b"__spl_flags", Value::Long(nf));
                if flags & IS_SELF != 0 {
                    set_prop(obj, b"__spl_array", Value::Null);
                } else {
                    if !matches!(storage.deref(), Value::Array(_) | Value::Object(_)) {
                        return self.spl_throw(
                            b"InvalidArgumentException",
                            "Passed variable is not an array or object",
                        );
                    }
                    let base = self.ao_base_name(obj);
                    let func = format!("{}::__unserialize", base);
                    if !self.ao_set_array(obj, &storage, 0, true, &func) {
                        return Some(Value::Null);
                    }
                    // keep the deserialized clone-mask flags
                    let cur = ao_flags(obj);
                    set_prop(
                        obj,
                        b"__spl_flags",
                        Value::Long(
                            (cur & !CLONE_MASK & !IS_SELF)
                                | (flags & CLONE_MASK & !IS_SELF)
                                | (cur & USE_OTHER),
                        ),
                    );
                }
                self.spl_load_props(obj, &members);
                if let Some(Value::String(c)) = iter_class {
                    let cl: Vec<u8> = c
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    if !self.classes.contains_key(&cl) && !self.is_known_builtin_class(&cl) {
                        self.try_autoload_class(c.as_bytes());
                    }
                    if !self.classes.contains_key(&cl) && !self.is_known_builtin_class(&cl) {
                        let msg = format!(
                            "Cannot deserialize ArrayObject with iterator class '{}'; no such class exists",
                            c.to_msg_string()
                        );
                        return self.spl_throw(b"UnexpectedValueException", &msg);
                    }
                    let is_iter = self.class_implements_interface(&cl, b"iterator")
                        || crate::vm::is_builtin_subclass(&cl, b"iterator");
                    if !is_iter {
                        let msg = format!(
                            "Cannot deserialize ArrayObject with iterator class '{}'; this class does not implement the Iterator interface",
                            c.to_msg_string()
                        );
                        return self.spl_throw(b"UnexpectedValueException", &msg);
                    }
                    let canonical = self
                        .classes
                        .get(&cl)
                        .map(|ce| PhpString::from_vec(ce.name.clone()))
                        .unwrap_or_else(|| {
                            PhpString::from_string(self.builtin_canonical_name(&cl))
                        });
                    set_prop(obj, b"__spl_iterator_class", Value::String(canonical));
                }
                Value::Null
            }
            b"serialize" => {
                let flags = ao_flags(obj);
                let mut out: Vec<u8> = b"x:".to_vec();
                // (one var_hash for all the pieces, the flags included)
                let scope = self.serialize_scope_hook;
                if let Some(h) = scope {
                    h(true);
                }
                let fs = self
                    .spl_call_builtin(b"serialize", &[Value::Long(flags & CLONE_MASK)])
                    .unwrap_or(Value::Null);
                out.extend_from_slice(fs.to_php_string().as_bytes());
                if flags & IS_SELF == 0 {
                    let st = match prop(obj, b"__spl_array") {
                        v @ (Value::Array(_) | Value::Object(_)) => v,
                        _ => val_arr(PhpArray::new()),
                    };
                    let s = self
                        .spl_call_builtin(b"serialize", &[st])
                        .unwrap_or(Value::Null);
                    out.extend_from_slice(s.to_php_string().as_bytes());
                    out.push(b';');
                }
                out.extend_from_slice(b"m:");
                let props = val_arr(self.spl_std_props(obj));
                let s = self
                    .spl_call_builtin(b"serialize", &[props])
                    .unwrap_or(Value::Null);
                if let Some(h) = scope {
                    h(false);
                }
                out.extend_from_slice(s.to_php_string().as_bytes());
                Value::String(PhpString::from_vec(out))
            }
            b"unserialize" => {
                let data = match a.first() {
                    Some(v) => match self.spl_arg_string(
                        &format!("{}::unserialize", cls_name),
                        1,
                        "data",
                        v,
                    ) {
                        Some(s) => s.as_bytes().to_vec(),
                        None => return Some(Value::Null),
                    },
                    None => Vec::new(),
                };
                if data.is_empty() {
                    return Some(Value::Null);
                }
                if !self.ao_sort_check(obj) {
                    return Some(Value::Null);
                }
                let n = data.len();
                let fail = |vm: &mut Vm, p: usize| -> Option<Value> {
                    let msg = format!("Error at offset {} of {} bytes", p, n);
                    vm.spl_throw(b"UnexpectedValueException", &msg)
                };
                if !(data.len() >= 2 && data[0] == b'x' && data[1] == b':') {
                    return fail(self, if data.first() == Some(&b'x') { 1 } else { 0 });
                }
                let mut p = 2usize;
                let flags = match self.spl_unserialize_at(&data, &mut p) {
                    Some(Value::Long(f)) => f,
                    _ => return fail(self, p),
                };
                p -= 1;
                if data.get(p) != Some(&b';') {
                    return fail(self, p);
                }
                p += 1;
                if flags & IS_SELF != 0 {
                    let nf = (ao_flags(obj) & !CLONE_MASK) | (flags & CLONE_MASK);
                    set_prop(obj, b"__spl_flags", Value::Long(nf));
                    set_prop(obj, b"__spl_array", Value::Null);
                } else {
                    if !matches!(
                        data.get(p),
                        Some(b'a') | Some(b'O') | Some(b'C') | Some(b'r')
                    ) {
                        return fail(self, p);
                    }
                    let st = match self.spl_unserialize_at(&data, &mut p) {
                        Some(v @ (Value::Array(_) | Value::Object(_))) => v,
                        _ => return fail(self, p),
                    };
                    let nf = (ao_flags(obj) & !CLONE_MASK) | (flags & CLONE_MASK);
                    set_prop(obj, b"__spl_flags", Value::Long(nf));
                    match &st {
                        Value::Array(_) => set_prop(obj, b"__spl_array", st.clone()),
                        _ => {
                            let func = format!("{}::unserialize", self.ao_base_name(obj));
                            // (the members are still loaded after a failure)
                            let _ = self.ao_set_array(obj, &st, 0, true, &func);
                        }
                    }
                    if data.get(p) != Some(&b';') {
                        return fail(self, p);
                    }
                    p += 1;
                }
                if !(data.get(p) == Some(&b'm') && data.get(p + 1) == Some(&b':')) {
                    return fail(self, p);
                }
                p += 2;
                match self.spl_unserialize_at(&data, &mut p) {
                    Some(Value::Array(m)) => {
                        let m = m.borrow().clone();
                        self.spl_load_props(obj, &m);
                    }
                    _ => return fail(self, p),
                }
                Value::Null
            }
            // ArrayIterator
            b"rewind" => {
                self.ao_rewind(obj);
                Value::Null
            }
            b"valid" => {
                if self.ao_valid(obj) {
                    Value::True
                } else {
                    Value::False
                }
            }
            b"current" => match self.ao_current(obj) {
                Some((_, _, v)) => v.deref(),
                None => Value::Null,
            },
            b"key" => match self.ao_current(obj) {
                Some((_, k, _)) => Self::key_value(&k),
                None => Value::Null,
            },
            b"next" => {
                self.ao_next(obj);
                Value::Null
            }
            b"seek" => {
                let pos = match a.first() {
                    Some(v) => match self.spl_arg_long("ArrayIterator::seek", 1, "offset", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                let t = self.ao_table(obj);
                let (len, key) = self.table_len_key_at(&t, pos.max(0) as usize);
                if pos >= 0 && (pos as usize) < len {
                    self.ao_set_pos(obj, pos as usize, key.as_ref());
                    Value::Null
                } else {
                    // (a non-negative position was walked to the end)
                    if pos >= 0 {
                        self.ao_set_pos(obj, len, None);
                    }
                    let msg = format!("Seek position {} is out of range", pos);
                    return self.spl_throw(b"OutOfBoundsException", &msg);
                }
            }
            // RecursiveArrayIterator
            b"haschildren" => match self.ao_current(obj) {
                Some((_, _, v)) => {
                    let v = v.deref();
                    let f = ao_flags(obj);
                    let r = matches!(v, Value::Array(_))
                        || (matches!(v, Value::Object(_)) && f & CHILD_ARRAYS_ONLY == 0);
                    if r { Value::True } else { Value::False }
                }
                None => Value::False,
            },
            b"getchildren" => {
                let (entry_key, entry) = match self.ao_current(obj) {
                    Some((_, k, v)) => (k, v.deref()),
                    None => return Some(Value::Null),
                };
                let entry_is_array = matches!(entry, Value::Array(_));
                let f = ao_flags(obj);
                if let Value::Object(eo) = &entry {
                    if f & CHILD_ARRAYS_ONLY != 0 {
                        return Some(Value::Null);
                    }
                    let my_cl = obj_class_lower(obj);
                    let ecl = obj_class_lower(eo);
                    if ecl == my_cl || self.class_extends(&ecl, &my_cl) {
                        return Some(entry.clone());
                    }
                }
                let class = obj.borrow().class_name.clone();
                let cl = obj_class_lower(obj);
                let child = self.spl_instantiate(&class);
                let cargs = vec![entry, Value::Long(f & NOT_INT_MASK)];
                if self.spl_has_user_ctor(&cl) {
                    self.call_object_method(&Value::Object(child.clone()), b"__construct", &cargs);
                } else {
                    self.spl_push_internal_ctor_frame(&cl, &cargs);
                    self.ao_construct(&child, &cargs);
                    if let Some(f) = self.call_stack.pop() {
                        self.recycle_frame(f);
                    }
                }
                if self.current_exception.is_some() {
                    return Some(Value::Null);
                }
                // spl_instantiate_child_arg(): the child's array replaces the
                // parent's element, so changes made through either one show
                // in the other.
                if entry_is_array {
                    if let Value::Array(a) = prop(&child, b"__spl_array") {
                        let t = self.ao_table(obj);
                        self.table_set(&t, entry_key, Value::Array(a));
                    }
                }
                Value::Object(child)
            }
            _ => return None,
        };
        Some(r)
    }
}

/// Native constructor handler for ArrayObject / ArrayIterator.
pub fn spl_array_construct_native(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    vm.spl_push_ctor_frame(&obj, &args[1..]);
    let ok = vm.ao_construct(&obj, &args[1..]);
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
