//! PHP arrays: ordered maps with int and string keys.
//!
//! PHP manual (Arrays): "An array in PHP is actually an ordered map. A map is
//! a type that associates values to keys." Keys are ints or strings; strings
//! holding canonical decimal integers are stored as ints (see
//! [`Key::from_bytes`]).
//!
//! # Representation
//!
//! [`PhpArray`] is a copy-on-write handle: one optional pointer (8 bytes;
//! the empty array is `None` and allocates nothing). Assigning an array
//! clones the handle; the first write through a shared handle copies the
//! data ([`PhpArray::make_mut`]).
//!
//! [`ArrayData`] stores either
//! - **packed**: a `Vec<Value>` for list-like arrays whose keys are exactly
//!   `0..len` in order (the common case: no hashing at all), or
//! - **hash**: an insertion-ordered entry vector plus an open-addressing
//!   index of `(entry, hash tag)` slots at load factor ≤ 1/2. Deleting
//!   leaves a tombstone in the entry vector so positions stay stable; the
//!   vector is compacted when it next has to grow.
//!
//! Positions (the internal pointer used by `current()`/`next()`, and
//! by-reference `foreach` iterators registered with [`PhpArray::iter_new`])
//! are entry indices. Converting packed → hash keeps them; compaction remaps
//! them, so iteration stays correct while the array is modified.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use crate::gc;
use crate::reference::PhpRef;
use crate::string::PhpStr;
use crate::value::Value;

// ---------------------------------------------------------------- keys

/// An array key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Int(i64),
    Str(PhpStr),
}

/// A borrowed array key, as yielded by iteration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyRef<'a> {
    Int(i64),
    Str(&'a PhpStr),
}

impl KeyRef<'_> {
    pub fn to_owned(self) -> Key {
        match self {
            KeyRef::Int(i) => Key::Int(i),
            KeyRef::Str(s) => Key::Str(s.clone()),
        }
    }
    /// The key as a PHP value.
    pub fn to_value(self) -> Value {
        match self {
            KeyRef::Int(i) => Value::Int(i),
            KeyRef::Str(s) => Value::Str(s.clone()),
        }
    }
}

impl fmt::Debug for KeyRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyRef::Int(i) => write!(f, "{i}"),
            KeyRef::Str(s) => write!(f, "{s:?}"),
        }
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

/// Parses a canonical decimal integer as PHP array keys do: an optional
/// `-`, then `0` or a non-zero digit followed by digits, within i64 range.
/// PHP manual (Arrays): "Strings containing valid decimal ints, unless the
/// number is preceded by a + sign, will be cast to the int type. E.g. the
/// key "8" will actually be stored under 8. On the other hand "08" will not
/// be cast". "-0" stays a string (observed on PHP 8.5).
pub fn canonical_int(b: &[u8]) -> Option<i64> {
    let (neg, digits) = match b.first()? {
        b'-' => (true, &b[1..]),
        _ => (false, b),
    };
    let first = *digits.first()?;
    if !first.is_ascii_digit() || (first == b'0' && (digits.len() > 1 || neg)) {
        return None;
    }
    if digits.len() > 19 {
        return None;
    }
    let mut v: u64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - b'0') as u64)?;
    }
    if neg {
        if v <= i64::MAX as u64 + 1 {
            Some((v as i64).wrapping_neg())
        } else {
            None
        }
    } else if v <= i64::MAX as u64 {
        Some(v as i64)
    } else {
        None
    }
}

impl Key {
    /// A key from string bytes, normalising canonical integers.
    pub fn from_bytes(b: &[u8]) -> Key {
        match canonical_int(b) {
            Some(i) => Key::Int(i),
            None => Key::Str(PhpStr::new(b)),
        }
    }

    /// A key from a string, normalising canonical integers (reuses the
    /// string otherwise).
    pub fn from_php_str(s: &PhpStr) -> Key {
        match canonical_int(s) {
            Some(i) => Key::Int(i),
            None => Key::Str(s.clone()),
        }
    }

    pub fn as_ref(&self) -> KeyRef<'_> {
        match self {
            Key::Int(i) => KeyRef::Int(*i),
            Key::Str(s) => KeyRef::Str(s),
        }
    }

    pub fn to_value(&self) -> Value {
        self.as_ref().to_value()
    }
}

impl From<i64> for Key {
    fn from(i: i64) -> Key {
        Key::Int(i)
    }
}
impl From<&str> for Key {
    fn from(s: &str) -> Key {
        Key::from_bytes(s.as_bytes())
    }
}

