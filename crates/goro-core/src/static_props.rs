//! Static property access: `C::$p`, `static::$p`, `$obj::$p`.
//!
//! Mirrors php-src's zend_std_get_static_property_with_info() and
//! zend_fetch_static_property_address(): the property is looked up on the
//! accessed class, its visibility checked against the calling scope, and the
//! slot located on the declaring class. A child class shares its parent's
//! slot unless it redeclares the property; a class using a trait gets its own
//! copy (the trait binding inserts it into the class's own map).

use std::cell::RefCell;
use std::rc::Rc;

use super::{Vm, param_type_accepts_array};
use crate::array::PhpArray;
use crate::object::Visibility;
use crate::opcode::{OpArray, ParamType};
use crate::value::Value;

/// Failure of a static property operation.
pub(crate) enum SpErr {
    /// Throw a new exception of the given class with the message.
    Throw(&'static [u8], String),
    /// An exception is already pending (e.g. thrown by an autoloader).
    Pending,
}

/// How the property is fetched (BP_VAR_* in php-src).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpFetch {
    /// Plain read.
    R,
    /// Read for a read-modify-write (compound assignment, ++/--).
    RW,
    /// Container fetch for a dimension write (`C::$p[k] = v`).
    W,
    /// Reference fetch (`$r = &C::$p`, by-ref argument, ...).
    Ref,
}

/// A resolved static property slot (indices into `Vm::classes`, so the
/// hot path does not allocate).
pub(crate) struct StaticPropSlot {
    /// Class whose `static_properties` holds the slot.
    owner: usize,
    /// Declaring class of the property.
    decl: usize,
    /// The property declaration: (class index, index in its properties).
    def: Option<(usize, usize)>,
    typed: bool,
    set_vis: Option<Visibility>,
}

impl Vm {
    /// Calling class scope (lowercase) for static property visibility.
    pub(crate) fn static_prop_scope(&self, op_array: &OpArray) -> Option<Vec<u8>> {
        op_array
            .scope_class
            .clone()
            .or_else(|| self.scope_top().cloned())
            .map(|s| s.to_ascii_lowercase())
    }

    /// Resolve the class operand of a static property access to the index
    /// of a loaded class in `classes`, autoloading if needed.
    pub(crate) fn static_prop_class_idx(&mut self, v: &Value, prop: &[u8]) -> Result<usize, SpErr> {
        let s = match v {
            Value::Object(o) => {
                let lower = o.borrow().class_name.to_ascii_lowercase();
                if let Some(i) = self.classes.get_index_of(&lower) {
                    return Ok(i);
                }
                return Err(SpErr::Throw(
                    b"Error",
                    format!(
                        "Access to undeclared static property {}::${}",
                        self.builtin_canonical_name(&lower),
                        String::from_utf8_lossy(prop)
                    ),
                ));
            }
            Value::Reference(r) => {
                let inner = r.borrow().clone();
                return self.static_prop_class_idx(&inner, prop);
            }
            Value::String(s) => s,
            _ => {
                return Err(SpErr::Throw(
                    b"Error",
                    "Class name must be a valid object or a string".to_string(),
                ));
            }
        };
        let name = s.as_bytes();
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        // Class keys are lowercase: a lowercase literal hits directly.
        if let Some(i) = self.classes.get_index_of(name) {
            return Ok(i);
        }
        let resolved: Vec<u8> = if name.eq_ignore_ascii_case(b"static") {
            match self.called_class_stack.last() {
                Some(c) => c.clone(),
                None => match self.scope_top() {
                    Some(s) => s.clone(),
                    None => {
                        return Err(SpErr::Throw(
                            b"Error",
                            "Cannot access \"static\" when no class scope is active".to_string(),
                        ));
                    }
                },
            }
        } else if name.eq_ignore_ascii_case(b"self") || name.eq_ignore_ascii_case(b"parent") {
            let scope = match self.scope_top() {
                Some(s) => s.clone(),
                None => {
                    return Err(SpErr::Throw(
                        b"Error",
                        format!(
                            "Cannot access \"{}\" when no class scope is active",
                            String::from_utf8_lossy(name).to_ascii_lowercase()
                        ),
                    ));
                }
            };
            if name.eq_ignore_ascii_case(b"self") {
                scope
            } else {
                match self.classes.get(&scope).and_then(|c| c.parent.clone()) {
                    Some(p) => p,
                    None => {
                        return Err(SpErr::Throw(
                            b"Error",
                            "Cannot access \"parent\" when current class scope has no parent"
                                .to_string(),
                        ));
                    }
                }
            }
        } else {
            name.to_vec()
        };
        let lower = resolved.to_ascii_lowercase();
        if let Some(i) = self.classes.get_index_of(&lower) {
            return Ok(i);
        }
        self.try_autoload_class(&resolved);
        if self.current_exception.is_some() {
            return Err(SpErr::Pending);
        }
        if let Some(i) = self.classes.get_index_of(&lower) {
            return Ok(i);
        }
        if self.is_known_builtin_class(&lower) {
            // Internal classes without a class entry declare no statics.
            return Err(SpErr::Throw(
                b"Error",
                format!(
                    "Access to undeclared static property {}::${}",
                    self.builtin_canonical_name(&lower),
                    String::from_utf8_lossy(prop)
                ),
            ));
        }
        Err(SpErr::Throw(
            b"Error",
            format!("Class \"{}\" not found", String::from_utf8_lossy(&resolved)),
        ))
    }

