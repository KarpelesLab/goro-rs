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
    pub args_op_array: OpArray,
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
    pub doc_comment: Option<String>,
    /// Class constants referenced in this constant's value expression.
    /// Each pair is (class_name_lower, constant_name). Used to re-emit
    /// #[\Deprecated] banners for those referenced constants when this
    /// constant is accessed (e.g. CONST = self::PREFIX . self::SUFFIX).
    pub referenced_constants: Vec<(Vec<u8>, Vec<u8>)>,
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
    pub methods: IndexMap<Vec<u8>, MethodDef>,
    pub constants: IndexMap<Vec<u8>, Value>,
    /// Metadata for constants (visibility, final flag, declaring class)
    pub constants_meta: IndexMap<Vec<u8>, ConstantMeta>,
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
    pub doc_comment: Option<String>,
    pub attributes: Vec<RuntimeAttribute>,
    /// Closures compiled from const expressions (for deferred resolution)
    pub const_closures: Vec<crate::opcode::OpArray>,
    /// Whether the class has #[AllowDynamicProperties]
    pub allow_dynamic_properties: bool,
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
    pub doc_comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MethodDef {
    pub name: Vec<u8>,
    pub op_array: OpArray,
    pub param_count: usize,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub visibility: Visibility,
    /// The class that originally declared this method (lowercase)
    pub declaring_class: Vec<u8>,
    /// Doc comment for the method (if any)
    pub doc_comment: Option<String>,
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
            methods: IndexMap::new(),
            constants: IndexMap::new(),
            constants_meta: IndexMap::new(),
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
        }
    }

    pub fn get_method(&self, name: &[u8]) -> Option<&MethodDef> {
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        self.methods.get(&lower)
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
    /// Extension hook run when an object is freed, with the address of the
    /// `PhpObject` (`Rc::as_ptr`). Used by ext/xmlwriter to flush output when
    /// the writer object goes away, as PHP's object dtor does.
    pub static OBJECT_FREE_HOOK: std::cell::Cell<Option<fn(usize)>> =
        const { std::cell::Cell::new(None) };
}

/// A PHP object instance
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: Vec<u8>,
    /// Properties stored as ordered Vec to preserve declaration order
    pub properties: Vec<(Vec<u8>, Value)>,
    pub object_id: u64,
    /// Registered with the cycle collector (see gc.rs).
    pub gc_tracked: GcFlag,
    /// Listed in the VM's objects awaiting destruction (a hint for
    /// `gc::note_dtor_candidate`).
    pub dtor_listed: GcFlag,
    /// For `Closure` instances: the underlying callable (the internal
    /// `__closure_N` name, a `[name, $this?, captures...]` array, a function
    /// name, or a bound-method `["Class::m", $obj]` array).
    pub closure: Option<Box<Value>>,
    /// Opaque extension-owned state (e.g. the XML node behind a DOM or
    /// SimpleXML object). Objects carrying it get their native object
    /// handlers consulted (see `Vm::native_object_handlers`).
    pub native: Option<NativeData>,
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

impl Drop for PhpObject {
    fn drop(&mut self) {
        if let Ok(Some(hook)) = OBJECT_FREE_HOOK.try_with(|h| h.get()) {
            hook(self as *const PhpObject as usize);
        }
        for (_, v) in self.properties.iter() {
            crate::gc::on_child_released(v);
        }
        if let Some(c) = &self.closure {
            crate::gc::on_child_released(c);
        }
        // Release what the object holds before its handle goes back to the
        // free list (zend_objects_store_del: free_obj, then the handle).
        drop(self.native.take());
        drop(std::mem::take(&mut self.properties));
        drop(self.closure.take());
        crate::value::memory_free(OBJECT_SIZE);
        // Don't recycle the sentinel zero id.
        if self.object_id != 0 {
            // Use try_with so we don't panic during thread-local
            // destruction (when other TLS holding PhpObject Rcs may be
            // torn down after FREED_OBJECT_IDS).
            let _ = FREED_OBJECT_IDS.try_with(|ids| ids.borrow_mut().push(self.object_id));
        }
    }
}

/// Accounted size of an object (zend_object header).
const OBJECT_SIZE: usize = 56;

impl PhpObject {
    pub fn new(class_name: Vec<u8>, object_id: u64) -> Self {
        crate::value::memory_charge(OBJECT_SIZE);
        Self {
            class_name,
            properties: Vec::new(),
            object_id,
            gc_tracked: GcFlag::default(),
            dtor_listed: GcFlag::default(),
            closure: None,
            native: None,
        }
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

    pub fn set_property(&mut self, name: Vec<u8>, value: Value) {
        // Update existing or append new. A slot holding a PHP reference is
        // written through (the reference set stays intact).
        for (k, v) in &mut self.properties {
            if *k == name {
                match v {
                    Value::Reference(r) if !matches!(value, Value::Reference(_)) => {
                        *r.borrow_mut() = value;
                    }
                    _ => *v = value,
                }
                return;
            }
        }
        self.properties.push((name, value));
    }

    /// Bind a property slot to an existing reference (`$obj->p = &$x`).
    pub fn set_property_ref(&mut self, name: Vec<u8>, r: std::rc::Rc<std::cell::RefCell<Value>>) {
        for (k, v) in &mut self.properties {
            if *k == name {
                *v = Value::Reference(r);
                return;
            }
        }
        self.properties.push((name, Value::Reference(r)));
    }

    /// Get (creating if needed) a reference to a property slot (`$x = &$obj->p`).
    pub fn get_property_ref(&mut self, name: &[u8]) -> std::rc::Rc<std::cell::RefCell<Value>> {
        for (k, v) in &mut self.properties {
            if k.as_slice() == name {
                if let Value::Reference(r) = v {
                    return r.clone();
                }
                let cur = std::mem::replace(v, Value::Null);
                let cur = if matches!(cur, Value::Undef) {
                    Value::Null
                } else {
                    cur
                };
                let r = std::rc::Rc::new(std::cell::RefCell::new(cur));
                *v = Value::Reference(r.clone());
                return r;
            }
        }
        let r = std::rc::Rc::new(std::cell::RefCell::new(Value::Null));
        self.properties
            .push((name.to_vec(), Value::Reference(r.clone())));
        r
    }

    pub fn has_property(&self, name: &[u8]) -> bool {
        self.properties.iter().any(|(k, _)| k == name)
    }

    pub fn remove_property(&mut self, name: &[u8]) {
        self.properties.retain(|(k, _)| k != name);
    }
}
