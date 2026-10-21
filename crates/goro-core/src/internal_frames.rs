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

use crate::value::Value;
use crate::vm::Vm;

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
        });
    }

    /// End of the builtin call recorded by the last `push_builtin_frame`.
    #[inline]
    pub fn pop_builtin_frame(&mut self) {
        if let Some(f) = self.builtin_frames.pop() {
            if f.materialized && f.depth < self.call_stack.len() {
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
                let args: Vec<Value> = args.iter().map(|v| v.deref()).collect();
                (display, args, f.line, f.depth)
            };
            let at = depth.min(self.call_stack.len());
            // Some builtins push an equivalent frame themselves around the
            // exceptions they throw: don't show it twice.
            if self
                .call_stack
                .get(at)
                .is_some_and(|e| e.2 == line && e.0.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            let file = if line == 0 {
                String::new()
            } else {
                self.current_file.clone()
            };
            self.builtin_frames[i].depth = at;
            self.builtin_frames[i].materialized = true;
            self.call_stack.insert(at, (name, file, line, args, false));
        }
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
        let at = self.ucf_call_line.take().unwrap_or(0);
        let Some((info, flags)) = self.zpp_info(lname) else {
            self.push_builtin_frame(lname, args, at);
            let r = f(self, args);
            self.pop_builtin_frame();
            return r;
        };
        let line = self.current_line;
        let mut owned = args.to_vec();
        let mut orig = None;
        if let Err(e) = self.zpp_check(info, flags, &mut owned, &mut orig, false, line) {
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
        let r = f(self, &owned);
        self.pop_builtin_frame();
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
        if !named.is_empty() {
            if is_ucf || lname == b"forward_static_call" || lname == b"forward_static_call_array" {
                self.pending_named_args = named;
            } else if let Some(info) = crate::arginfo::lookup(lname) {
                if let Err(e) = self.zpp_resolve_named(info, &mut args, named) {
                    err = Some((e, false));
                }
            }
        }
        let mut orig = None;
        if err.is_none() {
            if let Some((info, flags)) = self.zpp_info(lname) {
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
                }
                let exc = self.create_exception(cls, &msg, line);
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
        }
        let r = f(self, &args);
        if own_frame {
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
/// Cases depending on literal arguments (defined(), in_array()) or on the
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
                | b"chr"
                | b"ord"
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
}

impl Vm {
    /// Register the frame of `op_array`, about to be run, for the
    /// call_stack frame pushed for it (if the top frame is that frame).
    /// Returns whether an entry was pushed; its `cvs` is set by the caller
    /// once the CV vector is in place, and the caller pops the entry when the
    /// function ends.
    #[inline]
    pub(crate) fn register_cv_frame(&mut self, op_array: &crate::opcode::OpArray) -> bool {
        if op_array.param_count == 0 {
            return false;
        }
        let Some(depth) = self.call_stack.len().checked_sub(1) else {
            return false;
        };
        let frame = &self.call_stack[depth];
        if frame.0.is_empty()
            || frame.3.is_empty()
            || self
                .cv_frames
                .last()
                .is_some_and(|f| f.name == frame.0.as_ptr())
            || !frame_is_for(&frame.0, op_array)
        {
            return false;
        }
        // A builtin's frame (the function called us without a frame of our own)
        if self
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
        if nfixed == 0 {
            return false;
        }
        self.cv_frames.push(CvFrame {
            depth,
            name: frame.0.as_ptr(),
            cvs: std::ptr::null(),
            base,
            nfixed,
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
            if f.cvs.is_null() {
                continue;
            }
            let at = if self
                .call_stack
                .get(f.depth)
                .is_some_and(|e| e.0.as_ptr() == f.name)
            {
                f.depth
            } else {
                match self.call_stack.iter().rposition(|e| e.0.as_ptr() == f.name) {
                    Some(i) => i,
                    None => continue,
                }
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

    /// A fiber suspends: hand it the CV frames of its call_stack frames,
    /// i.e. of its root frame (the fiber function's, at `root`) and above,
    /// made relative to the root.
    pub(crate) fn fiber_take_cv_frames(&mut self, root: usize) -> Vec<CvFrame> {
        let at = self
            .cv_frames
            .iter()
            .position(|f| f.depth >= root)
            .unwrap_or(self.cv_frames.len());
        let mut out = self.cv_frames.split_off(at);
        for f in &mut out {
            f.depth -= root;
        }
        out
    }

    /// A fiber resumes: re-install its CV frames once its root frame has
    /// been pushed again (the top frame), before the rest of its call_stack
    /// frames are appended.
    pub(crate) fn fiber_restore_cv_frames(&mut self, frames: &mut Vec<CvFrame>) {
        let Some(root) = self.call_stack.len().checked_sub(1) else {
            frames.clear();
            return;
        };
        let root_name = self.call_stack[root].0.as_ptr();
        for f in frames.iter_mut() {
            if f.depth == 0 {
                f.name = root_name;
            }
            f.depth += root;
        }
        self.cv_frames.append(frames);
    }

    /// A fiber ended: drop the CV frames of its root frame (at `root`) and above.
    pub(crate) fn fiber_drop_cv_frames(&mut self, root: usize) {
        while self.cv_frames.last().is_some_and(|f| f.depth >= root) {
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
    if name.starts_with("{closure") {
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
