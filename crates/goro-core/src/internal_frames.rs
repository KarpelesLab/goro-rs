//! Stack frames of internal (builtin) functions.
//!
//! php-src gives every internal function call an execute_data frame, so
//! backtraces taken while it runs (exceptions it throws, user callbacks it
//! invokes, error handlers it triggers) show it:
//!
//! ```text
//! #0 [internal function]: {closure}(1)
//! #1 /t.php(3): array_map(Object(Closure), Array)
//! ```
//!
//! Recording a full `call_stack` entry for every builtin call would cost
//! several allocations per call, so a builtin call only pushes a small
//! record pointing at its (borrowed) name and arguments. It is turned into a
//! real `call_stack` entry ("materialized") only when something observes the
//! stack: user code being entered from Rust, an exception being created, or
//! debug_backtrace().

use crate::array::{ArrayKey, PhpArray};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;
use std::cell::RefCell;
use std::rc::Rc;

/// A builtin call in progress.
///
/// The pointers borrow the caller's function name and argument vector; they
/// are only dereferenced between `push_builtin_frame` and the matching
/// `pop_builtin_frame`, while the caller keeps both alive and unmodified.
pub struct BuiltinFrame {
    name: *const u8,
    name_len: usize,
    args: *const Value,
    nargs: usize,
    /// Calling line; 0 = called from internal code ("[internal function]")
    line: u32,
    /// Index this frame takes in `call_stack` once materialized
    depth: usize,
    materialized: bool,
    /// Materialized as the equivalent entry the builtin pushed itself
    /// (which it pops)
    shadowed: bool,
}

impl Vm {
    /// Record that builtin `lname` is being called with `args` from `line`
    /// (0 when called from internal code).
    #[inline]
    pub fn push_builtin_frame(&mut self, lname: &[u8], args: &[Value], line: u32) {
        let pending = self
            .builtin_frames
            .iter()
            .rev()
            .take_while(|f| !f.materialized)
            .count();
        let depth = self.call_stack.len() + pending;
        self.builtin_frames.push(BuiltinFrame {
            name: lname.as_ptr(),
            name_len: lname.len(),
            args: args.as_ptr(),
            nargs: args.len(),
            line,
            depth,
            materialized: false,
            shadowed: false,
        });
    }

    /// Name of the innermost builtin call in progress (for
    /// php_error_docref()-style "func(): " message prefixes).
    pub fn current_builtin_name(&self) -> Option<String> {
        if let Some((name, depth)) = &self.spl_active_fn {
            if *depth == self.builtin_frames.len() {
                return Some(name.clone());
            }
        }
        self.builtin_frames.last().map(|f| {
            // SAFETY: the name is borrowed for as long as the frame is live.
            let name = unsafe { std::slice::from_raw_parts(f.name, f.name_len) };
            String::from_utf8_lossy(name).into_owned()
        })
    }

    /// Values a builtin released by writing to its by-reference arguments
    /// are destroyed while its frame is still running (gh17442: the
    /// destructor is called from [internal function], under the builtin).
    #[inline]
    pub(crate) fn release_in_builtin_frame(&mut self, args: &[Value]) {
        if self.has_release_work() && args.iter().any(|a| matches!(a, Value::Reference(_))) {
            self.run_pending_destructors_pub();
        }
    }

    /// A deprecated builtin's "Function f() is deprecated" deprecation, as
    /// the call raises it (zend_deprecated_function(), before the function
    /// is entered): from the caller's code, not under the builtin's frame.
    /// Err (with the exception set) when the error handler threw: the
    /// function is not run.
    #[inline(never)]
    pub fn emit_function_deprecation(&mut self, msg: &str) -> Result<(), crate::vm::VmError> {
        let line = self.current_line;
        let hidden = match self.builtin_frames.last() {
            Some(f) if !f.materialized => self.builtin_frames.pop(),
            _ => None,
        };
        self.emit_deprecated_at(msg, line);
        if let Some(f) = hidden {
            self.builtin_frames.push(f);
        }
        if self.current_exception.is_some() {
            return Err(crate::vm::VmError {
                message: self.uncaught_exception_message(),
                line,
            });
        }
        Ok(())
    }

    /// A deprecated internal method's "Method C::m() is deprecated"
    /// deprecation, as the call raises it: from the caller's code, without
    /// the frame of method `method` (its name as called) if one was pushed.
    /// True when the error handler threw: the method is not run.
    #[inline(never)]
    pub fn emit_method_deprecation(&mut self, msg: &str, method: &str) -> bool {
        let line = self.current_line;
        let hidden_builtin = match self.builtin_frames.last() {
            Some(f) if !f.materialized => self.builtin_frames.pop(),
            _ => None,
        };
        let hidden = match self.call_stack.last() {
            Some(f) => {
                let n = f.0.as_bytes();
                let m = method.as_bytes();
                let is_method = n.len() > m.len() + 2
                    && n[n.len() - m.len()..].eq_ignore_ascii_case(m)
                    && matches!(&n[n.len() - m.len() - 2..n.len() - m.len()], b"->" | b"::");
                if is_method {
                    self.call_stack.pop()
                } else {
                    None
                }
            }
            None => None,
        };
        self.emit_deprecated_at(msg, line);
        if let Some(f) = hidden {
            self.call_stack.push(f);
        }
        if let Some(f) = hidden_builtin {
            self.builtin_frames.push(f);
        }
        self.current_exception.is_some()
    }

    /// End of the builtin call recorded by the last `push_builtin_frame`.
    #[inline]
    pub fn pop_builtin_frame(&mut self) {
        if let Some(f) = self.builtin_frames.pop() {
            if f.materialized && !f.shadowed && f.depth < self.call_stack.len() {
                self.call_stack.remove(f.depth);
            }
        }
    }