/// Slot hash of an int key: the splitmix64 finaliser (Steele, Lea and
/// Flood, "Fast splittable pseudorandom number generators", 2014), so that
/// both the low bits (slot position) and the high bits (tag) are mixed.
#[inline]
fn hash_int(i: i64) -> u64 {
    let mut x = i as u64;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

// ---------------------------------------------------------------- storage

/// The hash stored in a bucket: the key itself for int keys (as PHP does),
/// the string hash for string keys.
#[inline]
fn bucket_hash(k: KeyRef<'_>) -> u64 {
    match k {
        KeyRef::Int(i) => i as u64,
        KeyRef::Str(s) => s.hash_code(),
    }
}

/// The hash used to place a key in the index (int keys are mixed).
#[inline]
fn slot_hash(bh: u64, is_int: bool) -> u64 {
    if is_int {
        // Position (low half) from the key itself, folded, so runs of
        // nearby int keys land in nearby slots (cache friendly, like PHP's
        // identity hash for ints); tag (high half) from the mixed key.
        (hash_int(bh as i64) & 0xFFFF_FFFF_0000_0000)
            | ((bh ^ (bh >> 16) ^ (bh >> 32)) & 0xFFFF_FFFF)
    } else {
        bh
    }
}

/// An index slot: 0 when empty, else `(tag << 32) | (entry + 1)` where the
/// tag is the upper half of the slot hash.
type Slot = u64;

/// Hash arrays with at most this many entries have no index: lookups scan
/// the entries, comparing stored hashes first.
const SMALL: usize = 8;

/// One entry, 32 bytes: the value (`None` for a deleted entry), the stored
/// hash (the key for int keys) and the string key (`None` for int keys).
struct Bucket {
    val: Option<Value>,
    h: u64,
    skey: Option<PhpStr>,
}

impl Bucket {
    #[inline]
    fn key(&self) -> KeyRef<'_> {
        match &self.skey {
            Some(s) => KeyRef::Str(s),
            None => KeyRef::Int(self.h as i64),
        }
    }

    #[inline]
    fn matches(&self, bh: u64, k: KeyRef<'_>) -> bool {
        self.h == bh
            && match (&self.skey, k) {
                (None, KeyRef::Int(_)) => true,
                (Some(a), KeyRef::Str(b)) => a == b,
                _ => false,
            }
    }

    #[inline]
    fn new(k: Key, bh: u64, v: Value) -> Bucket {
        Bucket {
            val: Some(v),
            h: bh,
            skey: match k {
                Key::Int(_) => None,
                Key::Str(s) => Some(s),
            },
        }
    }
}

struct HashStore {
    entries: Vec<Bucket>,
    index: Box<[Slot]>,
    live: u32,
}

enum Store {
    Packed(Vec<Value>),
    Hash(HashStore),
}

const A_MAY_CYCLE: u8 = 1;
const A_GC_BUFFERED: u8 = 2;
const A_LITERAL: u8 = 4;
const N_HAS_INT: u8 = 1;
const N_FULL: u8 = 2;

/// The data of an array (shared between copies until written).
pub struct ArrayData {
    store: Store,
    /// Next index for `$a[] = v` (valid when `nstate & N_HAS_INT`).
    next_free: i64,
    nstate: u8,
    pub(crate) flags: Cell<u8>,
    /// Internal pointer (entry position).
    pos: u32,
    /// Registered iterator positions `(id, pos)` (by-reference foreach).
    iters: Vec<(u32, u32)>,
}

/// Returned by appends when the next index would overflow. PHP: "Cannot add
/// element to the array as the next element is already occupied" (thrown as
/// `Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextIndexOccupied;

impl NextIndexOccupied {
    pub const MESSAGE: &'static str =
        "Cannot add element to the array as the next element is already occupied";
    pub fn to_error(self) -> crate::PhpError {
        crate::PhpError::error(Self::MESSAGE)
    }
}

#[inline]
fn may_cycle_value(v: &Value) -> bool {
    matches!(v, Value::Array(_) | Value::Object(_) | Value::Ref(_))
}

#[inline]
fn needs_drop(v: &Value) -> bool {
    !matches!(
        v,
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_)
    )
}

/// Copy of an element for a separated array: references held only by the
/// array being copied are dereferenced (observed on PHP 8.5: after
/// `$r = &$a[0]; unset($r); $b = $a; $b[0] = 2;`, `$a[0]` is unchanged).
#[inline(always)]
fn copy_elem(v: &Value) -> Value {
    if needs_drop(v) {
        copy_owned_elem(v)
    } else {
        // SAFETY: scalar variants own nothing; a bitwise copy is a clone.
        unsafe { std::ptr::read(v) }
    }
}

#[inline(never)]
fn copy_owned_elem(v: &Value) -> Value {
    match v {
        Value::Ref(r) if r.refcount() == 1 => r.get(),
        v => v.clone(),
    }
}

impl HashStore {
    fn with_capacity(n: usize) -> HashStore {
        if n <= SMALL {
            return HashStore {
                entries: Vec::with_capacity(n.max(2)),
                index: Box::new([]),
                live: 0,
            };
        }
        let size = (n * 2).next_power_of_two();
        HashStore {
            entries: Vec::with_capacity(size / 2),
            index: vec![0; size].into_boxed_slice(),
            live: 0,
        }
    }

    #[inline]
    fn limit(&self) -> usize {
        if self.index.is_empty() {
            SMALL
        } else {
            self.index.len() / 2
        }
    }

