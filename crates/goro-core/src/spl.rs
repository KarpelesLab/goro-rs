//! SPL helpers split out of vm.rs (string conversion, debug info, ...).
//!
//! The SPL classes keep their internal state in hidden `__spl_*` properties
//! on the PhpObject; this module only adds behaviour on top of that model.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

impl Vm {
    /// Walk the class chain (user parents, then built-in parents) of
    /// `class_lower` and return the first built-in SPL class from `bases`
    /// that it is or extends.
    pub(crate) fn spl_find_base(&self, class_lower: &[u8], bases: &[&'static [u8]]) -> Option<&'static [u8]> {
        let mut cur: Vec<u8> = class_lower.to_vec();
        for _ in 0..64 {
            if let Some(b) = bases.iter().find(|b| **b == cur.as_slice()) {
                return Some(b);
            }
            let next: Option<Vec<u8>> = self
                .classes
                .get(&cur)
                .and_then(|c| c.parent.clone())
                .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect())
                .or_else(|| {
                    crate::vm::get_builtin_parent(&cur)
                        .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect())
                });
            match next {
                Some(n) => cur = n,
                None => return None,
            }
        }
        None
    }

    /// Whether objects of this class get a built-in SPL `__toString`
    /// (SplFileInfo family and CachingIterator family).
    pub(crate) fn spl_has_builtin_tostring(&self, class_lower: &[u8]) -> bool {
        self.spl_find_base(class_lower, &[b"splfileinfo", b"cachingiterator"]).is_some()
    }

    /// Set current_exception to a new exception of `class` and return
    /// Some(Null) (the conventional "threw" result of SPL dispatchers).
    pub(crate) fn spl_throw(&mut self, class: &[u8], msg: &str) -> Option<Value> {
        let line = self.current_line;
        let exc = self.create_exception(class, msg, line);
        self.current_exception = Some(exc);
        Some(Value::Null)
    }

    /// PHP `<=>` that also handles objects with __toString compared against
    /// strings (calls the user's __toString).
    pub(crate) fn spl_compare_values(&mut self, a: &Value, b: &Value) -> i64 {
        let a = a.deref();
        let b = b.deref();
        match (&a, &b) {
            (Value::Object(_), Value::String(_)) | (Value::String(_), Value::Object(_)) => {
                let obj_left = matches!(a, Value::Object(_));
                let o = if obj_left { &a } else { &b };
                let cl = if let Value::Object(ob) = o { obj_class_lower(ob) } else { Vec::new() };
                let has_ts = self.classes.get(&cl).map(|c| c.get_method(b"__tostring").is_some()).unwrap_or(false);
                if has_ts {
                    let s = Value::String(self.value_to_string(o));
                    if self.current_exception.is_some() {
                        return 0;
                    }
                    return if obj_left { s.compare(&b) } else { a.compare(&s) };
                }
                a.compare(&b)
            }
            _ => a.compare(&b),
        }
    }

    /// Properties of the object as zend_std_get_properties would expose
    /// them (mangled names for private/protected, hidden SPL state omitted).
    pub(crate) fn spl_std_props(&self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        self.object_props_mangled(obj).borrow().clone()
    }