    /// Turn pending builtin frames into `call_stack` entries (all of them, or
    /// all but the innermost one, e.g. for debug_backtrace() itself).
    pub fn materialize_builtin_frames(&mut self, skip_top: bool) {
        let n = self.builtin_frames.len().saturating_sub(skip_top as usize);
        let first = self.builtin_frames[..n]
            .iter()
            .rposition(|f| f.materialized)
            .map_or(0, |i| i + 1);
        for i in first..n {
            let (name, args, line, depth) = {
                let f = &self.builtin_frames[i];
                // SAFETY: see BuiltinFrame; the frame is still active.
                let name = unsafe { std::slice::from_raw_parts(f.name, f.name_len) };
                let args = unsafe { std::slice::from_raw_parts(f.args, f.nargs) };
                let display = match crate::arginfo::lookup(name) {
                    Some(info) => info.name.to_string(),
                    None => String::from_utf8_lossy(name).into_owned(),
                };
                // (by-reference arguments are kept as references: the frame
                // must not hold a copy of the value the builtin works on)
                let args: Vec<Value> = args.to_vec();
                (display, args, f.line, f.depth)
            };
            let at = depth.min(self.call_stack.len());
            // Some builtins push an equivalent frame themselves around the
            // exceptions they throw: don't show it twice.
            // Likewise a builtin run as a fiber's callback right under the
            // fiber's own "[internal function]" entry for it.
            let same = |e: &crate::vm::CallFrame| e.2 == line && e.0.eq_ignore_ascii_case(&name);
            // (it stands for the frame, which later frames are counted
            // above)
            let shadow = if self.call_stack.get(at).is_some_and(same) {
                Some(at)
            } else if line == 0 && at > 0 && self.call_stack.get(at - 1).is_some_and(same) {
                Some(at - 1)
            } else {
                None
            };
            if let Some(d) = shadow {
                let f = &mut self.builtin_frames[i];
                f.depth = d;
                f.materialized = true;
                f.shadowed = true;
                continue;
            }
            let file = if line == 0 {
                crate::rcstr::RcStr::new()
            } else {
                self.current_file.clone()
            };
            self.builtin_frames[i].depth = at;
            self.builtin_frames[i].materialized = true;
            self.call_stack.insert(at, (name, file, line, args, false));
        }
    }

    /// The call_stack frame of the function whose code is running: included
    /// files run in their includer's frame; eval'd code (ZEND_CALL_CODE) has
    /// none, like the global scope.
    pub fn current_function_frame(&self) -> Option<&crate::vm::CallFrame> {
        for f in self.call_stack.iter().rev() {
            match f.0.as_str() {
                "include" | "include_once" | "require" | "require_once" => continue,
                "eval" => return None,
                _ => return Some(f),
            }
        }
        None
    }

    /// Whether the innermost running frame is a builtin (as opposed to user
    /// code, possibly a callback the builtin invoked).
    pub fn in_builtin_frame(&self) -> bool {
        match self.builtin_frames.last() {
            None => false,
            Some(f) => !f.materialized || f.depth + 1 >= self.call_stack.len(),
        }
    }

    /// Whether the code running now is internal code called from the
    /// innermost running user code (a frame was pushed above it), rather
    /// than that user code itself (the VM calling a magic method, foreach
    /// calling Iterator methods, ...).
    pub fn caller_is_internal(&mut self) -> bool {
        self.sync_builtin_frames();
        self.call_stack.len() > self.exec_frame_len
    }

    /// Internal code is about to run user method `name` ("Class->method" or
    /// "Class::method"; zend_call_method()): give it a frame, located at
    /// "[internal function]" when called from internal code, or at the
    /// current line when the VM itself calls it. No frame is pushed when the
    /// caller already pushed one for this call. Returns whether a frame was
    /// pushed (for the caller to pop).
    pub fn push_callee_frame(&mut self, name: String, args: &[Value], is_instance: bool) -> bool {
        let internal = self.caller_is_internal();
        if internal {
            if let Some(top) = self.call_stack.last() {
                if same_frame_name(&top.0, &name) {
                    return false;
                }
            }
        }
        // (in the buffers of finished calls' frames)
        let (file, line) = if internal {
            (crate::rcstr::RcStr::new(), 0)
        } else {
            (self.current_file.clone(), self.current_line)
        };
        let mut frame_args = self.take_values(0);
        frame_args.extend(args.iter().map(|v| v.deref()));
        self.call_stack
            .push((name, file, line, frame_args, is_instance));
        true
    }

    /// The frame name ("Class->method", naming the declaring class) of
    /// internal method `method_lower` of an object of class `class_lower`.
    pub fn internal_method_frame_name(
        &self,
        class_lower: &[u8],
        method_lower: &[u8],
    ) -> Option<String> {
        let cur = self.internal_base_class(class_lower)?;
        let (c, m) = crate::stubs::find_method(&cur, method_lower)?;
        Some(format!("{}->{}", c.name, m.name))
    }

    /// The nearest internal class in the class chain of `class_lower`
    /// (itself when internal), lowercase.
    pub fn internal_base_class(&self, class_lower: &[u8]) -> Option<Vec<u8>> {
        let mut cur = class_lower.to_vec();
        for _ in 0..64 {
            if crate::stubs::find_class_lower(&cur).is_some() {
                return Some(cur);
            }
            cur = match self.classes.get(&cur).and_then(|c| c.parent.as_ref()) {
                Some(p) => p.to_ascii_lowercase(),
                None => crate::vm::get_builtin_parent(&cur)?.to_ascii_lowercase(),
            };
        }
        None
    }

    /// Whether sync_builtin_frames() has nothing to do.
    #[inline]
    pub(crate) fn builtin_frames_synced(&self) -> bool {
        !self.builtin_frames.last().is_some_and(|f| !f.materialized)
    }

    /// Materialize pending builtin frames if there are any (cheap check).
    #[inline]
    pub fn sync_builtin_frames(&mut self) {
        if self.builtin_frames.last().is_some_and(|f| !f.materialized) {
            self.materialize_builtin_frames(false);
        }
    }

