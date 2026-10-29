//! Calls: callee resolution, argument passing, frame activation and
//! return, callables, closures and static variables.

use crate::class::{DisplayName, TypeName};
use std::cell::RefCell;
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use super::types::Check;
use super::{Frame, Pending, R, Slot, StackMark, Vm, frame_flags, pending_flags};
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

/// The statics of a callee: closures have their own, and their `use`
/// variables go into the callee's fresh slots at `base` (when not null).
#[inline]
fn closure_statics(c: &Callee, base: *mut Slot) -> *const RefCell<Vec<Option<PhpRef>>> {
    let Some(cl) = &c.closure else {
        return &c.func.statics;
    };
    let n = cl.obj().native.borrow();
    let Some(n) = n.as_ref() else {
        return &c.func.statics;
    };
    let any: &dyn std::any::Any = &**n;
    let Some(cd) = any.downcast_ref::<ClosureData>() else {
        return &c.func.statics;
    };
    if !base.is_null()
        && let FuncKind::User(code) = &c.func.kind
    {
        for (k, (inner, _, _)) in code.uses.iter().enumerate() {
            if let Some(Some(v)) = cd.captured.get(k) {
                // SAFETY: the callee's fresh slots.
                unsafe { *base.add(*inner as usize) = Some(v.clone()) };
            }
        }
    }
    &cd.statics
}

/// Code flags that need the general activation path.
const SLOW_ACTIVATION: u32 = fflags::DEPRECATED
    | fflags::ABSTRACT
    | fflags::NODISCARD
    | fflags::GENERATOR
    | fflags::VARIADIC;

/// Pushes and starts the frame of a user function whose arguments are
/// bound simply (see `args_bind_simply`): fills the constant defaults of
/// parameters not passed (the others are computed by the prologue), then
/// writes the frame in place, field by field (built whole, the large
/// struct would be copied). A macro so that each caller reads its sources
/// where they are written. Unsafe: `base` must be the callee's slots.
macro_rules! enter_simple_frame {
    ($vm:expr, $code:expr, $base:expr, $argc:expr,
     pending_base: $pb:expr, func: $func:expr, this: $this:expr,
     static_class: $sc:expr, scope: $scope:expr, ret: $ret:expr,
     closure: $closure:expr, flags: $flags:expr, mark: $mark:expr,
     statics: $statics:expr) => {{
        let code: &FuncCode = $code;
        let base: *mut Slot = $base;
        let argc: u32 = $argc;
        for i in argc as usize..code.params.len() {
            let param = &code.params[i];
            if let ParamDefault::Const(v) = &param.default {
                let mut v = v.clone();
                if let Some(ty) = &param.ty
                    && let Value::Int(n) = v
                    && ty.mask & tmask::FLOAT != 0
                    && ty.mask & tmask::INT == 0
                {
                    v = Value::Float(n as f64);
                }
                *base.add(i) = Some(v);
            }
        }
        if let Some(top) = $vm.ex.frames.last_mut() {
            top.ip = $vm.ex.ip as u32;
        }
        let native_base = $vm.ex.native_calls.len() as u32;
        let frames = &mut $vm.ex.frames;
        frames.reserve(1);
        {
            use std::ptr::addr_of_mut as at;
            let d = frames.as_mut_ptr().add(frames.len());
            at!((*d).code).write(code as *const FuncCode);
            at!((*d).func).write($func);
            at!((*d).ip).write(0);
            at!((*d).base).write(base);
            at!((*d).this).write($this);
            at!((*d).static_class).write($sc);
            at!((*d).scope).write($scope);
            at!((*d).ret).write($ret);
            at!((*d).argc).write(argc);
            at!((*d).extra_args).write(Vec::new());
            at!((*d).dyn_vars).write(None);
            at!((*d).closure).write($closure);
            at!((*d).generator).write(None);
            at!((*d).flags).write($flags);
            at!((*d).mark).write($mark);
            at!((*d).statics).write($statics);
            at!((*d).owner).write(0);
            at!((*d).pending_base).write($pb);
            at!((*d).native_base).write(native_base);
            at!((*d).line_hint).write(0);
            frames.set_len(frames.len() + 1);
        }
        $vm.ex.top = super::TopFrame::of(base, code);
        $vm.ex.ip = 0;
    }};
}

/// A callable that natives call repeatedly (array_map, usort,
/// preg_replace_callback...), resolved on the first call only: PHP
/// resolves the callable argument of an internal function once, when the
/// parameter is parsed (PHP manual, Callables).
pub struct PreparedCall {
    value: Value,
    callee: Option<Callee>,
    /// A user function or closure that can be entered directly (no
    /// trampoline, nothing the general activation must handle).
    direct: bool,
    /// The closure's data (direct closures): the closure object, which
    /// `callee` holds, keeps it in place.
    cd: *const ClosureData,
}

impl PreparedCall {
    pub fn new(value: Value) -> PreparedCall {
        PreparedCall {
            value,
            callee: None,
            direct: false,
            cd: std::ptr::null(),
        }
    }

    /// The callable value.
    pub fn value(&self) -> &Value {
        &self.value
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

/// The property a `parent::$prop::get/set` stand-in works on: the object
/// and its own declaration of the property.
fn parent_plain_target(vm: &mut Vm, a: &Args) -> R<(ObjRef, Rc<crate::class::PropInfo>)> {
    let o = a.this.clone().expect("hook stand-in without $this");
    let n = a.func.name.as_bytes();
    let n = &n[1..n.len() - 5];
    let cls = vm.class_of(&o);
    let idx = cls
        .prop_map
        .borrow()
        .get(&PhpStr::new(n))
        .and_then(|v| v.first().copied());
    let p = cls.props.borrow()[idx.expect("parent hook property") as usize].clone();
    Ok((o, p))
}

fn parent_plain_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, p) = parent_plain_target(vm, a)?;
    let v = o.obj().slots.borrow()[p.slot as usize].clone();
    if vm.is_uninit(&v) {
        if p.ty.is_some() {
            return vm.throw(
                "Error",
                &format!(
                    "Typed property {}::${} must not be accessed before initialization",
                    vm.class_of(&o).name.dname(),
                    p.name.to_string_lossy()
                ),
            );
        }
        return Ok(Value::Null);
    }
    Ok(v.into_deref())
}

fn parent_plain_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, p) = parent_plain_target(vm, a)?;
    let v = a.vals[0].clone().into_deref();
    let v = match &p.ty {
        Some(t) => {
            let cls = vm.class_of(&o);
            vm.coerce_prop(&p, t, v, &cls)?
        }
        None => v,
    };
    let slots_ptr = o.obj().slots.as_ptr();
    let slot = unsafe { &mut (&mut *slots_ptr)[p.slot as usize] };
    if vm.is_uninit(slot) {
        let old = std::mem::replace(slot, v.clone());
        drop(old);
    } else {
        let old = slot.assign(v.clone());
        drop(old);
    }
    Ok(v)
}

impl Vm {
    // ------------------------------------------------------------ pending calls

