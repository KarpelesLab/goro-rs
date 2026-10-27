//! The class table as PHP code sees it (EG(class_table)): the classes in
//! `vm.classes` (user classes, plus internal classes that extensions register
//! with a `ClassEntry`) and the internal classes the VM implements natively,
//! which are described by the stubs (`stub_sigs::CLASSES`).
//!
//! A stub class only counts as declared when its extension is enabled, so
//! e.g. tests skipping on `class_exists('IntlDateFormatter')` keep skipping.
//! Lookups never autoload.

use crate::object::ClassEntry;
use crate::stubs::{self, K_ENUM, K_INTERFACE, K_TRAIT, StubClass};
use crate::value::Value;
use crate::vm::Vm;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

impl ClassKind {
    /// zend_get_object_type()
    pub fn keyword(self) -> &'static str {
        match self {
            ClassKind::Class => "class",
            ClassKind::Interface => "interface",
            ClassKind::Trait => "trait",
            ClassKind::Enum => "enum",
        }
    }

    fn of_stub(c: &StubClass) -> Self {
        match c.kind {
            K_INTERFACE => ClassKind::Interface,
            K_TRAIT => ClassKind::Trait,
            K_ENUM => ClassKind::Enum,
            _ => ClassKind::Class,
        }
    }

    fn of_entry(ce: &crate::object::ClassEntry) -> Self {
        if ce.is_interface {
            ClassKind::Interface
        } else if ce.is_trait {
            ClassKind::Trait
        } else if ce.is_enum {
            ClassKind::Enum
        } else {
            ClassKind::Class
        }
    }
}

/// A class found in the class table.
pub struct ClassLookup<'a> {
    /// Declared (canonical) name.
    pub name: &'a [u8],
    pub kind: ClassKind,
    /// ZEND_INTERNAL_CLASS
    pub internal: bool,
}

impl Vm {
    /// Whether the extension a stub class belongs to is enabled.
    pub fn stub_ext_enabled(&self, ext: &str) -> bool {
        // Always-on parts of the engine that don't register themselves.
        if matches!(ext, "Core" | "standard" | "SPL" | "random" | "uri" | "pcre") {
            return true;
        }
        let mut buf = [0u8; 32];
        let e = ext.as_bytes();
        if e.len() > buf.len() {
            return false;
        }
        for (d, s) in buf.iter_mut().zip(e) {
            *d = s.to_ascii_lowercase();
        }
        self.loaded_extensions.contains(&buf[..e.len()])
    }

