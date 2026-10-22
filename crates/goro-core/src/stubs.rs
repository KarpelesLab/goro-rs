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
    pub fn is_static(&self) -> bool {
        self.flags & F_STATIC != 0
    }
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

fn class_index() -> &'static crate::fxhash::FxHashMap<Vec<u8>, usize> {
    static IDX: OnceLock<crate::fxhash::FxHashMap<Vec<u8>, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        crate::stub_sigs::CLASSES
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.to_ascii_lowercase().into_bytes(), i))
            .collect()
    })
}

/// Index in CLASSES of an internal class (any case, optional leading
/// backslash).
fn class_index_of(name: &[u8]) -> Option<usize> {
    let n = name.strip_prefix(b"\\").unwrap_or(name);
    if n.iter().any(|b| b.is_ascii_uppercase()) {
        class_index().get(&n.to_ascii_lowercase()).copied()
    } else {
        class_index().get(n).copied()
    }
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
    let n = if name.first() == Some(&b'\\') {
        &name[1..]
    } else {
        name
    };
    String::from_utf8_lossy(n).to_ascii_lowercase()
}

/// Look up the stub of an internal class/interface/trait/enum.
pub fn find_class(name: &[u8]) -> Option<&'static StubClass> {
    class_index_of(name).map(|i| &crate::stub_sigs::CLASSES[i])
}

/// The stub of a class an extension declares itself
/// (`stub_sigs::SELF_DECLARED_CLASSES`).
pub fn find_self_declared_class(name: &[u8]) -> Option<&'static StubClass> {
    let n = name.strip_prefix(b"\\").unwrap_or(name);
    crate::stub_sigs::SELF_DECLARED_CLASSES
        .iter()
        .find(|c| c.name.as_bytes().eq_ignore_ascii_case(n))
}

/// Hierarchy of an internal class, precomputed from the stubs.
pub struct StubHierarchy {
    /// Declared name of the parent class.
    pub parent: Option<&'static str>,
    /// `ce->interfaces` (declared names) in php-src's order.
    pub interfaces: Vec<&'static str>,
    /// Lowercased ancestor classes, nearest first.
    pub parents_lower: Vec<Vec<u8>>,
    /// Lowercased `interfaces`.
    pub interfaces_lower: Vec<Vec<u8>>,
}

struct StubTable {
    by_lower: crate::fxhash::FxHashMap<Vec<u8>, usize>,
    hierarchy: Vec<StubHierarchy>,
}

/// `ce->interfaces` of an internal class as php-src builds it at
/// registration: the implicit Stringable of a class declaring `__toString()`
/// (zend_register_functions), then the parent's interfaces in reverse
/// (zend_do_inherit_interfaces), then each implemented interface followed by
/// its own interfaces in reverse (zend_do_implement_interface).
fn ce_interfaces(c: &'static StubClass, depth: usize) -> Vec<&'static str> {
    fn push(out: &mut Vec<&'static str>, n: &'static str) {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(n)) {
            out.push(n);
        }
    }
    let mut out = Vec::new();
    if depth > 32 {
        return out;
    }
    if c.kind != K_TRAIT
        && !c.name.eq_ignore_ascii_case("Stringable")
        && c.methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("__toString"))
    {
        push(&mut out, "Stringable");
    }
    if let Some(pc) = c.parent.and_then(|p| find_class(p.as_bytes())) {
        for i in ce_interfaces(pc, depth + 1).into_iter().rev() {
            push(&mut out, i);
        }
    }
    for i in c.interfaces {
        let ic = find_class(i.as_bytes());
        push(&mut out, ic.map(|ic| ic.name).unwrap_or(i));
        if let Some(ic) = ic {
            for sub in ce_interfaces(ic, depth + 1).into_iter().rev() {
                push(&mut out, sub);
            }
        }
    }
    out
}

