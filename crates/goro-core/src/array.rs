use crate::string::PhpString;
use crate::value::{Value, memory_alloc_reporting, memory_charge, memory_free};

/// Array key - either integer or string
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    Int(i64),
    String(PhpString),
}

impl PartialOrd for ArrayKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ArrayKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (ArrayKey::Int(a), ArrayKey::Int(b)) => a.cmp(b),
            (ArrayKey::String(a), ArrayKey::String(b)) => a.as_bytes().cmp(b.as_bytes()),
            (ArrayKey::Int(_), ArrayKey::String(_)) => std::cmp::Ordering::Less,
            (ArrayKey::String(_), ArrayKey::Int(_)) => std::cmp::Ordering::Greater,
        }
    }
}

impl std::fmt::Display for ArrayKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArrayKey::Int(n) => write!(f, "{}", n),
            ArrayKey::String(s) => write!(f, "{}", s.to_string_lossy()),
        }
    }
}

/// PHP ordered hash map (HashTable equivalent)
///
/// Preserves insertion order. Supports both integer and string keys.
/// Lookups are O(1): "packed" arrays (keys 0..n in order) are indexed
/// directly; other arrays above a small size get an open-addressing hash
/// index of positions into `entries`.
#[derive(Debug)]
pub struct PhpArray {
    /// Current storage mode. `entries[..head]` are dead slots left by
    /// removals at the front (see `head`); positions handed out by the
    /// public API count live entries only. (Empty while the storage is
    /// `shared`: see ents().)
    entries: Vec<(ArrayKey, Value)>,
    /// Number of dead slots at the start of `entries` (always 0 while the
    /// array is packed), so removing the first entry or inserting before it
    /// is O(1) amortized.
    head: u32,
    /// Next integer key to use for append ($arr[] = val)
    next_int_key: i64,
    /// Internal array pointer position (for current/next/prev/reset/end)
    pub pointer: usize,
    /// True while every entry i has key Int(i)
    packed: bool,
    /// zend_hash_real_init_mixed(): PHP holds the array as a hash even
    /// while its keys would fit a packed one (see `looks_packed`).
    mixed: bool,
    /// Hash index (slot -> entry position + 1, 0 = empty); empty when unused
    index: Vec<u32>,
    /// In the cycle collector's root buffer (Zend's GC_INFO); see gc.rs.
    pub(crate) gc_buffered: crate::object::GcSlot,
    /// memory_limit accounting: the nTableSize of the Zend HashTable this
    /// array stands for (0 = no data block allocated yet) and whether that
    /// block is packed. See `zend_size`.
    zcap: u32,
    zpacked: bool,
    /// Not accounted (the global symbol table, preallocated by the engine).
    untracked: bool,
    /// The zend_array this array stands for (0 = not assigned yet): goro
    /// copies arrays eagerly where PHP shares one refcounted zend_array
    /// until a write separates it, so a copy keeps its source's identity
    /// until either is modified. Recursion guards (var_dump(), print_r(),
    /// ...) key on it. See `identity`.
    ident: std::cell::Cell<u64>,
    /// By-reference foreach iterators over this array (EG(ht_iterators)
    /// entries pointing at it): each holds the insertion-order position of
    /// the next element to fetch, kept in step with removals and
    /// insertions the way zend_hash keeps its iterator positions. Dead
    /// iterators (the loop is gone) are pruned lazily. (Boxed: one word in
    /// every array, most of which never have one.)
    #[allow(clippy::box_collection)]
    iters: Option<Box<Vec<std::rc::Weak<std::cell::Cell<usize>>>>>,
    /// The storage (entries and hash index) this array shares with copies
    /// of it (see cow_clone()): PHP's refcounted zend_array, separated by
    /// the first write to any of them. None: `entries` / `index` are this
    /// array's own.
    shared: Option<std::rc::Rc<Store>>,
}

/// The storage of an array shared by copies of it, copy-on-write. Only
/// storage holding no references is shared: an element that is a
/// reference is written through without the array itself being written
/// (and zend_array_dup() turns the references only the array holds into
/// values when a copy is made).
#[derive(Debug)]
pub(crate) struct Store {
    entries: Vec<(ArrayKey, Value)>,
    index: Vec<u32>,
    /// The memory_limit accounting of the table (zend_size() of the array
    /// that made it shared), released with the storage.
    charged: usize,
}

impl Store {
    /// The values the storage holds (for the cycle collector).
    pub(crate) fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if self.charged > 0 {
            memory_free(self.charged);
        }
        for (_, v) in self.entries.iter() {
            crate::gc::on_child_released(v);
        }
    }
}

/// Size of a zend_array (56-byte header) whose data block holds `cap`
/// slots: 16-byte zvals plus a 2-slot hash for a packed array, 32-byte
/// Buckets plus a 2*cap uint32 hash otherwise.
#[inline]
fn zend_ht_data_size(cap: usize, packed: bool) -> usize {
    if packed { cap * 16 + 8 } else { cap * 40 }
}

/// Arrays smaller than this are searched linearly
const INDEX_THRESHOLD: usize = 8;

#[inline]
fn hash_int(n: i64) -> u64 {
    let mut x = n as u64;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x
}

/// The hash of a string key (cached by PhpString::hash_value()): eight
/// bytes at a time, then mixed so that the low bits the index uses depend
/// on all of them.
#[inline]
pub(crate) fn hash_bytes(b: &[u8]) -> u64 {
    const K: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut h: u64 = (b.len() as u64).wrapping_mul(K);
    let (chunks, r) = b.as_chunks::<8>();
    for c in chunks {
        let w = u64::from_le_bytes(*c);
        h = (h ^ w).wrapping_mul(K).rotate_left(29);
    }
    if !r.is_empty() {
        let mut buf = [0u8; 8];
        buf[..r.len()].copy_from_slice(r);
        h = (h ^ u64::from_le_bytes(buf))
            .wrapping_mul(K)
            .rotate_left(29);
    }
    h ^= h >> 32;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 29)
}

