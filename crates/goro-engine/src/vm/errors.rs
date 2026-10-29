//! Diagnostics (PHP manual: Error Handling and Logging), exceptions and
//! their dispatch to handlers, backtraces.

use crate::class::{DisplayName, TypeName};
use std::rc::Rc;

use goro_runtime::{ErrorKind, Key, ObjRef, PhpArray, PhpError, PhpStr, Value};

use super::{R, Unwind, Vm, frame_flags};

pub const E_ERROR: i64 = 1;
pub const E_WARNING: i64 = 2;
pub const E_PARSE: i64 = 4;
pub const E_NOTICE: i64 = 8;
pub const E_CORE_ERROR: i64 = 16;
pub const E_CORE_WARNING: i64 = 32;
pub const E_COMPILE_ERROR: i64 = 64;
pub const E_COMPILE_WARNING: i64 = 128;
pub const E_USER_ERROR: i64 = 256;
pub const E_USER_WARNING: i64 = 512;
pub const E_USER_NOTICE: i64 = 1024;
pub const E_STRICT: i64 = 2048;
pub const E_RECOVERABLE_ERROR: i64 = 4096;
pub const E_DEPRECATED: i64 = 8192;
pub const E_USER_DEPRECATED: i64 = 16384;
pub const E_ALL: i64 = 30719;

pub fn level_label(level: i64) -> &'static str {
    match level {
        E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR => "Fatal error",
        E_RECOVERABLE_ERROR => "Recoverable fatal error",
        E_WARNING | E_CORE_WARNING | E_COMPILE_WARNING | E_USER_WARNING => "Warning",
        E_PARSE => "Parse error",
        E_NOTICE | E_USER_NOTICE => "Notice",
        E_STRICT => "Strict Standards",
        E_DEPRECATED | E_USER_DEPRECATED => "Deprecated",
        _ => "Unknown error",
    }
}

/// One entry of a backtrace.
#[derive(Clone)]
pub struct TraceFrame {
    pub file: Option<PhpStr>,
    pub line: u32,
    pub function: PhpStr,
    pub class: Option<PhpStr>,
    pub call_type: &'static str,
    pub object: Option<ObjRef>,
    pub args: Option<Vec<Value>>,
    /// Positions in `args` of #[\SensitiveParameter] parameters (shown
    /// as SensitiveParameterValue objects).
    pub sensitive: Vec<u32>,
}

fn is_sensitive(p: &crate::bytecode::ParamInfo) -> bool {
    p.attrs.iter().any(|a| {
        a.name
            .strip_prefix(b"\\")
            .unwrap_or(&a.name)
            .eq_ignore_ascii_case(b"SensitiveParameter")
    })
}

impl Vm {
    // ------------------------------------------------------------ diagnostics

    pub fn warn(&mut self, msg: &str) -> R<()> {
        self.raise(E_WARNING, msg)
    }

    pub fn notice_msg(&mut self, msg: &str) -> R<()> {
        self.raise(E_NOTICE, msg)
    }

    pub fn deprecated(&mut self, msg: &str) -> R<()> {
        self.raise(E_DEPRECATED, msg)
    }

    /// Raises a non-fatal diagnostic at the current location.
    pub fn raise(&mut self, level: i64, msg: &str) -> R<()> {
        let file = self.cur_file();
        let line = self.cur_line();
        self.raise_at(level, msg, file, line)
    }

