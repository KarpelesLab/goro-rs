//! Signatures of PHP's internal functions and classes, generated from
//! php-src's `*.stub.php` files (see `tools/gen_stub_sigs.php`).
//!
//! The VM implements internal classes natively (by name), so they have no
//! `ClassEntry` in `vm.classes`. Reflection, `get_class_methods()` and similar
//! introspection need their declared shape anyway: this module exposes the
//! raw stub data plus synthesized `ClassEntry`/`OpArray` views of it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::object::{ClassEntry, ConstantMeta, MethodDef, PropertyDef, Visibility};
use crate::opcode::{OpArray, ParamType, ParamTypeInfo};
use crate::value::Value;

pub const F_PUBLIC: u32 = 1;
pub const F_PROTECTED: u32 = 2;
pub const F_PRIVATE: u32 = 4;
pub const F_STATIC: u32 = 0x10;
pub const F_FINAL: u32 = 0x20;
pub const F_ABSTRACT: u32 = 0x40;
pub const F_READONLY: u32 = 0x80;
pub const F_TENTATIVE: u32 = 0x10000;
pub const F_DEPRECATED: u32 = 0x20000;
pub const F_RETURNS_REF: u32 = 0x40000;
pub const F_PROTECTED_SET: u32 = 0x100000;
pub const F_PRIVATE_SET: u32 = 0x200000;
pub const F_VIRTUAL: u32 = 0x400000;
pub const F_NOT_SERIALIZABLE: u32 = 0x800000;
/// Internal class declared with #[Attribute].
pub const F_ATTRIBUTE: u32 = 0x1000000;

pub const A_BY_REF: u8 = 1;
pub const A_PREFER_REF: u8 = 2;
pub const A_VARIADIC: u8 = 4;
pub const A_SENSITIVE: u8 = 8;
/// Optional parameter whose default can't be expressed (`= UNKNOWN`).
pub const A_UNKNOWN_DEFAULT: u8 = 16;

pub const K_CLASS: u8 = 0;
pub const K_INTERFACE: u8 = 1;
pub const K_TRAIT: u8 = 2;
pub const K_ENUM: u8 = 3;

#[derive(Debug)]
pub struct StubArg {
    pub name: &'static str,
    /// Declared type ("" when untyped).
    pub ty: &'static str,
    /// Default value expression as written in the stub.
    pub default: Option<&'static str>,
    pub flags: u8,
}

#[derive(Debug)]
pub struct StubFunc {
    pub name: &'static str,
    pub flags: u32,
    pub args: &'static [StubArg],
    pub ret: &'static str,
    /// Extension name (functions only; "" for methods).
    pub ext: &'static str,
}

#[derive(Debug)]
pub struct StubConst {
    pub name: &'static str,
    pub ty: &'static str,
    pub value: Option<&'static str>,
    pub flags: u32,
    pub ext: &'static str,
}

#[derive(Debug)]
pub struct StubProp {
    pub name: &'static str,
    pub ty: &'static str,
    pub default: Option<&'static str>,
    pub flags: u32,
}

#[derive(Debug)]
pub struct StubCase {
    pub name: &'static str,
    pub value: Option<&'static str>,
}

#[derive(Debug)]
pub struct StubClass {
    pub name: &'static str,
    pub kind: u8,
    pub flags: u32,
    pub ext: &'static str,
    pub parent: Option<&'static str>,
    pub interfaces: &'static [&'static str],
    pub backing: &'static str,
    pub consts: &'static [StubConst],
    pub props: &'static [StubProp],
    pub cases: &'static [StubCase],
    pub methods: &'static [StubFunc],
}

impl StubFunc {
    pub fn is_static(&self) -> bool { self.flags & F_STATIC != 0 }
    pub fn visibility(&self) -> Visibility {
        if self.flags & F_PRIVATE != 0 {
            Visibility::Private
        } else if self.flags & F_PROTECTED != 0 {
            Visibility::Protected
        } else {
            Visibility::Public
        }
    }
    /// Number of required arguments (PHP's `required_num_args`).
    pub fn required_args(&self) -> usize {
        let mut req = 0;
        for (i, a) in self.args.iter().enumerate() {
            if a.default.is_none() && a.flags & (A_VARIADIC | A_UNKNOWN_DEFAULT) == 0 {
                req = i + 1;
            }
        }
        req
    }
}

