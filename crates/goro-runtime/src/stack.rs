//! The native stack limit (PHP's `zend.max_allowed_stack_size`).
//!
//! Script frames live on the heap, but each callback, destructor or magic
//! method runs a nested interpreter loop on the native stack, and compiling
//! recurses on it. These are checked against the running stack's limit, which
//! the engine sets at startup and swaps when it switches to a fiber.

use std::cell::Cell;

thread_local! {
    /// The lowest address code may reach (0: no limit).
    static LOW: Cell<usize> = const { Cell::new(0) };
    /// The size named in error messages.
    static SIZE: Cell<usize> = const { Cell::new(0) };
}

/// The address of the top of the running stack, near enough.
#[inline(always)]
pub fn position() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

/// Whether the running stack has reached its limit.
#[inline(always)]
pub fn exceeded() -> bool {
    position() < LOW.get()
}

/// Bytes left before the limit (`usize::MAX` without one).
pub fn remaining() -> usize {
    match LOW.get() {
        0 => usize::MAX,
        low => position().saturating_sub(low),
    }
}

/// The limit in force, as (lowest address, size for messages).
pub fn current() -> (usize, usize) {
    (LOW.get(), SIZE.get())
}

/// Sets the limit for what runs next on this thread.
pub fn set(low: usize, size: usize) {
    LOW.set(low);
    SIZE.set(size);
}
