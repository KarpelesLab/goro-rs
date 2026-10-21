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
