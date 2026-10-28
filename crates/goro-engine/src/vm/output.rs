//! Output: a buffered stdout under PHP's output buffering stack (PHP
//! manual: Output Control).

use std::io::Write;

use goro_runtime::{PhpStr, Value};

use super::{R, Unwind, Vm};

/// One `ob_start()` level.
pub struct ObBuffer {
    pub data: Vec<u8>,
    pub callback: Option<Value>,
    pub chunk_size: usize,
    /// The PHP_OUTPUT_HANDLER_CLEANABLE/FLUSHABLE/REMOVABLE bits given.
    pub flags: i64,
    pub name: PhpStr,
    /// The handler has been called at least once (PHP_OUTPUT_HANDLER_START
    /// sent).
    pub started: bool,
    /// Inside the handler: output from it stays in `data` and is dropped
    /// when the handler returns.
    pub in_handler: bool,
    /// The handler returned false or failed: it is not called again and
    /// the contents pass through as they are.
    pub disabled: bool,
    /// The handler has processed output.
    pub processed: bool,
    /// Output was produced while the handler ran.
    pub produced: bool,
    /// Allocated size as `ob_get_status()` reports it.
    pub size: usize,
}

impl ObBuffer {
    pub fn new(callback: Option<Value>, chunk_size: usize, flags: i64, name: PhpStr) -> Self {
        // Sizes as observed on PHP 8.5 (test tests/output/gh16135.phpt):
        // the chunk size rounded up to 4 KiB, 16 KiB without one.
        let size = if chunk_size > 0 {
            chunk_size.div_ceil(4096) * 4096
        } else {
            16384
        };
        ObBuffer {
            data: Vec::new(),
            callback,
            chunk_size,
            flags: flags & PHP_OUTPUT_HANDLER_STDFLAGS,
            name,
            started: false,
            in_handler: false,
            disabled: false,
            processed: false,
            produced: false,
            size,
        }
    }

    /// The `flags` entry of `ob_get_status()`.
    pub fn status_flags(&self) -> i64 {
        let mut f = self.flags;
        if self.callback.is_some() {
            f |= PHP_OUTPUT_HANDLER_USER;
        }
        if self.started {
            f |= PHP_OUTPUT_HANDLER_STARTED;
        }
        if self.disabled {
            f |= PHP_OUTPUT_HANDLER_DISABLED;
        }
        if self.processed {
            f |= PHP_OUTPUT_HANDLER_PROCESSED;
        }
        f
    }

    #[inline]
    fn append(&mut self, b: &[u8]) {
        // Growth as observed on PHP 8.5: when the free space is not larger
        // than the write, the size grows by at least 16 KiB, rounded up to
        // 4 KiB.
        let free = self.size.saturating_sub(self.data.len());
        if free <= b.len() {
            let grow = (b.len() - free).max(16384);
            self.size = (self.size + grow).div_ceil(4096) * 4096;
        }
        self.data.extend_from_slice(b);
    }
}

pub const PHP_OUTPUT_HANDLER_START: i64 = 1;
pub const PHP_OUTPUT_HANDLER_WRITE: i64 = 0;
pub const PHP_OUTPUT_HANDLER_FLUSH: i64 = 4;
pub const PHP_OUTPUT_HANDLER_CLEAN: i64 = 2;
pub const PHP_OUTPUT_HANDLER_FINAL: i64 = 8;
pub const PHP_OUTPUT_HANDLER_CLEANABLE: i64 = 16;
pub const PHP_OUTPUT_HANDLER_FLUSHABLE: i64 = 32;
pub const PHP_OUTPUT_HANDLER_REMOVABLE: i64 = 64;
pub const PHP_OUTPUT_HANDLER_STDFLAGS: i64 = 112;
pub const PHP_OUTPUT_HANDLER_USER: i64 = 1;
pub const PHP_OUTPUT_HANDLER_STARTED: i64 = 0x1000;
pub const PHP_OUTPUT_HANDLER_DISABLED: i64 = 0x2000;
pub const PHP_OUTPUT_HANDLER_PROCESSED: i64 = 0x4000;

pub struct Output {
    pub stdout: Vec<u8>,
    pub buffers: Vec<ObBuffer>,
    pub implicit_flush: bool,
    /// Total bytes written to stdout (headers_sent-like checks).
    pub written: u64,
    /// Output goes to this many levels (below the level whose handler
    /// runs) instead of the top one.
    pub redirect: Option<usize>,
    /// The end-of-request flush is running.
    pub final_flush: bool,
    /// Output buffering was shut down by a forbidden call in a handler:
    /// no more buffers can be started.
    pub deactivated: bool,
}

