use indexmap::IndexMap;

use crate::opcode::OpArray;
use crate::value::Value;

/// A trait adaptation rule
#[derive(Debug, Clone)]
pub enum TraitAdaptation {
    /// Alias: trait_name::method as [visibility] new_name
    Alias {
        trait_name: Option<Vec<u8>>,
        method: Vec<u8>,
        new_name: Option<Vec<u8>>,
        new_visibility: Option<Visibility>,
        /// `as final`
        new_final: bool,
    },
    /// Precedence: trait_name::method insteadof other_trait(s)
    Precedence {
        trait_name: Vec<u8>,
        method: Vec<u8>,
        instead_of: Vec<Vec<u8>>,
    },
}

#[derive(Debug, Clone)]
pub struct RuntimeAttribute {
    pub name: Vec<u8>,
    pub args_op_array: std::rc::Rc<OpArray>,
    /// A validator error delayed by #[\DelayedTargetValidation]
    /// (zend_attribute.validation_error), thrown by newInstance().
    pub validation_error: Option<String>,
}

/// Whether a call of code with attributes `attrs` has nothing to check for
/// them: none is #[\Deprecated] or #[\NoDiscard] (see
/// Vm::get_deprecated_message, Vm::emit_nodiscard_warning), the only ones a
/// call looks at (#[\Override], #[\ReturnTypeWillChange] & co. are seen
/// when the class is linked).
#[inline]
pub(crate) fn call_attributes_inert(attrs: &[RuntimeAttribute]) -> bool {
    attrs.is_empty() || attrs.iter().all(|a| call_attribute_inert(&a.name))
}

#[inline(never)]
fn call_attribute_inert(name: &[u8]) -> bool {
    !(name.eq_ignore_ascii_case(b"deprecated") || name.eq_ignore_ascii_case(b"nodiscard"))
}

/// Metadata for a class constant (visibility and final flag)
#[derive(Debug, Clone)]
pub struct ConstantMeta {
    pub visibility: Visibility,
    pub is_final: bool,
    /// The class that originally declared this constant (lowercase)
    pub declaring_class: Vec<u8>,
    pub attributes: Vec<RuntimeAttribute>,
    /// Optional declared type for typed class constants (PHP 8.3+).
    pub declared_type: Option<crate::opcode::ParamType>,
    /// Doc comment for the constant (if any).
    pub doc_comment: Option<std::rc::Rc<str>>,
    /// Class constants referenced in this constant's value expression.
    /// Each pair is (class_name_lower, constant_name). Used to re-emit
    /// #[\Deprecated] banners for those referenced constants when this
    /// constant is accessed (e.g. CONST = self::PREFIX . self::SUFFIX).
    pub referenced_constants: Vec<(Vec<u8>, Vec<u8>)>,
    /// Line of the value expression (enum cases; 0 otherwise).
    pub line: u32,
}

/// A PHP class entry (class definition)
#[derive(Debug, Clone)]
pub struct ClassEntry {
    pub name: Vec<u8>,
    pub parent: Option<Vec<u8>>,
    pub interfaces: Vec<Vec<u8>>,
    pub traits: Vec<Vec<u8>>,
    pub trait_adaptations: Vec<TraitAdaptation>,
    pub properties: Vec<PropertyDef>,
    pub methods: crate::fxhash::FxIndexMap<Vec<u8>, MethodDef>,
    pub constants: crate::fxhash::FxIndexMap<Vec<u8>, Value>,
    /// Metadata for constants (visibility, final flag, declaring class)
    pub constants_meta: crate::fxhash::FxIndexMap<Vec<u8>, ConstantMeta>,
    pub static_properties: IndexMap<Vec<u8>, Value>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    pub is_interface: bool,
    pub is_trait: bool,
    pub is_enum: bool,
    /// For enums: the backing type (b"string" or b"int"), None for unit enums
    pub enum_backing_type: Option<Vec<u8>>,
    /// For enums: list of (case_name, backing_value) pairs
    /// backing_value is Value::Null for unit enums
    pub enum_cases: Vec<(Vec<u8>, Value)>,
    /// Line number where the class declaration starts
    pub start_line: u32,
    /// Line number where the class declaration ends
    pub end_line: u32,
    /// File where the class was declared
    pub filename: Option<String>,
    /// Doc comment for the class (if any)
    pub doc_comment: Option<std::rc::Rc<str>>,
    pub attributes: Vec<RuntimeAttribute>,
    /// Closures compiled from const expressions (for deferred resolution)
    pub const_closures: Vec<crate::opcode::OpArray>,
    /// Whether the class has #[AllowDynamicProperties]
    pub allow_dynamic_properties: bool,
    /// What NEW gives an object of the class, once known (see
    /// Vm::new_object_fast).
    pub new_template: std::cell::RefCell<Option<std::rc::Rc<NewTemplate>>>,
    /// The names of the properties the class declares itself, in
    /// declaration order (the start of Zend's properties_info), once
    /// inheritance put `properties` in slot order; empty when `properties`
    /// is in that order already.
    pub own_prop_order: Vec<Vec<u8>>,
}