    /// The stub of a natively implemented internal class whose extension is
    /// enabled (lowercased name, no leading backslash).
    pub fn internal_class(&self, lower: &[u8]) -> Option<&'static StubClass> {
        let c = stubs::find_class_lower(lower)?;
        (self.stub_ext_enabled(c.ext) && !stubs::is_pseudo_class(c.name)).then_some(c)
    }

    /// zend_lookup_class() without autoloading, by lowercased name (a leading
    /// backslash is ignored).
    pub fn lookup_class(&self, lower: &[u8]) -> Option<ClassLookup<'_>> {
        let lower = lower.strip_prefix(b"\\").unwrap_or(lower);
        if let Some(ce) = self.classes.get(lower) {
            return Some(ClassLookup {
                name: &ce.name,
                kind: ClassKind::of_entry(ce),
                internal: ce.filename.is_none(),
            });
        }
        let c = self.internal_class(lower)?;
        Some(ClassLookup {
            name: c.name.as_bytes(),
            kind: ClassKind::of_stub(c),
            internal: true,
        })
    }

    /// The entry of `vm.classes` named `name` in any case (no leading
    /// backslash), its name lowercased on the stack when short.
    pub fn class_entry_ci(&self, name: &[u8]) -> Option<&crate::object::ClassEntry> {
        let mut buf = [0u8; 128];
        match buf.get_mut(..name.len()) {
            Some(out) => {
                for (o, c) in out.iter_mut().zip(name) {
                    *o = c.to_ascii_lowercase();
                }
                self.classes.get(&*out)
            }
            None => self.classes.get(name.to_ascii_lowercase().as_slice()),
        }
    }

    /// Kind of a declared class (lowercased name), without autoloading.
    pub fn class_kind(&self, lower: &[u8]) -> Option<ClassKind> {
        self.lookup_class(lower).map(|c| c.kind)
    }

    /// Names of the declared classes of the given kinds, in class-table order
    /// (internal classes first): get_declared_classes() and friends.
    pub fn declared_class_names(&self, want: &[ClassKind]) -> Vec<Vec<u8>> {
        let bound = ANON_BOUND.with(|b| b.borrow().clone());
        let mut out = Vec::new();
        for c in crate::stub_sigs::CLASSES {
            if want.contains(&ClassKind::of_stub(c))
                && self.stub_ext_enabled(c.ext)
                && !stubs::is_pseudo_class(c.name)
            {
                let l = c.name.to_ascii_lowercase();
                if !self.classes.contains_key(l.as_bytes()) {
                    out.push(c.name.as_bytes().to_vec());
                }
            }
        }
        for (key, ce) in &self.classes {
            if !want.contains(&ClassKind::of_entry(ce)) {
                continue;
            }
            // An anonymous class is declared when its `new` first runs.
            if ce.name.contains(&0) && !bound.contains(&ce.name) {
                continue;
            }
            // Aliases are listed under their (lowercased) alias name (an
            // empty one is not listed).
            if key.is_empty() {
                continue;
            }
            if !ce.name.eq_ignore_ascii_case(key) && !ce.name.contains(&0) {
                out.push(key.clone());
            } else {
                out.push(ce.name.clone());
            }
        }
        out
    }
}

/// The class table (EG(class_table)): lowercased name -> class entry, in
/// declaration order. The entries are shared (`Rc`) and copied on write:
/// one can be held across code that may declare classes (autoloading,
/// error handlers) and a parent's entry is not copied to link a child.
#[derive(Clone)]
pub struct ClassMap {
    map: crate::fxhash::FxIndexMap<Vec<u8>, Rc<ClassEntry>>,
    /// Changed (to a number no class table had before) by every insert and
    /// every mutable access: what is derived from the table holds while it
    /// does not change (see Vm::class_chain()).
    stamp: u64,
    /// Like `stamp`, but kept by the insertion of a class under its own
    /// name not in the table before: what is derived from the classes
    /// already there (and their ancestors) holds while it does not change.
    link_stamp: u64,
    /// A cache in front of the lookups by name: slot (front_slot() of a
    /// name) -> 1 + the index of the entry last found by a name of that
    /// slot (0: none). An entry keeps its index (entries are only added or
    /// replaced), so a slot needs no invalidation: a hit is the entry whose
    /// key is the name looked up.
    front: Box<[std::cell::Cell<u32>; 1 << FRONT_BITS]>,
    /// The names last looked up and not found, by front slot / 4: (the
    /// `stamp` then, the name). One holds while `stamp` does not change
    /// (it changes with every insertion). Empty until the first one.
    missing: std::cell::RefCell<Vec<(u64, Vec<u8>)>>,
}

/// log2 of the number of ClassMap::front slots.
const FRONT_BITS: u32 = 10;

/// A cheap hash of a name (before mixing): its length, first, middle and
/// last 8 bytes.
#[inline]
fn name_mix(k: &[u8]) -> u64 {
    let n = k.len();
    let w = |i: usize| u64::from_le_bytes(k[i..i + 8].try_into().unwrap());
    let h = if n >= 8 {
        let mut h = w(0) ^ w(n - 8).rotate_left(29);
        if n > 16 {
            h ^= w(n / 2 - 4).rotate_left(47);
        }
        h
    } else if n >= 4 {
        let v = |i: usize| u32::from_le_bytes(k[i..i + 4].try_into().unwrap()) as u64;
        (v(0) << 32) | v(n - 4)
    } else if n > 0 {
        k[0] as u64 | (k[n / 2] as u64) << 8 | (k[n - 1] as u64) << 16
    } else {
        0
    };
    h ^ n as u64
}