    /// Finds the live entry for `k`, whose bucket hash is `bh`.
    #[inline]
    fn find(&self, k: KeyRef<'_>, bh: u64) -> Option<u32> {
        if self.index.is_empty() {
            for (i, b) in self.entries.iter().enumerate() {
                if b.matches(bh, k) && b.val.is_some() {
                    return Some(i as u32);
                }
            }
            return None;
        }
        let sh = slot_hash(bh, matches!(k, KeyRef::Int(_)));
        let mask = self.index.len() - 1;
        let tag = sh >> 32;
        let mut i = sh as usize & mask;
        let mut step = 0;
        loop {
            let s = self.index[i];
            if s == 0 {
                return None;
            }
            if s >> 32 == tag {
                let idx = (s as u32).wrapping_sub(1);
                let b = &self.entries[idx as usize];
                if b.matches(bh, k) && b.val.is_some() {
                    return Some(idx);
                }
            }
            // Triangular probing visits every slot of a power-of-two table
            // and avoids the primary clustering of linear probing.
            step += 1;
            i = (i + step) & mask;
        }
    }

    /// Records entry `idx` (with slot hash `sh`) in the index.
    #[inline]
    fn place(&mut self, idx: u32, sh: u64) {
        if self.index.is_empty() {
            return;
        }
        let mask = self.index.len() - 1;
        let mut i = sh as usize & mask;
        let mut step = 0;
        while self.index[i] != 0 {
            step += 1;
            i = (i + step) & mask;
        }
        self.index[i] = ((sh >> 32) << 32) | (idx as u64 + 1);
    }

    /// Rebuilds the index with room for at least `min_cap` entries,
    /// dropping tombstones. Returns the position remap (old → new) if any
    /// entry moved.
    #[inline(never)]
    fn rebuild(&mut self, min_cap: usize) -> Option<Vec<u32>> {
        let had_holes = self.live as usize != self.entries.len();
        let remap = if had_holes {
            let mut remap = Vec::with_capacity(self.entries.len() + 1);
            let mut n = 0u32;
            for b in &self.entries {
                remap.push(n);
                if b.val.is_some() {
                    n += 1;
                }
            }
            remap.push(n);
            self.entries.retain(|b| b.val.is_some());
            Some(remap)
        } else {
            None
        };
        let n = min_cap.max(self.entries.len());
        if n <= SMALL {
            self.index = Box::new([]);
            return remap;
        }
        let size = (n * 2).next_power_of_two();
        self.index = vec![0; size].into_boxed_slice();
        if self.entries.capacity() < size / 2 {
            self.entries.reserve_exact(size / 2 - self.entries.len());
        }
        for idx in 0..self.entries.len() {
            let b = &self.entries[idx];
            let sh = slot_hash(b.h, b.skey.is_none());
            self.place(idx as u32, sh);
        }
        remap
    }
}

impl Drop for ArrayData {
    fn drop(&mut self) {
        // Drop only the elements that own something; scalars are skipped
        // without calling drop glue (most arrays hold mostly scalars).
        match &mut self.store {
            Store::Packed(v) => {
                let len = v.len();
                let p = v.as_mut_ptr();
                // SAFETY: the length is reset first, so each element is
                // dropped at most once even if a drop panics.
                unsafe {
                    v.set_len(0);
                    for i in 0..len {
                        let e = &mut *p.add(i);
                        if needs_drop(e) {
                            std::ptr::drop_in_place(e);
                        }
                    }
                }
            }
            Store::Hash(h) => {
                let len = h.entries.len();
                let p = h.entries.as_mut_ptr();
                // SAFETY: as above.
                unsafe {
                    h.entries.set_len(0);
                    for i in 0..len {
                        let b = &mut *p.add(i);
                        if b.skey.is_some() {
                            std::ptr::drop_in_place(&mut b.skey);
                        }
                        if let Some(v) = &mut b.val
                            && needs_drop(v)
                        {
                            std::ptr::drop_in_place(v);
                        }
                    }
                }
            }
        }
    }
}

impl Clone for ArrayData {
    fn clone(&self) -> ArrayData {
        let store = match &self.store {
            Store::Packed(v) => {
                let mut out = Vec::with_capacity(v.len());
                out.extend(v.iter().map(copy_elem));
                Store::Packed(out)
            }
            Store::Hash(h) => {
                let mut entries = Vec::with_capacity(h.entries.capacity());
                entries.extend(h.entries.iter().map(|b| Bucket {
                    val: b.val.as_ref().map(copy_elem),
                    h: b.h,
                    skey: b.skey.clone(),
                }));
                Store::Hash(HashStore {
                    entries,
                    index: h.index.clone(),
                    live: h.live,
                })
            }
        };
        ArrayData {
            store,
            next_free: self.next_free,
            nstate: self.nstate,
            flags: Cell::new(self.flags.get() & A_MAY_CYCLE),
            pos: self.pos,
            iters: self.iters.clone(),
        }
    }
}

impl Default for ArrayData {
    fn default() -> Self {
        Self::new()
    }
}

impl ArrayData {
    pub fn new() -> ArrayData {
        ArrayData {
            store: Store::Packed(Vec::new()),
            next_free: 0,
            nstate: 0,
            flags: Cell::new(0),
            pos: 0,
            iters: Vec::new(),
        }
    }

