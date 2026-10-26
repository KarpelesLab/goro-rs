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
    /// A private property inherited from a user class is kept under its bare
    /// name until a dynamic property of the same name is created on the
    /// object: it then moves to this (mangled) key. See
    /// `Vm::prop_access_key`.
    pub moved_key: Option<Vec<u8>>,
    /// Default of a property of an internal class (stub source text)
    pub internal_default: Option<&'static str>,
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

/// The declared instance property slots of a class.
#[derive(Debug)]
pub struct ClassLayout {
    pub slots: Vec<PropSlot>,
    /// Names of the private slots not stored under their bare name (or that
    /// may move away from it): the accesses that need `prop_access_key`.
    pub special_names: Vec<Vec<u8>>,
    /// For each special name: Some(slot index) when it is the only slot of
    /// that name and a movable private one (the common case of an
    /// ancestor's private property), resolved without the full lookup.
    pub special_movable: Vec<Option<usize>>,
    /// The table follows the storage order: classes registered by
    /// extensions keep their properties in the order they create them.
    pub storage_order: bool,
}

std::thread_local! {
    /// Class layouts by lowercase class name, valid while the class table
    /// has the recorded size (classes are only ever added).
    /// (by lowercase name, by name as stored in objects)
    #[allow(clippy::type_complexity)]
    static LAYOUTS: RefCell<(
        usize,
        crate::fxhash::FxHashMap<Vec<u8>, Rc<ClassLayout>>,
        crate::fxhash::FxHashMap<Vec<u8>, Rc<ClassLayout>>,
    )> = RefCell::new((0, Default::default(), Default::default()));
}

std::thread_local! {
    /// The layout class_layout_of() returned last: (class table size,
    /// class name, layout).
    #[allow(clippy::type_complexity)]
    static LAST_LAYOUT: RefCell<(usize, Vec<u8>, Option<Rc<ClassLayout>>)> =
        const { RefCell::new((0, Vec::new(), None)) };
}

/// Forget the cached class layouts (a class was declared).
pub fn invalidate_class_layouts() {
    LAST_LAYOUT.with(|c| c.borrow_mut().2 = None);
    LAYOUTS.with(|c| {
        let mut c = c.borrow_mut();
        c.0 = usize::MAX;
        c.1.clear();
        c.2.clear();
    });
}

/// VM bookkeeping stored among the properties.
/// zend_unmangle_property_name_ex(): the (class, name) of a property table
/// key, class None for an unmangled one. A key starting with a NUL that is
/// no mangled name is Err with the notice PHP raises for it (the name is
/// then the whole key).
pub fn unmangle_property_name_ex(key: &[u8]) -> Result<(Option<&[u8]>, &[u8]), &'static str> {
    let len = key.len();
    if len == 0 || key[0] != 0 {
        return Ok((None, key));
    }
    if len < 3 || key[1] == 0 {
        return Err("Illegal member variable name");
    }
    let strnlen = |s: &[u8]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
    let mut class_len = strnlen(&key[1..len - 1]);
    if class_len >= len - 2 {
        return Err("Corrupt member variable name");
    }
    // (an anonymous class name holds a NUL itself)
    let anon_len = strnlen(&key[class_len + 2..]);
    if class_len + anon_len + 2 != len {
        class_len += anon_len + 1;
    }
    Ok((Some(&key[1..1 + class_len]), &key[class_len + 2..]))
}

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
        b"__hash_",
        b"__hmac_",
        b"__pkey_id",
        b"__cert_id",
        b"__csr_id",
        b"__xml_",
        b"__dir_",
        b"__rng_",
        b"__sensitive_value",
    ];
    name.starts_with(b"__")
        && (P.iter().any(|p| name.starts_with(p))
            // ReflectionType and DateInterval state
            || matches!(
                name,
                b"__type_name"
                    | b"__allows_null"
                    | b"__is_builtin"
                    | b"__types"
                    | b"__y"
                    | b"__m"
                    | b"__d"
                    | b"__h"
                    | b"__i"
                    | b"__s"
                    | b"__f"
                    | b"__invert"
            ))
}

