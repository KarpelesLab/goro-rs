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

    /// Like spl_stub_lookup but also returns the declaring class (lowercase).
    pub(crate) fn spl_stub_lookup_decl(&self, class_lower: &[u8], method_lower: &[u8]) -> Option<(Vec<u8>, &'static crate::spl_stubs::StubMethod)> {
        let mut cur = class_lower.to_vec();
        for _ in 0..16 {
            if let Some(m) = crate::spl_stubs::spl_stub_method(&cur, method_lower) {
                return Some((cur, m));
            }
            match crate::vm::get_builtin_parent(&cur) {
                Some(p) => cur = p.iter().map(|b| b.to_ascii_lowercase()).collect(),
                None => return None,
            }
        }
        None
    }

    /// The built-in SPL class an object's class derives from (or is).
    pub(crate) fn spl_builtin_base(&self, class_lower: &[u8]) -> Option<&'static [u8]> {
        // Most-derived first so the walk stops at the nearest built-in.
        self.spl_find_base(class_lower, SPL_CLASSES)
    }

    /// Whether a method call on an object of `class_lower` should be
    /// dispatched as a deferred SPL call (see spl_deferred_call).
    pub(crate) fn spl_should_defer(&self, class_lower: &[u8], method_lower: &[u8]) -> bool {
        if method_lower.starts_with(b"__") {
            return false;
        }
        match self.spl_builtin_base(class_lower) {
            Some(base) => self.spl_stub_lookup(base, method_lower).is_some(),
            None => false,
        }
    }

    /// Execute a deferred SPL method call: `args[0]` is $this. Maps named
    /// arguments, checks arity against the stub signature, pushes a stack
    /// frame (`Class->method`) and runs the built-in implementation.
    /// Exceptions are left in current_exception.
    pub(crate) fn spl_deferred_call(
        &mut self,
        method_orig: &[u8],
        args: &[Value],
        named: &[(Vec<u8>, Value)],
        line: u32,
    ) -> Value {
        let obj = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return Value::Null,
        };
        let class_lower = obj_class_lower(&obj);
        let method_lower: Vec<u8> = method_orig.iter().map(|b| b.to_ascii_lowercase()).collect();
        let base = match self.spl_builtin_base(&class_lower) {
            Some(b) => b,
            None => return Value::Null,
        };
        let (decl, stub) = match self.spl_stub_lookup_decl(base, &method_lower) {
            Some(x) => x,
            None => return Value::Null,
        };
        let decl_disp = self.builtin_canonical_name(&decl);
        let method_disp = String::from_utf8_lossy(method_orig).to_string();
        let mut call_args: Vec<Value> = args.to_vec();
        if !named.is_empty() && !self.spl_map_named_args(base, &method_lower, &mut call_args, named) {
            return Value::Null;
        }
        // Drop trailing skipped (Undef) optionals.
        while call_args.len() > 1 && matches!(call_args.last(), Some(Value::Undef)) {
            call_args.pop();
        }
        let given = call_args.len() - 1;
        let max = stub.params.len();
        if given < stub.required || (!stub.variadic && given > max) {
            let (word, n) = if stub.required == max && !stub.variadic {
                ("exactly", max)
            } else if given < stub.required {
                ("at least", stub.required)
            } else {
                ("at most", max)
            };
            let msg = format!(
                "{}::{}() expects {} {} argument{}, {} given",
                decl_disp,
                method_disp,
                word,
                n,
                if n == 1 { "" } else { "s" },
                given
            );
            self.call_stack.push((format!("{}->{}", decl_disp, method_disp), self.current_file.clone(), line, call_args[1..].to_vec(), true));
            self.spl_throw(b"ArgumentCountError", &msg);
            self.call_stack.pop();
            return Value::Null;
        }
        self.call_stack.push((format!("{}->{}", decl_disp, method_disp), self.current_file.clone(), line, call_args[1..].to_vec(), true));
        let r = self.spl_invoke_builtin(&obj, &class_lower, &method_lower, &call_args);
        self.call_stack.pop();
        match r {
            Some(v) => v,
            None => {
                if self.current_exception.is_none() {
                    let msg = format!(
                        "Call to undefined method {}::{}()",
                        crate::value::display_class_name(&obj.borrow().class_name),
                        method_disp
                    );
                    self.spl_throw(b"Error", &msg);
                }
                Value::Null
            }
        }
    }

    /// Run a built-in SPL method over the class chain. With user arguments
    /// the argument-taking implementation is preferred.
    fn spl_invoke_builtin(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        class_lower: &[u8],
        method_lower: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        let mut chain: Vec<Vec<u8>> = vec![class_lower.to_vec()];
        let mut cur = class_lower.to_vec();
        for _ in 0..50 {
            let next: Option<Vec<u8>> = self
                .classes
                .get(&cur)
                .and_then(|c| c.parent.clone())
                .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect())
                .or_else(|| crate::vm::get_builtin_parent(&cur).map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect()));
            match next {
                Some(n) => {
                    chain.push(n.clone());
                    cur = n;
                }
                None => break,
            }
        }
        let has_args = args.len() > 1;
        let try_args = |vm: &mut Vm| -> Option<Value> {
            for c in &chain {
                if vm.is_spl_args_method(c, method_lower) {
                    if let Some(r) = vm.handle_spl_docall(c, method_lower, args) {
                        return Some(r);
                    }
                }
            }
            None
        };
        if has_args {
            if let Some(r) = try_args(self) {
                return Some(r);
            }
        }
        for c in &chain {
            if let Some(r) = self.dispatch_spl_method(c, method_lower, obj) {
                return Some(r);
            }
            if self.current_exception.is_some() {
                return Some(Value::Null);
            }
        }
        if !has_args {
            return try_args(self);
        }
        None
    }

    /// Unknown method on an IteratorIterator-family / RecursiveIteratorIterator
    /// object: php-src forwards the call to the inner iterator (or, for
    /// RecursiveIteratorIterator, the current sub-iterator). `name` is
    /// "Class::method" and `args[0]` is $this. Returns None when not
    /// applicable.
    pub(crate) fn spl_forward_unknown_method(&mut self, name: &[u8], args: &[Value]) -> Option<Value> {
        let sep = name.windows(2).position(|w| w == b"::")?;
        let class_lower: Vec<u8> = name[..sep].iter().map(|b| b.to_ascii_lowercase()).collect();
        let method = &name[sep + 2..];
        let obj = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return None,
        };
        if obj_class_lower(&obj) != class_lower {
            return None;
        }
        let base = self.spl_find_base(&class_lower, &[b"recursiveiteratoriterator", b"iteratoriterator"])?;
        let target = if base == b"recursiveiteratoriterator" {
            let stack = obj.borrow().get_property(b"__spl_rii_its");
            let top = match &stack {
                Value::Array(s) => s.borrow().values().last().cloned(),
                _ => None,
            };
            top.unwrap_or_else(|| obj.borrow().get_property(b"__spl_inner"))
        } else {
            obj.borrow().get_property(b"__spl_inner")
        };
        let tobj = match &target {
            Value::Object(t) => t.clone(),
            _ => return None,
        };
        // Only forward when the target actually has the method.
        let tcl = obj_class_lower(&tobj);
        let mlower: Vec<u8> = method.iter().map(|b| b.to_ascii_lowercase()).collect();
        let user_has = self.classes.get(&tcl).map(|c| c.get_method(&mlower).is_some()).unwrap_or(false);
        let builtin_has = self
            .spl_builtin_base(&tcl)
            .map(|b| self.spl_stub_lookup(b, &mlower).is_some())
            .unwrap_or(false);
        if !user_has && !builtin_has {
            return None;
        }
        let r = self.call_object_method(&target, method, &args[1..]);
        Some(r.unwrap_or(Value::Null))
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
                b"directoryiterator", b"splfileobject", b"splfileinfo", b"splfixedarray",
            ],
        )?;
        match base {
            b"splfixedarray" => Some(self.fa_debug_info(obj, true)),
            b"splfileobject" => Some(self.sfi_debug_info(obj, true)),
            b"splfileinfo" => Some(self.sfi_debug_info(obj, false)),
            b"splpriorityqueue" => Some(self.heap_debug_info(obj, true)),
            b"splminheap" | b"splmaxheap" | b"splheap" => Some(self.heap_debug_info(obj, false)),
            _ => None,
        }
    }

    /// Push a `Base->__construct` stack frame (Base = nearest built-in SPL
    /// class of obj) while a native constructor runs.
    pub(crate) fn spl_push_ctor_frame(&mut self, obj: &Rc<RefCell<PhpObject>>, args: &[Value]) {
        let cl = obj_class_lower(obj);
        let base = self.spl_builtin_base(&cl).map(|b| b.to_vec()).unwrap_or(cl);
        let disp = self.builtin_canonical_name(&base);
        let line = self.current_line;
        self.call_stack.push((format!("{}->__construct", disp), self.current_file.clone(), line, args.to_vec(), true));
    }

    /// get_iterator-time checks done by foreach. Returns false with an
    /// exception set when iteration must not start.
    pub(crate) fn spl_foreach_check(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        if self.spl_find_base(&cl, &[b"recursiveiteratoriterator"]).is_some()
            && !matches!(obj.borrow().get_property(b"__spl_rii_its"), Value::Array(_))
        {
            self.spl_throw(b"Error", "Object is not initialized");
            return false;
        }
        true
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

/// All built-in SPL classes (lowercase).
pub(crate) const SPL_CLASSES: &[&[u8]] = &[
    b"arrayobject", b"arrayiterator", b"recursivearrayiterator",
    b"splfixedarray", b"spldoublylinkedlist", b"splqueue", b"splstack",
    b"splheap", b"splminheap", b"splmaxheap", b"splpriorityqueue",
    b"splobjectstorage", b"multipleiterator",
    b"splfileinfo", b"directoryiterator", b"filesystemiterator",
    b"recursivedirectoryiterator", b"globiterator", b"splfileobject", b"spltempfileobject",
    b"emptyiterator", b"iteratoriterator", b"recursiveiteratoriterator", b"recursivetreeiterator",
    b"filteriterator", b"recursivefilteriterator", b"callbackfilteriterator",
    b"recursivecallbackfilteriterator", b"parentiterator", b"limititerator",
    b"cachingiterator", b"recursivecachingiterator", b"norewinditerator",
    b"appenditerator", b"infiniteiterator", b"regexiterator", b"recursiveregexiterator",
];

/// ce->interfaces of an interface/class known from the SPL stubs (or a core
/// iterator interface), in php-src's order. None for unknown classes.
pub fn spl_class_interfaces(class_lower: &[u8]) -> Option<Vec<&'static str>> {
    match class_lower {
        b"iterator" | b"iteratoraggregate" => return Some(vec!["Traversable"]),
        b"traversable" | b"arrayaccess" | b"countable" | b"serializable" | b"stringable"
        | b"jsonserializable" => return Some(vec![]),
        b"internaliterator" => return Some(vec!["Iterator", "Traversable"]),
        _ => {}
    }
    if class_lower.ends_with(b"exception") {
        return None;
    }
    let (_, parent, ifaces, _, _) = crate::spl_stubs::spl_stub_class(class_lower)?;
    let mut out: Vec<&'static str> = match parent {
        Some(p) => spl_class_interfaces(p.to_ascii_lowercase().as_bytes()).unwrap_or_default(),
        None => Vec::new(),
    };
    for i in ifaces.iter() {
        if out.iter().any(|x| x.eq_ignore_ascii_case(i)) {
            continue;
        }
        out.push(i);
        for sub in spl_class_interfaces(i.to_ascii_lowercase().as_bytes()).unwrap_or_default() {
            if !out.iter().any(|x| x.eq_ignore_ascii_case(sub)) {
                out.push(sub);
            }
        }
    }
    Some(out)
}

/// Whether an SPL class implements `iface_lower` (None: not an SPL class).
pub fn spl_class_implements(class_lower: &[u8], iface_lower: &[u8]) -> Option<bool> {
    let list = spl_class_interfaces(class_lower)?;
    Some(list.iter().any(|i| i.as_bytes().eq_ignore_ascii_case(iface_lower)))
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
