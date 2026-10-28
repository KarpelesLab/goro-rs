//! PHP strings: refcounted, copy-on-write byte strings.
//!
//! PHP manual (Strings): "A string is series of characters, where a character
//! is the same as a byte." Strings are therefore `[u8]`, never assumed UTF-8.
//!
//! Representation: a [`PhpStr`] is one thin pointer (8 bytes) to a single heap
//! block holding a header (refcount, cached hash, length, capacity) followed by
//! the bytes. Cloning bumps a non-atomic refcount; writing through
//! [`PhpStr::make_mut`] / [`PhpStr::push_bytes`] copies only when the block is
//! shared (copy on write), and appends in place with amortised growth when it
//! is not, so `$s .= "x"` loops are linear.
//!
//! Immortal strings (the empty string, the 256 one-byte strings, and interned
//! literals) carry a flag in the refcount word: cloning and dropping them
//! touches nothing, and their hash is computed up front so they are never
//! written to. That makes them safe to keep in process-wide tables.

use std::alloc::{Layout, alloc, dealloc, realloc};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::ptr::NonNull;

const IMMORTAL: usize = 1 << (usize::BITS - 1);

#[repr(C)]
struct Header {
    /// Reference count; the top bit marks an immortal string.
    rc: Cell<usize>,
    /// Cached hash of the bytes; 0 means "not computed yet".
    hash: Cell<u64>,
    len: usize,
    cap: usize,
}

const HDR: usize = std::mem::size_of::<Header>();

/// A static block for immortal strings with up to `N` bytes.
#[repr(C)]
struct StaticStr<const N: usize> {
    hdr: Header,
    data: [u8; N],
}

// SAFETY: immortal blocks are never written after construction: their
// refcount is never touched (IMMORTAL bit) and their hash is precomputed.
struct SyncCell<T>(T);
unsafe impl<T> Sync for SyncCell<T> {}

/// Hash function for string bytes. Not cryptographic; PHP arrays are ordered
/// so the hash only affects speed, never iteration order. Never returns 0.
#[inline]
pub const fn hash_bytes(b: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut h: u64 = (b.len() as u64).wrapping_mul(K) ^ 0x243F_6A88_85A3_08D3;
    let mut i = 0;
    while i + 8 <= b.len() {
        let w = u64::from_le_bytes([
            b[i],
            b[i + 1],
            b[i + 2],
            b[i + 3],
            b[i + 4],
            b[i + 5],
            b[i + 6],
            b[i + 7],
        ]);
        h = (h ^ w).wrapping_mul(K);
        h ^= h >> 29;
        i += 8;
    }
    if i < b.len() {
        let mut w: u64 = 0;
        let mut s = 0;
        while i < b.len() {
            w |= (b[i] as u64) << s;
            s += 8;
            i += 1;
        }
        h = (h ^ w).wrapping_mul(K);
        h ^= h >> 29;
    }
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 32;
    h | 1
}

const fn static_one(b: u8) -> StaticStr<1> {
    StaticStr {
        hdr: Header {
            rc: Cell::new(IMMORTAL),
            hash: Cell::new(hash_bytes(&[b])),
            len: 1,
            cap: 1,
        },
        data: [b],
    }
}

static EMPTY: SyncCell<StaticStr<0>> = SyncCell(StaticStr {
    hdr: Header {
        rc: Cell::new(IMMORTAL),
        hash: Cell::new(hash_bytes(&[])),
        len: 0,
        cap: 0,
    },
    data: [],
});

static ONE_BYTE: SyncCell<[StaticStr<1>; 256]> = SyncCell({
    let mut i = 0;
    // A const array initialiser cannot call a const fn per element directly
    // with the index, so build it with a loop over a placeholder.
    let mut arr = [const { static_one(0) }; 256];
    while i < 256 {
        arr[i] = static_one(i as u8);
        i += 1;
    }
    arr
});

/// A PHP string value. 8 bytes; cheap to clone.
pub struct PhpStr {
    ptr: NonNull<Header>,
}

