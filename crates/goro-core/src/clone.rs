//! `clone` (ZEND_CLONE and the PHP 8.5 `clone()` function, including
//! "clone with": `clone($obj, ['prop' => $value])`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::typed_ref;
use crate::vm::{Vm, native_handlers};

impl Vm {
    /// zend_lazy_object_clone() of an initialized proxy: a new proxy (whose
    /// own destructor never runs) over a clone of the instance.
    #[inline(never)]
    fn clone_lazy_proxy(&mut self, obj: &Rc<RefCell<PhpObject>>, line: u32) -> Option<Value> {
        let real = {
            let ob = obj.borrow();
            if !matches!(ob.get_property(b"__lazy_state"), Value::String(ref s) if s.as_bytes() == b"proxy")
            {
                return None;
            }
            match ob.get_property(b"__lazy_real") {
                Value::Object(r) => r,
                _ => return None,
            }
        };
        let id = self.alloc_object_id();
        let mut proxy = {
            let ob = obj.borrow();
            let mut p = PhpObject::new(ob.class_name.clone(), id);
            for (n, v) in ob.properties.iter() {
                if n.as_slice() != b"__lazy_real" {
                    p.set_property(n.clone(), v.clone());
                }
            }
            p
        };
        let inner = self.clone_value(&Value::Object(real), line);
        proxy.set_property(b"__lazy_real".to_vec(), inner);
        Some(Value::Object(Rc::new(RefCell::new(proxy))))
    }

    /// Clone `val` like php-src's clone_obj handler (copy the properties,
    /// then run `__clone()`). On failure `current_exception` is set and
    /// `Value::Null` is returned (the clone, when `__clone()` threw).
    /// ZEND_CLONE: the error for a __clone() the calling scope cannot call
    /// (checked before the object is copied).
    #[inline(never)]
    fn clone_access_error(&self, class_lower: &[u8], class_name_display: &str) -> Option<String> {
        let class_def = self.classes.get(class_lower)?;
        let clone_method = class_def.get_method(b"__clone")?;
        let class_lower = class_lower.to_vec();
        // Check __clone visibility
        let current_scope = self.current_class_scope();
        // Visibility is checked against the declaring class
        // (private) / the root of the method's hierarchy (protected).
        let declaring_lower: Vec<u8> = if clone_method.declaring_class.is_empty() {
            class_lower.clone()
        } else {
            clone_method
                .declaring_class
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect()
        };
        let mut root_lower = declaring_lower.clone();
        while let Some(parent) = self.classes.get(&root_lower).and_then(|c| c.parent.clone()) {
            let pl: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            if self
                .classes
                .get(&pl)
                .map(|c| c.get_method(b"__clone").is_some())
                .unwrap_or(false)
            {
                root_lower = pl;
            } else {
                break;
            }
        }
        let accessible = match clone_method.visibility {
            crate::object::Visibility::Public => true,
            crate::object::Visibility::Protected => {
                if let Some(scope) = &current_scope {
                    let scope_lower: Vec<u8> =
                        scope.iter().map(|b| b.to_ascii_lowercase()).collect();
                    scope_lower == class_lower
                        || self.class_extends(&scope_lower, &class_lower)
                        || self.class_extends(&class_lower, &scope_lower)
                        || scope_lower == root_lower
                        || self.class_extends(&scope_lower, &root_lower)
                        || self.class_extends(&root_lower, &scope_lower)
                } else {
                    false
                }
            }
            crate::object::Visibility::Private => {
                if let Some(scope) = &current_scope {
                    let scope_lower: Vec<u8> =
                        scope.iter().map(|b| b.to_ascii_lowercase()).collect();
                    scope_lower == declaring_lower
                } else {
                    false
                }
            }
        };
        if !accessible {
            let scope_str = current_scope
                .map(|s| String::from_utf8_lossy(&s).to_string())
                .unwrap_or_else(|| "global scope".to_string());
            let vis_str = match clone_method.visibility {
                crate::object::Visibility::Private => "private",
                crate::object::Visibility::Protected => "protected",
                _ => "public",
            };
            let msg = format!(
                "Call to {} method {}::__clone() from {}",
                vis_str, class_name_display, scope_str
            );
            return Some(msg);
        }

        None
    }