/// Internal property of an object whose property table has an order of its
/// own (ArrayObject sorted it): the table follows the storage order.
pub const OWN_ORDER: &[u8] = b"__spl_own_order";

/// The rows of `ob`'s property table, in table order: declared slots (with
/// the index of their storage entry, if any), then dynamic properties.
fn table_rows<'a>(
    ob: &PhpObject,
    layout: &'a ClassLayout,
) -> Vec<(Option<&'a PropSlot>, Option<usize>)> {
    let slots = &layout.slots;
    let props = &ob.properties;
    let find = |slot: &PropSlot| match &slot.moved_key {
        Some(mk) => props
            .iter()
            .position(|(k, _)| k == mk)
            .or_else(|| props.iter().position(|(k, _)| *k == slot.storage_key)),
        None => props.iter().position(|(k, _)| *k == slot.storage_key),
    };
    let slot_at: Vec<Option<usize>> = slots.iter().map(find).collect();
    let mut owner: Vec<Option<usize>> = vec![None; props.len()];
    for (si, at) in slot_at.iter().enumerate() {
        if let Some(i) = at {
            owner[*i] = Some(si);
        }
    }
    let dynamic = |i: usize| {
        let (k, v) = &props[i];
        !is_internal_prop_name(k) && !matches!(v, Value::Undef)
    };
    let mut out = Vec::with_capacity(slots.len() + props.len());
    if layout.storage_order || ob.has_property(OWN_ORDER) {
        for (i, own) in owner.iter().enumerate() {
            match own {
                Some(si) => out.push((Some(&slots[*si]), Some(i))),
                None if dynamic(i) => out.push((None, Some(i))),
                None => {}
            }
        }
        // Slots without a storage entry come last.
        for (si, slot) in slots.iter().enumerate() {
            if slot_at[si].is_none() {
                out.push((Some(slot), None));
            }
        }
        return out;
    }
    for (si, slot) in slots.iter().enumerate() {
        out.push((Some(slot), slot_at[si]));
    }
    for (i, own) in owner.iter().enumerate() {
        if own.is_none() && dynamic(i) {
            out.push((None, Some(i)));
        }
    }
    out
}

/// A view of a lazy object's properties table: building one for a ghost not
/// initialized yet creates the table (rebuild_object_properties(), noted as
/// `__lazy_ht`); a table its initialization separated (`__lazy_frozen`,
/// see PhpObject::lazy_ht_separate) shows the declared values it copied.
fn lazy_ht_view(obj: &Rc<RefCell<PhpObject>>) -> Option<crate::array::PhpArray> {
    let (ghost, has_ht, frozen) = {
        let ob = obj.borrow();
        let mut ghost = false;
        let mut has_ht = false;
        let mut frozen = None;
        for (k, v) in &ob.properties {
            if !k.starts_with(b"__lazy_") {
                continue;
            }
            match k.as_slice() {
                b"__lazy_state" => {
                    ghost = matches!(v, Value::String(s) if s.as_bytes() == b"ghost")
                }
                b"__lazy_ht" => has_ht = true,
                b"__lazy_frozen" => {
                    if let Value::Array(a) = v {
                        frozen = Some(a.borrow().clone());
                    }
                }
                _ => {}
            }
        }
        (ghost, has_ht, frozen)
    };
    if ghost && !has_ht {
        if let Ok(mut b) = obj.try_borrow_mut() {
            b.properties.push((b"__lazy_ht".to_vec(), Value::True));
        }
    }
    frozen
}

/// The key a slot's value has in a `__lazy_frozen` table.
fn frozen_key(obj: &Rc<RefCell<PhpObject>>, slot: &PropSlot) -> crate::array::ArrayKey {
    let ob = obj.borrow();
    let key = match &slot.moved_key {
        Some(mk) if ob.has_property(mk) => mk.clone(),
        _ => slot.storage_key.clone(),
    };
    crate::array::ArrayKey::String(crate::string::PhpString::from_vec(key))
}