    /// Stub signature of an SPL method, looked up on `class_lower` and its
    /// built-in parents.
    pub(crate) fn spl_stub_lookup(&self, class_lower: &[u8], method_lower: &[u8]) -> Option<&'static crate::spl_stubs::StubMethod> {
        let mut cur = class_lower.to_vec();
        for _ in 0..16 {
            if let Some(m) = crate::spl_stubs::spl_stub_method(&cur, method_lower) {
                return Some(m);
            }
            match crate::vm::get_builtin_parent(&cur) {
                Some(p) => cur = p.iter().map(|b| b.to_ascii_lowercase()).collect(),
                None => return None,
            }
        }
        None
    }

    /// Place named arguments of an SPL method call at their positions
    /// (`args[0]` is $this). Skipped optional parameters become Undef.
    /// Returns false with an Error set on unknown/duplicate names.
    pub(crate) fn spl_map_named_args(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
        args: &mut Vec<Value>,
        named: &[(Vec<u8>, Value)],
    ) -> bool {
        let stub = match self.spl_stub_lookup(class_lower, method_lower) {
            Some(s) => s,
            None => {
                // Unknown signature: append in order.
                for (_, v) in named {
                    args.push(v.clone());
                }
                return true;
            }
        };
        let positional = args.len().saturating_sub(1);
        for (name, v) in named {
            let idx = stub.params.iter().position(|p| p.as_bytes() == name.as_slice());
            match idx {
                None => {
                    let msg = format!("Unknown named parameter ${}", String::from_utf8_lossy(name));
                    self.spl_throw(b"Error", &msg);
                    return false;
                }
                Some(i) if i < positional => {
                    let msg = format!("Named parameter ${} overwrites previous argument", String::from_utf8_lossy(name));
                    self.spl_throw(b"Error", &msg);
                    return false;
                }
                Some(i) => {
                    while args.len() < i + 2 {
                        args.push(Value::Undef);
                    }
                    args[i + 1] = v.clone();
                }
            }
        }
        true
    }

    /// Allocate a new object of `class` (user or built-in) with property
    /// defaults applied; the constructor is not called.
    pub(crate) fn spl_instantiate(&mut self, class: &[u8]) -> Rc<RefCell<PhpObject>> {
        let cl: Vec<u8> = class.iter().map(|b| b.to_ascii_lowercase()).collect();
        let canonical = self
            .classes
            .get(&cl)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| self.builtin_canonical_name(&cl).into_bytes());
        let id = self.alloc_object_id();
        let mut o = PhpObject::new(canonical, id);
        self.apply_class_property_defaults(&mut o, &cl);
        Rc::new(RefCell::new(o))
    }

    /// Whether `class_lower` declares a userland constructor.
    pub(crate) fn spl_has_user_ctor(&self, class_lower: &[u8]) -> bool {
        self.classes.get(class_lower).map(|c| c.get_method(b"__construct").is_some()).unwrap_or(false)
    }

    /// Native get_debug_info for SPL classes: the property table (mangled
    /// keys) var_dump/print_r should show, or None for non-SPL objects.
    pub fn spl_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Option<PhpArray> {
        let cl = obj_class_lower(obj);
        let base = self.spl_find_base(
            &cl,
            &[
                b"splpriorityqueue", b"splminheap", b"splmaxheap", b"splheap",
                b"directoryiterator", b"splfileobject", b"splfileinfo",
            ],
        )?;
        match base {
            b"splfileobject" => Some(self.sfi_debug_info(obj, true)),
            b"splfileinfo" => Some(self.sfi_debug_info(obj, false)),
            b"splpriorityqueue" => Some(self.heap_debug_info(obj, true)),
            b"splminheap" | b"splmaxheap" | b"splheap" => Some(self.heap_debug_info(obj, false)),
            _ => None,
        }
    }

    /// foreach-by-reference over an object. Traversables other than the
    /// ArrayObject/ArrayIterator family cannot be iterated by reference
    /// (Err: exception set). ArrayObject/ArrayIterator iterate their
    /// backing array in place (Ok(Some)). Plain objects: Ok(None).
    pub(crate) fn spl_foreach_ref_target(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Result<Option<Rc<RefCell<PhpArray>>>, ()> {
        let cl = obj_class_lower(obj);
        if cl == b"weakmap" {
            return Ok(None);
        }
        if self.spl_find_base(&cl, &[b"arrayobject", b"arrayiterator"]).is_some() {
            return Ok(match obj.borrow().get_property(b"__spl_array") {
                Value::Array(a) => Some(a),
                _ => None,
            });
        }
        let is_iter = self.class_implements_interface(&cl, b"iterator")
            || crate::vm::is_builtin_subclass(&cl, b"iterator");
        if is_iter {
            self.spl_throw(b"Error", "An iterator cannot be used with foreach by reference");
            return Err(());
        }
        let is_agg = self.class_implements_interface(&cl, b"iteratoraggregate")
            || crate::vm::is_builtin_subclass(&cl, b"iteratoraggregate");
        if is_agg {
            let it = self.call_object_method(&Value::Object(obj.clone()), b"getIterator", &[]);
            if self.current_exception.is_some() {
                return Err(());
            }
            if let Some(Value::Object(io)) = it {
                return self.spl_foreach_ref_target(&io);
            }
        }
        Ok(None)
    }

    /// Load a serialized property table (possibly with mangled keys) into obj.
    pub(crate) fn spl_load_props(&mut self, obj: &Rc<RefCell<PhpObject>>, props: &PhpArray) {
        for (k, v) in props.iter() {
            let name: Vec<u8> = match k {
                ArrayKey::Int(n) => n.to_string().into_bytes(),
                ArrayKey::String(s) => {
                    let b = s.as_bytes();
                    if b.first() == Some(&0) {
                        if let Some(end) = b[1..].iter().position(|&c| c == 0) {
                            b[end + 2..].to_vec()
                        } else {
                            b.to_vec()
                        }
                    } else {
                        b.to_vec()
                    }
                }
            };
            obj.borrow_mut().set_property(name, v.clone());
        }
    }
}

pub(crate) fn mangle_private(class: &[u8], name: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(class.len() + name.len() + 2);
    m.push(0);
    m.extend_from_slice(class);
    m.push(0);
    m.extend_from_slice(name);
    m
}

pub(crate) fn skey(s: &[u8]) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s))
}

pub(crate) fn obj_class_lower(obj: &Rc<RefCell<PhpObject>>) -> Vec<u8> {
    obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect()
}

pub(crate) fn val_arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}