    /// Locate static property `name` accessed through class `ce_idx` (index
    /// in `classes`). With `access = Some(op_array)` the property's
    /// visibility is checked against the calling scope.
    pub(crate) fn locate_static_prop(
        &self,
        ce_idx: usize,
        name: &[u8],
        access: Option<&OpArray>,
    ) -> Result<StaticPropSlot, SpErr> {
        let Some((class_key, ce)) = self.classes.get_index(ce_idx) else {
            return Err(SpErr::Throw(b"Error", "Class not found".to_string()));
        };
        let undeclared = || {
            SpErr::Throw(
                b"Error",
                format!(
                    "Access to undeclared static property {}::${}",
                    crate::value::display_class_name(&ce.name),
                    String::from_utf8_lossy(name)
                ),
            )
        };
        let def_idx = self.nearest_prop_decl(ce, name);
        let mut decl: Option<usize> = None;
        let (typed, set_vis) = match def_idx.map(|i| &ce.properties[i]) {
            Some(d) => {
                // zend_std_get_static_property_with_info(): visibility is
                // checked before the property is known to be static.
                if let (Some(op_array), true) = (access, d.visibility != Visibility::Public) {
                    let scope = self.static_prop_scope(op_array);
                    let scope = scope.as_deref();
                    let ok = match d.visibility {
                        Visibility::Private => {
                            scope.is_some_and(|s| s.eq_ignore_ascii_case(&d.declaring_class))
                        }
                        _ => scope.is_some_and(|s| {
                            self.check_visibility(
                                Visibility::Protected,
                                &d.declaring_class,
                                &ce.name,
                                &String::from_utf8_lossy(name),
                                true,
                                Some(s),
                            )
                            .is_none()
                        }),
                    };
                    if !ok {
                        return Err(SpErr::Throw(
                            b"Error",
                            format!(
                                "Cannot access {} property {}::${}",
                                if d.visibility == Visibility::Private {
                                    "private"
                                } else {
                                    "protected"
                                },
                                crate::value::display_class_name(&ce.name),
                                String::from_utf8_lossy(name)
                            ),
                        ));
                    }
                }
                if !d.is_static {
                    return Err(undeclared());
                }
                decl = if d.declaring_class.eq_ignore_ascii_case(class_key) {
                    Some(ce_idx)
                } else {
                    self.classes.get_index_of(&d.declaring_class).or_else(|| {
                        self.classes
                            .get_index_of(&d.declaring_class.to_ascii_lowercase())
                    })
                };
                (d.property_type.is_some(), d.set_visibility)
            }
            None => (false, None),
        };
        let def = def_idx.map(|i| (ce_idx, i));
        // The slot lives on the declaring class; fall back to the first
        // class in the chain holding one (classes without property metadata).
        if let Some(di) = decl {
            if self.classes[di].static_properties.contains_key(name) {
                return Ok(StaticPropSlot {
                    owner: di,
                    decl: di,
                    def,
                    typed,
                    set_vis,
                });
            }
        }
        let mut cur = Some(ce_idx);
        for _ in 0..64 {
            let Some(ci) = cur else { break };
            let c = &self.classes[ci];
            if c.static_properties.contains_key(name) {
                return Ok(StaticPropSlot {
                    owner: ci,
                    decl: decl.unwrap_or(ci),
                    def,
                    typed,
                    set_vis,
                });
            }
            cur = c.parent.as_ref().and_then(|p| {
                self.classes
                    .get_index_of(p.as_slice())
                    .or_else(|| self.classes.get_index_of(&p.to_ascii_lowercase()))
            });
        }
        Err(undeclared())
    }

    /// The static slot's current cell.
    fn sp_cell(&mut self, slot: &StaticPropSlot, name: &[u8]) -> Option<&mut Value> {
        self.classes
            .get_index_mut(slot.owner)
            .and_then(|(_, c)| c.static_properties.get_mut(name))
    }

    /// The property's declared type.
    fn sp_ptype(&self, slot: &StaticPropSlot) -> Option<ParamType> {
        let (ci, pi) = slot.def?;
        self.classes[ci].properties.get(pi)?.property_type.clone()
    }

    /// `pt` with self / parent replaced by the declaring class / its parent
    /// (an unresolvable parent by a type nothing satisfies).
    fn sp_resolve_relative_type(&self, pt: ParamType, slot: &StaticPropSlot) -> ParamType {
        fn walk(pt: ParamType, selfn: &[u8], parent: Option<&[u8]>) -> ParamType {
            match pt {
                ParamType::Simple(n) if n.eq_ignore_ascii_case(b"self") => {
                    ParamType::Simple(selfn.to_vec())
                }
                ParamType::Simple(n) if n.eq_ignore_ascii_case(b"parent") => ParamType::Simple(
                    parent
                        .map(|p| p.to_vec())
                        .unwrap_or_else(|| b"\0never".to_vec()),
                ),
                ParamType::Nullable(i) => ParamType::Nullable(Box::new(walk(*i, selfn, parent))),
                ParamType::Union(v) => {
                    ParamType::Union(v.into_iter().map(|t| walk(t, selfn, parent)).collect())
                }
                ParamType::Intersection(v) => {
                    ParamType::Intersection(v.into_iter().map(|t| walk(t, selfn, parent)).collect())
                }
                other => other,
            }
        }
        let Some((_, ce)) = self.classes.get_index(slot.decl) else {
            return pt;
        };
        walk(pt, &ce.name, ce.parent.as_deref())
    }

    /// Lowercase key of the declaring class.
    fn sp_decl_key(&self, slot: &StaticPropSlot) -> &[u8] {
        self.classes
            .get_index(slot.decl)
            .map(|(k, _)| k.as_slice())
            .unwrap_or(b"")
    }

    /// Display name of the declaring class.
    fn sp_decl_display(&self, slot: &StaticPropSlot) -> String {
        crate::value::display_class_name(&self.classes[slot.decl].name)
    }

    /// The declaration of property `name` that applies to class `ce`: the
    /// nearest one in its class chain (a class's property list also carries
    /// inherited entries, private ones included).
    fn nearest_prop_decl(&self, ce: &crate::object::ClassEntry, name: &[u8]) -> Option<usize> {
        let mut k = ce;
        for _ in 0..64 {
            if let Some(p) = ce
                .properties
                .iter()
                .position(|p| p.name == name && p.declaring_class.eq_ignore_ascii_case(&k.name))
            {
                return Some(p);
            }
            match k
                .parent
                .as_ref()
                .and_then(|p| self.classes.get(&p.to_ascii_lowercase()))
            {
                Some(pc) => k = pc,
                None => break,
            }
        }
        ce.properties.iter().position(|p| p.name == name)
    }

