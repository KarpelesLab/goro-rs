//! Callbacks builtins call, made at once.
//!
//! A builtin calling a callback (call_user_func(), array_map(), usort(),
//! a hook's call_user_func_array()...) resolves it as zend_is_callable()
//! does each time: the callable's form, its name lowercased, the class and
//! the method looked up, their visibility and deprecation checked; then
//! execute_fn_with_named_args() resolves the arguments (named ones, the
//! ones by reference, the typed ones and their coercions, too few of
//! them...) before the frame is set up. For the common callables (a user
//! function by its plain name, a closure, `[$obj, 'method']` of a public
//! instance method of a user class) called with arguments none of those
//! checks has anything to do with, fast_user_callback() makes the call
//! with just what the frame needs, run as a lean call's when it may be (see
//! Vm::callback_frame).

use std::cell::RefCell;
use std::rc::Rc;

use super::{CallTarget, PendingCall, Vm, VmError, clear_values, push_lossy};
use crate::object::Visibility;
use crate::rcstr::RcStr;
use crate::string::PhpString;
use crate::value::Value;

/// `b` lowercased into `buf` (None: longer than it).
#[inline]
fn lower_into<'a>(b: &[u8], buf: &'a mut [u8; 128]) -> Option<&'a [u8]> {
    if b.len() > buf.len() {
        return None;
    }
    for (d, s) in buf.iter_mut().zip(b) {
        *d = s.to_ascii_lowercase();
    }
    Some(&buf[..b.len()])
}

