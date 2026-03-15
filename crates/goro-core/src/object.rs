use std::collections::HashMap;

use crate::opcode::OpArray;
use crate::value::Value;

/// A PHP class entry (class definition)
#[derive(Debug, Clone)]
pub struct ClassEntry {
    pub name: Vec<u8>,
    pub parent: Option<Vec<u8>>,
    pub interfaces: Vec<Vec<u8>>,
    pub properties: Vec<PropertyDef>,
    pub methods: HashMap<Vec<u8>, MethodDef>,
    pub constants: HashMap<Vec<u8>, Value>,
    pub static_properties: HashMap<Vec<u8>, Value>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_interface: bool,
    pub is_trait: bool,
}

#[derive(Debug, Clone)]
pub struct PropertyDef {
    pub name: Vec<u8>,
    pub default: Value,
    pub is_static: bool,
    pub visibility: Visibility,
}

#[derive(Debug, Clone)]
pub struct MethodDef {
    pub name: Vec<u8>,
    pub op_array: OpArray,
    pub param_count: usize,
    pub is_static: bool,
    pub is_abstract: bool,
    pub visibility: Visibility,
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
            properties: Vec::new(),
            methods: HashMap::new(),
            constants: HashMap::new(),
            static_properties: HashMap::new(),
            is_abstract: false,
            is_final: false,
            is_interface: false,
            is_trait: false,
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
}
