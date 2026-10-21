//! Output buffering layer, modelled on php-src main/output.c.
//!
//! The buffers themselves live in `Vm::ob_stack` (one `Vec<u8>` per level,
//! kept for compatibility with code that peeks at the active buffer); the
//! per-level handler metadata lives in `Vm::ob_handlers`, index-aligned with
//! `ob_stack`.

use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{Vm, VmError};

// Handler operation flags (PHP_OUTPUT_HANDLER_*)
pub const OB_OP_WRITE: i64 = 0x00;
pub const OB_OP_START: i64 = 0x01;
pub const OB_OP_CLEAN: i64 = 0x02;
pub const OB_OP_FLUSH: i64 = 0x04;
pub const OB_OP_FINAL: i64 = 0x08;

// Handler type / ability / status flags
pub const OBH_INTERNAL: i64 = 0x0000;
pub const OBH_USER: i64 = 0x0001;
pub const OBH_CLEANABLE: i64 = 0x0010;
pub const OBH_FLUSHABLE: i64 = 0x0020;
pub const OBH_REMOVABLE: i64 = 0x0040;
pub const OBH_STDFLAGS: i64 = 0x0070;
pub const OBH_STARTED: i64 = 0x1000;
pub const OBH_DISABLED: i64 = 0x2000;
pub const OBH_PROCESSED: i64 = 0x4000;
pub const OBH_PRODUCED_OUTPUT: i64 = 0x8000;

// Stack pop flags (PHP_OUTPUT_POP_*)
pub const OB_POP_TRY: i64 = 0x000;
pub const OB_POP_FORCE: i64 = 0x001;
pub const OB_POP_DISCARD: i64 = 0x010;
pub const OB_POP_SILENT: i64 = 0x100;

const OB_ALIGNTO_SIZE: usize = 0x1000;
const OB_DEFAULT_SIZE: usize = 0x4000;

fn initbuf_size(s: usize) -> usize {
    if s > 0 {
        (s + OB_ALIGNTO_SIZE - 1) & !(OB_ALIGNTO_SIZE - 1)
    } else {
        OB_DEFAULT_SIZE
    }
}

/// Internal handler function: receives the buffered input and the op flags,
/// returns `Some(output)` to replace the data or `None` to pass it through.
pub type ObInternalFn = fn(&mut Vm, &[u8], i64) -> Option<Vec<u8>>;

/// Internal handler that can fail: `Some(output)` is SUCCESS, `None` is
/// FAILURE (the handler gets disabled and its input passes through).
pub type ObInternalFallibleFn = fn(&mut Vm, &[u8], i64) -> Option<Vec<u8>>;

#[derive(Clone)]
pub enum ObKind {
    Default,
    Devnull,
    User(Value),
    Internal(ObInternalFn),
    InternalFallible(ObInternalFallibleFn),
}

#[derive(Clone)]
pub struct ObHandler {
    pub id: u64,
    pub name: Vec<u8>,
    pub kind: ObKind,
    pub chunk_size: usize,
    pub flags: i64,
    pub buffer_size: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ObStatus {
    Failure,
    NoData,
    Success,
}

struct ObCtx {
    op: i64,
    input: Vec<u8>,
    out: Option<Vec<u8>>,
}

impl ObCtx {
    fn new(op: i64) -> Self {
        ObCtx {
            op,
            input: Vec::new(),
            out: None,
        }
    }
    fn reset(&mut self) {
        self.input.clear();
        self.out = None;
    }
    fn swap(&mut self) {
        self.input = self.out.take().unwrap_or_default();
    }
    fn pass(&mut self) {
        self.out = Some(std::mem::take(&mut self.input));
    }
}

impl Vm {
    /// Keep `ob_handlers` index-aligned with `ob_stack` (code outside this
    /// module may push/pop raw buffers).
    fn ob_sync(&mut self) {
        while self.ob_handlers.len() < self.ob_stack.len() {
            let id = self.ob_alloc_id();
            self.ob_handlers.push(ObHandler {
                id,
                name: b"default output handler".to_vec(),
                kind: ObKind::Default,
                chunk_size: 0,
                flags: OBH_STDFLAGS | OBH_INTERNAL,
                buffer_size: OB_DEFAULT_SIZE,
            });
        }
        self.ob_handlers.truncate(self.ob_stack.len());
    }

