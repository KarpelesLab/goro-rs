//! Callable validation: a port of php-src's `zend_is_callable_ex()`,
//! `zend_is_callable_check_class()`, `zend_is_callable_check_func()` and
//! `zend_get_callable_name_ex()` (Zend/zend_API.c).
//!
//! Every place that validates a callable (is_callable(), the `callable`
//! parameter / return type, zpp `f` parameters of internal functions,
//! call_user_func() and the array callbacks, set_error_handler(), ...) goes
//! through [`Vm::zend_is_callable_ex`] (or the side-effect free
//! [`Vm::is_callable_pure`] where no `&mut Vm` is at hand), so they all accept
//! the same callables and report the same error strings.

use crate::object::Visibility;
use crate::value::Value;
use crate::vm::Vm;

/// `IS_CALLABLE_CHECK_SYNTAX_ONLY`
pub const IS_CALLABLE_CHECK_SYNTAX_ONLY: u32 = 1;
/// `IS_CALLABLE_SUPPRESS_DEPRECATIONS`
pub const IS_CALLABLE_SUPPRESS_DEPRECATIONS: u32 = 2;

/// Result of a callable check (the parts of zend_fcall_info_cache the
/// callers need, plus the error and the callable name).
#[derive(Debug, Default, Clone)]
pub struct CallableCheck {
    pub ok: bool,
    /// zend_get_callable_name_ex()
    pub name: Vec<u8>,
    /// The reason it is not callable (zend_is_callable_ex's `error`)
    pub error: Option<String>,
    /// Lowercase name of the class the method is looked up in
    pub calling_scope: Option<Vec<u8>>,
    /// The method is called with an object
    pub has_object: bool,
    /// That object
    pub object: Option<Value>,
    /// The call goes through __call() / __callStatic() for this method name
    /// (a trampoline: the method is missing or not accessible).
    pub trampoline: Option<Vec<u8>>,
    /// Deprecations raised by the check (emitted by zend_is_callable_ex)
    pub deprecations: Vec<String>,
}

/// A method as zend_is_callable_check_func sees it.
#[derive(Debug, Clone)]
struct MethodInfo {
    name: Vec<u8>,
    /// Lowercase declaring class
    decl: Vec<u8>,
    vis: Visibility,
    is_static: bool,
    is_abstract: bool,
}

#[derive(Default)]
struct Fcc {
    calling_scope: Option<Vec<u8>>,
    called_scope: Option<Vec<u8>>,
    object: Option<Value>,
    deprecations: Vec<String>,
    trampoline: Option<Vec<u8>>,
}

fn lower(b: &[u8]) -> Vec<u8> {
    b.to_ascii_lowercase()
}

fn is_closure_bytes(b: &[u8]) -> bool {
    b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_")
}

/// Language constructs are not functions, although the VM may implement
/// some of them as internal helpers.
fn is_construct_name(l: &[u8]) -> bool {
    matches!(
        l,
        b"isset"
            | b"empty"
            | b"eval"
            | b"echo"
            | b"print"
            | b"unset"
            | b"list"
            | b"array"
            | b"include"
            | b"include_once"
            | b"require"
            | b"require_once"
    )
}

/// The class name of an object-like value.
fn object_class(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Object(o) => Some(o.borrow().class_name.clone()),
        Value::Generator(_) => Some(b"Generator".to_vec()),
        _ => None,
    }
}

impl Vm {
    /// Class scope of the calling user code (lowercase), for callables.
    fn callable_scope(&self) -> Option<Vec<u8>> {
        self.current_class_scope().map(|s| lower(&s))
    }

    /// `$this` of the running user frame (zend_get_this_object).
    pub fn current_frame_this(&self) -> Option<Value> {
        let f = self.gc_frames.iter().rev().find(|f| !f.is_pin())?;
        // SAFETY: the frames in gc_frames are running (each is popped
        // before its function returns).
        unsafe { f.this_object() }
    }

    /// The class exists (without autoloading).
    fn callable_class_exists(&self, cl: &[u8]) -> bool {
        self.classes.contains_key(cl)
            || self.is_known_builtin_class(cl)
            || self.native_class_handlers.contains_key(cl)
    }

