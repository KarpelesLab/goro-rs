//! Heap accounting for `memory_limit`.
//!
//! [`CountingAlloc`] wraps the system allocator and keeps a running count
//! of the bytes in use. When an allocation would take the count past the
//! armed limit, the installed hook runs once (the engine turns it into
//! PHP's "Allowed memory size of N bytes exhausted" fatal error at its next
//! safe point) and a headroom is granted for the error's own handling and
//! the shutdown sequence; going past the headroom too ends the process
//! from the hook.
//!
//! The counters are plain relaxed loads and stores, not read-modify-write
//! operations: the interpreter allocates from one thread, and an
//! occasional lost update from a helper thread only blurs the count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

/// The global allocator of the `goro` binary.
pub struct CountingAlloc;

static USED: AtomicUsize = AtomicUsize::new(0);
/// The highest [`USED`] since the last [`reset_peak`].
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// The count an allocation may not exceed (`usize::MAX`: unarmed).
static LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);
/// Set when the limit was crossed; cleared by the engine.
static PENDING: AtomicBool = AtomicBool::new(false);
/// Inside the hook: allocations are not checked.
static IN_HOOK: AtomicBool = AtomicBool::new(false);
/// `fn(requested: usize, hard: bool)` as an address (0: none).
static HOOK: AtomicUsize = AtomicUsize::new(0);
/// Set after the first crossing: the next one is final.
static HEADROOM_USED: AtomicBool = AtomicBool::new(false);

/// Extra bytes granted after the limit is crossed, for the fatal error and
/// the shutdown functions.
pub const HEADROOM: usize = 64 << 20;

#[cold]
#[inline(never)]
fn over(size: usize, total: usize) {
    if IN_HOOK.swap(true, Relaxed) {
        return;
    }
    // A single allocation beyond the headroom is refused at once.
    let hard =
        HEADROOM_USED.swap(true, Relaxed) || total > LIMIT.load(Relaxed).saturating_add(HEADROOM);
    if !hard {
        PENDING.store(true, Relaxed);
        let l = LIMIT.load(Relaxed);
        LIMIT.store(l.saturating_add(HEADROOM), Relaxed);
    }
    let h = HOOK.load(Relaxed);
    if h != 0 {
        let f: fn(usize, bool) = unsafe { std::mem::transmute(h) };
        f(size, hard);
    }
    IN_HOOK.store(false, Relaxed);
}

#[inline(always)]
fn charge(size: usize) {
    let n = USED.load(Relaxed).wrapping_add(size);
    if n > LIMIT.load(Relaxed) {
        over(size, n);
    }
    USED.store(n, Relaxed);
    if n > PEAK.load(Relaxed) {
        PEAK.store(n, Relaxed);
    }
}

#[inline(always)]
fn credit(size: usize) {
    USED.store(USED.load(Relaxed).saturating_sub(size), Relaxed);
}

unsafe impl GlobalAlloc for CountingAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            credit(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            credit(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        credit(layout.size());
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old {
            charge(new_size - old);
        }
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            if new_size > old {
                credit(new_size - old);
            }
        } else if new_size < old {
            credit(old - new_size);
        }
        p
    }
}

/// Bytes currently allocated (through [`CountingAlloc`]).
#[inline]
pub fn used() -> usize {
    USED.load(Relaxed)
}

/// The highest heap use since the last [`reset_peak`].
#[inline]
pub fn peak() -> usize {
    PEAK.load(Relaxed)
}

/// Starts a new peak measurement from the current use.
pub fn reset_peak() {
    PEAK.store(USED.load(Relaxed), Relaxed);
}

/// Arms the limit: allocations may take the count up to `limit` bytes in
/// total (`None` disarms).
pub fn set_limit(limit: Option<usize>) {
    HEADROOM_USED.store(false, Relaxed);
    LIMIT.store(limit.unwrap_or(usize::MAX), Relaxed);
}

/// The armed limit, if any.
pub fn limit() -> Option<usize> {
    match LIMIT.load(Relaxed) {
        usize::MAX => None,
        l => Some(l),
    }
}

/// Installs the hook run when the limit is crossed: `requested` is the
/// allocation's size, `hard` is true when the headroom is exhausted too
/// (the hook should then end the process).
pub fn set_hook(f: fn(usize, bool)) {
    HOOK.store(f as usize, Relaxed);
}

/// Whether the limit was crossed since the last [`take_pending`].
#[inline(always)]
pub fn pending() -> bool {
    PENDING.load(Relaxed)
}

/// Clears the crossing flag; true when it was set.
pub fn take_pending() -> bool {
    PENDING.swap(false, Relaxed)
}