    /// A static property accessed with instance syntax (`$obj->p`), as in
    /// zend_get_property_offset(): an inaccessible one is an error, an
    /// accessible one raises "Accessing static property as non static" and
    /// is then treated as a dynamic property. `magic` is the lowercase name of
    /// the magic method that makes the lookup silent.
    pub(crate) fn static_as_instance_check(
        &mut self,
        obj: &Rc<RefCell<crate::object::PhpObject>>,
        name: &[u8],
        magic: &[u8],
        op_array: &OpArray,
        line: u32,
    ) -> Result<(), SpErr> {
        // Cheap filter for the hot path: is `name` a static property anywhere?
        if self.static_name_cache.0 != self.classes.len() {
            let names: std::collections::HashSet<Vec<u8>> = self
                .classes
                .values()
                .flat_map(|c| c.properties.iter())
                .filter(|p| p.is_static)
                .map(|p| p.name.clone())
                .collect();
            let bloom = names.iter().fold(0u64, |b, n| b | name_bloom_bit(n));
            self.static_name_cache = (self.classes.len(), names, bloom);
        }
        if self.static_name_cache.2 & name_bloom_bit(name) == 0
            || !self.static_name_cache.1.contains(name)
        {
            return Ok(());
        }
        let scope = self.static_prop_scope(op_array);
        let scope = scope.as_deref();
        let lower = obj.borrow().class_name.to_ascii_lowercase();
        let Some(ce) = self.classes.get(&lower) else {
            return Ok(());
        };
        if !ce.properties.iter().any(|p| p.is_static && p.name == name) {
            return Ok(());
        }
        // The lookup is silent when the matching magic method exists
        // (__get / __set / __unset): it then handles the access.
        if ce.methods.contains_key(magic) {
            return Ok(());
        }
        let Some(mut d) = self.nearest_prop_decl(ce, name).map(|i| &ce.properties[i]) else {
            return Ok(());
        };
        // Inside an ancestor declaring a private property of that name, that
        // one applies (zend_get_parent_private_property).
        if let Some(s) = scope {
            if !d.declaring_class.eq_ignore_ascii_case(s) && self.class_extends(&lower, s) {
                if let Some(p) = ce.properties.iter().find(|p| {
                    p.name == name
                        && p.visibility == Visibility::Private
                        && p.declaring_class.eq_ignore_ascii_case(s)
                }) {
                    // An instance property on the class wins over a private
                    // static one of the scope.
                    if !p.is_static || d.is_static {
                        d = p;
                    }
                }
            }
        }
        if !d.is_static {
            return Ok(());
        }
        let cname = crate::value::display_class_name(&ce.name);
        if d.visibility != Visibility::Public {
            let ok = match d.visibility {
                Visibility::Private => {
                    scope.is_some_and(|s| s.eq_ignore_ascii_case(&d.declaring_class))
                }
                _ => scope.is_some_and(|s| {
                    self.check_visibility(
                        Visibility::Protected,
                        &d.declaring_class,
                        &ce.name,
                        &String::from_utf8_lossy(name),
                        true,
                        Some(s),
                    )
                    .is_none()
                }),
            };
            if !ok {
                if d.visibility == Visibility::Private
                    && !d.declaring_class.eq_ignore_ascii_case(&lower)
                {
                    // A parent's private static is invisible: dynamic property.
                    return Ok(());
                }
                return Err(SpErr::Throw(
                    b"Error",
                    format!(
                        "Cannot access {} property {}::${}",
                        if d.visibility == Visibility::Private {
                            "private"
                        } else {
                            "protected"
                        },
                        cname,
                        String::from_utf8_lossy(name)
                    ),
                ));
            }
        }
        let msg = format!(
            "Accessing static property {}::${} as non static",
            cname,
            String::from_utf8_lossy(name)
        );
        self.emit_notice_at(&msg, line);
        Ok(())
    }

    /// Canonical display name of a (lowercase) class key.
    fn sp_class_display(&self, key: &[u8]) -> String {
        self.classes
            .get(key)
            .map(|c| crate::value::display_class_name(&c.name))
            .unwrap_or_else(|| String::from_utf8_lossy(key).to_string())
    }

    /// Asymmetric visibility (`private(set)` / `protected(set)`) check for
    /// a write; `indirect` selects the "indirectly modify" wording.
    fn sp_check_set_visibility(
        &self,
        slot: &StaticPropSlot,
        name: &[u8],
        scope: Option<&[u8]>,
        indirect: bool,
    ) -> Result<(), SpErr> {
        let Some(sv) = slot.set_vis else {
            return Ok(());
        };
        let decl = self.sp_decl_key(slot);
        let ok = match sv {
            Visibility::Public => true,
            Visibility::Private => scope.is_some_and(|s| s == decl),
            Visibility::Protected => scope.is_some_and(|s| {
                s == decl || self.class_extends(s, decl) || self.class_extends(decl, s)
            }),
        };
        if ok {
            return Ok(());
        }
        let vis = if sv == Visibility::Private {
            "private(set)"
        } else {
            "protected(set)"
        };
        let scope_str = match scope {
            Some(s) => format!("scope {}", self.sp_class_display(s)),
            None => "global scope".to_string(),
        };
        Err(SpErr::Throw(
            b"Error",
            format!(
                "Cannot {}modify {} property {}::${} from {}",
                if indirect { "indirectly " } else { "" },
                vis,
                self.sp_decl_display(slot),
                String::from_utf8_lossy(name),
                scope_str
            ),
        ))
    }

    /// Deprecation for accessing a trait's static property directly.
    fn sp_trait_deprecation(&mut self, ce_idx: usize, name: &[u8], line: u32) {
        if let Some((_, c)) = self.classes.get_index(ce_idx) {
            if c.is_trait {
                let msg = format!(
                    "Accessing static trait property {}::${} is deprecated, it should only be accessed on a class using the trait",
                    String::from_utf8_lossy(&c.name),
                    String::from_utf8_lossy(name)
                );
                self.emit_deprecated_at(&msg, line);
            }
        }
    }

    fn sp_uninit_error(&self, slot: &StaticPropSlot, name: &[u8]) -> SpErr {
        SpErr::Throw(
            b"Error",
            format!(
                "Typed static property {}::${} must not be accessed before initialization",
                self.sp_decl_display(slot),
                String::from_utf8_lossy(name)
            ),
        )
    }