/// What NEW gives an object of a plain user class (all its hierarchy user
/// code, constant expressions evaluated, no default needing evaluation):
/// the same for every object while the class's properties are the ones it
/// was made from.
#[derive(Debug)]
pub struct NewTemplate {
    /// The properties it was made from (a class entry copied or changed
    /// since does not use it).
    pub(crate) props_ptr: *const PropertyDef,
    pub(crate) props_len: usize,
    pub(crate) methods_len: usize,
    /// The class's declared name, and the name it is looked up by.
    pub(crate) class_name: Vec<u8>,
    pub(crate) lower: Vec<u8>,
    /// Where the class table held `lower` when last looked up (checked
    /// against its key before use: no hashing of the name).
    pub(crate) class_idx: std::cell::Cell<usize>,
    /// The declared property slots and their defaults (an array default
    /// is a placeholder here, see `arrays`).
    pub(crate) slots: PropTable,
    /// The class_tag() of the class's objects.
    pub(crate) tag: u32,
    /// The slots whose default is an array, shared copy-on-write with the
    /// class entry's default (not held here: the template adds no count).
    pub(crate) arrays: Vec<(
        usize,
        std::rc::Weak<std::cell::RefCell<crate::array::PhpArray>>,
    )>,
    /// The class has a destructor.
    pub(crate) has_destruct: bool,
    /// The class has no constructor (none declared or inherited): `new`
    /// without arguments skips the constructor call (ZEND_NEW).
    pub(crate) no_ctor: bool,
    /// The extension create_object hook its objects get (see
    /// Vm::object_create_hook_of).
    pub(crate) create_hook:
        Option<fn(&mut crate::vm::Vm, &std::rc::Rc<std::cell::RefCell<PhpObject>>)>,
}

impl NewTemplate {
    /// Made from `ce`'s properties as they are.
    #[inline]
    pub(crate) fn fits(&self, ce: &ClassEntry) -> bool {
        self.props_ptr == ce.properties.as_ptr()
            && self.props_len == ce.properties.len()
            && self.methods_len == ce.methods.len()
            && self.class_name == ce.name
            && self.arrays.iter().all(|(_, a)| a.strong_count() > 0)
    }
}

/// What the general NEW handler made of an object of an internal class (a
/// class only the stubs declare, or one an extension registered, all of
/// whose ancestors are internal too) at a NEW site naming it: its
/// properties as they are before the object exists, the same for every
/// object of the class but for a throwable's file, line and trace (see
/// Vm::new_internal_fast). Internal classes do not change once declared.
#[derive(Debug)]
pub(crate) struct InternalNewTemplate {
    /// The class's declared name, and its lowercased name.
    pub(crate) class_name: Vec<u8>,
    pub(crate) lower: Vec<u8>,
    /// The class table entry it came from (null: a stub-only class, the
    /// class table not having one), and the class table's generation it was
    /// last seen in there.
    pub(crate) entry: *const ClassEntry,
    pub(crate) stamp: std::cell::Cell<u64>,
    /// The properties (an empty array is a placeholder: each object gets
    /// its own, see `arrays`).
    pub(crate) slots: PropTable,
    pub(crate) arrays: Vec<usize>,
    /// A throwable's `file`, `line` and `trace` slots.
    pub(crate) throwable: Option<(usize, usize, usize)>,
    /// Whether its trace loses its arguments (filter_new_exception_trace()),
    /// with how many classes trace_without_args_classes had.
    pub(crate) filter_trace: (usize, bool),
    /// The class has no constructor (none in the stubs, no extension
    /// construct handler): `new` without arguments skips the constructor
    /// call (ZEND_NEW).
    pub(crate) no_ctor: bool,
    /// The class_tag() of the class's objects.
    pub(crate) tag: u32,
    /// The extension create_object hook its objects get, and how many hooks
    /// there were when it was looked up.
    pub(crate) create_hook:
        Option<fn(&mut crate::vm::Vm, &std::rc::Rc<std::cell::RefCell<PhpObject>>)>,
    pub(crate) hooks_len: usize,
}

#[derive(Debug, Clone)]
pub struct PropertyDef {
    pub name: Vec<u8>,
    pub default: Value,
    pub is_static: bool,
    pub is_readonly: bool,
    pub visibility: Visibility,
    /// Asymmetric set visibility (PHP 8.4): if Some, write access uses this instead of visibility
    pub set_visibility: Option<Visibility>,
    /// The class that originally declared this property (lowercase)
    pub declaring_class: Vec<u8>,
    /// Optional type constraint for the property
    pub property_type: Option<crate::opcode::ParamType>,
    /// Whether this property has a get hook (PHP 8.4)
    pub has_get_hook: bool,
    /// Whether this property has a set hook (PHP 8.4)
    pub has_set_hook: bool,
    /// Whether this property is virtual (hooks don't access the backing store)
    pub is_virtual: bool,
    /// Whether the property itself is marked abstract (requires subclass to
    /// implement hooks). Only valid on hooked properties.
    pub is_abstract: bool,
    /// Whether the property itself is marked final (subclasses cannot
    /// override its hooks).
    pub is_final: bool,
    pub attributes: Vec<RuntimeAttribute>,
    /// Doc comment for the property (if any).
    pub doc_comment: Option<std::rc::Rc<str>>,
}