    pub fn with_capacity(n: usize) -> ArrayData {
        let mut a = Self::new();
        a.store = Store::Packed(Vec::with_capacity(n));
        a
    }

    #[inline]
    pub fn len(&self) -> usize {
        match &self.store {
            Store::Packed(v) => v.len(),
            Store::Hash(h) => h.live as usize,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether this array uses the packed (list) representation.
    pub fn is_packed(&self) -> bool {
        matches!(self.store, Store::Packed(_))
    }

    /// Whether keys are exactly 0..len in order (`array_is_list`).
    pub fn is_list(&self) -> bool {
        match &self.store {
            Store::Packed(_) => true,
            Store::Hash(h) => h
                .entries
                .iter()
                .filter(|b| b.val.is_some())
                .enumerate()
                .all(|(i, b)| b.key() == KeyRef::Int(i as i64)),
        }
    }

    /// The index the next append would use, or `None` if it would overflow.
    pub fn next_index(&self) -> Option<i64> {
        if self.nstate & N_FULL != 0 {
            None
        } else if self.nstate & N_HAS_INT != 0 {
            Some(self.next_free)
        } else {
            Some(0)
        }
    }

    /// Sets the next append index (`array_pop` resets it, for example).
    pub fn set_next_index(&mut self, n: i64) {
        self.next_free = n;
        self.nstate = N_HAS_INT;
    }

    #[inline]
    fn note_int_key(&mut self, k: i64) {
        if self.nstate & N_HAS_INT == 0 {
            self.nstate |= N_HAS_INT;
            match k.checked_add(1) {
                Some(n) => self.next_free = n,
                None => self.nstate |= N_FULL,
            }
        } else if k >= self.next_free && self.nstate & N_FULL == 0 {
            match k.checked_add(1) {
                Some(n) => self.next_free = n,
                None => self.nstate |= N_FULL,
            }
        }
    }

    #[inline]
    fn note_value(&self, v: &Value) {
        if may_cycle_value(v) {
            self.flags.set(self.flags.get() | A_MAY_CYCLE);
        }
    }

    #[inline]
    fn note_mut_access(&self) {
        self.flags.set(self.flags.get() | A_MAY_CYCLE);
    }

    pub(crate) fn may_cycle(&self) -> bool {
        self.flags.get() & A_MAY_CYCLE != 0
    }

    /// Marks this array as a compile-time literal (an immutable constant
    /// array shared by every evaluation of the expression). Literals are
    /// leaves for the cycle collector and are not counted as collected
    /// (observed on PHP 8.5: a literal `[1, 2]` inside a garbage cycle is not
    /// counted by `gc_collect_cycles()`, a runtime-built one is). The flag is
    /// dropped when a write separates a copy.
    pub fn mark_literal(&self) {
        self.flags.set(self.flags.get() | A_LITERAL);
    }

    /// Whether this array is a compile-time literal ([`Self::mark_literal`]).
    pub fn is_literal(&self) -> bool {
        self.flags.get() & A_LITERAL != 0
    }

    // ------------------------------------------------------------ lookup

    #[inline]
    pub fn get(&self, k: KeyRef<'_>) -> Option<&Value> {
        match k {
            KeyRef::Int(i) => self.get_int(i),
            KeyRef::Str(s) => self.get_str(s),
        }
    }

    #[inline]
    pub fn get_int(&self, i: i64) -> Option<&Value> {
        match &self.store {
            Store::Packed(v) => {
                if (i as u64) < v.len() as u64 {
                    Some(&v[i as usize])
                } else {
                    None
                }
            }
            Store::Hash(h) => {
                let idx = h.find(KeyRef::Int(i), i as u64)?;
                h.entries[idx as usize].val.as_ref()
            }
        }
    }

    /// Lookup by a string key that is *already normalised* (not a canonical
    /// integer). Use [`Key::from_php_str`] or [`ArrayData::get_bytes`] otherwise.
    #[inline]
    pub fn get_str(&self, s: &PhpStr) -> Option<&Value> {
        match &self.store {
            Store::Packed(_) => None,
            Store::Hash(h) => {
                let idx = h.find(KeyRef::Str(s), s.hash_code())?;
                h.entries[idx as usize].val.as_ref()
            }
        }
    }

    /// Lookup by raw string bytes, normalising integer-like strings.
    pub fn get_bytes(&self, b: &[u8]) -> Option<&Value> {
        match canonical_int(b) {
            Some(i) => self.get_int(i),
            None => match &self.store {
                Store::Packed(_) => None,
                Store::Hash(h) => {
                    let s = PhpStr::new(b);
                    let idx = h.find(KeyRef::Str(&s), s.hash_code())?;
                    h.entries[idx as usize].val.as_ref()
                }
            },
        }
    }

    pub fn contains_key(&self, k: KeyRef<'_>) -> bool {
        self.get(k).is_some()
    }

    /// Mutable access to an existing element.
    pub fn get_mut(&mut self, k: KeyRef<'_>) -> Option<&mut Value> {
        self.note_mut_access();
        match &mut self.store {
            Store::Packed(v) => match k {
                KeyRef::Int(i) if (i as u64) < v.len() as u64 => Some(&mut v[i as usize]),
                _ => None,
            },
            Store::Hash(h) => {
                let idx = h.find(k, bucket_hash(k))?;
                h.entries[idx as usize].val.as_mut()
            }
        }
    }

    // ------------------------------------------------------------ insert

    #[inline]
    fn convert_to_hash(&mut self, extra: usize) {
        if let Store::Packed(_) = &self.store {
            self.packed_to_hash(extra);
        }
    }

    #[inline(never)]
    fn packed_to_hash(&mut self, extra: usize) {
        if let Store::Packed(v) = &mut self.store {
            let v = std::mem::take(v);
            let mut h = HashStore::with_capacity(v.len() + extra);
            h.live = v.len() as u32;
            h.entries
                .extend(v.into_iter().enumerate().map(|(i, val)| Bucket {
                    val: Some(val),
                    h: i as u64,
                    skey: None,
                }));
            for i in 0..h.entries.len() {
                h.place(i as u32, slot_hash(i as u64, true));
            }
            self.store = Store::Hash(h);
        }
    }

    fn apply_remap(&mut self, remap: Option<Vec<u32>>) {
        if let Some(r) = remap {
            let map = |p: u32| r[(p as usize).min(r.len() - 1)];
            self.pos = map(self.pos);
            for it in &mut self.iters {
                it.1 = map(it.1);
            }
        }
    }

    /// Ensures room for one more hash entry.
    #[inline]
    fn hash_reserve_one(&mut self) {
        let Store::Hash(h) = &mut self.store else {
            return;
        };
        if h.entries.len() < h.limit() {
            return;
        }
        let live = h.live as usize;
        // Many tombstones: compact at the same size; otherwise double.
        let target = if live * 2 <= h.entries.len() {
            live + 1
        } else {
            h.limit() * 2
        };
        let remap = h.rebuild(target);
        self.apply_remap(remap);
    }

    #[inline]
    fn hash_insert_new(&mut self, k: Key, bh: u64, v: Value) -> &mut Value {
        self.hash_reserve_one();
        let Store::Hash(h) = &mut self.store else {
            unreachable!()
        };
        let idx = h.entries.len() as u32;
        let sh = slot_hash(bh, matches!(k, Key::Int(_)));
        h.entries.push(Bucket::new(k, bh, v));
        h.live += 1;
        h.place(idx, sh);
        h.entries[idx as usize].val.as_mut().unwrap()
    }

    /// Sets `k` to `v` (replacing the slot, references included), returning
    /// the previous value. For PHP assignment through references use
    /// [`ArrayData::assign`].
    pub fn insert(&mut self, k: Key, v: Value) -> Option<Value> {
        self.note_value(&v);
        if let Key::Int(i) = k {
            let ni = self.next_index();
            if let Store::Packed(vec) = &mut self.store {
                let len = vec.len() as u64;
                if (i as u64) < len {
                    return Some(std::mem::replace(&mut vec[i as usize], v));
                }
                if i as u64 == len && ni == Some(i) {
                    vec.push(v);
                    self.note_int_key(i);
                    return None;
                }
            }
            self.convert_to_hash(1);
            self.note_int_key(i);
        } else {
            self.convert_to_hash(1);
        }
        let bh = bucket_hash(k.as_ref());
        let Store::Hash(h) = &mut self.store else {
            unreachable!()
        };
        if let Some(idx) = h.find(k.as_ref(), bh) {
            return h.entries[idx as usize].val.replace(v);
        }
        self.hash_insert_new(k, bh, v);
        None
    }

    /// PHP assignment `$a[k] = v`: writes through a reference element.
    pub fn assign(&mut self, k: Key, v: Value) -> Option<Value> {
        let v = v.into_deref();
        if let Some(slot) = self.get_mut(k.as_ref()) {
            return Some(slot.assign(v));
        }
        self.insert(k, v)
    }

    /// Returns the element for `k`, inserting `null` if missing (for nested
    /// writes like `$a[k][] = v` or `$a[k] .= s`).
    pub fn entry(&mut self, k: Key) -> &mut Value {
        self.note_mut_access();
        if let Key::Int(i) = k {
            let ni = self.next_index();
            if let Store::Packed(vec) = &self.store {
                let len = vec.len() as u64;
                let append = i as u64 == len && ni == Some(i);
                if (i as u64) < len || append {
                    if append {
                        self.note_int_key(i);
                    }
                    let Store::Packed(vec) = &mut self.store else {
                        unreachable!()
                    };
                    if append {
                        vec.push(Value::Null);
                    }
                    return &mut vec[i as usize];
                }
            }
            self.convert_to_hash(1);
            self.note_int_key(i);
        } else {
            self.convert_to_hash(1);
        }
        let bh = bucket_hash(k.as_ref());
        let Store::Hash(h) = &mut self.store else {
            unreachable!()
        };
        if let Some(idx) = h.find(k.as_ref(), bh) {
            let Store::Hash(h) = &mut self.store else {
                unreachable!()
            };
            return h.entries[idx as usize].val.as_mut().unwrap();
        }
        self.hash_insert_new(k, bh, Value::Null)
    }

    /// Appends `v` at the next index (`$a[] = v`), returning the key used.
    #[inline]
    pub fn push(&mut self, v: Value) -> Result<i64, NextIndexOccupied> {
        let i = self.next_index().ok_or(NextIndexOccupied)?;
        self.note_value(&v);
        if let Store::Packed(vec) = &mut self.store
            && vec.len() as i64 == i
        {
            vec.push(v);
            self.note_int_key(i);
            return Ok(i);
        }
        self.insert(Key::Int(i), v);
        Ok(i)
    }

    /// Appends `null` and returns the new element (`$a[][k] = v`).
    pub fn push_entry(&mut self) -> Result<&mut Value, NextIndexOccupied> {
        let i = self.next_index().ok_or(NextIndexOccupied)?;
        Ok(self.entry(Key::Int(i)))
    }

    // ------------------------------------------------------------ remove

    /// Removes `k` (`unset($a[k])`), returning its value.
    pub fn remove(&mut self, k: KeyRef<'_>) -> Option<Value> {
        if let Store::Packed(vec) = &mut self.store {
            let KeyRef::Int(i) = k else {
                return None;
            };
            let len = vec.len() as u64;
            if (i as u64) >= len {
                return None;
            }
            if i as u64 == len - 1 {
                return vec.pop();
            }
            self.convert_to_hash(0);
        }
        let Store::Hash(h) = &mut self.store else {
            unreachable!()
        };
        let idx = h.find(k, bucket_hash(k))?;
        let v = h.entries[idx as usize].val.take();
        h.live -= 1;
        if h.live == 0 && self.iters.is_empty() {
            // Reset storage but keep the next index (PHP keeps counting).
            let pos_end = 0;
            self.store = Store::Packed(Vec::new());
            self.pos = pos_end;
        }
        v
    }

    /// Removes and returns the last element (`array_pop`), resetting the
    /// next index to follow the remaining largest int key as `array_pop`
    /// does, and the internal pointer.
    pub fn pop(&mut self) -> Option<Value> {
        let last = self.last_pos()?;
        let key = self.key_at(last).unwrap().to_owned();
        let v = self.remove(key.as_ref());
        self.recompute_next_index();
        self.pos = 0;
        v
    }

    /// Recomputes the next append index from the current int keys.
    pub fn recompute_next_index(&mut self) {
        let mut max: Option<i64> = None;
        for (k, _) in self.iter() {
            if let KeyRef::Int(i) = k {
                max = Some(max.map_or(i, |m| m.max(i)));
            }
        }
        match max {
            None => {
                self.nstate = 0;
                self.next_free = 0;
            }
            Some(m) => {
                self.nstate = N_HAS_INT;
                match m.checked_add(1) {
                    Some(n) => self.next_free = n,
                    None => self.nstate |= N_FULL,
                }
            }
        }
    }

    /// Removes every element (keeps nothing, resets the next index).
    pub fn clear(&mut self) {
        *self = ArrayData::new();
    }

    // ------------------------------------------------------------ positions

    /// Number of positions (live entries plus tombstones).
    #[inline]
    fn used(&self) -> u32 {
        match &self.store {
            Store::Packed(v) => v.len() as u32,
            Store::Hash(h) => h.entries.len() as u32,
        }
    }

    /// First live position at or after `p`.
    #[inline]
    pub fn valid_pos(&self, p: u32) -> Option<u32> {
        match &self.store {
            Store::Packed(v) => ((p as usize) < v.len()).then_some(p),
            Store::Hash(h) => {
                let mut p = p as usize;
                while p < h.entries.len() {
                    if h.entries[p].val.is_some() {
                        return Some(p as u32);
                    }
                    p += 1;
                }
                None
            }
        }
    }

    /// Last live position at or before `p`.
    fn valid_pos_back(&self, p: u32) -> Option<u32> {
        match &self.store {
            Store::Packed(v) => {
                if v.is_empty() {
                    None
                } else {
                    Some(p.min(v.len() as u32 - 1))
                }
            }
            Store::Hash(h) => {
                let mut p = (p as usize).min(h.entries.len().checked_sub(1)?) as isize;
                while p >= 0 {
                    if h.entries[p as usize].val.is_some() {
                        return Some(p as u32);
                    }
                    p -= 1;
                }
                None
            }
        }
    }

    pub fn first_pos(&self) -> Option<u32> {
        self.valid_pos(0)
    }

    pub fn last_pos(&self) -> Option<u32> {
        self.valid_pos_back(self.used().checked_sub(1)?)
    }

    /// The key at a live position.
    pub fn key_at(&self, p: u32) -> Option<KeyRef<'_>> {
        match &self.store {
            Store::Packed(v) => ((p as usize) < v.len()).then_some(KeyRef::Int(p as i64)),
            Store::Hash(h) => {
                let b = h.entries.get(p as usize)?;
                b.val.as_ref()?;
                Some(b.key())
            }
        }
    }

    /// The entry at a live position.
    #[inline]
    pub fn entry_at(&self, p: u32) -> Option<(KeyRef<'_>, &Value)> {
        match &self.store {
            Store::Packed(v) => v.get(p as usize).map(|v| (KeyRef::Int(p as i64), v)),
            Store::Hash(h) => {
                let b = h.entries.get(p as usize)?;
                Some((b.key(), b.val.as_ref()?))
            }
        }
    }

    /// Mutable value at a live position.
    pub fn value_at_mut(&mut self, p: u32) -> Option<&mut Value> {
        self.note_mut_access();
        match &mut self.store {
            Store::Packed(v) => v.get_mut(p as usize),
            Store::Hash(h) => h.entries.get_mut(p as usize)?.val.as_mut(),
        }
    }

    /// The next live entry at or after position `p`, with its position.
    /// Iterate with `p = found + 1`.
    #[inline]
    pub fn next_from(&self, p: u32) -> Option<(u32, KeyRef<'_>, &Value)> {
        let p = self.valid_pos(p)?;
        let (k, v) = self.entry_at(p)?;
        Some((p, k, v))
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter { a: self, p: 0 }
    }

    // ------------------------------------------------------------ internal pointer

    /// `current()`: the element at the internal pointer.
    pub fn current(&self) -> Option<&Value> {
        let p = self.valid_pos(self.pos)?;
        self.entry_at(p).map(|e| e.1)
    }

    /// `key()`: the key at the internal pointer.
    pub fn current_key(&self) -> Option<KeyRef<'_>> {
        let p = self.valid_pos(self.pos)?;
        self.key_at(p)
    }