#[inline]
fn hash_key(k: &ArrayKey) -> u64 {
    match k {
        ArrayKey::Int(n) => hash_int(*n),
        ArrayKey::String(s) => s.hash_value(),
    }
}

thread_local! {
    static NEXT_IDENT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Whether an array its holder and one fetched copy (`base` counts) hold
/// is also held elsewhere: written, it is separated (copy-on-write). The
/// running by-reference foreach loops over it don't count (each holds it
/// twice; PHP's holds the reference instead, FE_RESET_RW).
#[inline]
pub fn shared_beyond(rc: &std::rc::Rc<std::cell::RefCell<PhpArray>>, base: usize) -> bool {
    let n = std::rc::Rc::strong_count(rc);
    n > base
        && match rc.try_borrow() {
            Ok(a) => n > base + 2 * a.foreach_ref_pins(),
            Err(_) => true,
        }
}

/// SEPARATE_ARRAY() for a slot about to be bound to a reference: an array
/// still shared with another holder (properties and elements share an
/// array assigned from one another until either is written) gets its own
/// copy, so what is done through the reference (sort(), next(), a
/// by-reference parameter ...) does not show through the other holder.
pub fn separate_shared(v: &mut Value) {
    if let Value::Array(a) = v {
        if std::rc::Rc::strong_count(a) > 1 {
            let copy = match a.try_borrow() {
                Ok(inner) => inner.clone(),
                Err(_) => return,
            };
            *v = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(copy)));
        }
    }
}

/// A copy of the array `a` for another holder (a by-value parameter, an
/// assignment): it shares the storage until either is written when it can
/// (see PhpArray::cow_clone()).
pub fn cow_copy(a: &std::rc::Rc<std::cell::RefCell<PhpArray>>) -> PhpArray {
    match a.try_borrow_mut() {
        Ok(mut b) => b.cow_clone(),
        Err(_) => a.borrow().clone(),
    }
}

impl Clone for PhpArray {
    fn clone(&self) -> Self {
        // (a copy of shared storage shares it too)
        if self.shared.is_some() {
            return self.share_header();
        }
        // zend_array_dup(): a table of the same size.
        let zcap = if self.is_empty() { 0 } else { self.zcap.max(8) };
        let zpacked = if self.untracked {
            self.packed
        } else {
            self.zpacked
        };
        if zcap > 0 {
            memory_charge(
                56 + crate::value::zend_mm_size(zend_ht_data_size(zcap as usize, zpacked)),
            );
        }
        // Like zend_array_dup(): references held only by this array (rc=1)
        // are copied as plain values, unless they point back at this array.
        // Closure values (arrays holding captured variables) are copied as-is.
        let self_ptr = self as *const PhpArray;
        let is_closure = matches!(self.live().first(), Some((_, Value::String(s)))
            if s.as_bytes().starts_with(b"__closure_") || s.as_bytes().starts_with(b"__arrow_"));
        let entries = if is_closure {
            self.live().to_vec()
        } else {
            self.live()
                .iter()
                .map(|(k, v)| {
                    let v2 = match v {
                        Value::Reference(r) if std::rc::Rc::strong_count(r) == 1 => {
                            let inner = r.borrow();
                            match &*inner {
                                Value::Array(a)
                                    if std::ptr::eq(a.as_ptr() as *const PhpArray, self_ptr) =>
                                {
                                    v.clone()
                                }
                                other => other.clone(),
                            }
                        }
                        // Nested arrays are shared with the copy (refcounted
                        // copy-on-write): a write fetch of an element array
                        // held elsewhere too separates it (fetch_dim_from_array).
                        _ => v.clone(),
                    };
                    (k.clone(), v2)
                })
                .collect()
        };
        let mut copy = Self {
            entries,
            head: 0,
            next_int_key: self.next_int_key,
            pointer: self.pointer,
            packed: self.packed,
            mixed: self.mixed,
            index: Vec::new(),
            gc_buffered: crate::object::GcSlot::default(),
            zcap,
            zpacked,
            untracked: false,
            ident: std::cell::Cell::new(if is_closure { 0 } else { self.identity() }),
            iters: None,
            shared: None,
        };
        if self.head == 0 {
            copy.index = self.index.clone();
        } else {
            copy.rebuild_index();
        }
        copy
    }
}

