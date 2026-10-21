//! An object's property table as php-src sees it (zend_std_get_properties):
//! the declared property slots in declaration (slot) order — the inherited
//! ones first — followed by the dynamic properties in insertion order, each
//! with its mangled name (`\0Class\0name` for private, `\0*\0name` for
//! protected). Used by get_object_vars(), get_mangled_object_vars() and
//! `(array)` casts, plus the scope-dependent property lookup of
//! zend_get_property_info() / zend_check_property_access().

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{PhpObject, Visibility};
use crate::value::Value;
use crate::vm::Vm;

/// A declared property slot of a class.
#[derive(Debug, Clone)]
pub struct PropSlot {
    pub name: Vec<u8>,
    /// Lowercase declaring class
    pub decl: Vec<u8>,
    /// Declaring class name as written (`ce->name`)
    pub decl_name: Vec<u8>,
    pub vis: Visibility,
    pub has_get_hook: bool,
    pub is_virtual: bool,
    /// Key of the slot in `PhpObject::properties`
    pub storage_key: Vec<u8>,
}

impl PropSlot {
    /// The property table key (zend_mangle_property_name).
    pub fn mangled(&self) -> Vec<u8> {
        mangle(&self.name, self.vis, &self.decl_name)
    }
}

fn mangle(name: &[u8], vis: Visibility, decl_name: &[u8]) -> Vec<u8> {
    match vis {
        Visibility::Public => name.to_vec(),
        Visibility::Protected => {
            let mut m = Vec::with_capacity(3 + name.len());
            m.extend_from_slice(b"\0*\0");
            m.extend_from_slice(name);
            m
        }
        Visibility::Private => {
            let mut m = Vec::with_capacity(2 + decl_name.len() + name.len());
            m.push(0);
            m.extend_from_slice(decl_name);
            m.push(0);
            m.extend_from_slice(name);
            m
        }
    }
}

/// One entry of an object's property table.
#[derive(Debug, Clone)]
pub struct PropEntry {
    /// The property table key (mangled for declared private/protected)
    pub key: Vec<u8>,
    pub value: Value,
    /// The declared slot (None: a dynamic property)
    pub slot: Option<PropSlot>,
}

/// VM bookkeeping stored among the properties.
pub fn is_internal_prop_name(name: &[u8]) -> bool {
    const P: &[&[u8]] = &[
        b"__spl_",
        b"__reflection_",
        b"__timestamp",
        b"__enum_",
        b"__fiber_",
        b"__ctor_",
        b"__clone_",
        b"__destructed",
        b"__weak_ref_id",
        b"__sxml_",
        b"__lazy_",
        b"__uri_",
        b"__hash_",
        b"__hmac_",
        b"__pkey_id",
        b"__cert_id",
        b"__csr_id",
        b"__xml_",
    ];
    name.starts_with(b"__") && P.iter().any(|p| name.starts_with(p))
}

fn lower(b: &[u8]) -> Vec<u8> {
    b.to_ascii_lowercase()
}

impl Vm {
    fn class_display_name(&self, cl: &[u8]) -> Vec<u8> {
        if let Some(c) = self.classes.get(cl) {
            return c.name.clone();
        }
        if let Some(c) = crate::stubs::find_class(cl) {
            return c.name.as_bytes().to_vec();
        }
        cl.to_vec()
    }

