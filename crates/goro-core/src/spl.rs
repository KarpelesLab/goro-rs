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
    pub(crate) fn spl_find_base(
        &self,
        class_lower: &[u8],
        bases: &[&'static [u8]],
    ) -> Option<&'static [u8]> {
        let mut cur: std::borrow::Cow<[u8]> = std::borrow::Cow::Borrowed(class_lower);
        for _ in 0..64 {
            if let Some(b) = bases.iter().find(|b| **b == &*cur) {
                return Some(b);
            }
            let next: Option<Vec<u8>> = self
                .classes
                .get(&*cur)
                .and_then(|c| c.parent.as_ref())
                .map(|p| p.to_ascii_lowercase())
                .or_else(|| crate::vm::get_builtin_parent(&cur).map(|p| p.to_ascii_lowercase()));
            cur = std::borrow::Cow::Owned(next?);
        }
        None
    }

    /// Whether objects of this class get a built-in SPL `__toString`
    /// (SplFileInfo family and CachingIterator family).
    pub(crate) fn spl_has_builtin_tostring(&self, class_lower: &[u8]) -> bool {
        self.spl_find_base(class_lower, &[b"splfileinfo", b"cachingiterator"])
            .is_some()
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
                let cl = if let Value::Object(ob) = o {
                    obj_class_lower(ob)
                } else {
                    Vec::new()
                };
                let has_ts = self
                    .classes
                    .get(&cl)
                    .map(|c| c.get_method(b"__tostring").is_some())
                    .unwrap_or(false);
                if has_ts {
                    let s = Value::String(self.value_to_string(o));
                    if self.current_exception.is_some() {
                        return 0;
                    }
                    return if obj_left {
                        s.compare(&b)
                    } else {
                        a.compare(&s)
                    };
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
    pub(crate) fn spl_stub_lookup(
        &self,
        class_lower: &[u8],
        method_lower: &[u8],
    ) -> Option<&'static crate::spl_stubs::StubMethod> {
        let mut cur: std::borrow::Cow<[u8]> = std::borrow::Cow::Borrowed(class_lower);
        for _ in 0..16 {
            if let Some(m) = crate::spl_stubs::spl_stub_method(&cur, method_lower) {
                return Some(m);
            }
            {
                let p = crate::vm::get_builtin_parent(&cur)?;
                cur = std::borrow::Cow::Owned(p.to_ascii_lowercase());
            }
        }
        None
    }

    /// Like spl_stub_lookup but also returns the declaring class (lowercase).
    pub(crate) fn spl_stub_lookup_decl(
        &self,
        class_lower: &'static [u8],
        method_lower: &[u8],
    ) -> Option<(
        std::borrow::Cow<'static, [u8]>,
        &'static crate::spl_stubs::StubMethod,
    )> {
        use std::borrow::Cow;
        let mut cur: Cow<'static, [u8]> = Cow::Borrowed(class_lower);
        for _ in 0..16 {
            if let Some(m) = crate::spl_stubs::spl_stub_method(&cur, method_lower) {
                return Some((cur, m));
            }
            {
                let p = crate::vm::get_builtin_parent(&cur)?;
                cur = if p.iter().any(|b| b.is_ascii_uppercase()) {
                    Cow::Owned(p.to_ascii_lowercase())
                } else {
                    Cow::Borrowed(p)
                };
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
        strict: bool,
    ) -> Value {
        let obj = match args.first() {
            Some(Value::Object(o)) => o.clone(),
            _ => return Value::Null,
        };
        // (lowercased in buffers when the names fit)
        let mut cbuf = [0u8; 64];
        let mut mbuf = [0u8; 64];
        let lower = |s: &[u8], buf: &mut [u8; 64]| -> Option<usize> {
            let out = buf.get_mut(..s.len())?;
            for (o, c) in out.iter_mut().zip(s) {
                *o = c.to_ascii_lowercase();
            }
            Some(s.len())
        };
        let class_owned;
        let class_lower: &[u8] = {
            let ob = obj.borrow();
            match lower(&ob.class_name, &mut cbuf) {
                Some(n) => &cbuf[..n],
                None => {
                    class_owned = ob.class_name.to_ascii_lowercase();
                    &class_owned
                }
            }
        };
        let method_owned;
        let method_lower: &[u8] = match lower(method_orig, &mut mbuf) {
            Some(n) => &mbuf[..n],
            None => {
                method_owned = method_orig.to_ascii_lowercase();
                &method_owned
            }
        };
        let base = match self.spl_builtin_base(class_lower) {
            Some(b) => b,
            None => return Value::Null,
        };
        let (decl, stub) = match self.spl_stub_lookup_decl(base, method_lower) {
            Some(x) => x,
            None => return Value::Null,
        };
        let decl_disp: std::borrow::Cow<str> = match crate::stubs::find_class_lower(&decl) {
            Some(c) => std::borrow::Cow::Borrowed(c.name),
            None => std::borrow::Cow::Owned(self.builtin_canonical_name(&decl)),
        };
        let method_disp = String::from_utf8_lossy(method_orig);
        let mut call_args: Vec<Value> = self.take_values(0);
        call_args.extend_from_slice(args);
        // Generic zpp (verified stub arginfo of the method)
        if self.zpp_method_info(base, method_lower).is_some() {
            let mut named = named.to_vec();
            if self
                .zpp_method_call(
                    base,
                    method_lower,
                    &mut call_args,
                    1,
                    &mut named,
                    Some(true),
                    strict,
                    line,
                )
                .is_err()
            {
                return Value::Null;
            }
        } else if !named.is_empty()
            && !self.spl_map_named_args(base, method_lower, &mut call_args, named)
        {
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
            self.call_stack.push((
                format!("{}->{}", decl_disp, method_disp),
                self.current_file.clone(),
                line,
                call_args[1..].to_vec(),
                true,
            ));
            self.spl_throw(b"ArgumentCountError", &msg);
            self.call_stack.pop();
            return Value::Null;
        }
        if let Some((since, message)) = stub.deprecated {
            let msg = if message.is_empty() {
                format!(
                    "Method {}::{}() is deprecated since {}",
                    decl_disp, stub.name, since
                )
            } else {
                format!(
                    "Method {}::{}() is deprecated since {}, {}",
                    decl_disp, stub.name, since, message
                )
            };
            self.emit_deprecated_at(&msg, line);
            if self.current_exception.is_some() {
                return Value::Null;
            }
        }
        // (the frame's buffers are those of finished calls)
        let mut frame = self.string_pool.pop().unwrap_or_default();
        frame.push_str(&decl_disp);
        frame.push_str("->");
        frame.push_str(&method_disp);
        let mut frame2 = self.string_pool.pop().unwrap_or_default();
        frame2.push_str(&frame);
        let file = self.current_file.clone();
        let mut frame_args = self.take_values(0);
        frame_args.extend_from_slice(&call_args[1..]);
        self.call_stack.push((frame, file, line, frame_args, true));
        self.spl_internal_frames
            .push((self.call_stack.len(), frame2));
        let r = self.spl_invoke_builtin(&obj, class_lower, method_lower, &call_args);
        if let Some((_, mut f)) = self.spl_internal_frames.pop() {
            if f.capacity() != 0 && f.capacity() <= 1024 && self.string_pool.len() < 64 {
                f.clear();
                self.string_pool.push(f);
            }
        }
        crate::vm::clear_values(&mut call_args);
        self.recycle_values(call_args);
        // Values the built-in dropped from its storage are destroyed now.
        if !self.destructible_objects.is_empty() {
            self.run_pending_destructors_pub();
        }
        if let Some(f) = self.call_stack.pop() {
            self.recycle_frame(f);
        }
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

    /// Whether argument `arg_index` (counting $this) of a deferred SPL call
    /// is a by-reference parameter of the built-in method.
    pub(crate) fn spl_deferred_arg_by_ref(
        &self,
        call: &crate::vm::PendingCall,
        arg_index: usize,
    ) -> bool {
        let name = call.name.as_bytes();
        if !name.starts_with(b"__splx::") || arg_index == 0 {
            return false;
        }
        let obj = match call.args.first() {
            Some(Value::Object(o)) => o,
            _ => return false,
        };
        let method: Vec<u8> = name[8..].iter().map(|b| b.to_ascii_lowercase()).collect();
        let base = match self.spl_builtin_base(&obj_class_lower(obj)) {
            Some(b) => b,
            None => return false,
        };
        let stub = match self.spl_stub_lookup(base, &method) {
            Some(s) => s,
            None => return false,
        };
        let p = arg_index - 1;
        let last = stub.params.len().saturating_sub(1);
        stub.byref.contains(&p) || (stub.variadic && p >= last && stub.byref.contains(&last))
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
        use std::borrow::Cow;
        // The class chain, walked as needed (the class itself, then up to
        // 50 ancestors).
        let next_of = |vm: &Vm, cur: &[u8]| -> Option<Vec<u8>> {
            vm.classes
                .get(cur)
                .and_then(|c| c.parent.as_ref())
                .map(|p| p.to_ascii_lowercase())
                .or_else(|| crate::vm::get_builtin_parent(cur).map(|p| p.to_ascii_lowercase()))
        };
        let has_args = args.len() > 1;
        let try_args = |vm: &mut Vm| -> Option<Value> {
            let mut c: Cow<[u8]> = Cow::Borrowed(class_lower);
            for _ in 0..51 {
                if vm.is_spl_args_method(&c, method_lower) {
                    if let Some(r) = vm.handle_spl_docall(&c, method_lower, args) {
                        return Some(r);
                    }
                }
                c = Cow::Owned(next_of(vm, &c)?);
            }
            None
        };
        if has_args {
            if let Some(r) = try_args(self) {
                return Some(r);
            }
        }
        let mut c: Cow<[u8]> = Cow::Borrowed(class_lower);
        for _ in 0..51 {
            if let Some(r) = self.dispatch_spl_method(&c, method_lower, obj) {
                return Some(r);
            }
            if self.current_exception.is_some() {
                return Some(Value::Null);
            }
            match next_of(self, &c) {
                Some(n) => c = Cow::Owned(n),
                None => break,
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
    pub(crate) fn spl_forward_unknown_method(
        &mut self,
        name: &[u8],
        args: &[Value],
    ) -> Option<Value> {
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
        let base = self.spl_find_base(
            &class_lower,
            &[b"recursiveiteratoriterator", b"iteratoriterator"],
        )?;
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
        let user_has = self
            .classes
            .get(&tcl)
            .map(|c| c.get_method(&mlower).is_some())
            .unwrap_or(false);
        let builtin_has = self
            .spl_builtin_base(&tcl)
            .map(|b| self.spl_stub_lookup(b, &mlower).is_some())
            .unwrap_or(false);
        if !user_has && !builtin_has {
            // An outer iterator wrapping another one: its get_method
            // handler forwards in turn (spl_dual_it_get_method).
            let mut fwd = tcl.clone();
            fwd.extend_from_slice(b"::");
            fwd.extend_from_slice(method);
            let mut fargs = vec![target.clone()];
            fargs.extend_from_slice(&args[1..]);
            return self.spl_forward_unknown_method(&fwd, &fargs);
        }
        let r = self.call_object_method(&target, method, &args[1..]);
        Some(r.unwrap_or(Value::Null))
    }

    /// Named arguments of a static-syntax call (`parent::m(name: v)`) to the
    /// built-in method `class_lower::method_lower` (`args[0]` is $this):
    /// checked by the generic zpp layer when it knows the method, else
    /// placed by the native handler's or the stub's parameter names. False
    /// with the exception set on failure.
    pub(crate) fn spl_place_named_args(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
        args: &mut Vec<Value>,
        named: &[(Vec<u8>, Value)],
        strict: bool,
        line: u32,
    ) -> bool {
        let mut named = named.to_vec();
        if self.zpp_spl_args(class_lower, method_lower, args, &mut named, strict, line) {
            return false;
        }
        if named.is_empty() {
            return true;
        }
        if self.native_class_handlers.contains_key(class_lower) {
            if let Err(n) = self.merge_native_named_args(class_lower, method_lower, args, 1, &named)
            {
                let msg = format!("Unknown named parameter ${}", String::from_utf8_lossy(&n));
                let exc = self.create_exception(b"Error", &msg, line);
                self.current_exception = Some(exc);
                return false;
            }
            return true;
        }
        self.spl_map_named_args(class_lower, method_lower, args, &named)
    }

    /// Whether an IteratorIterator-family object has method `mlower`
    /// through its inner iterator (spl_dual_it_get_method()).
    pub fn spl_forwarded_method_exists(&self, obj: &Rc<RefCell<PhpObject>>, mlower: &[u8]) -> bool {
        let mut cur = obj.clone();
        for _ in 0..32 {
            let class_lower = obj_class_lower(&cur);
            let Some(base) = self.spl_find_base(
                &class_lower,
                &[b"recursiveiteratoriterator", b"iteratoriterator"],
            ) else {
                return false;
            };
            let target = if base == b"recursiveiteratoriterator" {
                let stack = cur.borrow().get_property(b"__spl_rii_its");
                let top = match &stack {
                    Value::Array(s) => s.borrow().values().last().cloned(),
                    _ => None,
                };
                top.unwrap_or_else(|| cur.borrow().get_property(b"__spl_inner"))
            } else {
                cur.borrow().get_property(b"__spl_inner")
            };
            let Value::Object(t) = target else {
                return false;
            };
            let tcl = obj_class_lower(&t);
            let user_has = self
                .classes
                .get(&tcl)
                .is_some_and(|c| c.get_method(mlower).is_some());
            let builtin_has = self
                .spl_builtin_base(&tcl)
                .is_some_and(|b| self.spl_stub_lookup(b, mlower).is_some());
            if user_has || builtin_has {
                return true;
            }
            cur = t;
        }
        false
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
            let idx = stub
                .params
                .iter()
                .position(|p| p.as_bytes() == name.as_slice());
            match idx {
                None => {
                    let msg = format!("Unknown named parameter ${}", String::from_utf8_lossy(name));
                    self.spl_throw(b"Error", &msg);
                    return false;
                }
                Some(i) if i < positional => {
                    let msg = format!(
                        "Named parameter ${} overwrites previous argument",
                        String::from_utf8_lossy(name)
                    );
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

    /// include_once a file in a fresh function scope (used by spl_autoload).
    /// Returns false if it could not be read/compiled.
    pub fn spl_include_once(&mut self, abs_path: &str) -> bool {
        if self.included_files.contains(abs_path) {
            return true;
        }
        let source = match std::fs::read(abs_path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        self.included_files.insert(abs_path.to_string());
        let mut lexer = goro_parser::Lexer::new(&source);
        let tokens = lexer.tokenize();
        let mut parser = goro_parser::Parser::new(tokens).with_source(&source);
        let program = match parser.parse() {
            Ok(p) => p,
            Err(_) => return false,
        };
        let mut compiler = crate::compiler::Compiler::new();
        compiler.source_file = abs_path.as_bytes().to_vec();
        let old_file = std::mem::replace(&mut self.current_file, abs_path.into());
        let ok = match compiler.compile(&program) {
            Ok((op_array, classes, warnings)) => {
                for (msg, wline) in &warnings {
                    self.emit_compile_diagnostic(msg, *wline);
                }
                let saved_pending = std::mem::replace(&mut self.pending_classes, classes);
                let cvs = vec![Value::Undef; op_array.cv_names.len()];
                let _ = self.execute_op_array_pub(&op_array, cvs);
                self.pending_classes = saved_pending;
                true
            }
            Err(_) => false,
        };
        self.current_file = old_file;
        ok
    }

    /// Invoke one registered autoloader (normalized as by
    /// spl_autoload_register: function name, [class|object, method], or a
    /// closure/invokable) with the class name. Pushes an
    /// "[internal function]" frame like zend_call_known_function.
    pub fn spl_call_autoloader(
        &mut self,
        cb: &Value,
        class_name: &Value,
    ) -> Option<crate::vm::VmError> {
        let mut fatal: Option<crate::vm::VmError> = None;
        let args = vec![class_name.clone()];
        // Called by the engine for user code (new, static access, ...) the
        // autoloader's frame is at the current line; from a built-in
        // function (class_exists(), ...) at [internal function].
        let from_builtin = self.spl_caller_is_internal() || self.in_builtin_frame();
        let (site_file, site_line) = if from_builtin {
            (crate::rcstr::RcStr::new(), 0)
        } else {
            (self.current_file.clone(), self.current_line)
        };
        // Invokable objects: call __invoke with the object's class scope.
        if let Value::Object(o) = cb {
            let cl = obj_class_lower(o);
            if cl != b"closure"
                && self
                    .classes
                    .get(&cl)
                    .map(|c| c.get_method(b"__invoke").is_some())
                    .unwrap_or(false)
            {
                let mut a = PhpArray::new();
                a.push(cb.clone());
                a.push(Value::String(PhpString::from_bytes(b"__invoke")));
                return self.spl_call_autoloader(&val_arr(a), class_name);
            }
        }
        match cb {
            Value::Array(a) => {
                let vals: Vec<Value> = a.borrow().values().cloned().collect();
                if vals.len() == 2 {
                    let method = vals[1].to_php_string();
                    let mlower: Vec<u8> = method
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    let (called_lower, this, disp_cls) = match &vals[0] {
                        Value::Object(o) => (
                            obj_class_lower(o),
                            Some(vals[0].clone()),
                            crate::value::display_class_name(&o.borrow().class_name).to_string(),
                        ),
                        other => {
                            let s = other.to_php_string();
                            (
                                s.as_bytes()
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect(),
                                None,
                                s.to_string_lossy(),
                            )
                        }
                    };
                    let mut mdef = self
                        .classes
                        .get(&called_lower)
                        .and_then(|c| c.get_method(&mlower))
                        .cloned();
                    // (get_called_class() names the class as declared)
                    let called_name = self
                        .classes
                        .get(&called_lower)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| called_lower.clone());
                    // Non-public methods are not callable from the internal
                    // scope: route through __call when available.
                    if let Some(m) = &mdef {
                        if m.visibility != crate::object::Visibility::Public && this.is_some() {
                            if let Some(call) = self
                                .classes
                                .get(&called_lower)
                                .and_then(|c| c.get_method(b"__call"))
                                .cloned()
                            {
                                let mut argv = PhpArray::new();
                                for a in &args {
                                    argv.push(a.clone());
                                }
                                let call_args = vec![Value::String(method.clone()), val_arr(argv)];
                                self.called_class_stack.push(called_name.clone());
                                self.push_class_scope(call.declaring_class.clone());
                                if let Err(e) = self.execute_fn_with_named_args(
                                    &call.op_array,
                                    call_args,
                                    vec![],
                                    this.clone(),
                                ) {
                                    fatal = Some(e);
                                }
                                self.pop_class_scope();
                                self.called_class_stack.pop();
                                return fatal;
                            }
                            mdef = Some(m.clone());
                        }
                    }
                    if let Some(m) = mdef {
                        let sep = if this.is_some() { "->" } else { "::" };
                        let decl_disp = self
                            .classes
                            .get(&m.declaring_class)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or(disp_cls);
                        let fname =
                            format!("{}{}{}", decl_disp, sep, String::from_utf8_lossy(&m.name));
                        self.call_stack.push((
                            fname,
                            site_file,
                            site_line,
                            args.clone(),
                            this.is_some(),
                        ));
                        self.called_class_stack.push(called_name.clone());
                        self.push_class_scope(m.declaring_class.clone());
                        if let Err(e) =
                            self.execute_fn_with_named_args(&m.op_array, args, vec![], this)
                        {
                            fatal = Some(e);
                        }
                        self.pop_class_scope();
                        self.called_class_stack.pop();
                        self.call_stack.pop();
                        return fatal;
                    }
                }
                if let Err(e) = self.call_callback(cb, &args) {
                    fatal = Some(e);
                }
            }
            Value::String(s) => {
                let lower: Vec<u8> = s
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if self.user_functions.contains_key(&lower) && from_builtin {
                    self.call_stack.push((
                        s.to_string_lossy(),
                        site_file,
                        site_line,
                        args.clone(),
                        false,
                    ));
                    if let Err(e) = self.call_callback(cb, &args) {
                        fatal = Some(e);
                    }
                    self.call_stack.pop();
                } else if self.user_functions.contains_key(&lower) {
                    // The callee's frame is located at the current line.
                    self.ucf_call_line = Some(site_line);
                    if let Err(e) = self.call_callback(cb, &args) {
                        fatal = Some(e);
                    }
                    self.ucf_call_line = None;
                } else {
                    if let Err(e) = self.call_callback(cb, &args) {
                        fatal = Some(e);
                    }
                }
            }
            _ => {
                // A closure called by the engine: its frame is at the current
                // line (zend_call_known_function from the running opcode).
                if !from_builtin {
                    self.ucf_call_line = Some(site_line);
                }
                let r = self.call_callback(cb, &args);
                self.ucf_call_line = None;
                if let Err(e) = r {
                    fatal = Some(e);
                }
            }
        }
        fatal
    }

    /// Allocate a new object of `class` (user or built-in) with property
    /// defaults applied; the constructor is not called.
    pub fn spl_instantiate(&mut self, class: &[u8]) -> Rc<RefCell<PhpObject>> {
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
        self.classes
            .get(class_lower)
            .map(|c| c.get_method(b"__construct").is_some())
            .unwrap_or(false)
    }

    /// Native get_debug_info for SPL classes: the property table (mangled
    /// keys) var_dump/print_r should show, or None for non-SPL objects.
    pub fn spl_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Option<PhpArray> {
        let cl = obj_class_lower(obj);
        let base = self.spl_find_base(
            &cl,
            &[
                b"splpriorityqueue",
                b"splminheap",
                b"splmaxheap",
                b"splheap",
                b"directoryiterator",
                b"splfileobject",
                b"splfileinfo",
                b"splfixedarray",
                b"spldoublylinkedlist",
                b"splobjectstorage",
                b"multipleiterator",
                b"arrayobject",
                b"arrayiterator",
            ],
        )?;
        match base {
            b"arrayobject" | b"arrayiterator" => Some(self.ao_debug_info(obj)),
            b"splobjectstorage" | b"multipleiterator" => Some(self.sos_debug_info(obj)),
            b"spldoublylinkedlist" => Some(self.dll_debug_info(obj)),
            b"splfixedarray" => Some(self.fa_debug_info(obj, true)),
            b"splfileobject" | b"splfileinfo" | b"directoryiterator" => {
                Some(self.fs_debug_info(obj))
            }
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
        let frame = format!("{}->__construct", disp);
        self.call_stack.push((
            frame.clone(),
            self.current_file.clone(),
            line,
            args.to_vec(),
            true,
        ));
        self.spl_internal_frames
            .push((self.call_stack.len(), frame));
    }

    /// Raise a fatal error from a built-in SPL method; the deferred call
    /// returns an error so execution stops.
    pub(crate) fn spl_fatal(&mut self, msg: &str) {
        let line = self.current_line;
        self.emit_fatal_at(msg, line);
    }

    /// Whether the innermost frame is a built-in SPL method (stale entries
    /// of frames popped without unregistering are discarded).
    pub(crate) fn spl_caller_is_internal(&mut self) -> bool {
        let len = self.call_stack.len();
        while matches!(self.spl_internal_frames.last(), Some((n, _)) if *n > len) {
            self.spl_internal_frames.pop();
        }
        match (self.spl_internal_frames.last(), self.call_stack.last()) {
            (Some((n, name)), Some(top)) if *n == len && *name == top.0 => true,
            _ => self.caller_is_internal(),
        }
    }

    /// Call a method from built-in SPL code. A user-defined method gets a
    /// stack frame located at "[internal function]" when called from a
    /// built-in method, or at the current line otherwise (e.g. foreach).
    pub(crate) fn spl_call_method(
        &mut self,
        obj_val: &Value,
        method: &[u8],
        args: &[Value],
    ) -> Option<Value> {
        let internal = self.spl_caller_is_internal();
        self.spl_call_method_at(obj_val, method, args, internal)
    }

    /// spl_call_method() with an explicit caller kind (`internal`: called
    /// from a built-in function or method).
    pub(crate) fn spl_call_method_at(
        &mut self,
        obj_val: &Value,
        method: &[u8],
        args: &[Value],
        internal: bool,
    ) -> Option<Value> {
        let frame = match obj_val {
            Value::Object(o) => {
                // (lowercased in buffers when the names fit)
                let mut cbuf = [0u8; 64];
                let mut mbuf = [0u8; 64];
                let lower = |s: &[u8], buf: &'_ mut [u8; 64]| -> Option<usize> {
                    let out = buf.get_mut(..s.len())?;
                    for (d, c) in out.iter_mut().zip(s) {
                        *d = c.to_ascii_lowercase();
                    }
                    Some(s.len())
                };
                let cl_owned;
                let cl: &[u8] = {
                    let ob = o.borrow();
                    match lower(&ob.class_name, &mut cbuf) {
                        Some(n) => &cbuf[..n],
                        None => {
                            cl_owned = ob.class_name.to_ascii_lowercase();
                            &cl_owned
                        }
                    }
                };
                let ml_owned;
                let ml: &[u8] = match lower(method, &mut mbuf) {
                    Some(n) => &mbuf[..n],
                    None => {
                        ml_owned = method.to_ascii_lowercase();
                        &ml_owned
                    }
                };
                match self.classes.get(cl).and_then(|c| c.get_method(ml)) {
                    Some(m) => {
                        // (in the buffer of a finished call's frame)
                        let mut name = self.string_pool.pop().unwrap_or_default();
                        match self.classes.get(&m.declaring_class) {
                            Some(dc) => name.push_str(&crate::value::display_class_name(&dc.name)),
                            None => name.push_str(&String::from_utf8_lossy(&m.declaring_class)),
                        }
                        name.push_str(if m.is_static { "::" } else { "->" });
                        name.push_str(&String::from_utf8_lossy(&m.name));
                        Some(name)
                    }
                    None => None,
                }
            }
            _ => None,
        };
        match frame {
            Some(name) => {
                let (file, line) = if internal {
                    (crate::rcstr::RcStr::new(), 0)
                } else {
                    (self.current_file.clone(), self.current_line)
                };
                let mut frame_args = self.take_values(0);
                frame_args.extend_from_slice(args);
                self.call_stack.push((name, file, line, frame_args, true));
                let r = self.call_object_method(obj_val, method, args);
                if let Some(f) = self.call_stack.pop() {
                    self.recycle_frame(f);
                }
                r
            }
            None => self.call_object_method(obj_val, method, args),
        }
    }

    /// get_iterator-time checks done by foreach. Returns false with an
    /// exception set when iteration must not start.
    pub(crate) fn spl_foreach_check(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let cl = obj_class_lower(obj);
        if self
            .spl_find_base(&cl, &[b"recursiveiteratoriterator"])
            .is_some()
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
    ) -> Result<Option<Result<Rc<RefCell<PhpArray>>, Rc<RefCell<PhpObject>>>>, ()> {
        let cl = obj_class_lower(obj);
        if cl == b"weakmap" {
            return Ok(None);
        }
        if let Some(base) = self.spl_find_base(&cl, &[b"arrayobject", b"arrayiterator"]) {
            // A subclass overriding an Iterator method iterates through
            // zend_user_it_get_iterator(), which refuses by-ref foreach.
            if base == b"arrayiterator"
                && [&b"rewind"[..], b"valid", b"current", b"key", b"next"]
                    .iter()
                    .any(|m| {
                        self.classes
                            .get(&cl)
                            .map(|c| c.get_method(m).is_some())
                            .unwrap_or(false)
                    })
            {
                self.spl_throw(
                    b"Error",
                    "An iterator cannot be used with foreach by reference",
                );
                return Err(());
            }
            let backing = obj.borrow().get_property(b"__spl_array");
            return match backing {
                Value::Array(a) => Ok(Some(Ok(a))),
                // Backed by an object: its property table (or the
                // storage of the ArrayObject it wraps) is iterated.
                Value::Object(o) => {
                    let ocl = obj_class_lower(&o);
                    if self
                        .spl_find_base(&ocl, &[b"arrayobject", b"arrayiterator"])
                        .is_some()
                    {
                        self.spl_foreach_ref_target(&o)
                    } else {
                        Ok(Some(Err(o)))
                    }
                }
                _ => Ok(None),
            };
        }
        let is_iter = self.class_implements_interface(&cl, b"iterator")
            || crate::vm::is_builtin_subclass(&cl, b"iterator");
        if is_iter {
            self.spl_throw(
                b"Error",
                "An iterator cannot be used with foreach by reference",
            );
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
            // object_properties_load(): a property the class does not
            // declare is a dynamic one.
            if !obj.borrow().has_property(&name) && !self.spl_allows_dynamic_props(obj) {
                let msg = format!(
                    "Creation of dynamic property {}::${} is deprecated",
                    crate::value::display_class_name(&obj.borrow().class_name),
                    String::from_utf8_lossy(&name)
                );
                let had_exc = self.current_exception.is_some();
                self.emit_deprecated(&msg);
                if !had_exc && self.current_exception.is_some() {
                    return;
                }
            }
            obj.borrow_mut().set_property(name, v.clone());
        }
    }

    /// Whether the object's class (or an ancestor) is
    /// #[AllowDynamicProperties].
    fn spl_allows_dynamic_props(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let mut cur = Some(obj_class_lower(obj));
        for _ in 0..64 {
            let Some(c) = cur else { break };
            match self.classes.get(&c) {
                Some(ce) => {
                    if ce.allow_dynamic_properties {
                        return true;
                    }
                    cur = ce.parent.as_ref().map(|p| p.to_ascii_lowercase());
                }
                None => break,
            }
        }
        false
    }
}

/// All built-in SPL classes (lowercase).
pub(crate) const SPL_CLASSES: &[&[u8]] = &[
    b"arrayobject",
    b"arrayiterator",
    b"recursivearrayiterator",
    b"splfixedarray",
    b"spldoublylinkedlist",
    b"splqueue",
    b"splstack",
    b"splheap",
    b"splminheap",
    b"splmaxheap",
    b"splpriorityqueue",
    b"splobjectstorage",
    b"multipleiterator",
    b"splfileinfo",
    b"directoryiterator",
    b"filesystemiterator",
    b"recursivedirectoryiterator",
    b"globiterator",
    b"splfileobject",
    b"spltempfileobject",
    b"emptyiterator",
    b"iteratoriterator",
    b"recursiveiteratoriterator",
    b"recursivetreeiterator",
    b"filteriterator",
    b"recursivefilteriterator",
    b"callbackfilteriterator",
    b"recursivecallbackfilteriterator",
    b"parentiterator",
    b"limititerator",
    b"cachingiterator",
    b"recursivecachingiterator",
    b"norewinditerator",
    b"appenditerator",
    b"infiniteiterator",
    b"regexiterator",
    b"recursiveregexiterator",
];

std::thread_local! {
    static AUTOLOADING: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

/// Recursion guard for autoloading: returns false if `class_lower` is
/// already being autoloaded.
pub(crate) fn autoload_enter(class_lower: &[u8]) -> bool {
    AUTOLOADING.with(|a| {
        let mut a = a.borrow_mut();
        if a.iter().any(|c| c == class_lower) {
            return false;
        }
        a.push(class_lower.to_vec());
        true
    })
}

pub(crate) fn autoload_leave(class_lower: &[u8]) {
    AUTOLOADING.with(|a| {
        let mut a = a.borrow_mut();
        if let Some(p) = a.iter().rposition(|c| c == class_lower) {
            a.remove(p);
        }
    });
}

/// Whether a freshly instantiated object of this class needs an empty
/// `__spl_array` storage (classes whose older code paths require it).
pub fn spl_class_needs_storage(class_lower: &[u8]) -> bool {
    matches!(
        class_lower,
        b"arrayobject"
            | b"arrayiterator"
            | b"recursivearrayiterator"
            | b"splobjectstorage"
            | b"spldoublylinkedlist"
            | b"splqueue"
            | b"splstack"
    )
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
    obj.borrow()
        .class_name
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect()
}

pub(crate) fn val_arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// The array held in the hidden storage property `name`, for in-place
/// updates: created empty when missing, and replaced by a copy first when
/// anything else holds it (or it is not a plain array).
pub(crate) fn storage_arr(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Rc<RefCell<PhpArray>> {
    {
        let ob = obj.borrow();
        if let Some((_, Value::Array(a))) = ob.properties.iter().find(|(k, _)| k == name) {
            if Rc::strong_count(a) == 1 {
                return a.clone();
            }
        }
    }
    let copy = match obj.borrow().get_property(name) {
        Value::Array(a) => a.borrow().clone(),
        _ => PhpArray::new(),
    };
    let a = Rc::new(RefCell::new(copy));
    obj.borrow_mut()
        .set_property_slice(name, Value::Array(a.clone()));
    a
}
