//! Classes: compile-time declarations ([`ClassDecl`]) and linked run-time
//! classes ([`Class`]).
//!
//! A class is linked when its declaration runs (or at load time when its
//! parent, interfaces and traits are already known, PHP's early binding).
//! Linking resolves the parent and interfaces, copies inherited members,
//! imports traits, lays out property slots and checks the inheritance rules
//! (PHP manual: Objects, Inheritance, Interfaces, Traits, Abstract classes).
//!
//! Classes are never freed while the engine runs, so instructions, methods
//! and frames refer to them with plain pointers.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::object::ClassInfo;
use goro_runtime::{PhpRef, PhpStr, Value};

use crate::bytecode::{AttrInfo, ConstExpr, FuncCode, TypeHint};
use crate::func::Func;

pub mod cflags {
    pub const ABSTRACT: u32 = 1;
    pub const FINAL: u32 = 2;
    pub const INTERFACE: u32 = 4;
    pub const TRAIT: u32 = 8;
    pub const ENUM: u32 = 16;
    pub const READONLY: u32 = 32;
    pub const ANON: u32 = 64;
    pub const INTERNAL: u32 = 128;
    /// Instances may get dynamic properties without deprecation
    /// (`#[AllowDynamicProperties]`, stdClass).
    pub const ALLOW_DYNAMIC: u32 = 256;
    /// Instances can't have dynamic properties at all (enums, closures...).
    pub const NO_DYNAMIC: u32 = 512;
    pub const NOT_SERIALIZABLE: u32 = 1024;
    pub const NOT_CLONEABLE: u32 = 2048;
}