/// The ClassMap::front slot of a name.
#[inline]
fn front_slot(k: &[u8]) -> usize {
    (name_mix(k).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (64 - FRONT_BITS)) as usize
}

fn new_front() -> Box<[std::cell::Cell<u32>; 1 << FRONT_BITS]> {
    Box::new(std::array::from_fn(|_| std::cell::Cell::new(0)))
}

/// `a == b` for names of the same length, without a call to memcmp.
#[inline]
fn same_name(a: &[u8], b: &[u8]) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    let w = |s: &[u8], i: usize| u64::from_le_bytes(s[i..i + 8].try_into().unwrap());
    if n >= 8 {
        let mut i = 0;
        while i + 8 < n {
            if w(a, i) != w(b, i) {
                return false;
            }
            i += 8;
        }
        w(a, n - 8) == w(b, n - 8)
    } else if n >= 4 {
        let v = |s: &[u8], i: usize| u32::from_le_bytes(s[i..i + 4].try_into().unwrap());
        v(a, 0) == v(b, 0) && v(a, n - 4) == v(b, n - 4)
    } else if n > 0 {
        a[0] == b[0] && a[n / 2] == b[n / 2] && a[n - 1] == b[n - 1]
    } else {
        true
    }
}

thread_local! {
    static CLASS_MAP_GEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[inline]
fn fresh_gen() -> u64 {
    CLASS_MAP_GEN.with(|g| {
        let n = g.get() + 1;
        g.set(n);
        n
    })
}

impl Default for ClassMap {
    fn default() -> Self {
        ClassMap {
            map: Default::default(),
            stamp: fresh_gen(),
            link_stamp: fresh_gen(),
            front: new_front(),
            missing: Default::default(),
        }
    }
}

impl ClassMap {
    /// See `stamp`.
    #[inline]
    pub fn generation(&self) -> u64 {
        self.stamp
    }

    /// See `link_stamp`.
    #[inline]
    pub fn link_generation(&self) -> u64 {
        self.link_stamp
    }

    /// A change that may alter what the classes in the table are.
    #[inline]
    fn changed(&mut self) {
        self.stamp = fresh_gen();
        self.link_stamp = self.stamp;
    }

    /// An insertion: a new class under its own name only adds to the table
    /// (an alias, or a replaced entry, changes what names resolve to).
    #[inline]
    fn inserted(&mut self, key: &[u8], ce: &ClassEntry, old: bool) {
        if old || !ce.name.eq_ignore_ascii_case(key) {
            self.changed();
        } else {
            self.stamp = fresh_gen();
        }
    }

    /// The entry named `key` (exact bytes), and its index.
    #[inline(never)]
    fn find_entry(&self, key: &[u8]) -> Option<(usize, &Rc<ClassEntry>)> {
        let slot = front_slot(key);
        let i = self.front[slot].get().wrapping_sub(1) as usize;
        if let Some((k, c)) = self.map.get_index(i) {
            if same_name(k, key) {
                return Some((i, c));
            }
        }
        let i = self.find_slow(key, slot)?;
        self.map.get_index(i).map(|(_, c)| (i, c))
    }

    /// The index of the entry named `key` (exact bytes).
    #[inline(always)]
    fn find(&self, key: &[u8]) -> Option<usize> {
        self.find_entry(key).map(|(i, _)| i)
    }

    #[inline(never)]
    fn find_slow(&self, key: &[u8], slot: usize) -> Option<usize> {
        let ms = slot >> 2;
        if let Some((stamp, name)) = self.missing.borrow().get(ms) {
            if *stamp == self.stamp && same_name(name, key) {
                return None;
            }
        }
        let Some(i) = self.map.get_index_of(key) else {
            let mut m = self.missing.borrow_mut();
            if m.is_empty() {
                m.resize((1 << FRONT_BITS) >> 2, (0, Vec::new()));
            }
            let (stamp, name) = &mut m[ms];
            *stamp = self.stamp;
            name.clear();
            name.extend_from_slice(key);
            return None;
        };
        if let Ok(v) = u32::try_from(i + 1) {
            self.front[slot].set(v);
        }
        Some(i)
    }

    #[inline(always)]
    pub fn get<Q: ?Sized + AsRef<[u8]>>(&self, key: &Q) -> Option<&ClassEntry> {
        self.find_entry(key.as_ref()).map(|(_, c)| &**c)
    }

    /// The shared entry (cloning it holds the entry as it is now).
    #[inline(always)]
    pub fn get_rc<Q: ?Sized + AsRef<[u8]>>(&self, key: &Q) -> Option<&Rc<ClassEntry>> {
        self.find_entry(key.as_ref()).map(|(_, c)| c)
    }

    /// The entry, to change (copied first when shared).
    #[inline]
    pub fn get_mut<Q: ?Sized + AsRef<[u8]>>(&mut self, key: &Q) -> Option<&mut ClassEntry> {
        self.changed();
        let i = self.find(key.as_ref())?;
        Some(Rc::make_mut(&mut self.map[i]))
    }

    #[inline]
    pub fn get_full<Q: ?Sized + AsRef<[u8]>>(
        &self,
        key: &Q,
    ) -> Option<(usize, &Vec<u8>, &ClassEntry)> {
        let i = self.find(key.as_ref())?;
        self.map.get_index(i).map(|(k, c)| (i, k, &**c))
    }

    #[inline(always)]
    pub fn get_index_of<Q: ?Sized + AsRef<[u8]>>(&self, key: &Q) -> Option<usize> {
        self.find(key.as_ref())
    }

    #[inline(always)]
    pub fn contains_key<Q: ?Sized + AsRef<[u8]>>(&self, key: &Q) -> bool {
        self.find(key.as_ref()).is_some()
    }

    #[inline]
    pub fn get_index(&self, i: usize) -> Option<(&Vec<u8>, &ClassEntry)> {
        self.map.get_index(i).map(|(k, c)| (k, &**c))
    }

    #[inline]
    pub fn get_index_rc(&self, i: usize) -> Option<(&Vec<u8>, &Rc<ClassEntry>)> {
        self.map.get_index(i)
    }

    /// The entry at `i`, to change (copied first when shared).
    #[inline]
    pub fn get_index_mut(&mut self, i: usize) -> Option<(&Vec<u8>, &mut ClassEntry)> {
        self.changed();
        self.map.get_index_mut(i).map(|(k, c)| (k, Rc::make_mut(c)))
    }

    /// The static property values of the entry at `i`, to change (the
    /// entry copied first when shared). What is derived from the table
    /// (see `stamp`) does not depend on them: the stamps are kept.
    #[inline]
    pub fn static_props_mut(
        &mut self,
        i: usize,
    ) -> Option<&mut indexmap::IndexMap<Vec<u8>, Value>> {
        self.map
            .get_index_mut(i)
            .map(|(_, c)| &mut Rc::make_mut(c).static_properties)
    }

    #[inline]
    pub fn insert(&mut self, key: Vec<u8>, mut ce: ClassEntry) -> Option<Rc<ClassEntry>> {
        if method_attributes_unshared(&ce) {
            share_method_attributes(&mut ce);
        }
        let old = self.map.contains_key(&key);
        self.inserted(&key, &ce, old);
        self.map.insert(key, Rc::new(ce))
    }

    #[inline]
    pub fn insert_rc(&mut self, key: Vec<u8>, mut ce: Rc<ClassEntry>) -> Option<Rc<ClassEntry>> {
        if method_attributes_unshared(&ce) {
            share_method_attributes(Rc::make_mut(&mut ce));
        }
        let old = self.map.contains_key(&key);
        self.inserted(&key, &ce, old);
        self.map.insert(key, ce)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&Vec<u8>, &ClassEntry)> + Clone {
        self.map.iter().map(|(k, c)| (k, &**c))
    }

    #[inline]
    pub fn keys(&self) -> indexmap::map::Keys<'_, Vec<u8>, Rc<ClassEntry>> {
        self.map.keys()
    }

    #[inline]
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &ClassEntry> + Clone {
        self.map.values().map(|c| &**c)
    }

    /// Every entry, to change (each copied first when shared).
    #[inline]
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut ClassEntry> {
        self.changed();
        self.map.values_mut().map(Rc::make_mut)
    }
}