impl Default for Output {
    fn default() -> Self {
        Output {
            stdout: Vec::with_capacity(1 << 16),
            buffers: Vec::new(),
            implicit_flush: false,
            written: 0,
            redirect: None,
            final_flush: false,
            deactivated: false,
        }
    }
}

impl Output {
    /// Writes pending stdout bytes.
    pub fn flush(&mut self) {
        if !self.stdout.is_empty() {
            let mut so = std::io::stdout().lock();
            let _ = so.write_all(&self.stdout);
            let _ = so.flush();
            self.stdout.clear();
        }
    }

    #[inline]
    pub fn write_stdout(&mut self, b: &[u8]) {
        self.written += b.len() as u64;
        self.stdout.extend_from_slice(b);
        if self.stdout.len() >= 1 << 16 || self.implicit_flush {
            self.flush();
        }
    }

    /// Appends to level `n` (the buffer at index n-1, stdout for 0)
    /// without running handlers.
    pub fn write_level(&mut self, n: usize, b: &[u8]) {
        if n == 0 {
            self.write_stdout(b);
        } else {
            self.buffers[n - 1].append(b);
        }
    }

    /// Index of the buffer whose handler runs, if any.
    pub fn handler_level(&self) -> Option<usize> {
        self.buffers.iter().rposition(|b| b.in_handler)
    }
}

impl Vm {
    /// `echo`: through the output buffers.
    #[inline]
    pub fn echo(&mut self, b: &[u8]) {
        if self.out.buffers.is_empty() {
            self.out.write_stdout(b);
            return;
        }
        if let Some(n) = self.out.redirect {
            self.out.write_level(n, b);
            return;
        }
        let top = self.out.buffers.last_mut().unwrap();
        if top.in_handler {
            if !b.is_empty() {
                top.produced = true;
            }
            top.data.extend_from_slice(b);
            return;
        }
        top.append(b);
        if top.chunk_size > 0 && top.data.len() >= top.chunk_size {
            // Errors of the handler (exceptions) surface at the statement
            // that produced the output.
            if let Err(e) = self.ob_flush_top(PHP_OUTPUT_HANDLER_WRITE)
                && self.err.deferred.is_none()
            {
                self.err.deferred = Some(e);
            }
        }
    }

    pub fn echo_value(&mut self, v: &Value) -> R<()> {
        match v {
            Value::Str(s) => {
                self.echo(s);
                Ok(())
            }
            Value::Int(i) => {
                let mut buf = [0u8; 20];
                let d = goro_runtime::convert::int_digits(*i, &mut buf);
                let d = d.to_vec();
                self.echo(&d);
                Ok(())
            }
            _ => {
                let mut out = Vec::new();
                self.append_to_string(&mut out, v)?;
                self.echo(&out);
                Ok(())
            }
        }
    }

    pub fn echo_str(&mut self, s: &str) {
        self.echo(s.as_bytes());
    }

    /// The function PHP names in diagnostics raised outside of any
    /// function's own checks: the running internal function, else the
    /// running user function (`main` at the top level).
    pub fn active_function_name(&self) -> String {
        let depth = self.ex.frames.len() as u32;
        if let Some(nc) = self.ex.native_calls.last()
            && nc.depth == depth
        {
            return nc.func.display_name();
        }
        match self.ex.frames.last() {
            Some(f) => {
                let code = f.code();
                if code.flags & crate::bytecode::fflags::PSEUDO_MAIN != 0 || code.name.is_empty() {
                    "main".to_string()
                } else {
                    code.name.to_string_lossy()
                }
            }
            None => "main".to_string(),
        }
    }