fn class_index() -> &'static HashMap<String, usize> {
    static IDX: OnceLock<HashMap<String, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        crate::stub_sigs::CLASSES
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.to_ascii_lowercase(), i))
            .collect()
    })
}

fn function_index() -> &'static HashMap<String, usize> {
    static IDX: OnceLock<HashMap<String, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        crate::stub_sigs::FUNCTIONS
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.to_ascii_lowercase(), i))
            .collect()
    })
}

fn lower_str(name: &[u8]) -> String {
    let n = if name.first() == Some(&b'\\') { &name[1..] } else { name };
    String::from_utf8_lossy(n).to_ascii_lowercase()
}

/// Look up the stub of an internal class/interface/trait/enum.
pub fn find_class(name: &[u8]) -> Option<&'static StubClass> {
    class_index().get(&lower_str(name)).map(|&i| &crate::stub_sigs::CLASSES[i])
}

/// Look up the stub of an internal function.
pub fn find_function(name: &[u8]) -> Option<&'static StubFunc> {
    function_index().get(&lower_str(name)).map(|&i| &crate::stub_sigs::FUNCTIONS[i])
}

/// Look up a global constant declared in a stub.
pub fn find_constant(name: &[u8]) -> Option<&'static StubConst> {
    let n = String::from_utf8_lossy(name);
    crate::stub_sigs::CONSTANTS.iter().find(|c| c.name == n)
}

/// Methods visible on an internal class, in PHP's function-table order:
/// own methods, then inherited parent methods, then interface methods.
/// Each entry is (declaring class stub, method stub).
pub fn class_methods(name: &[u8]) -> Vec<(&'static StubClass, &'static StubFunc)> {
    let mut out: Vec<(&'static StubClass, &'static StubFunc)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    fn add(
        c: &'static StubClass,
        out: &mut Vec<(&'static StubClass, &'static StubFunc)>,
        seen: &mut Vec<String>,
        depth: usize,
    ) {
        if depth > 32 {
            return;
        }
        for m in c.methods {
            let l = m.name.to_ascii_lowercase();
            if !seen.contains(&l) {
                seen.push(l);
                out.push((c, m));
            }
        }
        if let Some(p) = c.parent.and_then(|p| find_class(p.as_bytes())) {
            add(p, out, seen, depth + 1);
        }
        for i in c.interfaces {
            if let Some(ic) = find_class(i.as_bytes()) {
                add(ic, out, seen, depth + 1);
            }
        }
    }
    if let Some(c) = find_class(name) {
        add(c, &mut out, &mut seen, 0);
    }
    out
}

/// Find a method of an internal class (including inherited ones).
pub fn find_method(class: &[u8], method: &[u8]) -> Option<(&'static StubClass, &'static StubFunc)> {
    let ml = lower_str(method);
    class_methods(class).into_iter().find(|(_, m)| m.name.eq_ignore_ascii_case(&ml))
}

/// All interfaces implemented by an internal class (transitively), in
/// PHP's order (parent's interfaces first, then own ones with their parents).
pub fn class_interfaces(name: &[u8]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    fn add_iface(n: &'static str, out: &mut Vec<&'static str>, depth: usize) {
        if depth > 32 {
            return;
        }
        let c = find_class(n.as_bytes());
        let canon = c.map(|c| c.name).unwrap_or(n);
        if let Some(c) = c {
            for i in c.interfaces {
                add_iface(i, out, depth + 1);
            }
        }
        if !out.iter().any(|x| x.eq_ignore_ascii_case(canon)) {
            out.push(canon);
        }
    }
    fn walk(c: &'static StubClass, out: &mut Vec<&'static str>, depth: usize) {
        if depth > 32 {
            return;
        }
        if let Some(p) = c.parent.and_then(|p| find_class(p.as_bytes())) {
            walk(p, out, depth + 1);
        }
        for i in c.interfaces {
            add_iface(i, out, depth + 1);
        }
    }
    if let Some(c) = find_class(name) {
        walk(c, &mut out, 0);
    }
    out
}