/// Whether a method of `ce` has attributes its code does not carry.
fn method_attributes_unshared(ce: &ClassEntry) -> bool {
    ce.methods
        .values()
        .any(|m| !m.attributes.is_empty() && m.op_array.attributes.is_empty())
}

/// Give the code of each method of `ce` its attributes, as the calls of it
/// do (the deprecation and #[\NoDiscard] checks look at the code's): once,
/// here, instead of a copy of the code by each call.
#[cold]
#[inline(never)]
fn share_method_attributes(ce: &mut ClassEntry) {
    for m in ce.methods.values_mut() {
        if !m.attributes.is_empty() && m.op_array.attributes.is_empty() {
            Rc::make_mut(&mut m.op_array).attributes = m.attributes.clone();
        }
    }
}

impl std::ops::Index<usize> for ClassMap {
    type Output = ClassEntry;
    #[inline]
    fn index(&self, i: usize) -> &ClassEntry {
        &self.map[i]
    }
}

impl<'a> IntoIterator for &'a ClassMap {
    type Item = (&'a Vec<u8>, &'a ClassEntry);
    type IntoIter = std::iter::Map<
        indexmap::map::Iter<'a, Vec<u8>, Rc<ClassEntry>>,
        fn((&'a Vec<u8>, &'a Rc<ClassEntry>)) -> (&'a Vec<u8>, &'a ClassEntry),
    >;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter().map(|(k, c)| (k, &**c))
    }
}