    /// The declared (non-static) property slots of class `cl` (lowercase),
    /// in slot order.
    pub fn object_prop_slots(&self, cl: &[u8]) -> Vec<PropSlot> {
        let mut slots: Vec<PropSlot> = Vec::new();
        // The internal ancestor (or the class itself, when internal).
        let mut cur = cl.to_vec();
        let mut internal = None;
        for _ in 0..64 {
            match self.classes.get(&cur) {
                Some(c) => match &c.parent {
                    Some(p) => cur = lower(p),
                    None => break,
                },
                None => {
                    internal = crate::stubs::find_class(&cur);
                    break;
                }
            }
        }
        if let Some(ic) = internal {
            let mut chain = vec![ic];
            while let Some(p) = chain
                .last()
                .and_then(|c| c.parent)
                .and_then(|p| crate::stubs::find_class(p.as_bytes()))
            {
                if chain.len() > 32 {
                    break;
                }
                chain.push(p);
            }
            for c in chain.iter().rev() {
                for p in c.props {
                    use crate::stubs::{F_PRIVATE, F_PROTECTED, F_STATIC, F_VIRTUAL};
                    if p.flags & F_STATIC != 0 {
                        continue;
                    }
                    let vis = if p.flags & F_PRIVATE != 0 {
                        Visibility::Private
                    } else if p.flags & F_PROTECTED != 0 {
                        Visibility::Protected
                    } else {
                        Visibility::Public
                    };
                    let name = p.name.as_bytes().to_vec();
                    let slot = PropSlot {
                        decl: lower(c.name.as_bytes()),
                        decl_name: c.name.as_bytes().to_vec(),
                        vis,
                        has_get_hook: p.flags & F_VIRTUAL != 0,
                        is_virtual: p.flags & F_VIRTUAL != 0,
                        storage_key: name.clone(),
                        name,
                    };
                    match slots
                        .iter_mut()
                        .find(|s| s.name == slot.name && s.vis != Visibility::Private)
                    {
                        Some(s) if vis != Visibility::Private => *s = slot,
                        _ => slots.push(slot),
                    }
                }
            }
        }
        if let Some(ce) = self.classes.get(cl) {
            // Inherited slots come first: order by the declaring class depth
            // (root first), keeping the declaration order within a class.
            let depth = |d: &[u8]| -> usize {
                let mut n = 0;
                let mut c = d.to_vec();
                while let Some(p) = self.classes.get(&c).and_then(|e| e.parent.as_ref()) {
                    n += 1;
                    c = lower(p);
                    if n > 64 {
                        break;
                    }
                }
                n
            };
            // A redeclared (non-private) property keeps the slot of its
            // topmost declaration.
            let mut defs: Vec<(usize, usize, &crate::object::PropertyDef)> = ce
                .properties
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.is_static)
                .map(|(i, p)| {
                    let decl = lower(&p.declaring_class);
                    let root = if p.visibility == Visibility::Private {
                        decl
                    } else {
                        self.prop_root_of(&decl, &p.name)
                    };
                    (depth(&root), i, p)
                })
                .collect();
            defs.sort_by_key(|(d, i, _)| (*d, *i));
            for (_, _, p) in defs {
                let decl = lower(&p.declaring_class);
                let slot = PropSlot {
                    name: p.name.clone(),
                    decl_name: self.class_display_name(&decl),
                    decl,
                    vis: p.visibility,
                    has_get_hook: p.has_get_hook,
                    is_virtual: p.is_virtual,
                    storage_key: p.name.clone(),
                };
                // A redeclared non-private property reuses the inherited slot.
                match slots.iter_mut().find(|s| {
                    s.name == slot.name
                        && s.vis != Visibility::Private
                        && slot.vis != Visibility::Private
                }) {
                    Some(s) => {
                        let at = s.storage_key.clone();
                        *s = slot;
                        s.storage_key = at;
                    }
                    None => {
                        if !slots
                            .iter()
                            .any(|s| s.name == slot.name && s.decl == slot.decl)
                        {
                            slots.push(slot)
                        }
                    }
                }
            }
            // Property_storage_key(): a private property sharing its name with
            // another slot is stored under its mangled name.
            let names: Vec<(Vec<u8>, Vec<u8>)> = slots
                .iter()
                .map(|s| (s.name.clone(), s.decl.clone()))
                .collect();
            for s in slots.iter_mut() {
                if s.vis == Visibility::Private
                    && names.iter().any(|(n, d)| *n == s.name && *d != s.decl)
                {
                    s.storage_key = mangle(&s.name, Visibility::Private, &s.decl_name);
                }
            }
        }
        slots
    }

    /// The property table of `obj` (zend_std_get_properties): declared slots
    /// in slot order (uninitialized ones skipped, except hooked properties,
    /// which have no backing value to show), then dynamic properties.
    pub fn object_prop_table(&self, obj: &Rc<RefCell<PhpObject>>) -> Vec<PropEntry> {
        let ob = obj.borrow();
        let cl = lower(&ob.class_name);
        let slots = self.object_prop_slots(&cl);
        let props = &ob.properties;
        let mut used = vec![false; props.len()];
        let mut out = Vec::with_capacity(props.len());
        for slot in slots {
            match props.iter().position(|(k, _)| *k == slot.storage_key) {
                Some(i) => {
                    used[i] = true;
                    if matches!(props[i].1, Value::Undef) && !slot.has_get_hook {
                        continue;
                    }
                    out.push(PropEntry {
                        key: slot.mangled(),
                        value: props[i].1.clone(),
                        slot: Some(slot),
                    });
                }
                None if slot.has_get_hook => out.push(PropEntry {
                    key: slot.mangled(),
                    value: Value::Undef,
                    slot: Some(slot),
                }),
                None => {}
            }
        }
        for (i, (k, v)) in props.iter().enumerate() {
            if used[i] || is_internal_prop_name(k) || matches!(v, Value::Undef) {
                continue;
            }
            out.push(PropEntry {
                key: k.clone(),
                value: v.clone(),
                slot: None,
            });
        }
        out
    }

    /// The property info zend_get_property_info() finds for `name` on class
    /// `cl` from the current scope: Ok(Some(slot)) when accessible, Ok(None)
    /// when the name is a dynamic property, Err(()) when inaccessible
    /// (ZEND_WRONG_PROPERTY_INFO).
    pub(crate) fn scoped_prop_info(&self, cl: &[u8], name: &[u8]) -> Result<Option<PropSlot>, ()> {
        let slots = self.object_prop_slots(cl);
        let scope = self.get_current_class_name().map(|s| lower(&s));
        // The class's own view: the most derived declaration of the name.
        let Some(info) = slots
            .iter()
            .filter(|s| s.name == name)
            .max_by_key(|s| self.class_depth(&s.decl))
            .cloned()
        else {
            return if name.first() == Some(&0) && !name.is_empty() {
                Err(())
            } else {
                Ok(None)
            };
        };
        if info.vis == Visibility::Public && !self.prop_changed(&slots, &info) {
            return Ok(Some(info));
        }
        if scope.as_deref() == Some(info.decl.as_slice()) {
            return Ok(Some(info));
        }
        if self.prop_changed(&slots, &info) {
            // zend_get_parent_private_property()
            if let Some(sc) = &scope {
                if sc.as_slice() != cl && self.class_extends(cl, sc) {
                    if let Some(p) = slots
                        .iter()
                        .find(|s| s.name == name && s.vis == Visibility::Private && s.decl == *sc)
                    {
                        return Ok(Some(p.clone()));
                    }
                }
            }
            if info.vis == Visibility::Public {
                return Ok(Some(info));
            }
        }
        match info.vis {
            Visibility::Private => {
                if info.decl.as_slice() != cl {
                    Ok(None)
                } else {
                    Err(())
                }
            }
            _ => {
                let root = self.prop_root_class(&slots, &info);
                let ok = scope.as_ref().is_some_and(|sc| {
                    sc.as_slice() == root.as_slice()
                        || self.class_extends(sc, &root)
                        || self.class_extends(&root, sc)
                });
                if ok { Ok(Some(info)) } else { Err(()) }
            }
        }
    }

    fn class_depth(&self, cl: &[u8]) -> usize {
        let mut n = 0;
        let mut c = cl.to_vec();
        loop {
            let p = match self.classes.get(&c) {
                Some(e) => e.parent.as_ref().map(|p| lower(p)),
                None => crate::stubs::find_class(&c)
                    .and_then(|s| s.parent)
                    .map(|p| lower(p.as_bytes())),
            };
            match p {
                Some(p) if n < 64 => {
                    n += 1;
                    c = p;
                }
                _ => return n,
            }
        }
    }

    /// ZEND_ACC_CHANGED: the property redeclares an ancestor's private one.
    fn prop_changed(&self, slots: &[PropSlot], info: &PropSlot) -> bool {
        slots
            .iter()
            .any(|s| s.name == info.name && s.decl != info.decl && s.vis == Visibility::Private)
    }

    /// The class of the property's prototype (its topmost non-private declaration).
    fn prop_root_class(&self, _slots: &[PropSlot], info: &PropSlot) -> Vec<u8> {
        self.prop_root_of(&info.decl, &info.name)
    }

    /// The topmost ancestor of `decl` declaring non-private property `name`.
    fn prop_root_of(&self, decl: &[u8], name: &[u8]) -> Vec<u8> {
        let mut root = decl.to_vec();
        let mut cur = decl.to_vec();
        for _ in 0..64 {
            let Some(p) = self.classes.get(&cur).and_then(|c| c.parent.as_ref()) else {
                break;
            };
            let p = lower(p);
            let declares = self.classes.get(&p).is_some_and(|c| {
                c.properties.iter().any(|d| {
                    d.name == name
                        && !d.is_static
                        && d.visibility != Visibility::Private
                        && lower(&d.declaring_class) == p
                })
            });
            if declares {
                root = p.clone();
            }
            cur = p;
        }
        root
    }

    /// zend_check_property_access() for property table key `key`.
    pub fn check_property_access(&self, cl: &[u8], key: &[u8], is_dynamic: bool) -> bool {
        if key.first() == Some(&0) {
            if is_dynamic {
                return true;
            }
            let rest = &key[1..];
            let Some(sep) = rest.iter().position(|&b| b == 0) else {
                return false;
            };
            let class_name = &rest[..sep];
            let prop_name = &rest[sep + 1..];
            let info = match self.scoped_prop_info(cl, prop_name) {
                Ok(Some(i)) => i,
                _ => return false,
            };
            if class_name != b"*" {
                info.vis == Visibility::Private && info.mangled() == key
            } else {
                info.vis != Visibility::Private
            }
        } else {
            match self.scoped_prop_info(cl, key) {
                Ok(None) => true,
                Err(()) => false,
                Ok(Some(i)) => i.vis == Visibility::Public,
            }
        }
    }
}
