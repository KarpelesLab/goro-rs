use crate::string::PhpString;
use crate::value::{Value, memory_alloc, memory_free};

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
    /// Current storage mode
    entries: Vec<(ArrayKey, Value)>,
    /// Next integer key to use for append ($arr[] = val)
    next_int_key: i64,
    /// Internal array pointer position (for current/next/prev/reset/end)
    pub pointer: usize,
    /// True while every entry i has key Int(i)
    packed: bool,
    /// Hash index (slot -> entry position + 1, 0 = empty); empty when unused
    index: Vec<u32>,
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

#[inline]
fn hash_bytes(b: &[u8]) -> u64 {
    // FNV-1a
    let mut h: u64 = 0xcbf29ce484222325;
    for &c in b {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^ (h >> 29)
}

#[inline]
fn hash_key(k: &ArrayKey) -> u64 {
    match k {
        ArrayKey::Int(n) => hash_int(*n),
        ArrayKey::String(s) => hash_bytes(s.as_bytes()),
    }
}

thread_local! {
    /// Arrays currently being cloned (guards against cycles).
    static CLONE_STACK: std::cell::RefCell<Vec<*const PhpArray>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Copy a nested array for PhpArray::clone; None means share it instead
/// (cycle, or the array is currently mutably borrowed).
fn dup_nested_array(a: &std::rc::Rc<std::cell::RefCell<PhpArray>>) -> Option<Value> {
    let ptr = a.as_ptr() as *const PhpArray;
    if CLONE_STACK.with(|st| st.borrow().iter().any(|p| std::ptr::eq(*p, ptr))) {
        return None;
    }
    let inner = a.try_borrow().ok()?;
    Some(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        inner.clone(),
    ))))
}

impl Clone for PhpArray {
    fn clone(&self) -> Self {
        // Track memory for the cloned entries
        let bytes = self.entries.len() * Self::ENTRY_SIZE;
        if bytes > 0 {
            memory_alloc(bytes); // Track even if over limit - we're cloning existing data
        }
        // Like zend_array_dup(): references held only by this array (rc=1)
        // are copied as plain values, unless they point back at this array.
        // Closure values (arrays holding captured variables) are copied as-is.
        let self_ptr = self as *const PhpArray;
        let is_closure = matches!(self.entries.first(), Some((_, Value::String(s)))
            if s.as_bytes().starts_with(b"__closure_") || s.as_bytes().starts_with(b"__arrow_"));
        let entries = if is_closure {
            self.entries.clone()
        } else {
            CLONE_STACK.with(|st| st.borrow_mut().push(self_ptr));
            let e = self
                .entries
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
                                Value::Array(a) => dup_nested_array(a).unwrap_or_else(|| v.clone()),
                                other => other.clone(),
                            }
                        }
                        // Nested arrays are values: give the copy its own storage
                        // (goro copies eagerly instead of refcounted copy-on-write).
                        Value::Array(a) => dup_nested_array(a).unwrap_or_else(|| v.clone()),
                        _ => v.clone(),
                    };
                    (k.clone(), v2)
                })
                .collect();
            CLONE_STACK.with(|st| {
                st.borrow_mut().pop();
            });
            e
        };
        Self {
            entries,
            next_int_key: self.next_int_key,
            pointer: self.pointer,
            packed: self.packed,
            index: self.index.clone(),
        }
    }
}

