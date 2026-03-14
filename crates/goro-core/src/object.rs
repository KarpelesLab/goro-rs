use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::opcode::OpArray;
use crate::string::PhpString;
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
    pub properties: HashMap<Vec<u8>, Value>,
    pub object_id: u64,
}

impl PhpObject {
    pub fn new(class_name: Vec<u8>, object_id: u64) -> Self {
        Self {
            class_name,
            properties: HashMap::new(),
            object_id,
        }
    }

    pub fn get_property(&self, name: &[u8]) -> Value {
        self.properties.get(name).cloned().unwrap_or(Value::Null)
    }

    pub fn set_property(&mut self, name: Vec<u8>, value: Value) {
        self.properties.insert(name, value);
    }
}
