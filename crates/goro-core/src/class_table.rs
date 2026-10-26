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
use crate::vm::Vm;
use std::hash::Hash;
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
            // Aliases are listed under their (lowercased) alias name.
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
        }
    }
}

impl ClassMap {
    /// See `stamp`.
    #[inline]
    pub fn generation(&self) -> u64 {
        self.stamp
    }

    #[inline]
    pub fn get<Q: ?Sized + Hash + indexmap::Equivalent<Vec<u8>>>(
        &self,
        key: &Q,
    ) -> Option<&ClassEntry> {
        self.map.get(key).map(|c| &**c)
    }

    /// The shared entry (cloning it holds the entry as it is now).
    #[inline]
    pub fn get_rc<Q: ?Sized + Hash + indexmap::Equivalent<Vec<u8>>>(
        &self,
        key: &Q,
    ) -> Option<&Rc<ClassEntry>> {
        self.map.get(key)
    }

    /// The entry, to change (copied first when shared).
    #[inline]
    pub fn get_mut<Q: ?Sized + Hash + indexmap::Equivalent<Vec<u8>>>(
        &mut self,
        key: &Q,
    ) -> Option<&mut ClassEntry> {
        self.stamp = fresh_gen();
        self.map.get_mut(key).map(Rc::make_mut)
    }

    #[inline]
    pub fn get_full<Q: ?Sized + Hash + indexmap::Equivalent<Vec<u8>>>(
        &self,
        key: &Q,
    ) -> Option<(usize, &Vec<u8>, &ClassEntry)> {
        self.map.get_full(key).map(|(i, k, c)| (i, k, &**c))
    }

    #[inline]
    pub fn get_index_of<Q: ?Sized + Hash + indexmap::Equivalent<Vec<u8>>>(
        &self,
        key: &Q,
    ) -> Option<usize> {
        self.map.get_index_of(key)
    }

    #[inline]
    pub fn contains_key<Q: ?Sized + Hash + indexmap::Equivalent<Vec<u8>>>(&self, key: &Q) -> bool {
        self.map.contains_key(key)
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
        self.stamp = fresh_gen();
        self.map.get_index_mut(i).map(|(k, c)| (k, Rc::make_mut(c)))
    }

    #[inline]
    pub fn insert(&mut self, key: Vec<u8>, ce: ClassEntry) -> Option<Rc<ClassEntry>> {
        self.stamp = fresh_gen();
        self.map.insert(key, Rc::new(ce))
    }

    #[inline]
    pub fn insert_rc(&mut self, key: Vec<u8>, ce: Rc<ClassEntry>) -> Option<Rc<ClassEntry>> {
        self.stamp = fresh_gen();
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
        self.stamp = fresh_gen();
        self.map.values_mut().map(Rc::make_mut)
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
    map: crate::fxhash::FxHashMap<Vec<u8>, Rc<ClassChain>>,
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
            .insert(name.to_vec(), c.clone());
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
