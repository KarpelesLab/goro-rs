//! Method/type variance checks used during class linking.
//!
//! This is a port of the relevant parts of Zend/zend_inheritance.c
//! (`zend_perform_covariant_type_check`, `zend_do_perform_implementation_check`,
//! `zend_get_function_declaration`, ...). It is a child module of `vm` so it can
//! use the VM's private class-hierarchy helpers.

use std::cell::RefCell;

use super::Vm;
use crate::object::{ClassEntry, MethodDef};
use crate::opcode::{OpArray, OpCode, OperandType, ParamType};
use crate::value::Value;

pub(crate) const T_NULL: u32 = 1 << 0;
pub(crate) const T_FALSE: u32 = 1 << 1;
pub(crate) const T_TRUE: u32 = 1 << 2;
pub(crate) const T_LONG: u32 = 1 << 3;
pub(crate) const T_DOUBLE: u32 = 1 << 4;
pub(crate) const T_STRING: u32 = 1 << 5;
pub(crate) const T_ARRAY: u32 = 1 << 6;
pub(crate) const T_OBJECT: u32 = 1 << 7;
pub(crate) const T_RESOURCE: u32 = 1 << 8;
pub(crate) const T_CALLABLE: u32 = 1 << 9;
pub(crate) const T_VOID: u32 = 1 << 10;
pub(crate) const T_STATIC: u32 = 1 << 11;
pub(crate) const T_NEVER: u32 = 1 << 12;
pub(crate) const T_BOOL: u32 = T_FALSE | T_TRUE;
pub(crate) const T_ANY: u32 =
    T_NULL | T_FALSE | T_TRUE | T_LONG | T_DOUBLE | T_STRING | T_ARRAY | T_OBJECT | T_RESOURCE;

/// One class-ish member of a type: either a single class name or an
/// intersection group (inside a DNF union).
#[derive(Debug, Clone)]
pub(crate) enum TElem {
    Name(Vec<u8>),
    Inter(Vec<Vec<u8>>),
}

/// A flattened view of a declared type, mirroring `zend_type`:
/// a builtin type mask plus a list of class names / intersection groups.
#[derive(Debug, Clone, Default)]
pub(crate) struct TypeView {
    pub mask: u32,
    pub elems: Vec<TElem>,
    /// True for a pure intersection type (A&B); elems are then all `Name`.
    pub is_intersection: bool,
}

impl TypeView {
    pub(crate) fn from_param_type(pt: &ParamType) -> TypeView {
        let mut tv = TypeView::default();
        match pt {
            ParamType::Intersection(parts) => {
                tv.is_intersection = true;
                for p in parts {
                    if let ParamType::Simple(n) = p {
                        tv.elems.push(TElem::Name(n.clone()));
                    }
                }
            }
            _ => tv.add(pt),
        }
        tv
    }

    fn add(&mut self, pt: &ParamType) {
        match pt {
            ParamType::Simple(n) => {
                let lower: Vec<u8> = n.iter().map(|b| b.to_ascii_lowercase()).collect();
                match lower.as_slice() {
                    b"int" => self.mask |= T_LONG,
                    b"float" => self.mask |= T_DOUBLE,
                    b"string" => self.mask |= T_STRING,
                    b"bool" => self.mask |= T_BOOL,
                    b"false" => self.mask |= T_FALSE,
                    b"true" => self.mask |= T_TRUE,
                    b"array" => self.mask |= T_ARRAY,
                    b"object" => self.mask |= T_OBJECT,
                    b"callable" => self.mask |= T_CALLABLE,
                    b"mixed" => self.mask |= T_ANY,
                    b"null" => self.mask |= T_NULL,
                    b"void" => self.mask |= T_VOID,
                    b"never" => self.mask |= T_NEVER,
                    b"static" => self.mask |= T_STATIC,
                    b"iterable" => {
                        self.mask |= T_ARRAY;
                        self.push_name(b"Traversable".to_vec());
                    }
                    _ => {
                        let name = if n.first() == Some(&b'\\') { n[1..].to_vec() } else { n.clone() };
                        self.push_name(name);
                    }
                }
            }
            ParamType::Nullable(inner) => {
                self.add(inner);
                self.mask |= T_NULL;
            }
            ParamType::Union(parts) => {
                for p in parts {
                    self.add(p);
                }
            }
            ParamType::Intersection(parts) => {
                let names: Vec<Vec<u8>> = parts
                    .iter()
                    .filter_map(|p| if let ParamType::Simple(n) = p { Some(n.clone()) } else { None })
                    .collect();
                self.elems.push(TElem::Inter(names));
            }
        }
    }