    pub fn clone_value(&mut self, val: &Value, line: u32) -> Value {
        if val.is_closure_obj() {
            return self.clone_closure(val);
        }
        // Lazy init on clone: ghosts are fully initialized, then cloned.
        // Proxies initialize their backing object; the clone is a fresh
        // proxy with a cloned instance.
        if let Value::Object(obj) = val {
            self.maybe_run_lazy_init_op(obj);
            // If the lazy initializer threw, propagate and skip cloning.
            if self.current_exception.is_some() {
                return Value::Null;
            }
            if let Some(v) = self.clone_lazy_proxy(obj, line) {
                return v;
            }
        }
        match val {
            Value::Object(obj) => {
                // ArrayObject clones its table; ArrayIterator clones share
                // the original (USE_OTHER).
                let ao_clone: Option<(Value, i64)> = {
                    let cl: Vec<u8> = obj
                        .borrow()
                        .class_name
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    self.spl_find_base(&cl, &[b"arrayobject", b"arrayiterator"])
                        .map(|b| self.ao_clone_storage(obj, b == b"arrayobject"))
                };
                let obj_borrow = obj.borrow();
                // Check if this is an uncloneable object
                let class_lower: Vec<u8> = obj_borrow
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                // Reflection classes and a few others are
                // explicitly marked uncloneable in PHP.
                let reflection_uncloneable = matches!(
                    class_lower.as_slice(),
                    b"reflectionclass"
                        | b"reflectionobject"
                        | b"reflectionenum"
                        | b"reflectionmethod"
                        | b"reflectionproperty"
                        | b"reflectionfunction"
                        | b"reflectionparameter"
                        | b"reflectionclassconstant"
                        | b"reflectionenumunitcase"
                        | b"reflectionenumbackedcase"
                        | b"reflectionnamedtype"
                        | b"reflectionuniontype"
                        | b"reflectionintersectiontype"
                        | b"reflectiontype"
                        | b"reflectionattribute"
                        | b"reflectionextension"
                        | b"reflectionconstant"
                        | b"reflectiongenerator"
                        | b"reflectionfiber"
                        | b"reflectionreference"
                );
                let spl_uncloneable = self
                    .spl_find_base(
                        &class_lower,
                        &[
                            b"splfileobject",
                            b"globiterator",
                            // spl_handlers_dual_it / spl_handlers_rec_it_it: clone_obj = NULL
                            b"iteratoriterator",
                            b"recursiveiteratoriterator",
                        ],
                    )
                    .is_some();
                if reflection_uncloneable
                    || spl_uncloneable
                    || class_lower == b"generator"
                    || class_lower == b"closure"
                    || class_lower == b"xmlparser"
                    || class_lower == b"xmlwriter"
                    || class_lower == b"xsltprocessor"
                    || class_lower == b"deflatecontext"
                    || class_lower == b"inflatecontext"
                    || class_lower == b"socket"
                    || class_lower == b"addressinfo"
                    || class_lower == b"ftp\\connection"
                    || class_lower == b"pgsql\\connection"
                    || class_lower == b"pgsql\\result"
                    || class_lower == b"pgsql\\lob"
                    || class_lower == b"shmop"
                    || class_lower == b"sysvsemaphore"
                    || class_lower == b"sysvsharedmemory"
                    || class_lower == b"sysvmessagequeue"
                    || class_lower == b"fiber"
                    || class_lower == b"weakreference"
                    // (default_exception_handlers: Exception, Error and
                    // every subclass)
                    || self.class_implements_interface(&class_lower, b"throwable")
                    || class_lower == b"internaliterator"
                    || class_lower == b"random\\engine\\secure"
                    || class_lower == b"random\\randomizer"
                    || obj_borrow.has_property(b"__enum_case")
                    || (!self.uncloneable_classes.is_empty()
                        && self
                            .uncloneable_classes
                            .iter()
                            .any(|u| *u == class_lower || self.class_extends(&class_lower, u)))
                {
                    let class_name = crate::value::display_class_name(&obj_borrow.class_name);
                    drop(obj_borrow);
                    let msg = format!(
                        "Trying to clone an uncloneable object of class {}",
                        class_name
                    );
                    let exc_val = self.create_exception(b"Error", &msg, line);
                    self.current_exception = Some(exc_val);
                    return Value::Null;
                }
                {
                    let display = crate::value::display_class_name(&obj_borrow.class_name);
                    if let Some(msg) = self.clone_access_error(&class_lower, &display) {
                        drop(obj_borrow);
                        let exc = self.create_exception(b"Error", &msg, line);
                        self.current_exception = Some(exc);
                        return Value::Null;
                    }
                }
                let clone_id = self.alloc_object_id();
                let mut new_obj = PhpObject::new(obj_borrow.class_name.clone(), clone_id);
                // Determine if we're cloning an ArrayObject-like class
                // that wraps an object: in that case the wrapped object
                // is replaced by a snapshot array of its visible
                // properties so the clone is detached from future
                // mutations of the wrapped object.
                // ArrayObject deep-clones its storage; ArrayIterator
                // shares the backing storage (PHP behaviour). We still
                // snapshot a wrapped object into a fresh array in
                // both cases because the wrapped object lifecycle is
                // separate.
                let is_array_object_like = false;
                let deep_clone_storage = false;
                // Copy all properties, deep-cloning arrays for SPL classes
                // and deep-cloning __lazy_real for lazy proxies.
                for (name, value) in &obj_borrow.properties {
                    let cloned_value = if name == b"__spl_array"
                        && is_array_object_like
                        && !deep_clone_storage
                        && matches!(value, Value::Array(_))
                    {
                        // Share storage by reference (ArrayIterator semantics).
                        value.clone()
                    } else if name == b"__spl_array" && is_array_object_like {
                        match value {
                            Value::Array(a) => {
                                Value::Array(Rc::new(RefCell::new(a.borrow().clone())))
                            }
                            Value::Object(inner) => {
                                // Snapshot the wrapped object's visible
                                // properties into a fresh array.
                                let ob_ref = inner.borrow();
                                let mut arr = PhpArray::new();
                                let inner_class_lower: Vec<u8> = ob_ref
                                    .class_name
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                // If wrapped is itself an ArrayObject, iterate
                                // its own __spl_array storage.
                                if matches!(
                                    inner_class_lower.as_slice(),
                                    b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
                                ) {
                                    let inner_storage = ob_ref.get_property(b"__spl_array");
                                    drop(ob_ref);
                                    match inner_storage {
                                        Value::Array(a2) => {
                                            Value::Array(Rc::new(RefCell::new(a2.borrow().clone())))
                                        }
                                        Value::Object(inner2) => {
                                            let inner2_b = inner2.borrow();
                                            for (n, v) in &inner2_b.properties {
                                                if n.starts_with(b"__") {
                                                    continue;
                                                }
                                                // strip mangling for public keys
                                                let key_bytes: Vec<u8> = if n.first() == Some(&0) {
                                                    if let Some(pos) =
                                                        n[1..].iter().position(|&c| c == 0)
                                                    {
                                                        n[pos + 2..].to_vec()
                                                    } else {
                                                        n.clone()
                                                    }
                                                } else {
                                                    n.clone()
                                                };
                                                arr.set(
                                                    ArrayKey::String(PhpString::from_bytes(
                                                        &key_bytes,
                                                    )),
                                                    v.clone(),
                                                );
                                            }
                                            Value::Array(Rc::new(RefCell::new(arr)))
                                        }
                                        _ => Value::Array(Rc::new(RefCell::new(arr))),
                                    }
                                } else {
                                    for (n, v) in &ob_ref.properties {
                                        if n.starts_with(b"__") {
                                            continue;
                                        }
                                        // Skip mangled private/protected names
                                        // (their raw form starts with NUL).
                                        if n.first() == Some(&0) {
                                            continue;
                                        }
                                        arr.set(
                                            ArrayKey::String(PhpString::from_bytes(n)),
                                            v.clone(),
                                        );
                                    }
                                    Value::Array(Rc::new(RefCell::new(arr)))
                                }
                            }
                            other => other.clone(),
                        }
                    } else if name.starts_with(b"__spl_") {
                        match value {
                            Value::Array(a) => {
                                Value::Array(Rc::new(RefCell::new(a.borrow().clone())))
                            }
                            other => other.clone(),
                        }
                    } else {
                        match value {
                            // Arrays are values: the clone gets its own copy.
                            Value::Array(a) => {
                                Value::Array(Rc::new(RefCell::new(a.borrow().clone())))
                            }
                            // A reference held only by this object is
                            // separated on clone (PHP semantics).
                            Value::Reference(r) if Rc::strong_count(r) == 1 => r.borrow().clone(),
                            _ => value.clone(),
                        }
                    };
                    new_obj.set_property(name.clone(), cloned_value);
                }
                if let Some((st, fl)) = ao_clone {
                    new_obj.set_property(b"__spl_array".to_vec(), st);
                    new_obj.set_property(b"__spl_flags".to_vec(), Value::Long(fl));
                    // (a fresh hash iterator: the clone starts at the beginning)
                    new_obj.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                    new_obj.set_property(b"__spl_pos_key".to_vec(), Value::Null);
                }
                // spl_dllist_object_new_ex(): the clone's traverse pointer
                // is the head of its list, at position 0.
                if new_obj.has_property(b"__spl_dll_cur")
                    && self
                        .spl_find_base(&class_lower, &[b"spldoublylinkedlist"])
                        .is_some()
                {
                    let head = match new_obj.get_property(b"__spl_array") {
                        Value::Array(a) => a.borrow().iter().next().map(|(k, _)| k.clone()),
                        _ => None,
                    };
                    let head = match head {
                        Some(crate::array::ArrayKey::Int(n)) => Value::Long(n),
                        _ => Value::Null,
                    };
                    new_obj.set_property(b"__spl_dll_cur".to_vec(), head);
                    new_obj.set_property(b"__spl_dll_tpos".to_vec(), Value::Long(0));
                }
                // spl_object_storage_new_ex(): the clone's position is reset.
                if new_obj.has_property(b"__spl_pos")
                    && self
                        .spl_find_base(&class_lower, &[b"splobjectstorage"])
                        .is_some()
                {
                    new_obj.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                    new_obj.set_property(b"__spl_index".to_vec(), Value::Long(0));
                }
                // For WeakMap, copy the live entries from the source.
                let original_oid = obj_borrow.object_id;
                let is_weakmap = class_lower.as_slice() == b"weakmap";
                let new_obj_val = Value::Object(Rc::new(RefCell::new(new_obj)));
                // The clone is an ordinary object: its destructor runs
                // once it is released (even when __clone throws).
                {
                    let cl: Vec<u8> = obj_borrow
                        .class_name
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    if self
                        .classes
                        .get(&cl)
                        .is_some_and(|c| c.get_method(b"__destruct").is_some())
                    {
                        if let Value::Object(rc) = &new_obj_val {
                            rc.borrow_mut().remove_property(b"__ctor_pending");
                            rc.borrow_mut().remove_property(b"__destructed");
                        }
                        self.list_destructible(new_obj_val.clone());
                    }
                }
                // Shared typed references gain the clone's properties
                // as type sources.
                if typed_ref::any() {
                    if let Value::Object(dst) = &new_obj_val {
                        let refs: Vec<(Vec<u8>, Rc<RefCell<Value>>)> = dst
                            .borrow()
                            .properties
                            .iter()
                            .filter_map(|(k, v)| match v {
                                Value::Reference(r) => Some((k.clone(), r.clone())),
                                _ => None,
                            })
                            .collect();
                        for (k, r) in &refs {
                            if typed_ref::is_typed(r) {
                                self.typed_ref_register_slot(dst, k, r);
                            }
                        }
                    }
                }
                if is_weakmap {
                    let entries_copy: Vec<(u64, std::rc::Weak<RefCell<PhpObject>>, Value)> =
                        if let Some(entries) = self.weak_map_entries.get(&original_oid) {
                            entries
                                .iter()
                                .filter(|(_, w, _)| w.upgrade().is_some())
                                .map(|(k, w, v)| (*k, w.clone(), v.clone()))
                                .collect()
                        } else {
                            Vec::new()
                        };
                    self.weak_map_entries.insert(clone_id, entries_copy);
                }
                // Call __clone() if defined
                let class_lower: Vec<u8> = obj_borrow
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                drop(obj_borrow);
                // The native clone handler copies the extension state
                // first (PHP's clone_obj), so __clone() sees it.
                if let Value::Object(dst) = &new_obj_val {
                    if let Some(f) = native_handlers(obj).and_then(|h| h.clone_obj) {
                        f(self, obj, dst);
                    }
                }
                if self.current_exception.is_none() {
                    if let Some(class_def) = self.classes.get(&class_lower) {
                        // (its visibility was checked before the copy)
                        if class_def.get_method(b"__clone").is_some() {
                            if let Value::Object(ref rc) = new_obj_val {
                                rc.borrow_mut()
                                    .set_property(b"__clone_window".to_vec(), Value::True);
                            }
                            // __clone() runs as a frame of its own.
                            let clone_frame = {
                                let cl = match &new_obj_val {
                                    Value::Object(o) => o.borrow().class_name.to_ascii_lowercase(),
                                    _ => Vec::new(),
                                };
                                let decl = self
                                    .classes
                                    .get(&cl)
                                    .and_then(|c| c.get_method(b"__clone"))
                                    .map(|m| m.declaring_class.clone())
                                    .unwrap_or(cl);
                                let name = self
                                    .classes
                                    .get(&decl)
                                    .map(|c| c.name.clone())
                                    .unwrap_or(decl);
                                format!("{}::__clone", String::from_utf8_lossy(&name))
                            };
                            self.call_stack.push((
                                clone_frame,
                                self.current_file.clone(),
                                line,
                                Vec::new(),
                                true,
                            ));
                            let hold = match &new_obj_val {
                                Value::Object(rc) => self
                                    .clone_hold_hook_of(&class_lower)
                                    .map(|h| (h, rc.clone())),
                                _ => None,
                            };
                            if let Some((h, rc)) = &hold {
                                h(rc, true);
                            }
                            self.call_object_method(&new_obj_val, b"__clone", &[]);
                            if let Some((h, rc)) = &hold {
                                h(rc, false);
                            }
                            self.call_stack.pop();
                            if let Value::Object(ref rc) = new_obj_val {
                                let mut ob = rc.borrow_mut();
                                ob.remove_property(b"__clone_window");
                                ob.properties
                                    .retain(|(k, _)| !k.starts_with(b"__clone_modified_"));
                            }
                            // (on failure the caller releases the clone: after
                            // its operand for ZEND_CLONE)
                        }
                    }
                }
                new_obj_val
            }
            Value::Generator(_) => {
                let msg = "Trying to clone an uncloneable object of class Generator".to_string();
                let exc_val = self.create_exception(b"Error", &msg, line);
                self.current_exception = Some(exc_val);
                Value::Null
            }
            _other => {
                let type_name = Self::value_type_name(_other);
                let msg = format!(
                    "clone(): Argument #1 ($object) must be of type object, {} given",
                    type_name
                );
                let exc_val = self.throw_type_error(msg);
                self.current_exception = Some(exc_val);
                Value::Null
            }
        }
    }