#[derive(Debug, Clone)]
pub struct MethodDef {
    pub name: Vec<u8>,
    pub op_array: std::rc::Rc<OpArray>,
    pub param_count: usize,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub visibility: Visibility,
    /// The class that originally declared this method (lowercase)
    pub declaring_class: Vec<u8>,
    /// Doc comment for the method (if any)
    pub doc_comment: Option<std::rc::Rc<str>>,
    pub attributes: Vec<RuntimeAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

impl ClassEntry {
    pub fn new(name: Vec<u8>) -> Self {
        Self {
            name,
            parent: None,
            interfaces: Vec::new(),
            traits: Vec::new(),
            trait_adaptations: Vec::new(),
            properties: Vec::new(),
            methods: Default::default(),
            constants: Default::default(),
            constants_meta: Default::default(),
            static_properties: IndexMap::new(),
            is_abstract: false,
            is_final: false,
            is_readonly: false,
            is_interface: false,
            is_trait: false,
            is_enum: false,
            enum_backing_type: None,
            enum_cases: Vec::new(),
            start_line: 0,
            end_line: 0,
            filename: None,
            doc_comment: None,
            attributes: Vec::new(),
            const_closures: Vec::new(),
            allow_dynamic_properties: false,
            new_template: Default::default(),
            own_prop_order: Vec::new(),
        }
    }

    pub fn get_method(&self, name: &[u8]) -> Option<&MethodDef> {
        // Callers mostly pass lowercase names already: no copy for those.
        if !name.iter().any(u8::is_ascii_uppercase) {
            return self.methods.get(name);
        }
        self.get_method_folded(name)
    }