    /// Fetch a static property for reading (R/RW), as a dimension-write
    /// container (W: null/unset is promoted to an array) or as a reference
    /// (Ref: the slot is turned into a reference shared with the result).
    pub(crate) fn static_prop_fetch(
        &mut self,
        class_v: &Value,
        name: &[u8],
        mode: SpFetch,
        op_array: &OpArray,
        line: u32,
    ) -> Result<Value, SpErr> {
        let key = self.static_prop_class_idx(class_v, name)?;
        let slot = self.locate_static_prop(key, name, Some(op_array))?;
        self.sp_trait_deprecation(key, name, line);
        if mode != SpFetch::R && slot.set_vis.is_some() {
            let scope = self.static_prop_scope(op_array);
            self.sp_check_set_visibility(&slot, name, scope.as_deref(), true)?;
        }
        let ptype = if slot.typed && mode != SpFetch::R && mode != SpFetch::RW {
            self.sp_ptype(&slot)
        } else {
            None
        };
        let typed = slot.typed;
        // A default referring to something declared later is resolved on
        // first use, whatever the fetch mode (zend_update_class_constants).
        let deferred = match self.sp_cell(&slot, name) {
            None => return Ok(Value::Null),
            Some(Value::Reference(r)) => r.borrow().clone(),
            Some(v) => v.clone(),
        };
        if let Value::String(s) = &deferred {
            // A bare constant that is still undefined throws.
            if let Some(c) = s.as_bytes().strip_prefix(b"__deferred_global_const__::") {
                let short = c.rsplit(|&b| b == b'\\').next().unwrap_or(c);
                if self.deferred_global_const(c).is_none()
                    && crate::stubs::find_constant(short).is_none()
                {
                    return Err(SpErr::Throw(
                        b"Error",
                        format!("Undefined constant \"{}\"", String::from_utf8_lossy(c)),
                    ));
                }
            }
        }
        if matches!(&deferred, Value::String(s) if s.as_bytes().starts_with(b"__deferred_")) {
            // A thunk runs in the scope of the class holding the slot (a
            // trait's default: the class using it).
            let resolved = match (
                thunk_marker_name(&deferred),
                self.classes.get_index(slot.owner),
            ) {
                (Some(t), Some((owner, _))) => {
                    let (t, owner) = (t.to_vec(), owner.clone());
                    self.eval_const_thunk(&t, Some(&owner), "")
                        .unwrap_or(Value::Null)
                }
                _ => self.resolve_deferred_value(&deferred),
            };
            if !matches!(&resolved, Value::String(s) if s.as_bytes().starts_with(b"__deferred_")) {
                if let Some(cell) = self.sp_cell(&slot, name) {
                    match cell {
                        Value::Reference(r) => *r.borrow_mut() = resolved,
                        c => *c = resolved,
                    }
                }
            }
        }
        let Some(cell) = self.sp_cell(&slot, name) else {
            return Ok(Value::Null);
        };
        match mode {
            SpFetch::R | SpFetch::RW => {
                let v = match cell {
                    Value::Reference(r) => r.borrow().clone(),
                    v => v.clone(),
                };
                if matches!(v, Value::Undef) {
                    if typed {
                        return Err(self.sp_uninit_error(&slot, name));
                    }
                    return Ok(Value::Null);
                }
                Ok(v)
            }
            SpFetch::W => {
                let rc = match cell {
                    Value::Reference(r) => Some(r.clone()),
                    _ => None,
                };
                let cur = match &rc {
                    Some(r) => r.borrow().clone(),
                    None => cell.clone(),
                };
                match cur {
                    Value::Null | Value::Undef => {
                        if let Some(pt) = &ptype {
                            // (zend_handle_fetch_obj_flags(): an uninitialized
                            // property is auto-initialized too)
                            if !param_type_accepts_array(pt) {
                                let td = self.param_type_name(pt);
                                return Err(SpErr::Throw(
                                    b"TypeError",
                                    format!(
                                        "Cannot auto-initialize an array inside property {}::${} of type {}",
                                        self.sp_decl_display(&slot),
                                        String::from_utf8_lossy(name),
                                        td
                                    ),
                                ));
                            }
                        }
                        let arr = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
                        match rc {
                            Some(r) => *r.borrow_mut() = arr.clone(),
                            None => *cell = arr.clone(),
                        }
                        Ok(arr)
                    }
                    // Arrays are copied on assignment, so the slot's array is
                    // not shared with another variable and is written in place.
                    other => Ok(other),
                }
            }
            SpFetch::Ref => {
                if let Value::Reference(r) = cell {
                    let r = r.clone();
                    if let Some(pt) = ptype {
                        let decl = self.sp_decl_key(&slot).to_vec();
                        self.typed_ref_register(&r, &decl, name, pt);
                    }
                    return Ok(Value::Reference(r));
                }
                if matches!(cell, Value::Undef) {
                    if let Some(pt) = &ptype {
                        if !self.value_matches_type_mode(&Value::Null, pt, true) {
                            return Err(SpErr::Throw(
                                b"Error",
                                format!(
                                    "Cannot access uninitialized non-nullable property {}::${} by reference",
                                    self.sp_decl_display(&slot),
                                    String::from_utf8_lossy(name)
                                ),
                            ));
                        }
                    }
                }
                let cell = self.sp_cell(&slot, name).expect("slot located above");
                let inner = match std::mem::replace(cell, Value::Null) {
                    Value::Undef => Value::Null,
                    v => v,
                };
                let r = Rc::new(RefCell::new(inner));
                *cell = Value::Reference(r.clone());
                if let Some(pt) = ptype {
                    let decl = self.sp_decl_key(&slot).to_vec();
                    self.typed_ref_register(&r, &decl, name, pt);
                }
                Ok(Value::Reference(r))
            }
        }
    }

    /// Verify / coerce a value for a typed static property. `overflow`
    /// selects the ++/-- overflow wording (Some(true) = increment).
    pub(crate) fn sp_verify_type(
        &mut self,
        slot: &StaticPropSlot,
        name: &[u8],
        value: &mut Value,
        strict: bool,
        overflow: Option<bool>,
        line: u32,
    ) -> Result<(), SpErr> {
        if !slot.typed {
            return Ok(());
        }
        let Some(pt) = self.sp_ptype(slot) else {
            return Ok(());
        };
        // self / parent name the declaring class (a trait's own, accessed on
        // the trait), whatever the calling scope.
        let declared = pt.clone();
        let pt = self.sp_resolve_relative_type(pt, slot);
        if !strict {
            self.emit_implicit_float_to_int_if_needed(value, &pt, line);
        }
        Self::coerce_value_to_type(value, &pt, strict);
        if self.value_matches_type_mode(value, &pt, true) {
            return Ok(());
        }
        let cd = self.sp_decl_display(slot);
        let pd = String::from_utf8_lossy(name);
        let td = self.param_type_name(&declared);
        let msg = match overflow {
            Some(true) => format!(
                "Cannot increment property {}::${} of type {} past its maximal value",
                cd, pd, td
            ),
            Some(false) => format!(
                "Cannot decrement property {}::${} of type {} past its minimal value",
                cd, pd, td
            ),
            None => format!(
                "Cannot assign {} to property {}::${} of type {}",
                Self::value_type_name(value),
                cd,
                pd,
                td
            ),
        };
        Err(SpErr::Throw(b"TypeError", msg))
    }

