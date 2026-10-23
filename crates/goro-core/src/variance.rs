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
                        let name = if n.first() == Some(&b'\\') {
                            n[1..].to_vec()
                        } else {
                            n.clone()
                        };
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
                    .filter_map(|p| {
                        if let ParamType::Simple(n) = p {
                            Some(n.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                self.elems.push(TElem::Inter(names));
            }
        }
    }

    fn push_name(&mut self, name: Vec<u8>) {
        let dup = self
            .elems
            .iter()
            .any(|e| matches!(e, TElem::Name(n) if n.eq_ignore_ascii_case(&name)));
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
    static LINKING: RefCell<Vec<(ClassEntry, bool)>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// Hoisted classes whose early binding was deferred to their source position.
    static DEFERRED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn defer(name: &[u8]) {
    let l = lower(name);
    DEFERRED.with(|d| d.borrow_mut().push(l));
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

/// Push a class being linked. `nearly_linked`: inheritance is done (only
/// variance obligations remain), so it can already serve as a parent.
pub(crate) fn push_linking_ex(ce: &ClassEntry, nearly_linked: bool) {
    LINKING.with(|l| l.borrow_mut().push((ce.clone(), nearly_linked)));
}

/// Whether `name_lower` is being linked (registered but unlinked in PHP terms).
pub(crate) fn is_linking(name_lower: &[u8]) -> bool {
    LINKING.with(|s| s.borrow().iter().any(|(c, _)| lower(&c.name) == name_lower))
}

thread_local! {
    /// Classes used by a variance check while they were being linked
    /// (CG(unlinked_uses)): one of them that then fails to link cannot be
    /// removed again, the failure is fatal.
    static UNLINKED_USES: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

fn note_unlinked_use(name_lower: &[u8]) {
    UNLINKED_USES.with(|u| {
        let mut u = u.borrow_mut();
        if !u.iter().any(|n| n.as_slice() == name_lower) {
            u.push(name_lower.to_vec());
        }
    });
}

/// Forget class `name_lower`'s unlinked uses (it linked, or failed);
/// whether it had any.
pub(crate) fn take_unlinked_use(name_lower: &[u8]) -> bool {
    UNLINKED_USES.with(|u| {
        let mut u = u.borrow_mut();
        match u.iter().position(|n| n.as_slice() == name_lower) {
            Some(i) => {
                u.remove(i);
                true
            }
            None => false,
        }
    })
}

thread_local! {
    /// CG(delayed_autoloads): classes a variance check of a linking class
    /// could not resolve, loaded in order by load_delayed_classes().
    static DELAYED_AUTOLOADS: RefCell<std::collections::VecDeque<Vec<u8>>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
}

fn register_delayed_autoload(name: &[u8]) {
    let l = lower(name);
    DELAYED_AUTOLOADS.with(|d| {
        let mut d = d.borrow_mut();
        if !d.iter().any(|n| lower(n) == l) {
            d.push_back(name.to_vec());
        }
    });
}

fn pop_delayed_autoload() -> Option<Vec<u8>> {
    let n = DELAYED_AUTOLOADS.with(|d| d.borrow_mut().pop_front())?;
    DELAYED_TRIED.with(|t| t.borrow_mut().push(lower(&n)));
    Some(n)
}

thread_local! {
    /// The delayed autoloads made for the class being linked (and those
    /// linked meanwhile): a check still missing one does not load it again.
    static DELAYED_TRIED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

fn delayed_tried(name_lower: &[u8]) -> bool {
    DELAYED_TRIED.with(|t| t.borrow().iter().any(|n| n.as_slice() == name_lower))
}

pub(crate) fn push_linking(ce: &ClassEntry) {
    push_linking_ex(ce, true);
}

thread_local! {
    /// A fatal (non-exception) error raised while running an autoloader.
    static AUTOLOAD_FATAL: RefCell<Option<super::VmError>> = const { RefCell::new(None) };
}

pub(crate) fn set_autoload_fatal(e: super::VmError) {
    AUTOLOAD_FATAL.with(|f| {
        let mut f = f.borrow_mut();
        if f.is_none() {
            *f = Some(e);
        }
    });
}

pub(crate) fn take_autoload_fatal() -> Option<super::VmError> {
    AUTOLOAD_FATAL.with(|f| f.borrow_mut().take())
}

/// A class whose linking is in progress (PHP's "nearly linked" class-table
/// entry), usable as a parent by classes loaded meanwhile.
pub(crate) fn linking_entry(name_lower: &[u8]) -> Option<ClassEntry> {
    LINKING.with(|s| {
        s.borrow()
            .iter()
            .rev()
            .find(|(c, nl)| *nl && lower(&c.name) == name_lower)
            .map(|(c, _)| c.clone())
    })
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
    let n = if n.first() == Some(&b'\\') {
        &n[1..]
    } else {
        n
    };
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
        Ctx {
            vm,
            cur,
            unresolved: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn lookup(&self, name: &[u8]) -> Option<LinkClass> {
        let l = lower(name);
        if lower(&self.cur.name) == l {
            return Some(self.cur.clone());
        }
        let linking = LINKING.with(|s| {
            s.borrow()
                .iter()
                .rev()
                .find(|(c, _)| lower(&c.name) == l)
                .map(|(c, _)| LinkClass::from_entry(c))
        });
        if linking.is_some() {
            note_unlinked_use(&l);
            return linking;
        }
        if let Some(ce) = self.vm.classes.get(&l) {
            return Some(LinkClass::from_entry(ce));
        }
        if self.vm.is_known_builtin_class(&l) {
            return Some(LinkClass {
                name: if name.first() == Some(&b'\\') {
                    name[1..].to_vec()
                } else {
                    name.to_vec()
                },
                parent: None,
                interfaces: Vec::new(),
                is_final: matches!(
                    l.as_slice(),
                    b"closure" | b"generator" | b"weakreference" | b"weakmap"
                ),
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
            // Builtins missing from the VM's hierarchy tables.
            let extra: &[&[u8]] = match al.as_slice() {
                b"generator" => &[b"iterator", b"traversable"],
                b"datetime" | b"datetimeimmutable" => &[b"datetimeinterface"],
                _ => &[],
            };
            if extra.contains(&bl.as_slice()) {
                return true;
            }
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
        if have_unresolved {
            Status::Unresolved
        } else {
            Status::Error
        }
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
                    match self.is_class_subtype_of_type(fe_scope, fe_class_name, proto_scope, &sub)
                    {
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
        if is_intersection {
            Status::Success
        } else {
            Status::Error
        }
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
        let early_exit = if proto.is_intersection {
            Status::Error
        } else {
            Status::Success
        };
        for e in &proto.elems {
            let status = match e {
                TElem::Inter(names) => {
                    let sub = TypeView::intersection_of(names);
                    self.is_intersection_subtype_of_type(fe_scope, fe, proto_scope, &sub)
                }
                TElem::Name(n) => {
                    let proto_class_name = self.resolve(proto_scope, n);
                    self.is_intersection_subtype_of_class(
                        fe_scope,
                        fe,
                        proto_scope,
                        &proto_class_name,
                    )
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
        if early_exit == Status::Error {
            Status::Success
        } else {
            Status::Error
        }
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
            early_exit = if proto.is_intersection {
                Status::Error
            } else {
                Status::Success
            };
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
            return if early_exit == Status::Error {
                Status::Success
            } else {
                Status::Error
            };
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
            let p = if i < pr_a.num {
                Some(i)
            } else if pr_a.variadic {
                Some(pr_a.num - 1)
            } else {
                None
            };
            let f = if i < fe_a.num {
                Some(i)
            } else if fe_a.variadic {
                Some(fe_a.num - 1)
            } else {
                None
            };
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
        self.unresolved
            .borrow()
            .first()
            .cloned()
            .unwrap_or_default()
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
        let off = if !m.is_static
            && oa
                .cv_names
                .first()
                .map(|n| n.as_slice() == b"this")
                .unwrap_or(false)
        {
            1
        } else {
            0
        };
        let num = (oa.param_count as usize).saturating_sub(off);
        FnArgs {
            off,
            num,
            required: oa.required_param_count as usize,
            variadic: oa.variadic_param.is_some(),
        }
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
            Some(s) if n.eq_ignore_ascii_case(b"parent") && s.parent.is_some() => {
                s.parent.clone().unwrap()
            }
            _ => n.to_vec(),
        };
        let cut = r
            .iter()
            .position(|b| *b == 0)
            .map(|p| &r[..p])
            .unwrap_or(&r[..]);
        String::from_utf8_lossy(cut).to_string()
    };
    let mut parts: Vec<String> = Vec::new();
    if tv.is_intersection {
        let s: Vec<String> = tv
            .all_names()
            .iter()
            .map(|n| String::from_utf8_lossy(n).to_string())
            .collect();
        parts.push(s.join("&"));
    } else {
        for e in &tv.elems {
            match e {
                TElem::Name(n) => parts.push(resolve(n)),
                TElem::Inter(ns) => {
                    let s: Vec<String> = ns
                        .iter()
                        .map(|n| String::from_utf8_lossy(n).to_string())
                        .collect();
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
    if m & T_STATIC != 0 {
        parts.push("static".into());
    }
    if m & T_CALLABLE != 0 {
        parts.push("callable".into());
    }
    if m & T_OBJECT != 0 {
        parts.push("object".into());
    }
    if m & T_ARRAY != 0 {
        parts.push("array".into());
    }
    if m & T_STRING != 0 {
        parts.push("string".into());
    }
    if m & T_LONG != 0 {
        parts.push("int".into());
    }
    if m & T_DOUBLE != 0 {
        parts.push("float".into());
    }
    if m & T_BOOL == T_BOOL {
        parts.push("bool".into());
    } else if m & T_FALSE != 0 {
        parts.push("false".into());
    } else if m & T_TRUE != 0 {
        parts.push("true".into());
    }
    if m & T_VOID != 0 {
        parts.push("void".into());
    }
    if m & T_NEVER != 0 {
        parts.push("never".into());
    }
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
    // Internal functions: the default as written in the stub.
    if let Some(f) = oa.internal_sig {
        let off = u32::from(oa.cv_names.first().is_some_and(|n| n == b"this"));
        if let Some(d) = cv
            .checked_sub(off)
            .and_then(|i| f.args.get(i as usize))
            .and_then(|a| a.default)
        {
            return d.to_string();
        }
    }
    // Default prelude: Identical(Cv(cv), Undef); JmpZ after; <default expr ops>;
    // Assign(Cv(cv), val); after:
    let ops = &oa.ops;
    for (k, op) in ops.iter().enumerate() {
        if op.opcode != OpCode::Identical || op.op1 != OperandType::Cv(cv) {
            continue;
        }
        let is_undef = matches!(op.op2, OperandType::Const(c) if matches!(oa.literals.get(c as usize), Some(Value::Undef)));
        if !is_undef || k + 2 >= ops.len() {
            continue;
        }
        // The Assign is the last one to the CV before the jump target (a
        // RecvDefaultCheck may follow it).
        let i = match ops[k + 1].op2 {
            OperandType::JmpTarget(t)
                if ops[k + 1].opcode == OpCode::JmpZ && t as usize > k + 2 =>
            {
                match (k + 2..(t as usize).min(ops.len()))
                    .rev()
                    .find(|&j| ops[j].opcode == OpCode::Assign && ops[j].op1 == OperandType::Cv(cv))
                {
                    Some(j) => j,
                    None => continue,
                }
            }
            _ => continue,
        };
        let asg = match ops.get(i) {
            Some(a) => a,
            None => continue,
        };
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
                            if let (OperandType::Const(ci), OperandType::Const(ni)) = (p.op1, p.op2)
                            {
                                if let (Some(Value::String(c)), Some(Value::String(n))) =
                                    (oa.literals.get(ci as usize), oa.literals.get(ni as usize))
                                {
                                    return format!(
                                        "{}::{}",
                                        c.to_string_lossy(),
                                        n.to_string_lossy()
                                    );
                                }
                            }
                        }
                        OpCode::ArrayNew => {
                            let has_elems = ops[j + 1..i].iter().any(|o| {
                                matches!(
                                    o.opcode,
                                    OpCode::ArrayAppend | OpCode::ArraySet | OpCode::ArraySpread
                                )
                            });
                            return if has_elems {
                                "[...]".into()
                            } else {
                                "[]".into()
                            };
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
            let shown = crate::string::msg_str(&b[..b.len().min(10)]).to_string();
            if b.len() > 10 {
                format!("'{}...'", shown)
            } else {
                format!("'{}'", shown)
            }
        }
        Value::Array(a) => {
            if a.borrow().is_empty() {
                "[]".into()
            } else {
                "[...]".into()
            }
        }
        other => other.to_php_string().to_string_lossy(),
    }
}

/// zend_get_function_declaration
pub(crate) fn function_declaration(
    m: &MethodDef,
    class_display: &[u8],
    scope: &LinkClass,
) -> String {
    let oa = &m.op_array;
    let mut s = String::new();
    if oa.returns_by_ref {
        s.push_str("& ");
    }
    let cut = class_display
        .iter()
        .position(|b| *b == 0)
        .map(|p| &class_display[..p])
        .unwrap_or(class_display);
    s.push_str(&String::from_utf8_lossy(cut));
    s.push_str("::");
    // Property hooks display as `$prop::get` / `$prop::set`.
    let lname = m.name.to_ascii_lowercase();
    let hook = if lname.starts_with(b"__property_get_") {
        Some((&m.name[b"__property_get_".len()..], "get"))
    } else if lname.starts_with(b"__property_set_") {
        Some((&m.name[b"__property_set_".len()..], "set"))
    } else {
        None
    };
    match hook {
        Some((prop, kind)) => {
            s.push('$');
            s.push_str(&String::from_utf8_lossy(prop));
            s.push_str("::");
            s.push_str(kind);
        }
        None => s.push_str(&String::from_utf8_lossy(&m.name)),
    }
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
            // Internal functions show the stub's default expression.
            match oa
                .internal_sig
                .and_then(|f| f.args.get(i))
                .and_then(|arg| arg.default)
            {
                Some(d) => s.push_str(d),
                None => s.push_str(&default_display(oa, cv as u32)),
            }
        }
        if i + 1 < a.num {
            s.push_str(", ");
        }
    }
    s.push(')');
    if let Some(rt) = &oa.return_type {
        s.push_str(": ");
        s.push_str(&type_to_string(&TypeView::from_param_type(rt), Some(scope)));
    } else if matches!(hook, Some((_, "set"))) {
        // A set hook's implicit return type.
        s.push_str(": void");
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
        // An internal method implemented in PHP (an extension class declared
        // from source): overriding methods are checked against its stub
        // signature, a tentative return type only deprecating.
        if let Some(sig) = parent
            .op_array
            .internal_sig
            .filter(|_| !parent.op_array.ops.is_empty())
        {
            let mut pm = parent.clone();
            pm.op_array = std::rc::Rc::new(crate::stubs::op_array_for(
                sig,
                Some(&parent.declaring_class),
                !parent.is_static,
            ));
            if sig.flags & crate::stubs::F_TENTATIVE != 0 && pm.op_array.return_type.is_some() {
                let mut relaxed = pm.clone();
                std::rc::Rc::make_mut(&mut relaxed.op_array).return_type = None;
                if let Some(r) =
                    self.variance_check_method_inner(cur, child, parent_class, &relaxed)
                {
                    return Some(
                        self.variance_check_method_inner(cur, child, parent_class, &pm)
                            .unwrap_or(r),
                    );
                }
                if let Some((msg, unresolved)) =
                    self.variance_check_method_inner(cur, child, parent_class, &pm)
                {
                    if unresolved {
                        return Some((msg, true));
                    }
                    let will_change = child.attributes.iter().any(|a| {
                        let n = a.name.strip_prefix(b"\\").unwrap_or(&a.name);
                        n.eq_ignore_ascii_case(b"ReturnTypeWillChange")
                    });
                    if !will_change {
                        if let Some(rest) = msg.strip_prefix("Declaration of ") {
                            if let Some((c, p)) = rest.split_once(" must be compatible with ") {
                                let dep = format!(
                                    "Return type of {} should either be compatible with {}, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice",
                                    c, p
                                );
                                let line = child.op_array.decl_line;
                                self.emit_deprecated_at(&dep, line);
                            }
                        }
                    }
                }
                return None;
            }
            return self.variance_check_method_inner(cur, child, parent_class, &pm);
        }
        self.variance_check_method_inner(cur, child, parent_class, parent)
    }

    fn variance_check_method_inner(
        &mut self,
        cur: &ClassEntry,
        child: &MethodDef,
        parent_class: &[u8],
        parent: &MethodDef,
    ) -> Option<(String, bool)> {
        let mut attempt = 0;
        loop {
            let (status, unresolved, msg) = self.variance_status(cur, child, parent_class, parent);
            if status == Status::Success {
                return None;
            }
            if status == Status::Unresolved && attempt == 0 && !self.autoload_functions.is_empty() {
                attempt += 1;
                push_linking(cur);
                for n in &unresolved {
                    if delayed_tried(&lower(n)) {
                        continue;
                    }
                    self.try_autoload_class(n);
                    // zend_exception_uncaught_error(): an exception thrown
                    // while loading a class the check needs is fatal.
                    if let Some(exc) = self.current_exception.take() {
                        pop_linking();
                        let shown = self
                            .call_object_method(&exc, b"__toString", &[])
                            .map(|v| v.to_php_string().to_msg_string())
                            .unwrap_or_default();
                        self.current_exception = None;
                        return Some((
                            format!(
                                "During inheritance of {}, while autoloading {}: Uncaught {}",
                                String::from_utf8_lossy(&cur.name),
                                String::from_utf8_lossy(n),
                                shown
                            ),
                            false,
                        ));
                    }
                }
                pop_linking();
                continue;
            }
            return Some((msg, status == Status::Unresolved));
        }
    }

    /// One compatibility check of `child` (of `cur`) against `parent`,
    /// without autoloading: the status, the classes it could not resolve
    /// and the error message.
    pub(crate) fn variance_status(
        &self,
        cur: &ClassEntry,
        child: &MethodDef,
        parent_class: &[u8],
        parent: &MethodDef,
    ) -> (Status, Vec<Vec<u8>>, String) {
        let cur_lc = LinkClass::from_entry(cur);
        let ctx = Ctx::new(self, &cur_lc);
        let parent_decl_lc = self.variance_scope_for(&ctx, &parent.declaring_class, parent_class);
        // Trait methods resolve self/parent against the using class.
        let parent_lc = if parent_decl_lc.is_trait {
            cur_lc.clone()
        } else {
            parent_decl_lc.clone()
        };
        let child_lc = cur_lc.clone();
        let status = ctx.implementation_check(child, &child_lc, parent, &parent_lc);
        if status == Status::Success {
            return (status, Vec::new(), String::new());
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
                crate::string::msg_str(&ctx.first_unresolved())
            )
        } else {
            format!(
                "Declaration of {} must be compatible with {}",
                child_decl, parent_decl
            )
        };
        (status, unresolved, msg)
    }

    /// The methods of `class` overriding methods of `parent` that are
    /// checked for compatibility (do_inheritance_check_on_method).
    fn variance_overrides<'c>(
        class: &'c ClassEntry,
        parent: &'c ClassEntry,
    ) -> Vec<(&'c MethodDef, &'c MethodDef)> {
        let mut out = Vec::new();
        for (name, pm) in &parent.methods {
            if pm.visibility == crate::object::Visibility::Private
                || (name.as_slice() == b"__construct" && !pm.is_abstract)
            {
                continue;
            }
            if let Some(cm) = class.methods.get(name) {
                if !std::rc::Rc::ptr_eq(&cm.op_array, &pm.op_array) {
                    out.push((cm, pm));
                }
            }
        }
        out
    }

    /// Before `class` checks its methods against `parent`'s: the classes
    /// those checks cannot resolve yet join the delayed autoloads
    /// (CG(delayed_autoloads)), which are then loaded in order
    /// (load_delayed_classes()). Err: the fatal error of an exception
    /// thrown while loading one.
    pub(crate) fn variance_load_delayed_for(
        &mut self,
        class: &ClassEntry,
        parent: &ClassEntry,
    ) -> Result<(), String> {
        if self.autoload_functions.is_empty() {
            return Ok(());
        }
        if !LINKING.with(|l| !l.borrow().is_empty()) {
            DELAYED_TRIED.with(|t| t.borrow_mut().clear());
        }
        let mut names: Vec<Vec<u8>> = Vec::new();
        for (cm, pm) in Self::variance_overrides(class, parent) {
            let (status, unresolved, _) = self.variance_status(class, cm, &parent.name, pm);
            if status == Status::Unresolved {
                names.extend(unresolved);
            }
        }
        for n in names {
            register_delayed_autoload(&n);
        }
        self.variance_load_delayed(class)
    }

    /// load_delayed_classes() while `class` links.
    pub(crate) fn variance_load_delayed(&mut self, class: &ClassEntry) -> Result<(), String> {
        while let Some(n) = pop_delayed_autoload() {
            let l = lower(&n);
            if self.classes.contains_key(&l) || is_linking(&l) {
                continue;
            }
            push_linking(class);
            self.try_autoload_class(&n);
            pop_linking();
            if let Some(exc) = self.current_exception.take() {
                let shown = self
                    .call_object_method(&exc, b"__toString", &[])
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                self.current_exception = None;
                return Err(format!(
                    "During inheritance of {}, while autoloading {}: Uncaught {}",
                    String::from_utf8_lossy(&class.name),
                    String::from_utf8_lossy(&n),
                    shown
                ));
            }
        }
        Ok(())
    }

    /// `class` links with a parent that is itself still linking (nearly
    /// linked, with variance obligations): the delayed autoloads are
    /// loaded, then the parent's obligations are checked
    /// (OBLIGATION_DEPENDENCY: resolve_delayed_variance_obligations()).
    /// Err: the parent's incompatibility (message, line).
    pub(crate) fn variance_resolve_dependency(
        &mut self,
        class: &ClassEntry,
        parent: &ClassEntry,
    ) -> Result<(), (String, u32)> {
        push_linking(class);
        let r = self.variance_load_delayed(class);
        let r = match r {
            Err(m) => Err((m, 0)),
            Ok(()) => {
                let gp = parent.parent.as_ref().and_then(|g| {
                    let gl = lower(g);
                    self.classes
                        .get(&gl)
                        .cloned()
                        .or_else(|| linking_entry(&gl))
                });
                let mut res = Ok(());
                if let Some(gp) = gp {
                    for (cm, pm) in Self::variance_overrides(parent, &gp) {
                        let (status, _, msg) = self.variance_status(parent, cm, &gp.name, pm);
                        if status == Status::Error {
                            res = Err((msg, cm.op_array.decl_line));
                            break;
                        }
                    }
                }
                res
            }
        };
        pop_linking();
        r
    }

    fn variance_scope_for(&self, ctx: &Ctx, declaring_lower: &[u8], fallback: &[u8]) -> LinkClass {
        // Internal classes are displayed with their canonical name.
        let canon = |mut c: LinkClass| {
            if c.builtin {
                c.name = match crate::stubs::find_class(&c.name) {
                    Some(sc) => sc.name.as_bytes().to_vec(),
                    None => self.builtin_canonical_name(&lower(&c.name)).into_bytes(),
                };
            }
            c
        };
        if !declaring_lower.is_empty() {
            if let Some(c) = ctx.lookup(declaring_lower) {
                return canon(c);
            }
        }
        ctx.lookup(fallback).map(canon).unwrap_or(LinkClass {
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

impl Vm {
    /// zend_verify_property_hook_variance: the set hook's `$value` type
    /// must accept every value of the property type. Unresolved classes
    /// are autoloaded (delayed variance obligation) and the check redone.
    pub(crate) fn variance_hook_param(
        &mut self,
        cur: &ClassEntry,
        prop_type: &ParamType,
        param_type: &ParamType,
    ) -> bool {
        let cur_lc = LinkClass::from_entry(cur);
        let prop_tv = TypeView::from_param_type(prop_type);
        let param_tv = TypeView::from_param_type(param_type);
        let mut attempt = 0;
        loop {
            let (status, unresolved) = {
                let ctx = Ctx::new(self, &cur_lc);
                let status = ctx.covariant(&cur_lc, &prop_tv, &cur_lc, &param_tv);
                let unresolved: Vec<Vec<u8>> = ctx.unresolved.borrow().clone();
                (status, unresolved)
            };
            if status == Status::Unresolved && attempt == 0 && !self.autoload_functions.is_empty() {
                attempt += 1;
                push_linking(cur);
                for n in &unresolved {
                    if delayed_tried(&lower(n)) {
                        continue;
                    }
                    self.try_autoload_class(n);
                }
                pop_linking();
                continue;
            }
            return status == Status::Success;
        }
    }

    /// verify_property_type_compatibility() for an abstract (interface)
    /// property with a set hook that the class implements with a plain
    /// property: the hook's `$value` type must be a subtype of the class's
    /// property type. Returns the hook type rendered in the interface's
    /// scope on failure.
    pub(crate) fn variance_abstract_set_type(
        &mut self,
        cur: &ClassEntry,
        iface_lower: &[u8],
        set_type: &ParamType,
        child_type: &ParamType,
    ) -> Option<String> {
        let cur_lc = LinkClass::from_entry(cur);
        let set_tv = TypeView::from_param_type(set_type);
        let child_tv = TypeView::from_param_type(child_type);
        let ctx = Ctx::new(self, &cur_lc);
        let iface_lc = self.variance_scope_for(&ctx, iface_lower, iface_lower);
        if ctx.covariant(&iface_lc, &set_tv, &cur_lc, &child_tv) == Status::Error {
            return Some(type_to_string(&set_tv, Some(&iface_lc)));
        }
        None
    }

    /// full_property_types_compatible: `variance` 0 = invariant, 1 = covariant,
    /// 2 = contravariant. Returns the status, the parent type rendered as
    /// PHP does in "Type of X::$p must be ..." errors, and the parent's class name.
    pub(crate) fn variance_property_types(
        &self,
        cur: &ClassEntry,
        parent_decl_class: &[u8],
        parent_type: &Option<ParamType>,
        child_type: &Option<ParamType>,
        variance: u8,
    ) -> (Status, String, Vec<u8>) {
        let cur_lc = LinkClass::from_entry(cur);
        let ctx = Ctx::new(self, &cur_lc);
        let parent_lc = self.variance_scope_for(&ctx, parent_decl_class, parent_decl_class);
        let parent_str = parent_type
            .as_ref()
            .map(|t| type_to_string(&TypeView::from_param_type(t), Some(&parent_lc)))
            .unwrap_or_default();
        let parent_name = parent_lc.name.clone();
        let (pt, ct) = match (parent_type, child_type) {
            (None, None) => return (Status::Success, parent_str, parent_name),
            (Some(p), Some(c)) => (TypeView::from_param_type(p), TypeView::from_param_type(c)),
            _ => return (Status::Error, parent_str, parent_name),
        };
        let s1 = if variance == 2 {
            Status::Success
        } else {
            ctx.covariant(&cur_lc, &ct, &parent_lc, &pt)
        };
        let s2 = if variance == 1 {
            Status::Success
        } else {
            ctx.covariant(&parent_lc, &pt, &cur_lc, &ct)
        };
        let st = if s1 == Status::Success && s2 == Status::Success {
            Status::Success
        } else if s1 == Status::Error || s2 == Status::Error {
            Status::Error
        } else {
            Status::Unresolved
        };
        (st, parent_str, parent_name)
    }
}

/// is_numeric_str_function: whole string numeric (leading/trailing whitespace ok)
pub(crate) fn numeric_str(b: &[u8]) -> Option<Value> {
    let t: &[u8] = {
        let mut st = 0;
        let mut en = b.len();
        while st < en && matches!(b[st], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            st += 1;
        }
        while en > st && matches!(b[en - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            en -= 1;
        }
        &b[st..en]
    };
    if t.is_empty() {
        return None;
    }
    let digits = if t[0] == b'+' || t[0] == b'-' {
        &t[1..]
    } else {
        t
    };
    if !digits.is_empty() && digits.iter().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = std::str::from_utf8(t)
            .ok()?
            .trim_start_matches('+')
            .parse::<i64>()
        {
            return Some(Value::Long(n));
        }
    }
    // float syntax: digits with optional '.', optional exponent
    let mut i = 0;
    let mut nd = 0;
    while i < digits.len() && digits[i].is_ascii_digit() {
        i += 1;
        nd += 1;
    }
    if i < digits.len() && digits[i] == b'.' {
        i += 1;
        while i < digits.len() && digits[i].is_ascii_digit() {
            i += 1;
            nd += 1;
        }
    }
    if nd == 0 {
        return None;
    }
    if i < digits.len() && (digits[i] == b'e' || digits[i] == b'E') {
        let mut j = i + 1;
        if j < digits.len() && (digits[j] == b'+' || digits[j] == b'-') {
            j += 1;
        }
        let es = j;
        while j < digits.len() && digits[j].is_ascii_digit() {
            j += 1;
        }
        if j == es {
            return None;
        }
        i = j;
    }
    if i != digits.len() {
        return None;
    }
    std::str::from_utf8(t)
        .ok()?
        .parse::<f64>()
        .ok()
        .map(Value::Double)
}

impl Vm {
    /// Runtime rendering of a declared type for TypeError messages
    /// (zend_type_to_string_resolved with the current scope): canonical PHP
    /// member order, self/parent/static resolved.
    pub(crate) fn canonical_type_display(&self, pt: &ParamType) -> String {
        let mut tv = TypeView::from_param_type(pt);
        let mut static_name: Option<String> = None;
        if tv.mask & T_STATIC != 0 {
            let s = self.param_type_name_simple(b"static");
            if s != "static" {
                static_name = Some(s);
            }
        }
        for e in tv.elems.iter_mut() {
            if let TElem::Name(n) = e {
                if n.eq_ignore_ascii_case(b"self") || n.eq_ignore_ascii_case(b"parent") {
                    let lower: Vec<u8> = n.iter().map(|b| b.to_ascii_lowercase()).collect();
                    *n = self.param_type_name_simple(&lower).into_bytes();
                }
            }
        }
        if let Some(sn) = static_name {
            // The called scope name takes the place of "static", right after class names.
            tv.mask &= !T_STATIC;
            tv.elems.push(TElem::Name(sn.into_bytes()));
        }
        type_to_string(&tv, None)
    }
}

impl Vm {
    /// class_constant_types_compatible + emit_incompatible_class_constant_error.
    /// Returns the error message when the child constant's type is incompatible.
    pub(crate) fn variance_class_constant(
        &self,
        cur: &ClassEntry,
        const_name: &[u8],
        parent_decl_class: &[u8],
        parent_type: &ParamType,
        child_type: Option<&ParamType>,
    ) -> Option<(String, bool)> {
        let cur_lc = LinkClass::from_entry(cur);
        let ctx = Ctx::new(self, &cur_lc);
        let parent_lc = self.variance_scope_for(&ctx, parent_decl_class, parent_decl_class);
        let pt = TypeView::from_param_type(parent_type);
        let st = match child_type {
            None => Status::Error,
            Some(ct) => ctx.covariant(&cur_lc, &TypeView::from_param_type(ct), &parent_lc, &pt),
        };
        if st == Status::Success {
            return None;
        }
        Some((
            format!(
                "Type of {}::{} must be compatible with {}::{} of type {}",
                String::from_utf8_lossy(&cur.name),
                String::from_utf8_lossy(const_name),
                String::from_utf8_lossy(&parent_lc.name),
                String::from_utf8_lossy(const_name),
                type_to_string(&pt, Some(&parent_lc))
            ),
            st == Status::Unresolved,
        ))
    }
}

impl Vm {
    /// Strict type check of a value against a declared type (no coercion),
    /// with real class-hierarchy checks for objects.
    pub(crate) fn value_satisfies_type_strict(
        &self,
        v: &Value,
        pt: &ParamType,
        scope: Option<&[u8]>,
    ) -> bool {
        let tv = TypeView::from_param_type(pt);
        let bit = match v {
            Value::Null | Value::Undef => T_NULL,
            Value::False => T_FALSE,
            Value::True => T_TRUE,
            Value::Long(_) => T_LONG,
            Value::Double(_) => T_DOUBLE,
            Value::String(_) => T_STRING,
            Value::Array(_) => T_ARRAY,
            Value::Object(o) => {
                if tv.mask & T_OBJECT != 0 {
                    return true;
                }
                let cname = o.borrow().class_name.clone();
                let dummy = LinkClass {
                    name: Vec::new(),
                    parent: None,
                    interfaces: Vec::new(),
                    is_final: false,
                    has_tostring: false,
                    builtin: false,
                    is_trait: false,
                };
                let ctx = Ctx::new(self, &dummy);
                let oc = match ctx.lookup(&cname) {
                    Some(c) => c,
                    None => return false,
                };
                let scope_lc = scope.and_then(|sc| ctx.lookup(sc));
                if tv.mask & T_STATIC != 0 {
                    if let Some(sc) = &scope_lc {
                        if ctx.instanceof(&oc, sc) {
                            return true;
                        }
                    }
                }
                let is_a = |n: &[u8]| -> bool {
                    let n: Vec<u8> = match &scope_lc {
                        Some(sc) if n.eq_ignore_ascii_case(b"self") => sc.name.clone(),
                        Some(sc) if n.eq_ignore_ascii_case(b"parent") => {
                            sc.parent.clone().unwrap_or_default()
                        }
                        _ => n.to_vec(),
                    };
                    match ctx.lookup(&n) {
                        Some(t) => ctx.instanceof(&oc, &t),
                        None => false,
                    }
                };
                if tv.is_intersection {
                    return tv.all_names().iter().all(|n| is_a(n));
                }
                return tv.elems.iter().any(|e| match e {
                    TElem::Name(n) => is_a(n),
                    TElem::Inter(ns) => ns.iter().all(|n| is_a(n)),
                });
            }
            _ => return true,
        };
        tv.mask & bit != 0
    }
}

impl Vm {
    /// Prototype of the constructor of class `class_lower` (zend_function.prototype):
    /// an abstract constructor or one declared by an implemented interface.
    /// Constructors are only signature-checked against such a prototype.
    pub(crate) fn ctor_prototype(&self, class_lower: &[u8]) -> Option<(Vec<u8>, MethodDef)> {
        self.ctor_prototype_depth(class_lower, 0)
    }

    fn iface_ctor(&self, iface_lower: &[u8], depth: u32) -> Option<(Vec<u8>, MethodDef)> {
        if depth > 32 {
            return None;
        }
        let ic = self.classes.get(iface_lower)?;
        if let Some(m) = ic.methods.get(b"__construct".as_slice()) {
            let decl = self
                .classes
                .get(&m.declaring_class)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| ic.name.clone());
            return Some((decl, m.clone()));
        }
        let mut parents: Vec<Vec<u8>> = ic.interfaces.clone();
        if let Some(p) = &ic.parent {
            parents.insert(0, p.clone());
        }
        for p in parents {
            if let Some(r) = self.iface_ctor(&lower(&p), depth + 1) {
                return Some(r);
            }
        }
        None
    }

    fn ctor_prototype_depth(&self, class_lower: &[u8], depth: u32) -> Option<(Vec<u8>, MethodDef)> {
        if depth > 32 {
            return None;
        }
        let c = self.classes.get(class_lower)?;
        let m = c.methods.get(b"__construct".as_slice())?;
        let decl_lower = if m.declaring_class.is_empty() {
            class_lower.to_vec()
        } else {
            lower(&m.declaring_class)
        };
        let d = self.classes.get(&decl_lower).unwrap_or(c);
        if m.is_abstract {
            return Some((d.name.clone(), m.clone()));
        }
        for i in &d.interfaces {
            if let Some(r) = self.iface_ctor(&lower(i), 0) {
                return Some(r);
            }
        }
        if let Some(p) = &d.parent {
            return self.ctor_prototype_depth(&lower(p), depth + 1);
        }
        None
    }
}

impl Vm {
    /// zend_enum_build_backed_enum_table: validates the (lazily evaluated)
    /// backing values of a backed enum. Returns (exception class, message);
    /// an empty class when evaluating a value threw (left pending).
    pub(crate) fn enum_backed_table_error(
        &mut self,
        class_lower: &[u8],
    ) -> Option<(&'static [u8], String)> {
        let class = self.classes.get(class_lower)?.clone();
        let bt = class.enum_backing_type.clone()?;
        let mut seen: Vec<(Vec<u8>, Value)> = Vec::new();
        for (case_name, raw) in &class.enum_cases {
            let v = self.resolve_deferred_value(raw);
            if self.current_exception.is_some() {
                return Some((b"", String::from("Uncaught exception")));
            }
            let ok = if bt.eq_ignore_ascii_case(b"int") {
                matches!(v, Value::Long(_))
            } else {
                matches!(v, Value::String(_))
            };
            if !ok {
                return Some((
                    b"TypeError",
                    format!(
                        "Enum case type {} does not match enum backing type {}",
                        v.type_name(),
                        String::from_utf8_lossy(&bt)
                    ),
                ));
            }
            if let Some((prev, _)) = seen.iter().find(|(_, pv)| pv.identical(&v)) {
                return Some((
                    b"Error",
                    format!(
                        "Duplicate value in enum {} for cases {} and {}",
                        String::from_utf8_lossy(&class.name),
                        String::from_utf8_lossy(prev),
                        String::from_utf8_lossy(case_name)
                    ),
                ));
            }
            seen.push((case_name.clone(), v));
        }
        None
    }
}