    pub(crate) fn push_callee(&mut self, c: Callee, flags: u32) {
        if c.closure.is_none() && c.magic_name.is_none() && !c.func.is_native() {
            self.push_user(c.func, c.this, c.static_class, c.scope, flags);
            return;
        }
        let mut fl = flags;
        let (base, mark, args, byref, np) = match &c.func.kind {
            FuncKind::User(code) if c.magic_name.is_none() => {
                const SLOW: u32 = fflags::DEPRECATED
                    | fflags::ABSTRACT
                    | fflags::NODISCARD
                    | fflags::GENERATOR
                    | fflags::VARIADIC;
                if code.flags & SLOW != 0 || code.file.is_empty() {
                    fl |= pending_flags::SLOW;
                }
                let (b, m) = self.ex.stack.alloc(code.num_slots as usize);
                (b, m, Vec::new(), code.byref_mask, code.direct_params)
            }
            FuncKind::User(_) => (
                std::ptr::null_mut(),
                StackMark::default(),
                self.take_args_vec(),
                0,
                0,
            ),
            FuncKind::Native { info, .. } => {
                let byref = if c.magic_name.is_some() {
                    0
                } else {
                    match info.get() {
                        Some(sig) => byref_mask(&sig.params),
                        None => byref_mask(&self.native_sig(&c.func).params),
                    }
                };
                (
                    std::ptr::null_mut(),
                    StackMark::default(),
                    self.take_args_vec(),
                    byref,
                    0,
                )
            }
        };
        let statics = closure_statics(&c, base);
        if c.magic_name.is_some() {
            fl |= pending_flags::MAGIC_CALL;
        }
        // Written in place, field by field (the struct is large: built
        // whole, it would be copied).
        let v = &mut self.ex.pending;
        v.reserve(1);
        // SAFETY: capacity reserved; every field is written before the
        // length covers the entry.
        unsafe {
            use std::ptr::addr_of_mut as at;
            let d = v.as_mut_ptr().add(v.len());
            at!((*d).func).write(c.func);
            at!((*d).base).write(base);
            at!((*d).mark).write(mark);
            at!((*d).args).write(args);
            at!((*d).argc).write(0);
            at!((*d).extra).write(Vec::new());
            at!((*d).named_extra).write(None);
            at!((*d).this).write(c.this);
            at!((*d).static_class).write(c.static_class);
            at!((*d).scope).write(c.scope);
            at!((*d).closure).write(c.closure);
            at!((*d).flags).write(fl);
            at!((*d).magic_name).write(c.magic_name);
            at!((*d).statics).write(statics);
            at!((*d).has_named).write(false);
            at!((*d).passed).write(0);
            at!((*d).byref).write(byref);
            at!((*d).np).write(np);
            v.set_len(v.len() + 1);
        }
    }

    /// [`push_callee`](Self::push_callee) of a user function or method
    /// (not a closure, not a magic trampoline).
    #[inline]
    fn push_user(
        &mut self,
        func: Rc<Func>,
        this: Option<ObjRef>,
        static_class: *const Class,
        scope: *const Class,
        flags: u32,
    ) {
        let statics = &func.statics as *const RefCell<Vec<Option<PhpRef>>>;
        self.push_user_ex(func, this, static_class, scope, flags, None, statics);
    }

    /// Pushes a call of closure object `o` when it runs user code (one
    /// look at its data, which [`push_callee`](Self::push_callee) would
    /// take twice); false for other closures.
    pub(crate) fn push_user_closure(&mut self, o: &ObjRef) -> bool {
        let n = o.obj().native.borrow();
        let Some(nd) = n.as_ref() else {
            return false;
        };
        let any: &dyn std::any::Any = &**nd;
        let Some(cd) = any.downcast_ref::<ClosureData>() else {
            return false;
        };
        if cd.magic_name.is_some() {
            return false;
        }
        let FuncKind::User(code) = &cd.func.kind else {
            return false;
        };
        let base = self.push_user_ex(
            cd.func.clone(),
            cd.this.clone(),
            cd.called_scope,
            cd.scope,
            0,
            Some(o.clone()),
            &cd.statics,
        );
        for (k, (inner, _, _)) in code.uses.iter().enumerate() {
            if let Some(Some(v)) = cd.captured.get(k) {
                // SAFETY: the callee's fresh slots.
                unsafe { *base.add(*inner as usize) = Some(v.clone()) };
            }
        }
        true
    }

