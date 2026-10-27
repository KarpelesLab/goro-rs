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
    /// The emulated VM stack (live in VM_USED / VM_CAP / VM_PAGE while
    /// this context runs; only meaningful in a saved context).
    vm_used: usize,
    vm_cap: usize,
    vm_page: usize,
}

thread_local! {
    static CTX: Cell<StackCtx> = const {
        Cell::new(StackCtx {
            limit: 0,
            emu_used: 0,
            emu_max: UNSET,
            vm_used: 0,
            vm_cap: 0,
            vm_page: VM_STACK_PAGE_SIZE,
        })
    };
    /// Bytes of extra segments in use on this thread.
    static GROWN: Cell<usize> = const { Cell::new(0) };
    static POOL: RefCell<Vec<DefaultStack>> = const { RefCell::new(Vec::new()) };
    /// zend.reserved_stack_size, and whether the emulated C stack is
    /// checked at all.
    static RESERVED: Cell<(usize, bool)> = const { Cell::new((DEFAULT_RESERVED, true)) };
    /// Emulated VM stack (zend_vm_stack) of the running context: bytes
    /// used in the current page (EG(vm_stack_top)), the current page's size
    /// (EG(vm_stack_end); 0: not set up, frames are not accounted) and the
    /// size new pages get (EG(vm_stack_page_size)).
    static VM_USED: Cell<usize> = const { Cell::new(0) };
    static VM_CAP: Cell<usize> = const { Cell::new(0) };
    static VM_PAGE: Cell<usize> = const { Cell::new(VM_STACK_PAGE_SIZE) };
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

/// Let the parser grow the native stack too (deeply nested expressions).
pub fn install_parser_stack_grower() {
    goro_parser::parser::set_stack_grower(|f| with_room(f).is_ok());
}

/// Emulated C stack a level of expression compilation uses
/// (zend_compile_expr() & co. check the stack limit at each level).
pub const COMPILE_COST: usize = 256;

/// Charge a level of expression compilation (zend_check_stack_limit() in
/// the compiler). Returns the emulated stack size when it is exhausted.
#[inline]
pub fn enter_compile() -> Result<(), usize> {
    if CTX.with(|c| c.get().emu_max) == UNSET {
        configure(0, 0);
    }
    CTX.with(|cell| {
        let mut c = cell.get();
        if c.emu_max > 0 && c.emu_used + COMPILE_COST > c.emu_max {
            return Err(c.emu_max);
        }
        c.emu_used += COMPILE_COST;
        cell.set(c);
        Ok(())
    })
}

/// Undo `enter_compile`.
#[inline]
pub fn leave_compile() {
    CTX.with(|cell| {
        let mut c = cell.get();
        c.emu_used = c.emu_used.saturating_sub(COMPILE_COST);
        cell.set(c);
    });
}

/// The stack was exhausted: `MAX_GROWN` bytes of extra segments are in use
/// (or no more could be mapped).
#[derive(Debug)]
pub struct Exhausted;

/// Whether the current stack segment has the room a call needs (with_room()
/// then runs it in place).
#[inline]
pub fn has_room() -> bool {
    sp().saturating_sub(current_limit()) >= RED_ZONE
}

/// Room below which releasing a value no longer recurses into what it holds
/// (see `release_stack_low`): what freeing one object or array needs, with
/// a wide margin.
const RELEASE_ZONE: usize = 256 * 1024;

/// Whether the current stack segment is nearly used up, for releases (a
/// long chain of objects let go at once recurses once per link; past this
/// the rest of the chain is freed iteratively, see object.rs). One load and
/// a compare: while the segment's limit is not known yet (no PHP code ran
/// on this thread) it says no.
#[inline(always)]
pub fn release_stack_low() -> bool {
    let limit = CTX.with(|c| c.get().limit);
    sp() < limit.wrapping_add(RELEASE_ZONE)
}

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

/// configure() from the zend.max_allowed_stack_size and
/// zend.reserved_stack_size INI values (OnUpdateMaxAllowedStackSize,
/// OnUpdateReservedStackSize: invalid values are rejected).
pub fn configure_from_ini(max: Option<&[u8]>, reserved: Option<&[u8]>) {
    let q = |v: Option<&[u8]>| {
        v.map(|s| crate::ini_quantity::parse_quantity(s, true).0 as i64)
            .unwrap_or(0)
    };
    let (max, reserved) = (q(max), q(reserved));
    let reserved = if reserved > 0 && (reserved as usize) >= DEFAULT_RESERVED {
        reserved as usize
    } else {
        0
    };
    let max = if max < -1 { 0 } else { max };
    configure(max, reserved);
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

/// Charge `cost` bytes of native recursion (e.g. php_var_dump() of a
/// nested object) against the emulated C stack; false when it would
/// overflow (zend_call_stack_overflowed()), nothing being charged then.
pub fn enter_native(cost: usize) -> bool {
    if CTX.with(|c| c.get().emu_max) == UNSET {
        configure(0, 0);
    }
    CTX.with(|cell| {
        let mut c = cell.get();
        if c.emu_max > 0 && c.emu_used + cost > c.emu_max {
            return false;
        }
        c.emu_used += cost;
        cell.set(c);
        true
    })
}

/// Undo `enter_native`.
pub fn leave_native(cost: usize) {
    CTX.with(|cell| {
        let mut c = cell.get();
        c.emu_used = c.emu_used.saturating_sub(cost);
        cell.set(c);
    });
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

/// zend_call_stack_get() and friends, in the emulated C stack's terms:
/// (base, max_size, position, EG(stack_limit)). The base is the top of the
/// running thread's stack; positions below it are the emulated use.
pub fn call_stack_info() -> (usize, usize, usize, usize) {
    if CTX.with(|c| c.get().emu_max) == UNSET {
        configure(0, 0);
    }
    let c = CTX.with(|c| c.get());
    let base = match thread_stack_bounds() {
        Some((low, size)) if size < (1 << 40) => low + size,
        _ => sp().next_multiple_of(1 << 20),
    };
    let (reserved, checked) = RESERVED.with(|r| r.get());
    let max_size = if checked && c.emu_max > 0 {
        c.emu_max + reserved
    } else {
        detected_stack_size()
    };
    let position = base.saturating_sub(c.emu_used);
    let limit = if checked {
        base.saturating_sub(c.emu_max)
    } else {
        0
    };
    (base, max_size, position, limit)
}

/// The current stack context (saved when switching to a fiber).
pub fn save() -> StackCtx {
    let mut c = CTX.with(|c| c.get());
    c.vm_used = VM_USED.with(|v| v.get());
    c.vm_cap = VM_CAP.with(|v| v.get());
    c.vm_page = VM_PAGE.with(|v| v.get());
    c
}

/// Switch back to a saved stack context.
pub fn restore(ctx: StackCtx) {
    CTX.with(|c| c.set(ctx));
    VM_USED.with(|v| v.set(ctx.vm_used));
    VM_CAP.with(|v| v.set(ctx.vm_cap));
    VM_PAGE.with(|v| v.set(ctx.vm_page));
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
        // (set up when the fiber starts: vm_stack_fiber_start())
        vm_used: 0,
        vm_cap: 0,
        vm_page: FIBER_VM_STACK_SIZE,
    }
}

/// ZEND_VM_STACK_PAGE_SIZE: 16K zvals.
pub const VM_STACK_PAGE_SIZE: usize = 256 * 1024;
/// ZEND_FIBER_VM_STACK_SIZE: 1024 zvals.
pub const FIBER_VM_STACK_SIZE: usize = 16 * 1024;
/// ZEND_VM_STACK_HEADER_SLOTS * sizeof(zval)
const VM_STACK_HEADER: usize = 32;
/// ZEND_CALL_FRAME_SLOT * sizeof(zval): a zend_execute_data.
pub const CALL_FRAME_SIZE: usize = 80;

/// A call frame on the emulated VM stack, to hand back to `vm_pop`: the
/// page's use before the frame and, when the frame needed a new page
/// (ZEND_CALL_ALLOCATED), the previous page's size (else 0).
#[derive(Clone, Copy, Debug)]
pub struct VmFrame {
    prev_used: usize,
    prev_cap: usize,
}

/// VM stack accounting is not set up.
const NO_FRAME: VmFrame = VmFrame {
    prev_used: usize::MAX,
    prev_cap: 0,
};

/// zend_vm_stack_init(): the request's first VM stack page. Returns its
/// size (already in use when the request starts).
pub fn vm_stack_reset() -> usize {
    VM_PAGE.with(|v| v.set(VM_STACK_PAGE_SIZE));
    VM_CAP.with(|v| v.set(VM_STACK_PAGE_SIZE));
    VM_USED.with(|v| v.set(VM_STACK_HEADER));
    VM_STACK_PAGE_SIZE
}

/// zend_vm_stack_push_call_frame(): reserve `size` bytes of VM stack for
/// a call frame. A frame that does not fit in the current page gets a new
/// one (zend_vm_stack_extend()); when that allocation is over
/// memory_limit, Err(bytes) is the failed allocation (the memory-limit
/// error is pending).
#[inline]
pub fn vm_push(size: usize) -> Result<VmFrame, usize> {
    let cap = VM_CAP.with(|v| v.get());
    let used = VM_USED.with(|v| v.get());
    if size <= cap.wrapping_sub(used) {
        VM_USED.with(|v| v.set(used + size));
        return Ok(VmFrame {
            prev_used: used,
            prev_cap: 0,
        });
    }
    if cap == 0 {
        return Ok(NO_FRAME);
    }
    vm_extend(used, cap, size)
}

#[cold]
#[inline(never)]
fn vm_extend(used: usize, cap: usize, size: usize) -> Result<VmFrame, usize> {
    let page = VM_PAGE.with(|v| v.get());
    let alloc = if size < page - VM_STACK_HEADER {
        page
    } else {
        // ZEND_VM_STACK_PAGE_ALIGNED_SIZE()
        (size + VM_STACK_HEADER + page - 1) & !(page - 1)
    };
    if !crate::value::memory_alloc(alloc) {
        return Err(alloc);
    }
    VM_USED.with(|v| v.set(VM_STACK_HEADER + size));
    VM_CAP.with(|v| v.set(alloc));
    Ok(VmFrame {
        prev_used: used,
        prev_cap: cap,
    })
}

/// zend_vm_stack_free_call_frame(): release a frame from `vm_push` (the
/// most recent one still pushed).
#[inline]
pub fn vm_pop(frame: VmFrame) {
    if frame.prev_cap == 0 {
        if frame.prev_used != usize::MAX {
            VM_USED.with(|v| v.set(frame.prev_used));
        }
    } else {
        vm_pop_page(frame);
    }
}

#[cold]
#[inline(never)]
fn vm_pop_page(frame: VmFrame) {
    crate::value::memory_free(VM_CAP.with(|v| v.get()));
    VM_USED.with(|v| v.set(frame.prev_used));
    VM_CAP.with(|v| v.set(frame.prev_cap));
}

/// Pops a frame from `vm_push` when dropped (also when a fiber is
/// unwound).
pub struct VmFrameGuard(pub VmFrame);

impl Drop for VmFrameGuard {
    #[inline]
    fn drop(&mut self) {
        vm_pop(self.0);
    }
}

/// zend_fiber_execute(): a starting fiber gets its own VM stack page
/// (with a frame slot reserved for the fiber's bottom frame).
pub fn vm_stack_fiber_start() {
    VM_PAGE.with(|v| v.set(FIBER_VM_STACK_SIZE));
    VM_CAP.with(|v| v.set(FIBER_VM_STACK_SIZE));
    VM_USED.with(|v| v.set(VM_STACK_HEADER + CALL_FRAME_SIZE));
    crate::value::memory_charge(FIBER_VM_STACK_SIZE);
}

/// zend_fiber_cleanup(): a finished fiber's VM stack is freed.
pub fn vm_stack_fiber_end() {
    let cap = VM_CAP.with(|v| v.replace(0));
    if cap != 0 {
        crate::value::memory_free(cap);
    }
    VM_USED.with(|v| v.set(0));
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