    /// `next()`: advances the internal pointer.
    pub fn advance(&mut self) {
        match self.valid_pos(self.pos) {
            Some(p) => self.pos = p + 1,
            None => self.pos = self.used(),
        }
    }

    /// `prev()`: moves the internal pointer back; past the start it becomes
    /// invalid.
    pub fn retreat(&mut self) {
        match self.valid_pos(self.pos) {
            Some(p) if p > 0 => match self.valid_pos_back(p - 1) {
                Some(q) => self.pos = q,
                None => self.pos = self.used(),
            },
            _ => self.pos = self.used(),
        }
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        self.pos = self.first_pos().unwrap_or(0);
    }

    /// `end()`.
    pub fn end(&mut self) {
        self.pos = self.last_pos().unwrap_or(0);
    }

    /// Raw internal pointer position.
    pub fn pos(&self) -> u32 {
        self.pos
    }

    pub fn set_pos(&mut self, p: u32) {
        self.pos = p;
    }

    // ------------------------------------------------------------ iterators

    fn iter_slot(&mut self, id: u32) -> usize {
        match self.iters.iter().position(|it| it.0 == id) {
            Some(i) => i,
            None => {
                self.iters.push((id, 0));
                self.iters.len() - 1
            }
        }
    }

    /// Position of a registered iterator (registering it at 0 if this
    /// array has not seen it).
    pub fn iter_pos(&mut self, id: u32) -> u32 {
        let i = self.iter_slot(id);
        self.iters[i].1
    }