    fn push_name(&mut self, name: Vec<u8>) {
        let dup = self.elems.iter().any(|e| matches!(e, TElem::Name(n) if n.eq_ignore_ascii_case(&name)));
        if !dup {
            self.elems.push(TElem::Name(name));
        }
    }

    fn intersection_of(names: &[Vec<u8>]) -> TypeView {
        TypeView {
            mask: 0,
            elems: names.iter().map(|n| TElem::Name(n.clone())).collect(),
            is_intersection: true,
        }
    }

    fn all_names(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for e in &self.elems {
            match e {
                TElem::Name(n) => out.push(n.clone()),
                TElem::Inter(ns) => out.extend(ns.iter().cloned()),
            }
        }
        out
    }
}

/// Minimal class information needed for variance checks. Can describe
/// classes in the class table, internal classes, or classes that are in the
/// middle of being linked.
#[derive(Debug, Clone)]
pub(crate) struct LinkClass {
    pub name: Vec<u8>,
    pub parent: Option<Vec<u8>>,
    pub interfaces: Vec<Vec<u8>>,
    pub is_final: bool,
    pub has_tostring: bool,
    pub builtin: bool,
    pub is_trait: bool,
}

impl LinkClass {
    pub(crate) fn from_entry(ce: &ClassEntry) -> LinkClass {
        LinkClass {
            name: ce.name.clone(),
            parent: ce.parent.clone(),
            interfaces: ce.interfaces.clone(),
            is_final: ce.is_final || ce.is_enum,
            has_tostring: ce.methods.contains_key(b"__tostring".as_slice()),
            builtin: false,
            is_trait: ce.is_trait,
        }
    }
}

thread_local! {
    /// Classes whose declaration is in progress (visible to variance checks of
    /// classes loaded while they link, like PHP's unlinked class table entries).
    static LINKING: RefCell<Vec<LinkClass>> = RefCell::new(Vec::new());
}

thread_local! {
    /// Hoisted classes whose early binding was deferred to their source position.
    static DEFERRED: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::new());
}

pub(crate) fn defer(name: &[u8]) {
    let l = lower(name);
    DEFERRED.with(|d| d.borrow_mut().push(l));
}

pub(crate) fn is_deferred(name_lower: &[u8]) -> bool {
    DEFERRED.with(|d| d.borrow().iter().any(|n| n.as_slice() == name_lower))
}

/// Returns true (and forgets it) if `name_lower` was deferred.
pub(crate) fn take_deferred(name_lower: &[u8]) -> bool {
    DEFERRED.with(|d| {
        let mut d = d.borrow_mut();
        if let Some(pos) = d.iter().position(|n| n.as_slice() == name_lower) {
            d.remove(pos);
            true
        } else {
            false
        }
    })
}

pub(crate) fn push_linking(ce: &ClassEntry) {
    LINKING.with(|l| l.borrow_mut().push(LinkClass::from_entry(ce)));
}