/// A map of names (byte strings) to answers, with a cache in front of
/// its hash lookups: for the memos looked up with the same few keys over
/// and over. Entries are only added (or replaced), until clear().
pub(crate) struct FrontMap<V> {
    /// key -> index in `entries`.
    map: crate::fxhash::NameHashMap<Box<[u8]>, u32>,
    entries: Vec<(Box<[u8]>, V)>,
    /// slot (map_slot()) -> 1 + the index in `entries` of the key last
    /// found with that slot (0: none; empty until the first insertion).
    front: Vec<std::cell::Cell<u32>>,
}

impl<V> std::fmt::Debug for FrontMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FrontMap({} entries)", self.entries.len())
    }
}

impl<V> Default for FrontMap<V> {
    fn default() -> Self {
        FrontMap {
            map: Default::default(),
            entries: Vec::new(),
            front: Vec::new(),
        }
    }
}

/// log2 of the number of FrontMap::front slots.
const FRONT_MAP_BITS: u32 = 11;

#[inline]
fn map_slot(k: &[u8]) -> usize {
    (name_mix(k).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (64 - FRONT_MAP_BITS)) as usize
}

impl<V> FrontMap<V> {
    #[inline]
    pub(crate) fn get(&self, key: &[u8]) -> Option<&V> {
        let slot = map_slot(key);
        if let Some(f) = self.front.get(slot) {
            if let Some((k, v)) = self.entries.get(f.get().wrapping_sub(1) as usize) {
                if same_name(k, key) {
                    return Some(v);
                }
            }
        }
        self.get_slow(slot, key)
    }

    #[inline(never)]
    fn get_slow(&self, slot: usize, key: &[u8]) -> Option<&V> {
        let &i = self.map.get(key)?;
        if let Some(f) = self.front.get(slot) {
            f.set(i + 1);
        }
        self.entries.get(i as usize).map(|e| &e.1)
    }