    /// get_method() of a name with uppercase letters (lowercased on the
    /// stack when short).
    #[inline(never)]
    fn get_method_folded(&self, name: &[u8]) -> Option<&MethodDef> {
        let lower = crate::class_table::LowerName::new(name);
        self.methods.get(lower.get())
    }
}

// A thread-local pool of freed object IDs. When a `PhpObject` is dropped
// (its Rc reference count hits zero), its ID is pushed here so subsequent
// `alloc_object_id` calls can reuse it. Matches PHP's handle recycling.
std::thread_local! {
    pub static FREED_OBJECT_IDS: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

std::thread_local! {
    /// Handles of freed objects that held an object awaiting its destructor
    /// (the `PhpObject` address they wait on, the handle): php-src runs that
    /// destructor and frees the child while freeing the parent, so the
    /// child's handle is recycled first. Released after the child's.
    static DEFERRED_OBJECT_IDS: std::cell::RefCell<Vec<(usize, u64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// (the key of the last deferral and a count of deferrals, so a parent
    /// whose descendants deferred during its own release follows them)
    static LAST_DEFER: std::cell::Cell<(usize, u64)> = const { std::cell::Cell::new((0, 0)) };
    /// Entries in DEFERRED_OBJECT_IDS (a drop skips the list when none).
    static DEFERRED_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A destructible child `v` of an object being freed that only the
/// destructor list will hold after it: its address.
#[inline]
fn pending_dtor_child(v: &Value) -> Option<usize> {
    match v {
        Value::Object(o)
            if std::rc::Rc::strong_count(o) == 2
                && o.try_borrow().is_ok_and(|b| b.dtor_listed.get()) =>
        {
            Some(o.as_ptr() as usize)
        }
        _ => None,
    }
}

std::thread_local! {
    /// Extension hook run when an object is freed, with the address of the
    /// `PhpObject` (`Rc::as_ptr`). Used by ext/xmlwriter to flush output when
    /// the writer object goes away, as PHP's object dtor does.
    pub static OBJECT_FREE_HOOK: std::cell::Cell<Option<fn(usize)>> =
        const { std::cell::Cell::new(None) };
}

/// An object's property slots, in order (declared ones first, in
/// declaration order, then dynamic ones), with what the fast property
/// paths need to know without scanning them: whether a key is an internal
/// state marker (`__`-prefixed: lazy, cloning, destructed, an internal
/// class's state...) or a NUL-mangled private one. The flags only ever go
/// up while slots are added (they are recomputed when the table is
/// replaced or cleared): a key changed in place through `iter_mut()` /
/// indexing must not become one of those.
#[derive(Debug, Clone, Default)]
pub struct PropTable {
    items: Vec<(Vec<u8>, Value)>,
    internal: bool,
    mangled: bool,
}

impl PropTable {
    #[inline]
    pub const fn new() -> Self {
        PropTable {
            items: Vec::new(),
            internal: false,
            mangled: false,
        }
    }

    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        PropTable {
            items: Vec::with_capacity(n),
            internal: false,
            mangled: false,
        }
    }

    #[inline]
    fn note(&mut self, k: &[u8]) {
        match k.first() {
            Some(0) => self.mangled = true,
            Some(b'_') if k.get(1) == Some(&b'_') => self.internal = true,
            _ => {}
        }
    }

    fn renote(&mut self) {
        self.internal = false;
        self.mangled = false;
        for i in 0..self.items.len() {
            let k = std::mem::take(&mut self.items[i].0);
            self.note(&k);
            self.items[i].0 = k;
        }
    }

    /// Some key may be an internal state marker (`__`-prefixed).
    #[inline]
    pub fn has_internal(&self) -> bool {
        self.internal
    }

    /// No key is an internal state marker but those of a Reflection
    /// object's own state (`__reflection_*`, a ReflectionType's), which say
    /// nothing of how its other properties are accessed.
    pub fn internal_markers_reflection_only(&self) -> bool {
        self.items.iter().all(|(k, _)| {
            !k.starts_with(b"__")
                || k.starts_with(b"__reflection_")
                || matches!(
                    k.as_slice(),
                    b"__allows_null" | b"__types" | b"__type_name" | b"__is_builtin"
                )
        })
    }

    /// Some key may be a NUL-mangled private property's.
    #[inline]
    pub fn has_mangled(&self) -> bool {
        self.mangled
    }

    #[inline]
    pub fn push(&mut self, slot: (Vec<u8>, Value)) {
        self.note(&slot.0);
        self.items.push(slot);
    }

    #[inline]
    pub fn insert(&mut self, at: usize, slot: (Vec<u8>, Value)) {
        self.note(&slot.0);
        self.items.insert(at, slot);
    }

    #[inline]
    pub fn remove(&mut self, at: usize) -> (Vec<u8>, Value) {
        self.items.remove(at)
    }

    #[inline]
    pub fn pop(&mut self) -> Option<(Vec<u8>, Value)> {
        self.items.pop()
    }

    #[inline]
    pub fn retain<F: FnMut(&(Vec<u8>, Value)) -> bool>(&mut self, f: F) {
        self.items.retain(f)
    }

    #[inline]
    pub fn retain_mut<F: FnMut(&mut (Vec<u8>, Value)) -> bool>(&mut self, f: F) {
        self.items.retain_mut(f)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
        self.internal = false;
        self.mangled = false;
    }

    #[inline]
    pub fn truncate(&mut self, n: usize) {
        self.items.truncate(n)
    }

    #[inline]
    pub fn reserve(&mut self, n: usize) {
        self.items.reserve(n)
    }

    #[inline]
    pub fn drain<R: std::ops::RangeBounds<usize>>(
        &mut self,
        r: R,
    ) -> std::vec::Drain<'_, (Vec<u8>, Value)> {
        self.items.drain(r)
    }

    #[inline]
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, (Vec<u8>, Value)> {
        self.items.iter_mut()
    }

    #[inline]
    pub fn get_mut(&mut self, i: usize) -> Option<&mut (Vec<u8>, Value)> {
        self.items.get_mut(i)
    }

    #[inline]
    pub fn last_mut(&mut self) -> Option<&mut (Vec<u8>, Value)> {
        self.items.last_mut()
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [(Vec<u8>, Value)] {
        &mut self.items
    }

    /// Reorder the slots (their keys stay the same).
    #[inline]
    pub fn sort_by<F: FnMut(&(Vec<u8>, Value), &(Vec<u8>, Value)) -> std::cmp::Ordering>(
        &mut self,
        f: F,
    ) {
        self.items.sort_by(f)
    }

    /// The slots, the table left empty.
    #[inline]
    pub fn take_vec(&mut self) -> Vec<(Vec<u8>, Value)> {
        self.internal = false;
        self.mangled = false;
        std::mem::take(&mut self.items)
    }

    #[inline]
    pub fn into_vec(self) -> Vec<(Vec<u8>, Value)> {
        self.items
    }

    /// Rename slot `i` (its value stays).
    #[inline]
    pub fn rename(&mut self, i: usize, k: Vec<u8>) {
        self.note(&k);
        self.items[i].0 = k;
    }
}

impl std::ops::Deref for PropTable {
    type Target = Vec<(Vec<u8>, Value)>;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl std::ops::Index<usize> for PropTable {
    type Output = (Vec<u8>, Value);
    #[inline]
    fn index(&self, i: usize) -> &Self::Output {
        &self.items[i]
    }
}

impl std::ops::IndexMut<usize> for PropTable {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        &mut self.items[i]
    }
}

impl From<Vec<(Vec<u8>, Value)>> for PropTable {
    fn from(items: Vec<(Vec<u8>, Value)>) -> Self {
        let mut t = PropTable {
            items,
            internal: false,
            mangled: false,
        };
        t.renote();
        t
    }
}

impl FromIterator<(Vec<u8>, Value)> for PropTable {
    fn from_iter<I: IntoIterator<Item = (Vec<u8>, Value)>>(it: I) -> Self {
        PropTable::from(it.into_iter().collect::<Vec<_>>())
    }
}

impl Extend<(Vec<u8>, Value)> for PropTable {
    fn extend<I: IntoIterator<Item = (Vec<u8>, Value)>>(&mut self, it: I) {
        for s in it {
            self.push(s);
        }
    }
}

impl<'a> IntoIterator for &'a PropTable {
    type Item = &'a (Vec<u8>, Value);
    type IntoIter = std::slice::Iter<'a, (Vec<u8>, Value)>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut PropTable {
    type Item = &'a mut (Vec<u8>, Value);
    type IntoIter = std::slice::IterMut<'a, (Vec<u8>, Value)>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}

impl IntoIterator for PropTable {
    type Item = (Vec<u8>, Value);
    type IntoIter = std::vec::IntoIter<(Vec<u8>, Value)>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

thread_local! {
    /// The class_tag() of each class name met (see class_tag_of).
    static CLASS_TAGS: std::cell::RefCell<crate::fxhash::FxHashMap<Box<[u8]>, u32>> =
        std::cell::RefCell::new(Default::default());
}

/// The number standing for class name `name` (exact bytes) in this
/// thread: given in order from 1, kept for the thread's life (a name
/// stands for one class in a request; the caches comparing them belong to
/// one request, see opcode::SITE_EPOCH). (u32::MAX as the thread ends:
/// no cache keeps that one.)
pub fn class_tag_of(name: &[u8]) -> u32 {
    CLASS_TAGS
        .try_with(|m| {
            let mut m = m.borrow_mut();
            if let Some(&t) = m.get(name) {
                return t;
            }
            let t = m.len() as u32 + 1;
            m.insert(name.into(), t);
            t
        })
        .unwrap_or(u32::MAX)
}

/// A PHP object instance
///
/// (`class_name` is changed with set_class_name(): see class_tag())
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: Vec<u8>,
    /// Properties stored as ordered Vec to preserve declaration order
    pub properties: PropTable,
    pub object_id: u64,
    /// Registered with the cycle collector (see gc.rs).
    pub gc_tracked: GcFlag,
    /// Listed in the VM's objects awaiting destruction (a hint for
    /// `gc::note_dtor_candidate`).
    pub dtor_listed: GcFlag,
    /// In the cycle collector's root buffer (Zend's GC_INFO).
    pub gc_buffered: GcSlot,
    /// Used as a WeakMap key (Zend's IS_OBJ_WEAKLY_REFERENCED): its entries
    /// are released when it dies.
    pub weak_key: GcFlag,
    /// For `Closure` instances: the underlying callable (the internal
    /// `__closure_N` name, a `[name, $this?, captures...]` array, a function
    /// name, or a bound-method `["Class::m", $obj]` array).
    pub closure: Option<Box<Value>>,
    /// Opaque extension-owned state (e.g. the XML node behind a DOM or
    /// SimpleXML object). Objects carrying it get their native object
    /// handlers consulted (see `Vm::native_object_handlers`).
    pub native: Option<NativeData>,
    /// memory_limit accounting (see `PhpObject::account`).
    pub mem: ObjectMem,
    /// The class_tag() of `class_name` (0: not looked up yet).
    tag: std::cell::Cell<u32>,
}

/// Bytes an object accounts for against memory_limit, and how many of its
/// properties are declared slots (the rest live in a properties
/// HashTable). A copy of an object is a new allocation: cloning charges
/// the same amount again, dropping releases it.
#[derive(Debug)]
pub struct ObjectMem {
    charged: std::cell::Cell<u32>,
    declared: std::cell::Cell<u32>,
}

impl ObjectMem {
    fn new() -> Self {
        ObjectMem {
            charged: std::cell::Cell::new(0),
            declared: std::cell::Cell::new(u32::MAX),
        }
    }
}

impl Clone for ObjectMem {
    fn clone(&self) -> Self {
        let c = self.charged.get();
        if c > 0 {
            crate::value::memory_charge(c as usize);
        }
        ObjectMem {
            charged: std::cell::Cell::new(c),
            declared: std::cell::Cell::new(self.declared.get()),
        }
    }
}

impl Drop for ObjectMem {
    fn drop(&mut self) {
        let c = self.charged.get();
        if c > 0 {
            crate::value::memory_free(c as usize);
        }
    }
}

/// zend_object_alloc(): the zend_object header (with its first property
/// slot) plus one zval per further declared property; dynamic properties
/// add a properties HashTable sized for all of them. A Closure is a
/// zend_closure (object + op_array copy).
fn object_zend_size(nprops: usize, declared: u32, closure: bool) -> usize {
    use crate::value::zend_mm_size;
    let d = nprops.min(declared as usize);
    let base = if closure { 320 } else { 40 };
    let mut size = zend_mm_size(base + 16 * d);
    if nprops > d {
        size += 56 + zend_mm_size(40 * nprops.next_power_of_two().max(8));
    }
    size
}

/// Extension payload attached to a `PhpObject`: opaque data plus the
/// object handlers that implement the object's behaviour.
#[derive(Clone)]
pub struct NativeData {
    pub data: std::rc::Rc<dyn std::any::Any>,
    pub handlers: &'static crate::vm::NativeObjectHandlers,
}

impl std::fmt::Debug for NativeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeData")
    }
}

/// A flag that is *not* copied by `Clone` (a cloned object is a new,
/// untracked allocation).
#[derive(Debug, Default)]
pub struct GcFlag(std::cell::Cell<bool>);
impl Clone for GcFlag {
    fn clone(&self) -> Self {
        GcFlag::default()
    }
}
impl GcFlag {
    #[inline]
    pub fn get(&self) -> bool {
        self.0.get()
    }
    #[inline]
    pub fn set(&self, v: bool) {
        self.0.set(v)
    }
}

/// The cycle collector's root buffer slot of a value (Zend's GC_INFO
/// address; 0 = not buffered). Not copied by `Clone`.
#[derive(Debug, Default)]
pub struct GcSlot(std::cell::Cell<u32>);
impl Clone for GcSlot {
    fn clone(&self) -> Self {
        GcSlot::default()
    }
}
impl GcSlot {
    /// Buffered as a possible root.
    #[inline]
    pub fn get(&self) -> bool {
        self.0.get() != 0
    }
    #[inline]
    pub fn slot(&self) -> u32 {
        self.0.get()
    }
    #[inline]
    pub fn set_slot(&self, v: u32) {
        self.0.set(v)
    }
}

/// spl_object_storage_dtor() releases each element's object, then its
/// data, entry by entry; the storage keeps them in two parallel arrays
/// (`__spl_objects` / `__spl_array`), which are released interleaved so
/// freed handles reach the free list in PHP's order.
#[inline(never)]
fn release_object_storage(props: &mut [(Vec<u8>, crate::value::Value)]) {
    use crate::value::Value;
    let find = |props: &[(Vec<u8>, Value)], n: &[u8]| props.iter().position(|(k, _)| k == n);
    // RecursiveIteratorIterator (spl_RecursiveIteratorIterator_free_iterators()):
    // the deepest level first, each one's iterator handle, then its object.
    if let (Some(hi), Some(ii)) = (find(props, b"__spl_rii_hs"), find(props, b"__spl_rii_its")) {
        if let (Value::Array(hs), Value::Array(its)) = (&props[hi].1, &props[ii].1) {
            if std::rc::Rc::strong_count(hs) == 1 && std::rc::Rc::strong_count(its) == 1 {
                let mut hs = std::mem::take(&mut *hs.borrow_mut());
                let mut its = std::mem::take(&mut *its.borrow_mut());
                let keys: Vec<_> = its.keys().cloned().collect();
                for k in keys.iter().rev() {
                    drop(hs.remove(k));
                    drop(its.remove(k));
                }
            }
        }
        return;
    }
    let (Some(oi), Some(ii)) = (find(props, b"__spl_objects"), find(props, b"__spl_array")) else {
        return;
    };
    let (Value::Array(objs), Value::Array(infs)) = (&props[oi].1, &props[ii].1) else {
        return;
    };
    if std::rc::Rc::strong_count(objs) != 1 || std::rc::Rc::strong_count(infs) != 1 {
        return;
    }
    let mut objs = std::mem::take(&mut *objs.borrow_mut());
    let mut infs = std::mem::take(&mut *infs.borrow_mut());
    let keys: Vec<_> = objs.keys().cloned().collect();
    for k in &keys {
        drop(objs.remove(k));
        drop(infs.remove(k));
    }
    drop(infs);
}

thread_local! {
    /// The property tables of objects released with the stack nearly used
    /// up (see release_props_deep()), and whether one of those releases is
    /// draining them.
    static DEEP_RELEASES: std::cell::RefCell<(Vec<Vec<(Vec<u8>, Value)>>, bool)> =
        const { std::cell::RefCell::new((Vec::new(), false)) };
}

/// Release the property table of an object freed with the stack nearly
/// used up (a long chain of objects let go at once recurses once per link):
/// the first such release frees it and what the ones below it hand over,
/// iteratively, instead of overflowing the native stack. Chains short
/// enough to fit are freed recursively (innermost handles first).
#[cold]
#[inline(never)]
fn release_props_deep(props: Vec<(Vec<u8>, Value)>) {
    let Ok(draining) = DEEP_RELEASES.try_with(|d| {
        let mut d = d.borrow_mut();
        if d.1 {
            d.0.push(props);
            None
        } else {
            d.1 = true;
            Some(props)
        }
    }) else {
        // (thread-local storage gone while the thread exits)
        return;
    };
    let Some(props) = draining else {
        return;
    };
    drop(props);
    while let Some(next) = DEEP_RELEASES
        .try_with(|d| d.borrow_mut().0.pop())
        .ok()
        .flatten()
    {
        drop(next);
    }
    let _ = DEEP_RELEASES.try_with(|d| d.borrow_mut().1 = false);
}

impl Drop for PhpObject {
    fn drop(&mut self) {
        if self.gc_buffered.get() {
            crate::gc::root_freed(self.gc_buffered.slot());
        }
        // A WeakMap key or map: its entries are released (by the VM, see
        // gc::purge_weakmaps) before its handle is recycled.
        let weak_target = self.weak_key.get() || self.class_name.eq_ignore_ascii_case(b"WeakMap");
        if weak_target {
            crate::gc::weak_target_freed(self.object_id, !self.weak_key.get());
        }
        if let Ok(Some(hook)) = OBJECT_FREE_HOOK.try_with(|h| h.get()) {
            hook(self as *const PhpObject as usize);
        }
        let mut defer_on: Option<usize> = None;
        // (only a held object, array or reference can release further
        // objects, recursively)
        let mut containers = false;
        for (_, v) in self.properties.iter() {
            crate::gc::on_child_released(v);
            containers |= matches!(v, Value::Object(_) | Value::Array(_) | Value::Reference(_));
            if let Some(k) = pending_dtor_child(v) {
                defer_on = Some(k);
            }
        }
        let defers_before = LAST_DEFER.try_with(|c| c.get().1).unwrap_or(0);
        if let Some(c) = &self.closure {
            crate::gc::on_child_released(c);
            crate::closure::note_dropped_closure(c);
        }
        // Release what the object holds before its handle goes back to the
        // free list (zend_objects_store_del: free_obj, then the handle).
        drop(self.native.take());
        // (SplObjectStorage's state is kept under `__`-prefixed keys)
        let internal = self.properties.has_internal();
        let mut props = self.properties.take_vec();
        if internal {
            release_object_storage(&mut props);
        }
        if containers && crate::stack::release_stack_low() {
            release_props_deep(props);
        } else {
            drop(props);
        }
        drop(self.closure.take());
        // Don't recycle the sentinel zero id.
        if self.object_id != 0 && !weak_target {
            if defer_on.is_none() {
                // (a descendant released with us deferred its handle)
                defer_on = LAST_DEFER
                    .try_with(|c| c.get())
                    .ok()
                    .filter(|&(_, n)| n != defers_before)
                    .map(|(k, _)| k);
            }
            if let Some(k) = defer_on {
                let _ = DEFERRED_OBJECT_IDS.try_with(|d| d.borrow_mut().push((k, self.object_id)));
                let _ = DEFERRED_COUNT.try_with(|c| c.set(c.get() + 1));
                let _ = LAST_DEFER.try_with(|c| c.set((k, c.get().1.wrapping_add(1))));
            } else {
                // Use try_with so we don't panic during thread-local
                // destruction (when other TLS holding PhpObject Rcs may be
                // torn down after FREED_OBJECT_IDS).
                let _ = FREED_OBJECT_IDS.try_with(|ids| ids.borrow_mut().push(self.object_id));
            }
        }
        // Handles deferred on this object follow its own.
        if DEFERRED_COUNT.try_with(|c| c.get()).unwrap_or(0) == 0 {
            return;
        }
        let me = self as *const PhpObject as usize;
        let _ = DEFERRED_OBJECT_IDS.try_with(|d| {
            let mut d = d.borrow_mut();
            if d.is_empty() || !d.iter().any(|(k, _)| *k == me) {
                return;
            }
            let mut i = 0;
            while i < d.len() {
                if d[i].0 == me {
                    let (_, id) = d.remove(i);
                    let _ = DEFERRED_COUNT.try_with(|c| c.set(c.get().saturating_sub(1)));
                    let _ = FREED_OBJECT_IDS.try_with(|ids| ids.borrow_mut().push(id));
                } else {
                    i += 1;
                }
            }
        });
    }
}

impl PhpObject {
    pub fn new(class_name: Vec<u8>, object_id: u64) -> Self {
        let o = Self {
            class_name,
            properties: PropTable::new(),
            object_id,
            gc_tracked: GcFlag::default(),
            dtor_listed: GcFlag::default(),
            gc_buffered: GcSlot::default(),
            weak_key: GcFlag::default(),
            closure: None,
            native: None,
            mem: ObjectMem::new(),
            tag: std::cell::Cell::new(0),
        };
        o.account();
        o
    }

    /// A number standing for the object's class name in this thread (the
    /// same for all objects of a class: a property access cache compares
    /// it instead of the names, see Vm::prop_ic_hit). Never 0.
    #[inline]
    pub fn class_tag(&self) -> u32 {
        match self.tag.get() {
            0 => self.lookup_class_tag(),
            t => t,
        }
    }

    #[cold]
    #[inline(never)]
    fn lookup_class_tag(&self) -> u32 {
        let t = class_tag_of(&self.class_name);
        self.tag.set(t);
        t
    }

    /// A new object of a class whose declared properties are `props`
    /// (init_declared_props() then mark_declared()), whose class_tag() is
    /// `tag`: memory_limit is charged once.
    #[inline]
    pub(crate) fn new_declared(
        class_name: Vec<u8>,
        object_id: u64,
        props: PropTable,
        tag: u32,
    ) -> Self {
        let o = Self {
            class_name,
            properties: props,
            object_id,
            gc_tracked: GcFlag::default(),
            dtor_listed: GcFlag::default(),
            gc_buffered: GcSlot::default(),
            weak_key: GcFlag::default(),
            closure: None,
            native: None,
            mem: ObjectMem::new(),
            tag: std::cell::Cell::new(tag),
        };
        o.mark_declared();
        o
    }

    /// Give the object another class (its class_tag() follows).
    pub fn set_class_name(&mut self, name: Vec<u8>) {
        self.class_name = name;
        self.tag.set(0);
    }

    /// Bring the memory_limit accounting in line with the object's current
    /// shape (going over the limit raises the pending fatal error).
    pub fn account(&self) {
        let want = object_zend_size(
            self.properties.len(),
            self.mem.declared.get(),
            self.closure.is_some(),
        ) as u32;
        let have = self.mem.charged.get();
        if want != have {
            if want > have {
                crate::value::memory_charge((want - have) as usize);
            } else {
                crate::value::memory_free((have - want) as usize);
            }
            self.mem.charged.set(want);
        }
    }

    /// The declared properties are initialized: further ones are dynamic.
    pub fn mark_declared(&self) {
        self.mem.declared.set(self.properties.len() as u32);
        self.account();
    }

    /// Whether this object is a Closure instance (wrapping a callable).
    #[inline]
    pub fn is_closure(&self) -> bool {
        self.closure.is_some()
    }

    pub fn get_property(&self, name: &[u8]) -> Value {
        self.properties
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.deref())
            .unwrap_or(Value::Null)
    }

    /// The storage key of Exception's / Error's private property `name`
    /// (`string`, `trace`, `previous`): its bare name, unless a dynamic
    /// property of that name made it move to its mangled key
    /// (`Vm::prop_access_key`).
    pub fn throwable_private_key(&self, name: &[u8]) -> Option<usize> {
        self.properties.iter().position(|(k, _)| {
            k.len() > name.len() + 2
                && k[0] == 0
                && k.ends_with(name)
                && k[k.len() - name.len() - 1] == 0
                && matches!(&k[1..k.len() - name.len() - 1], b"Exception" | b"Error")
        })
    }

    /// get_property() of Exception's / Error's private property `name`.
    pub fn throwable_get(&self, name: &[u8]) -> Value {
        match self.throwable_private_key(name) {
            Some(i) => match self.properties[i].1.deref() {
                Value::Undef => Value::Null,
                v => v,
            },
            None => self.get_property(name),
        }
    }

    /// set_property() of Exception's / Error's private property `name`.
    pub fn throwable_set(&mut self, name: &[u8], value: Value) {
        match self.throwable_private_key(name) {
            Some(i) => self.properties[i].1 = value,
            None => self.set_property_slice(name, value),
        }
    }

    /// has_property() of Exception's / Error's private property `name`.
    pub fn throwable_has(&self, name: &[u8]) -> bool {
        match self.throwable_private_key(name) {
            Some(i) => !matches!(self.properties[i].1, Value::Undef),
            None => self.has_property(name),
        }
    }

    pub fn set_property(&mut self, name: Vec<u8>, value: Value) {
        if let Some(value) = self.set_existing_property(&name, value) {
            self.lazy_ht_check(&name);
            self.properties.push((name, value));
            self.account();
        }
    }

    /// set_property() by a borrowed name (copied only for a new property).
    pub fn set_property_slice(&mut self, name: &[u8], value: Value) {
        if let Some(value) = self.set_existing_property(name, value) {
            self.lazy_ht_check(name);
            self.properties.push((name.to_vec(), value));
            self.account();
        }
    }

    /// Update property `name` if it exists; else hand the value back.
    fn set_existing_property(&mut self, name: &[u8], value: Value) -> Option<Value> {
        // A slot holding a PHP reference is written through (the reference
        // set stays intact).
        for (k, v) in &mut self.properties {
            if *k == name {
                // The overwritten value loses a holder (possible GC root).
                match v {
                    Value::Reference(r) if !matches!(value, Value::Reference(_)) => {
                        let old = std::mem::replace(&mut *r.borrow_mut(), value);
                        crate::gc::on_child_released(&old);
                    }
                    _ => {
                        let old = std::mem::replace(v, value);
                        crate::gc::on_value_replaced(&old, v);
                    }
                }
                return None;
            }
        }
        Some(value)
    }

    /// Bind a property slot to an existing reference (`$obj->p = &$x`).
    pub fn set_property_ref(&mut self, name: Vec<u8>, r: std::rc::Rc<std::cell::RefCell<Value>>) {
        for (k, v) in &mut self.properties {
            if *k == name {
                let old = std::mem::replace(v, Value::Reference(r));
                crate::gc::on_value_replaced(&old, v);
                return;
            }
        }
        self.lazy_ht_check(&name);
        self.properties.push((name, Value::Reference(r)));
        self.account();
    }

    /// A property is being added to this object: if it is the lazy ghost
    /// whose initializer runs with its properties table built (see
    /// LAZY_HT_WATCH), the table is separated now.
    #[inline]
    fn lazy_ht_check(&mut self, name: &[u8]) {
        let w = LAZY_HT_WATCH.load(std::sync::atomic::Ordering::Relaxed);
        if w != 0
            && w == self as *const PhpObject as usize
            && !LAZY_HT_DECLARED.with(|d| d.borrow().iter().any(|k| k == name))
        {
            self.lazy_ht_separate();
        }
    }

    /// zend_array_dup() of a properties table holding INDIRECT slots (a
    /// lazy ghost's, shared with the initialization snapshot when its
    /// initializer adds a dynamic property): the copy holds the declared
    /// properties' values as they are now, and the views built from the
    /// table (var_dump(), (array), get_object_vars(), ...) keep showing
    /// them. Stored as `__lazy_frozen` (storage key => value).
    #[cold]
    #[inline(never)]
    fn lazy_ht_separate(&mut self) {
        LAZY_HT_WATCH.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut arr = crate::array::PhpArray::new();
        for (k, v) in &self.properties {
            if crate::proptable::is_internal_prop_name(k) || matches!(v, Value::Undef) {
                continue;
            }
            arr.set(
                crate::array::ArrayKey::String(crate::string::PhpString::from_vec(k.clone())),
                v.clone(),
            );
        }
        self.properties.push((
            b"__lazy_frozen".to_vec(),
            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr))),
        ));
    }

    /// Get (creating if needed) a reference to a property slot (`$x = &$obj->p`).
    pub fn get_property_ref(&mut self, name: &[u8]) -> std::rc::Rc<std::cell::RefCell<Value>> {
        for (k, v) in &mut self.properties {
            if k.as_slice() == name {
                if let Value::Reference(r) = v {
                    if let Ok(mut inner) = r.try_borrow_mut() {
                        crate::array::separate_shared(&mut inner);
                    }
                    return r.clone();
                }
                let mut cur = std::mem::replace(v, Value::Null);
                if matches!(cur, Value::Undef) {
                    cur = Value::Null;
                }
                crate::array::separate_shared(&mut cur);
                let r = std::rc::Rc::new(std::cell::RefCell::new(cur));
                *v = Value::Reference(r.clone());
                return r;
            }
        }
        let r = std::rc::Rc::new(std::cell::RefCell::new(Value::Null));
        self.lazy_ht_check(name);
        self.properties
            .push((name.to_vec(), Value::Reference(r.clone())));
        self.account();
        r
    }

    pub fn has_property(&self, name: &[u8]) -> bool {
        // (an internal state marker: only in a table that has one)
        if name.starts_with(b"__") && !self.properties.has_internal() {
            return false;
        }
        self.properties
            .iter()
            .any(|(k, _)| crate::string::bytes_eq(k, name))
    }

    pub fn remove_property(&mut self, name: &[u8]) {
        self.properties.retain(|(k, v)| {
            if k == name {
                crate::gc::on_child_released(v);
                false
            } else {
                true
            }
        });
    }
}

/// The lazy ghost (its PhpObject address) whose initializer runs while its
/// properties table exists (built by a view while it was lazy): the first
/// property added to it separates the table (PhpObject::lazy_ht_separate).
pub static LAZY_HT_WATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

std::thread_local! {
    /// The storage keys of the watched object's declared properties (adding
    /// one back after unset() is not a dynamic property).
    pub static LAZY_HT_DECLARED: std::cell::RefCell<Vec<Vec<u8>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// A copy of a property slot's value (the slot still holds it) about to be
/// bound to a reference in its place: an array shared copy-on-write with
/// something else (the class's default, a copy) is separated first, as
/// writes through the reference change it in place.
#[inline]
pub fn separated(mut v: Value) -> Value {
    if matches!(&v, Value::Array(a) if std::rc::Rc::strong_count(a) > 2) {
        crate::array::separate_shared(&mut v);
    }
    v
}