pub(crate) fn pop_linking() {
    LINKING.with(|l| {
        l.borrow_mut().pop();
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Success,
    Error,
    Unresolved,
}

fn lower(n: &[u8]) -> Vec<u8> {
    let n = if n.first() == Some(&b'\\') { &n[1..] } else { n };
    n.iter().map(|b| b.to_ascii_lowercase()).collect()
}

fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    lower(a) == lower(b)
}

/// Variance-check context: the VM (for class lookup), the class currently
/// being linked, and the list of classes that could not be resolved
/// (PHP's CG(delayed_autoloads)).
pub(crate) struct Ctx<'a> {
    pub vm: &'a Vm,
    pub cur: &'a LinkClass,
    pub unresolved: RefCell<Vec<Vec<u8>>>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(vm: &'a Vm, cur: &'a LinkClass) -> Self {
        Ctx { vm, cur, unresolved: RefCell::new(Vec::new()) }
    }

    pub(crate) fn lookup(&self, name: &[u8]) -> Option<LinkClass> {
        let l = lower(name);
        if lower(&self.cur.name) == l {
            return Some(self.cur.clone());
        }
        let linking = LINKING.with(|s| s.borrow().iter().rev().find(|c| lower(&c.name) == l).cloned());
        if linking.is_some() {
            return linking;
        }
        if let Some(ce) = self.vm.classes.get(&l) {
            return Some(LinkClass::from_entry(ce));
        }
        if self.vm.is_known_builtin_class(&l) {
            return Some(LinkClass {
                name: if name.first() == Some(&b'\\') { name[1..].to_vec() } else { name.to_vec() },
                parent: None,
                interfaces: Vec::new(),
                is_final: matches!(l.as_slice(), b"closure" | b"generator" | b"weakreference" | b"weakmap"),
                has_tostring: false,
                builtin: true,
                is_trait: false,
            });
        }
        None
    }

    fn instanceof(&self, a: &LinkClass, b: &LinkClass) -> bool {
        self.instanceof_depth(a, b, 0)
    }

    fn instanceof_depth(&self, a: &LinkClass, b: &LinkClass, depth: u32) -> bool {
        let al = lower(&a.name);
        let bl = lower(&b.name);
        if al == bl {
            return true;
        }
        if depth > 64 {
            return false;
        }
        if bl == b"stringable" && a.has_tostring {
            return true;
        }
        if a.builtin {
            return super::is_builtin_subclass(&al, &bl)
                || self.vm.builtin_implements_interface(&al, &bl)
                || self.vm.builtin_interface_extends(&al, &bl);
        }
        if let Some(p) = &a.parent {
            if let Some(pc) = self.lookup(p) {
                if self.instanceof_depth(&pc, b, depth + 1) {
                    return true;
                }
            }
        }
        for i in &a.interfaces {
            if let Some(ic) = self.lookup(i) {
                if self.instanceof_depth(&ic, b, depth + 1) {
                    return true;
                }
            }
        }
        false
    }

    fn resolve(&self, scope: &LinkClass, name: &[u8]) -> Vec<u8> {
        if name.eq_ignore_ascii_case(b"parent") {
            if let Some(p) = &scope.parent {
                return p.clone();
            }
        } else if name.eq_ignore_ascii_case(b"self") {
            return scope.name.clone();
        }
        name.to_vec()
    }

    fn register_unresolved_classes(&self, scope: &LinkClass, tv: &TypeView) {
        for n in tv.all_names() {
            let r = self.resolve(scope, &n);
            if self.lookup(&r).is_none() {
                let mut u = self.unresolved.borrow_mut();
                if !u.iter().any(|x| eq_ci(x, &r)) {
                    u.push(r);
                }
            }
        }
    }

    fn type_permits_self(&self, tv: &TypeView, scope: &LinkClass, selfc: &LinkClass) -> bool {
        if tv.mask & T_OBJECT != 0 {
            return true;
        }
        for n in tv.all_names() {
            let r = self.resolve(scope, &n);
            if let Some(ce) = self.lookup(&r) {
                if self.instanceof(selfc, &ce) {
                    return true;
                }
            }
        }
        false
    }

    fn is_intersection_subtype_of_class(
        &self,
        fe_scope: &LinkClass,
        fe: &TypeView,
        proto_scope: &LinkClass,
        proto_class_name: &[u8],
    ) -> Status {
        let _ = proto_scope;
        let mut have_unresolved = false;
        let mut proto_ce: Option<LinkClass> = None;
        for e in &fe.elems {
            let n = match e {
                TElem::Name(n) => n,
                TElem::Inter(_) => continue,
            };
            let fe_class_name = self.resolve(fe_scope, n);
            if eq_ci(&fe_class_name, proto_class_name) {
                return Status::Success;
            }
            if proto_ce.is_none() {
                proto_ce = self.lookup(proto_class_name);
            }
            let fe_ce = self.lookup(&fe_class_name);
            match (&fe_ce, &proto_ce) {
                (Some(f), Some(p)) => {
                    if self.instanceof(f, p) {
                        return Status::Success;
                    }
                }
                _ => {
                    have_unresolved = true;
                }
            }
        }
        if have_unresolved { Status::Unresolved } else { Status::Error }
    }

    fn is_class_subtype_of_type(
        &self,
        fe_scope: &LinkClass,
        fe_class_name: &[u8],
        proto_scope: &LinkClass,
        proto: &TypeView,
    ) -> Status {
        let mut fe_ce: Option<Option<LinkClass>> = None;
        let mut have_unresolved = false;
        let get_fe = |fe_ce: &mut Option<Option<LinkClass>>| -> Option<LinkClass> {
            if fe_ce.is_none() {
                *fe_ce = Some(self.lookup(fe_class_name));
            }
            fe_ce.clone().unwrap()
        };

        if proto.mask & T_OBJECT != 0 {
            match get_fe(&mut fe_ce) {
                None => have_unresolved = true,
                Some(_) => return Status::Success,
            }
        }
        if proto.mask & T_CALLABLE != 0 {
            match get_fe(&mut fe_ce) {
                None => have_unresolved = true,
                Some(c) => {
                    if lower(&c.name) == b"closure" {
                        return Status::Success;
                    }
                }
            }
        }
        if proto.mask & T_STATIC != 0 && fe_scope.is_final {
            match get_fe(&mut fe_ce) {
                None => have_unresolved = true,
                Some(c) => {
                    if lower(&c.name) == lower(&fe_scope.name) {
                        return Status::Success;
                    }
                }
            }
        }

        let is_intersection = proto.is_intersection;
        for e in &proto.elems {
            match e {
                TElem::Inter(names) => {
                    let sub = TypeView::intersection_of(names);
                    match self.is_class_subtype_of_type(fe_scope, fe_class_name, proto_scope, &sub) {
                        Status::Error => {
                            if is_intersection {
                                return Status::Error;
                            }
                        }
                        Status::Unresolved => have_unresolved = true,
                        Status::Success => {
                            if !is_intersection {
                                return Status::Success;
                            }
                        }
                    }
                }
                TElem::Name(n) => {
                    let proto_class_name = self.resolve(proto_scope, n);
                    if eq_ci(fe_class_name, &proto_class_name) {
                        if !is_intersection {
                            return Status::Success;
                        }
                        continue;
                    }
                    let f = get_fe(&mut fe_ce);
                    let p = self.lookup(&proto_class_name);
                    match (f, p) {
                        (Some(f), Some(p)) => {
                            if self.instanceof(&f, &p) {
                                if !is_intersection {
                                    return Status::Success;
                                }
                            } else if is_intersection {
                                return Status::Error;
                            }
                        }
                        _ => {
                            have_unresolved = true;
                        }
                    }
                }
            }
        }
        if have_unresolved {
            return Status::Unresolved;
        }
        if is_intersection { Status::Success } else { Status::Error }
    }

    fn is_intersection_subtype_of_type(
        &self,
        fe_scope: &LinkClass,
        fe: &TypeView,
        proto_scope: &LinkClass,
        proto: &TypeView,
    ) -> Status {
        let mut have_unresolved = false;
        if proto.mask & T_OBJECT != 0 {
            for e in &fe.elems {
                if let TElem::Name(n) = e {
                    let r = self.resolve(fe_scope, n);
                    if self.lookup(&r).is_some() {
                        return Status::Success;
                    } else {
                        have_unresolved = true;
                    }
                }
            }
        }
        let early_exit = if proto.is_intersection { Status::Error } else { Status::Success };
        for e in &proto.elems {
            let status = match e {
                TElem::Inter(names) => {
                    let sub = TypeView::intersection_of(names);
                    self.is_intersection_subtype_of_type(fe_scope, fe, proto_scope, &sub)
                }
                TElem::Name(n) => {
                    let proto_class_name = self.resolve(proto_scope, n);
                    self.is_intersection_subtype_of_class(fe_scope, fe, proto_scope, &proto_class_name)
                }
            };
            if status == early_exit {
                return status;
            }
            if status == Status::Unresolved {
                have_unresolved = true;
            }
        }
        if have_unresolved {
            return Status::Unresolved;
        }
        if early_exit == Status::Error { Status::Success } else { Status::Error }
    }

    /// zend_perform_covariant_type_check: is `fe` a subtype of `proto`?
    pub(crate) fn covariant(
        &self,
        fe_scope: &LinkClass,
        fe: &TypeView,
        proto_scope: &LinkClass,
        proto: &TypeView,
    ) -> Status {
        if proto.mask == T_ANY && proto.elems.is_empty() && fe.mask & T_VOID == 0 {
            return Status::Success;
        }
        let mut added = fe.mask & !proto.mask;
        if added != 0 {
            if added & T_STATIC != 0 && self.type_permits_self(proto, proto_scope, fe_scope) {
                added &= !T_STATIC;
            }
            if added == T_NEVER {
                return Status::Success;
            }
            if added != 0 {
                return Status::Error;
            }
        }

        let early_exit;
        let mut have_unresolved = false;
        if fe.is_intersection {
            early_exit = if proto.is_intersection { Status::Error } else { Status::Success };
            let status = self.is_intersection_subtype_of_type(fe_scope, fe, proto_scope, proto);
            if status == early_exit {
                return status;
            }
            if status == Status::Unresolved {
                have_unresolved = true;
            }
        } else {
            early_exit = Status::Error;
            for e in &fe.elems {
                let status = match e {
                    TElem::Inter(names) => {
                        let sub = TypeView::intersection_of(names);
                        self.is_intersection_subtype_of_type(fe_scope, &sub, proto_scope, proto)
                    }
                    TElem::Name(n) => {
                        let fe_class_name = self.resolve(fe_scope, n);
                        self.is_class_subtype_of_type(fe_scope, &fe_class_name, proto_scope, proto)
                    }
                };
                if status == early_exit {
                    return status;
                }
                if status == Status::Unresolved {
                    have_unresolved = true;
                }
            }
        }
        if !have_unresolved {
            return if early_exit == Status::Error { Status::Success } else { Status::Error };
        }
        self.register_unresolved_classes(fe_scope, fe);
        self.register_unresolved_classes(proto_scope, proto);
        Status::Unresolved
    }

    /// zend_do_perform_implementation_check
    pub(crate) fn implementation_check(
        &self,
        fe: &MethodDef,
        fe_scope: &LinkClass,
        proto: &MethodDef,
        proto_scope: &LinkClass,
    ) -> Status {
        let fe_a = FnArgs::of(fe);
        let pr_a = FnArgs::of(proto);
        if pr_a.required < fe_a.required {
            return Status::Error;
        }
        if proto.op_array.returns_by_ref && !fe.op_array.returns_by_ref {
            return Status::Error;
        }
        if pr_a.variadic && !fe_a.variadic {
            return Status::Error;
        }
        let num_args = pr_a.num.max(fe_a.num);
        let mut status = Status::Success;
        for i in 0..num_args {
            let p = if i < pr_a.num { Some(i) } else if pr_a.variadic { Some(pr_a.num - 1) } else { None };
            let f = if i < fe_a.num { Some(i) } else if fe_a.variadic { Some(fe_a.num - 1) } else { None };
            let p = match p {
                Some(p) => p,
                None => continue,
            };
            let f = match f {
                Some(f) => f,
                None => return Status::Error,
            };
            let local = {
                let ft = fe_a.type_of(&fe.op_array, f);
                match ft {
                    None => Status::Success,
                    Some(ft) if ft.mask == T_ANY && ft.elems.is_empty() => Status::Success,
                    Some(ft) => match pr_a.type_of(&proto.op_array, p) {
                        None => Status::Error,
                        Some(pt) => self.covariant(proto_scope, &pt, fe_scope, &ft),
                    },
                }
            };
            if local != Status::Success {
                if local == Status::Error {
                    return Status::Error;
                }
                status = Status::Unresolved;
            }
            if fe_a.by_ref(&fe.op_array, f) != pr_a.by_ref(&proto.op_array, p) {
                return Status::Error;
            }
        }
        if let Some(prt) = &proto.op_array.return_type {
            match &fe.op_array.return_type {
                None => return Status::Error,
                Some(frt) => {
                    let local = self.covariant(
                        fe_scope,
                        &TypeView::from_param_type(frt),
                        proto_scope,
                        &TypeView::from_param_type(prt),
                    );
                    if local != Status::Success {
                        return local;
                    }
                }
            }
        }
        status
    }

    pub(crate) fn first_unresolved(&self) -> Vec<u8> {
        self.unresolved.borrow().first().cloned().unwrap_or_default()
    }
}