    pub fn set_iter_pos(&mut self, id: u32, p: u32) {
        let i = self.iter_slot(id);
        self.iters[i].1 = p;
    }

    pub fn iter_release(&mut self, id: u32) {
        self.iters.retain(|it| it.0 != id);
    }

    /// The step of a by-reference `foreach`: finds the next live entry from
    /// the iterator's position, turns the element into a reference (as
    /// `foreach ($a as &$v)` does), advances, and returns the key and the
    /// reference.
    pub fn iter_next_ref(&mut self, id: u32) -> Option<(Key, PhpRef)> {
        let p0 = self.iter_pos(id);
        let Some(p) = self.valid_pos(p0) else {
            let end = self.used();
            self.set_iter_pos(id, end);
            return None;
        };
        self.set_iter_pos(id, p + 1);
        let key = self.key_at(p)?.to_owned();
        let r = self.value_at_mut(p)?.make_ref();
        Some((key, r))
    }

    /// Visits every value (cycle collector).
    pub(crate) fn for_each_value(&self, mut f: impl FnMut(&Value)) {
        match &self.store {
            Store::Packed(v) => v.iter().for_each(f),
            Store::Hash(h) => {
                for b in &h.entries {
                    if let Some(v) = &b.val {
                        f(v)
                    }
                }
            }
        }
    }
}

