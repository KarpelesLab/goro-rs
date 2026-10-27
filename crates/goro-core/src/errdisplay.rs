//! Error display (php_error_cb / php_verror presentation): display_errors,
//! html_errors, docref links, error_prepend_string / error_append_string.

use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

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
pub const E_RECOVERABLE_ERROR: i64 = 4096;
pub const E_DEPRECATED: i64 = 8192;
pub const E_USER_DEPRECATED: i64 = 16384;
pub const E_ALL: i64 = 30719;
pub const E_CORE: i64 = E_CORE_ERROR | E_CORE_WARNING;
/// Errors the `@` operator does not silence.
pub const E_FATAL_ERRORS: i64 =
    E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR | E_RECOVERABLE_ERROR | E_PARSE;
/// `error_reporting` marker for regions where the engine itself emits no
/// diagnostics at all (isset(), empty(), `??`, internal probing): errors
/// raised there are dropped without reaching a user error handler, unlike
/// `@`-silenced ones. Compared by value (a user level of -1 sets every bit).
pub const ER_SILENT: i64 = 1 << 40;

thread_local! {
    static ERROR_DISPLAY_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Errors reaching the default handler are not displayed (only logged):
/// ext/soap's handler runs it with display_errors off.
pub fn set_error_display_suppressed(off: bool) {
    ERROR_DISPLAY_SUPPRESSED.with(|s| s.set(off));
}

/// ZEND_BEGIN_SILENCE: the error_reporting level inside `@` (`user`) or an
/// engine-internal silent region.
pub fn begin_silence(cur: i64, user: bool) -> i64 {
    if !user {
        ER_SILENT
    } else if cur & !E_FATAL_ERRORS != 0 {
        // Fatal errors are not silenced.
        cur & E_FATAL_ERRORS
    } else {
        cur
    }
}

/// ZEND_END_SILENCE: restore `saved` unless the silenced code changed
/// error_reporting to something that is not only-fatal.
pub fn end_silence(cur: i64, saved: i64) -> i64 {
    if cur == ER_SILENT || (cur & !E_FATAL_ERRORS == 0 && saved & !E_FATAL_ERRORS != 0) {
        saved
    } else {
        cur
    }
}

/// zend_fcc_equals()-like comparison of two callables.
pub fn callables_equal(a: &Value, b: &Value) -> bool {
    let (a, b) = (a.deref(), b.deref());
    match (&a, &b) {
        (Value::String(x), Value::String(y)) => {
            let x = x.as_bytes();
            let y = y.as_bytes();
            x.strip_prefix(b"\\")
                .unwrap_or(x)
                .eq_ignore_ascii_case(y.strip_prefix(b"\\").unwrap_or(y))
        }
        (Value::Object(x), Value::Object(y)) => std::rc::Rc::ptr_eq(x, y),
        (Value::Array(x), Value::Array(y)) => {
            if std::rc::Rc::ptr_eq(x, y) {
                return true;
            }
            let x: Vec<Value> = x.borrow().values().cloned().collect();
            let y: Vec<Value> = y.borrow().values().cloned().collect();
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| callables_equal(p, q))
        }
        _ => false,
    }
}

/// php_error_cb()'s label for an error type.
pub fn error_type_label(ty: i64) -> &'static str {
    match ty {
        E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR => "Fatal error",
        E_RECOVERABLE_ERROR => "Recoverable fatal error",
        E_WARNING | E_CORE_WARNING | E_COMPILE_WARNING | E_USER_WARNING => "Warning",
        E_PARSE => "Parse error",
        E_NOTICE | E_USER_NOTICE => "Notice",
        E_DEPRECATED | E_USER_DEPRECATED => "Deprecated",
        _ => "Unknown error",
    }
}

/// A compile error the parser raises as a thrown CompileError
/// (zend_add_member_modifier() and friends): zend_exception_error() reports
/// it as a fatal error without a backtrace.
fn compile_error_exception_message(msg: &str) -> bool {
    (msg.starts_with("Cannot use the ") && msg.contains(" modifier on a"))
        || (msg.starts_with("Multiple ") && msg.ends_with(" modifiers are not allowed"))
        || msg == "Encoding must be a literal"
        || msg == "__HALT_COMPILER() can only be used from the outermost scope"
}

fn ini_str(vm: &Vm, key: &[u8]) -> Option<String> {
    vm.constants
        .get(key)
        .map(|v| v.to_php_string().to_string_lossy())
}