impl PhpArray {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            head: 0,
            next_int_key: i64::MIN,
            pointer: 0,
            packed: true,
            mixed: false,
            index: Vec::new(),
            gc_buffered: crate::object::GcSlot::default(),
            zcap: 0,
            zpacked: true,
            untracked: false,
            ident: std::cell::Cell::new(0),
            iters: None,
            shared: None,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            head: 0,
            next_int_key: i64::MIN,
            pointer: 0,
            packed: true,
            mixed: false,
            index: Vec::new(),
            gc_buffered: crate::object::GcSlot::default(),
            zcap: 0,
            zpacked: true,
            untracked: false,
            ident: std::cell::Cell::new(0),
            iters: None,
            shared: None,
        }
    }

    /// The identity of the zend_array this array stands for: shared by
    /// unmodified copies (see `ident`), never 0.
    pub fn identity(&self) -> u64 {
        let v = self.ident.get();
        if v != 0 {
            return v;
        }
        let v = NEXT_IDENT.with(|n| {
            let v = n.get() + 1;
            n.set(v);
            v
        });
        self.ident.set(v);
        v
    }

    /// Give this array the identity `id` (a copy fixed up after cloning
    /// that still stands for the same zend_array).
    pub(crate) fn set_identity(&mut self, id: u64) {
        *self.ident.get_mut() = id;
    }

    /// A write separates this array from its copies.
    #[inline(always)]
    fn separate(&mut self) {
        *self.ident.get_mut() = 0;
        if self.shared.is_some() {
            self.unshare();
        }
    }

    /* ----- copy-on-write storage ----- */

    /// This array's entries (its own, or the storage it shares).
    #[inline(always)]
    /// The other copies sharing this array's storage (see cow_clone()):
    /// each holds the zend_array PHP would share once more.
    pub fn shared_copies(&self) -> usize {
        self.shared
            .as_ref()
            .map_or(0, |s| std::rc::Rc::strong_count(s) - 1)
    }

    fn ents(&self) -> &Vec<(ArrayKey, Value)> {
        match &self.shared {
            Some(s) => &s.entries,
            None => &self.entries,
        }
    }

    /// This array's hash index (its own, or the shared storage's).
    #[inline(always)]
    fn idx(&self) -> &[u32] {
        match &self.shared {
            Some(s) => &s.index,
            None => &self.index,
        }
    }

    /// The storage this array shares with copies of it, if any.
    pub(crate) fn shared_store(&self) -> Option<&std::rc::Rc<Store>> {
        self.shared.as_ref()
    }

    /// A copy of this array (`$b = $a`) that shares its storage until
    /// either is written (PHP's refcounted zend_array): the storage moves
    /// into a Store both hold. Storage holding references is copied at
    /// once, as clone() does (see Store).
    pub fn cow_clone(&mut self) -> PhpArray {
        if self.shared.is_none() {
            // (nor the global symbol table: the variables' cells are its own)
            if self.is_empty()
                || self.untracked
                || self
                    .live()
                    .iter()
                    .any(|(_, v)| matches!(v, Value::Reference(_)))
            {
                return self.clone();
            }
            // (the table's accounting goes with it)
            let charged = self.zend_size();
            self.shared = Some(std::rc::Rc::new(Store {
                entries: std::mem::take(&mut self.entries),
                index: std::mem::take(&mut self.index),
                charged,
            }));
        }
        self.share_header()
    }

    /// Make this array's storage its own (before an element of it is
    /// inspected for writing: an element array is shared by the copies
    /// sharing the storage, whatever its own count says).
    #[inline]
    pub fn make_unique(&mut self) {
        if self.shared.is_some() {
            self.separate();
        }
    }

    /// A copy of this array, which shares its storage.
    fn share_header(&self) -> PhpArray {
        // zend_array_dup(): a table of the same size (accounted once the
        // copy is separated).
        let zcap = if self.is_empty() { 0 } else { self.zcap.max(8) };
        let zpacked = if self.untracked {
            self.packed
        } else {
            self.zpacked
        };
        PhpArray {
            entries: Vec::new(),
            head: self.head,
            next_int_key: self.next_int_key,
            pointer: self.pointer,
            packed: self.packed,
            mixed: self.mixed,
            index: Vec::new(),
            gc_buffered: crate::object::GcSlot::default(),
            zcap,
            zpacked,
            untracked: false,
            ident: std::cell::Cell::new(self.identity()),
            iters: None,
            shared: self.shared.clone(),
        }
    }

    /// The first write to an array sharing its storage: the storage becomes
    /// its own again when no other array holds it any more, else it gets
    /// its own copy (zend_array_dup(); the storage holds no references).
    #[cold]
    #[inline(never)]
    fn unshare(&mut self) {
        let Some(rc) = self.shared.take() else {
            return;
        };
        let charged = match std::rc::Rc::try_unwrap(rc) {
            Ok(mut st) => {
                self.entries = std::mem::take(&mut st.entries);
                self.index = std::mem::take(&mut st.index);
                std::mem::replace(&mut st.charged, 0)
            }
            Err(rc) => {
                self.entries = rc.entries.clone();
                self.index = rc.index.clone();
                0
            }
        };
        // (this array's table is accounted to it again)
        let want = self.zend_size();
        if want > charged {
            memory_charge(want - charged);
        } else if charged > want {
            memory_free(charged - want);
        }
    }

    /* ----- foreach-by-reference iterators ----- */

    /// Track a by-reference foreach iterator over this array.
    pub fn iterator_attach(&mut self, it: &std::rc::Rc<std::cell::Cell<usize>>) {
        let list = self.iters.get_or_insert_with(Default::default);
        list.retain(|w| w.strong_count() > 0);
        if !list
            .iter()
            .any(|w| std::ptr::eq(w.as_ptr(), std::rc::Rc::as_ptr(it)))
        {
            list.push(std::rc::Rc::downgrade(it));
        }
    }

    /// The running by-reference foreach loops over this array: each holds
    /// it twice (its state and its iterator temporary), which a
    /// reference's holder does not count as sharing it.
    pub fn foreach_ref_pins(&self) -> usize {
        self.iters
            .as_ref()
            .map_or(0, |l| l.iter().filter(|w| w.strong_count() > 0).count())
    }

    /// Whether `it` is tracked by this array.
    pub fn iterator_attached(&self, it: &std::rc::Rc<std::cell::Cell<usize>>) -> bool {
        self.iters.as_ref().is_some_and(|l| {
            l.iter()
                .any(|w| std::ptr::eq(w.as_ptr(), std::rc::Rc::as_ptr(it)))
        })
    }

    /// Hand this array's iterators over to `other` (the array that
    /// replaces it, e.g. php_splice()'s output hash).
    pub fn iterators_move_to(&mut self, other: &mut PhpArray) {
        if let Some(list) = self.iters.take() {
            other.iters = Some(list);
        }
    }

    /// Iterator positions after php_splice(): positions inside the
    /// removed range move after the replacement, later ones shift.
    pub fn iterators_splice(&mut self, offset: usize, removed: usize, inserted: usize) {
        self.iters_adjust(|p| {
            if p < offset {
                p
            } else if p < offset + removed {
                offset + inserted
            } else {
                p - removed + inserted
            }
        });
    }

    #[inline]
    fn iters_adjust(&mut self, f: impl Fn(usize) -> usize) {
        if let Some(list) = &mut self.iters {
            Self::iters_adjust_cold(list, f);
        }
    }

    #[cold]
    fn iters_adjust_cold(
        list: &mut Vec<std::rc::Weak<std::cell::Cell<usize>>>,
        f: impl Fn(usize) -> usize,
    ) {
        list.retain(|w| match w.upgrade() {
            Some(c) => {
                c.set(f(c.get()));
                true
            }
            None => false,
        });
    }

    /// The element at live position `pos` was removed.
    #[inline]
    fn iters_removed_at(&mut self, pos: usize) {
        self.iters_adjust(|p| if p > pos { p - 1 } else { p });
    }

    /// An element was inserted at live position `pos` (not appended).
    #[inline]
    fn iters_inserted_at(&mut self, pos: usize) {
        self.iters_adjust(|p| if p >= pos { p + 1 } else { p });
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ents().len() - self.head as usize
    }

    /// The live entries.
    #[inline]
    fn live(&self) -> &[(ArrayKey, Value)] {
        &self.ents()[self.head as usize..]
    }

    #[inline]
    fn live_mut(&mut self) -> &mut [(ArrayKey, Value)] {
        debug_assert!(self.shared.is_none());
        let h = self.head as usize;
        &mut self.entries[h..]
    }

    /// Drop the dead front slots (positions move: the index is rebuilt).
    fn compact(&mut self) {
        debug_assert!(self.shared.is_none());
        if self.head > 0 {
            self.entries.drain(..self.head as usize);
            self.head = 0;
            self.rebuild_index();
        }
    }

    /* ----- memory_limit accounting ----- */

    /// Bytes this array accounts for (header plus data block).
    fn zend_size(&self) -> usize {
        if self.untracked || self.zcap == 0 {
            0
        } else {
            56 + crate::value::zend_mm_size(zend_ht_data_size(self.zcap as usize, self.zpacked))
        }
    }

    /// Exclude this array from memory accounting (the global symbol
    /// table, which the engine preallocates).
    pub fn set_untracked(&mut self) {
        self.make_unique();
        let size = self.zend_size();
        if size > 0 {
            memory_free(size);
        }
        self.untracked = true;
    }

    /// Room for one more element; grows the table (zend_hash_do_resize /
    /// packed grow) when full. False when memory_limit forbids the growth.
    #[inline]
    fn reserve_one(&mut self) -> bool {
        self.len() < self.zcap as usize || self.grow_table()
    }

    #[cold]
    fn grow_table(&mut self) -> bool {
        let new_cap = if self.zcap == 0 {
            8
        } else {
            self.zcap.saturating_mul(2)
        };
        if !self.untracked {
            let old = self.zend_size();
            let data = zend_ht_data_size(new_cap as usize, self.packed);
            let new = 56 + crate::value::zend_mm_size(data);
            if new > old && !memory_alloc_reporting(new - old, data) {
                return false;
            }
            if new < old {
                memory_free(old - new);
            }
        }
        self.zcap = new_cap;
        self.zpacked = self.packed;
        true
    }

    /// The table was converted between packed and hash.
    #[inline]
    fn sync_packed(&mut self) {
        if self.zpacked != self.packed {
            self.resize_accounting();
        }
    }

    #[cold]
    fn resize_accounting(&mut self) {
        let old = self.zend_size();
        self.zpacked = self.packed;
        let new = self.zend_size();
        if new > old {
            memory_charge(new - old);
        } else if old > new {
            memory_free(old - new);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /* ----- index maintenance ----- */

    fn index_insert_slot(index: &mut [u32], h: u64, pos: usize) {
        let mask = index.len() - 1;
        let mut i = (h as usize) & mask;
        loop {
            if index[i] == 0 {
                index[i] = pos as u32 + 1;
                return;
            }
            i = (i + 1) & mask;
        }
    }

    fn rebuild_index(&mut self) {
        debug_assert!(self.shared.is_none());
        if self.packed
            || self.len() < INDEX_THRESHOLD
            || self.entries.len() >= u32::MAX as usize / 4
        {
            self.index = Vec::new();
            return;
        }
        let cap = (self.len() * 2).next_power_of_two().max(16);
        let mut index = vec![0u32; cap];
        let head = self.head as usize;
        for (pos, (k, _)) in self.entries.iter().enumerate().skip(head) {
            Self::index_insert_slot(&mut index, hash_key(k), pos);
        }
        self.index = index;
    }

    /// Called after pushing a new entry at the end of `entries`
    fn after_push(&mut self) {
        debug_assert!(self.shared.is_none());
        let pos = self.entries.len() - 1;
        if self.packed {
            if matches!(self.entries[pos].0, ArrayKey::Int(n) if n == pos as i64) {
                return;
            }
            self.packed = false;
            self.rebuild_index();
            return;
        }
        if self.index.is_empty() {
            if self.len() >= INDEX_THRESHOLD {
                self.rebuild_index();
            }
            return;
        }
        if self.len() * 2 > self.index.len() {
            self.rebuild_index();
            return;
        }
        let h = hash_key(&self.entries[pos].0);
        Self::index_insert_slot(&mut self.index, h, pos);
    }

    /// Integer keys 0..n in order (no string keys).
    pub fn is_packed(&self) -> bool {
        self.packed
    }

    /// Whether PHP would hold this array as a packed array (HT_IS_PACKED),
    /// as far as can be told from its contents: integer keys in ascending
    /// order (holes allowed) that fit the packed table PHP would have
    /// allocated for that many elements. Only reported by debug_zval_dump().
    pub fn looks_packed(&self) -> bool {
        if self.mixed {
            return false;
        }
        if self.packed {
            return true;
        }
        let mut prev = -1i64;
        for (k, _) in self.live() {
            match k {
                ArrayKey::Int(n) if *n > prev => prev = *n,
                _ => return false,
            }
        }
        let table = self.len().next_power_of_two().max(8) as i64;
        prev < table
    }

    /// zend_hash_real_init_mixed(): the array is a hash table from now on
    /// (unserialize() builds its arrays so).
    pub fn init_mixed(&mut self) {
        self.mixed = true;
    }

    fn recompute_packed(&mut self) {
        self.packed = self
            .live()
            .iter()
            .enumerate()
            .all(|(i, (k, _))| matches!(k, ArrayKey::Int(n) if *n == i as i64));
    }

    #[inline]
    fn find_int(&self, key: i64) -> Option<usize> {
        let entries = self.ents();
        if self.packed {
            if key >= 0 && (key as usize) < entries.len() {
                return Some(key as usize);
            }
            return None;
        }
        let index = self.idx();
        if !index.is_empty() {
            let mask = index.len() - 1;
            let mut i = (hash_int(key) as usize) & mask;
            loop {
                let slot = index[i];
                if slot == 0 {
                    return None;
                }
                let pos = (slot - 1) as usize;
                if let ArrayKey::Int(n) = &entries[pos].0 {
                    if *n == key {
                        return Some(pos);
                    }
                }
                i = (i + 1) & mask;
            }
        }
        let head = self.head as usize;
        self.live()
            .iter()
            .position(|(k, _)| matches!(k, ArrayKey::Int(n) if *n == key))
            .map(|p| p + head)
    }

    #[inline]
    fn find_str(&self, key: &[u8]) -> Option<usize> {
        if self.packed {
            return None;
        }
        let index = self.idx();
        if !index.is_empty() {
            let entries = self.ents();
            let mask = index.len() - 1;
            let mut i = (hash_bytes(key) as usize) & mask;
            loop {
                let slot = index[i];
                if slot == 0 {
                    return None;
                }
                let pos = (slot - 1) as usize;
                if let ArrayKey::String(s) = &entries[pos].0 {
                    if crate::string::bytes_eq(s.as_bytes(), key) {
                        return Some(pos);
                    }
                }
                i = (i + 1) & mask;
            }
        }
        let head = self.head as usize;
        self.live()
            .iter()
            .position(|(k, _)| matches!(k, ArrayKey::String(s) if crate::string::bytes_eq(s.as_bytes(), key)))
            .map(|p| p + head)
    }

    /// find_str() of a key held as a string (its hash cached with it, and
    /// the same string as the entry's found without comparing bytes).
    #[inline]
    fn find_pstr(&self, key: &PhpString) -> Option<usize> {
        if self.packed {
            return None;
        }
        let index = self.idx();
        let entries = self.ents();
        if !index.is_empty() {
            let mask = index.len() - 1;
            let mut i = (key.hash_value() as usize) & mask;
            loop {
                let slot = index[i];
                if slot == 0 {
                    return None;
                }
                let pos = (slot - 1) as usize;
                if let ArrayKey::String(s) = &entries[pos].0 {
                    if s.ptr_eq(key) || crate::string::bytes_eq(s.as_bytes(), key.as_bytes()) {
                        return Some(pos);
                    }
                }
                i = (i + 1) & mask;
            }
        }
        let head = self.head as usize;
        let kb = key.as_bytes();
        self.live()
            .iter()
            .position(
                |(k, _)| matches!(k, ArrayKey::String(s) if s.ptr_eq(key) || crate::string::bytes_eq(s.as_bytes(), kb)),
            )
            .map(|p| p + head)
    }

    #[inline]
    fn find(&self, key: &ArrayKey) -> Option<usize> {
        match key {
            ArrayKey::Int(n) => self.find_int(*n),
            ArrayKey::String(s) => self.find_pstr(s),
        }
    }

    /// Get a value by integer key
    pub fn get_int(&self, key: i64) -> Option<&Value> {
        self.find_int(key).map(|p| &self.ents()[p].1)
    }

    /// Get a value by string key
    pub fn get_str(&self, key: &[u8]) -> Option<&Value> {
        self.find_str(key).map(|p| &self.ents()[p].1)
    }

    /// Get a value by a variable name used as a key (a numeric name is an
    /// integer key, as symtab::gkey() makes it), without building the key.
    pub fn get_name(&self, name: &[u8]) -> Option<&Value> {
        match crate::fastpath::canonical_int_key(name) {
            Some(n) => self.get_int(n),
            None => self.get_str(name),
        }
    }

    /// get_mut() by a variable name (see get_name()).
    pub fn get_name_mut(&mut self, name: &[u8]) -> Option<&mut Value> {
        self.separate();
        let p = match crate::fastpath::canonical_int_key(name) {
            Some(n) => self.find_int(n),
            None => self.find_str(name),
        }?;
        Some(&mut self.entries[p].1)
    }

    /// Get a value by ArrayKey
    pub fn get(&self, key: &ArrayKey) -> Option<&Value> {
        // (a string key's hash is cached with it: find_pstr())
        self.find(key).map(|p| &self.ents()[p].1)
    }

    /// Get a mutable reference by ArrayKey
    pub fn get_mut(&mut self, key: &ArrayKey) -> Option<&mut Value> {
        self.separate();
        match self.find(key) {
            Some(p) => Some(&mut self.entries[p].1),
            None => None,
        }
    }

    /// Set a value with a specific key
    pub fn set(&mut self, key: ArrayKey, value: Value) {
        self.separate();
        // Check if key already exists
        if let Some(p) = self.find(&key) {
            let old = std::mem::replace(&mut self.entries[p].1, value);
            crate::gc::on_value_replaced(&old, &self.entries[p].1);
            return;
        }
        // Check memory limit before adding
        if !self.reserve_one() {
            return; // Memory limit exceeded
        }
        // Track next_int_key (don't overflow: if n is i64::MAX, keep next_int_key at i64::MAX)
        if let ArrayKey::Int(n) = &key
            && *n >= self.next_int_key
        {
            self.next_int_key = n.saturating_add(1);
        }
        self.entries.push((key, value));
        self.after_push();
        self.sync_packed();
    }

    /// The key the next append would use (nNextFreeElement).
    pub fn next_free_key(&self) -> i64 {
        self.next_int_key
    }

    /// Set the key the next append uses (a sort keeping the keys keeps
    /// nNextFreeElement).
    pub fn set_next_free_key(&mut self, n: i64) {
        self.next_int_key = n;
    }

    /// Append a value with the next integer key ($arr[] = value).
    /// Silently ignores if memory limit is exceeded. Ignores key conflicts.
    pub fn push(&mut self, value: Value) {
        self.separate();
        // nNextFreeElement == ZEND_LONG_MIN means no integer key used yet
        let key = if self.next_int_key == i64::MIN {
            0
        } else {
            self.next_int_key
        };
        if !self.reserve_one() {
            return; // Memory limit exceeded
        }
        self.next_int_key = key.saturating_add(1);
        self.entries.push((ArrayKey::Int(key), value));
        self.after_push();
        self.sync_packed();
    }

    /// Append a value with the next integer key ($arr[] = value).
    /// Returns Err("Cannot add element...") if the key is already occupied (PHP_INT_MAX overflow).
    pub fn try_push(&mut self, value: Value) -> Result<(), &'static str> {
        self.separate();
        // Check if next_int_key is already occupied (happens after PHP_INT_MAX was used)
        let key = if self.next_int_key == i64::MIN {
            0
        } else {
            self.next_int_key
        };
        let already_occupied = self.find_int(key).is_some();
        if already_occupied {
            return Err("Cannot add element to the array as the next element is already occupied");
        }
        if !self.reserve_one() {
            return Ok(()); // Memory limit exceeded - silently ignore
        }
        self.next_int_key = key.saturating_add(1);
        self.entries.push((ArrayKey::Int(key), value));
        self.after_push();
        self.sync_packed();
        Ok(())
    }

    /// Remove and return the last element (like array_pop)
    pub fn pop(&mut self) -> Option<Value> {
        self.pop_entry_raw().map(|(_, v)| v)
    }

    fn pop_entry_raw(&mut self) -> Option<(ArrayKey, Value)> {
        self.separate();
        if self.is_empty() {
            return None;
        }
        let last = self.len() - 1;
        self.iters_removed_at(last);
        let e = self.entries.pop()?;
        if !self.packed && !self.index.is_empty() {
            // Remove the slot pointing at the popped position, then
            // re-insert the rest of its probe cluster (backward-shift).
            let pos = self.entries.len() as u32 + 1;
            let mask = self.index.len() - 1;
            let mut i = (hash_key(&e.0) as usize) & mask;
            while self.index[i] != 0 && self.index[i] != pos {
                i = (i + 1) & mask;
            }
            if self.index[i] == pos {
                self.index[i] = 0;
                let mut j = (i + 1) & mask;
                while self.index[j] != 0 {
                    let p = self.index[j];
                    self.index[j] = 0;
                    let h = hash_key(&self.entries[(p - 1) as usize].0);
                    Self::index_insert_slot(&mut self.index, h, (p - 1) as usize);
                    j = (j + 1) & mask;
                }
            }
        }
        if self.is_empty() && self.head > 0 {
            self.entries.clear();
            self.head = 0;
            self.index = Vec::new();
        }
        Some(e)
    }

    /// Remove the index slot of the entry at (physical) position `pos`,
    /// re-inserting the rest of its probe cluster (backward-shift).
    fn index_remove(&mut self, pos: usize) {
        debug_assert!(self.shared.is_none());
        if self.index.is_empty() {
            return;
        }
        let want = pos as u32 + 1;
        let mask = self.index.len() - 1;
        let mut i = (hash_key(&self.entries[pos].0) as usize) & mask;
        while self.index[i] != 0 && self.index[i] != want {
            i = (i + 1) & mask;
        }
        if self.index[i] != want {
            return;
        }
        self.index[i] = 0;
        let mut j = (i + 1) & mask;
        while self.index[j] != 0 {
            let p = self.index[j];
            self.index[j] = 0;
            let h = hash_key(&self.entries[(p - 1) as usize].0);
            Self::index_insert_slot(&mut self.index, h, (p - 1) as usize);
            j = (j + 1) & mask;
        }
    }

    /// Remove the first entry (O(1) amortized: its slot becomes dead).
    fn remove_front_raw(&mut self) -> Value {
        debug_assert!(self.shared.is_none());
        self.iters_removed_at(0);
        let pos = self.head as usize;
        if self.packed {
            // Key 0 goes: the rest is no longer packed.
            self.packed = false;
            self.head += 1;
            let v = std::mem::take(&mut self.entries[pos].1);
            self.entries[pos].0 = ArrayKey::Int(0);
            self.rebuild_index();
            self.sync_packed();
            return v;
        }
        self.index_remove(pos);
        let v = std::mem::take(&mut self.entries[pos].1);
        self.entries[pos].0 = ArrayKey::Int(0);
        self.head += 1;
        if self.head as usize >= 32 && self.head as usize > self.len() {
            self.compact();
        } else if !self.index.is_empty() && self.len() < INDEX_THRESHOLD {
            self.index = Vec::new();
        }
        v
    }

    /// Insert an entry before the first one (the key must not exist yet).
    /// O(1) amortized. Silently ignored when memory_limit forbids it.
    pub fn push_front(&mut self, key: ArrayKey, value: Value) {
        self.separate();
        if !self.reserve_one() {
            return;
        }
        if let ArrayKey::Int(n) = &key
            && *n >= self.next_int_key
        {
            self.next_int_key = n.saturating_add(1);
        }
        if self.head == 0 {
            // Make room at the front.
            let room = self.len().max(8);
            let mut e = Vec::with_capacity(room + self.entries.len() + 1);
            e.resize_with(room, || (ArrayKey::Int(0), Value::Null));
            e.append(&mut self.entries);
            self.entries = e;
            self.head = room as u32;
            // (positions moved: rebuilt below)
            self.index = Vec::new();
        }
        self.head -= 1;
        let pos = self.head as usize;
        self.entries[pos] = (key, value);
        self.iters_inserted_at(0);
        if self.len() > 1 {
            self.pointer += 1;
        }
        self.recompute_packed();
        if self.packed {
            self.compact();
        } else {
            self.rebuild_index_after_insert(pos);
        }
        self.sync_packed();
    }

    /// Index maintenance after an entry was placed at (physical) `pos`
    /// outside of the append path.
    fn rebuild_index_after_insert(&mut self, pos: usize) {
        debug_assert!(self.shared.is_none());
        if self.index.is_empty() || self.len() * 2 > self.index.len() {
            self.rebuild_index();
        } else {
            let h = hash_key(&self.entries[pos].0);
            Self::index_insert_slot(&mut self.index, h, pos);
        }
    }

    /// Insert an entry at insertion-order position `pos` (the key must not
    /// exist yet). O(n). Silently ignored when memory_limit forbids it.
    pub fn insert_at(&mut self, pos: usize, key: ArrayKey, value: Value) {
        self.separate();
        if pos == 0 {
            return self.push_front(key, value);
        }
        if pos >= self.len() {
            if !self.reserve_one() {
                return;
            }
            if let ArrayKey::Int(n) = &key
                && *n >= self.next_int_key
            {
                self.next_int_key = n.saturating_add(1);
            }
            self.entries.push((key, value));
            self.after_push();
            self.sync_packed();
            return;
        }
        if !self.reserve_one() {
            return;
        }
        if let ArrayKey::Int(n) = &key
            && *n >= self.next_int_key
        {
            self.next_int_key = n.saturating_add(1);
        }
        self.compact();
        self.entries.insert(pos, (key, value));
        self.iters_inserted_at(pos);
        if pos < self.pointer {
            self.pointer += 1;
        }
        self.recompute_packed();
        self.rebuild_index();
        self.sync_packed();
    }

    /// Remove and return the last entry, adjusting the next free integer
    /// key the way PHP's array_pop() does (decremented only if the popped
    /// key was the most recently assigned one).
    pub fn pop_entry(&mut self) -> Option<(ArrayKey, Value)> {
        let e = self.pop_entry_raw()?;
        if let ArrayKey::Int(k) = &e.0 {
            if self.next_int_key != i64::MIN && *k == self.next_int_key.wrapping_sub(1) {
                self.next_int_key -= 1;
            }
        }
        Some(e)
    }

    /// Remove and return the first element (like array_shift)
    pub fn shift(&mut self) -> Option<Value> {
        self.separate();
        self.compact();
        if self.entries.is_empty() {
            None
        } else {
            self.iters_removed_at(0);
            let v = self.entries.remove(0).1;
            self.recompute_packed();
            self.rebuild_index();
            self.sync_packed();
            Some(v)
        }
    }

    /// array_shift() on a packed array, in place: the first element goes,
    /// the rest move down and are renumbered, the internal pointer is
    /// reset. None (array untouched) when the array is empty or not packed.
    pub fn shift_packed(&mut self) -> Option<Value> {
        self.separate();
        if !self.packed || self.entries.is_empty() {
            return None;
        }
        self.iters_removed_at(0);
        let v = self.entries.remove(0).1;
        for (i, e) in self.entries.iter_mut().enumerate() {
            e.0 = ArrayKey::Int(i as i64);
        }
        self.next_int_key = if self.entries.is_empty() {
            i64::MIN
        } else {
            self.entries.len() as i64
        };
        self.pointer = 0;
        Some(v)
    }

    /// Insert a value at the beginning with key 0 (like array_unshift)
    pub fn unshift(&mut self, value: Value) {
        self.separate();
        if !self.reserve_one() {
            return;
        }
        self.compact();
        self.entries.insert(0, (ArrayKey::Int(0), value));
        self.iters_inserted_at(0);
        // Re-key all integer-keyed entries
        let mut next = 0i64;
        for entry in &mut self.entries {
            if let ArrayKey::Int(_) = &entry.0 {
                entry.0 = ArrayKey::Int(next);
                next += 1;
            }
        }
        self.next_int_key = next;
        self.recompute_packed();
        self.rebuild_index();
        self.sync_packed();
    }

    /// Remove an entry by key
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Value> {
        let v = self.remove_inner(key);
        if let Some(v) = &v {
            crate::gc::on_child_released(v);
        }
        v
    }

    /// Remove an entry by key and hand its value over to the caller (no
    /// release bookkeeping: the value lives on).
    pub fn take(&mut self, key: &ArrayKey) -> Option<Value> {
        self.remove_inner(key)
    }

    fn remove_inner(&mut self, key: &ArrayKey) -> Option<Value> {
        self.separate();
        if let Some(pos) = self.find(key) {
            // The internal pointer keeps designating the same element (or,
            // past the end, the slot the next append fills): entries after
            // the removed one move down.
            if pos - (self.head as usize) < self.pointer {
                self.pointer -= 1;
            }
            if pos + 1 == self.entries.len() {
                return self.pop_entry_raw().map(|(_, v)| v);
            }
            if pos == self.head as usize {
                return Some(self.remove_front_raw());
            }
            self.iters_removed_at(pos - self.head as usize);
            if self.packed || self.index.is_empty() {
                let v = self.entries.remove(pos).1;
                self.packed = false;
                self.rebuild_index();
                self.sync_packed();
                return Some(v);
            }
            // Hash: drop the entry's slot, then renumber the positions of
            // the entries that moved down (no rehash).
            self.index_remove(pos);
            let v = self.entries.remove(pos).1;
            if self.len() < INDEX_THRESHOLD {
                self.index = Vec::new();
            } else {
                let moved = pos as u32 + 1;
                for slot in self.index.iter_mut() {
                    if *slot > moved {
                        *slot -= 1;
                    }
                }
            }
            Some(v)
        } else {
            None
        }
    }

    /// Recalculate the next_int_key based on current entries.
    /// Called after array_pop to ensure array_push uses the correct key.
    pub fn recalculate_next_int_key(&mut self) {
        let mut next = i64::MIN;
        for (key, _) in self.live() {
            if let ArrayKey::Int(n) = key {
                if *n >= next {
                    next = n + 1;
                }
            }
        }
        self.next_int_key = next;
    }

    /// Iterate over entries in order
    pub fn iter(&self) -> impl Iterator<Item = (&ArrayKey, &Value)> {
        self.live().iter().map(|(k, v)| (k, v))
    }

    /// Iterate over entries mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ArrayKey, &mut Value)> {
        self.separate();
        self.live_mut().iter_mut().map(|(k, v)| (k as &ArrayKey, v))
    }

    /// Get keys
    pub fn keys(&self) -> impl Iterator<Item = &ArrayKey> {
        self.live().iter().map(|(k, _)| k)
    }

    /// Get values
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.live().iter().map(|(_, v)| v)
    }

    /// Check if a key exists
    pub fn contains_key(&self, key: &ArrayKey) -> bool {
        self.find(key).is_some()
    }

    /// The entry at insertion-order position `pos`.
    pub fn entry_at(&self, pos: usize) -> Option<(&ArrayKey, &Value)> {
        self.live().get(pos).map(|(k, v)| (k, v))
    }

    /// Mutable value of the entry at insertion-order position `pos`.
    pub fn value_at_mut(&mut self, pos: usize) -> Option<&mut Value> {
        self.separate();
        self.live_mut().get_mut(pos).map(|(_, v)| v)
    }

    /// Insertion-order position of `key`.
    pub fn position_of(&self, key: &ArrayKey) -> Option<usize> {
        self.find(key).map(|p| p - self.head as usize)
    }
}