impl Vm {
    /// The call of `callback` with the positional arguments `args` a
    /// builtin makes (call_user_func() and the builtins calling callbacks),
    /// when it is one of the callables the module's doc names and nothing
    /// about it needs the general path. None: nothing was done (the
    /// general path takes the call).
    pub fn fast_user_callback(
        &mut self,
        callback: &Value,
        args: &[Value],
    ) -> Option<Result<Value, VmError>> {
        if !self.callback_named_args.is_empty()
            || !self.pending_named_args.is_empty()
            || self.closure_invoke_call
        {
            return None;
        }
        let mut buf = [0u8; 128];
        match callback {
            Value::String(s) => {
                let f = match self.cached_callback_fn(s, 1) {
                    Some(f) => f,
                    None => {
                        let name = s.as_bytes();
                        // (a leading `\`, a "Class::method" name: the general
                        // path)
                        if name.is_empty() || name[0] == b'\\' || name.contains(&b':') {
                            return None;
                        }
                        let lower = lower_into(name, &mut buf)?;
                        let f = self.user_functions.get(lower)?;
                        if f.scope_class.is_some()
                            || !crate::object::call_attributes_inert(&f.attributes)
                        {
                            return None;
                        }
                        let f = Rc::clone(f);
                        self.cache_callback_fn(s, 1, &f);
                        f
                    }
                };
                let lead = self.take_empty_values();
                self.fast_callback_call(&f, lead, args)
            }
            Value::Object(o) => {
                let b = o.try_borrow().ok()?;
                let (name, captures) = match b.closure.as_deref()? {
                    Value::String(n) => (n.clone(), None),
                    Value::Array(a) => {
                        let a = a.try_borrow().ok()?;
                        let mut it = a.values();
                        let Some(Value::String(n)) = it.next() else {
                            return None;
                        };
                        let mut caps = self.take_empty_values();
                        caps.extend(it.cloned());
                        (n.clone(), Some(caps))
                    }
                    _ => return None,
                };
                drop(b);
                let f = match self.cached_callback_fn(&name, 2) {
                    Some(f) => Some(f),
                    None if crate::closure::is_closure_fn_name(name.as_bytes()) => {
                        let f = lower_into(name.as_bytes(), &mut buf)
                            .and_then(|l| self.user_functions.get(l))
                            .filter(|f| crate::object::call_attributes_inert(&f.attributes))
                            .map(Rc::clone);
                        if let Some(f) = &f {
                            if !name.as_bytes().contains(&b':') {
                                self.cache_callback_fn(&name, 2, f);
                            }
                        }
                        f
                    }
                    None => None,
                };
                // (the captured values move to the call, see
                // call_callback_array)
                let caps = captures.unwrap_or_else(|| self.take_empty_values());
                match &f {
                    Some(f) => self.fast_callback_call(f, caps, args),
                    None => {
                        let mut caps = caps;
                        clear_values(&mut caps);
                        self.recycle_values(caps);
                        None
                    }
                }
            }
            Value::Array(a) => {
                let (target, method) = {
                    let a = a.try_borrow().ok()?;
                    if a.len() != 2 {
                        return None;
                    }
                    match (a.get_int(0), a.get_int(1)) {
                        (
                            Some(t @ (Value::Object(_) | Value::String(_))),
                            Some(Value::String(m)),
                        ) => (t.clone(), m.clone()),
                        _ => return None,
                    }
                };
                match &target {
                    Value::Object(obj) => self.fast_method_callback(obj, method.as_bytes(), args),
                    Value::String(class) => {
                        self.fast_static_callback(class.as_bytes(), method.as_bytes(), args)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The function fast_user_callback() found for the callback by the
    /// function name `name` (`kind` 1) or of the closure whose function is
    /// named `name` (2) last time, when it is that same string and no
    /// function was added, replaced or removed since (see FnTable).
    #[inline(always)]
    fn cached_callback_fn(&self, name: &PhpString, kind: u8) -> Option<Rc<crate::opcode::OpArray>> {
        match &self.callback_fn_cache {
            Some((n, k, len, fgen, f))
                if n.ptr_eq(name)
                    && *k == kind
                    && *len == self.user_functions.len()
                    && *fgen == crate::vm::fn_table_gen() =>
            {
                f.upgrade()
            }
            _ => None,
        }
    }

    /// Remember the function `f` found for the callback (see
    /// cached_callback_fn; `name` has no `:`).
    #[inline(never)]
    fn cache_callback_fn(&mut self, name: &PhpString, kind: u8, f: &Rc<crate::opcode::OpArray>) {
        self.callback_fn_cache = Some((
            name.clone(),
            kind,
            self.user_functions.len(),
            crate::vm::fn_table_gen(),
            Rc::downgrade(f),
        ));
    }

    /// fast_user_callback() of `[$obj, 'method']`: a public instance method
    /// of a user class, which no private method of the calling scope
    /// shadows (see call_user_func()).
    fn fast_method_callback(
        &mut self,
        obj: &Rc<RefCell<crate::object::PhpObject>>,
        method: &[u8],
        args: &[Value],
    ) -> Option<Result<Value, VmError>> {
        if method.is_empty() || method.starts_with(b"__") || method.contains(&b':') {
            return None;
        }
        let mut mbuf = [0u8; 128];
        let m = lower_into(method, &mut mbuf)?;
        let mut cbuf = [0u8; 128];
        let (f, mut scope) = {
            let o = obj.try_borrow().ok()?;
            let cl = lower_into(&o.class_name, &mut cbuf)?;
            let class = self.classes.get(cl)?;
            let md = class.methods.get(m)?;
            if class.filename.is_none()
                || md.visibility != Visibility::Public
                || md.is_static
                || md.is_abstract
                || !crate::object::call_attributes_inert(&md.attributes)
                || !crate::object::call_attributes_inert(&md.op_array.attributes)
            {
                return None;
            }
            // (a private method of the calling scope, a parent class of
            // the object's, would shadow it: the general path)
            if let Some(sc) = self.scope_top() {
                if sc.as_slice() != cl
                    && self
                        .classes
                        .get(sc.as_slice())
                        .and_then(|c| c.methods.get(m))
                        .is_some_and(|sm| sm.visibility == Visibility::Private)
                {
                    return None;
                }
            }
            let mut scope = self.bytes_pool.pop().unwrap_or_default();
            scope.clear();
            if md.declaring_class.is_empty() {
                scope.extend_from_slice(cl);
            } else {
                scope.extend(md.declaring_class.iter().map(|b| b.to_ascii_lowercase()));
            }
            (Rc::clone(&md.op_array), scope)
        };
        if f.cv_names.first().map(|n| n.as_slice()) != Some(b"this") {
            self.recycle_bytes(std::mem::take(&mut scope));
            return None;
        }
        let mut called = self.bytes_pool.pop().unwrap_or_default();
        called.clear();
        called.extend_from_slice(&obj.borrow().class_name);
        // The method runs in its declaring class's scope, with the
        // object's class as the called class.
        self.class_scope_stack.push(scope);
        self.called_class_stack.push(called);
        let mut lead = self.take_empty_values();
        lead.push(Value::Object(Rc::clone(obj)));
        let r = self.fast_callback_call(&f, lead, args);
        for v in [self.called_class_stack.pop(), self.class_scope_stack.pop()]
            .into_iter()
            .flatten()
        {
            self.recycle_bytes(v);
        }
        r
    }

    /// fast_user_callback() of `['Class', 'method']`: a public static method
    /// of a declared user class (by its name: not `self`, `parent` or
    /// `static`), called on that class.
    fn fast_static_callback(
        &mut self,
        class: &[u8],
        method: &[u8],
        args: &[Value],
    ) -> Option<Result<Value, VmError>> {
        if method.is_empty()
            || method.starts_with(b"__")
            || method.contains(&b':')
            || class.is_empty()
            || class[0] == b'\\'
            || class.contains(&b':')
        {
            return None;
        }
        let mut mbuf = [0u8; 128];
        let m = lower_into(method, &mut mbuf)?;
        let mut cbuf = [0u8; 128];
        let cl = lower_into(class, &mut cbuf)?;
        if matches!(cl, b"self" | b"parent" | b"static") {
            return None;
        }
        let (f, mut scope, mut called) = {
            let class = self.classes.get(cl)?;
            let md = class.methods.get(m)?;
            if class.filename.is_none()
                || md.visibility != Visibility::Public
                || !md.is_static
                || md.is_abstract
                || !crate::object::call_attributes_inert(&md.attributes)
                || !crate::object::call_attributes_inert(&md.op_array.attributes)
            {
                return None;
            }
            let mut scope = self.bytes_pool.pop().unwrap_or_default();
            scope.clear();
            if md.declaring_class.is_empty() {
                scope.extend_from_slice(cl);
            } else {
                scope.extend(md.declaring_class.iter().map(|b| b.to_ascii_lowercase()));
            }
            let mut called = self.bytes_pool.pop().unwrap_or_default();
            called.clear();
            called.extend_from_slice(&class.name);
            (Rc::clone(&md.op_array), scope, called)
        };
        if f.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
            self.recycle_bytes(std::mem::take(&mut scope));
            self.recycle_bytes(std::mem::take(&mut called));
            return None;
        }
        // The method runs in its declaring class's scope, with the class
        // named as the called class.
        self.class_scope_stack.push(scope);
        self.called_class_stack.push(called);
        let lead = self.take_empty_values();
        let r = self.fast_callback_call(&f, lead, args);
        for v in [self.called_class_stack.pop(), self.class_scope_stack.pop()]
            .into_iter()
            .flatten()
        {
            self.recycle_bytes(v);
        }
        r
    }

    /// The call of `f` by a callback (see fast_user_callback): with the
    /// values leading its arguments `call_args` (a method's object, a
    /// closure's captured values), then the arguments `args`, as
    /// execute_fn_with_named_args() makes it when none of its checks has
    /// anything to do. None: one has (nothing was done).
    fn fast_callback_call(
        &mut self,
        f: &Rc<crate::opcode::OpArray>,
        mut call_args: Vec<Value>,
        args: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let op_array: &crate::opcode::OpArray = f;
        // (a fiber suspended in the call owns its arguments)
        call_args.extend_from_slice(args);
        let ok = !op_array.is_generator
            && op_array.variadic_param.is_none()
            && crate::object::call_attributes_inert(&op_array.attributes);
        let lead = if ok {
            self.fast_callback_args(op_array, &mut call_args)
        } else {
            None
        };
        let Some(lead) = lead else {
            clear_values(&mut call_args);
            self.recycle_values(call_args);
            return None;
        };
        let n = call_args.len();
        let this_cv = op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
        let first_obj = matches!(call_args.first(), Some(Value::Object(_)));
        let callback_closure = self.callback_closure.take();
        // The frame's name (as execute_fn_with_named_args names it).
        let mut frame_display = self.string_pool.pop().unwrap_or_default();
        frame_display.clear();
        self.push_callback_display(op_array, &mut frame_display);
        // The variables: the parameters, extra arguments only reach
        // func_get_args().
        let mut func_cvs = self.take_slots(op_array.cv_names.len());
        let nparams = (op_array.param_count as usize).min(func_cvs.len());
        for (slot, a) in func_cvs.iter_mut().zip(&call_args[..n.min(nparams)]) {
            *slot = a.clone();
        }
        // By-value array parameters (and a closure's values captured by
        // value: each call starts from them) get their own copy.
        for slot in func_cvs.iter_mut() {
            if let Value::Array(a) = slot {
                if Rc::strong_count(a) > 1 {
                    *slot = Value::Array(crate::array::cow_copy_new(a));
                }
            }
        }
        let mut frame_args = self.take_empty_values();
        frame_args.extend(call_args.iter().skip(lead).map(|v| match v {
            Value::Reference(r) => r.borrow().clone(),
            v => v.clone(),
        }));
        let is_instance = this_cv && lead > 0 && first_obj;
        let call = PendingCall {
            name: PhpString::empty(),
            args: call_args,
            named_args: Vec::new(),
            target: CallTarget::NONE,
        };
        // Record an "[internal function]" frame for the callback (unless the
        // caller recorded it already: see execute_fn_with_named_args).
        let pushed = self.call_stack.len() <= self.exec_frame_len
            || !matches!(self.call_stack.last(), Some((n, f, 0, _, _)) if (f.is_empty() || f == "[internal function]") && *n == frame_display);
        if pushed {
            self.ucf_strict = false;
            let (file, line) = match self.ucf_call_line.take() {
                Some(l) => (self.current_file.clone(), l),
                None => match self.release_call_at {
                    Some((depth, l)) if depth == self.call_stack.len() => {
                        (self.current_file.clone(), l)
                    }
                    _ => (RcStr::new(), 0),
                },
            };
            if let Some(weak) = callback_closure {
                self.note_callback_closure_frame(&frame_display, weak);
            }
            self.call_stack
                .push((frame_display, file, line, frame_args, is_instance));
        } else {
            clear_values(&mut frame_args);
            self.recycle_values(frame_args);
            if frame_display.capacity() != 0 && self.string_pool.len() < 64 {
                self.string_pool.push(frame_display);
            }
        }
        let was_global = std::mem::replace(&mut self.is_global_scope, false);
        let scope_pushed = self.closure_frame_enter(op_array, &func_cvs);
        if std::mem::take(&mut self.callback_keep_ref) {
            self.keep_ref_depth = Some(self.call_depth + 1);
        }
        self.gc_call_args = &call.args;
        let result = self.callback_frame(op_array, func_cvs, &call);
        self.gc_call_args = std::ptr::null();
        self.closure_frame_leave(scope_pushed);
        self.is_global_scope = was_global;
        if pushed {
            if let Some(frame) = self.call_stack.pop() {
                self.recycle_frame(frame);
            }
        }
        let mut call = call;
        self.recycle_args(&mut call);
        // (what is left of the call: its name, the empty string)
        let PendingCall {
            name,
            args,
            named_args,
            target,
        } = call;
        drop(name);
        debug_assert!(args.capacity() == 0 && named_args.capacity() == 0);
        // (as made above: nothing to drop)
        std::mem::forget((args, named_args, target));
        Some(result)
    }

    /// An empty buffer for values (take_values(0), inline).
    #[inline(always)]
    fn take_empty_values(&mut self) -> Vec<Value> {
        self.value_pool.pop().unwrap_or_default()
    }

    /// fast_callback_call(): whether the arguments `args` of a call of
    /// `f` need none of execute_fn_with_named_args()'s checks (an argument
    /// not passed, too few of them, one by value to a by-reference
    /// parameter, one of a typed parameter not of its type already, `$this`
    /// expected but not passed); the number of them leading the parameters
    /// (`$this`, a closure's captured values). The ones passed by value
    /// get the values of references.
    #[inline]
    fn fast_callback_args(&self, f: &crate::opcode::OpArray, args: &mut [Value]) -> Option<usize> {
        let n = args.len();
        let this_cv = f.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
        let hidden = (f.param_count as usize).saturating_sub(f.param_attributes.len());
        let first_obj = matches!(args.first(), Some(Value::Object(_)));
        if this_cv && !first_obj && hidden > 0 {
            return None;
        }
        if n < hidden || ((n - hidden) as u32) < f.required_param_count {
            return None;
        }
        // (what callback_lean_kind() knows of them, when it is one)
        let kind = f.callback_lean_kind();
        let typed = if kind != 0 {
            kind & 4 != 0
        } else {
            f.param_types.iter().any(|t| t.is_some())
        };
        if typed && !self.pending_callable_deprecations.borrow().is_empty() {
            return None;
        }
        let by_refs = &f.by_ref_params;
        for (i, a) in args.iter_mut().enumerate() {
            if matches!(a, Value::Undef) {
                return None;
            }
            if i < hidden {
                continue;
            }
            let by_ref = by_refs.get(i).copied().unwrap_or(false);
            if let Value::Reference(r) = a {
                let ok = !typed || {
                    let v = r.try_borrow().ok()?;
                    crate::recv_fast::arg_matches_exactly(self, f, i, &v)
                };
                if !ok {
                    return None;
                }
                if !by_ref && i < by_refs.len() {
                    let v = r.borrow().clone();
                    *a = v;
                }
            } else if by_ref || (typed && !crate::recv_fast::arg_matches_exactly(self, f, i, a)) {
                return None;
            }
        }
        Some(hidden)
    }

    /// The name of a callback's frame (as execute_fn_with_named_args()
    /// names it).
    #[inline]
    pub(crate) fn push_callback_display(&self, f: &crate::opcode::OpArray, out: &mut String) {
        // (a function of the function and of its class's name, kept once
        // the class was found)
        if let Some(d) = f.callback_display().get() {
            out.push_str(d);
            return;
        }
        let start = out.len();
        if self.push_callback_display_uncached(f, out) {
            let _ = f.callback_display().set(out[start..].into());
        }
    }

    /// push_callback_display(): the name made; whether it may be kept.
    fn push_callback_display_uncached(&self, f: &crate::opcode::OpArray, out: &mut String) -> bool {
        if !f.display_name.is_empty() {
            // (a closure is qualified by its scope: Class::{closure:...})
            if let Some(sc) = f.scope_class.as_ref().filter(|_| {
                crate::closure::is_closure_fn_name(&f.name)
                    && f.display_name.starts_with(b"{closure")
            }) {
                let found = match self.classes.get(sc) {
                    Some(c) => {
                        push_lossy(out, &c.name);
                        true
                    }
                    None => {
                        out.push_str(&self.builtin_canonical_name(sc));
                        false
                    }
                };
                out.push_str("::");
                push_lossy(out, &f.display_name);
                return found;
            }
            push_lossy(out, &f.display_name);
            true
        } else if let Some(fcc) = &f.fcc_target_name {
            push_lossy(out, fcc);
            true
        } else if let Some(sc) = &f.scope_class {
            let raw = crate::string::lossy(&f.name);
            let m = raw.rsplit("::").next().unwrap_or(&raw);
            let found = match self.classes.get(sc) {
                Some(c) => {
                    push_lossy(out, &c.name);
                    true
                }
                None => {
                    push_lossy(out, sc);
                    false
                }
            };
            out.push_str("::");
            out.push_str(m);
            found
        } else {
            push_lossy(out, &f.name);
            true
        }
    }
}
