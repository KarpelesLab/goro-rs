//! Diagnostics (PHP manual: Error Handling and Logging), exceptions and
//! their dispatch to handlers, backtraces.

use std::rc::Rc;

use goro_runtime::{ErrorKind, Key, ObjRef, PhpArray, PhpError, PhpStr, Value};

use super::{R, Unwind, Vm, frame_flags};
use crate::bytecode::slot_of;

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
pub const E_ALL: i64 = 32767;

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
        let msg_s = PhpStr::from(msg);
        self.err.last = Some((level, msg_s.clone(), file.clone(), line as i64));
        // User error handler (called regardless of error_reporting).
        if !self.err.in_handler
            && let Some((h, mask)) = self.err.handlers.last().cloned()
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
            self.err.in_handler = true;
            let args = vec![
                Value::Int(level),
                Value::Str(msg_s),
                Value::Str(file.clone()),
                Value::Int(line as i64),
            ];
            let r = self.call_value(&h, args);
            self.err.in_handler = false;
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
        let label = level_label(level);
        let file = String::from_utf8_lossy(file);
        if self.ini.log_errors {
            let text = format!("PHP {}:  {} in {} on line {}\n", label, msg, file, line);
            self.out.flush();
            self.write_stderr(text.as_bytes());
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
                    format!(
                        "<br />\n<b>{}</b>:  {} in <b>{}</b> on line <b>{}</b><br />\n",
                        label, msg, file, line
                    )
                } else {
                    format!("\n{}: {} in {} on line {}\n", label, msg, file, line)
                };
                self.echo(text.as_bytes());
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

    /// An exception was pending from a callback that could not fail.
    #[inline]
    pub fn check_deferred(&mut self) -> R<()> {
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
                self.throw_error_msg("Error", "Can only throw objects")
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
                let _ = i;
                if ip < tr.start {
                    continue;
                }
                if ip < tr.end && tr.catch != 0 {
                    self.truncate_pending(tr.call_depth);
                    self.cur_exception = Some(ex.clone());
                    self.ex.ip = tr.catch as usize;
                    handled = true;
                    break;
                }
                if tr.finally != 0 && ip < tr.finally {
                    self.truncate_pending(tr.call_depth);
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
                    {
                        self.chain_previous(&ex, prev);
                    }
                }
            }
            if handled {
                return Ok(());
            }
            if depth == entry {
                self.pop_frame();
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
                _ => self.instance_of_lname(&cls, &l),
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
            _ => format!("Unhandled match case of type {}", v.type_name_for_error()),
        }
    }

    // ------------------------------------------------------------ backtraces

    /// The call stack, innermost call first.
    pub fn backtrace(&self, with_args: bool) -> Vec<TraceFrame> {
        let mut out = Vec::new();
        let n = self.ex.frames.len();
        let mut natives = self.ex.native_calls.len();
        let mut i = n;
        loop {
            // Native calls made from frame i-1 (depth == i).
            while natives > 0 && self.ex.native_calls[natives - 1].depth as usize == i {
                let nc = &self.ex.native_calls[natives - 1];
                natives -= 1;
                let (file, line) = if i == 0 {
                    (None, 0)
                } else {
                    let f = &self.ex.frames[i - 1];
                    (Some(f.code().file.clone()), nc.line)
                };
                let scope = nc.func.scope();
                let args = if with_args {
                    Some(unsafe { (*nc.args).iter().map(|v| v.deref()).collect() })
                } else {
                    None
                };
                out.push(TraceFrame {
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
                });
            }
            if i <= 1 {
                break;
            }
            i -= 1;
            let f = &self.ex.frames[i];
            let code = f.code();
            if f.flags & frame_flags::GLOBAL != 0 {
                continue;
            }
            // Call site: in the caller frame, unless called from a native.
            let called_from_native =
                natives > 0 && self.ex.native_calls[natives - 1].depth as usize == i;
            let (file, line) = if called_from_native || f.flags & frame_flags::DTOR != 0 && false {
                (None, 0)
            } else {
                let caller = &self.ex.frames[i - 1];
                let cip = if i == n - 1 + 1 {
                    0
                } else {
                    caller.ip.saturating_sub(1)
                };
                (
                    Some(caller.code().file.clone()),
                    caller.code().line_of(cip as usize),
                )
            };
            if code.flags & crate::bytecode::fflags::PSEUDO_MAIN != 0 {
                // include / eval frame
                let fname = match f.line_hint {
                    2 => "include_once",
                    3 => "require",
                    4 => "require_once",
                    5 => "eval",
                    _ => "include",
                };
                let args = if with_args && fname != "eval" {
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
                });
                continue;
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
            let args = if with_args {
                let np = code.params.len();
                let mut v = Vec::new();
                let argc = f.argc as usize;
                for k in 0..argc.min(np) {
                    if code.params[k].variadic {
                        if let Some(Value::Array(a)) =
                            unsafe { (*f.base.add(k)).as_ref() }.map(|x| x.deref())
                        {
                            for (_, x) in a.iter() {
                                v.push(x.deref());
                            }
                        }
                        continue;
                    }
                    let x = unsafe { (*f.base.add(k)).as_ref() }
                        .map(|x| x.deref())
                        .unwrap_or_default();
                    v.push(x);
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
            out.push(TraceFrame {
                file,
                line,
                function: fname,
                class,
                call_type,
                object: f.this.clone(),
                args,
            });
        }
        let _ = slot_of;
        out
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
                out.push_str(
                    &c.deref()
                        .as_str()
                        .map(|s| s.to_string_lossy())
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
                    out.push_str(&trace_arg(&a.deref()));
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
        let s = match self.exception_to_string(&ex) {
            Ok(s) => s,
            Err(Unwind::Throw(_)) => {
                let cls = self.class_of(&ex).name.to_string_lossy();
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
                    other.type_name_for_error()
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

/// An argument as `getTraceAsString()` prints it.
pub fn trace_arg(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => goro_runtime::float_fmt::to_php_string(*f, 17),
        Value::Str(s) => {
            if s.len() > 15 {
                format!("'{}...'", String::from_utf8_lossy(&s[..15]))
            } else {
                format!("'{}'", String::from_utf8_lossy(s))
            }
        }
        Value::Array(_) => "Array".into(),
        Value::Object(o) => format!("Object({})", o.class_name().to_string_lossy()),
        Value::Resource(r) => format!("Resource id #{}", r.id()),
        Value::Ref(r) => trace_arg(&r.get()),
    }
}
