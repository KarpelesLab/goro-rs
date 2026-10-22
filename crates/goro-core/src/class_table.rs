//! The class table as PHP code sees it (EG(class_table)): the classes in
//! `vm.classes` (user classes, plus internal classes that extensions register
//! with a `ClassEntry`) and the internal classes the VM implements natively,
//! which are described by the stubs (`stub_sigs::CLASSES`).
//!
//! A stub class only counts as declared when its extension is enabled, so
//! e.g. tests skipping on `class_exists('IntlDateFormatter')` keep skipping.
//! Lookups never autoload.

use crate::stubs::{self, K_ENUM, K_INTERFACE, K_TRAIT, StubClass};
use crate::vm::Vm;

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
        self.stub_ext_enabled(c.ext).then_some(c)
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
            if want.contains(&ClassKind::of_stub(c)) && self.stub_ext_enabled(c.ext) {
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
