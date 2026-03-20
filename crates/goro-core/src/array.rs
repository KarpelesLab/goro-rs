use crate::string::PhpString;
use crate::value::Value;

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
/// Packed optimization: when keys are sequential integers 0..n, uses a simple Vec.
#[derive(Debug, Clone)]
pub struct PhpArray {
    /// Current storage mode
    entries: Vec<(ArrayKey, Value)>,
    /// Next integer key to use for append ($arr[] = val)
    next_int_key: i64,
    /// Internal array pointer position (for current/next/prev/reset/end)
    pub pointer: usize,
}

impl PhpArray {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_int_key: 0,
            pointer: 0,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            next_int_key: 0,
            pointer: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get a value by integer key
    pub fn get_int(&self, key: i64) -> Option<&Value> {
        for (k, v) in &self.entries {
            if let ArrayKey::Int(n) = k
                && *n == key
            {
                return Some(v);
            }
        }
        None
    }

    /// Get a value by string key
    pub fn get_str(&self, key: &[u8]) -> Option<&Value> {
        for (k, v) in &self.entries {
            if let ArrayKey::String(s) = k
                && s.as_bytes() == key
            {
                return Some(v);
            }
        }
        None
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
        for (k, v) in &mut self.entries {
            if k == key {
                return Some(v);
            }
        }
        None
    }

    /// Maximum array size to prevent OOM (128M elements)
    const MAX_SIZE: usize = 128 * 1024 * 1024;

    /// Set a value with a specific key
    pub fn set(&mut self, key: ArrayKey, value: Value) {
        // Check if key already exists
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = value;
                return;
            }
        }
        if self.entries.len() >= Self::MAX_SIZE {
            return; // Silently refuse to grow beyond limit
        }
        // Track next_int_key
        if let ArrayKey::Int(n) = &key
            && *n >= self.next_int_key
        {
            self.next_int_key = n + 1;
        }
        self.entries.push((key, value));
    }

    /// Append a value with the next integer key ($arr[] = value)
    pub fn push(&mut self, value: Value) {
        if self.entries.len() >= Self::MAX_SIZE {
            return;
        }
        let key = self.next_int_key;
        self.next_int_key = key + 1;
        self.entries.push((ArrayKey::Int(key), value));
    }

    /// Remove and return the last element (like array_pop)
    pub fn pop(&mut self) -> Option<Value> {
        self.entries.pop().map(|(_, v)| v)
    }

    /// Remove and return the first element (like array_shift)
    pub fn shift(&mut self) -> Option<Value> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0).1)
        }
    }

    /// Insert a value at the beginning with key 0 (like array_unshift)
    pub fn unshift(&mut self, value: Value) {
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
    }

    /// Remove an entry by key
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Value> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == key) {
            Some(self.entries.remove(pos).1)
        } else {
            None
        }
    }

    /// Recalculate the next_int_key based on current entries.
    /// Called after array_pop to ensure array_push uses the correct key.
    pub fn recalculate_next_int_key(&mut self) {
        self.next_int_key = 0;
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
        self.entries.iter().any(|(k, _)| k == key)
    }
}

impl Default for PhpArray {
    fn default() -> Self {
        Self::new()
    }
}
