//! Port of hash.c: the generic hash table keyed by case-insensitive
//! strings.  The element order (first/next) matches C exactly, which
//! matters wherever SQLite iterates over a hash (PRAGMA table_list, ...).

use crate::consts::SQLITE_MALLOC_SOFT_LIMIT;

struct HashElem<T> {
    next: usize,
    prev: usize,
    data: T,
    key: Vec<u8>,
    h: u32,
}

#[derive(Clone, Copy, Default)]
struct Ht {
    count: u32,
    chain: usize,
}

const NIL: usize = usize::MAX;

/// A hash table mapping case-insensitive names to values.
pub struct Hash<T> {
    elems: Vec<Option<HashElem<T>>>,
    free: Vec<usize>,
    first: usize,
    count: u32,
    ht: Vec<Ht>,
}

impl<T> Default for Hash<T> {
    fn default() -> Self {
        Hash::new()
    }
}

fn str_hash(z: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in z {
        if c == 0 {
            break;
        }
        h = h.wrapping_add((0xdf & c) as u32);
        h = h.wrapping_mul(0x9e3779b1);
    }
    h
}

fn str_icmp_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

impl<T> Hash<T> {
    pub fn new() -> Hash<T> {
        Hash {
            elems: Vec::new(),
            free: Vec::new(),
            first: NIL,
            count: 0,
            ht: Vec::new(),
        }
    }

    /// sqlite3HashClear()
    pub fn clear(&mut self) {
        self.elems.clear();
        self.free.clear();
        self.first = NIL;
        self.count = 0;
        self.ht.clear();
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    fn e(&self, i: usize) -> &HashElem<T> {
        self.elems[i].as_ref().unwrap()
    }
    fn em(&mut self, i: usize) -> &mut HashElem<T> {
        self.elems[i].as_mut().unwrap()
    }

    fn insert_element(&mut self, entry: Option<usize>, new: usize) {
        let head = match entry {
            Some(b) => {
                let ht = &mut self.ht[b];
                let head = if ht.count != 0 { ht.chain } else { NIL };
                ht.count += 1;
                ht.chain = new;
                head
            }
            None => NIL,
        };
        if head != NIL {
            let hprev = self.e(head).prev;
            self.em(new).next = head;
            self.em(new).prev = hprev;
            if hprev != NIL {
                self.em(hprev).next = new;
            } else {
                self.first = new;
            }
            self.em(head).prev = new;
        } else {
            let first = self.first;
            self.em(new).next = first;
            if first != NIL {
                self.em(first).prev = new;
            }
            self.em(new).prev = NIL;
            self.first = new;
        }
    }

    fn rehash(&mut self, mut new_size: u32) -> bool {
        let szht = 16u32;
        if new_size * szht > SQLITE_MALLOC_SOFT_LIMIT as u32 {
            new_size = SQLITE_MALLOC_SOFT_LIMIT as u32 / szht;
        }
        if new_size as usize == self.ht.len() {
            return false;
        }
        self.ht = vec![Ht::default(); new_size as usize];
        let mut elem = self.first;
        self.first = NIL;
        while elem != NIL {
            let next = self.e(elem).next;
            let b = (self.e(elem).h % new_size) as usize;
            self.insert_element(Some(b), elem);
            elem = next;
        }
        true
    }

    fn find_element(&self, key: &[u8]) -> (Option<usize>, u32) {
        let h = str_hash(key);
        let (mut elem, mut count) = if !self.ht.is_empty() {
            let b = &self.ht[(h % self.ht.len() as u32) as usize];
            (b.chain, b.count)
        } else {
            (self.first, self.count)
        };
        while count > 0 {
            let e = self.e(elem);
            if e.h == h && str_icmp_eq(&e.key, key) {
                return (Some(elem), h);
            }
            elem = e.next;
            count -= 1;
        }
        (None, h)
    }

    fn remove_element(&mut self, elem: usize) -> T {
        let (prev, next, h) = {
            let e = self.e(elem);
            (e.prev, e.next, e.h)
        };
        if prev != NIL {
            self.em(prev).next = next;
        } else {
            self.first = next;
        }
        if next != NIL {
            self.em(next).prev = prev;
        }
        if !self.ht.is_empty() {
            let n = self.ht.len() as u32;
            let b = &mut self.ht[(h % n) as usize];
            if b.chain == elem {
                b.chain = next;
            }
            b.count -= 1;
        }
        let e = self.elems[elem].take().unwrap();
        self.free.push(elem);
        self.count -= 1;
        if self.count == 0 {
            self.clear();
        }
        e.data
    }

    /// sqlite3HashFind()
    pub fn find(&self, key: &[u8]) -> Option<&T> {
        self.find_element(key).0.map(|i| &self.e(i).data)
    }

    pub fn find_mut(&mut self, key: &[u8]) -> Option<&mut T> {
        match self.find_element(key).0 {
            Some(i) => Some(&mut self.em(i).data),
            None => None,
        }
    }

    /// sqlite3HashInsert() with non-NULL data: insert or replace.  Returns
    /// the old value if the key was present.
    pub fn insert(&mut self, key: &[u8], data: T) -> Option<T> {
        let (found, h) = self.find_element(key);
        if let Some(i) = found {
            let e = self.em(i);
            e.key = key.to_vec();
            return Some(std::mem::replace(&mut e.data, data));
        }
        let ne = HashElem {
            next: NIL,
            prev: NIL,
            data,
            key: key.to_vec(),
            h,
        };
        let idx = if let Some(i) = self.free.pop() {
            self.elems[i] = Some(ne);
            i
        } else {
            self.elems.push(Some(ne));
            self.elems.len() - 1
        };
        self.count += 1;
        if self.count >= 5 && self.count as usize > 2 * self.ht.len() {
            self.rehash(self.count * 3);
        }
        let entry = if !self.ht.is_empty() {
            Some((h % self.ht.len() as u32) as usize)
        } else {
            None
        };
        self.insert_element(entry, idx);
        None
    }

    /// sqlite3HashInsert() with NULL data: remove the key.
    pub fn remove(&mut self, key: &[u8]) -> Option<T> {
        match self.find_element(key).0 {
            Some(i) => Some(self.remove_element(i)),
            None => None,
        }
    }

    /// Iterate in sqliteHashFirst()/sqliteHashNext() order.
    pub fn iter(&self) -> HashIter<'_, T> {
        HashIter {
            h: self,
            cur: self.first,
        }
    }

    /// Keys in iteration order (handy when the hash is modified while
    /// iterating).
    pub fn keys(&self) -> Vec<Vec<u8>> {
        self.iter().map(|(k, _)| k.to_vec()).collect()
    }
}

impl<T: Clone> Hash<T> {
    /// Values in iteration order.
    pub fn values(&self) -> Vec<T> {
        self.iter().map(|(_, v)| v.clone()).collect()
    }
}

pub struct HashIter<'a, T> {
    h: &'a Hash<T>,
    cur: usize,
}

impl<'a, T> Iterator for HashIter<'a, T> {
    type Item = (&'a [u8], &'a T);
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur == NIL {
            return None;
        }
        let e = self.h.e(self.cur);
        self.cur = e.next;
        Some((&e.key, &e.data))
    }
}