    /// Runs the handler of the top buffer on its contents (flags as PHP
    /// passes them) and returns the processed output, with an error to
    /// raise once that output has been passed on (the deprecation of
    /// output produced by the handler turned into an exception).
    fn ob_process(&mut self, mode: i64) -> R<(Vec<u8>, Option<Unwind>)> {
        let idx = self.out.buffers.len() - 1;
        let b = &mut self.out.buffers[idx];
        let Some(cb) = b.callback.clone().filter(|_| !b.disabled) else {
            return Ok((std::mem::take(&mut b.data), None));
        };
        let input = b.data.clone();
        let mut flags = mode;
        if !b.started {
            flags |= PHP_OUTPUT_HANDLER_START;
        }
        b.in_handler = true;
        b.produced = false;
        let caller = if self.out.final_flush {
            None
        } else {
            Some(self.active_function_name())
        };
        let r = self.call_value(
            &cb,
            vec![Value::Str(PhpStr::from(input.clone())), Value::Int(flags)],
        );
        let Some(b) = self.out.buffers.get_mut(idx) else {
            return r.map(|_| (Vec::new(), None));
        };
        b.in_handler = false;
        b.started = true;
        let v = match r {
            Ok(v) => v,
            Err(e) => {
                // A failing handler is disabled. After an exception the
                // contents (with what the handler printed) stay to pass
                // through; a fatal error discards them (observed on PHP
                // 8.5).
                b.disabled = true;
                if !matches!(e, Unwind::Throw(_)) {
                    b.data.clear();
                }
                return Err(e);
            }
        };
        let produced = b.produced;
        b.produced = false;
        b.data.clear();
        let out = match v {
            Value::Bool(false) => {
                b.disabled = true;
                input
            }
            Value::Str(s) => {
                b.processed = true;
                s.to_vec()
            }
            Value::Bool(true) | Value::Null => {
                b.processed = true;
                Vec::new()
            }
            Value::Array(_) => {
                b.processed = true;
                b"Array".to_vec()
            }
            other => {
                b.processed = true;
                let mut out = Vec::new();
                goro_runtime::convert::append_str(
                    &mut out,
                    &other,
                    &mut goro_runtime::Diagnostics::new(),
                )
                .ok();
                out
            }
        };
        let mut pending = None;
        if produced {
            // PHP 8.4+ deprecates output from user handlers (RFC:
            // Deprecations for PHP 8.4); the message goes to the level
            // below.
            let name = self.out.buffers[idx].name.to_string_lossy();
            let saved = self.out.redirect.replace(idx);
            let r = match &caller {
                Some(f) => self.deprecated(&format!(
                    "{}(): Producing output from user output handler {} is deprecated",
                    f, name
                )),
                None => self.raise_at(
                    super::errors::E_DEPRECATED,
                    &format!(
                        "PHP Request Shutdown: Producing output from user output handler {} is deprecated",
                        name
                    ),
                    PhpStr::from("Unknown"),
                    0,
                ),
            };
            self.out.redirect = saved;
            pending = r.err();
        }
        Ok((out, pending))
    }

    /// Passes the top buffer's contents to the level below.
    pub fn ob_flush_top(&mut self, mode: i64) -> R<()> {
        let n = self.out.buffers.len();
        let (out, pending) = self.ob_process(mode)?;
        if self.out.buffers.len() != n {
            // Output buffering was shut down by the handler.
            return Ok(());
        }
        self.out.write_level(n - 1, &out);
        pending.map_or(Ok(()), Err)
    }

    /// `ob_end_flush()` of the top buffer.
    pub fn ob_end_flush(&mut self) -> R<()> {
        let n = self.out.buffers.len();
        let r = self.ob_process(PHP_OUTPUT_HANDLER_FINAL);
        if self.out.buffers.len() != n {
            return r.map(|_| ());
        }
        let b = self.out.buffers.pop().unwrap();
        match r {
            Ok((out, pending)) => {
                self.out.write_level(n - 1, &out);
                pending.map_or(Ok(()), Err)
            }
            Err(e) => {
                // The level goes; what it held passes through (except at
                // the end of the request, see ob_end_all).
                if !self.out.final_flush {
                    self.out.write_level(n - 1, &b.data);
                }
                Err(e)
            }
        }
    }

    /// `ob_end_clean()` / `ob_clean()`: runs the handler with CLEAN and
    /// discards.
    pub fn ob_clean_top(&mut self, end: bool) -> R<()> {
        let mode = if end {
            PHP_OUTPUT_HANDLER_CLEAN | PHP_OUTPUT_HANDLER_FINAL
        } else {
            PHP_OUTPUT_HANDLER_CLEAN
        };
        let n = self.out.buffers.len();
        let r = if self
            .out
            .buffers
            .last()
            .is_some_and(|b| b.callback.is_some() && !b.disabled)
        {
            self.ob_process(mode).map(|(_, p)| p)
        } else {
            Ok(None)
        };
        if let Some(b) = self.out.buffers.last_mut() {
            b.data.clear();
        }
        if end && self.out.buffers.len() == n {
            self.out.buffers.pop();
        }
        match r {
            Ok(p) => p.map_or(Ok(()), Err),
            Err(e) => Err(e),
        }
    }

    /// End of the script: flush every buffer.
    pub fn ob_end_all(&mut self) {
        self.out.final_flush = true;
        while !self.out.buffers.is_empty() {
            match self.ob_end_flush() {
                Ok(()) => {}
                Err(Unwind::Throw(_)) => {
                    // An exception from a handler at shutdown ends the
                    // request without output (observed on PHP 8.5).
                    self.exit_status = 255;
                    self.out.buffers.clear();
                    break;
                }
                Err(_) => {}
            }
        }
        self.out.final_flush = false;
        self.out.flush();
    }
}