impl PhpStr {
    #[inline]
    fn hdr(&self) -> &Header {
        // SAFETY: ptr always points to a live header (heap or static).
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    fn data_ptr(&self) -> *mut u8 {
        // SAFETY: the bytes follow the header in the same block.
        unsafe { (self.ptr.as_ptr() as *mut u8).add(HDR) }
    }

    fn layout(cap: usize) -> Layout {
        Layout::from_size_align(HDR + cap, std::mem::align_of::<Header>())
            .expect("string too large")
    }

    /// Allocates an uninitialised block of capacity `cap` with length `len`.
    fn alloc_block(len: usize, cap: usize) -> PhpStr {
        debug_assert!(len <= cap);
        let layout = Self::layout(cap);
        // SAFETY: layout has non-zero size (header).
        let p = unsafe { alloc(layout) } as *mut Header;
        let Some(ptr) = NonNull::new(p) else {
            std::alloc::handle_alloc_error(layout)
        };
        // SAFETY: freshly allocated, properly aligned.
        unsafe {
            ptr.as_ptr().write(Header {
                rc: Cell::new(1),
                hash: Cell::new(0),
                len,
                cap,
            })
        };
        PhpStr { ptr }
    }

    /// The empty string (no allocation).
    #[inline]
    pub fn empty() -> PhpStr {
        PhpStr {
            ptr: NonNull::from(&EMPTY.0.hdr),
        }
    }

    /// A one-byte string (no allocation).
    #[inline]
    pub fn from_byte(b: u8) -> PhpStr {
        PhpStr {
            ptr: NonNull::from(&ONE_BYTE.0[b as usize].hdr),
        }
    }

    /// Copies `b` into a new string. Empty and one-byte inputs don't allocate.
    pub fn new(b: &[u8]) -> PhpStr {
        match b.len() {
            0 => Self::empty(),
            1 => Self::from_byte(b[0]),
            n => {
                let s = Self::alloc_block(n, n);
                // SAFETY: block has room for n bytes.
                unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), s.data_ptr(), n) };
                s
            }
        }
    }

    /// An empty string with room for `cap` bytes, for building.
    pub fn with_capacity(cap: usize) -> PhpStr {
        if cap == 0 {
            Self::empty()
        } else {
            Self::alloc_block(0, cap)
        }
    }

    /// Concatenation `a . b` into a new string.
    pub fn concat(a: &[u8], b: &[u8]) -> PhpStr {
        let n = a.len() + b.len();
        if n <= 1 {
            return Self::new(if a.is_empty() { b } else { a });
        }
        let s = Self::alloc_block(n, n);
        // SAFETY: room for n bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(a.as_ptr(), s.data_ptr(), a.len());
            std::ptr::copy_nonoverlapping(b.as_ptr(), s.data_ptr().add(a.len()), b.len());
        }
        s
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.hdr().len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.hdr().len == 0
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: len bytes are initialised.
        unsafe { std::slice::from_raw_parts(self.data_ptr(), self.hdr().len) }
    }

    /// True when this string is immortal (static or interned).
    #[inline]
    pub fn is_immortal(&self) -> bool {
        self.hdr().rc.get() & IMMORTAL != 0
    }

    /// Current reference count (immortal strings report `usize::MAX`).
    pub fn refcount(&self) -> usize {
        let rc = self.hdr().rc.get();
        if rc & IMMORTAL != 0 { usize::MAX } else { rc }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.hdr().rc.get() == 1
    }

    /// The cached hash of the bytes (computed on first use).
    #[inline]
    pub fn hash_code(&self) -> u64 {
        let h = self.hdr().hash.get();
        if h != 0 {
            return h;
        }
        let h = hash_bytes(self.as_bytes());
        // Only mortal strings reach here: immortal ones are prehashed.
        self.hdr().hash.set(h);
        h
    }

    /// Pointer identity.
    #[inline]
    pub fn ptr_eq(a: &PhpStr, b: &PhpStr) -> bool {
        a.ptr == b.ptr
    }

    /// Makes this string uniquely owned with room for `extra` more bytes.
    fn reserve(&mut self, extra: usize) {
        let len = self.len();
        let need = len.checked_add(extra).expect("string too large");
        if self.is_unique() {
            let cap = self.hdr().cap;
            if need <= cap {
                return;
            }
            let new_cap = need.max(cap.saturating_mul(2)).max(16);
            // SAFETY: unique heap block (immortal ones are never unique: their
            // rc has the top bit set), grown in place.
            let p = unsafe {
                realloc(
                    self.ptr.as_ptr() as *mut u8,
                    Self::layout(cap),
                    HDR + new_cap,
                )
            } as *mut Header;
            let Some(ptr) = NonNull::new(p) else {
                std::alloc::handle_alloc_error(Self::layout(new_cap))
            };
            // SAFETY: header moved with the block.
            unsafe { (*ptr.as_ptr()).cap = new_cap };
            self.ptr = ptr;
        } else {
            let new_cap = if extra == 0 { need } else { need.max(16) };
            let s = Self::alloc_block(len, new_cap.max(1));
            // SAFETY: room for len bytes.
            unsafe { std::ptr::copy_nonoverlapping(self.data_ptr(), s.data_ptr(), len) };
            *self = s;
        }
    }

    /// Mutable access to the bytes, copying first if shared.
    pub fn make_mut(&mut self) -> &mut [u8] {
        if !self.is_unique() {
            self.reserve(0);
        }
        self.hdr().hash.set(0);
        let len = self.len();
        // SAFETY: unique block with len initialised bytes.
        unsafe { std::slice::from_raw_parts_mut(self.data_ptr(), len) }
    }

    /// Appends bytes (`$s .= ...`), in place when unshared.
    pub fn push_bytes(&mut self, b: &[u8]) {
        if b.is_empty() {
            return;
        }
        if self.is_empty() && b.len() == 1 && !self.is_unique() {
            *self = Self::from_byte(b[0]);
            return;
        }
        self.reserve(b.len());
        let len = self.len();
        // SAFETY: reserve made room; the block is unique.
        unsafe {
            std::ptr::copy_nonoverlapping(b.as_ptr(), self.data_ptr().add(len), b.len());
            (*self.ptr.as_ptr()).len = len + b.len();
        }
        self.hdr().hash.set(0);
    }

    /// Appends one byte.
    #[inline]
    pub fn push(&mut self, b: u8) {
        self.push_bytes(&[b]);
    }

    /// Shortens the string to `n` bytes (no-op if already shorter).
    pub fn truncate(&mut self, n: usize) {
        if n >= self.len() {
            return;
        }
        if n <= 1 {
            *self = Self::new(&self.as_bytes()[..n]);
            return;
        }
        if !self.is_unique() {
            *self = Self::new(&self.as_bytes()[..n]);
            return;
        }
        // SAFETY: unique heap block.
        unsafe { (*self.ptr.as_ptr()).len = n };
        self.hdr().hash.set(0);
    }

    /// Lossy UTF-8 view, for messages and debugging.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(self.as_bytes()).into_owned()
    }

    /// Returns the interned (immortal, deduplicated) copy of `b`.
    ///
    /// Interned strings live for the rest of the process, like PHP's interned
    /// literals; use this for compile-time literals and names, not for
    /// runtime data.
    pub fn intern(b: &[u8]) -> PhpStr {
        if b.len() <= 1 {
            return Self::new(b);
        }
        INTERNER.with(|t| {
            let mut t = t.borrow_mut();
            if let Some(s) = t.get(b) {
                return s.clone();
            }
            let s = Self::new(b);
            s.hash_code();
            s.hdr().rc.set(IMMORTAL);
            // The key borrows the immortal block, which is never freed.
            // SAFETY: immortal strings are never deallocated or mutated.
            let key: &'static [u8] = unsafe { std::slice::from_raw_parts(s.data_ptr(), s.len()) };
            t.insert(key, s.clone());
            s
        })
    }
}