    /// A fiber suspends: hand it the builtin frames it pushed (depths made
    /// relative to its call_stack base).
    pub(crate) fn fiber_take_builtin_frames(
        &mut self,
        frames_base: usize,
        call_base: usize,
    ) -> Vec<BuiltinFrame> {
        if self.builtin_frames.len() <= frames_base {
            return Vec::new();
        }
        let mut out = self.builtin_frames.split_off(frames_base);
        for f in &mut out {
            f.depth = f.depth.saturating_sub(call_base);
        }
        out
    }

    /// A fiber resumes: re-install its builtin frames on top of the current
    /// call_stack (call before its call_stack frames are appended).
    pub(crate) fn fiber_restore_builtin_frames(&mut self, frames: &mut Vec<BuiltinFrame>) {
        let base = self.call_stack.len();
        for f in frames.iter_mut() {
            f.depth += base;
        }
        self.builtin_frames.append(frames);
    }

    /// Call builtin `f` (lowercase name `lname`) as a callback from internal
    /// code: generic zpp checks in weak mode, and an "[internal function]"
    /// frame while it runs.
    pub fn call_builtin_from_internal(
        &mut self,
        f: crate::vm::BuiltinFn,
        lname: &[u8],
        args: &[Value],
    ) -> Result<Value, crate::vm::VmError> {
        // Located at the call site when called through a direct call_user_func()
        // (ZEND_INIT_USER_CALL: the call is the calling code's, made in its
        // strict_types mode)
        let direct = self.ucf_call_line.take();
        let at = direct.unwrap_or(0);
        let strict = direct.is_some() && std::mem::take(&mut self.ucf_strict);
        // zend_call_function(): a by-reference parameter given a plain value
        // warns (zend_param_must_be_ref) and gets a temporary reference (to
        // a separated copy: the caller's array is not modified).
        let mut separated: Option<Vec<Value>> = None;
        if let Some(info) = crate::arginfo::lookup(lname) {
            for (i, a) in args.iter().enumerate() {
                let Some(p) = info
                    .params
                    .get(i)
                    .or_else(|| info.params.last().filter(|p| p.variadic))
                else {
                    break;
                };
                if p.by_ref != 0 {
                    if let Value::Array(arr) = a {
                        let copy = arr.borrow().clone();
                        separated.get_or_insert_with(|| args.to_vec())[i] =
                            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(copy)));
                    } else if let Some(arr) = Self::array_ref_builtin_arg(lname, i, a) {
                        separated.get_or_insert_with(|| args.to_vec())[i] = arr;
                    }
                }
                if p.by_ref == 1 && !matches!(a, Value::Reference(_)) {
                    let line = self.current_line;
                    self.emit_warning_at(
                        &format!(
                            "{}(): Argument #{} (${}) must be passed by reference, value given",
                            info.name,
                            i + 1,
                            p.name
                        ),
                        line,
                    );
                }
            }
        }
        let args: &[Value] = separated.as_deref().unwrap_or(args);
        let Some((info, flags)) = self.zpp_info(lname) else {
            // zend_call_function() -> the function's own zpp: the arity
            // check holds for every internal function, even those whose
            // types the generic layer doesn't check yet.
            if let Some(msg) = crate::arginfo::lookup(lname).and_then(|i| i.arity_error(args.len()))
            {
                let line = self.current_line;
                self.push_builtin_frame(lname, args, at);
                let exc = self.create_exception(b"ArgumentCountError", &msg, line);
                self.pop_builtin_frame();
                self.current_exception = Some(exc);
                return Err(crate::vm::VmError {
                    message: self.uncaught_exception_message(),
                    line,
                });
            }
            self.push_builtin_frame(lname, args, at);
            let r = self.call_builtin_observed(f, lname, args);
            self.pop_builtin_frame();
            self.interrupt_after_internal_call();
            return r;
        };
        let line = self.current_line;
        let mut owned = args.to_vec();
        let mut orig = None;
        self.zpp_frame_at = Some(at);
        if let Err(e) = self.zpp_check(info, flags, &mut owned, &mut orig, strict, line) {
            if let crate::arginfo::ZppError::Throw(cls, msg) = e {
                let shown = orig.unwrap_or_else(|| owned.clone());
                self.push_builtin_frame(lname, &shown, at);
                let exc = self.create_exception(cls, &msg, line);
                self.pop_builtin_frame();
                self.current_exception = Some(exc);
            }
            return Err(crate::vm::VmError {
                message: self.uncaught_exception_message(),
                line,
            });
        }
        self.push_builtin_frame(lname, orig.as_deref().unwrap_or(&owned), at);
        let r = self.call_builtin_observed(f, lname, &owned);
        self.pop_builtin_frame();
        self.interrupt_after_internal_call();
        r
    }

    /// A reference to an array given for the `array &$array` parameter of a
    /// builtin that sorts/modifies the array it is handed (goro passes those
    /// arrays by handle, see Compiler::is_array_ref_builtin): the referenced
    /// array, separated from other holders, so the change lands in it.
    #[inline(never)]
    fn array_ref_builtin_arg(lname: &[u8], i: usize, a: &Value) -> Option<Value> {
        let Value::Reference(r) = a else {
            return None;
        };
        if i != 0 || !crate::compiler::Compiler::is_array_ref_builtin(Some(lname)) {
            return None;
        }
        let inner = r.borrow().clone();
        let Value::Array(arr) = inner else {
            return None;
        };
        // (the reference and `arr` hold it: anyone else shares it)
        if std::rc::Rc::strong_count(&arr) > 2 {
            let copy = std::rc::Rc::new(std::cell::RefCell::new(arr.borrow().clone()));
            *r.borrow_mut() = Value::Array(copy.clone());
            return Some(Value::Array(copy));
        }
        Some(Value::Array(arr))
    }

    #[cold]
    #[inline(never)]
    fn fcall_interrupt(&mut self) {
        self.interrupt_after_internal_call();
    }

    /// zend_call_function() after an internal function returned: a pending
    /// EG(vm_interrupt) runs zend_interrupt_function (a timeout is left to
    /// the executor).
    #[inline]
    fn interrupt_after_internal_call(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        if crate::value::VM_INTERRUPT.load(Relaxed) && !crate::value::TIMED_OUT.load(Relaxed) {
            if let Some(hook) = self.interrupt_hook {
                crate::value::VM_INTERRUPT.store(false, Relaxed);
                hook(self);
            }
        }
    }

    /// call_builtin_from_internal() with named arguments: they are mapped
    /// onto the parameters first (zend_handle_named_arg), so the
    /// by-reference and zpp checks see them in place.
    pub fn call_builtin_from_internal_named(
        &mut self,
        f: crate::vm::BuiltinFn,
        lname: &[u8],
        args: &[Value],
        named: Vec<(Vec<u8>, Value)>,
    ) -> Result<Value, crate::vm::VmError> {
        if named.is_empty() {
            return self.call_builtin_from_internal(f, lname, args);
        }
        // Builtins collecting extra named arguments for a callback
        // (Z_PARAM_VARIADIC_WITH_NAMED) receive them as they are.
        if matches!(lname, b"call_user_func" | b"forward_static_call")
            || self.builtin_named_variadics.contains(lname)
        {
            self.pending_named_args = named;
            return self.call_builtin_from_internal(f, lname, args);
        }
        let Some(info) = crate::arginfo::lookup(lname) else {
            self.pending_named_args = named;
            return f(self, args);
        };
        let mut owned = args.to_vec();
        let extra = extra_named_args(info, &named);
        if let Err(e) = self.zpp_resolve_named(info, &mut owned, named) {
            let line = self.current_line;
            if let crate::arginfo::ZppError::Throw(cls, msg) = e {
                // (collected by the variadic: the call is made, its own
                // argument checks fail in its frame)
                let in_call = !extra.is_empty();
                if in_call {
                    let at = self.ucf_call_line.take().unwrap_or(0);
                    self.push_builtin_frame(lname, args, at);
                    self.note_builtin_frame_named(extra);
                }
                let exc = self.create_exception(cls, &msg, line);
                self.builtin_frame_named.clear();
                if in_call {
                    self.pop_builtin_frame();
                }
                self.current_exception = Some(exc);
            }
            return Err(crate::vm::VmError {
                message: self.uncaught_exception_message(),
                line,
            });
        }
        self.call_builtin_from_internal(f, lname, &owned)
    }

    /// Run builtin `f` (lowercase name `lname`), with the observer's
    /// begin/end hooks around it when there is an observer.
    #[inline(always)]
    pub(crate) fn call_builtin_observed(
        &mut self,
        f: crate::vm::BuiltinFn,
        lname: &[u8],
        args: &[Value],
    ) -> Result<Value, crate::vm::VmError> {
        let first_res = crate::value::next_resource_id();
        let r = if self.observer.is_none() {
            f(self, args)
        } else {
            self.call_builtin_with_observer(f, lname, args)
        };
        // ZEND_VM_FCALL_INTERRUPT_CHECK(call): a signal that arrived during
        // the call is handled while its frame is still the current one.
        if crate::value::VM_INTERRUPT.load(std::sync::atomic::Ordering::Relaxed) {
            self.fcall_interrupt();
        }
        if let Ok(v) = &r {
            crate::value::adopt_returned_resources(first_res, v);
        }
        r
    }

    #[cold]
    #[inline(never)]
    fn call_builtin_with_observer(
        &mut self,
        f: crate::vm::BuiltinFn,
        lname: &[u8],
        args: &[Value],
    ) -> Result<Value, crate::vm::VmError> {
        let Some(h) = self.observer else {
            return f(self, args);
        };
        // Engine-internal helpers ("__"-prefixed: the native bodies of
        // extensions' methods...) are not functions of their own.
        if lname.starts_with(b"__") {
            return f(self, args);
        }
        let name = match crate::arginfo::lookup(lname) {
            Some(info) => info.name.to_string(),
            None => String::from_utf8_lossy(lname).into_owned(),
        };
        let frame = crate::vm::ObservedFrame::Internal(&name);
        (h.begin)(self, &frame);
        let r = f(self, args);
        (h.end)(self, &frame, r.as_ref().ok());
        r
    }
}