    pub(crate) fn insert(&mut self, key: Box<[u8]>, v: V) {
        let slot = map_slot(&key);
        let i = match self.map.get(&key) {
            Some(&i) => {
                self.entries[i as usize].1 = v;
                i
            }
            None => {
                let Ok(i) = u32::try_from(self.entries.len()) else {
                    return;
                };
                if i == u32::MAX {
                    return;
                }
                self.map.insert(key.clone(), i);
                self.entries.push((key, v));
                i
            }
        };
        if self.front.is_empty() {
            self.front = (0..1 << FRONT_MAP_BITS)
                .map(|_| Default::default())
                .collect();
        }
        self.front[slot].set(i + 1);
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.entries.clear();
        for f in &self.front {
            f.set(0);
        }
    }
}

/// The ancestry of a class name as the walks over parents see it: the
/// name, then each parent (lowercased), the class table's parent while the
/// class is there with one, else the stub's (at most 65 names).
pub(crate) struct ClassChain {
    pub(crate) names: Vec<Box<[u8]>>,
    /// How many of `names` are reached through the class table's parents
    /// (the first always, each next one while the previous is in the class
    /// table with a parent).
    pub(crate) user_len: usize,
    /// The first of `names[..user_len]` an extension implements natively
    /// (Vm::native_class_handlers, which only grows): (its size + 1 when
    /// found, 0 not yet looked for; the index, usize::MAX for none).
    pub(crate) native: std::cell::Cell<(usize, usize)>,
}

/// Vm::class_chain()'s chains, for one class table generation.
#[derive(Default)]
pub(crate) struct ClassChainMemo {
    stamp: u64,
    map: FrontMap<Rc<ClassChain>>,
}

/// Vm::class_implements_interface()'s answers, for one class table link
/// generation (ClassMap::link_generation()), about classes in the table:
/// class -> interface -> answer. (Also the other memos of relations
/// between two names: see their Vm fields.)
#[derive(Default)]
pub(crate) struct ImplementsMemo {
    stamp: u64,
    /// class -> interface -> index in `answers`.
    map: crate::fxhash::NameHashMap<Vec<u8>, crate::fxhash::NameHashMap<Vec<u8>, u32>>,
    /// (class, interface, answer)
    answers: Vec<(Box<[u8]>, Box<[u8]>, bool)>,
    /// A cache in front of `map`: slot (pair_slot()) -> 1 + the index in
    /// `answers` of the pair last found with that slot (0: none; empty
    /// until the first answer).
    front: Vec<u32>,
}

/// log2 of the number of ImplementsMemo::front slots.
const PAIR_BITS: u32 = 12;

#[inline]
fn pair_slot(class: &[u8], iface: &[u8]) -> usize {
    let h = name_mix(class) ^ name_mix(iface).rotate_left(31);
    (h.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (64 - PAIR_BITS)) as usize
}

impl ImplementsMemo {
    #[inline]
    pub(crate) fn get(&mut self, stamp: u64, class: &[u8], iface: &[u8]) -> Option<bool> {
        if self.stamp != stamp {
            self.reset(stamp);
            return None;
        }
        let slot = pair_slot(class, iface);
        if let Some(&f) = self.front.get(slot) {
            if let Some((c, i, r)) = self.answers.get(f.wrapping_sub(1) as usize) {
                if same_name(c, class) && same_name(i, iface) {
                    return Some(*r);
                }
            }
        }
        self.get_slow(slot, class, iface)
    }

    #[inline(never)]
    fn get_slow(&mut self, slot: usize, class: &[u8], iface: &[u8]) -> Option<bool> {
        let k = *self.map.get(class)?.get(iface)?;
        if let Some(f) = self.front.get_mut(slot) {
            *f = k + 1;
        }
        self.answers.get(k as usize).map(|a| a.2)
    }

    #[inline(never)]
    fn reset(&mut self, stamp: u64) {
        self.map.clear();
        self.answers.clear();
        self.front.fill(0);
        self.stamp = stamp;
    }