/// Argument layout of a user method's op_array.
struct FnArgs {
    off: usize,
    num: usize,
    required: usize,
    variadic: bool,
}

impl FnArgs {
    fn of(m: &MethodDef) -> FnArgs {
        let oa = &m.op_array;
        let off = if !m.is_static && oa.cv_names.first().map(|n| n.as_slice() == b"this").unwrap_or(false) { 1 } else { 0 };
        let num = (oa.param_count as usize).saturating_sub(off);
        FnArgs { off, num, required: oa.required_param_count as usize, variadic: oa.variadic_param.is_some() }
    }

    fn type_of(&self, oa: &OpArray, i: usize) -> Option<TypeView> {
        oa.param_types
            .get(i + self.off)
            .and_then(|t| t.as_ref())
            .map(|t| TypeView::from_param_type(&t.param_type))
    }

    fn by_ref(&self, oa: &OpArray, i: usize) -> bool {
        oa.by_ref_params.get(i + self.off).copied().unwrap_or(false)
    }
}

/// zend_type_to_string_resolved
pub(crate) fn type_to_string(tv: &TypeView, scope: Option<&LinkClass>) -> String {
    let resolve = |n: &[u8]| -> String {
        let r: Vec<u8> = match scope {
            Some(s) if n.eq_ignore_ascii_case(b"self") => s.name.clone(),
            Some(s) if n.eq_ignore_ascii_case(b"parent") && s.parent.is_some() => s.parent.clone().unwrap(),
            _ => n.to_vec(),
        };
        let cut = r.iter().position(|b| *b == 0).map(|p| &r[..p]).unwrap_or(&r[..]);
        String::from_utf8_lossy(cut).to_string()
    };
    let mut parts: Vec<String> = Vec::new();
    if tv.is_intersection {
        let s: Vec<String> = tv.all_names().iter().map(|n| String::from_utf8_lossy(n).to_string()).collect();
        parts.push(s.join("&"));
    } else {
        for e in &tv.elems {
            match e {
                TElem::Name(n) => parts.push(resolve(n)),
                TElem::Inter(ns) => {
                    let s: Vec<String> = ns.iter().map(|n| String::from_utf8_lossy(n).to_string()).collect();
                    parts.push(format!("({})", s.join("&")));
                }
            }
        }
    }
    let m = tv.mask;
    if m == T_ANY {
        parts.push("mixed".into());
        return parts.join("|");
    }
    if m & T_STATIC != 0 { parts.push("static".into()); }
    if m & T_CALLABLE != 0 { parts.push("callable".into()); }
    if m & T_OBJECT != 0 { parts.push("object".into()); }
    if m & T_ARRAY != 0 { parts.push("array".into()); }
    if m & T_STRING != 0 { parts.push("string".into()); }
    if m & T_LONG != 0 { parts.push("int".into()); }
    if m & T_DOUBLE != 0 { parts.push("float".into()); }
    if m & T_BOOL == T_BOOL {
        parts.push("bool".into());
    } else if m & T_FALSE != 0 {
        parts.push("false".into());
    } else if m & T_TRUE != 0 {
        parts.push("true".into());
    }
    if m & T_VOID != 0 { parts.push("void".into()); }
    if m & T_NEVER != 0 { parts.push("never".into()); }
    if m & T_NULL != 0 {
        let s = parts.join("|");
        if parts.len() == 1 && !s.contains('&') {
            return format!("?{}", s);
        }
        parts.push("null".into());
    }
    parts.join("|")
}