impl Vm {
    /// Call builtin `f` (lowercase name `lname`) from PHP code run outside
    /// the main executor (generator bodies), at `line`: named arguments,
    /// the generic zpp checks and a frame, as the main DoFCall does.
    pub(crate) fn call_builtin_at(
        &mut self,
        f: crate::vm::BuiltinFn,
        lname: &[u8],
        mut args: Vec<Value>,
        named: Vec<(Vec<u8>, Value)>,
        strict: bool,
        line: u32,
    ) -> Result<Value, crate::vm::VmError> {
        let is_ucf = lname == b"call_user_func" || lname == b"call_user_func_array";
        let special = named.is_empty() && compiled_special(lname, &args);
        let mut err = None;
        // A variadic function's extra named arguments
        // (ZEND_CALL_HAS_EXTRA_NAMED_PARAMS): the call is made, its own
        // argument checks fail inside it.
        let mut extra_named: Vec<(PhpString, Value)> = Vec::new();
        if !named.is_empty() {
            if is_ucf || lname == b"forward_static_call" || lname == b"forward_static_call_array" {
                self.pending_named_args = named;
            } else if let Some(info) = crate::arginfo::lookup(lname) {
                extra_named = extra_named_args(info, &named);
                if let Err(e) = self.zpp_resolve_named(info, &mut args, named) {
                    err = Some((e, !extra_named.is_empty()));
                }
            }
        }
        let mut orig = None;
        if err.is_none() {
            if let Some((info, flags)) = self.zpp_info(lname) {
                if !is_ucf && !special {
                    self.zpp_frame_at = Some(line);
                }
                if let Err(e) = self.zpp_check(info, flags, &mut args, &mut orig, strict, line) {
                    err = Some((e, true));
                }
            }
        }
        if let Some((e, in_call)) = err {
            if let crate::arginfo::ZppError::Throw(cls, msg) = e {
                let shown = orig.unwrap_or_else(|| args.clone());
                let in_call = in_call && !special;
                if in_call {
                    self.push_builtin_frame(lname, &shown, line);
                    self.note_builtin_frame_named(std::mem::take(&mut extra_named));
                }
                let exc = self.create_exception(cls, &msg, line);
                self.builtin_frame_named.clear();
                if in_call {
                    self.pop_builtin_frame();
                }
                self.current_exception = Some(exc);
            }
            return Err(crate::vm::VmError {
                message: self.uncaught_exception_message(),
                line,
            });
        }
        let own_frame = !is_ucf && !special;
        let shown = orig.unwrap_or_default();
        if own_frame {
            let frame_args: &[Value] = if shown.is_empty() { &args } else { &shown };
            self.push_builtin_frame(lname, frame_args, line);
        } else {
            self.ucf_call_line = Some(line);
            self.ucf_strict = false;
        }
        let r = if own_frame {
            self.call_builtin_observed(f, lname, &args)
        } else {
            f(self, &args)
        };
        if own_frame {
            self.release_in_builtin_frame(&args);
            self.pop_builtin_frame();
        } else {
            self.ucf_call_line = None;
        }
        r
    }
}

