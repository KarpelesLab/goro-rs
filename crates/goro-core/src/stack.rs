//! Native stack used by PHP calls.
//!
//! Every PHP call recurses through the Rust stack (execute_op_array ->
//! execute_frame -> handler -> execute_op_array ...), so deep recursion
//! would run off the end of the thread's stack. Calls check how much room
//! is left and, when it runs low, continue on a fresh stack segment
//! (`corosensei::on_stack`), so the depth is only bounded by
//! `MAX_GROWN` bytes of extra segments.
//!
//! This module also keeps php-src's view of the C stack: in php-src only
//! re-entering the executor from native code (callbacks of internal
//! functions, magic methods, destructors, generators ...) uses C stack,
//! and once `zend.max_allowed_stack_size - zend.reserved_stack_size` of it
//! is used, zend_call_stack_size_error() throws. Re-entries are charged
//! `REENTRY_COST` bytes against that budget (see `enter_reentry`).
//!
//! Fibers run on their own stacks: switching to one swaps the whole
//! `StackCtx` (see fiber.rs).

use std::cell::{Cell, RefCell};

use corosensei::stack::{DefaultStack, Stack};

/// Room that must be left on the stack when a call starts: what a call
/// uses before the next check (opcode handlers, builtin functions).
const RED_ZONE: usize = 512 * 1024;
/// Size of an extra stack segment.
const SEGMENT_SIZE: usize = 8 * 1024 * 1024;
/// Bytes of extra segments a thread may use at once.
const MAX_GROWN: usize = 512 * 1024 * 1024;
/// Free segments kept for reuse (a call depth oscillating around a segment
/// boundary must not mmap/munmap on every call).
const POOL_SIZE: usize = 2;
/// Emulated C stack charged per executor re-entry.
pub const REENTRY_COST: usize = 2048;
/// zend.reserved_stack_size when unset: ZEND_ALLOCA_MAX_SIZE + 16K.
pub const DEFAULT_RESERVED: usize = 48 * 1024;
/// Stack size assumed when the thread's cannot be found (8 MiB, the usual
/// RLIMIT_STACK).
const DEFAULT_STACK: usize = 8 * 1024 * 1024;

/// The stack the current code runs on.
#[derive(Clone, Copy, Debug)]
pub struct StackCtx {
    /// Lowest address usable on the current stack segment (0: not known
    /// yet; found on first use).
    limit: usize,
    /// Emulated C stack in use (bytes).
    emu_used: usize,
    /// Emulated C stack size (zend.max_allowed_stack_size -
    /// zend.reserved_stack_size); 0 when unchecked, `UNSET` until
    /// configured.
    emu_max: usize,
}

thread_local! {
    static CTX: Cell<StackCtx> = const {
        Cell::new(StackCtx {
            limit: 0,
            emu_used: 0,
            emu_max: UNSET,
        })
    };
    /// Bytes of extra segments in use on this thread.
    static GROWN: Cell<usize> = const { Cell::new(0) };
    static POOL: RefCell<Vec<DefaultStack>> = const { RefCell::new(Vec::new()) };
    /// zend.reserved_stack_size, and whether the emulated C stack is
    /// checked at all.
    static RESERVED: Cell<(usize, bool)> = const { Cell::new((DEFAULT_RESERVED, true)) };
}

const UNSET: usize = usize::MAX;

/// Approximate current stack pointer.
#[inline(always)]
fn sp() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

/// Bounds (lowest address, size) of the running thread's stack.
#[cfg(target_os = "linux")]
fn thread_stack_bounds() -> Option<(usize, usize)> {
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let r = libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
        libc::pthread_attr_destroy(&mut attr);
        if r != 0 || addr.is_null() {
            return None;
        }
        Some((addr as usize, size))
    }
}

#[cfg(not(target_os = "linux"))]
fn thread_stack_bounds() -> Option<(usize, usize)> {
    None
}

/// Size of the main stack as php-src detects it (zend_call_stack_get():
/// the RLIMIT_STACK of the main thread).
pub fn detected_stack_size() -> usize {
    #[cfg(unix)]
    unsafe {
        let mut rl: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_STACK, &mut rl) == 0
            && rl.rlim_cur != libc::RLIM_INFINITY
            && rl.rlim_cur > 0
        {
            return rl.rlim_cur as usize;
        }
    }
    DEFAULT_STACK
}

#[cold]
fn find_limit() -> usize {
    let here = sp();
    let limit = match thread_stack_bounds() {
        // A main thread stack is reported with its rlimit size; an
        // unlimited one is treated as 8 MiB.
        Some((low, size)) if size < (1 << 40) && low < here => low,
        _ => here.saturating_sub(DEFAULT_STACK / 2),
    };
    // Keep clear of the guard page(s) at the bottom.
    limit + 64 * 1024
}

#[inline]
fn current_limit() -> usize {
    let c = CTX.with(|c| c.get());
    if c.limit != 0 {
        return c.limit;
    }
    let limit = find_limit();
    CTX.with(|cell| {
        let mut c = cell.get();
        c.limit = limit;
        cell.set(c);
    });
    limit
}

/// The stack was exhausted: `MAX_GROWN` bytes of extra segments are in use
/// (or no more could be mapped).
#[derive(Debug)]
pub struct Exhausted;

/// Run `f`, on a fresh stack segment if the current one is nearly used up.
#[inline]
pub fn with_room<R>(f: impl FnOnce() -> R) -> Result<R, Exhausted> {
    if sp().saturating_sub(current_limit()) >= RED_ZONE {
        return Ok(f());
    }
    grow(f)
}

