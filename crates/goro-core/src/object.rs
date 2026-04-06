use std::collections::HashMap;
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
    pub attributes: Vec<RuntimeAttribute>,
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
        }
    }

    pub fn get_method(&self, name: &[u8]) -> Option<&MethodDef> {
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        self.methods.get(&lower)
    }
}

/// A PHP object instance
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: Vec<u8>,
    /// Properties stored as ordered Vec to preserve declaration order
    pub properties: Vec<(Vec<u8>, Value)>,
    pub object_id: u64,
}

impl PhpObject {
    pub fn new(class_name: Vec<u8>, object_id: u64) -> Self {
        Self {
            class_name,
            properties: Vec::new(),
            object_id,
        }
    }

    pub fn get_property(&self, name: &[u8]) -> Value {
        self.properties
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null)
    }

    pub fn set_property(&mut self, name: Vec<u8>, value: Value) {
        // Update existing or append new
        for (k, v) in &mut self.properties {
            if *k == name {
                *v = value;
                return;
            }
        }
        self.properties.push((name, value));
    }

    pub fn has_property(&self, name: &[u8]) -> bool {
        self.properties.iter().any(|(k, _)| k == name)
    }

    pub fn remove_property(&mut self, name: &[u8]) {
        self.properties.retain(|(k, _)| k != name);
    }
}
