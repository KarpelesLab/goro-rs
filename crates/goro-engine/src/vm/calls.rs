//! Calls: callee resolution, argument passing, frame activation and
//! return, callables, closures and static variables.

use crate::class::{DisplayName, TypeName};
use std::cell::RefCell;
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use super::types::Check;
use super::{Frame, Pending, R, Slot, StackMark, Unwind, Vm, frame_flags, pending_flags};
use crate::bytecode::*;
use crate::class::{Class, mflags};
use crate::func::{Args, Func, FuncKind, NativeSig};

/// Frame flag: the return value goes to a variable (assign semantics).
pub const RET_VAR: u32 = 2048;

/// A resolved callee.
#[derive(Clone)]
pub struct Callee {
    pub func: Rc<Func>,
    pub this: Option<ObjRef>,
    pub static_class: *const Class,
    pub scope: *const Class,
    pub closure: Option<ObjRef>,
    /// `__call` / `__callStatic`: the method name asked for.
    pub magic_name: Option<PhpStr>,
}

impl Callee {
    pub fn func(func: Rc<Func>) -> Callee {
        let scope = func.scope.get();
        Callee {
            func,
            this: None,
            static_class: scope,
            scope,
            closure: None,
            magic_name: None,
        }
    }
}

/// Native data of `Closure` objects.
pub struct ClosureData {
    pub func: Rc<Func>,
    pub this: Option<ObjRef>,
    pub scope: *const Class,
    pub called_scope: *const Class,
    /// Values of `use` variables (references for `&$x`), by `code.uses`
    /// order; `None` for an undefined variable of an arrow function.
    pub captured: Vec<Option<Value>>,
    pub statics: RefCell<Vec<Option<PhpRef>>>,
    /// Created from a callable (`Closure::fromCallable`, `f(...)`): the
    /// magic method name for `__call` trampolines.
    pub magic_name: Option<PhpStr>,
    pub from_callable: bool,
}

impl goro_runtime::NativeData for ClosureData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(t) = &self.this {
            visit(&Value::Object(t.clone()));
        }
        for v in self.captured.iter().flatten() {
            visit(v);
        }
        for r in self.statics.borrow().iter().flatten() {
            visit(&Value::Ref(r.clone()));
        }
    }
    fn clear(&mut self) {
        self.this = None;
        self.captured.clear();
        self.statics.borrow_mut().clear();
    }
}

impl Vm {
    // ------------------------------------------------------------ pending calls

    pub(crate) fn push_callee(&mut self, c: Callee, flags: u32) {
        let (base, mark, args) = match &c.func.kind {
            FuncKind::User(code) if c.magic_name.is_none() => {
                let (b, m) = self.ex.stack.alloc(code.num_slots as usize);
                (b, m, Vec::new())
            }
            _ => (
                std::ptr::null_mut(),
                StackMark::default(),
                self.take_args_vec(),
            ),
        };
        let statics: *const RefCell<Vec<Option<PhpRef>>> = match &c.closure {
            Some(cl) => match cl.obj().native.borrow().as_ref() {
                Some(n) => {
                    let any: &dyn std::any::Any = &**n;
                    match any.downcast_ref::<ClosureData>() {
                        Some(cd) => &cd.statics as *const _,
                        None => &c.func.statics as *const _,
                    }
                }
                None => &c.func.statics as *const _,
            },
            None => &c.func.statics as *const _,
        };
        let mut fl = flags;
        if c.magic_name.is_some() {
            fl |= pending_flags::MAGIC_CALL;
        }
        self.ex.pending.push(Pending {
            func: c.func,
            base,
            mark,
            args,
            argc: 0,
            extra: Vec::new(),
            named_extra: None,
            this: c.this,
            static_class: c.static_class,
            scope: c.scope,
            closure: c.closure,
            flags: fl,
            new_obj: None,
            magic_name: c.magic_name,
            statics,
            has_named: false,
            passed: 0,
        });
    }

    /// Drops a pending call that won't be made.
    pub(crate) fn discard_pending(&mut self, p: Pending) {
        if !p.base.is_null()
            && let FuncKind::User(code) = &p.func.kind
        {
            for i in 0..code.num_slots as usize {
                let old = unsafe { (*p.base.add(i)).take() };
                drop(old);
            }
            self.ex.stack.release(p.mark);
        }
        let Pending { args, .. } = p;
        self.give_args_vec(args);
    }

    #[inline]
    fn pending(&mut self) -> &mut Pending {
        self.ex.pending.last_mut().expect("no pending call")
    }