    fn ob_alloc_id(&mut self) -> u64 {
        self.ob_next_id += 1;
        self.ob_next_id
    }

    fn ob_index_of(&self, id: u64) -> Option<usize> {
        self.ob_handlers.iter().position(|h| h.id == id)
    }

    /// Current output nesting level (ob_get_level()).
    pub fn ob_level(&mut self) -> usize {
        self.ob_sync();
        self.ob_stack.len()
    }

    /// Name of the function used as the error-message prefix for output
    /// layer diagnostics raised outside of an ob_* builtin.
    fn ob_origin(&self) -> String {
        if let Some(f) = &self.ob_fname {
            return f.clone();
        }
        match self.call_stack.last() {
            Some((name, _, _, _, _)) if !name.is_empty() => name.clone(),
            _ => "Unknown".to_string(),
        }
    }

    /// Buffered write (php_output_write).
    pub fn ob_write(&mut self, data: &[u8]) {
        if self.ob_deactivated || self.ob_stack.is_empty() {
            if !data.is_empty() {
                self.mark_output_started();
            }
            if self.output.len() + data.len() <= Self::MAX_OUTPUT_SIZE {
                self.output.extend_from_slice(data);
            }
            return;
        }
        self.ob_sync();
        // Fast path: the top handler simply stores the data.
        {
            let top = self.ob_stack.len() - 1;
            let h = &mut self.ob_handlers[top];
            if h.flags & OBH_DISABLED == 0 {
                let buf_len = self.ob_stack[top].len();
                let running = self.ob_running.is_some();
                if h.chunk_size == 0 || running || buf_len + data.len() < h.chunk_size {
                    if running {
                        h.flags |= OBH_PRODUCED_OUTPUT;
                    }
                    if data.is_empty() {
                        return;
                    }
                    Self::ob_grow(h, buf_len, data.len());
                    if buf_len + data.len() <= Self::MAX_OUTPUT_SIZE {
                        self.ob_stack[top].extend_from_slice(data);
                    }
                    return;
                }
            }
        }
        self.ob_op(OB_OP_WRITE, data);
    }

    fn ob_grow(h: &mut ObHandler, used: usize, add: usize) {
        if add == 0 {
            return;
        }
        let free = h.buffer_size.saturating_sub(used);
        if free <= add {
            let grow_int = initbuf_size(h.chunk_size);
            let grow_buf = initbuf_size(add - free);
            h.buffer_size += grow_int.max(grow_buf);
        }
    }

    /// php_output_lock_error: using ob functions from inside a running handler
    /// is fatal; the output layer is deactivated first.
    fn ob_lock_error(&mut self, op: i64) -> bool {
        if op != 0 && !self.ob_stack.is_empty() && self.ob_running.is_some() {
            let origin = self.ob_origin();
            self.ob_deactivate();
            let msg = format!(
                "{}(): Cannot use output buffering in output buffering display handlers",
                origin
            );
            let line = self.current_line;
            let err = self.emit_fatal_at(&msg, line);
            if self.ob_pending_fatal.is_none() {
                self.ob_pending_fatal = Some(err);
            }
            return true;
        }
        false
    }

    /// php_output_deactivate: drop every handler without running it.
    pub fn ob_deactivate(&mut self) {
        self.ob_stack.clear();
        self.ob_handlers.clear();
        self.ob_running = None;
        self.ob_deactivated = true;
    }