/// Display of a parameter default value, as zend_get_function_declaration does.
fn default_display(oa: &OpArray, cv: u32) -> String {
    // Default prelude: <default expr ops>; Identical(Cv(cv), Undef); JmpZ; Assign(Cv(cv), val)
    let ops = &oa.ops;
    for (i, op) in ops.iter().enumerate() {
        if op.opcode != OpCode::Identical || op.op1 != OperandType::Cv(cv) {
            continue;
        }
        let is_undef = matches!(op.op2, OperandType::Const(c) if matches!(oa.literals.get(c as usize), Some(Value::Undef)));
        if !is_undef || i + 2 >= ops.len() {
            continue;
        }
        let asg = &ops[i + 2];
        if asg.opcode != OpCode::Assign || asg.op1 != OperandType::Cv(cv) {
            continue;
        }
        match asg.op2 {
            OperandType::Const(c) => {
                if let Some(v) = oa.literals.get(c as usize) {
                    return value_display(v);
                }
            }
            OperandType::Tmp(t) => {
                for j in (0..i).rev() {
                    let p = &ops[j];
                    if p.result != OperandType::Tmp(t) {
                        continue;
                    }
                    match p.opcode {
                        OpCode::ConstLookup => {
                            if let OperandType::Const(idx) = p.op1 {
                                if let Some(Value::String(s)) = oa.literals.get(idx as usize) {
                                    let s = s.to_string_lossy();
                                    return s.trim_start_matches('\\').to_string();
                                }
                            }
                        }
                        OpCode::StaticPropGet => {
                            if let (OperandType::Const(ci), OperandType::Const(ni)) = (p.op1, p.op2) {
                                if let (Some(Value::String(c)), Some(Value::String(n))) =
                                    (oa.literals.get(ci as usize), oa.literals.get(ni as usize))
                                {
                                    return format!("{}::{}", c.to_string_lossy(), n.to_string_lossy());
                                }
                            }
                        }
                        OpCode::ArrayNew => {
                            let has_elems = ops[j + 1..i].iter().any(|o| {
                                matches!(o.opcode, OpCode::ArrayAppend | OpCode::ArraySet | OpCode::ArraySpread)
                            });
                            return if has_elems { "[...]".into() } else { "[]".into() };
                        }
                        _ => {}
                    }
                    break;
                }
                return "<expression>".into();
            }
            _ => {}
        }
        break;
    }
    "<default>".into()
}