    /// Pushes a pending call of a user function; its slots.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn push_user_ex(
        &mut self,
        func: Rc<Func>,
        this: Option<ObjRef>,
        static_class: *const Class,
        scope: *const Class,
        flags: u32,
        closure: Option<ObjRef>,
        statics: *const RefCell<Vec<Option<PhpRef>>>,
    ) -> *mut Slot {
        let FuncKind::User(code) = &func.kind else {
            unreachable!()
        };
        let mut fl = flags;
        if code.flags & SLOW_ACTIVATION != 0 || code.file.is_empty() {
            fl |= pending_flags::SLOW;
        }
        let (base, mark) = self.ex.stack.alloc(code.num_slots as usize);
        let (byref, np) = (code.byref_mask, code.direct_params);
        let v = &mut self.ex.pending;
        v.reserve(1);
        // SAFETY: as in push_callee.
        unsafe {
            use std::ptr::addr_of_mut as at;
            let d = v.as_mut_ptr().add(v.len());
            at!((*d).func).write(func);
            at!((*d).base).write(base);
            at!((*d).mark).write(mark);
            at!((*d).args).write(Vec::new());
            at!((*d).argc).write(0);
            at!((*d).extra).write(Vec::new());
            at!((*d).named_extra).write(None);
            at!((*d).this).write(this);
            at!((*d).static_class).write(static_class);
            at!((*d).scope).write(scope);
            at!((*d).closure).write(closure);
            at!((*d).flags).write(fl);
            at!((*d).magic_name).write(None);
            at!((*d).statics).write(statics);
            at!((*d).has_named).write(false);
            at!((*d).passed).write(0);
            at!((*d).byref).write(byref);
            at!((*d).np).write(np);
            v.set_len(v.len() + 1);
        }
        base
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

    #[inline]
    pub(crate) fn pending_arg_by_ref(&mut self, arg: u32) -> bool {
        let p = self.ex.pending.last().unwrap();
        if arg < 63 {
            return p.byref >> arg & 1 != 0;
        }
        if p.byref >> 63 == 0 {
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
                        ..Cache::default()
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
        if func.is_native() {
            self.push_callee(Callee::func(func), 0);
        } else {
            let scope = func.scope.get();
            self.push_user(func, None, scope, scope, 0);
        }
        Ok(())
    }

    /// `InitDynCall`: a call of the callable in register `r`.
    pub(crate) fn op_init_dyn_call(&mut self, r: Reg, nargs: u32) -> R<()> {
        // Closures, without copying the value.
        if let Some(Value::Object(o)) = self.peek(r) {
            let o = o.clone();
            if self.push_user_closure(&o) {
                drop(o);
                self.drop_tmp(r);
                return Ok(());
            }
            if let Some(c) = self.closure_data(&o) {
                drop(o);
                self.drop_tmp(r);
                if c.func.is_native() && is_scope_introspecting(&c.func.lname) {
                    return self.throw(
                        "Error",
                        &format!("Cannot call {}() dynamically", c.func.name.dname()),
                    );
                }
                self.push_callee(c, 0);
                return Ok(());
            }
        }
        let v = self.get(r)?;
        self.init_callable_call(v, nargs)
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
            other => {
                // Observed on PHP 8.5: "Value of type int is not callable".
                let msg = format!(
                    "Value of type {} is not callable",
                    other.type_name_for_error()
                );
                self.throw("Error", &msg)
            }
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
                    // a private method is named after its class (test
                    // tests/classes/private_003b)
                    if m.flags & mflags::PRIVATE != 0 {
                        m.scope()
                            .map(|c| c.name.dname())
                            .unwrap_or_else(|| cls.name.dname())
                    } else {
                        cls.name.dname()
                    },
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
        if cls.lname.as_bytes() == b"__php_incomplete_class" {
            return self.throw(
                "Error",
                &crate::builtins::serialize::incomplete_msg("call a method", o),
            );
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
        // Static native methods of classes asking for it see the object
        // they are called on (NativeHooks::static_this).
        let is_static = m.flags & mflags::STATIC != 0 && !(m.is_native() && cls.native.static_this);
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
                        if m.flags & mflags::PRIVATE != 0 {
                            m.scope()
                                .map(|c| c.name.dname())
                                .unwrap_or_else(|| cls.name.dname())
                        } else {
                            cls.name.dname()
                        },
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
                                &format!(
                                    "Call to a member function {}() on {}",
                                    n,
                                    Vm::type_name_given(&other)
                                ),
                            );
                        }
                    }
                }
            }
        };
        // Inline cache: same class, constant name.
        let cls_ptr = self.class_ptr_of(&o);
        if is_const(name) {
            let c = self.code_ref().caches[cache as usize].get().way(cls_ptr as usize);
            if c.key == cls_ptr as usize && c.key != 0 {
                let fp = c.a as *const Func;
                let f = unsafe {
                    Rc::increment_strong_count(fp);
                    Rc::from_raw(fp)
                };
                let is_static = f.flags & mflags::STATIC != 0
                    && !(f.is_native() && unsafe { (*cls_ptr).native.static_this });
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
        // `$closure->__invoke(...)` calls the closure itself, by-reference
        // parameters included (test Zend/tests/closures/
        // closure_invoke_case_insensitive).
        if n.as_bytes().eq_ignore_ascii_case(b"__invoke")
            && let Some(cd) = self.closure_data(&o)
        {
            self.push_callee(cd, pending_flags::INTERNAL_CALLER);
            return Ok(());
        }
        let cls = self.class_of(&o);
        let cb = self.resolve_method(&o, &cls, &n, true)?;
        let check_init = cls.has_flag(crate::class::cflags::CHECK_INIT);
        if check_init {
            crate::builtins::spl::check_init(self, &o, &cb.func)?;
        }
        if is_const(name) && cb.magic_name.is_none() && cb.closure.is_none() && !check_init {
            Cache::store_class(
                &self.code_ref().caches[cache as usize],
                cls_ptr as usize,
                Rc::as_ptr(&cb.func) as usize,
                0,
            );
        }
        self.push_callee(cb, 0);
        Ok(())
    }

    #[inline]
    pub(crate) fn class_ptr_of(&self, o: &ObjRef) -> *const Class {
        o.class_addr() as *const Class
    }

    /// `parent::$prop::get()` / `parent::$prop::set()`: calls the parent
    /// class's hook, or a stand-in reading or writing the backing value
    /// when the parent property is plain (RFC: Property hooks).
    pub(crate) fn op_init_parent_hook(&mut self, name: Reg, set: bool) -> R<()> {
        let Some(o) = self.this_obj() else {
            return self.throw("Error", "Using $this when not in object context");
        };
        let scope = self.scope_ptr();
        if scope.is_null() {
            return self.throw(
                "Error",
                "Cannot use \"parent\" when no class scope is active",
            );
        }
        let Some(parent) = unsafe { &*scope }.parent.clone() else {
            return self.throw(
                "Error",
                "Cannot use \"parent\" when current class scope has no parent",
            );
        };
        let n = self.get(name)?;
        let n = self.to_php_str(&n)?;
        let pi = parent
            .prop_map
            .borrow()
            .get(&n)
            .and_then(|v| v.first().copied())
            .map(|i| parent.props.borrow()[i as usize].clone());
        let Some(pi) = pi else {
            return self.throw(
                "Error",
                &format!(
                    "Undefined property {}::${}",
                    parent.name.dname(),
                    n.to_string_lossy()
                ),
            );
        };
        if pi.flags & mflags::PRIVATE != 0 && !std::ptr::eq(pi.class, scope) {
            return self.throw(
                "Error",
                &format!(
                    "Cannot access private property {}::${}",
                    unsafe { (*pi.class).name.dname() },
                    n.to_string_lossy()
                ),
            );
        }
        let hook = if set { pi.set.clone() } else { pi.get.clone() };
        let f = match hook {
            Some(h) => h,
            None => {
                let fname = format!(
                    "${}::{}",
                    n.to_string_lossy(),
                    if set { "set" } else { "get" }
                );
                let f = if set {
                    Func::native(
                        &fname,
                        "function set(mixed $value): mixed",
                        parent_plain_set,
                        mflags::PUBLIC,
                    )
                } else {
                    Func::native(
                        &fname,
                        "function get(): mixed",
                        parent_plain_get,
                        mflags::PUBLIC,
                    )
                };
                f.scope.set(Rc::as_ptr(&parent));
                Rc::new(f)
            }
        };
        let cb = Callee {
            scope: f.scope.get(),
            func: f,
            static_class: self.class_ptr_of(&o),
            this: Some(o),
            closure: None,
            magic_name: None,
        };
        self.push_callee(cb, 0);
        Ok(())
    }

    pub(crate) fn op_init_static_call(
        &mut self,
        class: ClassSpec,
        name: Reg,
        nargs: u32,
        cache: u32,
    ) -> R<()> {
        let _ = nargs;
        let forwarding = matches!(
            class,
            ClassSpec::SelfRef | ClassSpec::Parent | ClassSpec::Static
        );
        // Inline cache: (class, public method) of a constant name. Names
        // resolve to the same class for the rest of the request.
        let cacheable = is_const(name) && !matches!(class, ClassSpec::Dynamic(_));
        if cacheable {
            let c = self.code_ref().caches[cache as usize].get();
            if c.key != 0 {
                let cls_ptr: *const Class = match class {
                    ClassSpec::Named(_) => c.key as *const Class,
                    ClassSpec::SelfRef => self.scope_ptr(),
                    ClassSpec::Parent => {
                        let s = self.scope_ptr();
                        if s.is_null() {
                            s
                        } else {
                            unsafe { (*s).parent.as_ref() }.map_or(std::ptr::null(), Rc::as_ptr)
                        }
                    }
                    _ => self.static_class_ptr(),
                };
                if cls_ptr as usize == c.key {
                    let fp = c.a as *const Func;
                    // SAFETY: methods live as long as their class.
                    let f = unsafe {
                        Rc::increment_strong_count(fp);
                        Rc::from_raw(fp)
                    };
                    if let Some(cb) = self.cached_static_callee(f, cls_ptr, forwarding) {
                        self.push_callee(cb, 0);
                        return Ok(());
                    }
                }
            }
        }
        let cls = self.resolve_class_spec(class, true)?;
        let n = self.get(name)?;
        let n = match n {
            Value::Str(s) => s,
            _ => return self.throw("Error", "Method name must be a string"),
        };
        if cls.is_trait() && !forwarding {
            // PHP 8.1 (tests Zend/tests/traits/direct_static_member_access,
            // gh_17728).
            self.deprecated(&format!(
                "Calling static trait method {}::{} is deprecated, it should only be called on a class using the trait",
                cls.name.dname(),
                n.to_string_lossy()
            ))?;
        }
        let cb = self.resolve_static_method(&cls, &n, true, forwarding)?;
        if cacheable
            && cb.magic_name.is_none()
            && cb.closure.is_none()
            && cb.func.flags & mflags::PUBLIC != 0
            && cb.func.flags & mflags::ABSTRACT == 0
            && cls
                .find_method_str(&crate::class::lower(&n))
                .is_some_and(|m| Rc::ptr_eq(&m, &cb.func))
        {
            self.code_ref().caches[cache as usize].set(Cache {
                key: Rc::as_ptr(&cls) as usize,
                a: Rc::as_ptr(&cb.func) as usize,
                b: 0,
                ..Cache::default()
            });
        }
        self.push_callee(cb, 0);
        Ok(())
    }

    /// The callee of a cached `C::m()` (public, not abstract `m` found in
    /// class `cls`), as [`resolve_static_method`](Self::resolve_static_method)
    /// would give it; `None` when it would raise an error.
    fn cached_static_callee(
        &mut self,
        m: Rc<Func>,
        cls: *const Class,
        forwarding: bool,
    ) -> Option<Callee> {
        // SAFETY: `cls` is a live class (the cache key matched it).
        let cls_ref = unsafe { &*cls };
        if m.flags & mflags::STATIC != 0 {
            let cur = self.static_class_ptr();
            let sc = if forwarding && !cur.is_null() && unsafe { &*cur }.is_subclass_of(cls_ref) {
                cur
            } else {
                cls
            };
            return Some(Callee {
                scope: m.scope.get(),
                func: m,
                this: None,
                static_class: sc,
                closure: None,
                magic_name: None,
            });
        }
        let t = self.this_obj()?;
        let tc = self.class_ptr_of(&t);
        if unsafe { &*tc }.is_subclass_of(cls_ref) {
            return Some(Callee {
                scope: m.scope.get(),
                func: m,
                this: Some(t),
                static_class: tc,
                closure: None,
                magic_name: None,
            });
        }
        None
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
            if arg < p.np {
                let old = unsafe { (*p.base.add(arg as usize)).replace(v) };
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

    /// The slot of by-value argument `arg` of the pending call when it is
    /// a parameter of a user function (the common case of a send).
    #[inline(always)]
    fn direct_arg_slot(&mut self, arg: u32) -> Option<*mut Slot> {
        let p = self.ex.pending.last_mut()?;
        if arg < 63 && p.byref >> arg & 1 == 0 && arg < p.np && !p.base.is_null() {
            p.argc = p.argc.max(arg + 1);
            p.passed |= 1 << arg;
            // SAFETY: a parameter slot of the callee.
            Some(unsafe { p.base.add(arg as usize) })
        } else {
            None
        }
    }

    /// Appends by-value argument `arg` of a native (or trampoline) call
    /// when it is the next one; gives the value back otherwise.
    #[inline(always)]
    fn push_native_arg(&mut self, arg: u32, v: Value) -> Result<(), Value> {
        let Some(p) = self.ex.pending.last_mut() else {
            return Err(v);
        };
        if arg < 63 && p.byref >> arg & 1 == 0 && p.base.is_null() && arg as usize == p.args.len() {
            p.argc = p.argc.max(arg + 1);
            p.passed |= 1 << arg;
            p.args.push(v);
            Ok(())
        } else {
            Err(v)
        }
    }

    #[inline(always)]
    pub(crate) fn send_val(&mut self, arg: u32, v: Value, from_call: bool) -> R<()> {
        if let Some(s) = self.direct_arg_slot(arg) {
            let old = unsafe { (*s).replace(v) };
            drop(old);
            return Ok(());
        }
        match self.push_native_arg(arg, v) {
            Ok(()) => Ok(()),
            Err(v) => self.send_val_slow(arg, v, from_call),
        }
    }

    #[inline(never)]
    fn send_val_slow(&mut self, arg: u32, v: Value, from_call: bool) -> R<()> {
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

    #[inline]
    pub(crate) fn send_var(&mut self, var: Reg, arg: u32) -> R<()> {
        let src = self.slot_ptr(var);
        // A defined variable to a by-value parameter.
        if unsafe { (*src).is_some() }
            && let Some(s) = self.direct_arg_slot(arg)
        {
            let v = match unsafe { &*src } {
                Some(Value::Ref(r)) => r.get(),
                Some(v) => v.clone(),
                None => unreachable!(),
            };
            let old = unsafe { (*s).replace(v) };
            drop(old);
            return Ok(());
        }
        if unsafe { (*src).is_some() }
            && let Some(p) = self.ex.pending.last()
            && p.base.is_null()
        {
            let v = match unsafe { &*src } {
                Some(Value::Ref(r)) => r.get(),
                Some(v) => v.clone(),
                None => unreachable!(),
            };
            return match self.push_native_arg(arg, v) {
                Ok(()) => Ok(()),
                Err(v) => {
                    drop(v);
                    self.send_var_slow(var, arg)
                }
            };
        }
        self.send_var_slow(var, arg)
    }

    #[inline(never)]
    fn send_var_slow(&mut self, var: Reg, arg: u32) -> R<()> {
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
                self.pending().argc = argc;
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
        let ret = if dst == NONE {
            std::ptr::null_mut()
        } else {
            self.slot_ptr(dst)
        };
        let ret_var = dst != NONE && dst & TMP == 0;
        let top = self.ex.pending.last().expect("DoCall without pending call");
        if top.flags & (pending_flags::NO_CTOR | pending_flags::MAGIC_CALL) == 0 {
            if let FuncKind::User(_) = &top.func.kind {
                self.activate(ret, ret_var, false)?;
                return Ok(None);
            }
            let is_new = top.flags & pending_flags::NEW != 0;
            let parts = self.take_native_pending();
            let v = self.call_native_parts(parts, false)?;
            if !is_new && !ret.is_null() {
                self.put(dst, v);
            }
            return Ok(None);
        }
        let p = self.ex.pending.pop().unwrap();
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
                self.activate_slow(p, ret, ret_var, false)?;
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
        // Diagnostics about the call (#[\Deprecated], #[\NoDiscard]) name
        // the method asked for.
        let saved = self.magic_call_name.replace(name.clone());
        let r = self.call_callee(c, vec![Value::Str(name), Value::Array(arr)]);
        self.magic_call_name = saved;
        r
    }

    /// Runs a native function on the pending call's arguments.
    fn call_native_pending(&mut self, p: Pending) -> R<Value> {
        self.call_native_pending_ex(p, false)
    }

    fn call_native_pending_ex(&mut self, mut p: Pending, dynamic: bool) -> R<Value> {
        let parts = NativeParts {
            func: p.func.clone(),
            args: std::mem::take(&mut p.args),
            named: p.named_extra.take(),
            this: p.this.take(),
            static_class: p.static_class,
            passed: p.passed,
            has_named: p.has_named,
        };
        self.give_args_vec(std::mem::take(&mut p.extra));
        drop(p);
        self.call_native_parts(parts, dynamic)
    }

    /// Pops the pending (native) call on top, without moving it whole.
    #[inline]
    fn take_native_pending(&mut self) -> NativeParts {
        let len = self.ex.pending.len();
        // SAFETY: the entry on top is read out field by field and then
        // forgotten: every owning field is moved out or dropped here.
        unsafe {
            let p = self.ex.pending.as_mut_ptr().add(len - 1);
            self.ex.pending.set_len(len - 1);
            let parts = NativeParts {
                func: std::ptr::read(&(*p).func),
                args: std::ptr::read(&(*p).args),
                named: std::ptr::read(&(*p).named_extra),
                this: std::ptr::read(&(*p).this),
                static_class: (*p).static_class,
                passed: (*p).passed,
                has_named: (*p).has_named,
            };
            drop(std::ptr::read(&(*p).closure));
            drop(std::ptr::read(&(*p).magic_name));
            let extra = std::ptr::read(&(*p).extra);
            if extra.capacity() != 0 {
                self.give_args_vec(extra);
            }
            parts
        }
    }

    fn call_native_parts(&mut self, parts: NativeParts, dynamic: bool) -> R<Value> {
        let NativeParts {
            func,
            mut args,
            named,
            this,
            static_class,
            passed,
            has_named,
        } = parts;
        let FuncKind::Native { handler, info, .. } = &func.kind else {
            unreachable!()
        };
        let handler = *handler;
        let sig = match info.get() {
            Some(s) => s.clone(),
            None => self.native_sig(&func),
        };
        // The call is on the stack while its arguments are checked: their
        // errors show it in traces (observed on PHP 8.5; tests
        // Zend/tests/exceptions/exception_018 and others).
        self.ex.native_calls.push(super::NativeCall {
            func: func.clone(),
            args: &args as *const Vec<Value>,
            this: this.clone(),
            depth: self.ex.frames.len() as u32,
            line: self.cur_line(),
        });
        if let Err(e) = self.prepare_native_args(&func, &sig, &mut args, passed, has_named) {
            self.ex.native_calls.pop();
            self.give_args_vec(args);
            return Err(e);
        }
        if dynamic && is_scope_introspecting(&func.lname) {
            self.ex.native_calls.pop();
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
            let r = self.throw(
                "ArgumentCountError",
                &format!(
                    "{}() does not accept unknown named parameters",
                    func.display_name()
                ),
            );
            self.ex.native_calls.pop();
            self.give_args_vec(args);
            return r;
        }
        // (the arguments may have moved while being checked)
        if let Some(nc) = self.ex.native_calls.last_mut() {
            nc.args = &args as *const Vec<Value>;
        }
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
        // ext/pcntl: signals queued while async delivery is on run here,
        // with the call still on the stack (test Zend/tests/fibers/
        // signal-async.phpt).
        let r = match r {
            Ok(v) => crate::builtins::pcntl::after_native_call(self).map(|_| v),
            e => e,
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
        args: &mut [Value],
        passed: u64,
        has_named: bool,
    ) -> R<()> {
        let np = sig.params.len();
        let n = args.len();
        // The name for messages, made only when one is raised.
        let fname = || func.display_name();
        let plain: &[u8] = if func.scope().is_none() {
            func.name.as_bytes()
        } else {
            b""
        };
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
                                    fname(),
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
                                    fname(),
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
                        fname(),
                        n + 1,
                        p.name.dname()
                    ),
                );
            }
            return self.throw(
                "ArgumentCountError",
                &format!(
                    "{}() expects {} {} argument{}, {} given",
                    fname(),
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
                                fname(),
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
                && matches!(plain, b"get_parent_class" | b"get_class_methods")
                && !matches!(args[0], Value::Object(_) | Value::Str(_))
            {
                return self.throw(
                    "TypeError",
                    &format!(
                        "{}(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
                        fname(),
                        Vm::type_name_given(&args[0])
                    ),
                );
            }
            // ini_set()'s value names no given type (observed on PHP 8.5).
            if i == 1
                && plain == b"ini_set"
                && matches!(args[1], Value::Array(_) | Value::Resource(_))
            {
                return self.throw(
                    "TypeError",
                    "ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null",
                );
            }
            // strtr()'s $from is an array with two arguments and a string
            // with three (observed on PHP 8.5).
            if i == 1 && plain == b"strtr" {
                let want = if n == 2 { "array" } else { "string" };
                let bad = match &args[1] {
                    Value::Array(_) => n != 2,
                    Value::Null => false,
                    Value::Str(_) => n == 2,
                    Value::Resource(_) => true,
                    _ => n == 2,
                };
                if bad {
                    return self.throw(
                        "TypeError",
                        &format!(
                            "strtr(): Argument #2 ($from) must be of type {}, {} given",
                            want,
                            Vm::type_name_given(&args[1])
                        ),
                    );
                }
            }
            if args[i].is_null() && !ty.allows_null() {
                // Unions with classes too (observed on PHP 8.5:
                // ReflectionClass|string takes "" with a deprecation).
                let scalar =
                    ty.mask & (tmask::BOOL | tmask::INT | tmask::FLOAT | tmask::STRING) != 0;
                if scalar && !strict {
                    self.deprecated(&format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        fname(),
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
            // Exact scalar and array matches need no check (callable
            // parameters may still deprecate a string or array).
            let m = ty.mask;
            if m & tmask::CALLABLE == 0
                && match &args[i] {
                    Value::Int(_) => m & tmask::INT != 0,
                    Value::Float(_) => m & tmask::FLOAT != 0,
                    Value::Str(_) => m & tmask::STRING != 0,
                    Value::Array(_) => m & tmask::ARRAY != 0,
                    Value::Bool(true) => m & tmask::TRUE != 0,
                    Value::Bool(false) => m & tmask::FALSE != 0,
                    _ => false,
                }
            {
                continue;
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
                            fname(),
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
                            fname(),
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
    ///
    /// The call is the pending call on top. The common case (positional
    /// arguments matching the parameters, types matching exactly, no
    /// attributes to act on) builds the frame in place; anything else goes
    /// through [`activate_slow`](Self::activate_slow).
    pub(crate) fn activate(&mut self, ret: *mut Slot, ret_var: bool, entry: bool) -> R<()> {
        let len = self.ex.pending.len();
        // SAFETY: there is a pending call on top; it stays there (and keeps
        // the function and its code alive) until truncated below.
        let p: &Pending = unsafe { &*self.ex.pending.as_ptr().add(len - 1) };
        let FuncKind::User(code) = &p.func.kind else {
            unreachable!()
        };
        let code: &FuncCode = unsafe { &*Rc::as_ptr(code) };
        // Extra arguments make argc exceed np; named ones set has_named;
        // magic trampolines take values (not here: do_call checks).
        if p.flags & pending_flags::SLOW != 0
            || p.has_named
            || p.argc > p.np
            || !args_bind_simply(self, code, p.base, p.argc)
        {
            let p = self.ex.pending.pop().unwrap();
            return self.activate_slow(p, ret, ret_var, entry);
        }
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
            p.func.scope.get()
        };
        // The pending call's handles move into the frame; its other owning
        // fields are empty (checked above), so it is popped without a drop.
        debug_assert!(
            p.extra.is_empty()
                && p.args.capacity() == 0
                && p.named_extra.is_none()
                && p.magic_name.is_none()
        );
        let base = p.base;
        // SAFETY: the handles are moved out of the pending entry, which
        // set_len then forgets.
        unsafe {
            enter_simple_frame!(
                self,
                code,
                base,
                p.argc,
                pending_base: (len - 1) as u32,
                func: std::ptr::read(&p.func),
                this: std::ptr::read(&p.this),
                static_class: p.static_class,
                scope: scope,
                ret: ret,
                closure: std::ptr::read(&p.closure),
                flags: flags,
                mark: p.mark,
                statics: p.statics
            );
            self.ex.pending.set_len(len - 1);
        }
        Ok(())
    }

    /// [`activate`](Self::activate) in general: argument count errors,
    /// coercions, named and extra arguments, variadics, generators.
    pub(crate) fn activate_slow(
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
        if code.flags & (fflags::DEPRECATED | fflags::ABSTRACT | fflags::NODISCARD) != 0
            && let Err(e) = self.activate_checks(code, &func)
        {
            self.discard_pending(p);
            return Err(e);
        }
        let strict = code.flags & fflags::TYPED_PARAMS != 0 && self.caller_strict();
        // The call site, for binding errors ("called in %s on line %d"):
        // only computed when the binding can fail.
        // call_user_func() calls as its caller does (PHP compiles it to a
        // direct call; tests Zend/tests/func_get_arg_basic and others).
        let via_cuf = entry
            && p.argc < code.num_required
            && self.ex.native_calls.last().is_some_and(|nc| {
                nc.depth == self.ex.frames.len() as u32
                    && nc.func.scope().is_none()
                    && matches!(
                        nc.func.lname.as_bytes(),
                        b"call_user_func" | b"call_user_func_array"
                    )
            });
        let call_loc = if via_cuf {
            Some((self.cur_file(), self.cur_line()))
        } else if entry
            || p.flags & pending_flags::INTERNAL_CALLER != 0
            || self.ex.frames.is_empty()
            || (p.argc >= code.num_required
                && !p.has_named
                && code.flags & fflags::TYPED_PARAMS == 0)
        {
            None
        } else {
            Some((self.cur_file(), self.cur_line()))
        };
        let argc = p.argc;
        let named_hi = if p.has_named {
            (64 - p.passed.leading_zeros()).min(code.params.len() as u32)
        } else {
            0
        };
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
        // Argument errors of internal classes written in PHP are located at
        // the call, as an internal function's are (test
        // ext/spl/tests/AppendIterator_invalid_ctor).
        let saved_loc = if code.file.is_empty() {
            self.loc_override.take()
        } else {
            self.loc_override.replace(code as *const FuncCode)
        };
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
        // With named arguments, func_num_args() counts up to the last
        // parameter passed, skipped ones taking their defaults (observed on
        // PHP 8.5).
        if named_hi > argc {
            self.frame_mut().argc = named_hi;
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

    /// The rare checks of a call: #[\\Deprecated], abstract methods,
    /// #[\\NoDiscard].
    fn activate_checks(&mut self, code: &FuncCode, func: &Rc<Func>) -> R<()> {
        if code.flags & fflags::DEPRECATED != 0 {
            let what = if func.scope().is_some() {
                "Method"
            } else {
                "Function"
            };
            let magic = self.magic_call_name.clone().filter(|_| is_magic_call(func));
            self.raise_func_deprecation(code, what, func, magic.as_ref())?;
        }
        if code.flags & fflags::ABSTRACT != 0 {
            // Through __call()/__callStatic(): the name called (tests
            // Zend/tests/gh_17718_*).
            let shown = match (
                self.magic_call_name.clone().filter(|_| is_magic_call(func)),
                func.scope(),
            ) {
                (Some(n), Some(c)) => format!("{}::{}", c.name.dname(), n.to_string_lossy()),
                _ => func.display_name(),
            };
            return self.throw("Error", &format!("Cannot call abstract method {}()", shown));
        }
        if code.flags & fflags::NODISCARD != 0 && self.caller_discards_result() {
            let what = if func.scope().is_some() {
                "method"
            } else {
                "function"
            };
            self.raise_nodiscard(code, what, func)?;
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
        // Internal functions check the argument count before the types
        // (test ext/spl/tests/CallbackFilterIteratorTest-002).
        if internal && !has_named && (argc as usize) < code.num_required as usize {
            let exact = code.num_required as usize == np && code.flags & fflags::VARIADIC == 0;
            let n = code.num_required;
            let fname = self.func_display_name(func);
            return self.throw(
                "ArgumentCountError",
                &format!(
                    "{}() expects {} {} argument{}, {} given",
                    fname,
                    if exact { "exactly" } else { "at least" },
                    n,
                    if n == 1 { "" } else { "s" },
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
        // (Closure `use` variables were bound by push_callee.)
        Ok(())
    }

    /// The `since` and `message` of a `#[\\Deprecated]` attribute among
    /// `attrs` (none without one), checked like the arguments of
    /// `Deprecated::__construct(?string $message = null, ?string $since =
    /// null)` (tests Zend/tests/attributes/deprecated/type_validation_*).
    /// Empty strings count as absent.
    pub(crate) fn deprecation_detail(
        &mut self,
        attrs: &[AttrInfo],
        scope: *const Class,
        strict: bool,
    ) -> R<Option<(Option<String>, Option<String>)>> {
        let Some(a) = attrs
            .iter()
            .find(|a| {
                a.name
                    .strip_prefix(b"\\")
                    .unwrap_or(&a.name)
                    .eq_ignore_ascii_case(b"Deprecated")
            })
            .cloned()
        else {
            return Ok(None);
        };
        self.deprecating.push(attrs.as_ptr() as usize);
        let r = self.attr_string_args(&a, "Deprecated", &["message", "since"], scope, strict);
        self.deprecating.pop();
        let mut v = r?;
        let since = v.pop().flatten();
        let message = v.pop().flatten();
        Ok(Some((since, message)))
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
            Check::Fail
                if code.file.is_empty()
                    && ty.mask & !tmask::NULL == tmask::CALLABLE
                    && ty.classes.is_empty() =>
            {
                // Internal classes written in PHP word callable errors as
                // internal functions do (test
                // ext/spl/tests/CallbackFilterIteratorTest-002).
                let why = self.callable_error(&val);
                let msg = format!(
                    "{}(): Argument #{} (${}) must be a valid callback{}, {}",
                    self.func_display_name(func),
                    i + 1,
                    code.params[i].name.dname(),
                    if ty.allows_null() { " or null" } else { "" },
                    why
                );
                self.throw("TypeError", &msg)
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
        let len = self.ex.frames.len();
        // SAFETY: a frame is running.
        let f = unsafe { &mut *self.ex.frames.as_mut_ptr().add(len - 1) };
        let flags = f.flags;
        if flags & frame_flags::GENERATOR != 0 {
            return self.generator_return(v);
        }
        let ret = f.ret;
        // Deliver before releasing locals (as PHP copies the value first).
        let v = if by_ref { v } else { v.into_deref() };
        if flags & (frame_flags::ATTACHED | frame_flags::OWN_SLOTS) == 0 {
            // Released in place: the locals, then the frame's handles.
            let n = f.code().num_slots as usize;
            let base = f.base;
            let mark = f.mark;
            for i in 0..n {
                // SAFETY: the frame's slots.
                let s = unsafe { &mut *base.add(i) };
                if super::exec::holds_no_resources(s) {
                    unsafe { std::ptr::write(s, None) };
                } else {
                    let old = s.take();
                    drop(old);
                }
            }
            self.ex.stack.release(mark);
            self.ex.frames.truncate(len - 1);
            match self.ex.frames.last() {
                Some(top) => {
                    self.ex.top = super::TopFrame::of(top.base, top.code);
                    self.ex.ip = top.ip as usize;
                }
                None => self.ex.top = super::TopFrame::default(),
            }
        } else {
            let f = self.ex.frames.pop().unwrap();
            self.ex.sync_top();
            if let Some(top) = self.ex.frames.last() {
                self.ex.ip = top.ip as usize;
            }
            self.release_frame(f);
        }
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
                } else if super::exec::holds_no_resources(&*ret) {
                    std::ptr::write(ret, Some(v));
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
                (Some(Value::Object(o)), m) => {
                    m & tmask::OBJECT != 0 || names_class_of(self, ty, o)
                }
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
                // static names the called class (test Zend/tests/
                // type_declarations/static_type_return).
                let mut shown = ty.display();
                if ty.mask & tmask::STATIC != 0 && !static_class.is_null() {
                    let cn = unsafe { (*static_class).name.dname() };
                    shown = shown
                        .split('|')
                        .map(|p| match p {
                            "static" => cn.clone(),
                            "?static" => format!("?{}", cn),
                            o => o.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("|");
                }
                if none && ty.mask & tmask::NULL == 0 {
                    return self.type_error(&format!(
                        "{}(): Return value must be of type {}, none returned",
                        fname, shown
                    ));
                }
                self.type_error(&format!(
                    "{}(): Return value must be of type {}, {} returned",
                    fname,
                    shown,
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
        self.send_rust_args(args, magic)?;
        self.finish_rust_call()
    }

    /// Calls a prepared callable (see [`PreparedCall`]) with positional
    /// arguments, as [`call_callee`](Self::call_callee) does.
    #[inline]
    pub fn call_prepared<I>(&mut self, pc: &mut PreparedCall, args: I) -> R<Value>
    where
        I: IntoIterator<Item = Value>,
        I::IntoIter: ExactSizeIterator,
    {
        self.call_prepared_raw(pc, args).map(|v| v.into_deref())
    }

    /// [`call_prepared`](Self::call_prepared), keeping a reference returned
    /// by a by-reference function.
    pub fn call_prepared_raw<I>(&mut self, pc: &mut PreparedCall, args: I) -> R<Value>
    where
        I: IntoIterator<Item = Value>,
        I::IntoIter: ExactSizeIterator,
    {
        if pc.callee.is_none() {
            let c = self.resolve_callable(&pc.value, true)?;
            pc.direct = c.magic_name.is_none()
                && match &c.func.kind {
                    FuncKind::User(code) => {
                        code.flags & SLOW_ACTIVATION == 0 && !code.file.is_empty()
                    }
                    FuncKind::Native { .. } => false,
                };
            if pc.direct
                && let Some(cl) = &c.closure
            {
                let n = cl.obj().native.borrow();
                let any: Option<&dyn std::any::Any> = n.as_ref().map(|b| &**b as _);
                match any.and_then(|a| a.downcast_ref::<ClosureData>()) {
                    Some(cd) => pc.cd = cd,
                    None => pc.direct = false,
                }
            }
            pc.callee = Some(c);
        }
        let args = args.into_iter();
        let c = pc.callee.as_ref().unwrap();
        if pc.direct
            && let FuncKind::User(code) = &c.func.kind
        {
            // By-value parameters only (references go through the general
            // path, which also warns for values).
            let n = args.len();
            if n < 64 && n as u32 <= code.direct_params && code.byref_mask & ((1u64 << n) - 1) == 0
            {
                let c = c.clone();
                return self.call_direct(c, pc.cd, args);
            }
        }
        let c = c.clone();
        let magic = c.magic_name.is_some();
        self.push_callee(c, 0);
        self.send_rust_args(args, magic)?;
        self.finish_rust_call()
    }

    /// Enters a user function from Rust without a pending call: the
    /// arguments go straight into the callee's slots. Arguments needing
    /// coercion or checks take the general path.
    fn call_direct<I: Iterator<Item = Value>>(
        &mut self,
        c: Callee,
        cd: *const ClosureData,
        args: I,
    ) -> R<Value> {
        // Diagnostics queued by a write path come out before user code runs.
        if !self.err.queued.is_empty() {
            self.flush_deferred()?;
        }
        let FuncKind::User(code) = &c.func.kind else {
            unreachable!()
        };
        // SAFETY: `c.func` keeps the code alive until the frame (which
        // takes it over) starts.
        let code: &FuncCode = unsafe { &*Rc::as_ptr(code) };
        let n = code.num_slots as usize;
        let (base, mark) = self.ex.stack.alloc(n);
        let mut argc = 0u32;
        for a in args {
            // SAFETY: fresh (empty) parameter slots of the callee.
            unsafe { std::ptr::write(base.add(argc as usize), Some(a.into_deref())) };
            argc += 1;
        }
        let statics = if cd.is_null() {
            closure_statics(&c, base)
        } else {
            // SAFETY: see `PreparedCall::cd`.
            let cd = unsafe { &*cd };
            for (k, (inner, _, _)) in code.uses.iter().enumerate() {
                if let Some(Some(v)) = cd.captured.get(k) {
                    // SAFETY: the callee's fresh slots.
                    unsafe { *base.add(*inner as usize) = Some(v.clone()) };
                }
            }
            &cd.statics
        };
        if !args_bind_simply(self, code, base, argc) {
            let mut v = Vec::with_capacity(argc as usize);
            for i in 0..n {
                // SAFETY: the slots allocated above.
                let s = unsafe { (*base.add(i)).take() };
                if i < argc as usize {
                    v.push(s.unwrap_or_default());
                }
            }
            self.ex.stack.release(mark);
            return self.call_callee_raw(c, v);
        }
        let saved = std::mem::replace(&mut self.err.defer_depth, 0);
        let saved_loc = self.loc_override.take();
        let mut result: Slot = None;
        let scope = if c.closure.is_some() {
            c.scope
        } else {
            c.func.scope.get()
        };
        let Callee {
            func,
            this,
            static_class,
            closure,
            ..
        } = c;
        let pending_base = self.ex.pending.len() as u32;
        // SAFETY: the slots allocated above.
        unsafe {
            enter_simple_frame!(
                self,
                code,
                base,
                argc,
                pending_base: pending_base,
                func: func,
                this: this,
                static_class: static_class,
                scope: scope,
                ret: &mut result,
                closure: closure,
                flags: frame_flags::ENTRY,
                mark: mark,
                statics: statics
            );
        }
        let r = self.run();
        self.loc_override = saved_loc;
        self.err.defer_depth = saved;
        r
    }

    /// Passes the arguments of a call made from Rust to the pending call
    /// on top (discarding it on error). By-reference parameters get the
    /// given references, or a warning for values.
    #[inline]
    fn send_rust_args<I: IntoIterator<Item = Value>>(&mut self, args: I, magic: bool) -> R<()> {
        for (i, a) in args.into_iter().enumerate() {
            let i = i as u32;
            // Plain values to by-value parameters.
            let a = if matches!(a, Value::Ref(_)) {
                a
            } else if let Some(s) = self.direct_arg_slot(i) {
                let old = unsafe { (*s).replace(a) };
                drop(old);
                continue;
            } else {
                match self.push_native_arg(i, a) {
                    Ok(()) => continue,
                    Err(a) => a,
                }
            };
            if let Err(e) = self.send_rust_arg_slow(i, a, magic) {
                let p = self.ex.pending.pop().unwrap();
                self.discard_pending(p);
                return Err(e);
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn send_rust_arg_slow(&mut self, i: u32, a: Value, magic: bool) -> R<()> {
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
                    self.warn(&msg)?;
                    Value::Ref(PhpRef::new(other))
                }
            };
            self.put_arg(i, v);
        } else {
            self.put_arg(i, a.into_deref());
        }
        Ok(())
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
                self.ex.pending.push(p);
                self.activate(ret, false, true)?;
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
            if r.is_err()
                && let Some(h) = cls.native.destroy
            {
                h(self, &o);
            }
            r?;
        }
        if let Some(h) = cls.native.destroy {
            h(self, &o);
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

impl Vm {
    /// "Function f() is deprecated[ since X][, message]" for a
    /// `#[\\Deprecated]` user function (E_USER_DEPRECATED). Through
    /// `__call` the method asked for is named (test
    /// Zend/tests/attributes/deprecated/functions/magic_call.phpt).
    pub(crate) fn raise_func_deprecation(
        &mut self,
        code: &FuncCode,
        what: &str,
        func: &Func,
        magic: Option<&PhpStr>,
    ) -> R<()> {
        if self.deprecating.contains(&(code.attrs.as_ptr() as usize)) {
            return Ok(());
        }
        let strict = code.flags & fflags::STRICT != 0;
        let scope = func.scope.get();
        let detail = self.deprecation_detail(&code.attrs, scope, strict)?;
        let (since, msg) = detail.unwrap_or((None, None));
        let name = match (magic, func.scope()) {
            (Some(m), Some(c)) => format!("{}::{}", c.name.dname(), m.to_string_lossy()),
            _ => func.display_name(),
        };
        let text = super::include::deprecation_text(&format!("{} {}()", what, name), since, msg);
        self.raise(super::errors::E_USER_DEPRECATED, &text)
    }
}

impl Vm {
    /// Whether the running user code's current call has its result
    /// discarded by an expression statement (directly, or through an
    /// internal function such as call_user_func()).
    pub(crate) fn caller_discards_result(&self) -> bool {
        if self.ex.frames.is_empty() {
            return false;
        }
        let code = self.code_ref();
        if code.discarded_calls.is_empty() {
            return false;
        }
        let ip = self.ex.ip.saturating_sub(1) as u32;
        code.discarded_calls.binary_search(&ip).is_ok()
    }

    /// "The return value of function f() should either be used or
    /// intentionally ignored by casting it as (void)[, message]"
    /// (E_USER_WARNING; RFC: Marking return values as important).
    fn raise_nodiscard(&mut self, code: &FuncCode, what: &str, func: &Func) -> R<()> {
        let strict = code.flags & fflags::STRICT != 0;
        let msg = match code.attrs.iter().find(|a| {
            a.name
                .strip_prefix(b"\\")
                .unwrap_or(&a.name)
                .eq_ignore_ascii_case(b"NoDiscard")
        }) {
            Some(a) => {
                let a = a.clone();
                self.attr_string_args(&a, "NoDiscard", &["message"], func.scope.get(), strict)?
                    .pop()
                    .flatten()
            }
            None => None,
        };
        let magic = self.magic_call_name.clone().filter(|_| is_magic_call(func));
        let name = match (magic, func.scope()) {
            (Some(m), Some(c)) => format!("{}::{}", c.name.dname(), m.to_string_lossy()),
            _ => self.func_display_name(func),
        };
        let mut text = format!(
            "The return value of {} {}() should either be used or intentionally ignored by casting it as (void)",
            what, name
        );
        if let Some(m) = msg {
            text.push_str(", ");
            text.push_str(&m);
        }
        self.raise(super::errors::E_USER_WARNING, &text)
    }

    /// The string arguments of an attribute, checked as `?string`
    /// parameters of `{class}::__construct()` named `params`.
    pub(crate) fn attr_string_args(
        &mut self,
        a: &AttrInfo,
        class: &str,
        params: &[&str],
        scope: *const Class,
        strict: bool,
    ) -> R<Vec<Option<String>>> {
        let mut out: Vec<Option<String>> = vec![None; params.len()];
        for (i, (n, ce)) in a.args.iter().enumerate() {
            let v = self.eval_const_expr(ce, scope)?;
            let pos = match n {
                Some(n) => match params.iter().position(|p| n.as_bytes() == p.as_bytes()) {
                    Some(p) => p,
                    None => {
                        return self.throw(
                            "Error",
                            &format!("Unknown named parameter ${}", n.to_string_lossy()),
                        );
                    }
                },
                None if i < params.len() => i,
                None => {
                    return self.throw(
                        "ArgumentCountError",
                        &format!(
                            "{}::__construct() expects at most {} argument{}, {} given",
                            class,
                            params.len(),
                            if params.len() == 1 { "" } else { "s" },
                            a.args.len()
                        ),
                    );
                }
            };
            let s = match &v {
                Value::Null => None,
                Value::Str(s) => Some(s.to_string_lossy()),
                Value::Int(_) | Value::Float(_) | Value::Bool(_) if !strict => {
                    let mut o = Vec::new();
                    self.append_to_string(&mut o, &v)?;
                    Some(String::from_utf8_lossy(&o).into_owned())
                }
                _ => {
                    return self.type_error(&format!(
                        "{}::__construct(): Argument #{} (${}) must be of type ?string, {} given",
                        class,
                        pos + 1,
                        params[pos],
                        Vm::type_name_given(&v)
                    ));
                }
            };
            out[pos] = s.filter(|s| !s.is_empty());
        }
        Ok(out)
    }
}

/// `__call` / `__callStatic`.
/// A native call taken off the pending stack.
struct NativeParts {
    func: Rc<Func>,
    args: Vec<Value>,
    named: Option<PhpArray>,
    this: Option<ObjRef>,
    static_class: *const Class,
    passed: u64,
    has_named: bool,
}

/// Whether the arguments in `base` bind without work: every parameter is
/// passed (or has a default the frame fills in: a constant, or prologue
/// code), and each value matches its parameter's type exactly. Pure (no
/// diagnostics, no autoloading): anything else is left to the general
/// path.
/// Whether type `ty` names the class of `o` itself (one of its plain
/// class alternatives); other matches are left to the general check.
#[inline]
fn names_class_of(vm: &Vm, ty: &TypeHint, o: &ObjRef) -> bool {
    // SAFETY: an object's class outlives it.
    let name = unsafe { &*vm.class_ptr_of(o) }.name.as_bytes();
    for t in &ty.classes {
        if let [n] = &t[..] {
            let n = n.as_bytes();
            let n = n.strip_prefix(b"\\").unwrap_or(n);
            if n.len() == name.len() && (n == name || n.eq_ignore_ascii_case(name)) {
                return true;
            }
        }
    }
    false
}

#[inline(always)]
fn args_bind_simply(vm: &Vm, code: &FuncCode, base: *mut Slot, argc: u32) -> bool {
    for (i, param) in code.params.iter().enumerate() {
        if (i as u32) >= argc {
            match &param.default {
                ParamDefault::Const(_) if !param.by_ref => {}
                ParamDefault::Code => {}
                _ => return false,
            }
            continue;
        }
        let Some(ty) = &param.ty else { continue };
        let m = ty.mask;
        let ok = match unsafe { &*base.add(i) } {
            _ if m & tmask::MIXED != 0 => true,
            Some(Value::Int(_)) => m & tmask::INT != 0,
            Some(Value::Str(_)) => m & tmask::STRING != 0,
            Some(Value::Float(_)) => m & tmask::FLOAT != 0,
            Some(Value::Array(_)) => m & (tmask::ARRAY | tmask::ITERABLE) != 0,
            Some(Value::Bool(true)) => m & tmask::TRUE != 0,
            Some(Value::Bool(false)) => m & tmask::FALSE != 0,
            Some(Value::Null) => m & tmask::NULL != 0,
            Some(Value::Object(o)) => m & tmask::OBJECT != 0 || names_class_of(vm, ty, o),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

fn is_magic_call(f: &Func) -> bool {
    matches!(f.lname.as_bytes(), b"__call" | b"__callstatic")
}
