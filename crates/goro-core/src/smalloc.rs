//! A cache of freed small blocks in front of the system allocator.
//!
//! The engine allocates and frees small blocks at a high rate (strings,
//! arrays, objects, call buffers): a WordPress front page makes millions of
//! such calls, and glibc's per-thread cache only keeps 7 blocks of each
//! size, so most of them reach its arenas (locked once the process has a
//! second thread). Like the bins of zend_alloc.c, freed blocks of up to 1 KiB
//! are kept here per size class, per thread (an intrusive list in each
//! block), and handed out again by the next allocations of their class.
//!
//! A block is allocated from the system with its class's size, so any block
//! of a class serves any request of that class; a request's class follows
//! from its layout, which Rust passes to dealloc() and realloc() too. Blocks
//! beyond a class's cap go back to the system, and so do the blocks cached
//! by a thread that ends (they are leaked: their number is bounded).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;

/// The global allocator of the goro binaries.
pub struct SmallCache;

/// Requests of up to this size (and an alignment of at most 16) are cached.
const MAX_SMALL: usize = 1024;
/// Size classes: 16-byte steps up to 128, then 32-byte steps up to 256,
/// 64-byte steps up to 512 and 128-byte steps up to 1024 (index 0 unused).
const NCLASS: usize = 21;

/// The class of a request of `size` bytes (1..=MAX_SMALL).
#[inline]
fn class_of(size: usize) -> usize {
    CLASS_OF[(size + 15) >> 4] as usize
}

/// class_of() by size rounded up to 16 bytes, in 16-byte units.
const CLASS_OF: [u8; MAX_SMALL / 16 + 1] = {
    let mut t = [0; MAX_SMALL / 16 + 1];
    let mut i = 1;
    while i < t.len() {
        let size = i * 16;
        t[i] = if size <= 128 {
            i
        } else if size <= 256 {
            8 + ((size - 128 + 31) >> 5)
        } else if size <= 512 {
            12 + ((size - 256 + 63) >> 6)
        } else {
            16 + ((size - 512 + 127) >> 7)
        } as u8;
        i += 1;
    }
    t
};

/// The size of the blocks of class `c`.
#[inline]
const fn class_size(c: usize) -> usize {
    if c <= 8 {
        c << 4
    } else if c <= 12 {
        128 + ((c - 8) << 5)
    } else if c <= 16 {
        256 + ((c - 12) << 6)
    } else {
        512 + ((c - 16) << 7)
    }
}

/// How many freed blocks of each class a thread keeps (about 128 KiB of
/// each: at most 2.5 MiB in all).
const CLASS_CAP: [u32; NCLASS] = {
    let mut caps = [0; NCLASS];
    let mut c = 1;
    while c < NCLASS {
        caps[c] = (131072 / class_size(c)) as u32;
        c += 1;
    }
    caps
};

struct Bin {
    head: *mut u8,
    len: u32,
}

struct Cache {
    bins: [Bin; NCLASS],
    /// The rest of the thread's current chunk of the region (see
    /// region): new blocks are carved from it.
    cur: usize,
    end: usize,
}

thread_local! {
    static CACHE: UnsafeCell<Cache> = const {
        UnsafeCell::new(Cache {
            bins: [const {
                Bin {
                    head: std::ptr::null_mut(),
                    len: 0,
                }
            }; NCLASS],
            cur: 0,
            end: 0,
        })
    };
}

/// The chunks the threads carve new small blocks from, as zend_alloc.c
/// carves its bins' runs from its chunks: one system allocation per chunk
/// rather than one per block. A block carved from a chunk never goes back
/// to the system: once freed it stays cached (past its class's cap), for
/// the next allocations of its class. Past a number of chunks, blocks come
/// from the system one by one again.
mod region {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// What a thread takes at a time (and its alignment).
    pub(super) const CHUNK: usize = 1 << 20;
    /// The table of the chunks made (their addresses, by hash): twice as
    /// many slots as chunks may be made.
    const SLOTS: usize = 4096;
    const MAX_CHUNKS: usize = SLOTS / 2;
    static TABLE: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
    static COUNT: AtomicUsize = AtomicUsize::new(0);