    pub(crate) fn set(&mut self, stamp: u64, class: &[u8], iface: &[u8], r: bool) {
        if self.stamp != stamp {
            self.reset(stamp);
        }
        let Ok(k) = u32::try_from(self.answers.len()) else {
            return;
        };
        if k == u32::MAX {
            return;
        }
        let m = match self.map.get_mut(class) {
            Some(m) => m,
            None => self.map.entry(class.to_vec()).or_default(),
        };
        let k = match m.get(iface) {
            Some(&old) => {
                self.answers[old as usize].2 = r;
                old
            }
            None => {
                m.insert(iface.to_vec(), k);
                self.answers.push((class.into(), iface.into(), r));
                k
            }
        };
        if self.front.is_empty() {
            self.front = vec![0; 1 << PAIR_BITS];
        }
        self.front[pair_slot(class, iface)] = k + 1;
    }
}

impl Vm {
    /// The ancestry of `name` (see ClassChain), remembered while the class
    /// table does not change.
    pub(crate) fn class_chain(&self, name: &[u8]) -> Rc<ClassChain> {
        let stamp = self.classes.generation();
        {
            let mut m = self.class_chain_memo.borrow_mut();
            if m.stamp != stamp {
                m.map.clear();
                m.stamp = stamp;
            }
            if let Some(c) = m.map.get(name) {
                return c.clone();
            }
        }
        let mut names: Vec<Box<[u8]>> = vec![name.into()];
        let mut user_len = 1;
        let mut user = true;
        while names.len() < 65 {
            let cur = &names[names.len() - 1];
            let next: Box<[u8]> = match self.classes.get(&**cur).and_then(|c| c.parent.as_ref()) {
                Some(p) => p.to_ascii_lowercase().into(),
                None => {
                    user = false;
                    match crate::vm::get_builtin_parent(cur) {
                        Some(p) => p.to_ascii_lowercase().into(),
                        None => break,
                    }
                }
            };
            names.push(next);
            if user {
                user_len += 1;
            }
        }
        let c = Rc::new(ClassChain {
            names,
            user_len,
            native: std::cell::Cell::new((0, usize::MAX)),
        });
        self.class_chain_memo
            .borrow_mut()
            .map
            .insert(name.into(), c.clone());
        c
    }
}

/// A name lowercased for a lookup, on the stack when short (no allocation
/// for the class names walking a hierarchy meets).
pub(crate) struct LowerName {
    buf: [u8; 128],
    len: usize,
    heap: Vec<u8>,
}

impl LowerName {
    #[inline]
    pub(crate) fn new(s: &[u8]) -> Self {
        let mut n = LowerName {
            buf: [0u8; 128],
            len: 0,
            heap: Vec::new(),
        };
        n.set(s);
        n
    }

    #[inline]
    pub(crate) fn set(&mut self, s: &[u8]) {
        if let Some(out) = self.buf.get_mut(..s.len()) {
            for (o, c) in out.iter_mut().zip(s) {
                *o = c.to_ascii_lowercase();
            }
            self.len = s.len();
        } else {
            self.heap.clear();
            self.heap.extend(s.iter().map(|c| c.to_ascii_lowercase()));
            self.len = usize::MAX;
        }
    }

    #[inline]
    pub(crate) fn get(&self) -> &[u8] {
        if self.len == usize::MAX {
            &self.heap
        } else {
            &self.buf[..self.len]
        }
    }
}

thread_local! {
    /// The anonymous classes whose declaration ran (ZEND_DECLARE_ANON_CLASS).
    static ANON_BOUND: std::cell::RefCell<std::collections::HashSet<Vec<u8>>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// ZEND_DECLARE_ANON_CLASS: the anonymous class `name` is bound.
#[cold]
pub fn bind_anon_class(name: &[u8]) {
    ANON_BOUND.with(|b| {
        let mut b = b.borrow_mut();
        if !b.contains(name) {
            b.insert(name.to_vec());
        }
    });
}