/// Member flags (methods, properties, constants).
pub mod mflags {
    pub const PUBLIC: u32 = 1;
    pub const PROTECTED: u32 = 2;
    pub const PRIVATE: u32 = 4;
    pub const STATIC: u32 = 8;
    pub const ABSTRACT: u32 = 16;
    pub const FINAL: u32 = 32;
    pub const READONLY: u32 = 64;
    pub const PUBLIC_SET: u32 = 128;
    pub const PROTECTED_SET: u32 = 256;
    pub const PRIVATE_SET: u32 = 512;
    pub const VISIBILITY: u32 = PUBLIC | PROTECTED | PRIVATE;
    pub const SET_VISIBILITY: u32 = PUBLIC_SET | PROTECTED_SET | PRIVATE_SET;
    /// Property declared with hooks only (no backing value).
    pub const VIRTUAL: u32 = 1024;
    /// A promoted constructor parameter.
    pub const PROMOTED: u32 = 2048;
    /// Enum case constant.
    pub const CASE: u32 = 4096;
    pub const DEPRECATED: u32 = 8192;
    /// Method implicitly declared by the engine (enum cases()...).
    pub const IMPLICIT: u32 = 16384;
    /// Has a type (typed property: starts uninitialized).
    pub const TYPED: u32 = 32768;
    pub const HAS_DEFAULT: u32 = 65536;
    /// Method from a trait.
    pub const TRAIT_METHOD: u32 = 131072;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

#[derive(Debug)]
pub struct ConstDecl {
    pub name: PhpStr,
    pub value: ConstExpr,
    pub flags: u32,
    pub ty: Option<TypeHint>,
    pub doc: Option<PhpStr>,
    pub attrs: Rc<[AttrInfo]>,
    pub line: u32,
}

#[derive(Debug)]
pub struct PropDecl {
    pub name: PhpStr,
    pub default: Option<ConstExpr>,
    pub flags: u32,
    pub ty: Option<TypeHint>,
    pub get: Option<Rc<FuncCode>>,
    pub set: Option<Rc<FuncCode>>,
    /// Hook declared abstract (`get;` in an interface or abstract class).
    pub hook_flags: u32,
    pub doc: Option<PhpStr>,
    pub attrs: Rc<[AttrInfo]>,
    pub line: u32,
}

#[derive(Debug)]
pub struct MethodDecl {
    pub code: Rc<FuncCode>,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub enum TraitRule {
    /// `T::m insteadof U, V`
    Insteadof {
        trait_: PhpStr,
        method: PhpStr,
        instead: Vec<PhpStr>,
    },
    /// `[T::]m as [vis] [alias]`
    Alias {
        trait_: Option<PhpStr>,
        method: PhpStr,
        flags: u32,
        alias: Option<PhpStr>,
    },
}

#[derive(Debug)]
pub struct TraitUseDecl {
    pub traits: Vec<PhpStr>,
    pub rules: Vec<TraitRule>,
    pub line: u32,
}

/// A class declaration as compiled.
#[derive(Debug)]
pub struct ClassDecl {
    pub name: PhpStr,
    pub kind: ClassKind,
    pub flags: u32,
    pub parent: Option<PhpStr>,
    pub interfaces: Vec<PhpStr>,
    /// Backed enum type: `int` or `string`.
    pub enum_backing: Option<u32>,
    pub consts: Vec<ConstDecl>,
    pub props: Vec<PropDecl>,
    pub methods: Vec<MethodDecl>,
    pub traits: Vec<TraitUseDecl>,
    pub attrs: Rc<[AttrInfo]>,
    pub doc: Option<PhpStr>,
    pub file: PhpStr,
    pub line_start: u32,
    pub line_end: u32,
    /// Set once linked (a declaration runs at most once).
    pub declared: Cell<bool>,
}

/// A class constant of a linked class.
#[derive(Debug)]
pub struct ClassConst {
    pub name: PhpStr,
    pub value: RefCell<ConstState>,
    pub flags: u32,
    pub ty: Option<TypeHint>,
    /// Declaring class.
    pub class: *const Class,
    pub doc: Option<PhpStr>,
    pub attrs: Rc<[AttrInfo]>,
}

#[derive(Debug, Clone)]
pub enum ConstState {
    Value(Value),
    Pending(ConstExpr),
    /// Being evaluated (cycle detection).
    Evaluating,
}

/// A declared instance property of a linked class.
#[derive(Debug)]
pub struct PropInfo {
    pub name: PhpStr,
    /// Slot in objects.
    pub slot: u32,
    pub flags: u32,
    pub ty: Option<TypeHint>,
    /// Declaring class (for private visibility and messages).
    pub class: *const Class,
    pub get: Option<Rc<Func>>,
    pub set: Option<Rc<Func>>,
    pub doc: Option<PhpStr>,
    pub attrs: Rc<[AttrInfo]>,
    pub default: Option<ConstExpr>,
}

/// A static property of a linked class: the value cell is shared with the
/// parent when the child does not redeclare it.
#[derive(Debug)]
pub struct StaticProp {
    pub name: PhpStr,
    pub flags: u32,
    pub ty: Option<TypeHint>,
    pub class: *const Class,
    pub cell: PhpRef,
    pub init: RefCell<Option<ConstExpr>>,
    pub doc: Option<PhpStr>,
    pub attrs: Rc<[AttrInfo]>,
}

/// Magic methods found on a class (looked up once at link time).
#[derive(Default, Debug)]
pub struct Magic {
    pub construct: Option<Rc<Func>>,
    pub destruct: Option<Rc<Func>>,
    pub get: Option<Rc<Func>>,
    pub set: Option<Rc<Func>>,
    pub isset: Option<Rc<Func>>,
    pub unset: Option<Rc<Func>>,
    pub call: Option<Rc<Func>>,
    pub call_static: Option<Rc<Func>>,
    pub to_string: Option<Rc<Func>>,
    pub invoke: Option<Rc<Func>>,
    pub clone: Option<Rc<Func>>,
    pub serialize: Option<Rc<Func>>,
    pub unserialize: Option<Rc<Func>>,
    pub sleep: Option<Rc<Func>>,
    pub wakeup: Option<Rc<Func>>,
    pub debug_info: Option<Rc<Func>>,
}

/// Hooks internal classes provide (native behaviour).
#[derive(Default)]
pub struct NativeHooks {
    /// Called after a new object of the class (or a subclass) is created.
    pub create: Option<fn(&mut crate::vm::Vm, &goro_runtime::ObjRef)>,
    /// `clone`: copy native data.
    pub clone: Option<fn(&mut crate::vm::Vm, &goro_runtime::ObjRef, &goro_runtime::ObjRef)>,
    /// Object compare (`==`, `<=>`).
    pub compare: Option<fn(&mut crate::vm::Vm, &Value, &Value) -> Option<crate::vm::R<i32>>>,
    /// Cast to int/float/bool (GMP and friends).
    pub cast: Option<
        fn(
            &mut crate::vm::Vm,
            &goro_runtime::ObjRef,
            crate::bytecode::CastTo,
        ) -> Option<crate::vm::R<Value>>,
    >,
    /// `count()` on the object (Countable without user code).
    pub count: Option<fn(&mut crate::vm::Vm, &goro_runtime::ObjRef) -> crate::vm::R<i64>>,
    /// Properties shown by var_dump / (array) / get_object_vars.
    pub debug_props:
        Option<fn(&mut crate::vm::Vm, &goro_runtime::ObjRef) -> Option<goro_runtime::PhpArray>>,
    /// Native iteration for foreach (internal iterators).
    pub get_iterator: Option<
        fn(&mut crate::vm::Vm, &goro_runtime::ObjRef, bool) -> crate::vm::R<goro_runtime::ObjRef>,
    >,
}

impl std::fmt::Debug for NativeHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeHooks")
    }
}