/// Whether php-src compiles this call to builtin `lname` with `args` (no
/// named arguments, name resolved at compile time) into an opcode instead of
/// a function call (zend_try_compile_special_func_ex), so it has no frame.
/// Cases depending on literal arguments (defined(), in_array(); chr() and
/// ord() are only folded for a literal they cannot fail on) or on the
/// calling context (func_get_args(), array_slice()) are left out.
pub fn compiled_special(lname: &[u8], args: &[Value]) -> bool {
    if lname == b"sprintf" {
        return sprintf_is_rope(args);
    }
    match args.len() {
        0 => matches!(lname, b"get_class" | b"get_called_class"),
        1 => matches!(
            lname,
            b"strlen"
                | b"is_null"
                | b"is_bool"
                | b"is_long"
                | b"is_int"
                | b"is_integer"
                | b"is_float"
                | b"is_double"
                | b"is_string"
                | b"is_array"
                | b"is_object"
                | b"is_resource"
                | b"is_scalar"
                | b"boolval"
                | b"intval"
                | b"floatval"
                | b"doubleval"
                | b"strval"
                | b"count"
                | b"sizeof"
                | b"get_class"
                | b"gettype"
        ),
        2 => lname == b"array_key_exists",
        _ => false,
    }
}

/// Whether php-src compiles this sprintf() call into a rope instead of a
/// function call (zend_compile_func_sprintf: a short format with only %s,
/// %d and %% and one argument per placeholder), so it has no frame.
pub fn sprintf_is_rope(args: &[Value]) -> bool {
    let Some(Value::String(f)) = args.first() else {
        return false;
    };
    let b = f.as_bytes();
    if b.len() >= 256 {
        return false;
    }
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            match b.get(i + 1) {
                Some(b's') | Some(b'd') => n += 1,
                Some(b'%') => {}
                _ => return false,
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    n == args.len() - 1
}

/// `$this` of an internal method call in progress (see `note_frame_this`).
pub struct FrameThis {
    /// call_stack index of the method's frame
    depth: usize,
    /// Heap pointer of that frame's name, to recognize it
    name: *const u8,
    obj: WeakThis,
}

enum WeakThis {
    Object(std::rc::Weak<std::cell::RefCell<crate::object::PhpObject>>),
    Generator(std::rc::Weak<std::cell::RefCell<crate::generator::PhpGenerator>>),
}

/// The compiled variables of a running user function, so backtraces show
/// the current values of its parameters (zend_fetch_debug_backtrace reads
/// the frame's CVs: `f(Array)` after `$a = []` inside `f`, NULL once unset)
/// rather than the values it was called with.
///
/// `cvs` points at the CV vector owned by the executing frame
/// (execute_op_array_inner's local, or a running generator's); the entry is
/// removed before that frame ends (or moved with its fiber).
pub struct CvFrame {
    /// call_stack index of the function's frame
    depth: usize,
    /// Heap pointer of that frame's name, to recognize it
    name: *const u8,
    cvs: *const Vec<Value>,
    /// CV index of the first parameter ($this / captured variables come first)
    base: u32,
    /// Number of declared non-variadic parameters
    nfixed: u32,
    /// CV 0 is `$this` (a method or a closure bound to an object)
    has_this: bool,
    /// CV index + 1 of the variadic parameter (0: none), whose string keys
    /// are the call's extra named arguments
    variadic: u32,
}

impl Vm {
    /// Register the frame of `op_array`, about to be run, for the
    /// call_stack frame pushed for it (if the top frame is that frame).
    /// Returns whether an entry was pushed; its `cvs` is set by the caller
    /// once the CV vector is in place, and the caller pops the entry when the
    /// function ends.
    #[inline]
    /// The top `call_stack` frame is the user-code call of `op_array` (not
    /// the frame of a builtin that invoked it).
    pub(crate) fn top_frame_is_call_of(&self, op_array: &crate::opcode::OpArray) -> bool {
        let Some(depth) = self.call_stack.len().checked_sub(1) else {
            return false;
        };
        frame_is_for(&self.call_stack[depth].0, op_array)
            && !self
                .builtin_frames
                .iter()
                .any(|b| b.materialized && b.depth == depth)
    }

    pub(crate) fn register_cv_frame(&mut self, op_array: &crate::opcode::OpArray) -> bool {
        self.register_cv_frame_hinted(op_array, false)
    }

    /// register_cv_frame(); `hinted`: the top call_stack frame is known to
    /// be the call of `op_array` (DO_FCALL's fast path pushed it).
    #[inline]
    pub(crate) fn register_cv_frame_hinted(
        &mut self,
        op_array: &crate::opcode::OpArray,
        hinted: bool,
    ) -> bool {
        // debug_backtrace() shows the frame's `$this` as its "object"
        let has_this = op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
        let variadic = op_array.variadic_param.map_or(0, |v| v + 1);
        if op_array.param_count == 0 && !has_this {
            return false;
        }
        let Some(depth) = self.call_stack.len().checked_sub(1) else {
            return false;
        };
        let frame = &self.call_stack[depth];
        if frame.0.is_empty()
            || (frame.3.is_empty() && !has_this)
            || self
                .cv_frames
                .last()
                .is_some_and(|f| f.name == frame.0.as_ptr())
            || (!hinted && !frame_is_for(&frame.0, op_array))
        {
            return false;
        }
        // A builtin's frame (the function called us without a frame of our own)
        if !hinted
            && self
                .builtin_frames
                .iter()
                .rev()
                .any(|b| b.materialized && b.depth == depth)
        {
            return false;
        }
        let base = implicit_args(op_array) as u32;
        let nfixed = op_array
            .param_count
            .saturating_sub(base + op_array.variadic_param.is_some() as u32);
        if nfixed == 0 && !has_this && variadic == 0 {
            return false;
        }
        // The frame holds no copies of its parameters' values (see
        // sync_frame_args()): the CVs are their only holders, so they are
        // released with them, in declaration order
        // (i_free_compiled_variables), or when a parameter is reassigned.
        let args = &mut self.call_stack[depth].3;
        let n = (nfixed as usize).min(args.len());
        for a in &mut args[..n] {
            if matches!(a, Value::Object(_) | Value::Array(_) | Value::Reference(_)) {
                *a = Value::Null;
            }
        }
        let frame = &self.call_stack[depth];
        self.cv_frames.push(CvFrame {
            depth,
            name: frame.0.as_ptr(),
            cvs: std::ptr::null(),
            base,
            nfixed,
            has_this,
            variadic,
        });
        true
    }

    /// Point the CV frame registered last at the function's CV vector.
    #[inline]
    pub(crate) fn set_cv_frame_vars(&mut self, cvs: &Vec<Value>) {
        if let Some(f) = self.cv_frames.last_mut() {
            f.cvs = cvs;
        }
    }

    /// Copy the current values of running functions' parameters into their
    /// call_stack frames (before a backtrace or func_get_args() reads them).
    pub fn sync_frame_args(&mut self) {
        for k in 0..self.cv_frames.len() {
            let f = &self.cv_frames[k];
            if f.cvs.is_null() || f.nfixed == 0 {
                continue;
            }
            let Some(at) = self.cv_frame_index(f) else {
                continue;
            };
            // SAFETY: see CvFrame; the function owning `cvs` is still running.
            let cvs = unsafe { &*f.cvs };
            let (base, nfixed) = (f.base as usize, f.nfixed as usize);
            let args = &mut self.call_stack[at].3;
            let n = args.len().min(nfixed);
            for (i, a) in args.iter_mut().enumerate().take(n) {
                if let Some(v) = cvs.get(base + i) {
                    *a = match v {
                        Value::Undef => Value::Null,
                        Value::Reference(r) => match &*r.borrow() {
                            Value::Undef => Value::Null,
                            v => v.clone(),
                        },
                        v => v.clone(),
                    };
                }
            }
        }
    }

    /// The copies sync_frame_args() made of the parameters of the running
    /// frames are dropped again once the backtrace is built: the CVs stay
    /// their only holders (see release_cv_frame_arg_copies), so a parameter
    /// a function reassigns or unsets is released then, as php-src's frames
    /// (whose arguments are their CVs) release it.
    #[inline(never)]
    pub fn release_synced_frame_args(&mut self) {
        for k in 0..self.cv_frames.len() {
            let f = &self.cv_frames[k];
            if f.cvs.is_null() || f.nfixed == 0 {
                continue;
            }
            let nfixed = f.nfixed as usize;
            let Some(at) = self.cv_frame_index(f) else {
                continue;
            };
            let args = &mut self.call_stack[at].3;
            let n = args.len().min(nfixed);
            for a in args.iter_mut().take(n) {
                if matches!(a, Value::Object(_) | Value::Array(_) | Value::Reference(_)) {
                    *a = Value::Null;
                }
            }
        }
    }

    /// The call_stack index of the frame `f` was registered for.
    fn cv_frame_index(&self, f: &CvFrame) -> Option<usize> {
        if self
            .call_stack
            .get(f.depth)
            .is_some_and(|e| e.0.as_ptr() == f.name)
        {
            Some(f.depth)
        } else {
            self.call_stack.iter().rposition(|e| e.0.as_ptr() == f.name)
        }
    }

    /// The innermost (just pushed) builtin frame has these extra named
    /// arguments (see Vm::builtin_frame_named).
    pub(crate) fn note_builtin_frame_named(&mut self, extra: Vec<(PhpString, Value)>) {
        if extra.is_empty() {
            return;
        }
        self.sync_builtin_frames();
        let at = self.call_stack.len().saturating_sub(1);
        self.builtin_frame_named = extra.into_iter().map(|(k, v)| (at, k, v)).collect();
    }

    /// The extra named arguments (ZEND_CALL_HAS_EXTRA_NAMED_PARAMS) of the
    /// running variadic functions' frames: (call_stack index, name, value).
    pub fn frame_named_args(&self) -> Vec<(usize, PhpString, Value)> {
        let mut out = self.builtin_frame_named.clone();
        for f in &self.cv_frames {
            if f.variadic == 0 || f.cvs.is_null() {
                continue;
            }
            // SAFETY: see CvFrame; the function owning `cvs` is still running.
            let cvs = unsafe { &*f.cvs };
            let Some(Value::Array(a)) = cvs.get(f.variadic as usize - 1).map(|v| v.deref()) else {
                continue;
            };
            let a = a.borrow();
            if a.is_packed() {
                continue;
            }
            let Some(at) = self.cv_frame_index(f) else {
                continue;
            };
            for (k, v) in a.iter() {
                if let crate::array::ArrayKey::String(name) = k {
                    out.push((at, name.clone(), v.deref()));
                }
            }
        }
        out
    }

    /// A frame's "args": its positional arguments, then its extra named
    /// ones (string keys); #[\SensitiveParameter] ones wrapped.
    pub(crate) fn frame_args_array(
        &mut self,
        func_name: &str,
        args: &[Value],
        named: Vec<(PhpString, Value)>,
    ) -> PhpArray {
        let mut out = PhpArray::new();
        if named.is_empty() {
            for arg in self.trace_frame_args(func_name, args) {
                out.push(arg);
            }
            return out;
        }
        // (the named ones go to the variadic parameter)
        let mut all = args.to_vec();
        all.extend(named.iter().map(|n| n.1.clone()));
        let mut wrapped = self.trace_frame_args(func_name, &all).into_iter();
        for _ in 0..args.len() {
            if let Some(v) = wrapped.next() {
                out.push(v);
            }
        }
        for ((name, _), v) in named.into_iter().zip(wrapped) {
            out.set(ArrayKey::String(name), v);
        }
        out
    }

    /// The internal method call whose frame was just pushed (the top
    /// call_stack frame) runs on `this`: debug_backtrace() shows it as the
    /// frame's "object". The entry holds no reference (the object's
    /// lifetime is unchanged) and goes stale once the frame is popped.
    pub fn note_frame_this(&mut self, this: &Value) {
        let obj = match this {
            Value::Object(o) => WeakThis::Object(std::rc::Rc::downgrade(o)),
            Value::Generator(g) => WeakThis::Generator(std::rc::Rc::downgrade(g)),
            _ => return,
        };
        let Some(depth) = self.call_stack.len().checked_sub(1) else {
            return;
        };
        // (entries at this depth or above belong to frames that are gone)
        while self.frame_this.last().is_some_and(|f| f.depth >= depth) {
            self.frame_this.pop();
        }
        self.frame_this.push(FrameThis {
            depth,
            name: self.call_stack[depth].0.as_ptr(),
            obj,
        });
    }

    /// The `$this` of each call_stack frame (zend_fetch_debug_backtrace's
    /// "object"), for the frames of running methods.
    pub fn frame_objects(&self) -> Vec<Option<Value>> {
        let mut out = vec![None; self.call_stack.len()];
        for f in &self.frame_this {
            let at = if self
                .call_stack
                .get(f.depth)
                .is_some_and(|e| e.0.as_ptr() == f.name)
            {
                Some(f.depth)
            } else {
                self.call_stack.iter().rposition(|e| e.0.as_ptr() == f.name)
            };
            let obj = match &f.obj {
                WeakThis::Object(o) => o.upgrade().map(Value::Object),
                WeakThis::Generator(g) => g.upgrade().map(Value::Generator),
            };
            if let (Some(at), Some(obj)) = (at, obj) {
                out[at] = Some(obj);
            }
        }
        for f in &self.cv_frames {
            if !f.has_this || f.cvs.is_null() {
                continue;
            }
            let Some(at) = self.cv_frame_index(f) else {
                continue;
            };
            // SAFETY: see CvFrame; the function owning `cvs` is still running.
            let cvs = unsafe { &*f.cvs };
            if let Some(v @ Value::Object(_)) = cvs.first().map(|v| v.deref()) {
                out[at] = Some(v);
            }
        }
        out
    }

    /// zend_fetch_debug_backtrace(): the frames below the calling builtin
    /// (debug_backtrace(), debug_print_backtrace()), or the frames of call_stack
    /// range `range` (ReflectionFiber::getTrace()).
    pub fn debug_backtrace_array(
        &mut self,
        options: i64,
        limit: i64,
        range: Option<(usize, usize)>,
    ) -> PhpArray {
        let out = self.debug_backtrace_array_synced(options, limit, range);
        self.release_synced_frame_args();
        out
    }

    fn debug_backtrace_array_synced(
        &mut self,
        options: i64,
        limit: i64,
        range: Option<(usize, usize)>,
    ) -> PhpArray {
        let vm = self;
        if range.is_none() {
            // Frames of the builtins being run (not debug_backtrace() itself)
            vm.materialize_builtin_frames(true);
        }
        vm.sync_frame_args();
        let ignore_args = (options & 2) != 0; // DEBUG_BACKTRACE_IGNORE_ARGS = 2
        let provide_object = (options & 1) != 0; // DEBUG_BACKTRACE_PROVIDE_OBJECT = 1

        let mut trace_arr = PhpArray::new();
        // call_stack entries: (function_name, file, line_called_from, args, is_instance_method)
        // Ordered outermost to innermost. Reversed for backtrace output.
        let stack_len = vm.call_stack.len();
        let max_frames = if limit > 0 {
            limit as usize
        } else {
            usize::MAX
        };
        let objects = if provide_object {
            vm.frame_objects()
        } else {
            Vec::new()
        };
        let named_args = if ignore_args {
            Vec::new()
        } else {
            vm.frame_named_args()
        };

        let (lo, hi) = range.unwrap_or((0, stack_len));
        let frames: Vec<crate::vm::CallFrame> =
            vm.call_stack[lo..hi].iter().rev().cloned().collect();
        let key = |k: &[u8]| ArrayKey::String(PhpString::from_bytes(k));
        let string = |s: &str| Value::String(PhpString::from_string(s.to_string()));
        for (idx, (func_name, file, line, _, is_method)) in frames.iter().enumerate() {
            if idx >= max_frames {
                break;
            }
            let mut frame = PhpArray::new();
            // A frame called from internal code (callback of a builtin, fiber
            // function, generator body resumed by the engine) has no location.
            if !(file.is_empty() || file == "[internal function]" || *line == 0) {
                frame.set(key(b"file"), string(file));
                frame.set(key(b"line"), Value::Long(*line as i64));
            }
            // Parse class::method or class->method from func_name. For closures
            // (`{closure:...}`) we never split on `::` even if the closure name
            // embeds `Class::method()`.
            let split = if func_name.starts_with("{closure:") {
                None
            } else if let Some(sep) = func_name.find("::") {
                Some((sep, *is_method))
            } else {
                func_name.find("->").map(|sep| (sep, true))
            };
            if let Some((sep, instance)) = split {
                frame.set(key(b"function"), string(&func_name[sep + 2..]));
                frame.set(key(b"class"), string(&func_name[..sep]));
                if instance {
                    if let Some(Some(obj)) = objects.get(hi - 1 - idx) {
                        frame.set(key(b"object"), obj.clone());
                    }
                }
                // Use -> for instance methods, :: for static methods
                let type_str: &[u8] = if instance { b"->" } else { b"::" };
                frame.set(key(b"type"), Value::String(PhpString::from_bytes(type_str)));
            } else {
                frame.set(key(b"function"), string(func_name));
            }
            // Add args array unless DEBUG_BACKTRACE_IGNORE_ARGS is set (an
            // include or eval frame without arguments has none)
            if !ignore_args {
                let raw = Vm::trace_frame_args_at(&frames, idx);
                let skip = raw.is_empty()
                    && matches!(
                        func_name.as_str(),
                        "eval" | "include" | "include_once" | "require" | "require_once"
                    );
                if !skip {
                    let at = hi - 1 - idx;
                    let named: Vec<(PhpString, Value)> = named_args
                        .iter()
                        .filter(|n| n.0 == at)
                        .map(|n| (n.1.clone(), n.2.clone()))
                        .collect();
                    let args_arr = vm.frame_args_array(func_name, &raw, named);
                    frame.set(key(b"args"), Value::Array(Rc::new(RefCell::new(args_arr))));
                }
            }
            trace_arr.set(
                ArrayKey::Int(idx as i64),
                Value::Array(Rc::new(RefCell::new(frame))),
            );
        }
        trace_arr
    }

    /// A fiber suspends: hand it the CV frames of its call_stack frames,
    /// those from index `base` up, made relative to `base`.
    pub(crate) fn fiber_take_cv_frames(&mut self, base: usize) -> Vec<CvFrame> {
        let at = self
            .cv_frames
            .iter()
            .position(|f| f.depth >= base)
            .unwrap_or(self.cv_frames.len());
        let mut out = self.cv_frames.split_off(at);
        for f in &mut out {
            f.depth -= base;
        }
        out
    }

    /// A fiber resumes: re-install its CV frames, before its call_stack
    /// frames are appended (their names move with them).
    pub(crate) fn fiber_restore_cv_frames(&mut self, frames: &mut Vec<CvFrame>) {
        let base = self.call_stack.len();
        for f in frames.iter_mut() {
            f.depth += base;
        }
        self.cv_frames.append(frames);
    }

    /// A fiber ended: drop the CV frames of its frames (from index `base` up).
    pub(crate) fn fiber_drop_cv_frames(&mut self, base: usize) {
        while self.cv_frames.last().is_some_and(|f| f.depth >= base) {
            self.cv_frames.pop();
        }
    }
}

/// Number of leading CVs of `op_array` bound from the call's implicit
/// arguments ($this, or a closure's captured variables) rather than from
/// its parameters; frames show the arguments after them.
pub fn implicit_args(op_array: &crate::opcode::OpArray) -> usize {
    if crate::closure::is_closure_fn_name(&op_array.name)
        && !op_array.closure_capture_names.is_empty()
    {
        op_array.closure_capture_names.len()
    } else if op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
        1
    } else {
        0
    }
}