fn stub_table() -> &'static StubTable {
    static T: OnceLock<StubTable> = OnceLock::new();
    T.get_or_init(|| {
        let classes = crate::stub_sigs::CLASSES;
        let by_lower: crate::fxhash::FxHashMap<Vec<u8>, usize> = classes
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.to_ascii_lowercase().into_bytes(), i))
            .collect();
        let hierarchy = classes
            .iter()
            .map(|c| {
                let mut parents_lower = Vec::new();
                let mut cur = c;
                while let Some(p) = cur.parent.and_then(|p| find_class(p.as_bytes())) {
                    if parents_lower.len() > 32 {
                        break;
                    }
                    parents_lower.push(p.name.to_ascii_lowercase().into_bytes());
                    cur = p;
                }
                let interfaces = ce_interfaces(c, 0);
                let interfaces_lower = interfaces
                    .iter()
                    .map(|i| i.to_ascii_lowercase().into_bytes())
                    .collect();
                StubHierarchy {
                    parent: c
                        .parent
                        .map(|p| find_class(p.as_bytes()).map(|pc| pc.name).unwrap_or(p)),
                    interfaces,
                    parents_lower,
                    interfaces_lower,
                }
            })
            .collect();
        StubTable {
            by_lower,
            hierarchy,
        }
    })
}

/// Allocation-free stub lookup by an already lowercased name (no leading
/// backslash).
pub fn find_class_lower(lower: &[u8]) -> Option<&'static StubClass> {
    stub_table()
        .by_lower
        .get(lower)
        .map(|&i| &crate::stub_sigs::CLASSES[i])
}

/// The stub and precomputed hierarchy of an internal class (lowercased name).
pub fn class_hierarchy(lower: &[u8]) -> Option<(&'static StubClass, &'static StubHierarchy)> {
    let t = stub_table();
    t.by_lower
        .get(lower)
        .map(|&i| (&crate::stub_sigs::CLASSES[i], &t.hierarchy[i]))
}

/// instanceof between internal classes (both lowercased): Some(result) when
/// `child` is a stub class, None otherwise.
pub fn stub_instanceof(child: &[u8], ancestor: &[u8]) -> Option<bool> {
    let (_, h) = class_hierarchy(child)?;
    Some(
        child == ancestor
            || h.parents_lower.iter().any(|p| p == ancestor)
            || h.interfaces_lower.iter().any(|i| i == ancestor),
    )
}

/// Look up the stub of an internal function.
pub fn find_function(name: &[u8]) -> Option<&'static StubFunc> {
    function_index()
        .get(&lower_str(name))
        .map(|&i| &crate::stub_sigs::FUNCTIONS[i])
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
    // The first match in class_methods()' order (the class, then its
    // parent's chain and interfaces, depth first), without building the
    // whole list.
    fn search(
        c: &'static StubClass,
        method: &[u8],
        depth: usize,
    ) -> Option<(&'static StubClass, &'static StubFunc)> {
        if depth > 32 {
            return None;
        }
        if let Some(m) = c
            .methods
            .iter()
            .find(|m| m.name.as_bytes().eq_ignore_ascii_case(method))
        {
            return Some((c, m));
        }
        if let Some(p) = c.parent.and_then(|p| find_class(p.as_bytes())) {
            if let Some(r) = search(p, method, depth + 1) {
                return Some(r);
            }
        }
        c.interfaces
            .iter()
            .filter_map(|i| find_class(i.as_bytes()))
            .find_map(|ic| search(ic, method, depth + 1))
    }
    search(find_class(class)?, method, 0)
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
        return Some(ParamType::Union(
            parts.iter().filter_map(|p| parse_type(p)).collect(),
        ));
    }
    let s = s.trim();
    let s = if s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    };
    if s.contains('&') {
        return Some(ParamType::Intersection(
            s.split('&').filter_map(parse_type).collect(),
        ));
    }
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "int" | "float" | "string" | "bool" | "array" | "object" | "callable" | "iterable"
        | "mixed" | "null" | "void" | "self" | "parent" | "static" | "false" | "true" | "never" => {
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
        oa.param_types
            .push(parse_type(a.ty).map(|pt| ParamTypeInfo {
                param_type: pt,
                param_name: a.name.as_bytes().to_vec(),
                implicitly_nullable: false,
            }));
        oa.by_ref_params
            .push(a.flags & (A_BY_REF | A_PREFER_REF) != 0);
        oa.param_has_default
            .push(a.default.is_some() || a.flags & A_UNKNOWN_DEFAULT != 0);
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
        "[]" => {
            return Some(Value::Array(std::rc::Rc::new(RefCell::new(
                crate::array::PhpArray::new(),
            ))));
        }
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
    if e.len() >= 2
        && ((e.starts_with('"') && e.ends_with('"')) || (e.starts_with('\'') && e.ends_with('\'')))
    {
        let inner = &e[1..e.len() - 1];
        if !inner.contains('\\') && !inner.contains('$') {
            return Some(Value::String(crate::string::PhpString::from_string(
                inner.to_string(),
            )));
        }
    }
    None
}