impl Default for PhpArray {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PhpArray {
    fn drop(&mut self) {
        if self.gc_buffered.get() {
            crate::gc::root_freed(self.gc_buffered.slot());
        }
        // (shared storage is released by its last holder: see Store)
        if self.shared.is_some() {
            return;
        }
        // Release the accounted table.
        let bytes = self.zend_size();
        if bytes > 0 {
            memory_free(bytes);
        }
        let mut nested = false;
        for (_, v) in self.entries.iter() {
            crate::gc::on_child_released(v);
            nested |= matches!(v, Value::Array(_) | Value::Reference(_));
        }
        if !nested {
            return;
        }
        // A deeply nested array (`$p[0] = []; $p = &$p[0];` many times) is
        // released iteratively past a depth: its elements are left for the
        // outermost release to drop, so the native stack does not overflow.
        // (thread-local storage may be gone while the thread exits)
        let Ok(depth) = DROP_DEPTH.try_with(|d| d.get()) else {
            return;
        };
        if depth >= MAX_DROP_DEPTH {
            let entries = std::mem::take(&mut self.entries);
            let _ = DEFERRED_DROPS
                .try_with(|q| q.borrow_mut().extend(entries.into_iter().map(|(_, v)| v)));
            return;
        }
        let _ = DROP_DEPTH.try_with(|d| d.set(depth + 1));
        drop(std::mem::take(&mut self.entries));
        if depth == 0 {
            while let Some(batch) = DEFERRED_DROPS
                .try_with(|q| std::mem::take(&mut *q.borrow_mut()))
                .ok()
                .filter(|b| !b.is_empty())
            {
                drop(batch);
            }
        }
        let _ = DROP_DEPTH.try_with(|d| d.set(depth));
    }
}

/// Nesting of PhpArray releases in progress (see `Drop for PhpArray`).
const MAX_DROP_DEPTH: u32 = 512;

thread_local! {
    static DROP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static DEFERRED_DROPS: std::cell::RefCell<Vec<Value>> = const { std::cell::RefCell::new(Vec::new()) };
}
