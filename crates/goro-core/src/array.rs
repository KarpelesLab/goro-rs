use crate::string::PhpString;
use crate::value::Value;

/// Array key - either integer or string
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    Int(i64),
    String(PhpString),
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
}

impl PhpArray {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_int_key: 0,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            next_int_key: 0,
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
            if let ArrayKey::Int(n) = k {
                if *n == key {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Get a value by string key
    pub fn get_str(&self, key: &[u8]) -> Option<&Value> {
        for (k, v) in &self.entries {
            if let ArrayKey::String(s) = k {
                if s.as_bytes() == key {
                    return Some(v);
                }
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

    /// Set a value with a specific key
    pub fn set(&mut self, key: ArrayKey, value: Value) {
        // Check if key already exists
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = value;
                return;
            }
        }
        // Track next_int_key
        if let ArrayKey::Int(n) = &key {
            if *n >= self.next_int_key {
                self.next_int_key = n + 1;
            }
        }
        self.entries.push((key, value));
    }

    /// Append a value with the next integer key ($arr[] = value)
    pub fn push(&mut self, value: Value) {
        let key = self.next_int_key;
        self.next_int_key = key + 1;
        self.entries.push((ArrayKey::Int(key), value));
    }

    /// Remove an entry by key
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Value> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == key) {
            Some(self.entries.remove(pos).1)
        } else {
            None
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
