//! foreach over a plain object's property table (ZEND_FE_FETCH_R/RW on an
//! object without get_iterator): the live table is walked, so properties
//! added during the loop are visited and removed ones are skipped.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{PhpObject, Visibility};
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
        b"__weak_ref_id",
        b"__sxml_",
        b"__hash_",
        b"__hmac_",
        b"__pkey_id",
        b"__cert_id",
        b"__csr_id",
        b"__sensitive_value",
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
        {
            let cl = obj.borrow().class_name.to_ascii_lowercase();
            if self.class_has_hooked_props(&cl) {
                return self.foreach_hooked_step(obj, pos, cur, by_ref);
            }
        }
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
            // A dynamic property an inherited private one moved away from
            // (Vm::prop_access_key): zend_check_property_access() decides.
            let moved_from = self.class_layout_of(&class_name).slots.iter().any(|sl| {
                sl.storage_key == name
                    && sl
                        .moved_key
                        .as_ref()
                        .is_some_and(|mk| obj.borrow().has_property(mk))
            });
            let key = if moved_from {
                if !self.check_property_access(&class_lower, &name, true) {
                    continue;
                }
                name.clone()
            } else {
                match self.foreach_prop_key(&class_lower, scope_lower.as_deref(), &name) {
                    Some(k) => k,
                    None => continue,
                }
            };
            let value = if by_ref {
                match value {
                    Value::Reference(r) => {
                        // The loop variable binds the slot: a typed property
                        // types the reference.
                        self.typed_ref_register_slot(obj, &name, &r);
                        Value::Reference(r)
                    }
                    other => {
                        if let Some((_, decl, true, _, _)) =
                            self.find_property_def(&class_lower, &key)
                        {
                            let decl_lower: Vec<u8> =
                                decl.iter().map(|b| b.to_ascii_lowercase()).collect();
                            let disp = self
                                .class_name_for_display(&decl_lower)
                                .map(|n| crate::value::display_class_name(&n))
                                .unwrap_or_else(|| crate::value::display_class_name(&class_name));
                            return Err(format!(
                                "Cannot acquire reference to readonly property {}::${}",
                                disp,
                                String::from_utf8_lossy(&key)
                            ));
                        }
                        let r = Rc::new(RefCell::new(other));
                        {
                            let mut ob = obj.borrow_mut();
                            if let Some(slot) = ob.properties.get_mut(i - 1) {
                                slot.1 = Value::Reference(r.clone());
                            }
                        }
                        self.typed_ref_register_slot(obj, &name, &r);
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

    /// foreach over an object with hooked properties
    /// (zend_hooked_object_get_iterator): the declared properties visible
    /// from the current scope in declaration order (parents first), read
    /// when fetched (get hooks run per iteration; by reference only through
    /// `&get` hooks), then the dynamic properties of the live table.
    /// Positions below the number of declared properties index them; the
    /// rest index the property table. `Err("")` means the exception is set.
    fn foreach_hooked_step(
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
            (ob.class_name.to_ascii_lowercase(), ob.class_name.clone())
        };
        let defs = self.ordered_instance_prop_defs(&class_lower);
        let n = defs.len();
        let obj_val = Value::Object(obj.clone());
        let mut i = pos;
        while i < n {
            let d = defs[i].clone();
            i += 1;
            let accessible = match d.visibility {
                Visibility::Public => true,
                Visibility::Protected => scope_lower
                    .as_deref()
                    .is_some_and(|s| self.scope_can_see_protected(s, &class_lower)),
                Visibility::Private => scope_lower
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&d.declaring_class)),
            };
            if !accessible {
                continue;
            }
            let decl_lower = d.declaring_class.to_ascii_lowercase();
            // From a parent's scope, its own private property shadows a
            // child's redeclaration of the name.
            if d.visibility != Visibility::Private
                && scope_lower.as_deref().is_some_and(|s| {
                    s != decl_lower.as_slice()
                        && defs.iter().any(|o| {
                            o.name == d.name
                                && o.visibility == Visibility::Private
                                && o.declaring_class.eq_ignore_ascii_case(s)
                        })
                })
            {
                continue;
            }
            // The slot this scope's access to the name refers to.
            let storage = self.prop_key_for(&obj_val, &d.name).into_owned();
            let value = if d.has_get_hook || d.has_set_hook {
                if d.is_virtual && !d.has_get_hook {
                    continue;
                }
                let mut m = b"__property_get_".to_vec();
                m.extend(d.name.iter().map(|b| b.to_ascii_lowercase()));
                let get_by_ref = d.has_get_hook
                    && self
                        .classes
                        .get(&decl_lower)
                        .and_then(|c| c.get_method(&m))
                        .is_some_and(|md| md.op_array.returns_by_ref);
                let run_hook = d.has_get_hook && self.has_unguarded_get_hook(&class_lower, &d.name);
                if by_ref && !get_by_ref {
                    return Err(format!(
                        "Cannot create reference to property {}::${}",
                        crate::value::display_class_name(&class_name),
                        String::from_utf8_lossy(&d.name)
                    ));
                }
                if run_hook && by_ref {
                    let v = self.call_prop_get_hook(&obj_val, &class_lower, &d.name);
                    if self.current_exception.is_some() {
                        return Err(String::new());
                    }
                    match v {
                        Value::Reference(r) => Value::Reference(r),
                        other => Value::Reference(Rc::new(RefCell::new(other))),
                    }
                } else if run_hook {
                    let r = self.call_get_hook_if_accessible(obj, &d.name);
                    if self.current_exception.is_some() {
                        return Err(String::new());
                    }
                    match r {
                        Some(Ok(v)) => v.deref(),
                        Some(Err(e)) => return Err(e.message),
                        None => continue,
                    }
                } else {
                    match obj.borrow().get_property(&storage) {
                        Value::Undef => continue,
                        v => v.deref(),
                    }
                }
            } else {
                let slot = obj
                    .borrow()
                    .properties
                    .iter()
                    .position(|(k, _)| *k == storage);
                let Some(si) = slot else {
                    continue;
                };
                let v = obj.borrow().properties[si].1.clone();
                match v {
                    Value::Undef => continue,
                    Value::Reference(r) if by_ref => {
                        self.typed_ref_register_slot(obj, &storage, &r);
                        Value::Reference(r)
                    }
                    other if by_ref => {
                        if d.is_readonly {
                            return Err(format!(
                                "Cannot acquire reference to readonly property {}::${}",
                                crate::value::display_class_name(
                                    &self
                                        .class_name_for_display(&decl_lower)
                                        .unwrap_or_else(|| d.declaring_class.clone())
                                ),
                                String::from_utf8_lossy(&d.name)
                            ));
                        }
                        let r = Rc::new(RefCell::new(other));
                        obj.borrow_mut().properties[si].1 = Value::Reference(r.clone());
                        self.typed_ref_register_slot(obj, &storage, &r);
                        Value::Reference(r)
                    }
                    other => other.deref(),
                }
            };
            return Ok(Some(ObjIterStep {
                next_pos: i,
                name: storage,
                key: d.name.clone(),
                value,
            }));
        }
        // Dynamic properties.
        // The slots of declared properties (a private one moved out of the
        // way of a dynamic property of its name lives under its mangled key).
        let declared: Vec<Vec<u8>> = {
            let layout = self.class_layout_of(&class_name);
            let ob = obj.borrow();
            layout
                .slots
                .iter()
                .map(|sl| match &sl.moved_key {
                    Some(mk) if ob.has_property(mk) => mk.clone(),
                    _ => sl.storage_key.clone(),
                })
                .collect()
        };
        let mut j = {
            let ob = obj.borrow();
            let j = pos.saturating_sub(n);
            match cur {
                Some(c) if j > 0 => {
                    if ob.properties.get(j - 1).map(|(k, _)| k.as_slice()) == Some(c) {
                        j
                    } else {
                        ob.properties
                            .iter()
                            .position(|(k, _)| k.as_slice() == c)
                            .map(|p| p + 1)
                            .unwrap_or(j - 1)
                    }
                }
                _ => j,
            }
        };
        loop {
            let (name, value) = {
                let ob = obj.borrow();
                match ob.properties.get(j) {
                    Some((k, v)) => (k.clone(), v.clone()),
                    None => return Ok(None),
                }
            };
            j += 1;
            if is_internal_slot(&name) || matches!(value, Value::Undef) || declared.contains(&name)
            {
                continue;
            }
            let Some(key) = self.foreach_prop_key(&class_lower, scope_lower.as_deref(), &name)
            else {
                continue;
            };
            // zend_check_property_access(): a dynamic property named like a
            // non-public declared property the scope sees is not visited.
            if name.first() != Some(&0)
                && defs.iter().any(|d| {
                    d.name == name
                        && match d.visibility {
                            Visibility::Public => false,
                            Visibility::Protected => scope_lower
                                .as_deref()
                                .is_some_and(|s| self.scope_can_see_protected(s, &class_lower)),
                            Visibility::Private => scope_lower
                                .as_deref()
                                .is_some_and(|s| s.eq_ignore_ascii_case(&d.declaring_class)),
                        }
                })
            {
                continue;
            }
            let value = match value {
                Value::Reference(r) if by_ref => Value::Reference(r),
                other if by_ref => {
                    let r = Rc::new(RefCell::new(other));
                    if let Some(slot) = obj.borrow_mut().properties.get_mut(j - 1) {
                        slot.1 = Value::Reference(r.clone());
                    }
                    Value::Reference(r)
                }
                other => other.deref(),
            };
            return Ok(Some(ObjIterStep {
                next_pos: n + j,
                name,
                key,
                value,
            }));
        }
    }

    /// The reference bound to property slot `name` of `obj`, as a
    /// by-reference iteration binds it (array_walk() on an object): the
    /// slot becomes a reference typed by the property. None when there is
    /// no such slot.
    pub fn object_slot_ref(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        name: &[u8],
    ) -> Option<Rc<RefCell<Value>>> {
        let r = {
            let mut ob = obj.borrow_mut();
            let slot = ob.properties.iter_mut().find(|(k, _)| k == name)?;
            match &slot.1 {
                Value::Reference(r) => r.clone(),
                v => {
                    let r = Rc::new(RefCell::new(v.clone()));
                    slot.1 = Value::Reference(r.clone());
                    r
                }
            }
        };
        self.typed_ref_register_slot(obj, name, &r);
        Some(r)
    }

    /// Undo object_slot_ref() once nothing but the slot holds the reference.
    pub fn object_slot_unref(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        name: &[u8],
        r: &Rc<RefCell<Value>>,
    ) {
        if Rc::strong_count(r) != 2 {
            return;
        }
        let mut ob = obj.borrow_mut();
        if let Some(slot) = ob.properties.iter_mut().find(|(k, _)| k == name) {
            if matches!(&slot.1, Value::Reference(sr) if Rc::ptr_eq(sr, r)) {
                slot.1 = r.borrow().clone();
            }
        }
    }
}