    /// Apply `clone($obj, $withProperties)`'s updates to the fresh clone
    /// (zend_objects_clone_obj_with): each entry is written like
    /// `$clone->{$key} = $value` from the caller's scope, with readonly
    /// properties re-initializable once.
    pub fn clone_apply_with(&mut self, clone: &Value, with: &Rc<RefCell<PhpArray>>) {
        let Value::Object(obj) = clone else {
            return;
        };
        let entries: Vec<(ArrayKey, Result<Value, ()>)> = with
            .borrow()
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Value::Reference(r) if Rc::strong_count(r) == 1 => Ok(r.borrow().clone()),
                    Value::Reference(_) => Err(()),
                    v => Ok(v.clone()),
                };
                (k.clone(), v)
            })
            .collect();
        if entries.is_empty() {
            return;
        }
        let line = self.current_line;
        let source = b"<?php $__o->{$__k} = $__v;".to_vec();
        let mut lexer = goro_parser::Lexer::new(&source);
        let tokens = lexer.tokenize();
        let mut parser = goro_parser::Parser::new(tokens);
        let Ok(program) = parser.parse() else {
            return;
        };
        let Ok((mut op_array, _, _)) = crate::compiler::Compiler::new().compile(&program) else {
            return;
        };
        for op in op_array.ops.iter_mut() {
            op.line = line;
        }
        let slot = |n: &[u8]| op_array.cv_names.iter().position(|c| c.as_slice() == n);
        let (Some(o_i), Some(k_i), Some(v_i)) = (slot(b"__o"), slot(b"__k"), slot(b"__v")) else {
            return;
        };
        let was_window = obj.borrow().has_property(b"__clone_window");
        obj.borrow_mut()
            .set_property(b"__clone_window".to_vec(), Value::True);
        for (key, value) in entries {
            let Ok(value) = value else {
                let exc = self.create_exception(
                    b"Error",
                    "Cannot assign by reference when cloning with updated properties",
                    line,
                );
                self.current_exception = Some(exc);
                break;
            };
            let name = match key {
                ArrayKey::String(s) => s,
                ArrayKey::Int(n) => PhpString::from_string(n.to_string()),
            };
            if name.as_bytes().first() == Some(&0) {
                let exc = self.create_exception(
                    b"Error",
                    "Cannot access property starting with \"\\0\"",
                    line,
                );
                self.current_exception = Some(exc);
                break;
            }
            let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
            cvs[o_i] = clone.clone();
            cvs[k_i] = Value::String(name);
            cvs[v_i] = value;
            // (its assignment is the internal function's: a set hook's
            // TypeError has no "called in")
            let saved = std::mem::replace(
                &mut self.internal_assign_ops,
                &op_array as *const crate::opcode::OpArray as usize,
            );
            let _ = self.execute_op_array_pub(&op_array, cvs);
            self.internal_assign_ops = saved;
            if self.current_exception.is_some() {
                break;
            }
        }
        if !was_window {
            let mut ob = obj.borrow_mut();
            ob.remove_property(b"__clone_window");
            ob.properties
                .retain(|(k, _)| !k.starts_with(b"__clone_modified_"));
        }
    }
}