/// Parse a stub type declaration into a `ParamType`.
pub fn parse_type(s: &str) -> Option<ParamType> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(inner) = s.strip_prefix('?') {
        return parse_type(inner).map(|t| ParamType::Nullable(Box::new(t)));
    }
    // Split on top-level '|'.
    let mut parts: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '|' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    if parts.len() > 1 {
        return Some(ParamType::Union(parts.iter().filter_map(|p| parse_type(p)).collect()));
    }
    let s = s.trim();
    let s = if s.starts_with('(') && s.ends_with(')') { &s[1..s.len() - 1] } else { s };
    if s.contains('&') {
        return Some(ParamType::Intersection(s.split('&').filter_map(parse_type).collect()));
    }
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "int" | "float" | "string" | "bool" | "array" | "object" | "callable" | "iterable" | "mixed"
        | "null" | "void" | "self" | "parent" | "static" | "false" | "true" | "never" => {
            Some(ParamType::Simple(lower.into_bytes()))
        }
        _ => Some(ParamType::Simple(s.as_bytes().to_vec())),
    }
}

/// Synthesize an `OpArray` describing an internal function/method signature
/// (no body). Non-static methods get a leading `$this` CV, matching the
/// layout the compiler uses for user methods.
pub fn op_array_for(f: &'static StubFunc, scope: Option<&[u8]>, is_method: bool) -> OpArray {
    let mut oa = OpArray::new();
    oa.name = f.name.as_bytes().to_vec();
    let has_this = is_method && !f.is_static();
    if has_this {
        oa.cv_names.push(b"this".to_vec());
        oa.param_types.push(None);
        oa.by_ref_params.push(false);
        oa.param_has_default.push(false);
    }
    for a in f.args {
        let cv = oa.cv_names.len() as u32;
        oa.cv_names.push(a.name.as_bytes().to_vec());
        oa.param_types.push(parse_type(a.ty).map(|pt| ParamTypeInfo {
            param_type: pt,
            param_name: a.name.as_bytes().to_vec(),
            implicitly_nullable: false,
        }));
        oa.by_ref_params.push(a.flags & (A_BY_REF | A_PREFER_REF) != 0);
        oa.param_has_default.push(a.default.is_some() || a.flags & A_UNKNOWN_DEFAULT != 0);
        if a.flags & A_VARIADIC != 0 {
            oa.variadic_param = Some(cv);
        }
    }
    oa.param_count = f.args.len() as u32 + if has_this { 1 } else { 0 };
    oa.required_param_count = f.required_args() as u32;
    oa.return_type = parse_type(f.ret);
    oa.returns_by_ref = f.flags & F_RETURNS_REF != 0;
    oa.scope_class = scope.map(|s| s.to_ascii_lowercase());
    oa.internal_sig = Some(f);
    oa
}

/// Evaluate a simple literal stub expression (used for property defaults).
pub fn literal_value(expr: &str) -> Option<Value> {
    let e = expr.trim();
    match e.to_ascii_lowercase().as_str() {
        "null" => return Some(Value::Null),
        "true" => return Some(Value::True),
        "false" => return Some(Value::False),
        "[]" => return Some(Value::Array(std::rc::Rc::new(RefCell::new(crate::array::PhpArray::new())))),
        _ => {}
    }
    if let Ok(n) = e.parse::<i64>() {
        return Some(Value::Long(n));
    }
    if let Ok(f) = e.parse::<f64>() {
        if e.contains('.') || e.contains('e') || e.contains('E') {
            return Some(Value::Double(f));
        }
    }
    if e.len() >= 2 && ((e.starts_with('"') && e.ends_with('"')) || (e.starts_with('\'') && e.ends_with('\''))) {
        let inner = &e[1..e.len() - 1];
        if !inner.contains('\\') && !inner.contains('$') {
            return Some(Value::String(crate::string::PhpString::from_string(inner.to_string())));
        }
    }
    None
}