    /// php_output_handler_append. Returns true if the data was merely stored.
    fn ob_append(&mut self, idx: usize, data: &[u8]) -> bool {
        if !data.is_empty() {
            let used = self.ob_stack[idx].len();
            Self::ob_grow(&mut self.ob_handlers[idx], used, data.len());
            self.ob_stack[idx].extend_from_slice(data);
            let h = &self.ob_handlers[idx];
            if h.chunk_size > 0 && self.ob_stack[idx].len() >= h.chunk_size {
                return self.ob_running.is_some();
            }
        }
        true
    }

    /// php_output_handler_op
    fn ob_handler_op(&mut self, idx: usize, ctx: &mut ObCtx) -> ObStatus {
        if self.ob_handlers[idx].flags & OBH_DISABLED != 0 {
            return ObStatus::Failure;
        }
        if self.ob_lock_error(ctx.op) {
            return ObStatus::Failure;
        }
        let original_op = ctx.op;
        if ctx.op == OB_OP_WRITE && self.ob_running.is_some() {
            self.ob_handlers[idx].flags |= OBH_PRODUCED_OUTPUT;
        }
        let input = std::mem::take(&mut ctx.input);
        let stored = self.ob_append(idx, &input);
        ctx.input = input;
        if stored && ctx.op == 0 {
            ctx.op = original_op;
            return ObStatus::NoData;
        }
        if self.ob_handlers[idx].flags & OBH_STARTED == 0 {
            ctx.op |= OB_OP_START;
        }
        let id = self.ob_handlers[idx].id;
        self.ob_running = Some(id);
        let kind = self.ob_handlers[idx].kind.clone();
        let mut still_have_handler = true;
        let status;
        match kind {
            ObKind::User(cb) => {
                let data = self.ob_stack[idx].clone();
                let args = [
                    Value::String(PhpString::from_vec(data)),
                    Value::Long(ctx.op),
                ];
                let r = self.call_callback(&cb, &args);
                match r {
                    Ok(retval) if self.current_exception.is_none() => {
                        if let Some(i) = self.ob_index_of(id) {
                            if self.ob_handlers[i].flags & OBH_PRODUCED_OUTPUT != 0 {
                                self.ob_handlers[i].flags |= OBH_DISABLED;
                                self.ob_handlers[i].flags &= !OBH_PRODUCED_OUTPUT;
                                let name =
                                    String::from_utf8_lossy(&self.ob_handlers[i].name).to_string();
                                let origin = self.ob_origin();
                                self.emit_deprecated(&format!(
                                    "{}(): Producing output from user output handler {} is deprecated",
                                    origin, name
                                ));
                                still_have_handler = false;
                                if let Some(j) = self.ob_index_of(id) {
                                    self.ob_handlers[j].flags &= !OBH_DISABLED;
                                    still_have_handler = true;
                                }
                            }
                        } else {
                            still_have_handler = false;
                        }
                        let retval = retval.deref();
                        match retval {
                            Value::False => status = ObStatus::Failure,
                            Value::True | Value::Undef => status = ObStatus::NoData,
                            other => {
                                let s = other.to_php_string();
                                if s.as_bytes().is_empty() {
                                    status = ObStatus::NoData;
                                } else {
                                    ctx.out = Some(s.as_bytes().to_vec());
                                    status = ObStatus::Success;
                                }
                            }
                        }
                    }
                    Ok(_) => {
                        status = ObStatus::Failure;
                        if self.ob_index_of(id).is_none() {
                            still_have_handler = false;
                        }
                    }
                    Err(e) => {
                        if self.current_exception.is_none() && self.ob_pending_fatal.is_none() {
                            self.ob_pending_fatal = Some(e);
                        }
                        status = ObStatus::Failure;
                        if self.ob_index_of(id).is_none() {
                            still_have_handler = false;
                        }
                    }
                }
            }
            ObKind::Internal(f) => {
                let data = self.ob_stack[idx].clone();
                match f(self, &data, ctx.op) {
                    Some(out) => ctx.out = Some(out),
                    None => ctx.out = Some(data),
                }
                status = if ctx.out.as_ref().is_some_and(|o| !o.is_empty()) {
                    ObStatus::Success
                } else {
                    ObStatus::NoData
                };
                if self.ob_index_of(id).is_none() {
                    still_have_handler = false;
                }
            }
            ObKind::InternalFallible(f) => {
                let data = self.ob_stack[idx].clone();
                status = match f(self, &data, ctx.op) {
                    Some(out) => {
                        let empty = out.is_empty();
                        ctx.out = Some(out);
                        if empty {
                            ObStatus::NoData
                        } else {
                            ObStatus::Success
                        }
                    }
                    None => ObStatus::Failure,
                };
                if self.ob_index_of(id).is_none() {
                    still_have_handler = false;
                }
            }
            ObKind::Default => {
                let data = self.ob_stack[idx].clone();
                status = if data.is_empty() {
                    ObStatus::NoData
                } else {
                    ObStatus::Success
                };
                ctx.out = Some(data);
            }
            ObKind::Devnull => {
                status = ObStatus::NoData;
            }
        }
        let idx = match self.ob_index_of(id) {
            Some(i) if still_have_handler => i,
            _ => {
                self.ob_running = None;
                ctx.op = original_op;
                return status;
            }
        };
        self.ob_handlers[idx].flags |= OBH_STARTED;
        self.ob_running = None;

        match status {
            ObStatus::Failure => {
                self.ob_handlers[idx].flags |= OBH_DISABLED;
                ctx.out = Some(std::mem::take(&mut self.ob_stack[idx]));
                self.ob_handlers[idx].buffer_size = 0;
            }
            ObStatus::NoData => {
                ctx.reset();
                self.ob_stack[idx].clear();
                self.ob_handlers[idx].flags |= OBH_PROCESSED;
            }
            ObStatus::Success => {
                self.ob_stack[idx].clear();
                self.ob_handlers[idx].flags |= OBH_PROCESSED;
            }
        }
        ctx.op = original_op;
        status
    }