/// The value of a simple stub default (`""`, `0`, `null`, `[]`, ...).
/// The value a slot the object stores nothing for has when it moves to its
/// mangled key: an internal slot's default, else unset.
fn slot_unset_value(slot: &PropSlot) -> Value {
    slot.internal_default
        .and_then(stub_default_value)
        .unwrap_or(Value::Undef)
}

fn stub_default_value(src: &str) -> Option<Value> {
    let src = src.trim();
    Some(match src {
        "null" | "NULL" => Value::Null,
        "true" => Value::True,
        "false" => Value::False,
        "\"\"" | "''" => Value::String(crate::string::PhpString::empty()),
        "[]" => Value::Array(Rc::new(RefCell::new(crate::array::PhpArray::new()))),
        _ => {
            if let Ok(n) = src.parse::<i64>() {
                Value::Long(n)
            } else {
                return None;
            }
        }
    })
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
                // An extension class registered as a bare entry (DOMException):
                // its stub declares its properties.
                Some(c)
                    if c.filename.is_none()
                        && c.start_line == 0
                        && c.properties.is_empty()
                        && crate::stubs::find_class(&cur).is_some() =>
                {
                    internal = crate::stubs::find_class(&cur);
                    break;
                }
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
                // ext/date's objects build their property tables themselves
                // (date_object_get_properties_for): no slots of their own.
                if c.ext == "date" {
                    continue;
                }
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
                        moved_key: None,
                        internal_default: p.default,
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
            // topmost declaration with storage (a property its parent left
            // virtual gets a new slot, in the child's declaration order).
            let mut defs: Vec<(usize, usize, usize, &crate::object::PropertyDef)> = ce
                .properties
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.is_static)
                .map(|(i, p)| {
                    let decl = lower(&p.declaring_class);
                    let root = if p.visibility == Visibility::Private {
                        decl
                    } else {
                        self.slot_root_of(&decl, &p.name)
                    };
                    (depth(&root), self.own_decl_pos(&root, &p.name), i, p)
                })
                .collect();
            defs.sort_by_key(|(d, pos, i, _)| (*d, *pos, *i));
            for (_, _, _, p) in defs {
                let decl = lower(&p.declaring_class);
                let slot = PropSlot {
                    name: p.name.clone(),
                    decl_name: self.class_display_name(&decl),
                    decl,
                    vis: p.visibility,
                    has_get_hook: p.has_get_hook,
                    is_virtual: p.is_virtual,
                    storage_key: p.name.clone(),
                    moved_key: None,
                    internal_default: None,
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
                } else if s.vis == Visibility::Private
                    && s.decl.as_slice() != cl
                    && (self.classes.contains_key(&s.decl)
                        // Exception's / Error's privates (see
                        // PhpObject::throwable_get)
                        || matches!(s.decl.as_slice(), b"exception" | b"error"))
                {
                    s.moved_key = Some(mangle(&s.name, Visibility::Private, &s.decl_name));
                }
            }
        }
        slots
    }

    /// Whether property `name` of an object of class `cl` may be accessed
    /// from the current scope (a dynamic property always may).
    pub(crate) fn prop_visible_from_scope(&self, cl: &[u8], name: &[u8]) -> bool {
        let Some(ce) = self.prop_view_class(cl) else {
            return true;
        };
        if ce.parent.is_some() && !ce.properties.iter().any(|p| p.name == name) {
            // (maybe an internal ancestor's property)
            return self.internal_prop_visible(cl, name);
        }
        let scope = self.class_scope_ref();
        let in_scope = |d: &[u8]| scope.is_some_and(|s| s.eq_ignore_ascii_case(d));
        let mut non_private = None;
        let mut private = None;
        for p in ce.properties.iter().filter(|p| p.name == name) {
            match p.visibility {
                Visibility::Private if in_scope(&p.declaring_class) => return true,
                Visibility::Private => private = Some(p),
                v => {
                    if non_private.is_none() {
                        non_private = Some(v)
                    }
                }
            }
        }
        match non_private {
            Some(Visibility::Public) => true,
            // is_protected_compatible_scope(property_info->prototype->ce,
            // scope): relative to the class that first declared the
            // property, so a sibling subclass's method sees it too.
            Some(_) => scope.is_some_and(|s| {
                let s = lower(s);
                let proto = self.protected_prop_prototype_class(cl, name);
                s.as_slice() == proto.as_slice()
                    || self.class_extends(&s, &proto)
                    || self.class_extends(&proto, &s)
            }),
            // An ancestor's private property: the name is a dynamic one.
            None => private.is_none_or(|p| !p.declaring_class.eq_ignore_ascii_case(cl)),
        }
    }

    /// The topmost class in `cl`'s ancestry (lowercase) declaring a
    /// non-private property `name`: its prototype's class.
    #[inline(never)]
    fn protected_prop_prototype_class(&self, cl: &[u8], name: &[u8]) -> Vec<u8> {
        let mut proto = cl.to_vec();
        let mut cur = cl.to_vec();
        for _ in 0..64 {
            let Some(ce) = self.classes.get(&cur) else {
                break;
            };
            if ce
                .properties
                .iter()
                .any(|p| p.name == name && p.visibility != Visibility::Private && !p.is_static)
            {
                proto = cur.clone();
            }
            match &ce.parent {
                Some(p) => cur = lower(p),
                None => break,
            }
        }
        proto
    }

    /// prop_visible_from_scope() of a property the class entry does not
    /// list: one declared by an internal ancestor, or a dynamic one.
    #[inline(never)]
    fn internal_prop_visible(&self, cl: &[u8], name: &[u8]) -> bool {
        let scope = self.current_class_scope().map(|s| lower(&s));
        let Some((vis, decl, ..)) = self.find_property_def_for_scope(cl, name, scope.as_deref())
        else {
            return true;
        };
        self.check_visibility(vis, &decl, cl, "", true, scope.as_deref())
            .is_none()
    }

    /// zend_std_has_property(ZEND_PROPERTY_EXISTS) from the current scope:
    /// the slot the access refers to is initialized, or the dynamic
    /// property exists.
    pub fn prop_exists_from_scope(&self, obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> bool {
        let cl = lower(&obj.borrow().class_name);
        match self.scoped_prop_info(&cl, name) {
            Ok(Some(slot)) => {
                let b = obj.borrow();
                let v = slot
                    .moved_key
                    .as_ref()
                    .and_then(|mk| b.properties.iter().find(|(k, _)| k == mk))
                    .or_else(|| b.properties.iter().find(|(k, _)| *k == slot.storage_key));
                v.is_some_and(|(_, v)| !matches!(v, Value::Undef))
            }
            Ok(None) => self
                .object_prop_table(obj)
                .iter()
                .any(|e| e.slot.is_none() && e.key == name),
            Err(()) => false,
        }
    }

    /// Whether declared property `p` of class `cl` is invisible to the
    /// current scope as a slot: an ancestor's private property (accesses to
    /// its name find a dynamic property).
    pub fn prop_def_hidden(&self, p: &crate::object::PropertyDef, cl: &[u8]) -> bool {
        p.visibility == Visibility::Private
            && !p.declaring_class.eq_ignore_ascii_case(cl)
            && self
                .current_class_scope()
                .is_none_or(|s| !p.declaring_class.eq_ignore_ascii_case(&s))
    }

    /// The storage key of property `name` of `obj` for an access from the
    /// current scope (see prop_access_key).
    pub fn prop_key_for<'n>(&self, obj: &Value, name: &'n [u8]) -> std::borrow::Cow<'n, [u8]> {
        match match obj {
            Value::Object(o) => self.prop_access_key(o, name),
            Value::Reference(r) => match &*r.borrow() {
                Value::Object(o) => self.prop_access_key(o, name),
                _ => None,
            },
            _ => None,
        } {
            Some(k) => std::borrow::Cow::Owned(k),
            None => std::borrow::Cow::Borrowed(name),
        }
    }

    /// class_layout() by an object's class name (as stored in the object).
    pub fn class_layout_of(&self, class_name: &[u8]) -> Rc<ClassLayout> {
        let n = self.classes.len();
        if let Some(l) = LAST_LAYOUT.with(|c| {
            let c = c.borrow();
            (c.0 == n && c.1 == class_name)
                .then(|| c.2.clone())
                .flatten()
        }) {
            return l;
        }
        let l = self.class_layout_of_slow(class_name);
        LAST_LAYOUT.with(|c| *c.borrow_mut() = (n, class_name.to_vec(), Some(l.clone())));
        l
    }

    fn class_layout_of_slow(&self, class_name: &[u8]) -> Rc<ClassLayout> {
        let n = self.classes.len();
        if let Some(l) = LAYOUTS.with(|c| {
            let c = c.borrow();
            if c.0 != n {
                return None;
            }
            c.2.get(class_name).cloned()
        }) {
            return l;
        }
        let l = self.class_layout(&lower(class_name));
        LAYOUTS.with(|c| c.borrow_mut().2.insert(class_name.to_vec(), l.clone()));
        l
    }

    /// The class's property layout (object_prop_slots), cached.
    pub fn class_layout(&self, cl: &[u8]) -> Rc<ClassLayout> {
        let n = self.classes.len();
        if let Some(l) = LAYOUTS.with(|c| {
            let mut c = c.borrow_mut();
            if c.0 != n {
                c.0 = n;
                c.1.clear();
                c.2.clear();
            }
            c.1.get(cl).cloned()
        }) {
            return l;
        }
        let slots = self.object_prop_slots(cl);
        let special_names: Vec<Vec<u8>> = slots
            .iter()
            .filter(|s| s.moved_key.is_some() || s.storage_key != s.name)
            .map(|s| s.name.clone())
            .collect();
        let special_movable = special_names
            .iter()
            .map(|n| {
                let mut it = slots.iter().enumerate().filter(|(_, s)| s.name == *n);
                match (it.next(), it.next()) {
                    (Some((i, s)), None) if s.moved_key.is_some() => Some(i),
                    _ => None,
                }
            })
            .collect();
        let mut storage_order = false;
        let mut cur = cl.to_vec();
        for _ in 0..64 {
            let Some(c) = self.classes.get(&cur) else {
                break;
            };
            if c.filename.is_none() {
                // (an internal throwable, registered as a bare entry of a
                // stub class or declared by an extension's prelude, has the
                // slots of any exception: Exception's, then its own)
                storage_order =
                    !(c.start_line == 0 && self.class_extends_any(&cur, &[b"exception", b"error"]));
                break;
            }
            match &c.parent {
                Some(p) => cur = lower(p),
                None => break,
            }
        }
        let l = Rc::new(ClassLayout {
            slots,
            special_names,
            special_movable,
            storage_order,
        });
        LAYOUTS.with(|c| c.borrow_mut().1.insert(cl.to_vec(), l.clone()));
        l
    }

    /// The key in `obj`'s storage of the property `name` an access from the
    /// current scope refers to, when it isn't `name` itself: a private
    /// property stored under its mangled name (zend_get_property_offset()
    /// picks the slot by scope). An access that finds a dynamic property
    /// where an inherited private one is stored under the bare name first
    /// moves that one to its mangled key, so the bare name is the dynamic
    /// property's.
    pub fn prop_access_key(&self, obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Option<Vec<u8>> {
        let layout = {
            let b = obj.borrow();
            if b.native.is_some() {
                return None;
            }
            let n = self.classes.len();
            let hit = LAST_LAYOUT.with(|c| {
                let c = c.borrow();
                if c.0 != n || c.1 != b.class_name {
                    return None;
                }
                c.2.as_ref()
                    .map(|l| l.special_names.iter().any(|s| s == name))
            });
            if hit == Some(false) {
                return None;
            }
            self.class_layout_of(&b.class_name)
        };
        let si = layout.special_names.iter().position(|n| n == name)?;
        if let Some(i) = layout.special_movable[si] {
            // An ancestor's private property, the only slot of that name: it
            // is the one accessed from its class, else the name is dynamic.
            let slot = &layout.slots[i];
            let mk = slot.moved_key.as_ref()?;
            let from_decl = self
                .class_scope_ref()
                .is_some_and(|s| s.eq_ignore_ascii_case(&slot.decl));
            let mut b = obj.borrow_mut();
            let moved = b.has_property(mk);
            if from_decl {
                return moved.then(|| mk.clone());
            }
            if !moved {
                match b.properties.iter().position(|(k, _)| k.as_slice() == name) {
                    Some(i) => b.properties.rename(i, mk.clone()),
                    // (an internal slot the object kept no value for has
                    // its default: Exception::$string)
                    None => b.properties.push((mk.clone(), slot_unset_value(slot))),
                }
            }
            return None;
        }
        let cl = lower(&obj.borrow().class_name);
        let scope = self.current_class_scope().map(|s| lower(&s));
        match self.scoped_prop_info_in(&layout.slots, &cl, name, scope.as_deref()) {
            Ok(Some(slot)) => {
                if let Some(mk) = &slot.moved_key {
                    let has = obj.borrow().has_property(mk);
                    return has.then(|| mk.clone());
                }
                (slot.storage_key != name).then_some(slot.storage_key)
            }
            Ok(None) => {
                // A dynamic property: move the private one out of its way
                // (an unset one leaves an undef marker, so the bare name is
                // known to be the dynamic property's from now on).
                for s in layout.slots.iter().filter(|s| s.name == name) {
                    let Some(mk) = &s.moved_key else { continue };
                    let mut b = obj.borrow_mut();
                    if b.has_property(mk) {
                        continue;
                    }
                    match b.properties.iter().position(|(k, _)| k.as_slice() == name) {
                        Some(i) => b.properties.rename(i, mk.clone()),
                        None => b.properties.push((mk.clone(), slot_unset_value(s))),
                    }
                }
                None
            }
            Err(()) => None,
        }
    }

    /// The property table of `obj` (zend_std_get_properties): declared slots
    /// in slot order (uninitialized ones skipped, except hooked properties,
    /// which have no backing value to show), then dynamic properties.
    /// The property table as zend_hooked_object_build_properties() orders it
    /// for a class with hooked properties (get_object_vars(), json_encode()):
    /// the declared properties in property-info order (inherited first, a
    /// redeclared one at its first declaration), then the dynamic ones. Slot
    /// order differs when a child gives storage to a property its parent
    /// left virtual. Other classes: object_prop_table().
    pub fn object_prop_table_info_order(&self, obj: &Rc<RefCell<PhpObject>>) -> Vec<PropEntry> {
        let mut t = if lazy_ht_view(obj).is_none() {
            self.object_prop_table_live_ex(obj, true)
        } else {
            self.object_prop_table(obj)
        };
        let cl = obj.borrow().class_name.to_ascii_lowercase();
        let Some(ce) = self.classes.get(&cl) else {
            return t;
        };
        if !ce
            .properties
            .iter()
            .any(|p| p.has_get_hook || p.has_set_hook)
        {
            return t;
        }
        let pos = |e: &PropEntry| -> usize {
            match &e.slot {
                Some(s) => ce
                    .properties
                    .iter()
                    .filter(|p| !p.is_static)
                    .position(|p| {
                        p.name == s.name
                            && (s.vis != Visibility::Private
                                || p.declaring_class.eq_ignore_ascii_case(&s.decl))
                    })
                    .unwrap_or(usize::MAX - 1),
                None => usize::MAX,
            }
        };
        t.sort_by_cached_key(pos);
        t
    }

    pub fn object_prop_table(&self, obj: &Rc<RefCell<PhpObject>>) -> Vec<PropEntry> {
        let mut out = self.object_prop_table_live(obj);
        if let Some(frozen) = lazy_ht_view(obj) {
            out.retain_mut(|e| match &e.slot {
                Some(slot) => match frozen.get(&frozen_key(obj, slot)) {
                    Some(v) => {
                        e.value = v.clone();
                        true
                    }
                    None => false,
                },
                None => true,
            });
        }
        out
    }

    fn object_prop_table_live(&self, obj: &Rc<RefCell<PhpObject>>) -> Vec<PropEntry> {
        self.object_prop_table_live_ex(obj, false)
    }

    /// With `hooked_null`, an uninitialized backed property with only a set
    /// hook is NULL (the silent zend_read_property_ex() of
    /// zho_build_properties consumers: get_object_vars(), json_encode()...).
    fn object_prop_table_live_ex(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        hooked_null: bool,
    ) -> Vec<PropEntry> {
        let ob = obj.borrow();
        let layout = self.class_layout_of(&ob.class_name);
        let props = &ob.properties;
        let mut out = Vec::with_capacity(props.len());
        for (slot, at) in table_rows(&ob, &layout) {
            match (slot, at) {
                (Some(slot), Some(i)) => {
                    if matches!(props[i].1, Value::Undef) && !slot.has_get_hook {
                        if hooked_null
                            && self.classes.get(&slot.decl).is_some_and(|c| {
                                c.properties
                                    .iter()
                                    .any(|p| p.name == slot.name && !p.is_static && p.has_set_hook)
                            })
                        {
                            out.push(PropEntry {
                                key: slot.mangled(),
                                value: Value::Null,
                                slot: Some(slot.clone()),
                            });
                        }
                        continue;
                    }
                    out.push(PropEntry {
                        key: slot.mangled(),
                        value: props[i].1.clone(),
                        slot: Some(slot.clone()),
                    });
                }
                (Some(slot), None) if slot.has_get_hook => out.push(PropEntry {
                    key: slot.mangled(),
                    value: Value::Undef,
                    slot: Some(slot.clone()),
                }),
                // A slot of an internal class the object keeps no value
                // for has its default (Exception::$string).
                (Some(slot), None) => {
                    if let Some(v) = slot.internal_default.and_then(stub_default_value) {
                        if !self.classes.contains_key(&slot.decl) {
                            out.push(PropEntry {
                                key: slot.mangled(),
                                value: v,
                                slot: Some(slot.clone()),
                            });
                        }
                    }
                }
                (None, Some(i)) => out.push(PropEntry {
                    key: props[i].0.clone(),
                    value: props[i].1.clone(),
                    slot: None,
                }),
                (None, None) => {}
            }
        }
        out
    }

    /// The property table php_var_dump() / print_r() walk
    /// (zend_get_properties_for(DEBUG) of a standard object): the table
    /// keys (mangled for declared private/protected properties) with their
    /// values; an uninitialized typed slot comes with its type (shown as
    /// `uninitialized(type)`), virtual properties have no entry.
    pub fn object_debug_table(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Vec<(Vec<u8>, Value, Option<String>)> {
        let mut out = self.object_debug_table_live(obj);
        if let Some(frozen) = lazy_ht_view(obj) {
            let ob = obj.borrow();
            let layout = self.class_layout_of(&ob.class_name);
            drop(ob);
            out.retain_mut(|(k, v, t)| {
                let Some(slot) = layout.slots.iter().find(|s| s.mangled() == *k) else {
                    return true;
                };
                match frozen.get(&frozen_key(obj, slot)) {
                    Some(fv) => {
                        *v = fv.clone();
                        *t = None;
                        true
                    }
                    None => false,
                }
            });
        }
        out
    }

    fn object_debug_table_live(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Vec<(Vec<u8>, Value, Option<String>)> {
        let ob = obj.borrow();
        let layout = self.class_layout_of(&ob.class_name);
        let props = &ob.properties;
        let mut out = Vec::with_capacity(props.len());
        for (slot, at) in table_rows(&ob, &layout) {
            let Some(slot) = slot else {
                if let Some(i) = at {
                    out.push((props[i].0.clone(), props[i].1.clone(), None));
                }
                continue;
            };
            if slot.is_virtual {
                continue;
            }
            let internal = !self.classes.contains_key(&slot.decl);
            let value = match at {
                Some(i) => props[i].1.clone(),
                None => match slot.internal_default.and_then(stub_default_value) {
                    Some(v) => v,
                    None => Value::Undef,
                },
            };
            if matches!(value, Value::Undef) {
                // (internal objects keep their state elsewhere, unless the
                // slot was explicitly left uninitialized)
                if internal && at.is_none() {
                    continue;
                }
                match self.slot_type_name(slot) {
                    Some(t) => out.push((slot.mangled(), Value::Undef, Some(t))),
                    None => continue,
                }
            } else {
                out.push((slot.mangled(), value, None));
            }
        }
        out
    }

    /// The declared type of a slot, as var_dump() shows it.
    fn slot_type_name(&self, slot: &PropSlot) -> Option<String> {
        if let Some(c) = self.classes.get(&slot.decl) {
            let p = c
                .properties
                .iter()
                .find(|p| {
                    p.name == slot.name && !p.is_static && lower(&p.declaring_class) == slot.decl
                })
                .or_else(|| {
                    c.properties
                        .iter()
                        .find(|p| p.name == slot.name && !p.is_static)
                })?;
            return p.property_type.as_ref().map(|t| self.param_type_name(t));
        }
        let c = crate::stubs::find_class(&slot.decl)?;
        let p = c.props.iter().find(|p| p.name.as_bytes() == slot.name)?;
        (!p.ty.is_empty()).then(|| p.ty.to_string())
    }

    /// The property info zend_get_property_info() finds for `name` on class
    /// `cl` from the current scope: Ok(Some(slot)) when accessible, Ok(None)
    /// when the name is a dynamic property, Err(()) when inaccessible
    /// (ZEND_WRONG_PROPERTY_INFO).
    pub(crate) fn scoped_prop_info(&self, cl: &[u8], name: &[u8]) -> Result<Option<PropSlot>, ()> {
        let layout = self.class_layout(cl);
        let scope = self.get_current_class_name().map(|s| lower(&s));
        self.scoped_prop_info_in(&layout.slots, cl, name, scope.as_deref())
    }

    fn scoped_prop_info_in(
        &self,
        slots: &[PropSlot],
        cl: &[u8],
        name: &[u8],
        scope: Option<&[u8]>,
    ) -> Result<Option<PropSlot>, ()> {
        let scope = scope.map(|s| s.to_vec());
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
        if info.vis == Visibility::Public && !self.prop_changed(slots, &info) {
            return Ok(Some(info));
        }
        if scope.as_deref() == Some(info.decl.as_slice()) {
            return Ok(Some(info));
        }
        if self.prop_changed(slots, &info) {
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
                let root = self.prop_root_class(slots, &info);
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

    /// The topmost ancestor of `decl` declaring non-private property `name`
    /// with storage (the class whose slot the property uses).
    fn slot_root_of(&self, decl: &[u8], name: &[u8]) -> Vec<u8> {
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
                        && !d.is_virtual
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

    /// Position of `name` among the properties class `cl` itself declares.
    fn own_decl_pos(&self, cl: &[u8], name: &[u8]) -> usize {
        let Some(c) = self.classes.get(cl) else {
            return 0;
        };
        if c.own_prop_order.is_empty() {
            c.properties.iter().position(|p| p.name == name)
        } else {
            c.own_prop_order.iter().position(|n| n == name)
        }
        .unwrap_or(usize::MAX)
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