thread_local! {
    static ENTRY_CACHE: RefCell<HashMap<String, Option<&'static ClassEntry>>> = RefCell::new(HashMap::new());
}

/// A synthesized `ClassEntry` for an internal class, built from its stub
/// (methods flattened over the parent chain and interfaces). Cached per
/// thread for the life of the process. Constant values come from the VM.
pub fn class_entry(vm: &crate::vm::Vm, name: &[u8]) -> Option<&'static ClassEntry> {
    let key = lower_str(name);
    if let Some(e) = ENTRY_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return e;
    }
    let built = find_class(name).map(|c| &*Box::leak(Box::new(build_entry(vm, c))));
    ENTRY_CACHE.with(|c| c.borrow_mut().insert(key, built));
    built
}

fn build_entry(vm: &crate::vm::Vm, c: &'static StubClass) -> ClassEntry {
    let mut ce = ClassEntry::new(c.name.as_bytes().to_vec());
    ce.parent = c.parent.map(|p| find_class(p.as_bytes()).map(|pc| pc.name).unwrap_or(p).as_bytes().to_vec());
    ce.interfaces = c.interfaces.iter().map(|i| find_class(i.as_bytes()).map(|ic| ic.name).unwrap_or(i).as_bytes().to_vec()).collect();
    // Internal classes declaring __toString() implicitly implement Stringable
    // (registered before the explicitly implemented interfaces).
    if c.kind != K_TRAIT
        && c.methods.iter().any(|m| m.name.eq_ignore_ascii_case("__toString"))
        && !c.interfaces.iter().any(|i| i.eq_ignore_ascii_case("Stringable"))
        && !c.name.eq_ignore_ascii_case("Stringable")
    {
        ce.interfaces.insert(0, b"Stringable".to_vec());
    }
    ce.is_abstract = c.flags & F_ABSTRACT != 0;
    ce.is_final = c.flags & F_FINAL != 0;
    ce.is_readonly = c.flags & F_READONLY != 0;
    ce.is_interface = c.kind == K_INTERFACE;
    ce.is_trait = c.kind == K_TRAIT;
    ce.is_enum = c.kind == K_ENUM;
    if !c.backing.is_empty() {
        ce.enum_backing_type = Some(c.backing.as_bytes().to_vec());
    }
    for case in c.cases {
        let v = case.value.and_then(literal_value).unwrap_or(Value::Null);
        ce.enum_cases.push((case.name.as_bytes().to_vec(), v));
    }
    for (decl, m) in class_methods(c.name.as_bytes()) {
        let decl_lower = decl.name.to_ascii_lowercase().into_bytes();
        let mut is_abstract = m.flags & F_ABSTRACT != 0;
        if decl.kind == K_INTERFACE {
            is_abstract = true;
        }
        let oa = op_array_for(m, Some(&decl_lower), true);
        ce.methods.insert(
            m.name.to_ascii_lowercase().into_bytes(),
            MethodDef {
                name: m.name.as_bytes().to_vec(),
                param_count: m.args.len(),
                op_array: oa,
                is_static: m.is_static(),
                is_abstract,
                is_final: m.flags & F_FINAL != 0,
                visibility: m.visibility(),
                declaring_class: decl_lower,
                doc_comment: None,
                attributes: Vec::new(),
            },
        );
    }
    // Properties: own first, then inherited non-private ones from parents.
    let mut chain: Vec<&'static StubClass> = vec![c];
    let mut cur = c;
    while let Some(p) = cur.parent.and_then(|p| find_class(p.as_bytes())) {
        if chain.len() > 32 {
            break;
        }
        chain.push(p);
        cur = p;
    }
    for (depth, cls) in chain.iter().enumerate() {
        for p in cls.props {
            if depth > 0 && p.flags & F_PRIVATE != 0 {
                continue;
            }
            if ce.properties.iter().any(|x| x.name == p.name.as_bytes()) {
                continue;
            }
            let vis = if p.flags & F_PRIVATE != 0 {
                Visibility::Private
            } else if p.flags & F_PROTECTED != 0 {
                Visibility::Protected
            } else {
                Visibility::Public
            };
            let set_vis = if p.flags & F_PRIVATE_SET != 0 {
                Some(Visibility::Private)
            } else if p.flags & F_PROTECTED_SET != 0 {
                Some(Visibility::Protected)
            } else {
                None
            };
            let ty = parse_type(p.ty);
            let default = match p.default {
                Some(d) => literal_value(d).unwrap_or(Value::Null),
                None if ty.is_some() => Value::Undef,
                None => Value::Null,
            };
            let def = PropertyDef {
                name: p.name.as_bytes().to_vec(),
                default: default.clone(),
                is_static: p.flags & F_STATIC != 0,
                is_readonly: p.flags & F_READONLY != 0 || cls.flags & F_READONLY != 0,
                visibility: vis,
                set_visibility: set_vis,
                declaring_class: cls.name.to_ascii_lowercase().into_bytes(),
                property_type: ty,
                has_get_hook: p.flags & F_VIRTUAL != 0,
                has_set_hook: false,
                is_virtual: p.flags & F_VIRTUAL != 0,
                is_abstract: false,
                is_final: false,
                attributes: Vec::new(),
                doc_comment: None,
            };
            if def.is_static {
                ce.static_properties.insert(def.name.clone(), default);
            }
            ce.properties.push(def);
        }
    }
    // Constants: own, then parents', then interfaces'.
    let mut const_sources: Vec<&'static StubClass> = chain.clone();
    for i in class_interfaces(c.name.as_bytes()) {
        if let Some(ic) = find_class(i.as_bytes()) {
            const_sources.push(ic);
        }
    }
    let lower = c.name.to_ascii_lowercase().into_bytes();
    for cls in const_sources {
        for k in cls.consts {
            let kn = k.name.as_bytes().to_vec();
            if ce.constants.contains_key(&kn) {
                continue;
            }
            if cls.name != c.name && k.flags & F_PRIVATE != 0 {
                continue;
            }
            let val = vm
                .get_builtin_class_constant(&lower, &kn)
                .or_else(|| k.value.and_then(literal_value))
                .unwrap_or(Value::Null);
            ce.constants.insert(kn.clone(), val);
            let vis = if k.flags & F_PRIVATE != 0 {
                Visibility::Private
            } else if k.flags & F_PROTECTED != 0 {
                Visibility::Protected
            } else {
                Visibility::Public
            };
            ce.constants_meta.insert(
                kn,
                ConstantMeta {
                    visibility: vis,
                    is_final: k.flags & F_FINAL != 0,
                    declaring_class: cls.name.to_ascii_lowercase().into_bytes(),
                    attributes: Vec::new(),
                    declared_type: parse_type(k.ty),
                    doc_comment: None,
                    referenced_constants: Vec::new(),
                },
            );
        }
    }
    ce.filename = None;
    ce
}

/// Whether objects of the class can't be (un)serialized
/// (ZEND_ACC_NOT_SERIALIZABLE, inherited from internal ancestors).
pub fn class_is_not_serializable(vm: &crate::vm::Vm, class_lower: &[u8]) -> bool {
    let mut cur = class_lower.to_ascii_lowercase();
    for _ in 0..64 {
        if let Some(ce) = vm.classes.get(&cur) {
            match &ce.parent {
                Some(p) => {
                    cur = p.to_ascii_lowercase();
                    continue;
                }
                None => return false,
            }
        }
        let mut c = match find_class(&cur) {
            Some(c) => c,
            None => return false,
        };
        for _ in 0..64 {
            if c.flags & F_NOT_SERIALIZABLE != 0 {
                return true;
            }
            match c.parent.and_then(|p| find_class(p.as_bytes())) {
                Some(pc) => c = pc,
                None => return false,
            }
        }
        return false;
    }
    false
}