    /// php_output_op: pass data through the handler stack down to the SAPI.
    fn ob_op(&mut self, op: i64, data: &[u8]) {
        if self.ob_lock_error(op) {
            return;
        }
        let mut ctx = ObCtx::new(op);
        let cnt = if self.ob_deactivated {
            0
        } else {
            self.ob_stack.len()
        };
        if cnt > 0 {
            ctx.input = data.to_vec();
            if cnt > 1 {
                // ZEND_STACK_APPLY_TOPDOWN
                let mut i = cnt;
                while i > 0 {
                    i -= 1;
                    if i >= self.ob_handlers.len() {
                        break;
                    }
                    let was_disabled = self.ob_handlers[i].flags & OBH_DISABLED != 0;
                    let status = if was_disabled {
                        ObStatus::Failure
                    } else {
                        self.ob_handler_op(i, &mut ctx)
                    };
                    if self.ob_deactivated {
                        ctx.reset();
                        break;
                    }
                    match status {
                        ObStatus::NoData => break,
                        ObStatus::Success => {
                            if i > 0 {
                                ctx.swap();
                            }
                        }
                        ObStatus::Failure => {
                            if was_disabled {
                                if i == 0 {
                                    ctx.pass();
                                }
                            } else if i > 0 {
                                ctx.swap();
                            }
                        }
                    }
                }
            } else if self.ob_handlers[0].flags & OBH_DISABLED == 0 {
                self.ob_handler_op(0, &mut ctx);
            } else {
                ctx.pass();
            }
        } else {
            ctx.out = Some(data.to_vec());
        }
        if let Some(out) = ctx.out {
            if !out.is_empty() {
                self.mark_output_started();
            }
            if !out.is_empty() && self.output.len() + out.len() <= Self::MAX_OUTPUT_SIZE {
                self.output.extend_from_slice(&out);
            }
        }
    }