thread_local! {
    /// class_entry() results, by index in CLASSES.
    static ENTRY_CACHE: RefCell<Vec<Option<&'static ClassEntry>>> = const { RefCell::new(Vec::new()) };
}

/// A synthesized `ClassEntry` for an internal class, built from its stub
/// (methods flattened over the parent chain and interfaces). Cached per
/// thread for the life of the process. Constant values come from the VM.
pub fn class_entry(vm: &crate::vm::Vm, name: &[u8]) -> Option<&'static ClassEntry> {
    let idx = class_index_of(name)?;
    if let Some(e) = ENTRY_CACHE.with(|c| c.borrow().get(idx).copied().flatten()) {
        return Some(e);
    }
    let built: &'static ClassEntry =
        Box::leak(Box::new(build_entry(vm, &crate::stub_sigs::CLASSES[idx])));
    ENTRY_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() <= idx {
            c.resize(crate::stub_sigs::CLASSES.len().max(idx + 1), None);
        }
        c[idx] = Some(built);
    });
    Some(built)
}

fn build_entry(vm: &crate::vm::Vm, c: &'static StubClass) -> ClassEntry {
    let mut ce = ClassEntry::new(c.name.as_bytes().to_vec());
    ce.parent = c.parent.map(|p| {
        find_class(p.as_bytes())
            .map(|pc| pc.name)
            .unwrap_or(p)
            .as_bytes()
            .to_vec()
    });
    ce.interfaces = c
        .interfaces
        .iter()
        .map(|i| {
            find_class(i.as_bytes())
                .map(|ic| ic.name)
                .unwrap_or(i)
                .as_bytes()
                .to_vec()
        })
        .collect();
    // Internal classes declaring __toString() implicitly implement Stringable
    // (registered before the explicitly implemented interfaces).
    if c.kind != K_TRAIT
        && c.methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("__toString"))
        && !c
            .interfaces
            .iter()
            .any(|i| i.eq_ignore_ascii_case("Stringable"))
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
                op_array: std::rc::Rc::new(oa),
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
    // Properties in properties_info order: an internal class inherits its
    // parent's (non-private) properties when it is registered, before
    // declaring its own; a redeclared one keeps the inherited position.
    let mut chain: Vec<&'static StubClass> = vec![c];
    let mut cur = c;
    while let Some(p) = cur.parent.and_then(|p| find_class(p.as_bytes())) {
        if chain.len() > 32 {
            break;
        }
        chain.push(p);
        cur = p;
    }
    for (depth, cls) in chain.iter().enumerate().rev() {
        for p in cls.props {
            if depth > 0 && p.flags & F_PRIVATE != 0 {
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
            match ce.properties.iter_mut().find(|x| x.name == def.name) {
                Some(slot) => *slot = def,
                None => ce.properties.push(def),
            }
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
        // Constants an extension registers on the class at startup.
        let cls_lower = cls.name.to_ascii_lowercase().into_bytes();
        if let Some(extra) = vm.ext_class_constants.get(&cls_lower) {
            for (kn, val, _) in extra {
                if ce.constants.contains_key(kn) {
                    continue;
                }
                ce.constants.insert(kn.clone(), val.clone());
                ce.constants_meta.insert(
                    kn.clone(),
                    ConstantMeta {
                        visibility: Visibility::Public,
                        is_final: false,
                        declaring_class: cls_lower.clone(),
                        attributes: Vec::new(),
                        declared_type: None,
                        doc_comment: None,
                        referenced_constants: Vec::new(),
                    },
                );
            }
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
            // Internal classes registered by extensions carry the flag in
            // their stub.
            if ce.filename.is_none()
                && find_class(&cur).is_some_and(|c| c.flags & F_NOT_SERIALIZABLE != 0)
            {
                return true;
            }
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