fn value_display(v: &Value) -> String {
    match v {
        Value::False => "false".into(),
        Value::True => "true".into(),
        Value::Null => "null".into(),
        Value::String(s) => {
            let b = s.as_bytes();
            let shown = String::from_utf8_lossy(&b[..b.len().min(10)]).to_string();
            if b.len() > 10 { format!("'{}...'", shown) } else { format!("'{}'", shown) }
        }
        Value::Array(a) => {
            if a.borrow().len() == 0 { "[]".into() } else { "[...]".into() }
        }
        other => other.to_php_string().to_string_lossy(),
    }
}

/// zend_get_function_declaration
pub(crate) fn function_declaration(m: &MethodDef, class_display: &[u8], scope: &LinkClass) -> String {
    let oa = &m.op_array;
    let mut s = String::new();
    if oa.returns_by_ref {
        s.push_str("& ");
    }
    let cut = class_display.iter().position(|b| *b == 0).map(|p| &class_display[..p]).unwrap_or(class_display);
    s.push_str(&String::from_utf8_lossy(cut));
    s.push_str("::");
    s.push_str(&String::from_utf8_lossy(&m.name));
    s.push('(');
    let a = FnArgs::of(m);
    for i in 0..a.num {
        let cv = i + a.off;
        if let Some(tv) = a.type_of(oa, i) {
            s.push_str(&type_to_string(&tv, Some(scope)));
            s.push(' ');
        }
        if a.by_ref(oa, i) {
            s.push('&');
        }
        let is_variadic = oa.variadic_param == Some(cv as u32);
        if is_variadic {
            s.push_str("...");
        }
        s.push('$');
        if let Some(n) = oa.cv_names.get(cv) {
            s.push_str(&String::from_utf8_lossy(n));
        }
        if i >= a.required && !is_variadic {
            s.push_str(" = ");
            s.push_str(&default_display(oa, cv as u32));
        }
        if i + 1 < a.num {
            s.push_str(", ");
        }
    }
    s.push(')');
    if let Some(rt) = &oa.return_type {
        s.push_str(": ");
        s.push_str(&type_to_string(&TypeView::from_param_type(rt), Some(scope)));
    }
    s
}