/// Iterator over `(key, value)` in order.
pub struct Iter<'a> {
    a: &'a ArrayData,
    p: u32,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (KeyRef<'a>, &'a Value);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let (p, k, v) = self.a.next_from(self.p)?;
        self.p = p + 1;
        Some((k, v))
    }
}

// ---------------------------------------------------------------- handle

/// A PHP array value: a copy-on-write handle to [`ArrayData`]. 8 bytes.
#[derive(Default)]
pub struct PhpArray(pub(crate) Option<Rc<ArrayData>>);

impl PhpArray {
    /// The empty array (no allocation).
    #[inline]
    pub const fn new() -> PhpArray {
        PhpArray(None)
    }

    pub fn with_capacity(n: usize) -> PhpArray {
        if n == 0 {
            PhpArray(None)
        } else {
            PhpArray(Some(Rc::new(ArrayData::with_capacity(n))))
        }
    }

    pub fn from_data(d: ArrayData) -> PhpArray {
        PhpArray(Some(Rc::new(d)))
    }

    /// A list from values (keys 0..n).
    pub fn from_values(vals: impl IntoIterator<Item = Value>) -> PhpArray {
        let mut a = PhpArray::new();
        for v in vals {
            a.push(v).expect("list too large");
        }
        a
    }

    /// An array from key/value pairs (later keys overwrite earlier ones).
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Key, Value)>) -> PhpArray {
        let mut a = PhpArray::new();
        for (k, v) in pairs {
            a.insert(k, v);
        }
        a
    }

    /// Read access to the data (an empty static for the empty array).
    #[inline]
    pub fn data(&self) -> &ArrayData {
        match &self.0 {
            Some(rc) => rc,
            None => empty_data(),
        }
    }

    /// Write access, separating a shared copy first (copy on write).
    #[inline(always)]
    pub fn make_mut(&mut self) -> &mut ArrayData {
        let rc = self.0.get_or_insert_with(|| Rc::new(ArrayData::new()));
        // A unique array with a weak pointer (its GC root entry) is moved
        // to a new allocation by `make_mut`, leaving the root entry dead:
        // clear the "buffered" flag so it can be recorded again.
        let moved = Rc::strong_count(rc) == 1 && Rc::weak_count(rc) > 0;
        let d = Rc::make_mut(rc);
        if moved {
            d.flags.set(d.flags.get() & !A_GC_BUFFERED);
        }
        d
    }

    /// Number of handles sharing this array's data (0 for the empty array).
    pub fn refcount(&self) -> usize {
        self.0.as_ref().map_or(0, Rc::strong_count)
    }

    /// Whether two handles share the same data.
    pub fn ptr_eq(a: &PhpArray, b: &PhpArray) -> bool {
        match (&a.0, &b.0) {
            (Some(x), Some(y)) => Rc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |d| d.len())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get(&self, k: KeyRef<'_>) -> Option<&Value> {
        self.0.as_ref()?.get(k)
    }

    #[inline]
    pub fn get_int(&self, i: i64) -> Option<&Value> {
        self.0.as_ref()?.get_int(i)
    }

    #[inline]
    pub fn get_str(&self, s: &PhpStr) -> Option<&Value> {
        self.0.as_ref()?.get_str(s)
    }

    pub fn get_bytes(&self, b: &[u8]) -> Option<&Value> {
        self.0.as_ref()?.get_bytes(b)
    }

    #[inline]
    pub fn insert(&mut self, k: Key, v: Value) -> Option<Value> {
        self.make_mut().insert(k, v)
    }

    pub fn assign(&mut self, k: Key, v: Value) -> Option<Value> {
        self.make_mut().assign(k, v)
    }

    #[inline]
    pub fn push(&mut self, v: Value) -> Result<i64, NextIndexOccupied> {
        self.make_mut().push(v)
    }

    pub fn entry(&mut self, k: Key) -> &mut Value {
        self.make_mut().entry(k)
    }

    pub fn get_mut(&mut self, k: KeyRef<'_>) -> Option<&mut Value> {
        // Avoid separating when the key is absent.
        self.0.as_ref()?.get(k)?;
        self.make_mut().get_mut(k)
    }

    pub fn remove(&mut self, k: KeyRef<'_>) -> Option<Value> {
        self.0.as_ref()?.get(k)?;
        self.make_mut().remove(k)
    }

    pub fn iter(&self) -> Iter<'_> {
        self.data().iter()
    }

    /// Allocates a new iterator id for [`ArrayData::iter_next_ref`]. Ids are
    /// unique per thread.
    pub fn new_iter_id() -> u32 {
        thread_local! { static NEXT: Cell<u32> = const { Cell::new(1) } }
        NEXT.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1).max(1));
            id
        })
    }

    pub(crate) fn rc(&self) -> Option<&Rc<ArrayData>> {
        self.0.as_ref()
    }
}

fn empty_data() -> &'static ArrayData {
    // ArrayData holds Cells, so it isn't Sync; keep one leaked instance per
    // thread. It is never mutated (writes go through make_mut).
    thread_local! {
        static E: &'static ArrayData = Box::leak(Box::new(ArrayData::new()));
    }
    E.with(|e| *e)
}

impl Clone for PhpArray {
    #[inline]
    fn clone(&self) -> PhpArray {
        PhpArray(self.0.clone())
    }
}

impl Drop for PhpArray {
    #[inline]
    fn drop(&mut self) {
        if let Some(rc) = &self.0
            && Rc::strong_count(rc) > 1
            && rc.may_cycle()
        {
            gc::possible_root_array(rc);
        }
    }
}

impl fmt::Debug for PhpArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl fmt::Debug for ArrayData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

pub(crate) const ARRAY_GC_BUFFERED: u8 = A_GC_BUFFERED;