    /// Canonical name of a (lowercase) class, as `ce->name`.
    pub(crate) fn callable_class_name(&self, cl: &[u8]) -> String {
        if let Some(c) = self.classes.get(cl) {
            return crate::value::display_class_name(&c.name);
        }
        if let Some(c) = crate::stubs::find_class(cl) {
            return c.name.to_string();
        }
        self.builtin_canonical_name(cl)
    }

    fn callable_instanceof(&self, a: &[u8], b: &[u8]) -> bool {
        a == b || self.class_extends(a, b) || self.class_implements_interface(a, b)
    }

    /// Look up a method in the function table of class `cl` (lowercase).
    fn callable_find_method(&self, cl: &[u8], ml: &[u8]) -> Option<MethodInfo> {
        let mut cur = cl.to_vec();
        for _ in 0..64 {
            match self.classes.get(&cur) {
                Some(c) => {
                    if let Some(m) = c.methods.get(ml) {
                        return Some(MethodInfo {
                            name: m.name.clone(),
                            decl: lower(&m.declaring_class),
                            vis: m.visibility,
                            is_static: m.is_static,
                            is_abstract: m.is_abstract,
                        });
                    }
                    // An enum's from() / tryFrom() / cases()
                    if c.is_enum
                        && (ml == b"cases"
                            || (c.enum_backing_type.is_some()
                                && matches!(ml, b"from" | b"tryfrom")))
                    {
                        return Some(MethodInfo {
                            name: match ml {
                                b"tryfrom" => b"tryFrom".to_vec(),
                                _ => ml.to_vec(),
                            },
                            decl: cur,
                            vis: Visibility::Public,
                            is_static: true,
                            is_abstract: false,
                        });
                    }
                    match &c.parent {
                        Some(p) => cur = lower(p),
                        // an extension class whose methods its native
                        // handler serves: its stub declares them
                        None => break,
                    }
                }
                None => break,
            }
        }
        // An internal class (or the internal ancestor of a user class)
        if let Some((decl, m)) = crate::stubs::find_method(&cur, ml) {
            use crate::stubs::F_ABSTRACT;
            // Interface methods of a concrete internal class are implemented.
            let is_abstract = m.flags & F_ABSTRACT != 0
                && (decl.kind == crate::stubs::K_INTERFACE || decl.flags & F_ABSTRACT != 0)
                && crate::stubs::find_class(&cur).is_some_and(|c| {
                    c.kind == crate::stubs::K_INTERFACE || c.flags & F_ABSTRACT != 0
                });
            return Some(MethodInfo {
                name: m.name.as_bytes().to_vec(),
                decl: lower(decl.name.as_bytes()),
                vis: m.visibility(),
                is_static: m.is_static(),
                is_abstract,
            });
        }
        if crate::stubs::find_class(&cur).is_some() {
            return None;
        }
        // Natively implemented classes without a stub
        if let Some((native, h)) = self.find_native_class_handler(&cur) {
            let st = (h.has_method)(&native, ml, true);
            if st || (h.has_method)(&native, ml, false) {
                return Some(MethodInfo {
                    name: ml.to_vec(),
                    decl: native,
                    vis: Visibility::Public,
                    is_static: st,
                    is_abstract: false,
                });
            }
            return None;
        }
        if self.is_known_builtin_class(&cur) && cur.as_slice() == b"closure" {
            if matches!(
                ml,
                b"__invoke" | b"call" | b"bind" | b"bindto" | b"fromcallable" | b"getcurrent"
            ) {
                return Some(MethodInfo {
                    name: ml.to_vec(),
                    decl: cur,
                    vis: Visibility::Public,
                    is_static: matches!(ml, b"bind" | b"fromcallable" | b"getcurrent"),
                    is_abstract: false,
                });
            }
        }
        None
    }