    #[inline]
    fn slot_of(base: usize) -> usize {
        ((base >> 20).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 52) & (SLOTS - 1)
    }

    /// A new chunk (readable and writable), or None.
    #[cold]
    pub(super) fn chunk() -> Option<usize> {
        if COUNT.fetch_add(1, Ordering::Relaxed) >= MAX_CHUNKS {
            COUNT.fetch_sub(1, Ordering::Relaxed);
            return None;
        }
        // SAFETY: a valid non-zero layout (a power of two alignment).
        let p = unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK, CHUNK)) };
        if p.is_null() {
            COUNT.fetch_sub(1, Ordering::Relaxed);
            return None;
        }
        let base = p as usize;
        let mut i = slot_of(base);
        // (a free slot exists: at most half of them are used)
        while TABLE[i]
            .compare_exchange(0, base, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {
            i = (i + 1) & (SLOTS - 1);
        }
        Some(base)
    }

    /// Whether `p` was carved from a chunk.
    #[inline]
    pub(super) fn contains(p: *mut u8) -> bool {
        let base = p as usize & !(CHUNK - 1);
        let mut i = slot_of(base);
        loop {
            match TABLE[i].load(Ordering::Acquire) {
                0 => return false,
                b if b == base => return true,
                _ => i = (i + 1) & (SLOTS - 1),
            }
        }
    }
}

/// A new block of class `c` carved from the thread's chunk of the region
/// (a new chunk taken when it is used up), or null.
#[inline(never)]
fn carve(c: usize) -> *mut u8 {
    let size = class_size(c);
    CACHE
        .try_with(|cache| {
            // SAFETY: as in take().
            let cache = unsafe { &mut *cache.get() };
            if cache.end - cache.cur < size {
                match region::chunk() {
                    Some(at) => {
                        cache.cur = at;
                        cache.end = at + region::CHUNK;
                    }
                    None => return std::ptr::null_mut(),
                }
            }
            let p = cache.cur;
            cache.cur += size;
            p as *mut u8
        })
        .unwrap_or(std::ptr::null_mut())
}

/// Cache block `p` of class `c` whatever the bin holds (a block of the
/// region, which never goes back to the system).
#[inline(never)]
fn put_always(c: usize, p: *mut u8) -> bool {
    CACHE
        .try_with(|cache| {
            // SAFETY: as in take().
            let bin = unsafe { &mut (*cache.get()).bins[c] };
            // SAFETY: as in put().
            unsafe { (p as *mut *mut u8).write(bin.head) };
            bin.head = p;
            bin.len += 1;
        })
        .is_ok()
}

#[inline]
fn small(layout: &Layout) -> bool {
    layout.size() <= MAX_SMALL && layout.align() <= 16
}

/// The system layout of the blocks of class `c`.
#[inline]
fn class_layout(c: usize) -> Layout {
    // SAFETY: a non-zero multiple of 16 below isize::MAX, a power of two
    // alignment.
    unsafe { Layout::from_size_align_unchecked(class_size(c), 16) }
}

/// A cached block of class `c`, or null.
#[inline]
fn take(c: usize) -> *mut u8 {
    CACHE.with(|cache| {
        // SAFETY: the cache is this thread's, and nothing else borrows it
        // while this runs (no allocation happens in here).
        let bin = unsafe { &mut (*cache.get()).bins[c] };
        let p = bin.head;
        if !p.is_null() {
            // SAFETY: a cached block starts with the next block's address.
            bin.head = unsafe { (p as *mut *mut u8).read() };
            bin.len -= 1;
        }
        p
    })
}