    /// Parameters of a function (parsing a native signature once).
    pub fn params_of<'f>(&mut self, f: &'f Func) -> &'f [ParamInfo] {
        match &f.kind {
            FuncKind::User(c) => &c.params,
            FuncKind::Native { .. } => {
                let sig = self.native_sig(f);
                // SAFETY: the signature is kept by the function's OnceCell.
                unsafe { &*(&sig.params[..] as *const [ParamInfo]) }
            }
        }
    }

    pub fn native_sig(&mut self, f: &Func) -> Rc<NativeSig> {
        let FuncKind::Native { sig, info, .. } = &f.kind else {
            panic!("not native");
        };
        if let Some(i) = info.get() {
            return i.clone();
        }
        let consts = &self.constants;
        let kc = |n: &[u8]| consts.get(&PhpStr::new(n)).cloned();
        let parsed = crate::compiler::parse_native_sig(sig, Some(&kc))
            .unwrap_or_else(|e| panic!("bad native signature: {}", e));
        let rc = Rc::new(parsed);
        let _ = info.set(rc.clone());
        rc
    }

    fn param_info(&mut self, f: &Func, i: usize) -> Option<(bool, bool)> {
        let ps = self.params_of(f);
        match ps.get(i) {
            Some(p) => Some((p.by_ref, p.variadic)),
            None => ps.last().filter(|p| p.variadic).map(|p| (p.by_ref, true)),
        }
    }

    pub(crate) fn pending_arg_by_ref(&mut self, arg: u32) -> bool {
        let p = self.ex.pending.last().unwrap();
        if p.magic_name.is_some() {
            return false;
        }
        let f = p.func.clone();
        matches!(self.param_info(&f, arg as usize), Some((true, _)))
    }

    pub(crate) fn pending_named_by_ref(&mut self, name: &Value) -> bool {
        let p = self.ex.pending.last().unwrap();
        if p.magic_name.is_some() {
            return false;
        }
        let f = p.func.clone();
        let Value::Str(n) = name else { return false };
        let ps = self.params_of(&f);
        ps.iter().any(|p| p.name == *n && p.by_ref)
    }

    // ------------------------------------------------------------ init

    pub(crate) fn op_init_fcall(
        &mut self,
        name: u32,
        nargs: u32,
        cache: u32,
        fallback: bool,
    ) -> R<()> {
        let _ = nargs;
        let code = self.code_ref();
        let c = code.caches[cache as usize].get();
        let func: Rc<Func> = if c.key != 0 {
            let p = c.a as *const Func;
            unsafe {
                Rc::increment_strong_count(p);
                Rc::from_raw(p)
            }
        } else {
            let lname = code.consts[name as usize + 1].clone();
            let found = match lname.as_str().and_then(|s| self.functions.get(s)) {
                Some(f) => Some(f.clone()),
                None if fallback => {
                    let l2 = code.consts[name as usize + 3].clone();
                    l2.as_str().and_then(|s| self.functions.get(s)).cloned()
                }
                None => None,
            };
            let found = match found {
                None if code.file.is_empty() => lname
                    .as_str()
                    .and_then(|s| self.disabled_functions.get(s))
                    .cloned(),
                f => f,
            };
            match found {
                Some(f) => {
                    // Functions are never removed: cache the pointer.
                    code.caches[cache as usize].set(Cache {
                        key: 1,
                        a: Rc::as_ptr(&f) as usize,
                        b: 0,
                    });
                    f
                }
                None => {
                    let n = if fallback {
                        code.consts[name as usize + 2].clone()
                    } else {
                        code.consts[name as usize].clone()
                    };
                    let n = n.as_str().map(|s| s.to_string_lossy()).unwrap_or_default();
                    // Namespaced: PHP reports the qualified name.
                    let shown = if fallback {
                        code.consts[name as usize]
                            .as_str()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or(n)
                    } else {
                        n
                    };
                    return self.throw("Error", &format!("Call to undefined function {}()", shown));
                }
            }
        };
        self.push_callee(Callee::func(func), 0);
        Ok(())
    }

    /// Prepares a call of a callable value (`$f()`, `call_user_func`...).
    pub(crate) fn init_callable_call(&mut self, v: Value, nargs: u32) -> R<()> {
        let _ = nargs;
        if matches!(v, Value::Str(_) | Value::Array(_)) {
            self.check_dynamic_callable(&v)?;
        }
        let c = self.resolve_callable(&v, true)?;
        if c.func.is_native() && is_scope_introspecting(&c.func.lname) {
            return self.throw(
                "Error",
                &format!("Cannot call {}() dynamically", c.func.name.dname()),
            );
        }
        self.push_callee(c, 0);
        Ok(())
    }

    /// Resolves a callable value. With `for_call`, errors are thrown as
    /// PHP does for `$f()`.
    pub fn resolve_callable(&mut self, v: &Value, for_call: bool) -> R<Callee> {
        match v {
            Value::Ref(r) => {
                let v = r.get();
                self.resolve_callable(&v, for_call)
            }
            Value::Str(s) => {
                if let Some(pos) = find_double_colon(s) {
                    let cls = &s[..pos];
                    let m = &s[pos + 2..];
                    let c = self.class_for_callable(cls)?;
                    return self.resolve_static_method(&c, m, for_call, true);
                }
                let mut name = s.as_bytes();
                if name.first() == Some(&b'\\') {
                    name = &name[1..];
                }
                match self.functions.get(&crate::class::lower(name)).cloned() {
                    Some(f) => Ok(Callee::func(f)),
                    None => self.throw(
                        "Error",
                        &format!(
                            "Call to undefined function {}()",
                            String::from_utf8_lossy(name)
                        ),
                    ),
                }
            }
            Value::Array(a) => {
                if a.len() != 2 {
                    return self.throw("Error", "Array callback must have exactly two elements");
                }
                let (Some(target), Some(method)) = (a.get_int(0), a.get_int(1)) else {
                    return self.throw("Error", "Array callback must have exactly two elements");
                };
                let target = target.deref();
                let method = method.deref();
                let Value::Str(m) = method else {
                    return self.throw("Error", "Method name must be a string");
                };
                match target {
                    Value::Object(o) => {
                        let m = m.as_bytes();
                        let cls = self.class_of(&o);
                        // "parent::method", "A::method"
                        match self.qualified_method(&cls, m) {
                            Some(Ok((p, m))) => {
                                let mut c = self.resolve_method(&o, &p, m, for_call)?;
                                c.static_class = Rc::as_ptr(&cls);
                                Ok(c)
                            }
                            Some(Err(e)) => self.throw("Error", &e),
                            None => self.resolve_method(&o, &cls, m, for_call),
                        }
                    }
                    Value::Str(cn) => {
                        let c = self.class_for_callable(&cn)?;
                        let m = m.as_bytes();
                        match self.qualified_method(&c, m) {
                            Some(Ok((p, m))) => self.resolve_static_method(&p, m, for_call, true),
                            Some(Err(e)) => self.throw("Error", &e),
                            None => self.resolve_static_method(&c, m, for_call, true),
                        }
                    }
                    _ => self.throw("Error", "Array callback must have exactly two elements"),
                }
            }
            Value::Object(o) => {
                let cls = self.class_of(o);
                if let Some(cd) = self.closure_data(o) {
                    return Ok(cd);
                }
                let inv = cls.magic.borrow().invoke.clone();
                match inv {
                    Some(f) => Ok(Callee {
                        scope: f.scope.get(),
                        static_class: Rc::as_ptr(&cls),
                        func: f,
                        this: Some(o.clone()),
                        closure: None,
                        magic_name: None,
                    }),
                    None => self.throw(
                        "Error",
                        "Object of type {} is not callable"
                            .replace("{}", &cls.name.dname())
                            .as_str(),
                    ),
                }
            }
            _ => self.throw("Error", "Value not callable"),
        }
    }

    /// The PHP 8.2 deprecations of scope-relative callables, raised where
    /// a callable is checked (parameters of internal functions,
    /// is_callable()); observed on PHP 8.5: 'Use of "self" in callables is
    /// deprecated', 'Callables of the form ["B", "parent::g"] are
    /// deprecated'. Reported when the class part resolves, whether or not
    /// the method exists.
    pub fn callable_deprecation(&mut self, v: &Value) -> R<()> {
        fn rel(c: &[u8]) -> Option<&'static str> {
            let l = c.to_ascii_lowercase();
            match &l[..] {
                b"self" => Some("self"),
                b"parent" => Some("parent"),
                b"static" => Some("static"),
                _ => None,
            }
        }
        let msg = match v {
            Value::Str(s) => match find_double_colon(s).and_then(|p| rel(&s[..p])) {
                Some(r) if self.relative_class_resolves(r) => {
                    format!("Use of \"{}\" in callables is deprecated", r)
                }
                _ => return Ok(()),
            },
            Value::Array(a) if a.len() == 2 => {
                let (Some(t), Some(m)) = (a.get_int(0), a.get_int(1)) else {
                    return Ok(());
                };
                let (t, m) = (t.deref(), m.deref());
                let Value::Str(m) = m else { return Ok(()) };
                let cname = match &t {
                    Value::Str(c) => {
                        if let Some(r) = rel(c) {
                            if self.relative_class_resolves(r) {
                                self.deprecated(&format!(
                                    "Use of \"{}\" in callables is deprecated",
                                    r
                                ))?;
                            }
                            return Ok(());
                        }
                        if self.find_class(c, true)?.is_none() {
                            return Ok(());
                        }
                        c.to_string_lossy()
                    }
                    Value::Object(o) => self.class_of(o).name.to_string_lossy(),
                    _ => return Ok(()),
                };
                if find_double_colon(&m).is_none() {
                    return Ok(());
                }
                format!(
                    "Callables of the form [\"{}\", \"{}\"] are deprecated",
                    cname,
                    m.to_string_lossy()
                )
            }
            _ => return Ok(()),
        };
        self.deprecated(&msg)
    }

    fn relative_class_resolves(&self, r: &str) -> bool {
        let p = if r == "static" {
            self.static_class_ptr()
        } else {
            self.scope_ptr()
        };
        !p.is_null() && (r != "parent" || unsafe { (*p).parent.is_some() })
    }

    /// A class-qualified method in an array callable (`[$obj, "A::m"]`,
    /// `["B", "parent::m"]`): the class the method is looked up in, which
    /// must be `base` or one of its ancestors (observed on PHP 8.5: "class P
    /// is not a subclass of B"). `None` when the name is not qualified.
    pub(crate) fn qualified_method<'m>(
        &mut self,
        base: &Rc<Class>,
        m: &'m [u8],
    ) -> Option<Result<(Rc<Class>, &'m [u8]), String>> {
        let p = find_double_colon(m)?;
        let (cn, meth) = (&m[..p], &m[p + 2..]);
        let l = crate::class::lower(cn);
        let target = match l.as_bytes() {
            b"parent" => match base.parent.clone() {
                Some(c) => c,
                None => return Some(Err("class has no parent".to_string())),
            },
            b"self" | b"static" => base.clone(),
            _ => {
                let saved = self.err.deferred.take();
                let r = self.find_class(cn, true).ok().flatten();
                self.err.deferred = saved;
                match r {
                    Some(c) => c,
                    None => {
                        return Some(Err(format!(
                            "class \"{}\" not found",
                            String::from_utf8_lossy(cn)
                        )));
                    }
                }
            }
        };
        if !base.is_subclass_of(&target) {
            return Some(Err(format!(
                "class {} is not a subclass of {}",
                base.name.to_string_lossy(),
                target.name.to_string_lossy()
            )));
        }
        Some(Ok((target, meth)))
    }

    /// `$f()` takes class names literally: no self/parent/static and no
    /// "parent::" methods (observed on PHP 8.5: Class "self" not found).
    fn check_dynamic_callable(&mut self, v: &Value) -> R<()> {
        let is_rel = |c: &[u8]| {
            let l = c.to_ascii_lowercase();
            matches!(&l[..], b"self" | b"parent" | b"static")
        };
        match v {
            Value::Str(s) => {
                // The class part ends at the last "::" (observed on PHP 8.5:
                // Class "B::parent" not found for "B::parent::f").
                if let Some(p) = s.windows(2).rposition(|w| w == b"::")
                    && (is_rel(&s[..p]) || find_double_colon(&s[..p]).is_some())
                {
                    return self.throw(
                        "Error",
                        &format!("Class \"{}\" not found", String::from_utf8_lossy(&s[..p])),
                    );
                }
            }
            Value::Array(a) if a.len() == 2 => {
                let (Some(t), Some(m)) = (a.get_int(0), a.get_int(1)) else {
                    return Ok(());
                };
                let (t, m) = (t.deref(), m.deref());
                let cls = match &t {
                    Value::Str(c) if is_rel(c) => {
                        return self.throw(
                            "Error",
                            &format!("Class \"{}\" not found", c.to_string_lossy()),
                        );
                    }
                    Value::Str(c) => match self.find_class(c, true)? {
                        Some(c) => c,
                        None => return Ok(()),
                    },
                    Value::Object(o) => self.class_of(o),
                    _ => return Ok(()),
                };
                if let Value::Str(m) = m
                    && find_double_colon(&m).is_some()
                {
                    return self.throw(
                        "Error",
                        &format!(
                            "Call to undefined method {}::{}()",
                            cls.name.dname(),
                            m.to_string_lossy()
                        ),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn class_for_callable(&mut self, name: &[u8]) -> R<Rc<Class>> {
        let l = crate::class::lower(name);
        match l.as_bytes() {
            b"self" | b"static" | b"parent" => {
                let p = if l.as_bytes() == b"static" {
                    self.static_class_ptr()
                } else {
                    self.scope_ptr()
                };
                if p.is_null() {
                    return self.throw(
                        "Error",
                        &format!(
                            "Cannot use \"{}\" when no class scope is active",
                            l.to_string_lossy()
                        ),
                    );
                }
                let c = self.rc_class(p);
                if l.as_bytes() == b"parent" {
                    return match c.parent.clone() {
                        Some(p) => Ok(p),
                        None => self.throw(
                            "Error",
                            "Cannot use \"parent\" when current class scope has no parent",
                        ),
                    };
                }
                Ok(c)
            }
            _ => match self.find_class(name, true)? {
                Some(c) => Ok(c),
                None => self.throw(
                    "Error",
                    &format!("Class \"{}\" not found", String::from_utf8_lossy(name)),
                ),
            },
        }
    }

    /// The closure data as a callee.
    pub fn closure_data(&self, o: &ObjRef) -> Option<Callee> {
        let n = o.obj().native.borrow();
        let any: &dyn std::any::Any = &**n.as_ref()?;
        let cd = any.downcast_ref::<ClosureData>()?;
        Some(Callee {
            func: cd.func.clone(),
            this: cd.this.clone(),
            static_class: cd.called_scope,
            scope: cd.scope,
            closure: Some(o.clone()),
            magic_name: cd.magic_name.clone(),
        })
    }

    /// Finds an instance method for a call from the current scope.
    pub fn resolve_method(
        &mut self,
        o: &ObjRef,
        cls: &Rc<Class>,
        name: &[u8],
        for_call: bool,
    ) -> R<Callee> {
        let lname = crate::class::lower(name);
        let scope = self.scope_ptr();
        // A private method of the calling scope takes precedence.
        if !scope.is_null() {
            let sc = unsafe { &*scope };
            if !std::ptr::eq(sc, &**cls)
                && cls.is_subclass_of(sc)
                && let Some(m) = sc.find_method_str(&lname)
                && m.flags & mflags::PRIVATE != 0
                && std::ptr::eq(m.scope.get(), sc)
            {
                return Ok(self.method_callee(m, Some(o.clone()), cls));
            }
        }
        if let Some(m) = cls.find_method_str(&lname) {
            if self.method_visible(&m, scope) {
                if m.flags & mflags::ABSTRACT != 0 && for_call {
                    return self.throw(
                        "Error",
                        &format!("Cannot call abstract method {}()", m.display_name()),
                    );
                }
                return Ok(self.method_callee(m, Some(o.clone()), cls));
            }
            // Not visible: __call may take it.
            let call = cls.magic.borrow().call.clone();
            if let Some(c) = call {
                return Ok(Callee {
                    scope: c.scope.get(),
                    func: c,
                    this: Some(o.clone()),
                    static_class: Rc::as_ptr(cls),
                    closure: None,
                    magic_name: Some(PhpStr::new(name)),
                });
            }
            let vis = if m.flags & mflags::PRIVATE != 0 {
                "private"
            } else {
                "protected"
            };
            let from = self.scope_desc(scope);
            return self.throw(
                "Error",
                &format!(
                    "Call to {} method {}::{}() from {}",
                    vis,
                    cls.name.dname(),
                    m.name.dname(),
                    from
                ),
            );
        }
        let call = cls.magic.borrow().call.clone();
        if let Some(c) = call {
            return Ok(Callee {
                scope: c.scope.get(),
                func: c,
                this: Some(o.clone()),
                static_class: Rc::as_ptr(cls),
                closure: None,
                magic_name: Some(PhpStr::new(name)),
            });
        }
        self.throw(
            "Error",
            &format!(
                "Call to undefined method {}::{}()",
                cls.name.dname(),
                String::from_utf8_lossy(name)
            ),
        )
    }

    fn method_callee(&self, m: Rc<Func>, this: Option<ObjRef>, cls: &Rc<Class>) -> Callee {
        let is_static = m.flags & mflags::STATIC != 0;
        Callee {
            scope: m.scope.get(),
            func: m,
            this: if is_static { None } else { this },
            static_class: Rc::as_ptr(cls),
            closure: None,
            magic_name: None,
        }
    }

    pub(crate) fn scope_desc(&self, scope: *const Class) -> String {
        if scope.is_null() {
            "global scope".to_string()
        } else {
            format!("scope {}", unsafe { (*scope).name.dname() })
        }
    }

    /// Whether method `m` may be called from `scope`.
    pub fn method_visible(&self, m: &Func, scope: *const Class) -> bool {
        if m.flags & mflags::PUBLIC != 0 {
            return true;
        }
        if scope.is_null() {
            return false;
        }
        let decl = m.scope.get();
        if m.flags & mflags::PRIVATE != 0 {
            return std::ptr::eq(decl, scope);
        }
        // protected: related classes (the root declaring the method).
        let sc = unsafe { &*scope };
        let dc = unsafe { &*decl };
        if sc.is_subclass_of(dc) || dc.is_subclass_of(sc) {
            return true;
        }
        // Prototype in a common ancestor.
        let mut proto = m.proto.borrow().clone();
        while let Some(p) = proto {
            let pc = unsafe { &*p.scope.get() };
            if sc.is_subclass_of(pc) {
                return true;
            }
            proto = p.proto.borrow().clone();
        }
        false
    }

    /// `A::m()`, `parent::m()`, callable strings `"A::m"`.
    pub fn resolve_static_method(
        &mut self,
        cls: &Rc<Class>,
        name: &[u8],
        for_call: bool,
        forwarding: bool,
    ) -> R<Callee> {
        let lname = crate::class::lower(name);
        let scope = self.scope_ptr();
        let this = self.this_obj();
        let found = cls.find_method_str(&lname);
        if let Some(m) = found {
            if !self.method_visible(&m, scope) {
                let vis = if m.flags & mflags::PRIVATE != 0 {
                    "private"
                } else {
                    "protected"
                };
                let from = self.scope_desc(scope);
                // __callStatic may take an invisible method.
                if let Some(cs) = cls.magic.borrow().call_static.clone() {
                    return Ok(Callee {
                        scope: cs.scope.get(),
                        func: cs,
                        this: None,
                        static_class: Rc::as_ptr(cls),
                        closure: None,
                        magic_name: Some(PhpStr::new(name)),
                    });
                }
                return self.throw(
                    "Error",
                    &format!(
                        "Call to {} method {}::{}() from {}",
                        vis,
                        cls.name.dname(),
                        m.name.dname(),
                        from
                    ),
                );
            }
            if m.flags & mflags::ABSTRACT != 0 && for_call {
                return self.throw(
                    "Error",
                    &format!("Cannot call abstract method {}()", m.display_name()),
                );
            }
            if m.flags & mflags::STATIC != 0 {
                // Forwarding calls keep the late static binding class.
                let cur = self.static_class_ptr();
                let sc = if forwarding && !cur.is_null() && unsafe { &*cur }.is_subclass_of(cls) {
                    cur
                } else {
                    Rc::as_ptr(cls)
                };
                return Ok(Callee {
                    scope: m.scope.get(),
                    func: m,
                    this: None,
                    static_class: sc,
                    closure: None,
                    magic_name: None,
                });
            }
            // Non-static: needs a compatible $this.
            if let Some(t) = this {
                let tc = self.class_of(&t);
                if tc.is_subclass_of(cls) {
                    return Ok(Callee {
                        scope: m.scope.get(),
                        func: m,
                        this: Some(t),
                        static_class: Rc::as_ptr(&tc),
                        closure: None,
                        magic_name: None,
                    });
                }
            }
            return self.throw(
                "Error",
                &format!(
                    "Non-static method {}::{}() cannot be called statically",
                    // The declaring class (test ext/reflection/tests/
                    // ReflectionObject_getName_error1.phpt).
                    m.scope()
                        .map(|c| c.name.dname())
                        .unwrap_or_else(|| cls.name.dname()),
                    m.name.dname()
                ),
            );
        }
        // __call in object context, else __callStatic.
        if let Some(t) = this.clone() {
            let tc = self.class_of(&t);
            if tc.is_subclass_of(cls)
                && let Some(c) = cls.magic.borrow().call.clone()
            {
                return Ok(Callee {
                    scope: c.scope.get(),
                    func: c,
                    this: Some(t),
                    static_class: Rc::as_ptr(&tc),
                    closure: None,
                    magic_name: Some(PhpStr::new(name)),
                });
            }
        }
        if let Some(cs) = cls.magic.borrow().call_static.clone() {
            return Ok(Callee {
                scope: cs.scope.get(),
                func: cs,
                this: None,
                static_class: Rc::as_ptr(cls),
                closure: None,
                magic_name: Some(PhpStr::new(name)),
            });
        }
        self.throw(
            "Error",
            &format!(
                "Call to undefined method {}::{}()",
                cls.name.dname(),
                String::from_utf8_lossy(name)
            ),
        )
    }

    pub(crate) fn op_init_method(&mut self, obj: Reg, name: Reg, nargs: u32, cache: u32) -> R<()> {
        let _ = nargs;
        let o = if obj == THIS {
            match self.this_obj() {
                Some(o) => o,
                None => return self.throw("Error", "Using $this when not in object context"),
            }
        } else {
            match self.peek(obj) {
                Some(Value::Object(o)) => {
                    let o = o.clone();
                    self.drop_tmp(obj);
                    o
                }
                _ => {
                    let v = self.get(obj)?;
                    match v {
                        Value::Object(o) => o,
                        other => {
                            let n = self.get(name)?;
                            let n = match n {
                                Value::Str(s) => s.to_string_lossy(),
                                _ => return self.throw("Error", "Method name must be a string"),
                            };
                            return self.throw(
                                "Error",
                                &format!("Call to a member function {}() on {}", n, other.tname()),
                            );
                        }
                    }
                }
            }
        };
        // Inline cache: same class, constant name.
        let cls_ptr = self.class_ptr_of(&o);
        if is_const(name) {
            let c = self.code_ref().caches[cache as usize].get();
            if c.key == cls_ptr as usize && c.key != 0 {
                let fp = c.a as *const Func;
                let f = unsafe {
                    Rc::increment_strong_count(fp);
                    Rc::from_raw(fp)
                };
                let is_static = f.flags & mflags::STATIC != 0;
                let cb = Callee {
                    scope: f.scope.get(),
                    func: f,
                    this: if is_static { None } else { Some(o) },
                    static_class: cls_ptr,
                    closure: None,
                    magic_name: None,
                };
                self.push_callee(cb, 0);
                return Ok(());
            }
        }
        let n = self.get(name)?;
        let n = match n {
            Value::Str(s) => s,
            _ => return self.throw("Error", "Method name must be a string"),
        };
        let cls = self.class_of(&o);
        let cb = self.resolve_method(&o, &cls, &n, true)?;
        if is_const(name) && cb.magic_name.is_none() && cb.closure.is_none() {
            self.code_ref().caches[cache as usize].set(Cache {
                key: cls_ptr as usize,
                a: Rc::as_ptr(&cb.func) as usize,
                b: 0,
            });
        }
        self.push_callee(cb, 0);
        Ok(())
    }

    pub(crate) fn class_ptr_of(&self, o: &ObjRef) -> *const Class {
        let c: &Rc<dyn goro_runtime::ClassInfo> = o.class();
        Rc::as_ptr(c) as *const Class
    }

    pub(crate) fn op_init_static_call(
        &mut self,
        class: ClassSpec,
        name: Reg,
        nargs: u32,
        cache: u32,
    ) -> R<()> {
        let _ = (nargs, cache);
        let forwarding = matches!(
            class,
            ClassSpec::SelfRef | ClassSpec::Parent | ClassSpec::Static
        );
        let cls = self.resolve_class_spec(class, true)?;
        let n = self.get(name)?;
        let n = match n {
            Value::Str(s) => s,
            _ => return self.throw("Error", "Method name must be a string"),
        };
        let cb = self.resolve_static_method(&cls, &n, true, forwarding)?;
        self.push_callee(cb, 0);
        Ok(())
    }

    // ------------------------------------------------------------ sending

    fn arg_by_ref_error(&mut self, arg: u32) -> R<()> {
        let p = self.ex.pending.last().unwrap();
        let f = p.func.clone();
        let ps = self.params_of(&f);
        let pname = ps
            .get(arg as usize)
            .or(ps.last())
            .map(|p| p.name.dname())
            .unwrap_or_default();
        self.throw(
            "Error",
            &format!(
                "{}(): Argument #{} (${}) could not be passed by reference",
                self.func_display_name(&f),
                arg + 1,
                pname
            ),
        )
    }

    /// Why a value is not callable, as PHP words it in "must be a valid
    /// callback" errors (observed on PHP 8.5 and in tests
    /// ext/standard/tests/array/array_map_object*.phpt).
    pub fn callable_error(&mut self, v: &Value) -> String {
        let v = v.deref();
        let class_named = |vm: &mut Vm, n: &[u8]| -> Option<Rc<Class>> {
            let l = crate::class::lower(n);
            match l.as_bytes() {
                b"self" | b"static" | b"parent" => {
                    let p = if l.as_bytes() == b"static" {
                        vm.static_class_ptr()
                    } else {
                        vm.scope_ptr()
                    };
                    if p.is_null() {
                        return None;
                    }
                    let c = vm.rc_class(p);
                    if l.as_bytes() == b"parent" {
                        c.parent.clone()
                    } else {
                        Some(c)
                    }
                }
                _ => {
                    let saved = vm.err.deferred.take();
                    let r = vm.find_class(n, true).ok().flatten();
                    vm.err.deferred = saved;
                    r
                }
            }
        };
        let method_reason = |vm: &mut Vm, cls: &Rc<Class>, m: &[u8], has_this: bool| -> String {
            match cls.find_method(&crate::class::lower(m)) {
                None => format!(
                    "class {} does not have a method \"{}\"",
                    cls.name.to_string_lossy(),
                    String::from_utf8_lossy(m)
                ),
                Some(f) => {
                    let scope = vm.scope_ptr();
                    if !has_this && f.flags & mflags::STATIC == 0 && vm.this_obj().is_none() {
                        return format!(
                            "non-static method {}() cannot be called statically",
                            f.display_name()
                        );
                    }
                    if !vm.method_visible(&f, scope) {
                        let vis = if f.flags & mflags::PRIVATE != 0 {
                            "private"
                        } else {
                            "protected"
                        };
                        return format!("cannot access {} method {}()", vis, f.display_name());
                    }
                    if f.flags & mflags::ABSTRACT != 0 {
                        return format!("cannot call abstract method {}()", f.display_name());
                    }
                    "no array or string given".to_string()
                }
            }
        };
        match &v {
            Value::Str(s) => {
                if let Some(p) = find_double_colon(s) {
                    let cn = &s[..p];
                    return match class_named(self, cn) {
                        Some(c) => method_reason(self, &c, &s[p + 2..], false),
                        None => format!("class \"{}\" not found", String::from_utf8_lossy(cn)),
                    };
                }
                format!(
                    "function \"{}\" not found or invalid function name",
                    s.to_string_lossy()
                )
            }
            Value::Array(a) => {
                if a.len() != 2 {
                    return "array callback must have exactly two members".to_string();
                }
                let (Some(t), Some(m)) = (
                    a.get_int(0).map(|x| x.deref()),
                    a.get_int(1).map(|x| x.deref()),
                ) else {
                    return "array callback must have exactly two members".to_string();
                };
                if !matches!(t, Value::Object(_) | Value::Str(_)) {
                    return "first array member is not a valid class name or object".to_string();
                }
                let Value::Str(m) = m else {
                    return "second array member is not a valid method".to_string();
                };
                match t {
                    Value::Object(o) => {
                        let cls = self.class_of(&o);
                        match self.qualified_method(&cls, &m) {
                            Some(Ok((c, mm))) => method_reason(self, &c, mm, true),
                            Some(Err(e)) => e,
                            None => method_reason(self, &cls, &m, true),
                        }
                    }
                    Value::Str(cn) => match class_named(self, &cn) {
                        Some(c) => match self.qualified_method(&c, &m) {
                            Some(Ok((q, mm))) => method_reason(self, &q, mm, false),
                            Some(Err(e)) => e,
                            None => method_reason(self, &c, &m, false),
                        },
                        None => format!("class \"{}\" not found", cn.to_string_lossy()),
                    },
                    _ => "first array member is not a valid class name or object".to_string(),
                }
            }
            _ => "no array or string given".to_string(),
        }
    }

    pub fn func_display_name(&self, f: &Func) -> String {
        if let FuncKind::User(c) = &f.kind
            && c.flags & fflags::CLOSURE != 0
        {
            return c.name.dname();
        }
        f.display_name()
    }

    /// Puts argument `arg` of the pending call.
    #[inline]
    fn put_arg(&mut self, arg: u32, v: Value) {
        let p = self.ex.pending.last_mut().unwrap();
        p.argc = p.argc.max(arg + 1);
        if !p.base.is_null() {
            let code = match &p.func.kind {
                FuncKind::User(c) => c,
                _ => unreachable!(),
            };
            let np = code.params.len();
            let i = arg as usize;
            if i < np && !code.params[i].variadic {
                let old = unsafe { (*p.base.add(i)).replace(v) };
                drop(old);
            } else {
                p.extra.push(v);
            }
        } else {
            let i = arg as usize;
            if i < p.args.len() {
                p.args[i] = v;
            } else {
                while p.args.len() < i {
                    p.args.push(Value::Null);
                }
                p.args.push(v);
            }
        }
        if arg < 64 {
            p.passed |= 1 << arg;
        }
    }

    /// Whether the pending call's by-reference parameters also accept
    /// values without a notice ("prefer reference", as array_multisort()'s
    /// do: test ext/standard/tests/array/sort/bug77395.phpt, PHP manual
    /// array_multisort examples passing SORT_DESC).
    fn pending_prefers_ref(&self) -> bool {
        let p = self.ex.pending.last().unwrap();
        p.func.is_native() && matches!(p.func.lname.as_bytes(), b"array_multisort" | b"extract")
    }

    pub(crate) fn send_val(&mut self, arg: u32, v: Value, from_call: bool) -> R<()> {
        if self.pending_arg_by_ref(arg) {
            if self.pending_prefers_ref() {
                self.put_arg(arg, v);
                return Ok(());
            }
            if from_call {
                self.notice_msg("Only variables should be passed by reference")?;
                self.put_arg(arg, Value::Ref(PhpRef::new(v)));
                return Ok(());
            }
            return self.arg_by_ref_error(arg);
        }
        self.put_arg(arg, v);
        Ok(())
    }

    pub(crate) fn send_var(&mut self, var: Reg, arg: u32) -> R<()> {
        if self.pending_arg_by_ref(arg) {
            let r = self.var_make_ref(var)?;
            self.put_arg(arg, Value::Ref(r));
        } else {
            let v = self.get(var)?;
            self.put_arg(arg, v);
        }
        Ok(())
    }

    pub(crate) fn send_ref(&mut self, arg: u32, v: Value) -> R<()> {
        let v = if self.pending_arg_by_ref(arg) {
            match v {
                Value::Ref(_) => v,
                other => Value::Ref(PhpRef::new(other)),
            }
        } else {
            v.into_deref()
        };
        self.put_arg(arg, v);
        Ok(())
    }

    pub(crate) fn send_unpack(&mut self, v: Value) -> R<()> {
        match v {
            Value::Array(a) => {
                for (k, x) in a.iter() {
                    match k {
                        KeyRef::Int(_) => {
                            if self.pending().has_named {
                                return self.throw(
                                    "Error",
                                    "Cannot use positional argument after named argument during unpacking",
                                );
                            }
                            let arg = self.pending().argc;
                            let byref = self.pending_arg_by_ref(arg);
                            let val = if byref {
                                match x {
                                    Value::Ref(_) => x.clone(),
                                    other => Value::Ref(PhpRef::new(other.clone())),
                                }
                            } else {
                                x.deref()
                            };
                            self.put_arg(arg, val);
                        }
                        KeyRef::Str(s) => {
                            let s = s.clone();
                            self.send_named(Value::Str(s), x.deref(), true)?;
                        }
                    }
                }
                Ok(())
            }
            Value::Object(o) => {
                let cls = self.class_of(&o);
                if !self.is_traversable(&cls) {
                    return self.throw("Error", "Only arrays and Traversables can be unpacked");
                }
                let items = self.iterate_to_vec(&Value::Object(o), true)?;
                for (k, x) in items {
                    match k {
                        Value::Str(s) => self.send_named(Value::Str(s), x, true)?,
                        Value::Int(_) | Value::Null => {
                            if self.pending().has_named {
                                return self.throw(
                                    "Error",
                                    "Cannot use positional argument after named argument during unpacking",
                                );
                            }
                            let arg = self.pending().argc;
                            self.put_arg(arg, x);
                        }
                        _ => {
                            return self.throw(
                                "Error",
                                "Keys must be of type int|string during argument unpacking",
                            );
                        }
                    }
                }
                Ok(())
            }
            _ => self.throw("Error", "Only arrays and Traversables can be unpacked"),
        }
    }

    pub(crate) fn send_named(&mut self, name: Value, v: Value, _unpack: bool) -> R<()> {
        let Value::Str(n) = name else {
            return self.throw("Error", "Named argument name must be a string");
        };
        self.pending().has_named = true;
        let p = self.ex.pending.last().unwrap();
        let f = p.func.clone();
        if p.magic_name.is_some() {
            // __call receives named arguments in the args array.
            let p = self.pending();
            p.named_extra
                .get_or_insert_with(PhpArray::new)
                .insert(Key::Str(n), v.into_deref());
            return Ok(());
        }
        let ps = self.params_of(&f);
        match ps.iter().position(|p| p.name == n && !p.variadic) {
            Some(i) => {
                let by_ref = ps[i].by_ref;
                let p = self.ex.pending.last().unwrap();
                let already = if i < 64 {
                    p.passed & (1 << i) != 0
                } else {
                    false
                };
                if already || (i as u32) < p.argc && p.passed & (1u64 << i.min(63)) != 0 {
                    return self.throw(
                        "Error",
                        &format!(
                            "Named parameter ${} overwrites previous argument",
                            n.to_string_lossy()
                        ),
                    );
                }
                let v = if by_ref {
                    match v {
                        Value::Ref(_) => v,
                        other => Value::Ref(PhpRef::new(other)),
                    }
                } else {
                    v.into_deref()
                };
                let argc = self.pending().argc;
                self.put_arg(i as u32, v);
                // Named args don't count as positional.
                self.pending().argc = argc.max(0);
                let p = self.pending();
                if (i as u32) >= p.argc {
                    // keep argc as positional count
                }
                Ok(())
            }
            None => {
                if ps.last().is_some_and(|p| p.variadic) {
                    let p = self.pending();
                    let na = p.named_extra.get_or_insert_with(PhpArray::new);
                    if na.get_str(&n).is_some() {
                        return self.throw(
                            "Error",
                            &format!(
                                "Named parameter ${} overwrites previous argument",
                                n.to_string_lossy()
                            ),
                        );
                    }
                    na.insert(Key::Str(n), v.into_deref());
                    return Ok(());
                }
                self.throw(
                    "Error",
                    &format!("Unknown named parameter ${}", n.to_string_lossy()),
                )
            }
        }
    }

    pub(crate) fn send_named_var(&mut self, name: Value, var: Reg) -> R<()> {
        if self.pending_named_by_ref(&name) {
            let r = self.var_make_ref(var)?;
            self.send_named(name, Value::Ref(r), false)
        } else {
            let v = self.get(var)?;
            self.send_named(name, v, false)
        }
    }

    // ------------------------------------------------------------ calling

    /// `DoCall`: makes the pending call. User functions get a frame (the
    /// loop continues in it); natives run now.
    pub(crate) fn do_call(&mut self, dst: Reg) -> R<Option<Value>> {
        let p = self.ex.pending.pop().expect("DoCall without pending call");
        let ret = if dst == NONE {
            std::ptr::null_mut()
        } else {
            self.slot_ptr(dst)
        };
        let ret_var = dst != NONE && dst & TMP == 0;
        if p.flags & pending_flags::NO_CTOR != 0 {
            self.discard_pending(p);
            return Ok(None);
        }
        if p.magic_name.is_some() {
            let v = self.call_magic_pending(p)?;
            if !ret.is_null() {
                self.put(dst, v);
            }
            return Ok(None);
        }
        match &p.func.kind {
            FuncKind::User(_) => {
                self.activate(p, ret, ret_var, false)?;
                Ok(None)
            }
            FuncKind::Native { .. } => {
                let is_new = p.flags & pending_flags::NEW != 0;
                let v = self.call_native_pending(p)?;
                if !is_new && !ret.is_null() {
                    self.put(dst, v);
                }
                Ok(None)
            }
        }
    }

    /// `__call`/`__callStatic`: `($name, $args)`.
    fn call_magic_pending(&mut self, mut p: Pending) -> R<Value> {
        let name = p.magic_name.take().unwrap();
        let mut arr = PhpArray::from_values(p.args.drain(..).map(|v| v.into_deref()));
        if let Some(na) = p.named_extra.take() {
            for (k, v) in na.iter() {
                arr.insert(k.to_owned(), v.clone());
            }
        }
        let c = Callee {
            func: p.func.clone(),
            this: p.this.take(),
            static_class: p.static_class,
            scope: p.scope,
            closure: p.closure.take(),
            magic_name: None,
        };
        self.discard_pending(p);
        self.call_callee(c, vec![Value::Str(name), Value::Array(arr)])
    }

    /// Runs a native function on the pending call's arguments.
    fn call_native_pending(&mut self, p: Pending) -> R<Value> {
        self.call_native_pending_ex(p, false)
    }

    fn call_native_pending_ex(&mut self, mut p: Pending, dynamic: bool) -> R<Value> {
        let func = p.func.clone();
        let FuncKind::Native { handler, .. } = &func.kind else {
            unreachable!()
        };
        let handler = *handler;
        let sig = self.native_sig(&func);
        let mut args = std::mem::take(&mut p.args);
        let named = p.named_extra.take();
        let this = p.this.take();
        let static_class = p.static_class;
        let passed = p.passed;
        let has_named = p.has_named;
        self.give_args_vec(std::mem::take(&mut p.extra));
        drop(p);
        if let Err(e) = self.prepare_native_args(&func, &sig, &mut args, passed, has_named) {
            self.give_args_vec(args);
            return Err(e);
        }
        if dynamic && is_scope_introspecting(&func.lname) {
            self.give_args_vec(args);
            return self.throw(
                "Error",
                &format!("Cannot call {}() dynamically", func.name.dname()),
            );
        }
        // Internal variadic functions take no extra named arguments, except
        // the callers of callbacks (test Zend/tests/named_params/
        // internal_variadics.phpt).
        if named.as_ref().is_some_and(|n| !n.is_empty())
            && func.scope().is_none()
            && !matches!(
                func.lname.as_bytes(),
                b"call_user_func" | b"forward_static_call"
            )
        {
            self.give_args_vec(args);
            return self.throw(
                "ArgumentCountError",
                &format!(
                    "{}() does not accept unknown named parameters",
                    func.display_name()
                ),
            );
        }
        self.ex.native_calls.push(super::NativeCall {
            func: func.clone(),
            args: &args as *const Vec<Value>,
            this: this.clone(),
            depth: self.ex.frames.len() as u32,
            line: self.cur_line(),
        });
        let mut a = Args {
            this,
            vals: &mut args,
            func: &func,
            static_class,
            named,
        };
        let r = handler(self, &mut a);
        // An error deferred by the builtin (lazy object initialization
        // while enumerating properties) is raised now.
        let r = match (r, self.err.deferred.is_some()) {
            (Ok(_), true) if self.err.defer_depth == 0 => Err(self.err.deferred.take().unwrap()),
            (r, _) => r,
        };
        self.ex.native_calls.pop();
        self.give_args_vec(args);
        r
    }

    /// Argument count and type checks for a native call (PHP's parameter
    /// parsing), filling defaults of parameters skipped by named arguments.
    fn prepare_native_args(
        &mut self,
        func: &Rc<Func>,
        sig: &NativeSig,
        args: &mut Vec<Value>,
        passed: u64,
        has_named: bool,
    ) -> R<()> {
        let np = sig.params.len();
        let n = args.len();
        let fname = func.display_name();
        if has_named {
            for i in 0..n.min(np) {
                if i < 64 && passed & (1 << i) == 0 {
                    let p = &sig.params[i];
                    match &p.default {
                        ParamDefault::Const(v) => args[i] = v.clone(),
                        // Optional parameters without a default (written
                        // `= UNKNOWN` in signatures): observed on PHP 8.5,
                        // "Argument #2 ($filter_value) must be passed
                        // explicitly, because the default value is not known".
                        ParamDefault::Lazy(_) if p.default_text.as_deref() == Some("UNKNOWN") => {
                            return self.throw(
                                "ArgumentCountError",
                                &format!(
                                    "{}(): Argument #{} (${}) must be passed explicitly, because the default value is not known",
                                    fname,
                                    i + 1,
                                    p.name.to_string_lossy()
                                ),
                            );
                        }
                        ParamDefault::Lazy(ce) => {
                            let ce = ce.clone();
                            args[i] = self.eval_const_expr(&ce, std::ptr::null())?;
                        }
                        _ => {
                            if p.variadic {
                                continue;
                            }
                            return self.throw(
                                "ArgumentCountError",
                                &format!(
                                    "{}(): Argument #{} (${}) not passed",
                                    fname,
                                    i + 1,
                                    p.name.dname()
                                ),
                            );
                        }
                    }
                }
            }
        }
        let req = sig.num_required as usize;
        if n < req || (!sig.variadic && n > np) {
            let exact = req == np && !sig.variadic;
            let (word, cnt) = if n < req {
                (if exact { "exactly" } else { "at least" }, req)
            } else {
                (if exact { "exactly" } else { "at most" }, np)
            };
            if has_named && n < req {
                let p = &sig.params[n];
                return self.throw(
                    "ArgumentCountError",
                    &format!(
                        "{}(): Argument #{} (${}) not passed",
                        fname,
                        n + 1,
                        p.name.dname()
                    ),
                );
            }
            return self.throw(
                "ArgumentCountError",
                &format!(
                    "{}() expects {} {} argument{}, {} given",
                    fname,
                    word,
                    cnt,
                    if cnt == 1 { "" } else { "s" },
                    n
                ),
            );
        }
        let strict = self.caller_strict();
        // Internal "array or object" parameters are reported as arrays
        // (observed on PHP 8.5: "reset(): Argument #1 ($array) must be of
        // type array, string given").
        let shown = |ty: &TypeHint| {
            if ty.mask == tmask::ARRAY | tmask::OBJECT && ty.classes.is_empty() {
                "array".to_string()
            } else {
                ty.display()
            }
        };
        for i in 0..n {
            let p = if i < np {
                &sig.params[i]
            } else {
                &sig.params[np - 1]
            };
            let Some(ty) = &p.ty else { continue };
            if p.by_ref {
                if !matches!(args[i], Value::Ref(_)) {
                    args[i] = Value::Ref(PhpRef::new(std::mem::take(&mut args[i])));
                }
                // Container types (`array &$array`) are checked in argument
                // order like the others (observed on PHP 8.5: usort($null,
                // "nope") reports argument #1); scalar ones are left to the
                // function, which writes the result.
                let scalar = tmask::BOOL | tmask::INT | tmask::FLOAT | tmask::STRING;
                if ty.mask & (scalar | tmask::MIXED) == 0 {
                    let cur = args[i].deref();
                    if let Check::Fail =
                        self.check_type(&cur, ty, true, func.scope.get(), std::ptr::null())?
                    {
                        return self.throw(
                            "TypeError",
                            &format!(
                                "{}(): Argument #{} (${}) must be of type {}, {} given",
                                fname,
                                i + 1,
                                p.name.to_string_lossy(),
                                shown(ty),
                                Vm::type_name_given(&cur)
                            ),
                        );
                    }
                }
                continue;
            }
            if let Value::Ref(r) = &args[i] {
                args[i] = r.get();
            }
            // Class-name parameters are not juggled (observed on PHP 8.5:
            // "get_parent_class(): Argument #1 ($object_or_class) must be an
            // object or a valid class name, int given").
            if i == 0
                && matches!(fname.as_bytes(), b"get_parent_class" | b"get_class_methods")
                && !matches!(args[0], Value::Object(_) | Value::Str(_))
            {
                return self.throw(
                    "TypeError",
                    &format!(
                        "{}(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
                        fname,
                        Vm::type_name_given(&args[0])
                    ),
                );
            }
            if args[i].is_null() && !ty.allows_null() {
                // Unions with classes too (observed on PHP 8.5:
                // ReflectionClass|string takes "" with a deprecation).
                let scalar =
                    ty.mask & (tmask::BOOL | tmask::INT | tmask::FLOAT | tmask::STRING) != 0;
                if scalar && !strict {
                    self.deprecated(&format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        fname,
                        i + 1,
                        p.name.dname(),
                        ty.display()
                    ))?;
                    args[i] = if ty.mask & tmask::STRING != 0 {
                        Value::Str(PhpStr::empty())
                    } else if ty.mask & tmask::INT != 0 {
                        Value::Int(0)
                    } else if ty.mask & tmask::FLOAT != 0 {
                        Value::Float(0.0)
                    } else {
                        Value::Bool(false)
                    };
                    continue;
                }
            }
            match self.check_type(&args[i], ty, strict, func.scope.get(), std::ptr::null())? {
                Check::Ok
                    if ty.mask & tmask::CALLABLE != 0
                        && matches!(args[i], Value::Str(_) | Value::Array(_)) =>
                {
                    self.callable_deprecation(&args[i])?;
                }
                Check::Ok => {}
                Check::Coerced(v) => args[i] = v,
                Check::Fail
                    if ty.mask & !tmask::NULL == tmask::CALLABLE && ty.classes.is_empty() =>
                {
                    // PHP explains why a value is not callable (observed on
                    // PHP 8.5: "must be a valid callback or null, function
                    // \"x\" not found or invalid function name").
                    let why = self.callable_error(&args[i]);
                    return self.throw(
                        "TypeError",
                        &format!(
                            "{}(): Argument #{} (${}) must be a valid callback{}, {}",
                            fname,
                            i + 1,
                            p.name.to_string_lossy(),
                            if ty.allows_null() { " or null" } else { "" },
                            why
                        ),
                    );
                }
                Check::Fail => {
                    // Variadic parameters are not named (observed on PHP
                    // 8.5: "array_diff(): Argument #2 must be of type array").
                    let pname = if p.variadic {
                        String::new()
                    } else {
                        format!(" (${})", p.name.to_string_lossy())
                    };
                    return self.throw(
                        "TypeError",
                        &format!(
                            "{}(): Argument #{}{} must be of type {}, {} given",
                            fname,
                            i + 1,
                            pname,
                            shown(ty),
                            Vm::type_name_given(&args[i])
                        ),
                    );
                }
            }
        }
        Ok(())
    }

    /// Activates a user function: pushes its frame and binds the arguments
    /// (count, defaults, types: errors are raised from inside the callee,
    /// as PHP does). For generator functions, the frame goes into a
    /// Generator object written to `ret`.
    pub(crate) fn activate(
        &mut self,
        mut p: Pending,
        ret: *mut Slot,
        ret_var: bool,
        entry: bool,
    ) -> R<()> {
        let func = p.func.clone();
        let FuncKind::User(code) = &func.kind else {
            unreachable!()
        };
        let code: &FuncCode = code;
        if code.flags & fflags::DEPRECATED != 0 {
            let what = if func.scope().is_some() {
                "Method"
            } else {
                "Function"
            };
            let msg = self.deprecated_message(code, what, &func);
            if let Err(e) = self.deprecated(&msg) {
                self.discard_pending(p);
                return Err(e);
            }
        }
        if code.flags & fflags::ABSTRACT != 0 {
            let r = self.throw(
                "Error",
                &format!("Cannot call abstract method {}()", func.display_name()),
            );
            self.discard_pending(p);
            return r;
        }
        let strict = self.caller_strict();
        let call_loc = if entry || self.ex.frames.is_empty() {
            None
        } else {
            Some((self.cur_file(), self.cur_line()))
        };
        let argc = p.argc;
        let mut flags = 0;
        if entry {
            flags |= frame_flags::ENTRY;
        }
        if ret_var {
            flags |= RET_VAR;
        }
        if p.flags & pending_flags::NEW != 0 {
            flags |= frame_flags::CTOR;
        }
        let scope = if p.closure.is_some() {
            p.scope
        } else {
            func.scope.get()
        };
        let extra = std::mem::take(&mut p.extra);
        let named_extra = p.named_extra.take();
        let has_named = p.has_named;
        let static_class = p.static_class;
        let frame = Frame {
            code: code as *const FuncCode,
            func: func.clone(),
            ip: 0,
            base: p.base,
            this: p.this.take(),
            static_class,
            scope,
            ret,
            argc,
            extra_args: Vec::new(),
            dyn_vars: None,
            closure: p.closure.take(),
            generator: None,
            flags,
            mark: p.mark,
            statics: p.statics,
            owner: 0,
            pending_base: self.ex.pending.len() as u32,
            native_base: self.ex.native_calls.len() as u32,
            line_hint: 0,
        };
        let Pending { args, .. } = p;
        self.give_args_vec(args);
        self.push_frame(frame);
        let saved_loc = self
            .loc_override
            .replace((code.file.clone(), code.line_start));
        let r = self.bind_args(
            &func,
            code,
            argc,
            extra,
            named_extra,
            has_named,
            strict,
            static_class,
            call_loc,
        );
        self.loc_override = saved_loc;
        if let Err(e) = r {
            self.pop_frame();
            return Err(e);
        }
        if code.flags & fflags::GENERATOR != 0 {
            let f = self.ex.frames.pop().unwrap();
            self.ex.sync_top();
            if let Some(top) = self.ex.frames.last() {
                self.ex.ip = top.ip as usize;
            }
            let g = self.create_generator(f)?;
            if !ret.is_null() {
                let old = unsafe { (*ret).replace(Value::Object(g)) };
                drop(old);
            }
        }
        Ok(())
    }

    /// Binds the arguments in the new frame's slots: defaults of missing
    /// parameters, type checks, variadics, closure `use` variables.
    #[allow(clippy::too_many_arguments)]
    fn bind_args(
        &mut self,
        func: &Rc<Func>,
        code: &FuncCode,
        argc: u32,
        mut extra: Vec<Value>,
        named_extra: Option<PhpArray>,
        has_named: bool,
        strict: bool,
        static_class: *const Class,
        call_loc: Option<(PhpStr, u32)>,
    ) -> R<()> {
        let np = code.params.len();
        let base = self.frame().base;
        // Methods of internal classes written in PHP check their arguments
        // the way internal functions do.
        let internal = code.file.is_empty();
        let call_loc = if internal { None } else { call_loc };
        if internal && !has_named && code.flags & fflags::VARIADIC == 0 && argc as usize > np {
            let exact = code.num_required as usize == np;
            let fname = self.func_display_name(func);
            return self.throw(
                "ArgumentCountError",
                &format!(
                    "{}() expects {} {} argument{}, {} given",
                    fname,
                    if exact { "exactly" } else { "at most" },
                    np,
                    if np == 1 { "" } else { "s" },
                    argc
                ),
            );
        }
        let called_in = |loc: &Option<(PhpStr, u32)>| match loc {
            Some((f, l)) => format!(", called in {} on line {}", f.to_string_lossy(), l),
            None => String::new(),
        };
        for i in 0..np {
            let param = &code.params[i];
            if param.variadic {
                break;
            }
            let slot = unsafe { &mut *base.add(i) };
            if slot.is_none() {
                match &param.default {
                    ParamDefault::Const(v) => {
                        let mut v = v.clone();
                        if let Some(ty) = &param.ty
                            && let Value::Int(n) = v
                            && ty.mask & tmask::FLOAT != 0
                            && ty.mask & tmask::INT == 0
                        {
                            v = Value::Float(n as f64);
                        }
                        if param.by_ref {
                            v = Value::Ref(PhpRef::new(v));
                        }
                        *slot = Some(v);
                        continue;
                    }
                    ParamDefault::Code => continue,
                    ParamDefault::Lazy(ce) => {
                        let ce = ce.clone();
                        let v = self.eval_const_expr(&ce, func.scope.get())?;
                        unsafe { *base.add(i) = Some(v) };
                        continue;
                    }
                    ParamDefault::None => {
                        let fname = self.func_display_name(func);
                        let msg = if has_named {
                            format!(
                                "{}(): Argument #{} (${}) not passed",
                                fname,
                                i + 1,
                                param.name.dname()
                            )
                        } else if internal {
                            let exact = code.num_required as usize == np
                                && code.flags & fflags::VARIADIC == 0;
                            let n = code.num_required;
                            format!(
                                "{}() expects {} {} argument{}, {} given",
                                fname,
                                if exact { "exactly" } else { "at least" },
                                n,
                                if n == 1 { "" } else { "s" },
                                argc
                            )
                        } else {
                            let exact = code.num_required as usize == np
                                && code.flags & fflags::VARIADIC == 0;
                            let passed = match &call_loc {
                                Some((f, l)) => format!(
                                    "{} passed in {} on line {}",
                                    argc,
                                    f.to_string_lossy(),
                                    l
                                ),
                                None => format!("{} passed", argc),
                            };
                            format!(
                                "Too few arguments to function {}(), {} and {} {} expected",
                                fname,
                                passed,
                                if exact { "exactly" } else { "at least" },
                                code.num_required
                            )
                        };
                        return self.throw("ArgumentCountError", &msg);
                    }
                }
            }
            if internal
                && let Some(ty) = &param.ty
                && !param.by_ref
                && matches!(unsafe { &*base.add(i) }, Some(Value::Null))
                && !ty.allows_null()
                && ty.classes.is_empty()
                && ty.mask & (tmask::BOOL | tmask::INT | tmask::FLOAT | tmask::STRING) != 0
                && !strict
            {
                let fname = self.func_display_name(func);
                self.deprecated(&format!(
                    "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                    fname,
                    i + 1,
                    param.name.dname(),
                    ty.display()
                ))?;
                let v = if ty.mask & tmask::STRING != 0 {
                    Value::Str(PhpStr::empty())
                } else if ty.mask & tmask::INT != 0 {
                    Value::Int(0)
                } else if ty.mask & tmask::FLOAT != 0 {
                    Value::Float(0.0)
                } else {
                    Value::Bool(false)
                };
                unsafe { *base.add(i) = Some(v) };
                continue;
            }
            if let Some(ty) = &param.ty {
                self.check_param(func, code, base, i, ty, strict, static_class, &call_loc)?;
            }
        }
        // Variadics and extra arguments.
        if code.flags & fflags::VARIADIC != 0 {
            let vi = np - 1;
            let param = &code.params[vi];
            let mut arr = PhpArray::new();
            for (k, v) in extra.drain(..).enumerate() {
                let mut v = v;
                if let Some(ty) = &param.ty
                    && !param.by_ref
                {
                    match self.check_type(&v, ty, strict, func.scope.get(), static_class)? {
                        Check::Ok => {}
                        Check::Coerced(c) => v = c,
                        Check::Fail => {
                            let msg = format!(
                                "{}(): Argument #{} must be of type {}, {} given{}",
                                self.func_display_name(func),
                                vi + k + 1,
                                ty.display(),
                                Vm::type_name_given(&v),
                                called_in(&call_loc)
                            );
                            let r = self.throw("TypeError", &msg);
                            if call_loc.is_some() {
                                self.mark_defined_in(&r);
                            }
                            return r;
                        }
                    }
                }
                let _ = arr.push(v);
            }
            if let Some(na) = named_extra {
                for (k, v) in na.iter() {
                    arr.insert(k.to_owned(), v.clone());
                }
            }
            unsafe { *base.add(vi) = Some(Value::Array(arr)) };
        } else {
            self.frame_mut().extra_args = extra;
        }
        // Closure `use` variables.
        if let Some(cl) = self.frame().closure.clone()
            && !code.uses.is_empty()
        {
            let n = cl.obj().native.borrow();
            if let Some(n) = n.as_ref() {
                let any: &dyn std::any::Any = &**n;
                if let Some(cd) = any.downcast_ref::<ClosureData>() {
                    for (k, (inner, _, _)) in code.uses.iter().enumerate() {
                        if let Some(Some(v)) = cd.captured.get(k) {
                            unsafe { *base.add(*inner as usize) = Some(v.clone()) };
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn deprecated_message(&mut self, code: &FuncCode, what: &str, func: &Func) -> String {
        let mut msg = format!("{} {}() is deprecated", what, func.display_name());
        if let Some(a) = code
            .attrs
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(b"Deprecated"))
        {
            let mut message = None;
            let mut since = None;
            for (i, (n, v)) in a.args.iter().enumerate() {
                let v = match v {
                    ConstExpr::Value(v) => v.clone(),
                    _ => Value::Null,
                };
                let key = n.as_ref().map(|n| n.to_string_lossy());
                match (key.as_deref(), i) {
                    (Some("message"), _) | (None, 0) => {
                        message = v.as_str().map(|s| s.to_string_lossy())
                    }
                    (Some("since"), _) | (None, 1) => {
                        since = v.as_str().map(|s| s.to_string_lossy())
                    }
                    _ => {}
                }
            }
            if let Some(s) = since {
                msg = format!(
                    "{} {}() is deprecated since {}",
                    what,
                    func.display_name(),
                    s
                );
            }
            if let Some(m) = message {
                msg.push_str(", ");
                msg.push_str(&m);
            }
        }
        msg
    }

    /// Checks (and coerces) parameter `i` in the slots at `base`.
    #[allow(clippy::too_many_arguments)]
    fn check_param(
        &mut self,
        func: &Rc<Func>,
        code: &FuncCode,
        base: *mut Slot,
        i: usize,
        ty: &TypeHint,
        strict: bool,
        static_class: *const Class,
        call_loc: &Option<(PhpStr, u32)>,
    ) -> R<()> {
        let slot = unsafe { &mut *base.add(i) };
        let (cell, val) = match slot.as_ref() {
            Some(Value::Ref(r)) => (Some(r.clone()), r.get()),
            Some(v) => (None, v.clone()),
            None => return Ok(()),
        };
        // Fast exact matches.
        match (&val, ty.mask) {
            (Value::Int(_), m) if m & tmask::INT != 0 => return Ok(()),
            (Value::Str(_), m) if m & tmask::STRING != 0 => return Ok(()),
            (Value::Array(_), m) if m & tmask::ARRAY != 0 => return Ok(()),
            (Value::Bool(_), m) if m & tmask::BOOL == tmask::BOOL => return Ok(()),
            (Value::Float(_), m) if m & tmask::FLOAT != 0 => return Ok(()),
            _ => {}
        }
        let r = self.check_type(&val, ty, strict, func.scope.get(), static_class);
        match r? {
            Check::Ok => Ok(()),
            Check::Coerced(c) => {
                match cell {
                    Some(r) => {
                        let old = r.set(c);
                        drop(old);
                    }
                    None => *slot = Some(c),
                }
                Ok(())
            }
            Check::Fail => {
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type {}, {} given{}",
                    self.func_display_name(func),
                    i + 1,
                    code.params[i].name.dname(),
                    ty.display(),
                    Vm::type_name_given(&val),
                    match call_loc {
                        Some((f, l)) =>
                            format!(", called in {} on line {}", f.to_string_lossy(), l),
                        None => String::new(),
                    }
                );
                let r = self.throw("TypeError", &msg);
                if call_loc.is_some() {
                    self.mark_defined_in(&r);
                }
                r
            }
        }
    }

    /// Argument TypeErrors naming their call site print as "... called
    /// in F on line L and defined in F2:L2" (observed on PHP 8.5).
    pub(crate) fn mark_defined_in<T>(&mut self, r: &R<T>) {
        if let Err(super::Unwind::Throw(e)) = r {
            self.defined_in_errors.retain(|w| w.upgrade().is_some());
            self.defined_in_errors.push(e.downgrade());
        }
    }

    pub(crate) fn push_frame(&mut self, f: Frame) {
        if let Some(top) = self.ex.frames.last_mut() {
            top.ip = self.ex.ip as u32;
        }
        self.ex.frames.push(f);
        self.ex.sync_top();
        self.ex.ip = 0;
    }

    /// Removes the top frame, releasing its variables.
    pub(crate) fn pop_frame(&mut self) {
        let f = self.ex.frames.pop().expect("pop without frame");
        self.ex.sync_top();
        if let Some(top) = self.ex.frames.last() {
            self.ex.ip = top.ip as usize;
        }
        // Pending calls opened in the frame.
        while self.ex.pending.len() > f.pending_base as usize {
            let p = self.ex.pending.pop().unwrap();
            self.discard_pending(p);
        }
        self.ex.native_calls.truncate(f.native_base as usize);
        self.release_frame(f);
    }

    pub(crate) fn release_frame(&mut self, f: Frame) {
        if f.flags & frame_flags::ATTACHED != 0 {
            self.detach_frame(&f);
        }
        if f.flags & frame_flags::OWN_SLOTS == 0 {
            let n = f.code().num_slots as usize;
            for i in 0..n {
                let old = unsafe { (*f.base.add(i)).take() };
                drop(old);
            }
            self.ex.stack.release(f.mark);
        }
        drop(f);
    }

    /// `return`: pops the frame and delivers the value. `Some(v)` when the
    /// frame was an entry frame (the Rust caller gets `v`).
    pub(crate) fn do_return(&mut self, v: Value, by_ref: bool) -> R<Option<Value>> {
        if self.frame().flags & frame_flags::GENERATOR != 0 {
            return self.generator_return(v);
        }
        let f = self.ex.frames.pop().unwrap();
        self.ex.sync_top();
        if let Some(top) = self.ex.frames.last() {
            self.ex.ip = top.ip as usize;
        }
        let ret = f.ret;
        let flags = f.flags;
        // Deliver before releasing locals (as PHP copies the value first).
        let v = if by_ref { v } else { v.into_deref() };
        self.release_frame(f);
        if flags & frame_flags::ENTRY != 0 {
            return Ok(Some(v));
        }
        if !ret.is_null() {
            unsafe {
                if flags & RET_VAR != 0 {
                    match &mut *ret {
                        Some(Value::Ref(r)) => {
                            let old = r.set(v.into_deref());
                            drop(old);
                        }
                        slot => {
                            let old = slot.replace(v.into_deref());
                            drop(old);
                        }
                    }
                } else {
                    let old = (*ret).replace(v);
                    drop(old);
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn recv_check(&mut self, param: u32) -> R<()> {
        let func = self.frame().func.clone();
        let code = self.frame().code;
        let code = unsafe { &*code };
        let Some(ty) = &code.params[param as usize].ty else {
            return Ok(());
        };
        let p = self.slot_ptr(param);
        let v = unsafe { (*p).clone() }.unwrap_or_default();
        let strict = code.flags & fflags::STRICT != 0;
        match self.check_type(&v, ty, strict, func.scope.get(), self.frame().static_class)? {
            Check::Ok => Ok(()),
            Check::Coerced(c) => {
                unsafe { *p = Some(c) };
                Ok(())
            }
            Check::Fail => {
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type {}, {} given",
                    self.func_display_name(&func),
                    param + 1,
                    code.params[param as usize].name.dname(),
                    ty.display(),
                    Vm::type_name_given(&v)
                );
                self.throw("TypeError", &msg)
            }
        }
    }

    pub(crate) fn verify_return(&mut self, src: Reg, implicit: bool) -> R<()> {
        let code = unsafe { &*self.frame().code };
        let Some(ty) = &code.ret_type else {
            return Ok(());
        };
        if ty.mask & tmask::VOID != 0 {
            return Ok(());
        }
        // Fast exact matches.
        if !implicit {
            let ok = match (self.peek(src), ty.mask) {
                (Some(Value::Int(_)), m) => m & tmask::INT != 0,
                (Some(Value::Str(_)), m) => m & tmask::STRING != 0,
                (Some(Value::Array(_)), m) => m & tmask::ARRAY != 0,
                (Some(Value::Bool(_)), m) => m & tmask::BOOL == tmask::BOOL,
                (Some(Value::Float(_)), m) => m & tmask::FLOAT != 0,
                (Some(Value::Null), m) => m & tmask::NULL != 0,
                _ => false,
            };
            if ok {
                return Ok(());
            }
        }
        let func = self.frame().func.clone();
        if ty.mask & tmask::NEVER != 0 {
            let fname = self.func_display_name(&func);
            return self.type_error(&format!(
                "{}(): never-returning function must not implicitly return",
                fname
            ));
        }
        let none = implicit;
        let v = match self.peek(src) {
            Some(v) => v.deref(),
            None => Value::Null,
        };
        let strict = code.flags & fflags::STRICT != 0;
        let static_class = self.frame().static_class;
        match self.check_type(&v, ty, strict, func.scope.get(), static_class)? {
            Check::Ok => Ok(()),
            Check::Coerced(c) => {
                // The compiler puts constants in a temporary first.
                if src & CONST == 0 {
                    let p = self.slot_ptr(src);
                    unsafe {
                        match &*p {
                            Some(Value::Ref(r)) => {
                                let old = r.set(c);
                                drop(old);
                            }
                            _ => *p = Some(c),
                        }
                    }
                }
                Ok(())
            }
            Check::Fail => {
                let fname = self.func_display_name(&func);
                if none && ty.mask & tmask::NULL == 0 {
                    return self.type_error(&format!(
                        "{}(): Return value must be of type {}, none returned",
                        fname,
                        ty.display()
                    ));
                }
                self.type_error(&format!(
                    "{}(): Return value must be of type {}, {} returned",
                    fname,
                    ty.display(),
                    v.tname()
                ))
            }
        }
    }

    // ------------------------------------------------------------ calls from Rust

    /// Calls a callable value with arguments (callbacks of natives).
    pub fn call_value(&mut self, callable: &Value, args: Vec<Value>) -> R<Value> {
        let c = self.resolve_callable(callable, true)?;
        self.call_callee(c, args)
    }

    pub fn call_func_this(
        &mut self,
        f: &Rc<Func>,
        this: Option<ObjRef>,
        args: Vec<Value>,
    ) -> R<Value> {
        let static_class = match &this {
            Some(o) => self.class_ptr_of(o),
            None => f.scope.get(),
        };
        let c = Callee {
            func: f.clone(),
            this,
            static_class,
            scope: f.scope.get(),
            closure: None,
            magic_name: None,
        };
        self.call_callee(c, args)
    }

    /// Calls method `lname` on an object (no visibility checks).
    pub fn call_method(&mut self, o: &ObjRef, lname: &[u8], args: Vec<Value>) -> R<Value> {
        self.call_method_raw(o, lname, args).map(|v| v.into_deref())
    }

    /// Like [`call_method`](Self::call_method), keeping a reference
    /// returned by a by-reference method.
    pub fn call_method_raw(&mut self, o: &ObjRef, lname: &[u8], args: Vec<Value>) -> R<Value> {
        let cls = self.class_of(o);
        match cls.find_method(lname) {
            Some(f) => {
                let static_class = self.class_ptr_of(o);
                let c = Callee {
                    func: f.clone(),
                    this: if f.flags & mflags::STATIC != 0 {
                        None
                    } else {
                        Some(o.clone())
                    },
                    static_class,
                    scope: f.scope.get(),
                    closure: None,
                    magic_name: None,
                };
                self.call_callee_raw(c, args)
            }
            None => {
                let c = self.resolve_method(o, &cls, lname, true)?;
                self.call_callee_raw(c, args)
            }
        }
    }

    /// Calls a resolved callee with positional arguments; by-reference
    /// parameters get the given references (or a warning for values).
    pub fn call_callee(&mut self, c: Callee, args: Vec<Value>) -> R<Value> {
        self.call_callee_raw(c, args).map(|v| v.into_deref())
    }

    pub fn call_callee_raw(&mut self, c: Callee, args: Vec<Value>) -> R<Value> {
        let magic = c.magic_name.is_some();
        self.push_callee(c, 0);
        let mut named: Vec<(PhpStr, Value)> = Vec::new();
        for (i, a) in args.into_iter().enumerate() {
            let i = i as u32;
            if !magic && self.pending_arg_by_ref(i) {
                let v = match a {
                    Value::Ref(_) => a,
                    other if self.pending_prefers_ref() => other,
                    other => {
                        let f = self.ex.pending.last().unwrap().func.clone();
                        let pname = self
                            .params_of(&f)
                            .get(i as usize)
                            .map(|p| p.name.dname())
                            .unwrap_or_default();
                        let msg = format!(
                            "{}(): Argument #{} (${}) must be passed by reference, value given",
                            self.func_display_name(&f),
                            i + 1,
                            pname
                        );
                        if let Err(e) = self.warn(&msg) {
                            let p = self.ex.pending.pop().unwrap();
                            self.discard_pending(p);
                            return Err(e);
                        }
                        Value::Ref(PhpRef::new(other))
                    }
                };
                self.put_arg(i, v);
            } else {
                let v = if magic {
                    a.into_deref()
                } else {
                    a.into_deref()
                };
                self.put_arg(i, v);
            }
        }
        let _ = &mut named;
        self.finish_rust_call()
    }

    /// Calls with named arguments too (`call_user_func_array` with string
    /// keys).
    pub fn call_callee_named(&mut self, c: Callee, args: PhpArray) -> R<Value> {
        self.push_callee(c, 0);
        let mut seen_named = false;
        for (k, a) in args.iter() {
            match k {
                KeyRef::Int(_) => {
                    if seen_named {
                        let p = self.ex.pending.pop().unwrap();
                        self.discard_pending(p);
                        return self.throw(
                            "Error",
                            "Cannot use positional argument after named argument",
                        );
                    }
                    let i = self.ex.pending.last().unwrap().argc;
                    let v = if self.pending_arg_by_ref(i) {
                        match a {
                            Value::Ref(_) => a.clone(),
                            other if self.pending_prefers_ref() => other.clone(),
                            other => {
                                let f = self.ex.pending.last().unwrap().func.clone();
                                let pname = self
                                    .params_of(&f)
                                    .get(i as usize)
                                    .map(|p| p.name.dname())
                                    .unwrap_or_default();
                                let msg = format!(
                                    "{}(): Argument #{} (${}) must be passed by reference, value given",
                                    self.func_display_name(&f),
                                    i + 1,
                                    pname
                                );
                                if let Err(e) = self.warn(&msg) {
                                    let p = self.ex.pending.pop().unwrap();
                                    self.discard_pending(p);
                                    return Err(e);
                                }
                                Value::Ref(PhpRef::new(other.clone()))
                            }
                        }
                    } else {
                        a.deref()
                    };
                    self.put_arg(i, v);
                }
                KeyRef::Str(s) => {
                    seen_named = true;
                    let s = s.clone();
                    if let Err(e) = self.send_named(Value::Str(s), a.clone(), false) {
                        let p = self.ex.pending.pop().unwrap();
                        self.discard_pending(p);
                        return Err(e);
                    }
                }
            }
        }
        self.finish_rust_call()
    }

    /// Makes the pending call on top as an entry call and runs it.
    fn finish_rust_call(&mut self) -> R<Value> {
        // Diagnostics queued by a write path come out before user code runs.
        if !self.err.queued.is_empty()
            && let Err(e) = self.flush_deferred()
        {
            let p = self.ex.pending.pop().unwrap();
            self.discard_pending(p);
            return Err(e);
        }
        let saved = std::mem::replace(&mut self.err.defer_depth, 0);
        let saved_loc = self.loc_override.take();
        let r = self.finish_rust_call_inner();
        self.loc_override = saved_loc;
        self.err.defer_depth = saved;
        r
    }

    fn finish_rust_call_inner(&mut self) -> R<Value> {
        let p = self.ex.pending.pop().unwrap();
        if p.magic_name.is_some() {
            return self.call_magic_pending(p);
        }
        match &p.func.kind {
            FuncKind::User(_) => {
                let mut result: Slot = None;
                let ret = &mut result as *mut Slot;
                let is_gen = p.func.code().is_some_and(|c| c.is_generator());
                let depth = self.ex.frames.len();
                self.activate(p, ret, false, true)?;
                if is_gen || self.ex.frames.len() == depth {
                    return Ok(result.unwrap_or_default());
                }
                self.run()
            }
            FuncKind::Native { .. } => self.call_native_pending_ex(p, true),
        }
    }

    /// Runs `__destruct` of an object whose last reference went away.
    pub fn call_destructor(&mut self, o: ObjRef) -> R<()> {
        let cls = self.class_of(&o);
        let d = cls.magic.borrow().destruct.clone();
        if let Some(d) = d {
            // A non-public destructor runs only where it is visible (PHP
            // manual: Constructors and Destructors; tests/classes/
            // destructor_visibility_*.phpt).
            if d.flags & mflags::PUBLIC == 0 {
                let scope = self.scope_ptr();
                if !self.method_visible(&d, scope) {
                    let vis = if d.flags & mflags::PRIVATE != 0 {
                        "private"
                    } else {
                        "protected"
                    };
                    let msg = format!(
                        "Call to {} {}::__destruct() from {}",
                        vis,
                        cls.name.dname(),
                        self.scope_desc(scope)
                    );
                    if self.shutting_down {
                        self.raise_at(
                            super::errors::E_WARNING,
                            &format!("{} during shutdown ignored", msg),
                            PhpStr::from("Unknown"),
                            0,
                        )?;
                        return Ok(());
                    }
                    return self.throw("Error", &msg);
                }
            }
            self.dtor_depth += 1;
            let r = self.call_func_this(&d, Some(o.clone()), Vec::new());
            self.dtor_depth -= 1;
            r?;
        }
        drop(o);
        Ok(())
    }

    // ------------------------------------------------------------ closures

    pub(crate) fn closure_func(&mut self, code: Rc<FuncCode>) -> Rc<Func> {
        let key = Rc::as_ptr(&code) as usize;
        if let Some(f) = self.closure_funcs.get(&key) {
            return f.clone();
        }
        let f = Rc::new(Func::user(code, mflags::PUBLIC));
        self.closure_funcs.insert(key, f.clone());
        f
    }

    pub(crate) fn make_closure(&mut self, code: Rc<FuncCode>) -> R<Value> {
        let is_static = code.flags & fflags::STATIC_CLOSURE != 0;
        let is_arrow = code.flags & fflags::ARROW != 0;
        let mut captured = Vec::with_capacity(code.uses.len());
        for &(_, outer, by_ref) in &code.uses {
            if by_ref {
                let r = self.var_make_ref(outer)?;
                captured.push(Some(Value::Ref(r)));
            } else {
                let p = self.slot_ptr(outer);
                match unsafe { &*p } {
                    Some(v) => captured.push(Some(v.deref())),
                    None => {
                        if let Some(v) = self.attached_lookup(outer) {
                            captured.push(Some(v.deref()));
                        } else if is_arrow {
                            captured.push(None);
                        } else {
                            self.undefined_var(outer)?;
                            captured.push(Some(Value::Null));
                        }
                    }
                }
            }
        }
        let func = self.closure_func(code.clone());
        let f = self.frame();
        let this = if is_static { None } else { f.this.clone() };
        let scope = f.scope;
        let called = if f.static_class.is_null() {
            scope
        } else {
            f.static_class
        };
        let statics = RefCell::new(vec![None; code.num_statics as usize]);
        let cd = ClosureData {
            func,
            this,
            scope,
            called_scope: called,
            captured,
            statics,
            magic_name: None,
            from_callable: false,
        };
        let o = self.new_closure_object(cd)?;
        Ok(Value::Object(o))
    }

    pub fn new_closure_object(&mut self, cd: ClosureData) -> R<ObjRef> {
        let cls = self.core.closure.clone().expect("Closure class");
        let o = goro_runtime::ObjRef::new(cls, Vec::new());
        *o.obj().native.borrow_mut() = Some(Box::new(cd));
        Ok(o)
    }

    /// A Closure for a callee (`Closure::fromCallable`, `f(...)`).
    pub fn closure_from_callee(&mut self, c: Callee) -> R<ObjRef> {
        if let Some(cl) = &c.closure
            && c.magic_name.is_none()
        {
            return Ok(cl.clone());
        }
        let n = c.func.code().map(|c| c.num_statics as usize).unwrap_or(0);
        let cd = ClosureData {
            func: c.func.clone(),
            this: c.this,
            scope: c.scope,
            called_scope: c.static_class,
            captured: Vec::new(),
            statics: RefCell::new(vec![None; n]),
            magic_name: c.magic_name,
            from_callable: true,
        };
        self.new_closure_object(cd)
    }

    pub(crate) fn pending_to_closure(&mut self) -> R<Value> {
        let mut p = self.ex.pending.pop().unwrap();
        let c = Callee {
            func: p.func.clone(),
            this: p.this.take(),
            static_class: p.static_class,
            scope: p.scope,
            closure: p.closure.take(),
            magic_name: p.magic_name.take(),
        };
        self.discard_pending(p);
        let o = self.closure_from_callee(c)?;
        Ok(Value::Object(o))
    }

    // ------------------------------------------------------------ statics and functions

    fn statics_cell(&self) -> &RefCell<Vec<Option<PhpRef>>> {
        let f = self.frame();
        if f.statics.is_null() {
            &f.code().statics
        } else {
            unsafe { &*f.statics }
        }
    }

    /// `static $x`: binds to the existing cell (true) or asks for the
    /// initializer to run (false).
    pub(crate) fn bind_static(&mut self, var: Reg, index: u32) -> R<bool> {
        let cell = {
            let st = self.statics_cell();
            let mut st = st.borrow_mut();
            if st.len() <= index as usize {
                st.resize(index as usize + 1, None);
            }
            st[index as usize].clone()
        };
        match cell {
            Some(r) => {
                self.bind_ref_var(var, Value::Ref(r))?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub(crate) fn static_init(&mut self, index: u32, v: Value) -> R<()> {
        // The variable is the one bound by the preceding BindStatic.
        let ip = self.ex.ip;
        let code = self.code_ref();
        let mut var = NONE;
        for k in (0..ip).rev() {
            if let Op::BindStatic {
                var: v, index: i, ..
            } = code.ops[k]
                && i == index
            {
                var = v;
                break;
            }
        }
        let r = {
            let st = self.statics_cell();
            let mut st = st.borrow_mut();
            match &st[index as usize] {
                Some(r) => r.clone(),
                None => {
                    let r = PhpRef::new(v);
                    st[index as usize] = Some(r.clone());
                    r
                }
            }
        };
        if var != NONE {
            self.bind_ref_var(var, Value::Ref(r))?;
        }
        Ok(())
    }

    pub fn declare_function(&mut self, code: Rc<FuncCode>) -> R<()> {
        let lname = crate::class::lower(&code.name);
        if let Some(prev) = self.functions.get(&lname) {
            let msg = match prev.code() {
                Some(pc) => format!(
                    "Cannot redeclare function {}() (previously declared in {}:{})",
                    code.name.dname(),
                    pc.file.to_string_lossy(),
                    pc.line_start
                ),
                None => format!("Cannot redeclare function {}()", code.name.dname()),
            };
            let line = code.line_start;
            let file = code.file.clone();
            return Err(self.fatal_at(&msg, &file, line));
        }
        let f = Rc::new(Func::user(code.clone(), 0));
        self.functions.insert(lname.clone(), f);
        self.user_functions.push(lname);
        Ok(())
    }
}

fn find_double_colon(s: &[u8]) -> Option<usize> {
    s.windows(2).position(|w| w == b"::")
}

/// Functions that read or write the caller's variables can't be called
/// dynamically (observed on PHP 8.5: "Cannot call compact() dynamically").
pub fn is_scope_introspecting(lname: &[u8]) -> bool {
    matches!(
        lname,
        b"compact"
            | b"extract"
            | b"get_defined_vars"
            | b"func_get_args"
            | b"func_get_arg"
            | b"func_num_args"
    )
}