    /// A non-static method `m` of class `cl` (lowercase): its declaring
    /// class and method names, for "Non-static method ... cannot be called
    /// statically".
    pub(crate) fn callable_nonstatic_method(
        &self,
        cl: &[u8],
        m: &[u8],
    ) -> Option<(String, String)> {
        let mi = self.callable_find_method(cl, &lower(m))?;
        if mi.is_static {
            return None;
        }
        Some((
            self.callable_class_name(&mi.decl),
            String::from_utf8_lossy(&mi.name).into_owned(),
        ))
    }

    /// zend_check_method_accessible()
    fn callable_method_accessible(&self, m: &MethodInfo, ml: &[u8], scope: Option<&[u8]>) -> bool {
        match m.vis {
            Visibility::Public => true,
            Visibility::Private => scope == Some(m.decl.as_slice()),
            Visibility::Protected => {
                let Some(sc) = scope else {
                    return false;
                };
                let root = self.find_root_declaring_class_for_method(&m.decl, ml);
                self.callable_instanceof(sc, &root) || self.callable_instanceof(&root, sc)
            }
        }
    }

    /// zend_is_callable_check_class(): returns strict_class.
    #[allow(clippy::too_many_arguments)]
    fn callable_check_class_pure(
        &self,
        name: &[u8],
        scope: Option<&[u8]>,
        this: Option<&Value>,
        fcc: &mut Fcc,
        suppress: bool,
        error: &mut Option<String>,
    ) -> Option<bool> {
        let lc = lower(name);
        let called = self.called_class_stack.last().map(|c| lower(c));
        let set_this = |fcc: &mut Fcc| {
            if fcc.object.is_none() {
                fcc.object = this.cloned();
            }
        };
        match lc.as_slice() {
            b"self" => {
                let Some(sc) = scope else {
                    *error = Some("cannot access \"self\" when no class scope is active".into());
                    return None;
                };
                if !suppress {
                    fcc.deprecations
                        .push("Use of \"self\" in callables is deprecated".into());
                }
                fcc.called_scope = match called {
                    Some(c) if self.callable_instanceof(&c, sc) => Some(c),
                    _ => Some(sc.to_vec()),
                };
                fcc.calling_scope = Some(sc.to_vec());
                set_this(fcc);
                Some(false)
            }
            b"parent" => {
                let Some(sc) = scope else {
                    *error = Some("cannot access \"parent\" when no class scope is active".into());
                    return None;
                };
                let Some(p) = self
                    .classes
                    .get(sc)
                    .and_then(|c| c.parent.as_ref().map(|p| lower(p)))
                else {
                    *error = Some(
                        "cannot access \"parent\" when current class scope has no parent".into(),
                    );
                    return None;
                };
                if !suppress {
                    fcc.deprecations
                        .push("Use of \"parent\" in callables is deprecated".into());
                }
                fcc.called_scope = match called {
                    Some(c) if self.callable_instanceof(&c, &p) => Some(c),
                    _ => Some(p.clone()),
                };
                fcc.calling_scope = Some(p);
                set_this(fcc);
                Some(true)
            }
            b"static" => {
                let Some(c) = called.or_else(|| scope.map(|s| s.to_vec())) else {
                    *error = Some("cannot access \"static\" when no class scope is active".into());
                    return None;
                };
                if !suppress {
                    fcc.deprecations
                        .push("Use of \"static\" in callables is deprecated".into());
                }
                fcc.called_scope = Some(c.clone());
                fcc.calling_scope = Some(c);
                set_this(fcc);
                Some(true)
            }
            _ => {
                let cl = lc.strip_prefix(b"\\").unwrap_or(&lc).to_vec();
                if !self.callable_class_exists(&cl) {
                    *error = Some(format!(
                        "class \"{}\" not found",
                        crate::string::msg_str(name)
                    ));
                    return None;
                }
                // Resolve class aliases to the class itself.
                let cl = self
                    .classes
                    .get(&cl)
                    .map(|c| lower(&c.name))
                    .filter(|n| self.classes.contains_key(n))
                    .unwrap_or(cl);
                fcc.calling_scope = Some(cl.clone());
                match (scope, &fcc.object) {
                    (Some(sc), None) => {
                        let obj = this.filter(|o| {
                            object_class(o).is_some_and(|oc| {
                                self.callable_instanceof(&lower(&oc), sc)
                                    && self.callable_instanceof(sc, &cl)
                            })
                        });
                        match obj {
                            Some(o) => {
                                fcc.called_scope = object_class(o).map(|c| lower(&c));
                                fcc.object = Some(o.clone());
                            }
                            None => fcc.called_scope = Some(cl),
                        }
                    }
                    _ => {
                        fcc.called_scope = match &fcc.object {
                            Some(o) => object_class(o).map(|c| lower(&c)),
                            None => Some(cl),
                        };
                    }
                }
                Some(true)
            }
        }
    }