impl PhpArray {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_int_key: i64::MIN,
            pointer: 0,
            packed: true,
            index: Vec::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            next_int_key: i64::MIN,
            pointer: 0,
            packed: true,
            index: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
        if self.packed
            || self.entries.len() < INDEX_THRESHOLD
            || self.entries.len() >= u32::MAX as usize / 4
        {
            self.index = Vec::new();
            return;
        }
        let cap = (self.entries.len() * 2).next_power_of_two().max(16);
        let mut index = vec![0u32; cap];
        for (pos, (k, _)) in self.entries.iter().enumerate() {
            Self::index_insert_slot(&mut index, hash_key(k), pos);
        }
        self.index = index;
    }

    /// Called after pushing a new entry at the end of `entries`
    fn after_push(&mut self) {
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
            if self.entries.len() >= INDEX_THRESHOLD {
                self.rebuild_index();
            }
            return;
        }
        if (self.entries.len()) * 2 > self.index.len() {
            self.rebuild_index();
            return;
        }
        let h = hash_key(&self.entries[pos].0);
        Self::index_insert_slot(&mut self.index, h, pos);
    }

    fn recompute_packed(&mut self) {
        self.packed = self
            .entries
            .iter()
            .enumerate()
            .all(|(i, (k, _))| matches!(k, ArrayKey::Int(n) if *n == i as i64));
    }

    #[inline]
    fn find_int(&self, key: i64) -> Option<usize> {
        if self.packed {
            if key >= 0 && (key as usize) < self.entries.len() {
                return Some(key as usize);
            }
            return None;
        }
        if !self.index.is_empty() {
            let mask = self.index.len() - 1;
            let mut i = (hash_int(key) as usize) & mask;
            loop {
                let slot = self.index[i];
                if slot == 0 {
                    return None;
                }
                let pos = (slot - 1) as usize;
                if let ArrayKey::Int(n) = &self.entries[pos].0 {
                    if *n == key {
                        return Some(pos);
                    }
                }
                i = (i + 1) & mask;
            }
        }
        self.entries
            .iter()
            .position(|(k, _)| matches!(k, ArrayKey::Int(n) if *n == key))
    }

    #[inline]
    fn find_str(&self, key: &[u8]) -> Option<usize> {
        if self.packed {
            return None;
        }
        if !self.index.is_empty() {
            let mask = self.index.len() - 1;
            let mut i = (hash_bytes(key) as usize) & mask;
            loop {
                let slot = self.index[i];
                if slot == 0 {
                    return None;
                }
                let pos = (slot - 1) as usize;
                if let ArrayKey::String(s) = &self.entries[pos].0 {
                    if s.as_bytes() == key {
                        return Some(pos);
                    }
                }
                i = (i + 1) & mask;
            }
        }
        self.entries
            .iter()
            .position(|(k, _)| matches!(k, ArrayKey::String(s) if s.as_bytes() == key))
    }

    #[inline]
    fn find(&self, key: &ArrayKey) -> Option<usize> {
        match key {
            ArrayKey::Int(n) => self.find_int(*n),
            ArrayKey::String(s) => self.find_str(s.as_bytes()),
        }
    }

    /// Get a value by integer key
    pub fn get_int(&self, key: i64) -> Option<&Value> {
        self.find_int(key).map(|p| &self.entries[p].1)
    }

    /// Get a value by string key
    pub fn get_str(&self, key: &[u8]) -> Option<&Value> {
        self.find_str(key).map(|p| &self.entries[p].1)
    }

    /// Get a value by ArrayKey
    pub fn get(&self, key: &ArrayKey) -> Option<&Value> {
        match key {
            ArrayKey::Int(n) => self.get_int(*n),
            ArrayKey::String(s) => self.get_str(s.as_bytes()),
        }
    }

    /// Get a mutable reference by ArrayKey
    pub fn get_mut(&mut self, key: &ArrayKey) -> Option<&mut Value> {
        match self.find(key) {
            Some(p) => Some(&mut self.entries[p].1),
            None => None,
        }
    }

    /// Approximate bytes per entry (key + value overhead)
    const ENTRY_SIZE: usize = 80;

    /// Set a value with a specific key
    pub fn set(&mut self, key: ArrayKey, value: Value) {
        // Check if key already exists
        if let Some(p) = self.find(&key) {
            self.entries[p].1 = value;
            return;
        }
        // Check memory limit before adding
        if !memory_alloc(Self::ENTRY_SIZE) {
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
    }

    /// The key the next append would use (nNextFreeElement).
    pub fn next_free_key(&self) -> i64 {
        self.next_int_key
    }

    /// Append a value with the next integer key ($arr[] = value).
    /// Silently ignores if memory limit is exceeded. Ignores key conflicts.
    pub fn push(&mut self, value: Value) {
        // nNextFreeElement == ZEND_LONG_MIN means no integer key used yet
        let key = if self.next_int_key == i64::MIN {
            0
        } else {
            self.next_int_key
        };
        if !memory_alloc(Self::ENTRY_SIZE) {
            return; // Memory limit exceeded
        }
        self.next_int_key = key.saturating_add(1);
        self.entries.push((ArrayKey::Int(key), value));
        self.after_push();
    }

    /// Append a value with the next integer key ($arr[] = value).
    /// Returns Err("Cannot add element...") if the key is already occupied (PHP_INT_MAX overflow).
    pub fn try_push(&mut self, value: Value) -> Result<(), &'static str> {
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
        if !memory_alloc(Self::ENTRY_SIZE) {
            return Ok(()); // Memory limit exceeded - silently ignore
        }
        self.next_int_key = key.saturating_add(1);
        self.entries.push((ArrayKey::Int(key), value));
        self.after_push();
        Ok(())
    }

    /// Remove and return the last element (like array_pop)
    pub fn pop(&mut self) -> Option<Value> {
        self.pop_entry_raw().map(|(_, v)| v)
    }

    fn pop_entry_raw(&mut self) -> Option<(ArrayKey, Value)> {
        let e = self.entries.pop()?;
        memory_free(Self::ENTRY_SIZE);
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
        Some(e)
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
        if self.entries.is_empty() {
            None
        } else {
            memory_free(Self::ENTRY_SIZE);
            let v = self.entries.remove(0).1;
            self.recompute_packed();
            self.rebuild_index();
            Some(v)
        }
    }

    /// Insert a value at the beginning with key 0 (like array_unshift)
    pub fn unshift(&mut self, value: Value) {
        if !memory_alloc(Self::ENTRY_SIZE) {
            return;
        }
        self.entries.insert(0, (ArrayKey::Int(0), value));
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
    }

    /// Remove an entry by key
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Value> {
        if let Some(pos) = self.find(key) {
            if pos + 1 == self.entries.len() {
                return self.pop_entry_raw().map(|(_, v)| v);
            }
            memory_free(Self::ENTRY_SIZE);
            let v = self.entries.remove(pos).1;
            self.packed = false;
            self.rebuild_index();
            Some(v)
        } else {
            None
        }
    }

    /// Recalculate the next_int_key based on current entries.
    /// Called after array_pop to ensure array_push uses the correct key.
    pub fn recalculate_next_int_key(&mut self) {
        self.next_int_key = i64::MIN;
        for (key, _) in &self.entries {
            if let ArrayKey::Int(n) = key {
                if *n >= self.next_int_key {
                    self.next_int_key = n + 1;
                }
            }
        }
    }

    /// Iterate over entries in order
    pub fn iter(&self) -> impl Iterator<Item = (&ArrayKey, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Iterate over entries mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ArrayKey, &mut Value)> {
        self.entries.iter_mut().map(|(k, v)| (k as &ArrayKey, v))
    }

    /// Get keys
    pub fn keys(&self) -> impl Iterator<Item = &ArrayKey> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// Get values
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// Check if a key exists
    pub fn contains_key(&self, key: &ArrayKey) -> bool {
        self.find(key).is_some()
    }
}

impl Default for PhpArray {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PhpArray {
    fn drop(&mut self) {
        // Release tracked memory for all entries
        let bytes = self.entries.len() * Self::ENTRY_SIZE;
        if bytes > 0 {
            memory_free(bytes);
        }
        for (_, v) in self.entries.iter() {
            crate::gc::on_child_released(v);
        }
    }
}
