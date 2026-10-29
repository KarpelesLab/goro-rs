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
//! Small blocks come from per-thread size-class free lists (see the
//! "small blocks" section); larger ones from the system allocator.
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

// ---------------------------------------------------------------- small blocks
//
// Blocks of up to `SMALL_MAX` bytes (alignment up to 16) come from
// per-thread free lists, one per size class, refilled by carving 256 KiB
// chunks taken from the system allocator, as a request-oriented PHP heap
// does. Freed small blocks go back on the freeing thread's list and are
// reused; chunks are not returned to the system. The size class is found
// from the layout alone, which `dealloc` and `realloc` receive too, so
// blocks carry no header. Accounting (`charge`/`credit`) is by requested
// size, as before, so memory_limit sees the same numbers.

const SMALL_MAX: usize = 512;
const NCLASS: usize = 16;
const CHUNK: usize = 256 << 10;

/// Size of each class: 16-byte steps to 128, 32 to 256, 64 to 512.
const CLASS_SIZE: [usize; NCLASS] = [
    16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512,
];

#[inline(always)]
fn class_of(size: usize) -> usize {
    if size <= 128 {
        size.max(1).div_ceil(16) - 1
    } else if size <= 256 {
        8 + (size - 129) / 32
    } else {
        12 + (size - 257) / 64
    }
}

#[inline(always)]
fn is_small(layout: &Layout) -> bool {
    layout.size() <= SMALL_MAX && layout.align() <= 16
}

struct Pool {
    free: [*mut u8; NCLASS],
    bump: *mut u8,
    end: *mut u8,
}

thread_local! {
    static POOL: std::cell::UnsafeCell<Pool> = const {
        std::cell::UnsafeCell::new(Pool {
            free: [std::ptr::null_mut(); NCLASS],
            bump: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
        })
    };
}

#[inline(always)]
fn small_alloc(size: usize) -> *mut u8 {
    let c = class_of(size);
    POOL.with(|p| {
        // SAFETY: the pool is this thread's; nothing re-enters it.
        let p = unsafe { &mut *p.get() };
        let head = p.free[c];
        if !head.is_null() {
            // SAFETY: a free block holds the next free block's address.
            p.free[c] = unsafe { *(head as *mut *mut u8) };
            return head;
        }
        carve(p, CLASS_SIZE[c])
    })
}

#[inline(never)]
fn carve(p: &mut Pool, sz: usize) -> *mut u8 {
    if (p.end as usize) - (p.bump as usize) < sz {
        // SAFETY: a non-zero, valid layout.
        let chunk = unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK, 16)) };
        if chunk.is_null() {
            return chunk;
        }
        p.bump = chunk;
        // SAFETY: within the chunk.
        p.end = unsafe { chunk.add(CHUNK) };
    }
    let b = p.bump;
    // SAFETY: `sz` bytes are left in the chunk.
    p.bump = unsafe { b.add(sz) };
    b
}

#[inline(always)]
fn small_free(ptr: *mut u8, size: usize) {
    let c = class_of(size);
    POOL.with(|p| {
        // SAFETY: as in small_alloc; every block has room for a pointer.
        let p = unsafe { &mut *p.get() };
        unsafe { *(ptr as *mut *mut u8) = p.free[c] };
        p.free[c] = ptr;
    })
}

unsafe impl GlobalAlloc for CountingAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        let p = if is_small(&layout) {
            small_alloc(layout.size())
        } else {
            unsafe { System.alloc(layout) }
        };
        if p.is_null() {
            credit(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        let p = if is_small(&layout) {
            let p = small_alloc(layout.size());
            if !p.is_null() {
                unsafe { std::ptr::write_bytes(p, 0, layout.size()) };
            }
            p
        } else {
            unsafe { System.alloc_zeroed(layout) }
        };
        if p.is_null() {
            credit(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if is_small(&layout) {
            small_free(ptr, layout.size());
        } else {
            unsafe { System.dealloc(ptr, layout) };
        }
        credit(layout.size());
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old {
            charge(new_size - old);
        }
        // SAFETY: the caller guarantees a valid new layout.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        let p = match (is_small(&layout), is_small(&new_layout)) {
            (false, false) => unsafe { System.realloc(ptr, layout, new_size) },
            (true, true) if class_of(old) == class_of(new_size) => ptr,
            (small_old, small_new) => {
                let q = if small_new {
                    small_alloc(new_size)
                } else {
                    unsafe { System.alloc(new_layout) }
                };
                if !q.is_null() {
                    // SAFETY: both blocks hold at least the smaller size.
                    unsafe { std::ptr::copy_nonoverlapping(ptr, q, old.min(new_size)) };
                    if small_old {
                        small_free(ptr, old);
                    } else {
                        unsafe { System.dealloc(ptr, layout) };
                    }
                }
                q
            }
        };
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

/// Raises the armed limit by `bytes` while alive: for work whose memory
/// PHP takes outside its own heap (libargon2, libsodium), which
/// memory_limit does not count.
pub struct Allowance(usize);

impl Allowance {
    pub fn new(bytes: usize) -> Allowance {
        let l = LIMIT.load(Relaxed);
        if l == usize::MAX {
            return Allowance(0);
        }
        LIMIT.store(l.saturating_add(bytes), Relaxed);
        Allowance(bytes)
    }
}

impl Drop for Allowance {
    fn drop(&mut self) {
        if self.0 != 0 {
            let l = LIMIT.load(Relaxed);
            LIMIT.store(l.saturating_sub(self.0), Relaxed);
        }
    }
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
