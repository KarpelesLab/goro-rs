//! A small, fast, non-cryptographic hasher (the FxHash algorithm used by
//! rustc) for the VM's internal lookup tables: classes, functions and
//! constants keyed by (lowercased) names.
//!
//! These tables are looked up several times per call / property access, and
//! std's default SipHash dominated those lookups.  Their keys are names from
//! the program source, not request data, so HashDoS resistance is not needed
//! here.  Do not use this for tables keyed by user-supplied data (PHP arrays
//! keep their own hashing).

use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add_to_hash(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            self.add_to_hash(u64::from_le_bytes(bytes[..8].try_into().unwrap()));
            bytes = &bytes[8..];
        }
        if bytes.len() >= 4 {
            self.add_to_hash(u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64);
            bytes = &bytes[4..];
        }
        for &b in bytes {
            self.add_to_hash(b as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
pub type FxHashSet<K> = std::collections::HashSet<K, FxBuildHasher>;
pub type FxIndexMap<K, V> = indexmap::IndexMap<K, V, NameBuildHasher>;

/// FxHasher for byte-string keys hashed whole: a name is read 8 bytes at
/// a time, its last (up to) 8 bytes at once (the length is hashed before
/// the bytes), and the result is rotated so that the bits hashbrown picks
/// the bucket with are the well mixed ones. For tables whose order does
/// not depend on the hash (IndexMap) or is never looked at.
#[derive(Default, Clone, Copy)]
pub struct NameHasher {
    hash: u64,
}

impl NameHasher {
    #[inline]
    fn add_to_hash(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

impl Hasher for NameHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let n = bytes.len();
        let w = |i: usize| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        if n >= 8 {
            let mut i = 0;
            while i + 8 < n {
                self.add_to_hash(w(i));
                i += 8;
            }
            self.add_to_hash(w(n - 8));
        } else if n >= 4 {
            let v = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as u64;
            self.add_to_hash((v(0) << 32) | v(n - 4));
        } else if n > 0 {
            self.add_to_hash(
                bytes[0] as u64 | (bytes[n / 2] as u64) << 8 | (bytes[n - 1] as u64) << 16,
            );
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash.rotate_left(26)
    }
}

pub type NameBuildHasher = BuildHasherDefault<NameHasher>;
/// A FxHashMap with NameHasher (for maps that are never iterated).
pub type NameHashMap<K, V> = std::collections::HashMap<K, V, NameBuildHasher>;