/// Whether call_stack frame `name` is the frame of a call to `op_array`.
fn frame_is_for(name: &str, op_array: &crate::opcode::OpArray) -> bool {
    // `{closure:...}`, or `Class::{closure:...}` for a closure with a scope
    // (no other frame name ends with `}`)
    if name.ends_with('}') && (name.starts_with("{closure") || name.contains("::{closure")) {
        return crate::closure::is_closure_fn_name(&op_array.name);
    }
    let last = |s: &[u8]| -> usize {
        let mut i = s.len();
        while i > 0 {
            if s[i - 1] == b':' || s[i - 1] == b'>' {
                break;
            }
            i -= 1;
        }
        i
    };
    let n = name.as_bytes();
    let o = &op_array.name;
    n[last(n)..].eq_ignore_ascii_case(&o[last(o)..])
}

/// Whether two call_stack frame names name the same function ("A->f" and
/// "A::f" alike, case-insensitively).
fn same_frame_name(a: &str, b: &str) -> bool {
    // `s.replace("->", "::").to_ascii_lowercase()`, without building it.
    struct Norm<'a> {
        s: &'a [u8],
        i: usize,
        pending: bool,
    }
    impl Iterator for Norm<'_> {
        type Item = u8;
        fn next(&mut self) -> Option<u8> {
            if self.pending {
                self.pending = false;
                return Some(b':');
            }
            let c = *self.s.get(self.i)?;
            if c == b'-' && self.s.get(self.i + 1) == Some(&b'>') {
                self.i += 2;
                self.pending = true;
                return Some(b':');
            }
            self.i += 1;
            Some(c.to_ascii_lowercase())
        }
    }
    fn norm(s: &str) -> Norm<'_> {
        Norm {
            s: s.as_bytes(),
            i: 0,
            pending: false,
        }
    }
    a.len() == b.len() && norm(a).eq(norm(b))
}

/// The named arguments of a call to variadic internal function `info` that
/// name none of its parameters (collected by the variadic one).
pub(crate) fn extra_named_args(
    info: &crate::arginfo::FuncInfo,
    named: &[(Vec<u8>, Value)],
) -> Vec<(PhpString, Value)> {
    if !info.is_variadic() {
        return Vec::new();
    }
    let n = info.num_args();
    named
        .iter()
        .filter(|(k, _)| !info.params[..n].iter().any(|p| p.name.as_bytes() == k))
        .map(|(k, v)| (PhpString::from_vec(k.clone()), v.clone()))
        .collect()
}