/// PHP 8.5 `clone(object $object, array $withProperties = []): object`
/// (ZEND_FUNCTION(clone)); `clone $x` / `clone($x)` compile to ZEND_CLONE.
pub fn clone_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, crate::vm::VmError> {
    let line = vm.current_line;
    let fail = |vm: &mut Vm, class: &[u8], msg: String| {
        let exc = vm.create_exception(class, &msg, line);
        vm.current_exception = Some(exc);
        Err(crate::vm::VmError { message: msg, line })
    };
    if args.is_empty() || args.len() > 2 {
        let msg = if args.is_empty() {
            "clone() expects at least 1 argument, 0 given".to_string()
        } else {
            format!("clone() expects at most 2 arguments, {} given", args.len())
        };
        return fail(vm, b"ArgumentCountError", msg);
    }
    let obj = args[0].deref();
    if !matches!(obj, Value::Object(_)) && !obj.is_closure_obj() {
        let msg = format!(
            "clone(): Argument #1 ($object) must be of type object, {} given",
            Vm::value_type_name(&obj)
        );
        return fail(vm, b"TypeError", msg);
    }
    let with = match args.get(1).map(|v| v.deref()) {
        None | Some(Value::Undef) => None,
        Some(Value::Array(a)) => Some(a),
        Some(other) => {
            let msg = format!(
                "clone(): Argument #2 ($withProperties) must be of type array, {} given",
                Vm::value_type_name(&other)
            );
            return fail(vm, b"TypeError", msg);
        }
    };
    let cloned = vm.clone_value(&obj, line);
    if vm.current_exception.is_none() {
        if let Some(with) = with {
            vm.clone_apply_with(&cloned, &with);
        }
    }
    if vm.current_exception.is_some() {
        return Err(crate::vm::VmError {
            message: String::new(),
            line,
        });
    }
    Ok(cloned)
}