impl Vm {
    /// Check that `child` (in class `cur`, being linked) is compatible with
    /// `parent` (declared in `parent_class`). Returns the fatal error message
    /// on failure. Unresolved classes are autoloaded (like PHP's delayed
    /// variance obligations) and the check is redone.
    pub(crate) fn variance_check_method(
        &mut self,
        cur: &ClassEntry,
        child: &MethodDef,
        parent_class: &[u8],
        parent: &MethodDef,
    ) -> Option<(String, bool)> {
        let cur_lc = LinkClass::from_entry(cur);
        let mut attempt = 0;
        loop {
            let (status, unresolved, msg) = {
                let ctx = Ctx::new(self, &cur_lc);
                let parent_decl_lc = self.variance_scope_for(&ctx, &parent.declaring_class, parent_class);
                // Trait methods resolve self/parent against the using class.
                let parent_lc = if parent_decl_lc.is_trait { cur_lc.clone() } else { parent_decl_lc.clone() };
                let child_lc = cur_lc.clone();
                let status = ctx.implementation_check(child, &child_lc, parent, &parent_lc);
                if status == Status::Success {
                    return None;
                }
                let child_display = self.variance_method_class_display(&ctx, child, &cur_lc);
                let parent_display = parent_decl_lc.name.clone();
                let child_decl = function_declaration(child, &child_display, &child_lc);
                let parent_decl = function_declaration(parent, &parent_display, &parent_lc);
                let unresolved: Vec<Vec<u8>> = ctx.unresolved.borrow().clone();
                let msg = if status == Status::Unresolved {
                    format!(
                        "Could not check compatibility between {} and {}, because class {} is not available",
                        child_decl,
                        parent_decl,
                        String::from_utf8_lossy(&ctx.first_unresolved())
                    )
                } else {
                    format!("Declaration of {} must be compatible with {}", child_decl, parent_decl)
                };
                (status, unresolved, msg)
            };
            if status == Status::Unresolved && attempt == 0 && !self.autoload_functions.is_empty() {
                attempt += 1;
                push_linking(cur);
                for n in &unresolved {
                    self.try_autoload_class(n);
                }
                pop_linking();
                continue;
            }
            return Some((msg, status == Status::Unresolved));
        }
    }

    fn variance_scope_for(&self, ctx: &Ctx, declaring_lower: &[u8], fallback: &[u8]) -> LinkClass {
        if !declaring_lower.is_empty() {
            if let Some(c) = ctx.lookup(declaring_lower) {
                return c;
            }
        }
        ctx.lookup(fallback).unwrap_or(LinkClass {
            name: fallback.to_vec(),
            parent: None,
            interfaces: Vec::new(),
            is_final: false,
            has_tostring: false,
            builtin: false,
            is_trait: false,
        })
    }

    /// Class name displayed in front of a child method: the class being linked,
    /// unless the method was inherited from another (non-trait) class.
    fn variance_method_class_display(&self, ctx: &Ctx, m: &MethodDef, cur: &LinkClass) -> Vec<u8> {
        if !m.declaring_class.is_empty() && lower(&m.declaring_class) != lower(&cur.name) {
            if let Some(ce) = self.classes.get(&lower(&m.declaring_class)) {
                if !ce.is_trait {
                    return ce.name.clone();
                }
            }
        }
        let _ = ctx;
        cur.name.clone()
    }
}