    /// zend_is_callable_check_func()
    #[allow(clippy::too_many_arguments)]
    fn callable_check_func_pure(
        &self,
        callable: &[u8],
        scope: Option<&[u8]>,
        this: Option<&Value>,
        fcc: &mut Fcc,
        mut strict_class: bool,
        suppress: bool,
        error: &mut Option<String>,
    ) -> bool {
        let ce_org = fcc.calling_scope.take();
        if ce_org.is_none() {
            let name = callable.strip_prefix(b"\\").unwrap_or(callable);
            let l = lower(name);
            if !l.contains(&b':')
                && !is_construct_name(&l)
                && l.as_slice() != crate::gc::DESTRUCTOR_FIBER_FN
                && (self.functions.contains_key(&l)
                    || self
                        .user_functions
                        .get(&l)
                        .is_some_and(|op| !crate::closure::is_closure_fn_name(&op.name)))
            {
                return true;
            }
            // (an internal closure's name; not a string that merely looks
            // like one)
            if is_closure_bytes(callable) && self.user_functions.contains_key(&l) {
                return true;
            }
        }
        // Split "Class::method" at the last "::"
        let colon = callable
            .iter()
            .rposition(|&b| b == b':')
            .filter(|&p| p > 0 && callable[p - 1] == b':')
            .map(|p| p - 1);
        let mname: Vec<u8>;
        if let Some(c) = colon {
            if c == 0 {
                *error = Some("invalid function name".into());
                return false;
            }
            let cname = &callable[..c];
            let cscope = ce_org.clone().or_else(|| scope.map(|s| s.to_vec()));
            match self.callable_check_class_pure(
                cname,
                cscope.as_deref(),
                this,
                fcc,
                suppress || ce_org.is_some(),
                error,
            ) {
                Some(s) => strict_class = s,
                None => return false,
            }
            let cs = fcc.calling_scope.clone().unwrap_or_default();
            if let Some(org) = &ce_org {
                if !self.callable_instanceof(org, &cs) {
                    *error = Some(format!(
                        "class {} is not a subclass of {}",
                        self.callable_class_name(org),
                        self.callable_class_name(&cs)
                    ));
                    return false;
                }
                if !suppress {
                    fcc.deprecations.push(format!(
                        "Callables of the form [\"{}\", \"{}\"] are deprecated",
                        self.callable_class_name(org),
                        String::from_utf8_lossy(callable)
                    ));
                }
            }
            mname = callable[c + 2..].to_vec();
        } else if let Some(org) = &ce_org {
            mname = callable.to_vec();
            fcc.calling_scope = Some(org.clone());
        } else {
            *error = Some(format!(
                "function \"{}\" not found or invalid function name",
                // (printed with %s: a NUL ends it)
                crate::string::msg_str(callable.split(|&c| c == 0).next().unwrap_or_default())
            ));
            return false;
        }

        let ml = lower(&mname);
        let cs = fcc.calling_scope.clone().unwrap_or_default();
        let has_call = self.callable_find_method(&cs, b"__call").is_some();
        let has_callstatic = self.callable_find_method(&cs, b"__callstatic").is_some();
        let mut handler: Option<MethodInfo> = None;
        let mut via_handler = false;
        let mut found = self.callable_find_method(&cs, &ml);
        // ZEND_ACC_CHANGED: a private method of the calling scope shadows
        // the method a subclass redeclared.
        if let (Some(m), Some(sc), false) = (&found, scope, strict_class) {
            if m.decl.as_slice() != sc && self.callable_instanceof(&m.decl, sc) {
                if let Some(pm) = self.classes.get(sc).and_then(|c| c.methods.get(&ml)) {
                    if pm.visibility == Visibility::Private && lower(&pm.declaring_class) == sc {
                        found = Some(MethodInfo {
                            name: pm.name.clone(),
                            decl: sc.to_vec(),
                            vis: Visibility::Private,
                            is_static: pm.is_static,
                            is_abstract: pm.is_abstract,
                        });
                    }
                }
            }
        }
        let mut retval = false;
        if let Some(m) = found {
            retval = true;
            let magic =
                (fcc.object.is_some() && has_call) || (fcc.object.is_none() && has_callstatic);
            if m.vis != Visibility::Public
                && magic
                && !self.callable_method_accessible(&m, &ml, scope)
            {
                retval = false;
            } else {
                handler = Some(m);
            }
        }
        if !retval {
            // get_function_via_handler
            if fcc.object.is_some() && fcc.calling_scope == ce_org {
                if has_call {
                    via_handler = true;
                    retval = true;
                }
            } else if fcc.calling_scope.is_some() {
                // zend_std_get_static_method() fallback
                let this_ok = this
                    .and_then(object_class)
                    .is_some_and(|oc| self.callable_instanceof(&lower(&oc), &cs));
                if (has_call && this_ok) || has_callstatic {
                    via_handler = true;
                    retval = true;
                    if fcc.object.is_none() && this_ok && has_call {
                        fcc.object = this.cloned();
                    }
                }
            }
        }
        if retval {
            if let (Some(m), false) = (&handler, via_handler) {
                let cname = self.callable_class_name(&cs);
                let fname = String::from_utf8_lossy(&m.name).into_owned();
                if m.is_abstract {
                    retval = false;
                    *error = Some(format!(
                        "cannot call abstract method {}::{}()",
                        cname, fname
                    ));
                } else if fcc.object.is_none() && !m.is_static {
                    retval = false;
                    *error = Some(format!(
                        "non-static method {}::{}() cannot be called statically",
                        cname, fname
                    ));
                }
                if retval && !self.callable_method_accessible(m, &ml, scope) {
                    let v = if m.vis == Visibility::Private {
                        "private"
                    } else {
                        "protected"
                    };
                    *error = Some(format!("cannot access {} method {}::{}()", v, cname, fname));
                    retval = false;
                }
            }
        } else if fcc.calling_scope.is_some() {
            *error = Some(format!(
                "class {} does not have a method \"{}\"",
                self.callable_class_name(&cs),
                crate::string::msg_str(&mname)
            ));
        } else {
            *error = Some(format!(
                "function {}() does not exist",
                crate::string::msg_str(&mname)
            ));
        }
        if fcc.object.is_some() && handler.as_ref().is_some_and(|m| m.is_static) {
            fcc.object = None;
        }
        if retval && via_handler {
            fcc.trampoline = Some(mname);
        }
        retval
    }

