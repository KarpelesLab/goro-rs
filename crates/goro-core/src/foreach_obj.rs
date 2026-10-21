//! foreach over a plain object's property table (ZEND_FE_FETCH_R/RW on an
//! object without get_iterator): the live table is walked, so properties
//! added during the loop are visited and removed ones are skipped.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::PhpObject;
use crate::value::Value;
use crate::vm::Vm;

/// One fetched element of a plain-object foreach.
pub(crate) struct ObjIterStep {
    /// Position to resume from (index after the fetched slot).
    pub next_pos: usize,
    /// Storage name of the fetched slot (to find it again).
    pub name: Vec<u8>,
    /// Key as seen by PHP code (demangled property name).
    pub key: Vec<u8>,
    /// The value (by-value) or the slot's reference (by-ref).
    pub value: Value,
}

/// Internal bookkeeping slots that are not PHP properties.
pub(crate) fn is_internal_slot(name: &[u8]) -> bool {
    const PREFIXES: &[&[u8]] = &[
        b"__spl_",
        b"__reflection_",
        b"__enum_",
        b"__ctor_",
        b"__clone_",
        b"__destructed",
        b"__fiber_",
        b"__timestamp",
        b"__lazy_",
        b"__uri_",
        b"__weak_ref_id",
        b"__sxml_",
        b"__hash_",
        b"__hmac_",
        b"__pkey_id",
        b"__cert_id",
        b"__csr_id",
    ];
    name.starts_with(b"__") && PREFIXES.iter().any(|p| name.starts_with(p))
}

impl Vm {
    /// The foreach key of property slot `name` of an object of class
    /// `obj_class_lower` seen from `scope_lower`, or None when the slot is
    /// not visible there (zend_check_property_access).
    pub(crate) fn foreach_prop_key(
        &self,
        obj_class_lower: &[u8],
        scope_lower: Option<&[u8]>,
        name: &[u8],
    ) -> Option<Vec<u8>> {
        if name.first() == Some(&0) {
            // Mangled \0Class\0name (private) or \0*\0name (protected).
            let sep = name[1..].iter().position(|&b| b == 0)?;
            let decl = &name[1..1 + sep];
            let bare = &name[1 + sep + 1..];
            // A dynamic property merely named like a mangled one (e.g. from
            // an array cast) is accessible (zend_check_property_access).
            let declared = if decl == b"*" {
                self.find_instance_prop_def(obj_class_lower, bare).is_some()
            } else {
                let decl_lower: Vec<u8> = decl.iter().map(|b| b.to_ascii_lowercase()).collect();
                self.classes
                    .get(&decl_lower)
                    .map(|c| c.properties.iter().any(|p| !p.is_static && p.name == bare))
                    .unwrap_or(false)
            };
            if !declared {
                return Some(bare.to_vec());
            }
            if decl == b"*" {
                let s = scope_lower?;
                if !self.scope_can_see_protected(s, obj_class_lower) {
                    return None;
                }
            } else {
                let decl_lower: Vec<u8> = decl.iter().map(|b| b.to_ascii_lowercase()).collect();
                if scope_lower != Some(decl_lower.as_slice()) {
                    return None;
                }
            }
            return Some(bare.to_vec());
        }
        // From a parent's scope, its own private property shadows a child's
        // redeclaration of the same name (that slot is not accessible).
        if let Some(scope) = scope_lower {
            if scope != obj_class_lower
                && self.is_subclass_of(obj_class_lower, scope)
                && self
                    .classes
                    .get(scope)
                    .map(|c| {
                        c.properties.iter().any(|p| {
                            !p.is_static
                                && p.name == name
                                && p.visibility == crate::object::Visibility::Private
                        })
                    })
                    .unwrap_or(false)
                && self
                    .find_instance_prop_def(obj_class_lower, name)
                    .map(|(_, decl)| decl.to_ascii_lowercase() != scope)
                    .unwrap_or(false)
            {
                return None;
            }
        }
        if let Some((vis, decl)) = self.find_instance_prop_def(obj_class_lower, name) {
            match vis {
                crate::object::Visibility::Public => {}
                crate::object::Visibility::Protected => {
                    let s = scope_lower?;
                    if !self.scope_can_see_protected(s, obj_class_lower) {
                        return None;
                    }
                }
                crate::object::Visibility::Private => {
                    let decl_lower: Vec<u8> = decl.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if scope_lower != Some(decl_lower.as_slice()) {
                        return None;
                    }
                }
            }
        }
        Some(name.to_vec())
    }

    /// Fetch the next visible property of `obj` for a foreach. `pos` is the
    /// position after the previously fetched slot named `cur`; when slots
    /// before it were removed or added the slot is found again by name, and
    /// when it was itself removed the slot that took its place comes next.
    /// By-ref fetches turn the slot into a reference. `Err` carries an Error
    /// message to throw.
    pub(crate) fn foreach_obj_step(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        pos: usize,
        cur: Option<&[u8]>,
        by_ref: bool,
    ) -> Result<Option<ObjIterStep>, String> {
        let scope_lower: Option<Vec<u8>> = self
            .current_class_scope()
            .map(|n| n.iter().map(|b| b.to_ascii_lowercase()).collect());
        let (class_lower, class_name) = {
            let ob = obj.borrow();
            (
                ob.class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect::<Vec<u8>>(),
                ob.class_name.clone(),
            )
        };
        let mut i = {
            let ob = obj.borrow();
            match cur {
                Some(c) if pos > 0 => {
                    if ob.properties.get(pos - 1).map(|(n, _)| n.as_slice()) == Some(c) {
                        pos
                    } else {
                        ob.properties
                            .iter()
                            .position(|(n, _)| n.as_slice() == c)
                            .map(|p| p + 1)
                            .unwrap_or(pos - 1)
                    }
                }
                _ => pos,
            }
        };
        loop {
            let (name, value) = {
                let ob = obj.borrow();
                match ob.properties.get(i) {
                    Some((n, v)) => (n.clone(), v.clone()),
                    None => return Ok(None),
                }
            };
            i += 1;
            if is_internal_slot(&name) || matches!(value, Value::Undef) {
                continue;
            }
            let Some(key) = self.foreach_prop_key(&class_lower, scope_lower.as_deref(), &name)
            else {
                continue;
            };
            let value = if by_ref {
                match value {
                    Value::Reference(r) => Value::Reference(r),
                    other => {
                        if let Some((_, decl, true, _, _)) =
                            self.find_property_def(&class_lower, &key)
                        {
                            let decl_lower: Vec<u8> =
                                decl.iter().map(|b| b.to_ascii_lowercase()).collect();
                            let disp = self
                                .classes
                                .get(&decl_lower)
                                .map(|c| crate::value::display_class_name(&c.name))
                                .unwrap_or_else(|| crate::value::display_class_name(&class_name));
                            return Err(format!(
                                "Cannot acquire reference to readonly property {}::${}",
                                disp,
                                String::from_utf8_lossy(&key)
                            ));
                        }
                        let r = Rc::new(RefCell::new(other));
                        let mut ob = obj.borrow_mut();
                        if let Some(slot) = ob.properties.get_mut(i - 1) {
                            slot.1 = Value::Reference(r.clone());
                        }
                        Value::Reference(r)
                    }
                }
            } else {
                value.deref()
            };
            return Ok(Some(ObjIterStep {
                next_pos: i,
                name,
                key,
                value,
            }));
        }
    }
}