fn ini_bool(vm: &Vm, key: &[u8]) -> bool {
    match ini_str(vm, key) {
        Some(s) => {
            let l = s.trim().to_ascii_lowercase();
            matches!(l.as_str(), "1" | "on" | "yes" | "true")
                || l.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
        None => false,
    }
}

/// htmlspecialchars(ENT_COMPAT) for error messages.
/// Error messages travel as `String`s: bytes that are not UTF-8 (a
/// message built from binary data, `throw new Exception("\xF2")`) are kept
/// as the plane-16 private-use characters U+10FF00 + byte and written back
/// as the raw bytes (php_error_cb() outputs messages as they are).
pub fn raw_error_string(b: &[u8]) -> String {
    crate::string::msg_str(b).into_owned()
}

/// The bytes of a message made by `raw_error_string`.
pub fn raw_error_bytes(s: &str) -> std::borrow::Cow<'_, [u8]> {
    // (U+10FF00..U+10FFFF are encoded F4 8F BC..BF xx)
    if !s.as_bytes().contains(&0xF4) {
        return std::borrow::Cow::Borrowed(s.as_bytes());
    }
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if (0x10FF00..=0x10FFFF).contains(&u) {
            out.push((u - 0x10FF00) as u8);
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    std::borrow::Cow::Owned(out)
}

/// escape_html() of php_error_cb() for the html_errors charset: invalid
/// UTF-8 is substituted (ENT_SUBSTITUTE) unless a single-byte charset is
/// in use.
fn escape_html_cs(s: &str, utf8: bool) -> String {
    let e = escape_html(s);
    if !utf8 {
        return e;
    }
    e.chars()
        .map(|c| {
            if (0x10FF00..=0x10FFFF).contains(&(c as u32)) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// Split a php_error_docref style message "fname(args): text" into
/// (function name, origin, text).
fn split_docref(msg: &str) -> Option<(&str, &str, &str)> {
    let b = msg.as_bytes();
    let mut i = 0;
    while i < b.len()
        && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b':' || b[i] == b'\\')
    {
        i += 1;
    }
    if i == 0 || i >= b.len() || b[i] != b'(' {
        return None;
    }
    let name = &msg[..i];
    let rest = &msg[i..];
    let close = rest.find("): ")?;
    let origin = &msg[..i + close + 1];
    let text = &rest[close + 3..];
    Some((name, origin, text))
}

impl Vm {
    /// 0 = don't display, 1 = stdout (through the output layer), 2 = stderr.
    fn display_errors_mode(&self) -> u8 {
        match ini_str(self, b"display_errors") {
            None => 1,
            Some(s) => {
                let l = s.trim().to_ascii_lowercase();
                match l.as_str() {
                    "on" | "yes" | "true" | "stdout" => 1,
                    "stderr" => 2,
                    _ => match l.parse::<i64>() {
                        Ok(0) | Err(_) => 0,
                        Ok(2) => 2,
                        Ok(_) => 1,
                    },
                }
            }
        }
    }

    /// zend_error(): hand a non-fatal error raised at `line` of the current
    /// file to the user error handler, or to php_error_cb().
    /// A deprecation raised while linking a class, about the member on
    /// `line`: a user error handler is called from the executing opline.
    #[inline(never)]
    pub fn emit_link_deprecated_at(&mut self, msg: &str, line: u32) {
        self.error_frame_line = self.current_line;
        self.raise_error(E_DEPRECATED, msg, line);
        self.error_frame_line = 0;
    }

    pub fn raise_error(&mut self, ty: i64, msg: &str, line: u32) {
        if self.error_reporting == ER_SILENT {
            return;
        }
        if let Some(rec) = self.recorded_errors.as_mut() {
            rec.push((ty, msg.to_string(), line));
            return;
        }
        if self.call_user_error_handler(ty, msg, line) {
            return;
        }
        let file = self.current_file.clone();
        self.error_cb(ty, msg, &file, line);
    }

    /// The user-space half of zend_error_zstr_at(): call the handler
    /// installed by set_error_handler() if its `$error_levels` mask covers
    /// `ty` (error_reporting() does not matter). Returns true when the
    /// error was handled, false when php_error_cb() must still process it
    /// (no handler, or the handler returned false).
    pub fn call_user_error_handler(&mut self, ty: i64, msg: &str, line: u32) -> bool {
        if self.error_reporting == ER_SILENT {
            return true;
        }
        let handler = match &self.error_handler {
            Some(h) => h.clone(),
            None => return false,
        };
        // zend_call_function() does not run while an exception is pending;
        // the error then counts as handled.
        if self.current_exception.is_some() {
            return true;
        }
        if self.error_handler_mask & ty == 0
            || matches!(
                ty,
                E_ERROR
                    | E_PARSE
                    | E_CORE_ERROR
                    | E_CORE_WARNING
                    | E_COMPILE_ERROR
                    | E_COMPILE_WARNING
            )
            || self.warning_capture.is_some()
        {
            return false;
        }
        // The handler is detached while it runs, so errors raised inside it
        // get the standard handling; it is restored afterwards unless the
        // handler installed another one.
        let saved = self.error_handler.take();
        let file = self.current_file.clone();
        // A #[\Deprecated] handler reports its own deprecation first.
        self.emit_deprecation_for_callback_at(&handler, &file, line);
        let args = vec![
            Value::Long(ty),
            Value::String(PhpString::from_vec(raw_error_bytes(msg).into_owned())),
            Value::String(PhpString::from_string(file.to_string())),
            Value::Long(line as i64),
        ];
        // Called from user code, the handler's frame is located at the
        // error; called from a builtin, it is an "[internal function]" one.
        let saved_ucf = self.ucf_call_line.take();
        let frame_line = std::mem::take(&mut self.error_frame_line);
        if self.in_builtin_frame() {
            self.materialize_builtin_frames(false);
        } else {
            self.ucf_call_line = Some(if frame_line != 0 { frame_line } else { line });
            self.ucf_strict = false;
        }
        let fatal_before = self.fatal_emitted;
        let r = self.call_callback(&handler, &args);
        self.ucf_call_line = saved_ucf;
        if self.error_handler.is_none() {
            self.error_handler = saved;
        }
        // A fatal error inside the handler ends the script (bailout): the
        // code that raised the error does not continue.
        if !fatal_before && self.fatal_emitted && self.current_exception.is_none() {
            self.fatal_bailout = true;
        }
        match r {
            Ok(v) => {
                if self.current_exception.is_some() {
                    self.error_handler_threw = true;
                    return true;
                }
                !matches!(v.deref(), Value::False)
            }
            Err(_) => {
                self.error_handler_threw = true;
                true
            }
        }
    }

    /// php_error_cb() for a non-fatal error: remember it for
    /// error_get_last(), then log and display it as error_reporting allows.
    pub fn error_cb(&mut self, ty: i64, msg: &str, file: &str, line: u32) {
        if self.error_reporting == ER_SILENT {
            return;
        }
        let display = if ini_bool(self, b"ignore_repeated_errors") {
            match &self.last_error {
                Some((_, m, f, l)) => {
                    m != msg
                        || (!ini_bool(self, b"ignore_repeated_source") && (*l != line || f != file))
                }
                None => true,
            }
        } else {
            true
        };
        if !display {
            return;
        }
        self.last_error = Some((ty, msg.to_string(), file.to_string(), line));
        self.last_error_trace = None;
        if self.error_reporting & ty == 0 && ty & E_CORE == 0 {
            return;
        }
        let kind = error_type_label(ty);
        if ini_bool(self, b"log_errors") {
            self.log_err(&format!(
                "PHP {}:  {} in {} on line {}",
                kind, msg, file, line
            ));
        }
        // (see set_error_display_suppressed)
        if ERROR_DISPLAY_SUPPRESSED.with(|s| s.get()) {
            return;
        }
        self.display_error(kind, msg, file, line);
    }

    /// php_error_cb() for a fatal error (the caller bails out afterwards).
    pub fn fatal_error_cb(&mut self, ty: i64, msg: &str, file: &str, line: u32) {
        self.fatal_error_cb_ex(ty, msg, file, line, true);
        // php_error_cb(): a fatal error (not an uncaught exception's
        // E_DONT_BAIL report) bails out with every object marked destructed,
        // the locals of the frames it abandons included.
        if ty & E_FATAL_ERRORS != 0 && !msg.starts_with("Uncaught ") {
            self.mark_objects_destructed();
        }
    }

    /// fatal_error_cb(); `backtrace` false is zend_error_cb() called
    /// directly (no EG(last_fatal_error_backtrace)), as zend_exception_error()
    /// does for an uncaught ParseError / CompileError.
    pub fn fatal_error_cb_ex(
        &mut self,
        ty: i64,
        msg: &str,
        file: &str,
        line: u32,
        backtrace: bool,
    ) {
        self.last_error = Some((ty, msg.to_string(), file.to_string(), line));
        // zend_error_zstr_at(): EG(last_fatal_error_backtrace)
        self.last_error_trace = if backtrace
            && ini_bool(self, b"fatal_error_backtraces")
            && !msg.starts_with("Uncaught ")
        {
            // A declaration bound at compile time fails before the eval()
            // frame of the code being compiled exists.
            let hidden = if self.compile_time_binding
                && self.call_stack.last().is_some_and(|f| f.0 == "eval")
            {
                self.call_stack.pop()
            } else {
                None
            };
            let t = self.build_exception_trace();
            if let Some(f) = hidden {
                self.call_stack.push(f);
            }
            Some(t)
        } else if backtrace {
            None
        } else {
            // Some(non-array): known to have no backtrace (display_error
            // must not take one either).
            Some(Value::Null)
        };
        // Engine-internal silencing never hides fatal errors.
        let er = if self.error_reporting == ER_SILENT {
            E_FATAL_ERRORS
        } else {
            self.error_reporting
        };
        if er & ty == 0 {
            return;
        }
        let kind = error_type_label(ty);
        if ini_bool(self, b"log_errors") {
            self.log_err(&format!(
                "PHP {}:  {} in {} on line {}",
                kind, msg, file, line
            ));
        }
        self.display_error(kind, msg, file, line);
    }

    /// run_user_tick_functions(): call each registered tick function (one
    /// already running is skipped).
    pub fn run_tick_functions(&mut self) {
        // ZEND_TICKS: zend_fiber_switch_block() around zend_ticks_function.
        self.fiber_switch_blocking += 1;
        self.run_tick_functions_blocked();
        self.fiber_switch_blocking -= 1;
    }

    fn run_tick_functions_blocked(&mut self) {
        if let Some(hook) = self.tick_hook {
            hook(self);
        }
        let mut i = 0;
        while i < self.tick_functions.len() {
            if self.current_exception.is_some() {
                break;
            }
            let (cb, args, calling) = self.tick_functions[i].clone();
            if !calling {
                self.tick_functions[i].2 = true;
                // (called from the ticked statement's code)
                self.ucf_call_line = Some(self.current_line);
                self.ucf_strict = false;
                let _ = self.call_callback(&cb, &args);
                self.ucf_call_line = None;
                // The list may have changed while the function ran.
                if let Some(e) = self.tick_functions.get_mut(i) {
                    if callables_equal(&e.0, &cb) {
                        e.2 = false;
                    }
                }
            }
            i += 1;
        }
    }

    /// php_log_err(): append to the error_log file (with a timestamp), or
    /// hand the message to the SAPI logger (stderr for the CLI).
    pub fn log_err(&mut self, msg: &str) {
        if self.in_error_log {
            return;
        }
        self.in_error_log = true;
        let dest = ini_str(self, b"error_log").unwrap_or_default();
        if !dest.is_empty() && dest != "syslog" {
            use std::io::Write;
            let mode = ini_str(self, b"error_log_mode")
                .and_then(|m| {
                    let m = m.trim();
                    let m = m.strip_prefix("0o").unwrap_or(m);
                    u32::from_str_radix(m, 8).ok()
                })
                .filter(|m| *m > 0 && *m <= 0o777)
                .unwrap_or(0o644);
            let mut opts = std::fs::OpenOptions::new();
            opts.append(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(mode);
            }
            #[cfg(not(unix))]
            let _ = mode;
            if let Ok(mut f) = opts.open(&dest) {
                let stamp = self.log_timestamp();
                let _ = f.write_all(&raw_error_bytes(&format!("[{}] {}\n", stamp, msg)));
                self.in_error_log = false;
                return;
            }
        }
        if let Some(log) = self.sapi_log_message {
            log(msg);
        } else {
            use std::io::Write;
            self.flush_cli_output();
            let _ = std::io::stderr().write_all(&raw_error_bytes(&format!("{}\n", msg)));
        }
        self.in_error_log = false;
    }

    /// php_format_date("d-M-Y H:i:s e", now, local) through ext/date's date().
    fn log_timestamp(&mut self) -> String {
        let fmt = Value::String(PhpString::from_bytes(b"d-M-Y H:i:s e"));
        if let Some(f) = self.functions.get(b"date".as_slice()).copied() {
            let saved = self.error_reporting;
            self.error_reporting = ER_SILENT;
            let r = f(self, &[fmt]);
            self.error_reporting = saved;
            if let Ok(Value::String(s)) = r {
                return s.to_string_lossy();
            }
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let days = secs.div_euclid(86400);
        let rem = secs.rem_euclid(86400);
        // civil_from_days
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + (m <= 2) as i64;
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{:02}-{}-{:04} {:02}:{:02}:{:02} UTC",
            d,
            MONTHS[(m - 1) as usize],
            y,
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    }

    /// Format and output one error message the way php_error_cb displays it.
    pub fn display_error(&mut self, kind: &str, msg: &str, file: &str, line: u32) {
        let mode = self.display_errors_mode();
        if mode == 0 {
            return;
        }
        // fatal_error_backtraces: php_error_cb() appends the backtrace taken
        // when the fatal error was raised (not for uncaught exceptions,
        // which print their own trace).
        let trace = if kind == "Fatal error"
            && !msg.starts_with("Uncaught ")
            && !compile_error_exception_message(msg)
            && ini_bool(self, b"fatal_error_backtraces")
        {
            // The backtrace taken when the error was raised (fatal_error_cb).
            match self.last_error_trace.clone() {
                Some(Value::Null) => String::new(),
                Some(t) => format!(
                    "\nStack trace:\n{}",
                    Vm::format_exception_trace_value(
                        &t,
                        "",
                        self.exception_string_param_max_len,
                        &mut Vec::new(),
                    )
                ),
                None => {
                    self.materialize_builtin_frames(false);
                    self.sync_frame_args();
                    format!("\nStack trace:\n{}", self.format_stack_trace())
                }
            }
        } else {
            String::new()
        };
        if ini_bool(self, b"xmlrpc_errors") {
            let num = ini_str(self, b"xmlrpc_error_number")
                .and_then(|n| n.trim().parse::<i64>().ok())
                .unwrap_or(0);
            let text = format!(
                "<?xml version=\"1.0\"?><methodResponse><fault><value><struct><member><name>faultCode</name><value><int>{}</int></value></member><member><name>faultString</name><value><string>{}:{} in {} on line {}{}</string></value></member></struct></value></fault></methodResponse>",
                num, kind, msg, file, line, trace
            );
            self.write_output(text.as_bytes());
            return;
        }
        let prepend = ini_str(self, b"error_prepend_string").unwrap_or_default();
        let append = ini_str(self, b"error_append_string").unwrap_or_default();
        let text = if ini_bool(self, b"html_errors") {
            let utf8 = ini_str(self, b"default_charset").is_none_or(|c| {
                c.is_empty() || c.eq_ignore_ascii_case("utf-8") || c.eq_ignore_ascii_case("utf8")
            });
            let body = if kind == "Fatal error" || kind == "Parse error" {
                escape_html_cs(msg, utf8)
            } else if let Some((name, origin, rest)) = split_docref(msg) {
                let root = ini_str(self, b"docref_root").unwrap_or_default();
                if !root.is_empty() {
                    let mut docref =
                        format!("function.{}", name.replace('_', "-").to_ascii_lowercase());
                    let ext = ini_str(self, b"docref_ext").unwrap_or_default();
                    if !ext.is_empty() {
                        docref.push_str(&ext);
                    }
                    format!(
                        "{} [<a href='{}{}'>{}</a>]: {}",
                        escape_html_cs(origin, utf8),
                        root,
                        docref,
                        docref,
                        escape_html_cs(rest, utf8)
                    )
                } else {
                    format!(
                        "{}: {}",
                        escape_html_cs(origin, utf8),
                        escape_html_cs(rest, utf8)
                    )
                }
            } else {
                msg.to_string()
            };
            format!(
                "{}<br />\n<b>{}</b>:  {} in <b>{}</b> on line <b>{}</b><br />{}\n{}",
                prepend, kind, body, file, line, trace, append
            )
        } else {
            format!(
                "{}\n{}: {} in {} on line {}{}\n{}",
                prepend, kind, msg, file, line, trace, append
            )
        };
        let raw = raw_error_bytes(&text);
        if mode == 2 {
            use std::io::Write;
            let start = raw.iter().position(|&b| b != b'\n').unwrap_or(raw.len());
            // (the CLI writes its output unbuffered: what was output so far
            // comes before the error)
            self.flush_cli_output();
            let _ = std::io::stderr().write_all(&raw[start..]);
        } else {
            self.write_output(&raw);
        }
    }
}