    /// zend_get_callable_name_ex()
    pub fn callable_name(&self, callable: &Value) -> Vec<u8> {
        match callable {
            Value::String(s) => s.as_bytes().to_vec(),
            Value::Array(arr) => {
                let a = arr.borrow();
                if a.len() != 2 {
                    return b"Array".to_vec();
                }
                let obj = a.get_int(0).map(|v| v.deref());
                let method = a.get_int(1).map(|v| v.deref());
                let (Some(obj), Some(Value::String(m))) = (obj, method) else {
                    return b"Array".to_vec();
                };
                let mut n = match &obj {
                    Value::String(c) => c.as_bytes().to_vec(),
                    o => match object_class(o) {
                        Some(c) => c,
                        None => return b"Array".to_vec(),
                    },
                };
                n.extend_from_slice(b"::");
                n.extend_from_slice(m.as_bytes());
                n
            }
            Value::Object(o) => {
                if o.borrow().closure.is_some() {
                    return self.closure_function_name(callable);
                }
                let mut n = o.borrow().class_name.clone();
                n.extend_from_slice(b"::__invoke");
                n
            }
            Value::Generator(_) => b"Generator::__invoke".to_vec(),
            Value::Reference(r) => {
                let v = r.borrow().clone();
                self.callable_name(&v)
            }
            Value::Null | Value::Undef => Vec::new(),
            v => v.to_php_string().as_bytes().to_vec(),
        }
    }