    pub fn raise_at(&mut self, level: i64, msg: &str, file: PhpStr, line: u32) -> R<()> {
        // While a write path holds pointers into arrays, user error
        // handlers can't run (they could free them): queue the diagnostic.
        if self.err.defer_depth > 0 {
            self.err.queued.push((level, msg.to_string(), file, line));
            return Ok(());
        }
        let msg_s = PhpStr::from(msg);
        self.err.last = Some((level, msg_s.clone(), file.clone(), line as i64));
        // User error handler (called regardless of error_reporting). While
        // it runs it is not the handler: errors it raises go to a handler
        // it installs, or to the standard one (test Zend/tests/bug63206.phpt).
        if let Some((h, mask)) = self.err.handlers.last().cloned()
            && mask & level != 0
            && !h.is_null()
            && level
                & (E_ERROR
                    | E_CORE_ERROR
                    | E_COMPILE_ERROR
                    | E_PARSE
                    | E_CORE_WARNING
                    | E_COMPILE_WARNING)
                == 0
        {
            let depth = self.err.handlers.len();
            if let Some(top) = self.err.handlers.last_mut() {
                top.0 = Value::Null;
            }
            self.err.in_handler = true;
            let args = vec![
                Value::Int(level),
                Value::Str(msg_s),
                Value::Str(file.clone()),
                Value::Int(line as i64),
            ];
            let r = self.call_value(&h, args);
            self.err.in_handler = false;
            if self.err.handlers.len() == depth
                && let Some(top) = self.err.handlers.last_mut()
                && top.0.is_null()
            {
                top.0 = h.clone();
            }
            match r {
                Ok(Value::Bool(false)) => {}
                Ok(_) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
        if self.ini.error_reporting & level == 0 {
            return Ok(());
        }
        self.display_error(level, msg, &file, line);
        Ok(())
    }

    pub fn display_error(&mut self, level: i64, msg: &str, file: &[u8], line: u32) {
        // ignore_repeated_errors / ignore_repeated_source (PHP manual:
        // Error Handling and Logging Configuration Options).
        if self.ini_bool(b"ignore_repeated_errors", false) {
            let same_src = self.ini_bool(b"ignore_repeated_source", false);
            if let Some((m, f, l)) = &self.err.last_shown
                && m == msg
                && (same_src || (f.as_slice() == file && *l == line))
            {
                return;
            }
            self.err.last_shown = Some((msg.to_string(), file.to_vec(), line));
        }
        let label = level_label(level);
        let file = String::from_utf8_lossy(file);
        let fatal_level =
            level & (E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR | E_PARSE) != 0;
        if fatal_level
            && self.out.sapi.web()
            && self.ini.display_errors == 0
            && !crate::builtins::headers::sent(self)
        {
            // A fatal error with errors hidden makes the web server's
            // response a 500 (test sapi/cli/tests/bug43177).
            self.out.sapi.status = 500;
            self.out.sapi.status_line = Some(b"HTTP/1.0 500 Internal Server Error".to_vec());
        }
        if self.ini.log_errors {
            let text = format!("PHP {}:  {} in {} on line {}\n", label, msg, file, line);
            // to the error_log file when one is set, time-stamped (PHP
            // manual: error_log ini setting; test
            // general_functions/bug60723.phpt)
            let target = self.get_ini(b"error_log").unwrap_or_default();
            if !target.is_empty() && target.as_bytes() != b"syslog" {
                use std::io::Write;
                use std::os::unix::ffi::OsStrExt;
                let stamp = crate::builtins::date::log_timestamp(self);
                let p = self.fs_path(&target);
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(std::path::Path::new(std::ffi::OsStr::from_bytes(
                        p.as_os_str().as_bytes(),
                    )))
                    .and_then(|mut f| f.write_all(format!("[{stamp}] {text}").as_bytes()));
            } else {
                self.out.flush();
                self.write_stderr(text.as_bytes());
            }
        }
        match self.ini.display_errors {
            0 => {}
            2 => {
                let text = format!("{}: {} in {} on line {}\n", label, msg, file, line);
                self.out.flush();
                self.write_stderr(text.as_bytes());
            }
            _ => {
                let text = if self.ini.html_errors {
                    // The message is HTML-escaped (test
                    // sapi/cli/tests/gh13113).
                    let msg = msg
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                        .replace('"', "&quot;");
                    format!(
                        "<br />\n<b>{}</b>:  {} in <b>{}</b> on line <b>{}</b><br />\n",
                        label, msg, file, line
                    )
                } else {
                    format!("\n{}: {} in {} on line {}\n", label, msg, file, line)
                };
                // A fatal error inside an output handler is shown below
                // that level (observed on PHP 8.5).
                let fatal = level
                    & (E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR | E_PARSE)
                    != 0;
                match self.out.handler_level() {
                    Some(i) if fatal && self.out.redirect.is_none() => {
                        self.out.redirect = Some(i);
                        self.echo(text.as_bytes());
                        self.out.redirect = None;
                    }
                    _ => self.echo(text.as_bytes()),
                }
            }
        }
    }

    pub fn write_stderr(&mut self, b: &[u8]) {
        use std::io::Write;
        let _ = std::io::stderr().write_all(b);
    }

    /// Reports a fatal error and unwinds.
    pub fn fatal(&mut self, msg: &str) -> Unwind {
        let file = self.cur_file();
        let line = self.cur_line();
        self.fatal_at(msg, &file, line)
    }

    pub fn fatal_at(&mut self, msg: &str, file: &[u8], line: u32) -> Unwind {
        self.err.last = Some((E_ERROR, PhpStr::from(msg), PhpStr::new(file), line as i64));
        if self.ini.error_reporting & E_ERROR != 0 {
            self.display_error(E_ERROR, msg, file, line);
        }
        self.exit_status = 255;
        Unwind::Fatal
    }

    /// Raises the diagnostics queued while handlers couldn't run.
    pub fn flush_deferred(&mut self) -> R<()> {
        let saved = std::mem::replace(&mut self.err.defer_depth, 0);
        let q = std::mem::take(&mut self.err.queued);
        let mut r = Ok(());
        for (level, msg, file, line) in q {
            if r.is_ok() {
                r = self.raise_at(level, &msg, file, line);
            }
        }
        self.err.defer_depth = saved;
        r
    }

    /// An exception was pending from a callback that could not fail.
    #[inline]
    pub fn check_deferred(&mut self) -> R<()> {
        if self.err.deferred.is_none() {
            return Ok(());
        }
        match self.err.deferred.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    // ------------------------------------------------------------ throwing

    /// Converts a runtime error into a thrown exception.
    pub fn php_error(&mut self, e: PhpError) -> Unwind {
        if let Some(d) = self.err.deferred.take() {
            return d;
        }
        let class = match e.kind {
            ErrorKind::Error => "Error",
            ErrorKind::TypeError => "TypeError",
            ErrorKind::ValueError => "ValueError",
            ErrorKind::ArithmeticError => "ArithmeticError",
            ErrorKind::DivisionByZero => "DivisionByZeroError",
        };
        self.throw_error_msg(class, &e.message)
    }

    /// Creates an exception of internal class `class` with `msg` and
    /// returns it as an unwind.
    pub fn throw_error_msg(&mut self, class: &str, msg: &str) -> Unwind {
        match self.create_exception(class, msg, 0) {
            Ok(o) => Unwind::Throw(o),
            Err(e) => e,
        }
    }

    pub fn throw<T>(&mut self, class: &str, msg: &str) -> R<T> {
        Err(self.throw_error_msg(class, msg))
    }

    pub fn type_error<T>(&mut self, msg: &str) -> R<T> {
        self.throw("TypeError", msg)
    }

    pub fn value_error<T>(&mut self, msg: &str) -> R<T> {
        self.throw("ValueError", msg)
    }

    pub fn create_exception(&mut self, class: &str, msg: &str, code: i64) -> R<ObjRef> {
        let cls = match self.find_class(class.as_bytes(), false)? {
            Some(c) => c,
            None => panic!("exception class {} missing", class),
        };
        let o = self.instantiate(&cls)?;
        self.set_prop_raw(&o, b"message", Value::Str(PhpStr::from(msg)));
        if code != 0 {
            self.set_prop_raw(&o, b"code", Value::Int(code));
        }
        Ok(o)
    }

    /// `throw $v`.
    pub fn throw_value(&mut self, v: Value) -> Unwind {
        match v {
            Value::Object(o) => {
                let throwable = self.core.throwable.clone().unwrap();
                if self.class_of(&o).is_subclass_of(&throwable) {
                    return Unwind::Throw(o);
                }
                self.throw_error_msg(
                    "Error",
                    "Cannot throw objects that do not implement Throwable",
                )
            }
            _ => self.throw_error_msg("Error", "Can only throw objects"),
        }
    }

    /// Sets a declared property directly (no visibility, hooks or types).
    pub fn set_prop_raw(&mut self, o: &ObjRef, name: &[u8], v: Value) {
        let cls = self.class_of(o);
        if let Some(idx) = self.find_prop_slot_any(&cls, name) {
            let mut slots = o.obj().slots.borrow_mut();
            let old = std::mem::replace(&mut slots[idx], v);
            drop(slots);
            drop(old);
        } else {
            let old = o.obj().props.borrow_mut().insert(Key::from_bytes(name), v);
            drop(old);
        }
    }

    pub fn get_prop_raw(&self, o: &ObjRef, name: &[u8]) -> Value {
        let cls = self.class_of(o);
        if let Some(idx) = self.find_prop_slot_any(&cls, name) {
            let v = o.obj().slots.borrow()[idx].clone();
            if self.is_uninit(&v) {
                return Value::Null;
            }
            return v.deref();
        }
        o.obj()
            .props
            .borrow()
            .get_bytes(name)
            .map(|v| v.deref())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------ dispatch

    /// Finds a handler for `ex` in frames at depth >= `entry` (the entry
    /// frame is `frames[entry - 1]`). Jumps to it, or pops the frames and
    /// returns the exception.
    pub(crate) fn dispatch_exception(&mut self, mut ex: ObjRef, entry: usize) -> R<()> {
        loop {
            let depth = self.ex.frames.len();
            let ip = self.ex.ip.saturating_sub(1) as u32;
            let code = self.ex.frames[depth - 1].code;
            let code = unsafe { &*code };
            let mut handled = false;
            for (i, tr) in code.try_ranges.iter().enumerate().rev() {
                if ip < tr.start {
                    continue;
                }
                if !code.try_holes.is_empty()
                    && code
                        .try_holes
                        .iter()
                        .any(|&(s, e, t)| t == i as u32 && s <= ip && ip < e)
                {
                    continue;
                }
                if ip < tr.end && tr.catch != 0 {
                    self.truncate_pending(tr.call_depth);
                    self.free_live_ranges(code, ip, tr.catch);
                    self.cur_exception = Some(ex.clone());
                    self.ex.ip = tr.catch as usize;
                    handled = true;
                    break;
                }
                if tr.finally != 0 && ip < tr.finally {
                    self.truncate_pending(tr.call_depth);
                    self.free_live_ranges(code, ip, tr.finally);
                    let p = self.slot_ptr(tr.exc_slot);
                    let old = unsafe { (*p).replace(Value::Object(ex.clone())) };
                    drop(old);
                    let p = self.slot_ptr(tr.finally_ret);
                    let old = unsafe { (*p).replace(Value::Null) };
                    drop(old);
                    self.ex.ip = tr.finally as usize;
                    handled = true;
                    break;
                }
                if tr.finally != 0 && ip >= tr.finally && ip < tr.finally_end {
                    // Thrown inside a finally block while an exception was
                    // pending: the pending one becomes the previous of the
                    // new one.
                    let p = self.slot_ptr(tr.exc_slot);
                    if let Some(Value::Object(prev)) = unsafe { (*p).take() }
                        && !ObjRef::ptr_eq(&prev, &ex)
                        && !matches!(
                            prev.class_name().as_bytes(),
                            b"GeneratorExit_" | b"FiberExit_"
                        )
                    {
                        self.chain_previous(&ex, prev);
                    }
                }
            }
            if handled {
                return Ok(());
            }
            // A constructor run by `new` that throws: the object's
            // destructor is never called (Zend/tests/try/catch_002.phpt).
            {
                let fr = &self.ex.frames[depth - 1];
                if fr.flags & super::frame_flags::CTOR != 0
                    && let Some(t) = &fr.this
                {
                    t.mark_destructor_called();
                }
            }
            if depth == entry {
                self.pop_frame();
                // Destructors of the frame's objects may throw too (test
                // Zend/tests/ex_func_null_during_property_write.phpt).
                if goro_runtime::object::has_pending_destructors()
                    && let Err(Unwind::Throw(e2)) = self.run_pending_destructors()
                {
                    self.chain_previous(&e2, ex);
                    return Err(Unwind::Throw(e2));
                }
                return Err(Unwind::Throw(ex));
            }
            self.pop_frame();
            // A destructor run while unwinding may throw: chain it.
            if goro_runtime::object::has_pending_destructors()
                && let Err(Unwind::Throw(e2)) = self.run_pending_destructors()
            {
                self.chain_previous(&e2, ex);
                ex = e2;
            }
        }
    }

    /// Frees the loop temporaries live at `ip` but not at the handler
    /// `target`, innermost first (Zend/tests/try/try_finally_020.phpt).
    fn free_live_ranges(&mut self, code: &crate::bytecode::FuncCode, ip: u32, target: u32) {
        if code.live_ranges.is_empty() {
            return;
        }
        let mut v: Vec<(u32, u32, crate::bytecode::Reg, bool)> = code
            .live_ranges
            .iter()
            .copied()
            .filter(|&(s, e, _, _)| s <= ip && ip < e && !(s <= target && target < e))
            .collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, _, slot, is_iter) in v {
            if is_iter {
                let _ = self.fe_free(slot);
            } else {
                let p = self.slot_ptr(slot);
                let old = unsafe { (*p).take() };
                drop(old);
            }
        }
    }

    /// Appends `prev` at the end of `ex`'s previous chain.
    pub(crate) fn chain_previous(&mut self, ex: &ObjRef, prev: ObjRef) {
        let mut cur = ex.clone();
        let mut guard = 0;
        loop {
            let p = self.get_prop_raw(&cur, b"previous");
            match p {
                Value::Object(o) => {
                    if ObjRef::ptr_eq(&o, &prev) {
                        return;
                    }
                    cur = o;
                }
                _ => break,
            }
            guard += 1;
            if guard > 10000 {
                return;
            }
        }
        if ObjRef::ptr_eq(&cur, &prev) {
            return;
        }
        // No cycle: `ex` must not already be in prev's chain
        // (Zend/tests/try/try_finally_recursive_previous.phpt).
        let mut p = prev.clone();
        for _ in 0..10000 {
            if ObjRef::ptr_eq(&p, ex) {
                return;
            }
            match self.get_prop_raw(&p, b"previous") {
                Value::Object(o) => p = o,
                _ => break,
            }
        }
        self.set_prop_raw(&cur, b"previous", Value::Object(prev));
    }

    /// `catch (A|B $e)`: binds the current exception if it matches,
    /// otherwise jumps to the next handler.
    pub(crate) fn op_catch(&mut self, list: u32, var: crate::bytecode::Reg, next: u32) -> R<()> {
        let ex = self.cur_exception.clone().expect("catch without exception");
        let cls = self.class_of(&ex);
        let n = self.code_ref().lists[list as usize].len();
        let mut matched = false;
        for i in 0..n {
            let k = self.code_ref().lists[list as usize][i];
            let name = self.code_ref().consts[(k & !crate::bytecode::CONST) as usize].clone();
            let Value::Str(name) = name else { continue };
            let l = crate::class::lower(name.strip_prefix(b"\\").unwrap_or(&name));
            let ok = match l.as_bytes() {
                b"self" | b"static" | b"parent" => {
                    let spec = match l.as_bytes() {
                        b"self" => crate::bytecode::ClassSpec::SelfRef,
                        b"static" => crate::bytecode::ClassSpec::Static,
                        _ => crate::bytecode::ClassSpec::Parent,
                    };
                    match self.resolve_class_spec(spec, false) {
                        Ok(c) => cls.is_subclass_of(&c),
                        Err(_) => false,
                    }
                }
                _ => {
                    self.instance_of_lname(&cls, &l)
                        // Class aliases (class_alias(), internal aliases
                        // such as Dom\DOMException; test
                        // ext/dom/tests/modern/xml/canonicalize_unattached.phpt).
                        || self
                            .classes
                            .get(&l)
                            .is_some_and(|c| c.lname != l && cls.is_subclass_of(c))
                }
            };
            if ok {
                matched = true;
                break;
            }
        }
        if !matched {
            self.ex.ip = next as usize;
            return Ok(());
        }
        let ex = self.cur_exception.take().unwrap();
        if var != crate::bytecode::NONE {
            self.assign_var(var, Value::Object(ex))?;
        }
        Ok(())
    }

    /// Drops pending calls opened in the current frame after depth `d`.
    pub(crate) fn truncate_pending(&mut self, d: u32) {
        let base = self.frame().pending_base as usize + d as usize;
        while self.ex.pending.len() > base {
            let p = self.ex.pending.pop().unwrap();
            self.discard_pending(p);
        }
        let nb = self.frame().native_base as usize;
        self.ex
            .native_calls
            .truncate(nb.max(self.ex.native_calls.len().min(nb)));
    }

    pub(crate) fn match_error_desc(&mut self, v: &Value) -> String {
        match v {
            Value::Str(s) => {
                let mut out = String::from("Unhandled match case ");
                out.push_str(&php_quote(s));
                out
            }
            Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::Null => {
                let mut out = Vec::new();
                self.var_export_into(&mut out, v, 0).ok();
                format!("Unhandled match case {}", String::from_utf8_lossy(&out))
            }
            _ => format!("Unhandled match case of type {}", v.tname()),
        }
    }

    // ------------------------------------------------------------ backtraces

    /// The call stack, innermost call first.
    pub fn backtrace(&self, with_args: bool) -> Vec<TraceFrame> {
        let mut out = Vec::new();
        self.backtrace_ex(&self.ex, with_args, &mut out);
        // Inside a fiber: the resumers' stacks follow.
        for ex in self.ex_parents.iter().rev() {
            self.backtrace_ex(ex, with_args, &mut out);
        }
        out
    }

    fn native_trace_frame(
        &self,
        ex: &super::ExecState,
        nc: &super::NativeCall,
        i: usize,
        with_args: bool,
    ) -> TraceFrame {
        let (file, line) = if i == 0 || ex.frames[i - 1].code().file.is_empty() {
            (None, 0)
        } else {
            let f = &ex.frames[i - 1];
            (Some(f.code().file.clone()), nc.line)
        };
        let scope = nc.func.scope();
        let args: Option<Vec<Value>> = if with_args {
            Some(unsafe { (*nc.args).iter().map(|v| v.deref()).collect() })
        } else {
            None
        };
        let mut sensitive = Vec::new();
        if let (Some(args), crate::func::FuncKind::Native { info, .. }) = (&args, &nc.func.kind)
            && let Some(sig) = info.get()
        {
            for k in 0..args.len() {
                let p = sig
                    .params
                    .get(k)
                    .or_else(|| sig.params.last().filter(|p| p.variadic));
                if p.is_some_and(is_sensitive) {
                    sensitive.push(k as u32);
                }
            }
        }
        TraceFrame {
            file,
            line,
            function: nc.func.name.clone(),
            class: scope.map(|c| c.name.clone()),
            call_type: if scope.is_some() {
                if nc.this.is_some() { "->" } else { "::" }
            } else {
                ""
            },
            object: nc.this.clone(),
            args,
            sensitive,
        }
    }

    pub(crate) fn backtrace_ex(
        &self,
        ex: &super::ExecState,
        with_args: bool,
        out: &mut Vec<TraceFrame>,
    ) {
        let n = ex.frames.len();
        let mut natives = ex.native_calls.len();
        let mut i = n;
        // call_user_func() called from user code is no frame of its own:
        // PHP compiles it to a direct call (observed on PHP 8.5; tests
        // Zend/tests/gh16799, exceptions/bug73338).
        let direct_cuf = |k: usize| -> bool {
            let nc = &ex.native_calls[k];
            let d = nc.depth as usize;
            nc.func.scope().is_none()
                && matches!(
                    nc.func.lname.as_bytes(),
                    b"call_user_func" | b"call_user_func_array"
                )
                && d > 0
                && !ex.frames[d - 1].code().file.is_empty()
                // (in a namespace an unqualified name is resolved at run time)
                && !ex.frames[d - 1].code().in_namespace
                && !(k > 0 && ex.native_calls[k - 1].depth as usize == d)
        };
        // A generator destroyed (its `finally` running) is no frame of its
        // own either (test Zend/tests/generators/errors/
        // yield_in_force_closed_finally_error).
        let gen_dtor = |k: usize| -> bool {
            let nc = &ex.native_calls[k];
            nc.func.lname.as_bytes() == b"__destruct"
                && nc
                    .func
                    .scope()
                    .is_some_and(|c| c.lname.as_bytes() == b"generator")
        };
        loop {
            // Native calls made from frame i-1 (depth == i).
            while natives > 0 && ex.native_calls[natives - 1].depth as usize == i {
                let nc = &ex.native_calls[natives - 1];
                natives -= 1;
                // A fiber destroyed (its function unwinding) is no frame
                // either (tests Zend/tests/fibers/suspend-in-force-close-fiber,
                // throw-during-fiber-destruct).
                let fiber_dtor = nc.func.lname.as_bytes() == b"__destruct"
                    && nc
                        .func
                        .scope()
                        .is_some_and(|c| c.lname.as_bytes() == b"fiber");
                if direct_cuf(natives) || gen_dtor(natives) || fiber_dtor {
                    continue;
                }
                let mut e = self.native_trace_frame(ex, nc, i, with_args);
                // Called by another internal function (call_user_func):
                // no call site (test ReflectionFiber_notrace_2.phpt).
                if natives > 0 && ex.native_calls[natives - 1].depth as usize == i {
                    e.file = None;
                    e.line = 0;
                }
                out.push(e);
            }
            if i == 0 {
                break;
            }
            i -= 1;
            let f = &ex.frames[i];
            let code = f.code();
            if f.flags & frame_flags::GLOBAL != 0 {
                continue;
            }
            // A constant expression evaluated by another one shows only the
            // outer "[constant expression]" frame (test
            // Zend/tests/constants/bug41633_3).
            if code.flags & crate::bytecode::fflags::CONST_EXPR != 0
                && i > 0
                && ex.frames[i - 1].code().flags & crate::bytecode::fflags::CONST_EXPR != 0
            {
                continue;
            }
            // Call site: in the caller frame, unless called from a native
            // (or from the engine itself: shutdown functions, fibers).
            let called_from_native =
                natives > 0 && ex.native_calls[natives - 1].depth as usize == i;
            let (file, line) = if called_from_native
                && (direct_cuf(natives - 1)
                    || (gen_dtor(natives - 1) && i > 0 && !ex.frames[i - 1].code().file.is_empty()))
            {
                (
                    Some(ex.frames[i - 1].code().file.clone()),
                    ex.native_calls[natives - 1].line,
                )
            } else if called_from_native || i == 0 || ex.frames[i - 1].code().file.is_empty() {
                (None, 0)
            } else {
                let caller = &ex.frames[i - 1];
                let cip = caller.ip.saturating_sub(1);
                (
                    Some(caller.code().file.clone()),
                    caller.code().line_of(cip as usize),
                )
            };
            if code.flags & crate::bytecode::fflags::PSEUDO_MAIN != 0 {
                // (6: a front controller's file, without a frame of its own)
                if f.flags & frame_flags::ATTACHED == 0 || f.line_hint == 6 {
                    continue;
                }
                let fname = match f.line_hint {
                    2 => "include_once",
                    3 => "require",
                    4 => "require_once",
                    5 => "eval",
                    _ => "include",
                };
                // The innermost frame (code of the included file itself
                // running) shows no argument (observed on PHP 8.5; test
                // sapi/cli/tests/php_cli_server_015).
                let called_above = out.iter().any(|f| {
                    !matches!(
                        f.function.as_bytes(),
                        b"debug_backtrace" | b"debug_print_backtrace"
                    )
                });
                let args = if with_args && fname != "eval" && called_above {
                    Some(vec![Value::Str(code.file.clone())])
                } else if with_args {
                    Some(Vec::new())
                } else {
                    None
                };
                out.push(TraceFrame {
                    file,
                    line,
                    function: PhpStr::from(fname),
                    class: None,
                    call_type: "",
                    object: None,
                    args,
                    sensitive: Vec::new(),
                });
                continue;
            }
            // A generator run through `yield from`: the delegating
            // generators follow, each called where the next one stands
            // (tests Zend/tests/generators/gh15275-*, bug74954).
            if let Some(gx) = &f.generator
                && !self.gen_delegators.is_empty()
            {
                let mut chain: Vec<ObjRef> = Vec::new();
                let mut cur = gx.clone();
                while let Some((_, o)) = self
                    .gen_delegators
                    .iter()
                    .rev()
                    .find(|(inner, _)| ObjRef::ptr_eq(inner, &cur))
                {
                    if chain.iter().any(|c| ObjRef::ptr_eq(c, o)) {
                        break;
                    }
                    chain.push(o.clone());
                    cur = o.clone();
                }
                if !chain.is_empty() {
                    let site = |k: usize| match self.generator_executing(&chain[k]) {
                        Some((f, l)) => (Some(f), l),
                        None => (None, 0),
                    };
                    let (f0, l0) = site(0);
                    if let Some(e) = self.func_frame_entry(f, f0, l0, with_args) {
                        out.push(e);
                    }
                    for k in 0..chain.len() {
                        let (sf, sl) = if k + 1 < chain.len() {
                            site(k + 1)
                        } else {
                            (file.clone(), line)
                        };
                        if let Some(Some(e)) = self.with_gen(&chain[k], |d| {
                            d.frame
                                .as_ref()
                                .and_then(|fr| self.func_frame_entry(fr, sf.clone(), sl, with_args))
                        }) {
                            out.push(e);
                        }
                    }
                    continue;
                }
            }
            if let Some(e) = self.func_frame_entry(f, file, line, with_args) {
                out.push(e);
            }
        }
    }

    /// The backtrace entry of a function frame called from `file`:`line`
    /// (none for frames traces leave out).
    pub(crate) fn func_frame_entry(
        &self,
        f: &super::Frame,
        file: Option<PhpStr>,
        line: u32,
        with_args: bool,
    ) -> Option<TraceFrame> {
        let code = f.code();
        if code.flags & crate::bytecode::fflags::CONST_EXPR != 0 {
            // Errors of class constant lookups in constant expressions show
            // the expression as a frame (test
            // Zend/tests/constants/bug41633_2.phpt; observed on PHP 8.5).
            if !self.trace_const_expr {
                return None;
            }
            return Some(TraceFrame {
                file,
                line,
                function: PhpStr::from("[constant expression]"),
                class: None,
                call_type: "",
                object: None,
                args: with_args.then(Vec::new),
                sensitive: Vec::new(),
            });
        }
        // Helpers of internal classes written in PHP are not shown.
        if code.file.is_empty() && f.func.name.starts_with(b"__h_") {
            return None;
        }
        let scope = f.func.scope();
        let is_closure = code.flags & crate::bytecode::fflags::CLOSURE != 0;
        let class = if is_closure {
            if f.scope.is_null() {
                None
            } else {
                Some(unsafe { (*f.scope).name.clone() })
            }
        } else {
            scope.map(|c| c.name.clone())
        };
        let call_type = if class.is_some() {
            if f.this.is_some() { "->" } else { "::" }
        } else {
            ""
        };
        let mut sensitive = Vec::new();
        let args = if with_args {
            let np = code.params.len();
            let mut v = Vec::new();
            let argc = f.argc as usize;
            for k in 0..argc.min(np) {
                let x = unsafe { (*f.base.add(k)).as_ref() }.map(|x| x.deref());
                let sens = is_sensitive(&code.params[k]);
                if code.params[k].variadic {
                    if let Some(Value::Array(a)) = x {
                        for (_, y) in a.iter() {
                            if sens {
                                sensitive.push(v.len() as u32);
                            }
                            v.push(y.deref());
                        }
                    }
                    continue;
                }
                if sens {
                    sensitive.push(v.len() as u32);
                }
                v.push(x.unwrap_or_default());
            }
            for x in &f.extra_args {
                v.push(x.deref());
            }
            Some(v)
        } else {
            None
        };
        let fname = if is_closure {
            code.name.clone()
        } else {
            f.func.name.clone()
        };
        Some(TraceFrame {
            file,
            line,
            function: fname,
            class,
            call_type,
            object: f.this.clone(),
            args,
            sensitive,
        })
    }

    /// The call stack with #[\SensitiveParameter] arguments wrapped in
    /// SensitiveParameterValue objects (PHP manual: SensitiveParameter;
    /// RFC: Redacting parameters in back traces).
    pub fn backtrace_sealed(&mut self, with_args: bool) -> Vec<TraceFrame> {
        let mut frames = self.backtrace(with_args);
        // Exceptions thrown by sodium functions carry no argument values
        // (test ext/sodium/tests/sodium_error_001: "TypeErrors will not
        // contain param values in backtrace").
        if frames
            .first()
            .is_some_and(|f| f.class.is_none() && f.function.starts_with(b"sodium_"))
        {
            for f in frames.iter_mut() {
                f.args = None;
                f.sensitive.clear();
            }
            return frames;
        }
        if frames.iter().all(|f| f.sensitive.is_empty()) {
            return frames;
        }
        let Ok(Some(cls)) = self.find_class(b"SensitiveParameterValue", false) else {
            return frames;
        };
        for f in frames.iter_mut() {
            let idx = std::mem::take(&mut f.sensitive);
            if let Some(args) = &mut f.args {
                for i in idx {
                    if let Some(slot) = args.get_mut(i as usize) {
                        let v = std::mem::take(slot);
                        if let Ok(o) = self.instantiate_raw(&cls) {
                            self.set_prop_raw(&o, b"value", v);
                            *slot = Value::Object(o);
                        }
                    }
                }
            }
        }
        frames
    }

    /// The trace as the array stored in exceptions / returned by
    /// `debug_backtrace()`.
    pub fn trace_array(&self, frames: &[TraceFrame], with_object: bool) -> PhpArray {
        let mut arr = PhpArray::new();
        for f in frames {
            let mut e = PhpArray::new();
            if let Some(file) = &f.file {
                e.insert(Key::from("file"), Value::Str(file.clone()));
                e.insert(Key::from("line"), Value::Int(f.line as i64));
            }
            e.insert(Key::from("function"), Value::Str(f.function.clone()));
            if let Some(c) = &f.class {
                e.insert(Key::from("class"), Value::Str(c.clone()));
                if with_object && let Some(o) = &f.object {
                    e.insert(Key::from("object"), Value::Object(o.clone()));
                }
                e.insert(Key::from("type"), Value::Str(PhpStr::from(f.call_type)));
            }
            if let Some(args) = &f.args {
                e.insert(
                    Key::from("args"),
                    Value::Array(PhpArray::from_values(args.iter().cloned())),
                );
            }
            let _ = arr.push(Value::Array(e));
        }
        arr
    }

    /// `getTraceAsString()` format.
    pub fn trace_string(&mut self, trace: &PhpArray) -> String {
        let max_len = self
            .get_ini(b"zend.exception_string_param_max_len")
            .map(|v| goro_runtime::convert::str_to_int(&v).clamp(0, 1_000_000) as usize)
            .unwrap_or(15);
        let mut out = String::new();
        let mut i = 0;
        for (_, fr) in trace.iter() {
            let Value::Array(fr) = fr.deref() else {
                continue;
            };
            out.push_str(&format!("#{} ", i));
            match (fr.get_bytes(b"file"), fr.get_bytes(b"line")) {
                (Some(f), Some(l)) => {
                    let f = f.deref();
                    let l = l.deref();
                    out.push_str(&format!(
                        "{}({}): ",
                        String::from_utf8_lossy(f.as_str().map(|s| s.as_bytes()).unwrap_or(b"")),
                        match l {
                            Value::Int(n) => n,
                            _ => 0,
                        }
                    ));
                }
                _ => out.push_str("[internal function]: "),
            }
            if let Some(c) = fr.get_bytes(b"class") {
                // Anonymous class names end at their NUL (observed on PHP
                // 8.5: "class@anonymous->m()").
                out.push_str(
                    &c.deref()
                        .as_str()
                        .map(crate::class::DisplayName::dname)
                        .unwrap_or_default(),
                );
                out.push_str(
                    &fr.get_bytes(b"type")
                        .map(|t| {
                            t.deref()
                                .as_str()
                                .map(|s| s.to_string_lossy())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default(),
                );
            }
            out.push_str(
                &fr.get_bytes(b"function")
                    .map(|t| {
                        t.deref()
                            .as_str()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
            );
            out.push('(');
            if let Some(Value::Array(args)) = fr.get_bytes(b"args").map(|a| a.deref()) {
                let mut first = true;
                for (_, a) in args.iter() {
                    if !first {
                        out.push_str(", ");
                    }
                    first = false;
                    out.push_str(&trace_arg_max(&a.deref(), max_len));
                }
            }
            out.push_str(")\n");
            i += 1;
        }
        out.push_str(&format!("#{} {{main}}", i));
        out
    }

    /// Reports an exception nobody caught ("Fatal error: Uncaught ...").
    pub fn report_uncaught(&mut self, ex: ObjRef) {
        // Exception handler set by the script.
        if let Some(h) = self.err.exception_handlers.last().cloned()
            && !h.is_null()
        {
            self.err.exception_handlers.pop();
            match self.call_value(&h, vec![Value::Object(ex)]) {
                Ok(_) => {
                    self.exit_status = 255;
                    return;
                }
                Err(Unwind::Throw(e2)) => {
                    self.report_uncaught(e2);
                    return;
                }
                Err(_) => return,
            }
        }
        // Parse and compile errors that nobody caught are reported as such.
        let cls = self.class_of(&ex);
        let is_parse = self.instance_of_lname(&cls, b"parseerror");
        if is_parse || self.instance_of_lname(&cls, b"compileerror") {
            let msg = self.get_prop_raw(&ex, b"message");
            let msg = msg
                .as_str()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();
            let file = self.get_prop_raw(&ex, b"file");
            let file = file.as_str().cloned().unwrap_or_default();
            let line = match self.get_prop_raw(&ex, b"line") {
                Value::Int(n) => n as u32,
                _ => 0,
            };
            let level = if is_parse { E_PARSE } else { E_COMPILE_ERROR };
            self.err.last = Some((level, PhpStr::from(msg.clone()), file.clone(), line as i64));
            if self.ini.error_reporting & level != 0 {
                self.display_error(level, &msg, &file, line);
            }
            self.exit_status = 255;
            return;
        }
        let s = match self.exception_to_string(&ex) {
            Ok(s) => s,
            Err(Unwind::Throw(_)) => {
                let cls = self.class_of(&ex).name.dname();
                format!("{} (exception thrown in __toString)", cls)
            }
            Err(_) => return,
        };
        let file = self.get_prop_raw(&ex, b"file");
        let line = self.get_prop_raw(&ex, b"line");
        let file = file
            .as_str()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let line = match line {
            Value::Int(n) => n,
            _ => 0,
        };
        let msg = format!("Uncaught {}\n  thrown", s);
        self.err.last = Some((
            E_ERROR,
            PhpStr::from(msg.clone()),
            PhpStr::from(file.clone()),
            line,
        ));
        if self.ini.error_reporting & E_ERROR != 0 {
            self.display_error(E_ERROR, &msg, file.as_bytes(), line as u32);
        }
        self.exit_status = 255;
    }

    /// An uncaught exception reported as a warning ("Warning: Uncaught
    /// ..."): assert.bail after a failing assert callback (tests
    /// Zend/tests/assert/gh16293_001.phpt, gh16293_002.phpt).
    pub(crate) fn warn_uncaught(&mut self, ex: &ObjRef) {
        let Ok(s) = self.exception_to_string(ex) else {
            return;
        };
        let file = self.get_prop_raw(ex, b"file");
        let file = file.as_str().cloned().unwrap_or_default();
        let line = match self.get_prop_raw(ex, b"line") {
            Value::Int(n) => n,
            _ => 0,
        };
        let msg = format!("Uncaught {}\n  thrown", s);
        if self.ini.error_reporting & E_WARNING != 0 {
            self.display_error(E_WARNING, &msg, file.as_bytes(), line as u32);
        }
    }

    pub fn exception_to_string(&mut self, ex: &ObjRef) -> R<String> {
        let v = self.call_method(ex, b"__tostring", Vec::new())?;
        Ok(match v {
            Value::Str(s) => s.to_string_lossy(),
            other => {
                let s = self.to_php_str(&other)?;
                s.to_string_lossy()
            }
        })
    }

    pub(crate) fn do_exit(&mut self, v: Option<Value>) -> R<Unwind> {
        match v {
            None => {}
            Some(Value::Int(n)) => self.exit_status = n as i32,
            Some(Value::Str(s)) => {
                self.echo(&s);
            }
            Some(Value::Bool(_)) | Some(Value::Float(_)) | Some(Value::Null) => {
                // exit(true) etc.: deprecated non-int status; PHP 8.4
                // makes exit() a function: bool → int, others TypeError.
                let v = v.unwrap();
                match v {
                    Value::Bool(b) => self.exit_status = b as i32,
                    Value::Float(f) => self.exit_status = f as i32,
                    _ => {
                        self.deprecated("exit(): Passing null to parameter #1 ($status) of type string|int is deprecated")?;
                    }
                }
            }
            Some(other) => {
                return self.type_error(&format!(
                    "exit(): Argument #1 ($status) must be of type string|int, {} given",
                    other.tname()
                ));
            }
        }
        Ok(Unwind::Exit)
    }

    pub fn class_of(&self, o: &ObjRef) -> Rc<crate::class::Class> {
        let c: &Rc<dyn goro_runtime::ClassInfo> = o.class();
        let any: &dyn std::any::Any = &**c;
        let p = any
            .downcast_ref::<crate::class::Class>()
            .expect("foreign class") as *const crate::class::Class;
        self.rc_class(p)
    }
}

/// `'abc...'` style quoting used in traces and match errors.
pub fn php_quote(s: &[u8]) -> String {
    let mut out = String::from("'");
    out.push_str(
        &String::from_utf8_lossy(s)
            .replace('\\', "\\\\")
            .replace('\'', "\\'"),
    );
    out.push('\'');
    out
}

/// Control and non-ASCII bytes of a trace argument as C-style escapes
/// (observed on PHP 8.5: `f('a\nb\x01\xC3\xA9')`).
fn trace_escape(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s {
        match c {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x0b => out.push_str("\\v"),
            0x0c => out.push_str("\\f"),
            0x1b => out.push_str("\\e"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(c as char),
            _ => out.push_str(&format!("\\x{:02X}", c)),
        }
    }
    out
}

/// An argument as `getTraceAsString()` prints it.
pub fn trace_arg(v: &Value) -> String {
    trace_arg_max(v, 15)
}

/// The same with strings cut at `max` bytes (zend.exception_string_param_max_len).
pub fn trace_arg_max(v: &Value, max: usize) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            // Integral floats keep a ".0" (observed on PHP 8.5: "f(1.0)").
            let mut s = goro_runtime::float_fmt::to_php_string(*f, 17);
            if f.is_finite() && !s.contains(['.', 'E', 'e']) {
                s.push_str(".0");
            }
            s
        }
        Value::Str(s) => {
            let (head, more) = if s.len() > max {
                (&s[..max], "...")
            } else {
                (&s[..], "")
            };
            format!("'{}{}'", trace_escape(head), more)
        }
        Value::Array(_) => "Array".into(),
        Value::Object(o) => {
            // Enum cases show as Foo::Bar (test Zend/tests/enum/enum_in_stack_trace.phpt).
            let c: &Rc<dyn goro_runtime::ClassInfo> = o.class();
            let any: &dyn std::any::Any = &**c;
            if let Some(cls) = any.downcast_ref::<crate::class::Class>()
                && cls.kind == crate::class::ClassKind::Enum
                && let Some(Value::Str(n)) = o.obj().slots.borrow().first().map(|v| v.deref())
            {
                return format!("{}::{}", cls.name.dname(), n.to_string_lossy());
            }
            format!("Object({})", o.class_name().to_string_lossy())
        }
        Value::Resource(r) => format!("Resource id #{}", r.id()),
        Value::Ref(r) => trace_arg_max(&r.get(), max),
    }
}