thread_local! {
    static INTERNER: RefCell<HashMap<&'static [u8], PhpStr>> = RefCell::new(HashMap::new());
}

impl Clone for PhpStr {
    #[inline]
    fn clone(&self) -> PhpStr {
        let rc = self.hdr().rc.get();
        if rc & IMMORTAL == 0 {
            self.hdr().rc.set(rc + 1);
        }
        PhpStr { ptr: self.ptr }
    }
}

impl Drop for PhpStr {
    #[inline]
    fn drop(&mut self) {
        let rc = self.hdr().rc.get();
        if rc & IMMORTAL != 0 {
            return;
        }
        if rc == 1 {
            let cap = self.hdr().cap;
            // SAFETY: last reference to a heap block allocated with this layout.
            unsafe { dealloc(self.ptr.as_ptr() as *mut u8, Self::layout(cap)) };
        } else {
            self.hdr().rc.set(rc - 1);
        }
    }
}

impl Default for PhpStr {
    fn default() -> Self {
        Self::empty()
    }
}

impl Deref for PhpStr {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsRef<[u8]> for PhpStr {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl PartialEq for PhpStr {
    #[inline]
    fn eq(&self, other: &PhpStr) -> bool {
        if self.ptr == other.ptr {
            return true;
        }
        if self.len() != other.len() {
            return false;
        }
        let (ha, hb) = (self.hdr().hash.get(), other.hdr().hash.get());
        if ha != 0 && hb != 0 && ha != hb {
            return false;
        }
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for PhpStr {}

impl PartialEq<[u8]> for PhpStr {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }
}
impl PartialEq<&[u8]> for PhpStr {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_bytes() == *other
    }
}
impl PartialEq<&str> for PhpStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Hash for PhpStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash_code());
    }
}