    /// The scope a closure over a non-public internal method was created in,
    /// when `a` is its `[$obj, 'method']` (see Vm::fcc_scoped_arrays).
    #[inline(never)]
    fn fcc_array_scope(
        &self,
        a: &std::rc::Rc<std::cell::RefCell<crate::array::PhpArray>>,
    ) -> Option<Vec<u8>> {
        self.fcc_scoped_arrays
            .iter()
            .find(|(w, _)| std::ptr::eq(w.as_ptr(), std::rc::Rc::as_ptr(a)))
            .map(|(_, s)| s.clone())
    }

    /// zend_is_callable_at_frame() without side effects: no autoloading
    /// (classes must already be loaded) and deprecations are returned in
    /// `deprecations` instead of being raised.
    pub fn is_callable_pure(&self, callable: &Value, flags: u32) -> CallableCheck {
        let callable = callable.deref();
        let mut out = CallableCheck {
            name: self.callable_name(&callable),
            ..Default::default()
        };
        let syntax_only = flags & IS_CALLABLE_CHECK_SYNTAX_ONLY != 0;
        let suppress = flags & IS_CALLABLE_SUPPRESS_DEPRECATIONS != 0;
        let scope = match &callable {
            Value::Array(a) if !self.fcc_scoped_arrays.is_empty() => {
                self.fcc_array_scope(a).or_else(|| self.callable_scope())
            }
            _ => self.callable_scope(),
        };
        // (a parameter's check runs in the callee's frame)
        let this = self.recv_this.clone().or_else(|| self.current_frame_this());
        let mut fcc = Fcc::default();
        let mut error = None;
        let ok = match &callable {
            Value::String(s) => {
                if syntax_only {
                    true
                } else {
                    self.callable_check_func_pure(
                        s.as_bytes(),
                        scope.as_deref(),
                        this.as_ref(),
                        &mut fcc,
                        false,
                        suppress,
                        &mut error,
                    )
                }
            }
            Value::Array(arr) => {
                let (len, obj, method, first) = {
                    let a = arr.borrow();
                    (
                        a.len(),
                        a.get_int(0).map(|v| v.deref()),
                        a.get_int(1).map(|v| v.deref()),
                        a.values().next().map(|v| v.deref()),
                    )
                };
                // Internal closure representation (function name + captures)
                if matches!(&first, Some(Value::String(f)) if is_closure_bytes(f.as_bytes())) {
                    true
                } else if len != 2 {
                    error = Some("array callback must have exactly two members".into());
                    false
                } else {
                    match (obj, method) {
                        (Some(obj), Some(method)) => {
                            if object_class(&obj).is_none() && !matches!(obj, Value::String(_)) {
                                error = Some(
                                    "first array member is not a valid class name or object".into(),
                                );
                                false
                            } else if let Value::String(m) = &method {
                                let m = m.as_bytes().to_vec();
                                let mut strict_class = false;
                                let ready = match &obj {
                                    Value::String(c) => {
                                        if syntax_only {
                                            None
                                        } else {
                                            match self.callable_check_class_pure(
                                                c.as_bytes(),
                                                scope.as_deref(),
                                                this.as_ref(),
                                                &mut fcc,
                                                suppress,
                                                &mut error,
                                            ) {
                                                Some(s) => {
                                                    strict_class = s;
                                                    Some(true)
                                                }
                                                None => Some(false),
                                            }
                                        }
                                    }
                                    o => {
                                        let oc = object_class(o).unwrap_or_default();
                                        fcc.calling_scope = Some(lower(&oc));
                                        fcc.object = Some(o.clone());
                                        if syntax_only { None } else { Some(true) }
                                    }
                                };
                                match ready {
                                    None => true,
                                    Some(false) => false,
                                    Some(true) => {
                                        if obj.is_closure_obj()
                                            && m.eq_ignore_ascii_case(b"__invoke")
                                        {
                                            true
                                        } else {
                                            self.callable_check_func_pure(
                                                &m,
                                                scope.as_deref(),
                                                this.as_ref(),
                                                &mut fcc,
                                                strict_class,
                                                suppress,
                                                &mut error,
                                            )
                                        }
                                    }
                                }
                            } else {
                                error = Some("second array member is not a valid method".into());
                                false
                            }
                        }
                        _ => {
                            error = Some("array callback has to contain indices 0 and 1".into());
                            false
                        }
                    }
                }
            }
            Value::Object(o) => {
                if o.borrow().closure.is_some() {
                    true
                } else {
                    let cl = lower(&o.borrow().class_name);
                    if self.callable_find_method(&cl, b"__invoke").is_some() {
                        fcc.calling_scope = Some(cl);
                        fcc.object = Some(callable.clone());
                        true
                    } else {
                        error = Some("no array or string given".into());
                        false
                    }
                }
            }
            _ => {
                error = Some("no array or string given".into());
                false
            }
        };
        out.ok = ok;
        out.error = if ok { None } else { error };
        out.calling_scope = fcc.calling_scope;
        out.has_object = fcc.object.is_some();
        out.deprecations = fcc.deprecations;
        out.object = fcc.object;
        out.trampoline = fcc.trampoline;
        out
    }

