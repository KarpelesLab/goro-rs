//! Output: a buffered stdout under PHP's output buffering stack (PHP
//! manual: Output Control).

use std::io::Write;

use goro_runtime::{PhpStr, Value};

use super::{R, Vm};

/// One `ob_start()` level.
pub struct ObBuffer {
    pub data: Vec<u8>,
    pub callback: Option<Value>,
    pub chunk_size: usize,
    pub flags: i64,
    pub name: PhpStr,
    /// The handler has been called at least once (PHP_OUTPUT_HANDLER_START
    /// sent).
    pub started: bool,
    /// Inside the handler (output from it is discarded).
    pub in_handler: bool,
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

pub struct Output {
    pub stdout: Vec<u8>,
    pub buffers: Vec<ObBuffer>,
    pub implicit_flush: bool,
    /// Total bytes written to stdout (headers_sent-like checks).
    pub written: u64,
}

impl Default for Output {
    fn default() -> Self {
        Output {
            stdout: Vec::with_capacity(1 << 16),
            buffers: Vec::new(),
            implicit_flush: false,
            written: 0,
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
}

impl Vm {
    /// `echo`: through the output buffers.
    #[inline]
    pub fn echo(&mut self, b: &[u8]) {
        if self.out.buffers.is_empty() {
            self.out.write_stdout(b);
            return;
        }
        let top = self.out.buffers.last_mut().unwrap();
        if top.in_handler {
            return;
        }
        top.data.extend_from_slice(b);
        if top.chunk_size > 0 && top.data.len() >= top.chunk_size {
            let _ = self.ob_flush_top(PHP_OUTPUT_HANDLER_WRITE);
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

    /// Runs the handler of the top buffer on its contents (flags as PHP
    /// passes them) and returns the processed output.
    fn ob_process(&mut self, mode: i64) -> R<Vec<u8>> {
        let idx = self.out.buffers.len() - 1;
        let data = std::mem::take(&mut self.out.buffers[idx].data);
        let Some(cb) = self.out.buffers[idx].callback.clone() else {
            return Ok(data);
        };
        let mut flags = mode;
        if !self.out.buffers[idx].started {
            flags |= PHP_OUTPUT_HANDLER_START;
            self.out.buffers[idx].started = true;
        }
        self.out.buffers[idx].in_handler = true;
        let r = self.call_value(
            &cb,
            vec![Value::Str(PhpStr::from(data.clone())), Value::Int(flags)],
        );
        if let Some(b) = self.out.buffers.get_mut(idx) {
            b.in_handler = false;
        }
        let r = r?;
        Ok(match r {
            Value::Bool(false) => data,
            Value::Str(s) => s.to_vec(),
            Value::Bool(true) => Vec::new(),
            other => {
                let mut out = Vec::new();
                self.append_to_string(&mut out, &other)?;
                out
            }
        })
    }

    /// Passes the top buffer's contents to the level below.
    pub fn ob_flush_top(&mut self, mode: i64) -> R<()> {
        let out = self.ob_process(mode)?;
        let n = self.out.buffers.len();
        if n >= 2 {
            self.out.buffers[n - 2].data.extend_from_slice(&out);
        } else {
            self.out.write_stdout(&out);
        }
        Ok(())
    }

    /// `ob_end_flush()` of the top buffer.
    pub fn ob_end_flush(&mut self) -> R<()> {
        let out = self.ob_process(PHP_OUTPUT_HANDLER_FINAL)?;
        self.out.buffers.pop();
        self.echo(&out);
        Ok(())
    }

    /// `ob_end_clean()` / `ob_clean()`: runs the handler with CLEAN and
    /// discards.
    pub fn ob_clean_top(&mut self, end: bool) -> R<()> {
        let mode = if end {
            PHP_OUTPUT_HANDLER_CLEAN | PHP_OUTPUT_HANDLER_FINAL
        } else {
            PHP_OUTPUT_HANDLER_CLEAN
        };
        if self
            .out
            .buffers
            .last()
            .is_some_and(|b| b.callback.is_some())
        {
            let _ = self.ob_process(mode)?;
        } else if let Some(b) = self.out.buffers.last_mut() {
            b.data.clear();
        }
        if end {
            self.out.buffers.pop();
        }
        Ok(())
    }

    /// End of the script: flush every buffer.
    pub fn ob_end_all(&mut self) {
        while !self.out.buffers.is_empty() {
            if self.ob_end_flush().is_err() {
                // Discard the failing level.
                if let Some(b) = self.out.buffers.pop() {
                    let d = b.data;
                    self.echo(&d);
                }
            }
        }
        self.out.flush();
    }
}