/// Gives a segment back to the pool (also when unwinding).
struct SegmentGuard {
    seg: Option<DefaultStack>,
    prev_limit: usize,
}

impl Drop for SegmentGuard {
    fn drop(&mut self) {
        CTX.with(|cell| {
            let mut c = cell.get();
            c.limit = self.prev_limit;
            cell.set(c);
        });
        GROWN.with(|g| g.set(g.get().saturating_sub(SEGMENT_SIZE)));
        if let Some(seg) = self.seg.take() {
            POOL.with(|p| {
                let mut p = p.borrow_mut();
                if p.len() < POOL_SIZE {
                    p.push(seg);
                }
            });
        }
    }
}

#[inline(never)]
#[cold]
fn grow<R>(f: impl FnOnce() -> R) -> Result<R, Exhausted> {
    if GROWN.with(|g| g.get()) + SEGMENT_SIZE > MAX_GROWN {
        return Err(Exhausted);
    }
    let seg = match POOL.with(|p| p.borrow_mut().pop()) {
        Some(s) => s,
        None => DefaultStack::new(SEGMENT_SIZE).map_err(|_| Exhausted)?,
    };
    let seg_limit = seg.limit().get();
    let prev_limit = current_limit();
    GROWN.with(|g| g.set(g.get() + SEGMENT_SIZE));
    CTX.with(|cell| {
        let mut c = cell.get();
        // (limit() is the guard page)
        c.limit = seg_limit + 64 * 1024;
        cell.set(c);
    });
    let mut guard = SegmentGuard {
        seg: Some(seg),
        prev_limit,
    };
    let stack = guard.seg.as_mut().expect("segment");
    Ok(corosensei::on_stack(stack, f))
}

/// Set the emulated C stack size of the main stack
/// (zend.max_allowed_stack_size and zend.reserved_stack_size; 0 = default,
/// a negative maximum disables the check).
pub fn configure(max_allowed: i64, reserved: usize) {
    let max = if max_allowed < 0 {
        0
    } else if max_allowed == 0 {
        detected_stack_size()
    } else {
        max_allowed as usize
    };
    let reserved = if reserved == 0 {
        DEFAULT_RESERVED
    } else {
        reserved
    };
    let emu_max = max.saturating_sub(reserved).max(usize::from(max > 0));
    RESERVED.with(|r| r.set((reserved, max > 0)));
    CTX.with(|cell| {
        let mut c = cell.get();
        c.emu_max = emu_max;
        cell.set(c);
    });
}

/// Charge an executor re-entry against the emulated C stack. Returns the
/// emulated stack size when it is exhausted (nothing is charged then).
#[inline]
pub fn enter_reentry() -> Result<(), usize> {
    if CTX.with(|c| c.get().emu_max) == UNSET {
        configure(0, 0);
    }
    CTX.with(|cell| {
        let mut c = cell.get();
        if c.emu_max > 0 && c.emu_used + REENTRY_COST > c.emu_max {
            return Err(c.emu_max);
        }
        c.emu_used += REENTRY_COST;
        cell.set(c);
        Ok(())
    })
}

/// Undo `enter_reentry`.
#[inline]
pub fn leave_reentry() {
    CTX.with(|cell| {
        let mut c = cell.get();
        c.emu_used = c.emu_used.saturating_sub(REENTRY_COST);
        cell.set(c);
    });
}

/// The emulated C stack size reported by the "Maximum call stack size"
/// error of the current stack.
pub fn emulated_size() -> usize {
    if CTX.with(|c| c.get().emu_max) == UNSET {
        configure(0, 0);
    }
    CTX.with(|c| c.get().emu_max)
}

/// The current stack context (saved when switching to a fiber).
pub fn save() -> StackCtx {
    CTX.with(|c| c.get())
}

/// Switch back to a saved stack context.
pub fn restore(ctx: StackCtx) {
    CTX.with(|c| c.set(ctx));
}

/// Context of a fiber running on `stack`, whose emulated C stack has
/// `fiber_stack_size` bytes (fiber.stack_size).
pub fn fiber_ctx(stack: &DefaultStack, fiber_stack_size: usize) -> StackCtx {
    let (reserved, checked) = RESERVED.with(|r| r.get());
    let emu_max = if checked {
        fiber_stack_size.saturating_sub(reserved).max(1)
    } else {
        0
    };
    StackCtx {
        limit: stack.limit().get() + 64 * 1024,
        emu_used: 0,
        emu_max,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(n: usize) -> usize {
        // Uses about 4 KiB of stack per level.
        let buf = std::hint::black_box([0u8; 4096]);
        if n == 0 {
            return buf[0] as usize;
        }
        with_room(|| depth(n - 1) + 1).unwrap() + buf[1] as usize
    }

    #[test]
    fn grows_past_the_thread_stack() {
        // 40000 * 4 KiB = 160 MiB: far more than a test thread's stack.
        let h = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| depth(40_000))
            .unwrap();
        assert_eq!(h.join().unwrap(), 40_000);
    }

    #[test]
    fn reentry_budget() {
        configure(64 * 1024, 48 * 1024);
        let mut n = 0;
        while enter_reentry().is_ok() {
            n += 1;
        }
        assert_eq!(n, (16 * 1024) / REENTRY_COST);
        assert_eq!(enter_reentry(), Err(16 * 1024));
        for _ in 0..n {
            leave_reentry();
        }
        assert!(enter_reentry().is_ok());
    }
}