    /// Class names a callable check would look up (to autoload them first).
    fn callable_classes_to_load(callable: &Value) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let add_from_method = |s: &[u8], out: &mut Vec<Vec<u8>>| {
            if let Some(p) = s
                .iter()
                .rposition(|&b| b == b':')
                .filter(|&p| p > 1 && s[p - 1] == b':')
            {
                out.push(s[..p - 1].to_vec());
            }
        };
        match callable {
            Value::String(s) => add_from_method(s.as_bytes(), &mut out),
            Value::Array(arr) => {
                let a = arr.borrow();
                // (only a [class, method] pair names a class: an internal
                // closure callable ["__closure_N", $captured] does not)
                if a.len() == 2 {
                    if let Some(Value::String(m)) = a.get_int(1).map(|v| v.deref()) {
                        if let Some(Value::String(c)) = a.get_int(0).map(|v| v.deref()) {
                            out.push(c.as_bytes().to_vec());
                        }
                        add_from_method(m.as_bytes(), &mut out);
                    }
                }
            }
            _ => {}
        }
        out.retain(|c| {
            !matches!(lower(c).as_slice(), b"self" | b"parent" | b"static" | b"")
                && !crate::closure::is_closure_fn_name(c)
                && !c.starts_with(b"__closure_fcc_")
        });
        out
    }

    /// Autoloads the not yet loaded classes a callable names; true when one
    /// got loaded.
    pub(crate) fn autoload_callable_classes(&mut self, callable: &Value) -> bool {
        let mut loaded = false;
        for c in Self::callable_classes_to_load(callable) {
            let name = c.strip_prefix(b"\\").unwrap_or(&c).to_vec();
            if !self.callable_class_exists(&lower(&name)) {
                self.try_autoload_class(&name);
                if self.current_exception.is_some() {
                    return false;
                }
                loaded |= self.callable_class_exists(&lower(&name));
            }
        }
        loaded
    }

    /// zend_is_callable_ex(): autoloads the classes the callable names and
    /// raises the deprecations (unless IS_CALLABLE_SUPPRESS_DEPRECATIONS).
    /// When an autoloader throws, the check fails with the exception left
    /// pending in `current_exception`.
    pub fn zend_is_callable_ex(&mut self, callable: &Value, flags: u32) -> CallableCheck {
        let callable = callable.deref();
        if flags & IS_CALLABLE_CHECK_SYNTAX_ONLY == 0 {
            for c in Self::callable_classes_to_load(&callable) {
                let name = c.strip_prefix(b"\\").unwrap_or(&c).to_vec();
                if !self.callable_class_exists(&lower(&name)) {
                    self.try_autoload_class(&name);
                    if self.current_exception.is_some() {
                        return CallableCheck {
                            name: self.callable_name(&callable),
                            ..Default::default()
                        };
                    }
                }
            }
        }
        if flags & IS_CALLABLE_CHECK_SYNTAX_ONLY == 0 {
            if let Some(msg) = Self::incomplete_method_call_msg(&callable) {
                // incomplete_class_get_method() throws.
                let exc = self.create_exception(b"Error", &msg, self.current_line);
                self.current_exception = Some(exc);
                let mut r = self.is_callable_pure(&callable, flags);
                r.ok = false;
                return r;
            }
        }
        let mut r = self.is_callable_pure(&callable, flags);
        let line = self.current_line;
        let deprecations = std::mem::take(&mut r.deprecations);
        if !deprecations.is_empty() && !std::mem::take(&mut self.mute_callable_deprecations) {
            for d in deprecations {
                self.emit_deprecated_at(&d, line);
            }
        }
        r
    }

    /// The class an __PHP_Incomplete_Class object of a `[$obj, "method"]`
    /// callable stands for.
    #[inline(never)]
    /// incomplete_class_get_method()'s Error for a `[$incomplete, "method"]`
    /// callable.
    pub fn incomplete_method_call_msg(callable: &Value) -> Option<String> {
        let name = Self::incomplete_callable_class(callable)?;
        Some(format!(
            "The script tried to call a method on an incomplete object. Please ensure that the class definition \"{}\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition",
            crate::string::msg_str(&name)
        ))
    }

    pub fn incomplete_callable_class(callable: &Value) -> Option<Vec<u8>> {
        let Value::Array(arr) = callable else {
            return None;
        };
        let a = arr.borrow();
        if a.len() != 2 {
            return None;
        }
        let Some(Value::Object(o)) = a.get_int(0).map(|v| v.deref()) else {
            return None;
        };
        if !matches!(a.get_int(1).map(|v| v.deref()), Some(Value::String(_))) {
            return None;
        }
        let o = o.borrow();
        if !o.class_name.eq_ignore_ascii_case(b"__PHP_Incomplete_Class") {
            return None;
        }
        Some(match o.get_property(b"__PHP_Incomplete_Class_Name") {
            Value::String(s) => s.as_bytes().to_vec(),
            _ => b"unknown".to_vec(),
        })
    }

    /// Raise the deprecations queued by `callable` type checks.
    pub fn flush_callable_deprecations(&mut self) {
        if self.pending_callable_deprecations.borrow().is_empty() {
            return;
        }
        let q = std::mem::take(&mut *self.pending_callable_deprecations.borrow_mut());
        let line = self.current_line;
        for d in q {
            self.emit_deprecated_at(&d, line);
        }
    }

    /// Raise the deprecations queued by the `callable` parameter type
    /// checks of a call of `callee`: at its RECV, in its file, on its
    /// declaration's line.
    #[inline(never)]
    pub fn flush_callable_deprecations_recv(&mut self, callee: &crate::opcode::OpArray) {
        if self.pending_callable_deprecations.borrow().is_empty() {
            return;
        }
        if callee.decl_line == 0 {
            return self.flush_callable_deprecations();
        }
        let q = std::mem::take(&mut *self.pending_callable_deprecations.borrow_mut());
        let saved = self.enter_recv_file(callee);
        for d in q {
            self.emit_deprecated_at(&d, callee.decl_line);
        }
        if let Some(f) = saved {
            self.current_file = f;
        }
    }

    /// The error of zend_is_callable_ex() for `callable` (None when it is
    /// callable), raising its deprecations.
    pub fn callable_error(&mut self, callable: &Value) -> Option<String> {
        let r = self.zend_is_callable_ex(callable, 0);
        if r.ok {
            None
        } else {
            Some(r.error.unwrap_or_default())
        }
    }
}