/// Cache block `p` of class `c`; false when the class's bin is full.
#[inline]
fn put(c: usize, p: *mut u8) -> bool {
    CACHE.with(|cache| {
        // SAFETY: as in take().
        let bin = unsafe { &mut (*cache.get()).bins[c] };
        if bin.len >= CLASS_CAP[c] {
            return false;
        }
        // SAFETY: the block is at least 16 bytes, 16-aligned, and unused.
        unsafe { (p as *mut *mut u8).write(bin.head) };
        bin.head = p;
        bin.len += 1;
        true
    })
}

/// A new block from the system for `layout` (as large as its class for a
/// small one).
///
/// # Safety
///
/// GlobalAlloc::alloc()'s contract.
#[inline(never)]
unsafe fn alloc_system(layout: Layout) -> *mut u8 {
    if small(&layout) {
        // SAFETY: a valid non-zero layout.
        unsafe { System.alloc(class_layout(class_of(layout.size()))) }
    } else {
        // SAFETY: the caller's contract.
        unsafe { System.alloc(layout) }
    }
}

/// Give block `ptr` of `layout` back to the system.
///
/// # Safety
///
/// GlobalAlloc::dealloc()'s contract (and alloc_system() made the block).
#[inline(never)]
unsafe fn dealloc_system(ptr: *mut u8, layout: Layout) {
    if small(&layout) {
        // SAFETY: allocated by System with the class's layout.
        unsafe { System.dealloc(ptr, class_layout(class_of(layout.size()))) }
    } else {
        // SAFETY: allocated by System with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

// SAFETY: blocks handed out are either fresh system blocks of at least the
// requested size and alignment, or freed ones of the same class (as large);
// every block goes back to the system with the layout it was allocated with.
unsafe impl GlobalAlloc for SmallCache {
    // (out of line: inlined where Rust allocates, the fat LTO build of the
    // binaries takes many times as long)
    #[inline(never)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if small(&layout) {
            let c = class_of(layout.size());
            let p = take(c);
            if !p.is_null() {
                return p;
            }
            let p = carve(c);
            if !p.is_null() {
                return p;
            }
        }
        // SAFETY: the caller's contract.
        unsafe { alloc_system(layout) }
    }

    #[inline(never)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if small(&layout) {
            let c = class_of(layout.size());
            if put(c, ptr) {
                return;
            }
            if region::contains(ptr) {
                // (a thread whose cache is gone: the block is lost)
                put_always(c, ptr);
                return;
            }
        }
        // SAFETY: the caller's contract.
        unsafe { dealloc_system(ptr, layout) }
    }

    #[inline(never)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !small(&layout) {
            // SAFETY: the caller's contract.
            return unsafe { System.alloc_zeroed(layout) };
        }
        // SAFETY: the caller's contract.
        let p = unsafe { self.alloc(layout) };
        if !p.is_null() {
            // SAFETY: the block has at least layout.size() bytes.
            unsafe { std::ptr::write_bytes(p, 0, layout.size()) };
        }
        p
    }

    #[inline(never)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_small = small(&layout);
        let new_small = new_size <= MAX_SMALL && layout.align() <= 16;
        if old_small && new_small && class_of(layout.size()) == class_of(new_size) {
            // (the block is as large as its class)
            return ptr;
        }
        if !old_small && !new_small {
            // SAFETY: allocated by System with this layout.
            return unsafe { System.realloc(ptr, layout, new_size) };
        }
        // SAFETY: new_size is non-zero and, rounded up to the alignment,
        // does not overflow isize (the caller's contract).
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        // SAFETY: as above.
        let new = unsafe { self.alloc(new_layout) };
        if !new.is_null() {
            // SAFETY: both blocks hold at least the smaller size.
            unsafe {
                std::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
        }
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        for size in 1..=MAX_SMALL {
            let c = class_of(size);
            assert!((1..NCLASS).contains(&c), "{size}");
            assert!(class_size(c) >= size, "{size}");
            assert!(c == 1 || class_size(c - 1) < size, "{size}");
            assert_eq!(class_size(c) % 16, 0);
        }
    }
}