    /// `C::$p = value` (by_ref = false) or `C::$p = &ref` (by_ref = true).
    /// Returns the value stored.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn static_prop_assign(
        &mut self,
        class_v: &Value,
        name: &[u8],
        value: Value,
        by_ref: bool,
        op_array: &OpArray,
        overflow: Option<bool>,
        line: u32,
    ) -> Result<Value, SpErr> {
        let key = self.static_prop_class_idx(class_v, name)?;
        let slot = self.locate_static_prop(key, name, Some(op_array))?;
        self.sp_trait_deprecation(key, name, line);
        if slot.set_vis.is_some() {
            let scope = self.static_prop_scope(op_array);
            self.sp_check_set_visibility(&slot, name, scope.as_deref(), by_ref)?;
        }
        let strict = op_array.strict_types;
        if by_ref {
            if let Value::Reference(r) = value {
                // Binding an undefined variable makes it null.
                if matches!(*r.borrow(), Value::Undef) {
                    *r.borrow_mut() = Value::Null;
                }
                if slot.typed {
                    // A reference already bound to typed properties must
                    // agree with this one's type.
                    if let Some(pt) = self.sp_ptype(&slot) {
                        let decl = self.sp_decl_key(&slot).to_vec();
                        if let Some(msg) = self.typed_ref_bind_error(&r, &decl, name, &pt) {
                            return Err(SpErr::Throw(b"TypeError", msg));
                        }
                    }
                    let mut inner = r.borrow().clone();
                    self.sp_verify_type(&slot, name, &mut inner, strict, None, line)?;
                    *r.borrow_mut() = inner;
                    if let Some(pt) = self.sp_ptype(&slot) {
                        let decl = self.sp_decl_key(&slot).to_vec();
                        self.typed_ref_register(&r, &decl, name, pt);
                    }
                }
                let out = r.borrow().clone();
                // The previous reference no longer belongs to this property.
                if slot.typed {
                    if let Some(Value::Reference(old)) =
                        self.sp_cell(&slot, name).map(|c| c.clone())
                    {
                        if !Rc::ptr_eq(&old, &r) {
                            let decl = self.sp_decl_key(&slot).to_vec();
                            self.typed_ref_unregister(&old, &decl, name);
                        }
                    }
                }
                if let Some(cell) = self.sp_cell(&slot, name) {
                    *cell = Value::Reference(r);
                }
                return Ok(out);
            }
        }
        let mut value = match value {
            Value::Reference(r) => r.borrow().clone(),
            Value::Undef => Value::Null,
            v => v,
        };
        self.sp_verify_type(&slot, name, &mut value, strict, overflow, line)?;
        // Other typed properties sharing the slot's reference.
        if super::typed_ref::any() {
            if let Some(Value::Reference(r)) = self.sp_cell(&slot, name).map(|c| c.clone()) {
                if let Err(msg) = self.typed_ref_check(&r, &mut value, strict, Some(name)) {
                    return Err(SpErr::Throw(b"TypeError", msg));
                }
            }
        }
        if let Some(cell) = self.sp_cell(&slot, name) {
            match cell {
                Value::Reference(r) => *r.borrow_mut() = value.clone(),
                c => *c = value.clone(),
            }
        }
        Ok(value)
    }

    /// isset(C::$p): a missing or inaccessible property is simply not set.
    pub(crate) fn static_prop_isset(
        &mut self,
        class_v: &Value,
        name: &[u8],
        op_array: &OpArray,
    ) -> Result<bool, SpErr> {
        // Class resolution errors are not silenced (only property ones are).
        let key = self.static_prop_class_idx(class_v, name)?;
        let Ok(slot) = self.locate_static_prop(key, name, Some(op_array)) else {
            return Ok(false);
        };
        Ok(self.classes[slot.owner]
            .static_properties
            .get(name)
            .is_some_and(|v| !matches!(v.deref(), Value::Null | Value::Undef)))
    }

    /// unset(C::$p) always throws once the class resolves.
    pub(crate) fn static_prop_unset(&mut self, class_v: &Value, name: &[u8]) -> SpErr {
        match self.static_prop_class_idx(class_v, name) {
            Ok(key) => SpErr::Throw(
                b"Error",
                format!(
                    "Attempt to unset static property {}::${}",
                    crate::value::display_class_name(&self.classes[key].name),
                    String::from_utf8_lossy(name)
                ),
            ),
            Err(e) => e,
        }
    }

    /// Current value of a static property (dereferenced), looked up through
    /// class `class_key` (lowercase) without visibility checks. `None` if
    /// the property does not exist.
    pub fn static_prop_raw(&self, class_key: &[u8], name: &[u8]) -> Option<Value> {
        let idx = self.classes.get_index_of(class_key)?;
        let slot = self.locate_static_prop(idx, name, None).ok()?;
        let v = self.classes[slot.owner].static_properties.get(name)?;
        Some(match v {
            Value::Reference(r) => r.borrow().clone(),
            v => v.clone(),
        })
    }

    /// Store into a static property slot (writing through a reference),
    /// without visibility or type checks. Returns false if it does not exist.
    pub fn static_prop_put_raw(&mut self, class_key: &[u8], name: &[u8], value: Value) -> bool {
        let Some(idx) = self.classes.get_index_of(class_key) else {
            return false;
        };
        let Ok(slot) = self.locate_static_prop(idx, name, None) else {
            return false;
        };
        match self.sp_cell(&slot, name) {
            Some(Value::Reference(r)) => {
                *r.borrow_mut() = value;
                true
            }
            Some(c) => {
                *c = value;
                true
            }
            None => false,
        }
    }

    /// static_prop_put_raw(), but a reference held by typed properties
    /// verifies the value (ZEND_TRY_ASSIGN_REF): Err(()) with the TypeError
    /// pending when it does not fit.
    #[allow(clippy::result_unit_err)]
    pub fn static_prop_assign_typed(
        &mut self,
        class_key: &[u8],
        name: &[u8],
        value: Value,
    ) -> Result<bool, ()> {
        let Some(idx) = self.classes.get_index_of(class_key) else {
            return Ok(false);
        };
        let Ok(slot) = self.locate_static_prop(idx, name, None) else {
            return Ok(false);
        };
        let r = match self.sp_cell(&slot, name) {
            Some(Value::Reference(r)) => r.clone(),
            Some(c) => {
                *c = value;
                return Ok(true);
            }
            None => return Ok(false),
        };
        self.try_assign_ref(&r, value).map(|_| true)
    }

    /// Build the PHP-level exception for a static property error and make it
    /// current. Returns the "Uncaught ..." message for the no-handler path.
    pub(crate) fn sp_raise(&mut self, err: SpErr, line: u32) -> String {
        match err {
            SpErr::Throw(cls, msg) => {
                let exc = self.create_exception(cls, &msg, line);
                self.current_exception = Some(exc);
                format!("Uncaught {}: {}", String::from_utf8_lossy(cls), msg)
            }
            SpErr::Pending => "Uncaught exception".to_string(),
        }
    }

    /// Evaluate a constant-expression thunk (see
    /// Compiler::compile_const_thunk) with the scope of the class that owns
    /// it, as a "[constant expression]" frame. `what` names the constant
    /// for the self-reference error. Err when an exception was thrown (it
    /// is left pending).
    pub(crate) fn eval_const_thunk(
        &mut self,
        name: &[u8],
        owner_hint: Option<&[u8]>,
        what: &str,
    ) -> Result<Value, ()> {
        let found = owner_hint
            .and_then(|o| {
                self.classes.get(o).and_then(|c| {
                    c.const_closures
                        .iter()
                        .find(|op| op.name == name)
                        .map(|op| (o.to_vec(), c.name.clone(), op.clone()))
                })
            })
            .or_else(|| {
                self.classes.iter().find_map(|(k, c)| {
                    c.const_closures
                        .iter()
                        .find(|op| op.name == name)
                        .map(|op| {
                            // A trait's property default / constant runs in the
                            // scope of the class using it (self:: is that class).
                            match owner_hint.and_then(|o| self.classes.get(o).map(|oc| (o, oc))) {
                                Some((o, oc)) if c.is_trait => {
                                    (o.to_vec(), oc.name.clone(), op.clone())
                                }
                                _ => (k.clone(), c.name.clone(), op.clone()),
                            }
                        })
                })
            });
        let Some((scope, scope_name, op)) = found else {
            return Ok(Value::Null);
        };
        self.eval_const_thunk_op(&op, &scope, &scope_name, what)
    }

    /// Run a constant-expression thunk op array with class scope `scope`
    /// (lowercase key; `scope_name` its display name).
    pub(crate) fn eval_const_thunk_op(
        &mut self,
        op: &OpArray,
        scope: &[u8],
        scope_name: &[u8],
        what: &str,
    ) -> Result<Value, ()> {
        let name = op.name.as_slice();
        // Like php, the constant fetched by the outermost access is not
        // marked as being evaluated; nested ones are (IS_CONSTANT_VISITED).
        if self.const_thunks_active.iter().skip(1).any(|n| n == name) {
            let msg = format!("Cannot declare self-referencing constant {}", what);
            let line = self.current_line;
            let exc = self.create_exception(b"Error", &msg, line);
            self.current_exception = Some(exc);
            return Err(());
        }
        let file = self.current_file.clone();
        let line = self.current_line;
        // A bare constant reference runs at the access line; an expression
        // gets one "[constant expression]" frame for a whole nested
        // evaluation, when it is declared elsewhere than the access.
        let bare = op.decl_line == 0;
        let relined;
        let op = if bare {
            let mut o = op.clone();
            for x in o.ops.iter_mut() {
                x.line = line;
            }
            relined = o;
            &relined
        } else {
            op
        };
        let frame = !bare
            && self.const_thunks_active.is_empty()
            && (op.decl_line != line || op.filename != self.current_file.as_bytes());
        self.const_thunks_active.push(name.to_vec());
        self.class_scope_stack.push(scope.to_vec());
        self.called_class_stack.push(scope_name.to_vec());
        if frame {
            self.call_stack.push((
                "[constant expression]".to_string(),
                file,
                line,
                Vec::new(),
                false,
            ));
        }
        let r = self.execute_op_array_pub(op, Vec::new());
        if frame {
            self.call_stack.pop();
        }
        self.called_class_stack.pop();
        self.class_scope_stack.pop();
        self.const_thunks_active.pop();
        self.current_line = line;
        match r {
            Ok(v) if self.current_exception.is_none() => Ok(match v {
                Value::Reference(r) => r.borrow().clone(),
                Value::Undef => Value::Null,
                v => v,
            }),
            Ok(_) => Err(()),
            Err(e) => {
                if self.current_exception.is_none() {
                    let exc = self.create_exception(b"Error", &e.message, e.line);
                    self.current_exception = Some(exc);
                }
                Err(())
            }
        }
    }

    /// zend_update_class_constants() for instance property defaults: the
    /// constant-expression defaults of class `class_lower` are evaluated once
    /// (on first instantiation) and stored back. Err when one threw.
    /// update_class_constants() for other crates: false when an exception
    /// was thrown (it is left pending).
    pub fn update_class_constants_ok(&mut self, class_lower: &[u8]) -> bool {
        self.update_class_constants(class_lower).is_ok()
    }

    /// zend_update_class_constants(): an evaluated property default must
    /// satisfy the property's type (strictly).
    fn verify_default_type(
        &mut self,
        class_lower: &[u8],
        idx: usize,
        v: &mut Value,
    ) -> Result<(), ()> {
        let Some((pt, name, decl)) = self.classes.get(class_lower).and_then(|c| {
            c.properties.get(idx).and_then(|p| {
                p.property_type
                    .clone()
                    .map(|t| (t, p.name.clone(), p.declaring_class.clone()))
            })
        }) else {
            return Ok(());
        };
        Self::coerce_value_to_type(v, &pt, true);
        if self.value_matches_type_mode(v, &pt, true) {
            return Ok(());
        }
        let decl_disp = self
            .class_name_for_display(&decl.to_ascii_lowercase())
            .unwrap_or(decl);
        let msg = format!(
            "Cannot assign {} to property {}::${} of type {}",
            Self::value_type_name(v),
            crate::value::display_class_name(&decl_disp),
            String::from_utf8_lossy(&name),
            self.param_type_name(&pt)
        );
        let exc = self.create_exception(b"TypeError", &msg, self.current_line);
        self.current_exception = Some(exc);
        Err(())
    }

    pub(crate) fn update_class_constants(&mut self, class_lower: &[u8]) -> Result<(), ()> {
        if self.consts_updated.contains(class_lower) {
            return Ok(());
        }
        // The parent's constants first, then this class's constants (in
        // declaration order), then its property defaults.
        if let Some(parent) = self
            .classes
            .get(class_lower)
            .and_then(|c| c.parent.as_ref())
            .map(|p| p.to_ascii_lowercase())
        {
            self.update_class_constants(&parent)?;
        }
        let consts: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = match self.classes.get(class_lower) {
            Some(c) => c
                .constants
                .iter()
                .filter_map(|(k, v)| {
                    thunk_marker_name(v).map(|n| {
                        let decl = c
                            .constants_meta
                            .get(k)
                            .map(|m| m.declaring_class.to_ascii_lowercase())
                            .unwrap_or_else(|| class_lower.to_vec());
                        (k.clone(), n.to_vec(), decl)
                    })
                })
                .collect(),
            None => return Ok(()),
        };
        for (cname, thunk, decl) in consts {
            // Evaluated meanwhile (e.g. through another constant)?
            let still = self
                .classes
                .get(class_lower)
                .and_then(|c| c.constants.get(&cname))
                .is_some_and(|v| thunk_marker_name(v).is_some());
            if !still {
                continue;
            }
            let v = self.eval_const_thunk(&thunk, Some(&decl), "")?;
            for k in [&decl[..], class_lower] {
                if let Some(slot) = self
                    .classes
                    .get_mut(k)
                    .and_then(|c| c.constants.get_mut(&cname))
                {
                    if thunk_marker_name(slot) == Some(&thunk[..]) {
                        *slot = v.clone();
                    }
                }
            }
        }
        let pending: Vec<(usize, Vec<u8>, Vec<u8>, Vec<u8>)> = match self.classes.get(class_lower) {
            Some(c) => c
                .properties
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    thunk_marker_name(&p.default).map(|n| {
                        (
                            i,
                            n.to_vec(),
                            p.name.clone(),
                            p.declaring_class.to_ascii_lowercase(),
                        )
                    })
                })
                .collect(),
            None => return Ok(()),
        };
        for (i, thunk, pname, decl) in pending {
            let mut v = self.eval_const_thunk(&thunk, Some(&decl), "")?;
            self.verify_default_type(class_lower, i, &mut v)?;
            if let Some(p) = self
                .classes
                .get_mut(class_lower)
                .and_then(|c| c.properties.get_mut(i))
            {
                p.default = v.clone();
            }
            if decl.as_slice() != class_lower {
                if let Some(c) = self.classes.get_mut(&decl) {
                    for p in c.properties.iter_mut() {
                        if p.name == pname && thunk_marker_name(&p.default) == Some(&thunk[..]) {
                            p.default = v.clone();
                        }
                    }
                }
            }
        }
        // Defaults that are a bare global constant, once it is defined; an
        // undefined one throws on first use like any constant expression.
        const GC: &[u8] = b"__deferred_global_const__::";
        let globals: Vec<(usize, Value)> = self
            .classes
            .get(class_lower)
            .map(|c| {
                c.properties
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| {
                        !p.is_static
                            && matches!(&p.default, Value::String(s) if s.as_bytes().starts_with(GC))
                    })
                    .map(|(i, p)| (i, p.default.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for (i, marker) in globals {
            let name = match &marker {
                Value::String(s) => s.as_bytes()[GC.len()..].to_vec(),
                _ => continue,
            };
            let short = name
                .rsplit(|&b| b == b'\\')
                .next()
                .unwrap_or(&name)
                .to_vec();
            if self.deferred_global_const(&name).is_none() {
                if crate::stubs::find_constant(&short).is_some() {
                    continue;
                }
                let msg = format!("Undefined constant \"{}\"", String::from_utf8_lossy(&name));
                let exc = self.create_exception(b"Error", &msg, self.current_line);
                self.current_exception = Some(exc);
                return Err(());
            }
            let mut v = self.resolve_deferred_value(&marker);
            if self.current_exception.is_some() {
                return Err(());
            }
            self.verify_default_type(class_lower, i, &mut v)?;
            if let Some(p) = self
                .classes
                .get_mut(class_lower)
                .and_then(|c| c.properties.get_mut(i))
            {
                p.default = v;
            }
        }
        // Static property defaults declared by this class.
        let statics: Vec<(Vec<u8>, Vec<u8>)> = self
            .classes
            .get(class_lower)
            .map(|c| {
                c.static_properties
                    .iter()
                    .filter_map(|(k, v)| thunk_marker_name(v).map(|n| (k.clone(), n.to_vec())))
                    .collect()
            })
            .unwrap_or_default();
        for (pname, thunk) in statics {
            let v = self.eval_const_thunk(&thunk, Some(class_lower), "")?;
            if let Some(slot) = self
                .classes
                .get_mut(class_lower)
                .and_then(|c| c.static_properties.get_mut(&pname))
            {
                if thunk_marker_name(slot) == Some(&thunk[..]) {
                    *slot = v;
                }
            }
        }
        self.consts_updated.insert(class_lower.to_vec());
        Ok(())
    }
}

/// The thunk name of a `__deferred_thunk__::` marker value.
pub(crate) fn thunk_marker_name(v: &Value) -> Option<&[u8]> {
    match v {
        Value::String(s) => s.as_bytes().strip_prefix(b"__deferred_thunk__::"),
        _ => None,
    }
}

/// One-bit "bloom" signature of a property name (cheap pre-filter).
fn name_bloom_bit(n: &[u8]) -> u64 {
    let h = (n.len() as u64)
        .wrapping_mul(31)
        .wrapping_add(n.first().copied().unwrap_or(0) as u64)
        .wrapping_mul(17)
        .wrapping_add(n.last().copied().unwrap_or(0) as u64);
    1u64 << (h % 64)
}

impl Vm {
    /// Canonical name of an internal class.
    fn internal_class_display(&self, name: &[u8]) -> String {
        match crate::stubs::find_class(name) {
            Some(c) => c.name.to_string(),
            None => self.builtin_canonical_name(&name.to_ascii_lowercase()),
        }
    }

    /// do_inheritance_check_on_method() for a class extending an internal
    /// class: overriding methods are checked against the stub signatures.
    /// A return type only incompatible with a tentative one is deprecated
    /// (unless #[\ReturnTypeWillChange]). Err((message, line, unresolved)).
    ///
    /// Like zend_do_link_class(), the errors raised meanwhile (deprecations)
    /// are recorded: emitted once the checks pass (through the user error
    /// handler), or before the fatal error without the handler.
    pub(crate) fn internal_parent_method_checks(
        &mut self,
        class: &crate::object::ClassEntry,
        parent: &'static crate::object::ClassEntry,
        decl_line: u32,
        can_retry: bool,
    ) -> Result<(), (String, u32, bool)> {
        let outer = self.recorded_errors.replace(Vec::new());
        let r = self.internal_parent_method_checks_inner(class, parent, decl_line);
        let recorded = std::mem::replace(&mut self.recorded_errors, outer).unwrap_or_default();
        match &r {
            Ok(()) => {
                for (ty, msg, line) in recorded {
                    self.raise_error(ty, &msg, line);
                }
            }
            // Retried later: the errors will be raised again then.
            Err((_, _, true)) if can_retry => {}
            Err(_) => {
                let file = self.current_file.clone();
                for (ty, msg, line) in recorded {
                    self.error_cb(ty, &msg, &file, line);
                }
            }
        }
        r
    }

    fn internal_parent_method_checks_inner(
        &mut self,
        class: &crate::object::ClassEntry,
        parent: &'static crate::object::ClassEntry,
        decl_line: u32,
    ) -> Result<(), (String, u32, bool)> {
        let cls_lower = class.name.to_ascii_lowercase();
        let own: Vec<(Vec<u8>, crate::object::MethodDef)> = class
            .methods
            .iter()
            .filter(|(_, m)| m.declaring_class.eq_ignore_ascii_case(&cls_lower))
            .map(|(k, m)| (k.clone(), m.clone()))
            .collect();
        for (key, mut child) in own {
            let Some(pm) = parent.methods.get(&key) else {
                continue;
            };
            // __toString() implicitly returns string.
            if key.as_slice() == b"__tostring" && child.op_array.return_type.is_none() {
                std::rc::Rc::make_mut(&mut child.op_array).return_type =
                    Some(ParamType::Simple(b"string".to_vec()));
            }
            if pm.visibility == Visibility::Private {
                continue;
            }
            let line = if child.op_array.decl_line > 0 {
                child.op_array.decl_line
            } else {
                decl_line
            };
            let pdecl = self.internal_class_display(&pm.declaring_class);
            let mname = String::from_utf8_lossy(&child.name).to_string();
            if pm.is_final {
                return Err((
                    format!("Cannot override final method {}::{}()", pdecl, mname),
                    line,
                    false,
                ));
            }
            if pm.is_static != child.is_static {
                let msg = if child.is_static {
                    format!(
                        "Cannot make non static method {}::{}() static in class {}",
                        pdecl,
                        mname,
                        String::from_utf8_lossy(&class.name)
                    )
                } else {
                    format!(
                        "Cannot make static method {}::{}() non static in class {}",
                        pdecl,
                        mname,
                        String::from_utf8_lossy(&class.name)
                    )
                };
                return Err((msg, line, false));
            }
            let level = |v: Visibility| match v {
                Visibility::Public => 0,
                Visibility::Protected => 1,
                Visibility::Private => 2,
            };
            if key.as_slice() != b"__construct" && level(child.visibility) > level(pm.visibility) {
                let (vis, suffix) = if pm.visibility == Visibility::Public {
                    ("public", "")
                } else {
                    ("protected", " or weaker")
                };
                return Err((
                    format!(
                        "Access level to {}::{}() must be {} (as in class {}){}",
                        String::from_utf8_lossy(&class.name),
                        mname,
                        vis,
                        self.internal_class_display(&parent.name),
                        suffix
                    ),
                    line,
                    false,
                ));
            }
            // Constructors only have a prototype when abstract.
            if key.as_slice() == b"__construct" && !pm.is_abstract {
                continue;
            }
            let tentative = crate::stubs::find_method(&pm.declaring_class, &key)
                .is_some_and(|(_, f)| f.flags & crate::stubs::F_TENTATIVE != 0);
            if tentative && pm.op_array.return_type.is_some() {
                // The tentative return type only warns: check the rest first.
                let mut relaxed = pm.clone();
                std::rc::Rc::make_mut(&mut relaxed.op_array).return_type = None;
                if let Some((msg, unresolved)) =
                    self.variance_check_method(class, &child, &parent.name, &relaxed)
                {
                    // Report against the full signature.
                    let msg = self
                        .variance_check_method(class, &child, &parent.name, pm)
                        .map(|(m, _)| m)
                        .unwrap_or(msg);
                    return Err((msg, line, unresolved));
                }
                if let Some((msg, unresolved)) =
                    self.variance_check_method(class, &child, &parent.name, pm)
                {
                    if unresolved {
                        return Err((msg, line, true));
                    }
                    let will_change = child.attributes.iter().any(|a| {
                        let n = a.name.strip_prefix(b"\\").unwrap_or(&a.name);
                        n.eq_ignore_ascii_case(b"ReturnTypeWillChange")
                    });
                    if !will_change {
                        if let Some(rest) = msg.strip_prefix("Declaration of ") {
                            if let Some((c, p)) = rest.split_once(" must be compatible with ") {
                                let dep = format!(
                                    "Return type of {} should either be compatible with {}, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice",
                                    c, p
                                );
                                self.emit_deprecated_at(&dep, line);
                                if self.current_exception.is_some() {
                                    return Err((dep, line, false));
                                }
                            }
                        }
                    }
                }
                continue;
            }
            if let Some((msg, unresolved)) =
                self.variance_check_method(class, &child, &parent.name, pm)
            {
                return Err((msg, line, unresolved));
            }
        }
        Ok(())
    }
}

impl Vm {
    /// The value of class constant `name` of class `class_lower` (searched
    /// through the parents) for reflection: a constant expression is
    /// evaluated and cached, enum cases resolved and a typed constant
    /// verified. None if it does not exist; Some(Err) when an exception was
    /// thrown (left pending).
    pub fn class_constant_value(
        &mut self,
        class_lower: &[u8],
        name: &[u8],
    ) -> Option<Result<Value, ()>> {
        // The class in the chain that holds the constant.
        let mut holder = class_lower.to_vec();
        let raw = loop {
            let c = self.classes.get(&holder)?;
            if let Some(v) = c.constants.get(name) {
                break v.clone();
            }
            holder = c.parent.as_ref()?.to_ascii_lowercase();
        };
        let (decl, declared_type) = self
            .classes
            .get(&holder)
            .and_then(|c| c.constants_meta.get(name))
            .map(|m| {
                (
                    m.declaring_class.to_ascii_lowercase(),
                    m.declared_type.clone(),
                )
            })
            .unwrap_or_else(|| (holder.clone(), None));
        let mut val = raw;
        if let Some(t) = thunk_marker_name(&val).map(|t| t.to_vec()) {
            let what = format!(
                "{}::{}",
                self.sp_class_name_of(&decl),
                String::from_utf8_lossy(name)
            );
            match self.eval_const_thunk(&t, Some(&decl), &what) {
                Ok(v) => {
                    for k in [&decl[..], &holder[..]] {
                        if let Some(slot) = self
                            .classes
                            .get_mut(k)
                            .and_then(|c| c.constants.get_mut(name))
                        {
                            if thunk_marker_name(slot) == Some(&t[..]) {
                                *slot = v.clone();
                            }
                        }
                    }
                    val = v;
                }
                Err(()) => return Some(Err(())),
            }
        }
        if let Value::String(s) = &val {
            if let Some(case) = s.as_bytes().strip_prefix(b"__enum_case__::") {
                if let Some(obj) = self.get_enum_case(&holder, case) {
                    val = obj;
                }
            }
        }
        if let Some(t) = declared_type {
            let mut v2 = val.clone();
            Self::coerce_value_to_type(&mut v2, &t, true);
            if !self.value_satisfies_type_strict(&v2, &t, Some(&decl)) {
                let tstr = super::variance::type_to_string(
                    &super::variance::TypeView::from_param_type(&t),
                    None,
                );
                let msg = format!(
                    "Cannot assign {} to class constant {}::{} of type {}",
                    Self::value_type_name(&v2),
                    self.sp_class_name_of(&decl),
                    String::from_utf8_lossy(name),
                    tstr
                );
                let line = self.current_line;
                let exc = self.create_exception(b"TypeError", &msg, line);
                self.current_exception = Some(exc);
                return Some(Err(()));
            }
            val = v2;
        }
        Some(Ok(val))
    }

    /// Canonical name of a class (lowercase key).
    fn sp_class_name_of(&self, key: &[u8]) -> String {
        self.classes
            .get(key)
            .map(|c| String::from_utf8_lossy(&c.name).to_string())
            .unwrap_or_else(|| String::from_utf8_lossy(key).to_string())
    }
}

impl Vm {
    /// Visibility of instance property `name` when it is declared by an
    /// internal class in the chain of `class_lower` (not by a user class):
    /// (visibility, lowercase internal class). None otherwise.
    pub fn internal_prop_visibility(
        &self,
        class_lower: &[u8],
        name: &[u8],
    ) -> Option<(Visibility, Vec<u8>)> {
        let mut cur = class_lower.to_vec();
        for _ in 0..64 {
            match self.classes.get(&cur) {
                Some(c) if !c.properties.is_empty() || c.parent.is_some() => {
                    if c.properties.iter().any(|p| p.name == name && !p.is_static) {
                        return None;
                    }
                    if crate::stubs::find_class(&cur).is_none() {
                        cur = c.parent.as_ref()?.to_ascii_lowercase();
                        continue;
                    }
                }
                _ => {}
            }
            let se = crate::stubs::class_entry(self, &cur)?;
            let p = se
                .properties
                .iter()
                .find(|p| p.name == name && !p.is_static)?;
            return Some((p.visibility, p.declaring_class.to_ascii_lowercase()));
        }
        None
    }
}