    /// php_output_handler_start
    pub fn ob_handler_start(&mut self, handler: Option<ObHandler>) -> bool {
        if self.ob_deactivated {
            return false;
        }
        self.ob_sync();
        if self.ob_lock_error(OB_OP_START) {
            return false;
        }
        let mut handler = match handler {
            Some(h) => h,
            None => return false,
        };
        handler.id = self.ob_alloc_id();
        self.ob_stack.push(Vec::new());
        self.ob_handlers.push(handler);
        true
    }

    /// Build a handler structure (php_output_handler_init).
    pub fn ob_make_handler(
        name: Vec<u8>,
        kind: ObKind,
        chunk_size: usize,
        flags: i64,
    ) -> ObHandler {
        let type_flag = if matches!(kind, ObKind::User(_)) {
            OBH_USER
        } else {
            OBH_INTERNAL
        };
        ObHandler {
            id: 0,
            name,
            kind,
            chunk_size,
            flags: (flags & !0xf00f) | type_flag,
            buffer_size: initbuf_size(chunk_size),
        }
    }

    /// php_output_flush: flush the active handler's buffer to the next level.
    pub fn ob_flush_active(&mut self) -> bool {
        self.ob_sync();
        let top = match self.ob_stack.len() {
            0 => return false,
            n => n - 1,
        };
        if self.ob_handlers[top].flags & OBH_FLUSHABLE == 0 {
            return false;
        }
        let mut ctx = ObCtx::new(OB_OP_FLUSH);
        let id = self.ob_handlers[top].id;
        self.ob_handler_op(top, &mut ctx);
        if let Some(out) = ctx.out.take() {
            if !out.is_empty() {
                // Temporarily remove the handler so the data goes to the parent.
                if let Some(i) = self.ob_index_of(id) {
                    if i + 1 == self.ob_stack.len() {
                        let buf = self.ob_stack.pop().unwrap();
                        let h = self.ob_handlers.pop().unwrap();
                        self.ob_write(&out);
                        if !self.ob_deactivated {
                            self.ob_stack.push(buf);
                            self.ob_handlers.push(h);
                        }
                    } else {
                        self.ob_write(&out);
                    }
                } else {
                    self.ob_write(&out);
                }
            }
        }
        true
    }

    /// php_output_clean: discard the active handler's buffer.
    pub fn ob_clean_active(&mut self) -> bool {
        self.ob_sync();
        let top = match self.ob_stack.len() {
            0 => return false,
            n => n - 1,
        };
        if self.ob_handlers[top].flags & OBH_CLEANABLE == 0 {
            return false;
        }
        let mut ctx = ObCtx::new(OB_OP_CLEAN);
        self.ob_handler_op(top, &mut ctx);
        true
    }

    /// php_output_stack_pop
    pub fn ob_stack_pop(&mut self, flags: i64) -> bool {
        self.ob_sync();
        if self.ob_deactivated || self.ob_stack.is_empty() {
            if flags & OB_POP_SILENT == 0 {
                let what = if flags & OB_POP_DISCARD != 0 {
                    "discard"
                } else {
                    "send"
                };
                let origin = self.ob_origin();
                let line = self.current_line;
                self.emit_notice_at(
                    &format!(
                        "{}(): Failed to {} buffer. No buffer to {}",
                        origin, what, what
                    ),
                    line,
                );
            }
            return false;
        }
        let top = self.ob_stack.len() - 1;
        if flags & OB_POP_FORCE == 0 && self.ob_handlers[top].flags & OBH_REMOVABLE == 0 {
            if flags & OB_POP_SILENT == 0 {
                let what = if flags & OB_POP_DISCARD != 0 {
                    "discard"
                } else {
                    "send"
                };
                let name = String::from_utf8_lossy(&self.ob_handlers[top].name).to_string();
                let origin = self.ob_origin();
                let line = self.current_line;
                self.emit_notice_at(
                    &format!(
                        "{}(): Failed to {} buffer of {} ({})",
                        origin, what, name, top
                    ),
                    line,
                );
            }
            return false;
        }
        let id = self.ob_handlers[top].id;
        let mut ctx = ObCtx::new(OB_OP_FINAL);
        if self.ob_handlers[top].flags & OBH_DISABLED == 0 {
            if self.ob_handlers[top].flags & OBH_STARTED == 0 {
                ctx.op |= OB_OP_START;
            }
            if flags & OB_POP_DISCARD != 0 {
                ctx.op |= OB_OP_CLEAN;
            }
            self.ob_handler_op(top, &mut ctx);
        }
        if self.ob_deactivated {
            return true;
        }
        // Pop it off the stack.
        if let Some(i) = self.ob_index_of(id) {
            if i + 1 == self.ob_stack.len() {
                self.ob_stack.pop();
                self.ob_handlers.pop();
            } else {
                self.ob_stack.remove(i);
                self.ob_handlers.remove(i);
            }
        } else if !self.ob_stack.is_empty() {
            self.ob_stack.pop();
            self.ob_handlers.pop();
        }
        if let Some(out) = ctx.out {
            if !out.is_empty() && flags & OB_POP_DISCARD == 0 {
                self.ob_write(&out);
            }
        }
        true
    }