/// A linked class.
pub struct Class {
    pub name: PhpStr,
    pub lname: PhpStr,
    pub kind: ClassKind,
    pub flags: Cell<u32>,
    pub parent: Option<Rc<Class>>,
    /// Every interface implemented (directly or inherited), each once.
    pub interfaces: RefCell<Vec<Rc<Class>>>,
    /// Interfaces in declaration order as written (for Reflection).
    pub direct_interfaces: RefCell<Vec<PhpStr>>,
    pub traits: RefCell<Vec<Rc<Class>>>,
    pub consts: RefCell<Vec<Rc<ClassConst>>>,
    pub const_map: RefCell<HashMap<PhpStr, usize>>,
    pub props: RefCell<Vec<Rc<PropInfo>>>,
    /// Property name → indices into `props` (several for shadowed
    /// privates: the class's own first).
    pub prop_map: RefCell<HashMap<PhpStr, Vec<u32>>>,
    pub static_props: RefCell<Vec<Rc<StaticProp>>>,
    pub methods: RefCell<HashMap<PhpStr, Rc<Func>>>,
    /// Method names in declaration order (lower case), own then inherited.
    pub method_order: RefCell<Vec<PhpStr>>,
    pub magic: RefCell<Magic>,
    /// Default slot values (typed properties without default are
    /// uninitialized), computed on first instantiation.
    pub defaults: OnceCell<Vec<Value>>,
    pub num_slots: Cell<u32>,
    pub decl: Option<Rc<ClassDecl>>,
    pub enum_backing: Option<u32>,
    pub native: NativeHooks,
    pub attrs: Rc<[AttrInfo]>,
    pub doc: Option<PhpStr>,
    pub id: u32,
    pub has_dtor: Cell<bool>,
    /// Enum case objects, by name.
    pub enum_cases: RefCell<Vec<(PhpStr, goro_runtime::ObjRef)>>,
    /// Statics initialized (lazily on first access).
    pub statics_ready: Cell<bool>,
    /// Constants evaluated and checked.
    pub file: Option<PhpStr>,
    pub line_start: u32,
    pub line_end: u32,
}

impl std::fmt::Debug for Class {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "class {}", self.name.to_string_lossy())
    }
}

impl ClassInfo for Class {
    fn name(&self) -> &PhpStr {
        &self.name
    }
    fn has_destructor(&self) -> bool {
        self.has_dtor.get()
    }
}

impl Class {
    pub fn is_interface(&self) -> bool {
        self.kind == ClassKind::Interface
    }
    pub fn is_trait(&self) -> bool {
        self.kind == ClassKind::Trait
    }
    pub fn is_enum(&self) -> bool {
        self.kind == ClassKind::Enum
    }
    pub fn has_flag(&self, f: u32) -> bool {
        self.flags.get() & f != 0
    }

    /// Whether `self` is `other` or a subclass / implementor of it.
    pub fn is_subclass_of(&self, other: &Class) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        if other.is_interface() {
            return self
                .interfaces
                .borrow()
                .iter()
                .any(|i| std::ptr::eq(&**i, other));
        }
        let mut c = self.parent.as_deref();
        while let Some(p) = c {
            if std::ptr::eq(p, other) {
                return true;
            }
            c = p.parent.as_deref();
        }
        false
    }

    pub fn find_method(&self, lname: &[u8]) -> Option<Rc<Func>> {
        self.methods.borrow().get(&PhpStr::new(lname)).cloned()
    }

    pub fn find_method_str(&self, lname: &PhpStr) -> Option<Rc<Func>> {
        self.methods.borrow().get(lname).cloned()
    }

    pub fn find_const(&self, name: &PhpStr) -> Option<Rc<ClassConst>> {
        let m = self.const_map.borrow();
        let i = *m.get(name)?;
        Some(self.consts.borrow()[i].clone())
    }

    pub fn find_static(&self, name: &[u8]) -> Option<Rc<StaticProp>> {
        self.static_props
            .borrow()
            .iter()
            .find(|p| p.name.as_bytes() == name)
            .cloned()
    }

    /// Ancestor chain from this class up.
    pub fn ancestors(&self) -> Vec<*const Class> {
        let mut v = vec![self as *const Class];
        let mut c = self.parent.as_deref();
        while let Some(p) = c {
            v.push(p as *const Class);
            c = p.parent.as_deref();
        }
        v
    }
}

/// Lower-case ASCII copy (PHP class, function and method names are
/// case-insensitive for ASCII letters).
pub fn lower(b: &[u8]) -> PhpStr {
    if b.iter().any(|c| c.is_ascii_uppercase()) {
        PhpStr::from(b.to_ascii_lowercase())
    } else {
        PhpStr::new(b)
    }
}

pub fn lower_intern(b: &[u8]) -> PhpStr {
    if b.iter().any(|c| c.is_ascii_uppercase()) {
        PhpStr::intern(&b.to_ascii_lowercase())
    } else {
        PhpStr::intern(b)
    }
}

/// Names as PHP prints them in messages: up to a NUL byte (anonymous class
/// names continue with the file and line after a NUL).
pub trait DisplayName {
    fn dname(&self) -> String;
}

impl DisplayName for PhpStr {
    fn dname(&self) -> String {
        let b = self.as_bytes();
        let cut = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..cut]).into_owned()
    }
}

/// The type of a value for error messages ("int", "Foo", "class@anonymous").
pub trait TypeName {
    fn tname(&self) -> String;
}

impl TypeName for Value {
    fn tname(&self) -> String {
        match self {
            Value::Object(o) => o.class_name().dname(),
            Value::Ref(r) => r.get().tname(),
            v => v.type_name_for_error(),
        }
    }
}