impl PartialOrd for PhpStr {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PhpStr {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl fmt::Debug for PhpStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string_lossy())
    }
}

impl From<&[u8]> for PhpStr {
    fn from(b: &[u8]) -> Self {
        PhpStr::new(b)
    }
}
impl From<&str> for PhpStr {
    fn from(s: &str) -> Self {
        PhpStr::new(s.as_bytes())
    }
}
impl From<Vec<u8>> for PhpStr {
    fn from(v: Vec<u8>) -> Self {
        PhpStr::new(&v)
    }
}
impl From<String> for PhpStr {
    fn from(s: String) -> Self {
        PhpStr::new(s.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size() {
        assert_eq!(std::mem::size_of::<PhpStr>(), 8);
        assert_eq!(std::mem::size_of::<Option<PhpStr>>(), 8);
    }

    #[test]
    fn basics() {
        let s = PhpStr::new(b"hello");
        assert_eq!(s.len(), 5);
        assert_eq!(&*s, b"hello");
        let t = s.clone();
        assert_eq!(s.refcount(), 2);
        assert!(PhpStr::ptr_eq(&s, &t));
        drop(t);
        assert_eq!(s.refcount(), 1);
        assert!(PhpStr::empty().is_empty());
        assert!(PhpStr::empty().is_immortal());
        assert_eq!(&*PhpStr::from_byte(b'x'), b"x");
        assert!(PhpStr::new(b"x").is_immortal());
        assert_eq!(PhpStr::new(b"\x00\xff"), PhpStr::new(b"\x00\xff"));
    }

    #[test]
    fn cow() {
        let a = PhpStr::new(b"abc");
        let mut b = a.clone();
        b.make_mut()[0] = b'X';
        assert_eq!(&*a, b"abc");
        assert_eq!(&*b, b"Xbc");
        let mut c = a.clone();
        c.push_bytes(b"def");
        assert_eq!(&*a, b"abc");
        assert_eq!(&*c, b"abcdef");
    }

    #[test]
    fn append_in_place() {
        let mut s = PhpStr::empty();
        for i in 0..1000u32 {
            s.push((b'a' as u32 + i % 26) as u8);
        }
        assert_eq!(s.len(), 1000);
        assert_eq!(s[27], b'b');
        let h1 = s.hash_code();
        s.push(b'!');
        assert_ne!(h1, s.hash_code());
        assert_eq!(s.hash_code(), hash_bytes(&s));
        s.truncate(3);
        assert_eq!(&*s, b"abc");
    }

    #[test]
    fn interning() {
        let a = PhpStr::intern(b"literal");
        let b = PhpStr::intern(b"literal");
        assert!(PhpStr::ptr_eq(&a, &b));
        assert!(a.is_immortal());
        let mut c = a.clone();
        c.push(b'!');
        assert_eq!(&*a, b"literal");
        assert_eq!(&*c, b"literal!");
        assert!(!c.is_immortal());
    }

    #[test]
    fn hash_consistency() {
        for n in 0..40 {
            let v: Vec<u8> = (0..n as u8).collect();
            assert_eq!(PhpStr::new(&v).hash_code(), hash_bytes(&v));
            assert_ne!(hash_bytes(&v), 0);
        }
    }
}