    /// php_output_end_all: finalize every handler (request shutdown).
    pub fn ob_end_all(&mut self) {
        self.ob_sync();
        let mut guard = 0;
        while !self.ob_deactivated && !self.ob_stack.is_empty() && guard < 100_000 {
            guard += 1;
            if !self.ob_stack_pop(OB_POP_FORCE) {
                break;
            }
        }
    }

    /// php_output_discard_all
    pub fn ob_discard_all(&mut self) {
        self.ob_sync();
        let mut guard = 0;
        while !self.ob_deactivated && !self.ob_stack.is_empty() && guard < 100_000 {
            guard += 1;
            if !self.ob_stack_pop(OB_POP_DISCARD | OB_POP_FORCE) {
                break;
            }
        }
    }

    /// php_output_clean_all
    pub fn ob_clean_all(&mut self) {
        self.ob_sync();
        let mut i = self.ob_stack.len();
        while i > 0 {
            i -= 1;
            if i >= self.ob_stack.len() {
                continue;
            }
            self.ob_stack[i].clear();
            let mut ctx = ObCtx::new(OB_OP_CLEAN);
            self.ob_handler_op(i, &mut ctx);
        }
    }

    /// Take a pending fatal error raised inside the output layer.
    pub fn ob_take_pending_fatal(&mut self) -> Option<VmError> {
        self.ob_pending_fatal.take()
    }

    /// Status array data for one level: (name, type, flags, level, chunk, size, used).
    pub fn ob_status_entry(
        &mut self,
        level: usize,
    ) -> Option<(Vec<u8>, i64, i64, i64, i64, i64, i64)> {
        self.ob_sync();
        let h = self.ob_handlers.get(level)?;
        let used = self.ob_stack.get(level).map(|b| b.len()).unwrap_or(0);
        Some((
            h.name.clone(),
            h.flags & 0xf,
            h.flags,
            level as i64,
            h.chunk_size as i64,
            h.buffer_size as i64,
            used as i64,
        ))
    }

    pub fn ob_handler_names(&mut self) -> Vec<Vec<u8>> {
        self.ob_sync();
        self.ob_handlers.iter().map(|h| h.name.clone()).collect()
    }

    pub fn ob_active_flags(&mut self) -> Option<i64> {
        self.ob_sync();
        self.ob_handlers.last().map(|h| h.flags)
    }

    pub fn ob_active_name(&mut self) -> Vec<u8> {
        self.ob_sync();
        self.ob_handlers
            .last()
            .map(|h| h.name.clone())
            .unwrap_or_default()
    }

    /// Whether a handler with this name is currently on the stack.
    pub fn ob_handler_started(&mut self, name: &[u8]) -> bool {
        self.ob_sync();
        self.ob_handlers.iter().any(|h| h.name == name)
    }
}
