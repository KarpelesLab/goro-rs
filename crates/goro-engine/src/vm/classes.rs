//! Class lookup (with autoloading), declaration and linking: inheritance,
//! interfaces, traits, enums, constants and static properties.
//!
//! PHP manual: Objects (Inheritance, Abstract classes, Interfaces, Traits,
//! Enumerations), Autoloading classes. Error messages observed on PHP 8.5.

use crate::class::{DisplayName, TypeName};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{ObjRef, PhpRef, PhpStr, Value};

use super::{R, Vm};
use crate::bytecode::*;
use crate::class::*;
use crate::func::{Func, FuncKind};

impl Vm {
    /// Finds a class by name, declaring lazily-registered internal classes
    /// and calling autoloaders when `autoload` is set.
    pub fn find_class(&mut self, name: &[u8], autoload: bool) -> R<Option<Rc<Class>>> {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        let lname = lower(name);
        if let Some(c) = self.classes.get(&lname) {
            return Ok(Some(c.clone()));
        }
        if let Some(&(m, i)) = self.lazy_classes.get(&lname) {
            self.lazy_classes.remove(&lname);
            self.declare_internal_class(m, i)?;
            if let Some(c) = self.classes.get(&lname) {
                return Ok(Some(c.clone()));
            }
        }
        if !autoload || self.autoloaders.is_empty() {
            return Ok(None);
        }
        if self.autoloading.contains(&lname) {
            return Ok(None);
        }
        if !valid_class_name(name) {
            return Ok(None);
        }
        self.autoloading.push(lname.clone());
        // The live list: an autoloader may register or remove others
        // (test ext/spl/tests/bug71202).
        let mut result = Ok(None);
        let mut i = 0;
        while let Some(l) = self.autoloaders.get(i).cloned() {
            i += 1;
            if let Err(e) = self.call_callee(l, vec![Value::Str(PhpStr::new(name))]) {
                result = Err(e);
                break;
            }
            if let Some(c) = self.classes.get(&lname) {
                result = Ok(Some(c.clone()));
                break;
            }
        }
        self.autoloading.retain(|n| *n != lname);
        // Variance checks that waited for this class (test Zend/tests/
        // type_declarations/variance/class_order_autoload1).
        if result.is_ok() && self.delayed_obligations.iter().any(|(n, _, _)| *n == lname) {
            let all = std::mem::take(&mut self.delayed_obligations);
            let (mine, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|(n, _, _)| *n == lname);
            self.delayed_obligations = rest;
            for (_, ob, decl) in mine {
                self.check_obligations(vec![ob], &decl)?;
            }
        }
        result
    }

    /// Declares a class from its declaration (DeclareClass, early binding).
    pub fn declare_class(&mut self, decl: &Rc<ClassDecl>) -> R<Rc<Class>> {
        let was = std::mem::replace(&mut self.early_binding, false);
        let r = self.declare_class_opt(decl);
        self.early_binding = was;
        Ok(r?.expect("class deferred outside early binding"))
    }

    /// Binds a class while its file is loaded; `None` when its variance
    /// checks need classes not declared yet (it is declared when its
    /// declaration runs).
    pub fn early_bind_class(&mut self, decl: &Rc<ClassDecl>) -> R<Option<Rc<Class>>> {
        let was = std::mem::replace(&mut self.early_binding, true);
        let r = self.declare_class_opt(decl);
        self.early_binding = was;
        r
    }

    fn declare_class_opt(&mut self, decl: &Rc<ClassDecl>) -> R<Option<Rc<Class>>> {
        let lname = lower(&decl.name);
        if let Some(prev) = self.classes.get(&lname) {
            // A built-in class name (test Zend/tests/inter_06).
            if prev.has_flag(cflags::INTERNAL) {
                return Err(self.fatal_at(
                    &format!("Cannot redeclare class {}", prev.name.dname()),
                    &decl.file,
                    decl.line_start,
                ));
            }
            let what = match decl.kind {
                ClassKind::Interface => "interface",
                ClassKind::Trait => "trait",
                ClassKind::Enum => "enum",
                ClassKind::Class => "class",
            };
            return Err(self.fatal_at(
                &format!(
                    "Cannot declare {} {}, because the name is already in use",
                    what,
                    decl.name.dname()
                ),
                &decl.file,
                decl.line_start,
            ));
        }
        let Some(cls) = self.link_class(decl)? else {
            return Ok(None);
        };
        decl.declared.set(true);
        // PHP 8.1: Serializable without __serialize()/__unserialize() is
        // deprecated (message observed on PHP 8.5).
        if cls.kind != ClassKind::Interface
            && cls
                .interfaces
                .borrow()
                .iter()
                .any(|i| i.lname.as_bytes() == b"serializable")
            && !(cls.find_method(b"__serialize").is_some()
                && cls.find_method(b"__unserialize").is_some())
        {
            self.raise_at(
                super::errors::E_DEPRECATED,
                &format!(
                    "{} implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary)",
                    cls.name.dname()
                ),
                decl.file.clone(),
                decl.line_start,
            )?;
        }
        Ok(Some(cls))
    }

    /// Whether every dependency of a top-level class is already declared
    /// (PHP declares such classes before running the file).
    pub fn can_early_bind(&self, decl: &ClassDecl) -> bool {
        let known = |n: &PhpStr| {
            let l = lower(n);
            self.classes.contains_key(&l) || self.lazy_classes.contains_key(&l)
        };
        // Classes implementing interfaces or using traits, and enums, are
        // declared when their declaration runs (observed on PHP 8.5).
        decl.interfaces.is_empty()
            && decl.traits.is_empty()
            && decl.kind != ClassKind::Enum
            && decl.parent.as_ref().is_none_or(known)
            && !self.classes.contains_key(&lower(&decl.name))
    }

    fn link_err<T>(&mut self, decl: &ClassDecl, msg: &str) -> R<T> {
        Err(self.fatal_at(msg, &decl.file, decl.line_start))
    }

    fn link_class(&mut self, decl: &Rc<ClassDecl>) -> R<Option<Rc<Class>>> {
        // A class linked while another one is (autoloading) has its own
        // variance checks.
        let saved = std::mem::take(&mut self.obligations);
        // Classes declared by code run while linking (autoloaders) are
        // not bound early.
        let eb = std::mem::replace(&mut self.early_binding, false);
        let r = self.link_class_inner(decl, eb);
        self.early_binding = eb;
        self.obligations = saved;
        r
    }

    fn link_class_inner(&mut self, decl: &Rc<ClassDecl>, eb: bool) -> R<Option<Rc<Class>>> {
        let saved_early = std::mem::replace(&mut self.linking_early, eb);
        let saved_def = std::mem::replace(&mut self.link_deferred, false);
        let r = self.link_class_body(decl, eb);
        let deferred = std::mem::replace(&mut self.link_deferred, saved_def);
        self.linking_early = saved_early;
        match r {
            Ok(Some(_)) if deferred => Ok(None),
            r => r,
        }
    }

    fn link_class_body(&mut self, decl: &Rc<ClassDecl>, eb: bool) -> R<Option<Rc<Class>>> {
        let cname = decl.name.dname();
        // Parent.
        let parent = match &decl.parent {
            Some(p) => {
                let pc = match self.find_class(p, true)? {
                    Some(c) => c,
                    None => {
                        let msg = format!("Class \"{}\" not found", p.to_string_lossy());
                        return self.throw("Error", &msg);
                    }
                };
                if pc.is_interface() {
                    return self.link_err(
                        decl,
                        &format!(
                            "Class {} cannot extend interface {}",
                            cname,
                            pc.name.dname()
                        ),
                    );
                }
                if pc.is_trait() {
                    return self.link_err(
                        decl,
                        &format!("Class {} cannot extend trait {}", cname, pc.name.dname()),
                    );
                }
                if pc.is_enum() {
                    return self.link_err(
                        decl,
                        &format!("Class {} cannot extend enum {}", cname, pc.name.dname()),
                    );
                }
                if pc.has_flag(cflags::FINAL) {
                    return self.link_err(
                        decl,
                        &format!(
                            "Class {} cannot extend final class {}",
                            cname,
                            pc.name.dname()
                        ),
                    );
                }
                if pc.has_flag(cflags::READONLY) != (decl.flags & cflags::READONLY != 0) {
                    let (a, b) = if decl.flags & cflags::READONLY != 0 {
                        ("Readonly", "non-readonly")
                    } else {
                        ("Non-readonly", "readonly")
                    };
                    return self.link_err(
                        decl,
                        &format!(
                            "{} class {} cannot extend {} class {}",
                            a,
                            cname,
                            b,
                            pc.name.dname()
                        ),
                    );
                }
                Some(pc)
            }
            None => None,
        };
        // Interfaces.
        let mut ifaces: Vec<Rc<Class>> = Vec::new();
        let iface_names: Vec<PhpStr> = decl.interfaces.clone();
        let mut direct = Vec::new();
        for n in &iface_names {
            let ic = match self.find_class(n, true)? {
                Some(c) => c,
                None => {
                    let msg = format!("Interface \"{}\" not found", n.to_string_lossy());
                    return self.throw("Error", &msg);
                }
            };
            if !ic.is_interface() {
                if decl.kind == ClassKind::Interface {
                    return self.link_err(
                        decl,
                        &format!(
                            "{} cannot implement {} - it is not an interface",
                            cname,
                            ic.name.dname()
                        ),
                    );
                }
                return self.link_err(
                    decl,
                    &format!(
                        "{} cannot implement {} - it is not an interface",
                        cname,
                        ic.name.dname()
                    ),
                );
            }
            direct.push(ic.name.clone());
            ifaces.push(ic);
        }
        let id = self.next_class_id;
        self.next_class_id += 1;
        let mut flags = decl.flags;
        if decl
            .attrs
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(b"AllowDynamicProperties"))
        {
            if decl.flags & cflags::READONLY != 0 {
                return self.link_err(
                    decl,
                    &format!(
                        "Cannot apply #[\\AllowDynamicProperties] to readonly class {}",
                        cname
                    ),
                );
            }
            flags |= cflags::ALLOW_DYNAMIC;
        }
        let cls = Rc::new(Class {
            name: decl.name.clone(),
            lname: lower(&decl.name),
            kind: decl.kind,
            flags: Cell::new(flags),
            parent: parent.clone(),
            interfaces: RefCell::new(Vec::new()),
            direct_interfaces: RefCell::new(direct),
            traits: RefCell::new(Vec::new()),
            consts: RefCell::new(Vec::new()),
            const_map: RefCell::new(HashMap::new()),
            props: RefCell::new(Vec::new()),
            prop_map: RefCell::new(HashMap::new()),
            static_props: RefCell::new(Vec::new()),
            methods: RefCell::new(HashMap::new()),
            method_order: RefCell::new(Vec::new()),
            magic: RefCell::new(Magic::default()),
            defaults: OnceCell::new(),
            num_slots: Cell::new(0),
            decl: Some(decl.clone()),
            enum_backing: decl.enum_backing,
            native: match &parent {
                Some(p) => copy_hooks(&p.native),
                None => NativeHooks::default(),
            },
            attrs: decl.attrs.clone(),
            doc: decl.doc.clone(),
            id,
            has_dtor: Cell::new(false),
            enum_cases: RefCell::new(Vec::new()),
            statics_ready: Cell::new(false),
            file: Some(decl.file.clone()),
            line_start: decl.line_start,
            line_end: decl.line_end,
        });
        let cp = Rc::as_ptr(&cls);
        // Inherit from the parent.
        if let Some(p) = &parent {
            self.inherit(&cls, p);
        }
        // Enums implement UnitEnum / BackedEnum.
        if decl.kind == ClassKind::Enum {
            let ue = self.find_class(b"UnitEnum", false)?.unwrap();
            ifaces.insert(0, ue);
            if decl.enum_backing.is_some() {
                let be = self.find_class(b"BackedEnum", false)?.unwrap();
                ifaces.insert(1, be);
            }
        }
        // Interfaces (and their parents).
        for ic in &ifaces {
            self.add_interface(&cls, ic);
        }
        if decl.kind != ClassKind::Interface {
            let has = |n: &[u8]| {
                cls.interfaces
                    .borrow()
                    .iter()
                    .any(|i| i.lname.as_bytes() == n)
            };
            let is_enum = decl.kind == ClassKind::Enum;
            let what = if is_enum { "Enum" } else { "Class" };
            // tests Zend/tests/enum/gh7792_*, no-*-implements-*
            let mut seen: Vec<PhpStr> = Vec::new();
            for n in &decl.interfaces {
                let l = lower(n);
                let implied = is_enum
                    && (l.as_bytes() == b"unitenum"
                        || (l.as_bytes() == b"backedenum" && decl.enum_backing.is_some()));
                if seen.contains(&l) || implied {
                    let shown = cls
                        .interfaces
                        .borrow()
                        .iter()
                        .find(|i| i.lname == l)
                        .map(|i| i.name.dname())
                        .unwrap_or_else(|| n.dname());
                    return self.link_err(
                        decl,
                        &format!(
                            "{} {} cannot implement previously implemented interface {}",
                            what, cname, shown
                        ),
                    );
                }
                seen.push(l);
            }
            if !is_enum && !decl.file.is_empty() {
                for (l, n) in [
                    (&b"unitenum"[..], "UnitEnum"),
                    (b"backedenum", "BackedEnum"),
                ] {
                    if has(l) {
                        return self.link_err(
                            decl,
                            &format!("Non-enum class {} cannot implement interface {}", cname, n),
                        );
                    }
                }
            }
            if is_enum && decl.enum_backing.is_none() && has(b"backedenum") {
                return self.link_err(
                    decl,
                    &format!(
                        "Non-backed enum {} cannot implement interface BackedEnum",
                        cname
                    ),
                );
            }
            if is_enum && has(b"throwable") {
                return self.link_err(
                    decl,
                    &format!("Enum {} cannot implement interface Throwable", cname),
                );
            }
            if is_enum && has(b"serializable") {
                return self.link_err(
                    decl,
                    &format!("Enum {} cannot implement the Serializable interface", cname),
                );
            }
            if has(b"traversable")
                && !has(b"iterator")
                && !has(b"iteratoraggregate")
                && !decl.file.is_empty()
                && decl.kind != ClassKind::Interface
                && decl.flags & cflags::ABSTRACT == 0
                && cls
                    .parent
                    .as_ref()
                    .is_none_or(|p| !p.has_flag(cflags::INTERNAL))
            {
                // tests Zend/tests/inheritance/bug62609, enum/gh7792_5
                return Err(self.fatal_at(
                    &format!(
                        "{} {} must implement interface Traversable as part of either Iterator or IteratorAggregate",
                        what, cname
                    ),
                    &PhpStr::new(b"Unknown"),
                    0,
                ));
            }
            // test Zend/tests/throwable_003
            if has(b"throwable")
                && !decl.file.is_empty()
                && !cls.ancestors_have(|a| matches!(a.lname.as_bytes(), b"exception" | b"error"))
            {
                return self.link_err(
                    decl,
                    &format!(
                        "Class {} cannot implement interface Throwable, extend Exception or Error instead",
                        cname
                    ),
                );
            }
            if has(b"iterator") && has(b"iteratoraggregate") {
                return self.link_err(
                    decl,
                    &format!(
                        "Class {} cannot implement both Iterator and IteratorAggregate at the same time",
                        cname
                    ),
                );
            }
        }
        // Constants.
        for c in &decl.consts {
            if let Some(prev) = cls.find_const(&c.name) {
                if prev.flags & mflags::FINAL != 0 && !std::ptr::eq(prev.class, cp) {
                    let pcn = unsafe { (*prev.class).name.dname() };
                    return self.link_err(
                        decl,
                        &format!(
                            "{}::{} cannot override final constant {}::{}",
                            cname,
                            c.name.dname(),
                            pcn,
                            c.name.dname()
                        ),
                    );
                }
                let pvis = prev.flags & mflags::VISIBILITY;
                let nvis = c.flags & mflags::VISIBILITY;
                if vis_rank(nvis) > vis_rank(pvis) && prev.flags & mflags::PRIVATE == 0 {
                    let pcn = unsafe { (*prev.class).name.dname() };
                    return self.link_err(
                        decl,
                        &format!(
                            "Access level to {}::{} must be {} (as in class {}){}",
                            cname,
                            c.name.dname(),
                            vis_name(pvis),
                            pcn,
                            if pvis == mflags::PROTECTED {
                                " or weaker"
                            } else {
                                ""
                            }
                        ),
                    );
                }
            }
            let mut flags = c.flags;
            if c.attrs
                .iter()
                .any(|a| a.name.eq_ignore_ascii_case(b"Deprecated"))
            {
                flags |= mflags::DEPRECATED;
            }
            let cc = Rc::new(ClassConst {
                name: c.name.clone(),
                value: RefCell::new(match &c.value {
                    ConstExpr::Value(v) => ConstState::Value(v.clone()),
                    ce => ConstState::Pending(ce.clone()),
                }),
                flags,
                ty: c.ty.clone(),
                class: cp,
                doc: c.doc.clone(),
                attrs: c.attrs.clone(),
            });
            set_const(&cls, cc);
        }
        // Properties.
        for p in &decl.props {
            self.add_prop(&cls, decl, p)?;
        }
        // Methods.
        for m in &decl.methods {
            let f = Rc::new(Func::user(m.code.clone(), m.flags));
            f.scope.set(cp);
            self.add_method(&cls, f, decl)?;
        }
        // Traits.
        if !decl.traits.is_empty() {
            self.use_traits(&cls, decl)?;
        }
        // Enum machinery.
        if decl.kind == ClassKind::Enum {
            self.setup_enum(&cls, decl)?;
        }
        // Abstract methods left in a concrete class.
        if decl.kind == ClassKind::Class && flags & cflags::ABSTRACT == 0
            || decl.kind == ClassKind::Enum
        {
            let mut missing: Vec<String> = Vec::new();
            for ln in cls.method_order.borrow().iter() {
                let m = cls.methods.borrow().get(ln).cloned().unwrap();
                if m.flags & mflags::ABSTRACT != 0 {
                    missing.push(format!(
                        "{}::{}",
                        m.scope().map(|c| c.name.dname()).unwrap_or_default(),
                        m.name.dname()
                    ));
                }
            }
            // Abstract property hooks count as abstract methods (RFC:
            // Property hooks).
            for p in cls.props.borrow().iter() {
                for h in [&p.get, &p.set].into_iter().flatten() {
                    if h.flags & mflags::ABSTRACT != 0 {
                        missing.push(format!(
                            "{}::{}",
                            h.scope().map(|c| c.name.dname()).unwrap_or_default(),
                            h.name.dname()
                        ));
                    }
                }
            }
            if !missing.is_empty() {
                let n = missing.len();
                let what = if decl.kind == ClassKind::Enum {
                    "Enum"
                } else {
                    "Class"
                };
                let shown: Vec<String> = missing.iter().take(3).cloned().collect();
                if decl.kind == ClassKind::Enum {
                    // test Zend/tests/enum/gh7792_1
                    let msg = format!(
                        "Enum {} must implement {} abstract method{} ({}{})",
                        cname,
                        n,
                        if n == 1 { "" } else { "s" },
                        shown.join(", "),
                        if n > 3 { ", ..." } else { "" }
                    );
                    return self.link_err(decl, &msg);
                }
                let msg = format!(
                    "{} {} contains {} abstract method{} and must therefore be declared abstract or implement the remaining method{} ({}{})",
                    what,
                    cname,
                    n,
                    if n == 1 { "" } else { "s" },
                    if n == 1 { "" } else { "s" },
                    shown.join(", "),
                    if n > 3 { ", ..." } else { "" }
                );
                return self.link_err(decl, &msg);
            }
        }
        // Method order as PHP lists them: the class's own, then inherited,
        // then from traits, then from interfaces.
        {
            let mut order: Vec<PhpStr> = Vec::new();
            for m in &decl.methods {
                let l = lower(&m.code.name);
                if !order.contains(&l) {
                    order.push(l);
                }
            }
            let methods = cls.methods.borrow();
            let old = cls.method_order.borrow().clone();
            let is_iface = |n: &PhpStr| {
                methods
                    .get(n)
                    .and_then(|f| f.scope())
                    .is_some_and(|c| c.is_interface())
            };
            for n in old.iter().filter(|n| !is_iface(n)) {
                if !order.contains(n) {
                    order.push(n.clone());
                }
            }
            for n in old.iter() {
                if !order.contains(n) {
                    order.push(n.clone());
                }
            }
            drop(methods);
            *cls.method_order.borrow_mut() = order;
        }
        self.finish_class(&cls);
        // Interface methods against their implementations.
        if decl.kind != ClassKind::Interface {
            let ifs: Vec<Rc<Class>> = cls.interfaces.borrow().clone();
            for i in ifs {
                let names: Vec<PhpStr> = i.method_order.borrow().clone();
                for n in names {
                    let (Some(im), Some(m)) = (i.find_method_str(&n), cls.find_method_str(&n))
                    else {
                        continue;
                    };
                    if Rc::ptr_eq(&im, &m) || m.scope().is_some_and(|c| c.is_interface()) {
                        continue;
                    }
                    if !std::ptr::eq(im.scope.get(), Rc::as_ptr(&i)) {
                        continue;
                    }
                    self.obligations.push(super::variance::Obligation {
                        child: m,
                        parent: im,
                    });
                }
            }
        }
        let obs = std::mem::take(&mut self.obligations);
        if eb {
            self.early_binding = true;
            let ok = self.obligations_resolvable(&obs);
            self.early_binding = false;
            if !ok? || self.link_deferred {
                return Ok(None);
            }
        }
        self.register_class(cls.clone());
        self.check_obligations(obs, decl)?;
        // #[\Override] checks.
        self.check_override_attrs(&cls, decl)?;
        Ok(Some(cls))
    }

    /// #[\Override] on methods and properties: something above must
    /// declare them (RFC: Marking overridden methods; PHP 8.5 for
    /// properties; tests Zend/tests/attributes/override/*). Trait members
    /// are checked in the classes using them.
    fn check_override_attrs(&mut self, cls: &Rc<Class>, decl: &ClassDecl) -> R<()> {
        if decl.kind == ClassKind::Trait {
            return Ok(());
        }
        let cp = Rc::as_ptr(cls);
        let has_override = |attrs: &[AttrInfo]| {
            attrs.iter().any(|a| {
                a.name
                    .strip_prefix(b"\\")
                    .unwrap_or(&a.name)
                    .eq_ignore_ascii_case(b"Override")
            })
        };
        let mut methods: Vec<Rc<Func>> = cls
            .methods
            .borrow()
            .values()
            .filter(|m| std::ptr::eq(m.scope.get(), cp))
            .filter(|m| m.code().is_some_and(|c| has_override(&c.attrs)))
            .cloned()
            .collect();
        methods.sort_by_key(|m| m.code().map(|c| c.line_start).unwrap_or(0));
        for m in methods {
            let ln = m.lname.clone();
            let is_ctor = ln.as_bytes() == b"__construct";
            let mut found = false;
            if let Some(p) = &cls.parent
                && let Some(pm) = p.find_method_str(&ln)
                && pm.flags & mflags::PRIVATE == 0
                && (!is_ctor || pm.flags & mflags::ABSTRACT != 0)
            {
                found = true;
            }
            for i in cls.interfaces.borrow().iter() {
                if i.find_method_str(&ln).is_some() {
                    found = true;
                }
            }
            // An abstract method of a used trait (test override/016).
            for t in cls.traits.borrow().iter() {
                if t.find_method_str(&ln)
                    .is_some_and(|tm| tm.flags & mflags::ABSTRACT != 0)
                {
                    found = true;
                }
            }
            if !found {
                let msg = format!(
                    "{}::{}() has #[\\Override] attribute, but no matching parent method exists",
                    cls.name.dname(),
                    m.name.dname()
                );
                let line = m.code().map(|c| c.line_start).unwrap_or(decl.line_start);
                return Err(self.fatal_at(&msg, &decl.file, line));
            }
        }
        let statics: Vec<PhpStr> = cls
            .static_props
            .borrow()
            .iter()
            .filter(|p| std::ptr::eq(p.class, cp) && has_override(&p.attrs))
            .map(|p| p.name.clone())
            .collect();
        for n in statics {
            let found = cls.parent.as_ref().is_some_and(|par| {
                par.find_static(&n)
                    .is_some_and(|sp| sp.flags & mflags::PRIVATE == 0)
            });
            if !found {
                let msg = format!(
                    "{}::${} has #[\\Override] attribute, but no matching parent property exists",
                    cls.name.dname(),
                    n.dname()
                );
                return Err(self.fatal_at(&msg, &decl.file, decl.line_start));
            }
        }
        let props: Vec<Rc<PropInfo>> = cls
            .props
            .borrow()
            .iter()
            .filter(|p| std::ptr::eq(p.class, cp) && has_override(&p.attrs))
            .cloned()
            .collect();
        for p in props {
            let mut found = false;
            if let Some(par) = &cls.parent
                && let Some(idx) = par.prop_map.borrow().get(&p.name).map(|v| v[0])
                && par.props.borrow()[idx as usize].flags & mflags::PRIVATE == 0
            {
                found = true;
            }
            if let Some(par) = &cls.parent
                && par
                    .find_static(&p.name)
                    .is_some_and(|sp| sp.flags & mflags::PRIVATE == 0)
            {
                found = true;
            }
            for i in cls.interfaces.borrow().iter() {
                if i.prop_map.borrow().contains_key(&p.name) {
                    found = true;
                }
            }
            if !found {
                let msg = format!(
                    "{}::${} has #[\\Override] attribute, but no matching parent property exists",
                    cls.name.dname(),
                    p.name.dname()
                );
                return Err(self.fatal_at(&msg, &decl.file, decl.line_start));
            }
        }
        Ok(())
    }

    fn inherit(&mut self, cls: &Rc<Class>, p: &Rc<Class>) {
        // Constants (shared).
        for c in p.consts.borrow().iter() {
            set_const(cls, c.clone());
        }
        // Properties: same slots.
        *cls.props.borrow_mut() = p.props.borrow().clone();
        *cls.prop_map.borrow_mut() = p.prop_map.borrow().clone();
        cls.num_slots.set(p.num_slots.get());
        // Static properties: shared cells.
        *cls.static_props.borrow_mut() = p.static_props.borrow().clone();
        // Methods (shared).
        *cls.methods.borrow_mut() = p.methods.borrow().clone();
        *cls.method_order.borrow_mut() = p.method_order.borrow().clone();
        *cls.interfaces.borrow_mut() = p.interfaces.borrow().clone();
        // Dynamic-property behaviour.
        let pf = p.flags.get();
        let inherit_flags = pf
            & (cflags::ALLOW_DYNAMIC
                | cflags::NO_DYNAMIC
                | cflags::NOT_SERIALIZABLE
                | cflags::NOT_CLONEABLE);
        cls.flags.set(cls.flags.get() | inherit_flags);
        let _ = &p.native;
    }

    fn add_interface(&mut self, cls: &Rc<Class>, ic: &Rc<Class>) {
        let mut list = vec![ic.clone()];
        list.extend(ic.interfaces.borrow().iter().cloned());
        for i in list {
            if cls.interfaces.borrow().iter().any(|x| Rc::ptr_eq(x, &i)) {
                continue;
            }
            cls.interfaces.borrow_mut().push(i.clone());
            // Interface constants.
            for c in i.consts.borrow().iter() {
                if cls.find_const(&c.name).is_none() {
                    set_const(cls, c.clone());
                }
            }
            // Interface properties (hooks abstract until implemented).
            for ip in i.props.borrow().iter() {
                if !cls.prop_map.borrow().contains_key(&ip.name) {
                    let slot = cls.num_slots.get();
                    cls.num_slots.set(slot + 1);
                    let mut np = (**ip).clone();
                    np.slot = slot;
                    let idx = cls.props.borrow().len() as u32;
                    cls.props.borrow_mut().push(Rc::new(np));
                    cls.prop_map.borrow_mut().insert(ip.name.clone(), vec![idx]);
                }
            }
            // Interface methods (abstract until implemented).
            let ims: Vec<(PhpStr, Rc<Func>)> = i
                .method_order
                .borrow()
                .iter()
                .map(|n| (n.clone(), i.methods.borrow().get(n).cloned().unwrap()))
                .collect();
            for (n, m) in ims {
                if !cls.methods.borrow().contains_key(&n) {
                    cls.methods.borrow_mut().insert(n.clone(), m);
                    cls.method_order.borrow_mut().push(n);
                }
            }
        }
    }

    fn add_prop(&mut self, cls: &Rc<Class>, decl: &ClassDecl, p: &PropDecl) -> R<()> {
        let cp = Rc::as_ptr(cls);
        let cname = cls.name.dname();
        // Hooks are methods: abstract (`get;`) and final ones are marked.
        let hook_func = |c: &Rc<crate::bytecode::FuncCode>, abs: u32, fin: u32| {
            let mut fl = mflags::PUBLIC;
            if p.hook_flags & abs != 0 {
                fl |= mflags::ABSTRACT;
            }
            if p.hook_flags & fin != 0 {
                fl |= mflags::FINAL;
            }
            let f = Rc::new(Func::user(c.clone(), fl));
            f.scope.set(cp);
            f
        };
        // A body-less hook outside an abstract property (checked when the
        // class is declared: test Zend/tests/property_hooks/ast_printing).
        if p.hook_flags & 3 != 0
            && p.flags & mflags::ABSTRACT == 0
            && decl.kind != ClassKind::Interface
        {
            return self.link_err(decl, "Non-abstract property hook must have a body");
        }
        let get = p.get.as_ref().map(|c| hook_func(c, 1, 4));
        let set = p.set.as_ref().map(|c| hook_func(c, 2, 8));
        // A set hook's parameter accepts the property type (test
        // Zend/tests/property_hooks/set_value_parameter_type_variance_*).
        let mixed = TypeHint {
            mask: tmask::MIXED,
            classes: Vec::new(),
        };
        if let Some(sc) = &p.set
            && let Some(par) = sc.params.first()
            && let Some(at) = &par.ty
            && at.display() != p.ty.as_ref().unwrap_or(&mixed).display()
            && {
                let pt = p.ty.clone().unwrap_or(mixed.clone());
                let was = std::mem::replace(&mut self.early_binding, self.linking_early);
                let r = self.type_sub(&pt, cls, at, cls);
                self.early_binding = was;
                match r? {
                    super::variance::Sub::Yes => false,
                    super::variance::Sub::No => true,
                    super::variance::Sub::Unresolved(_) => {
                        // Bound when its declaration runs.
                        if self.linking_early {
                            self.link_deferred = true;
                            false
                        } else {
                            true
                        }
                    }
                }
            }
        {
            return self.link_err(
                decl,
                &format!(
                    "Type of parameter ${} of hook {}::${}::set must be compatible with property type",
                    par.name.dname(),
                    cname,
                    p.name.dname()
                ),
            );
        }
        if p.flags & mflags::STATIC != 0 {
            // Redeclaration of a parent static property gets its own cell.
            if let Some(prev) = cls.find_static(&p.name) {
                if prev.flags & mflags::STATIC == 0 {
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot redeclare non static {}::${} as static {}::${}",
                            unsafe { (*prev.class).name.dname() },
                            p.name.dname(),
                            cname,
                            p.name.dname()
                        ),
                    );
                }
                cls.static_props.borrow_mut().retain(|s| s.name != p.name);
            }
            if cls.prop_map.borrow().contains_key(&p.name) {
                let props = cls.props.borrow();
                let idx = cls.prop_map.borrow()[&p.name][0];
                let prev = &props[idx as usize];
                if prev.flags & mflags::PRIVATE == 0 {
                    let pcn = unsafe { (*prev.class).name.dname() };
                    drop(props);
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot redeclare non static {}::${} as static {}::${}",
                            pcn,
                            p.name.dname(),
                            cname,
                            p.name.dname()
                        ),
                    );
                }
            }
            let init = match &p.default {
                Some(ConstExpr::Value(v)) => {
                    let mut v = v.clone();
                    if let (Some(t), Value::Int(i)) = (&p.ty, &v)
                        && t.mask & tmask::FLOAT != 0
                        && t.mask & tmask::INT == 0
                    {
                        v = Value::Float(*i as f64);
                    }
                    (v, None)
                }
                Some(ce) => (Value::Null, Some(ce.clone())),
                None => (
                    if p.ty.is_some() {
                        self.uninit_value()
                    } else {
                        Value::Null
                    },
                    None,
                ),
            };
            cls.static_props.borrow_mut().push(Rc::new(StaticProp {
                name: p.name.clone(),
                flags: p.flags,
                ty: p.ty.clone(),
                class: cp,
                cell: RefCell::new(PhpRef::new(init.0)),
                init: RefCell::new(init.1),
                doc: p.doc.clone(),
                attrs: p.attrs.clone(),
            }));
            return Ok(());
        }
        let existing = cls.prop_map.borrow().get(&p.name).cloned();
        let info = |slot: u32| PropInfo {
            name: p.name.clone(),
            slot,
            flags: p.flags,
            ty: p.ty.clone(),
            class: cp,
            get: get.clone(),
            set: set.clone(),
            doc: p.doc.clone(),
            attrs: p.attrs.clone(),
            default: p.default.clone(),
        };
        if let Some(idxs) = existing {
            let props_b = cls.props.borrow();
            let prev = props_b[idxs[0] as usize].clone();
            drop(props_b);
            if cls.find_static(&p.name).is_some() {
                return self.link_err(
                    decl,
                    &format!(
                        "Cannot redeclare static {}::${} as non static {}::${}",
                        unsafe { (*prev.class).name.dname() },
                        p.name.dname(),
                        cname,
                        p.name.dname()
                    ),
                );
            }
            if prev.flags & mflags::PRIVATE == 0 && !std::ptr::eq(prev.class, cp) {
                // Redeclaration: same slot. Visibility can't be reduced.
                let pv = prev.flags & mflags::VISIBILITY;
                let nv = p.flags & mflags::VISIBILITY;
                if vis_rank(nv) > vis_rank(pv) {
                    return self.link_err(
                        decl,
                        &format!(
                            "Access level to {}::${} must be {} (as in class {}){}",
                            cname,
                            p.name.dname(),
                            vis_name(pv),
                            unsafe { (*prev.class).name.dname() },
                            if pv == mflags::PROTECTED {
                                " or weaker"
                            } else {
                                ""
                            }
                        ),
                    );
                }
                let prev_hooked = prev.get.is_some() || prev.set.is_some();
                if prev.flags & mflags::READONLY != p.flags & mflags::READONLY && !prev_hooked {
                    let pcn = unsafe { (*prev.class).name.dname() };
                    let (a, b) = if prev.flags & mflags::READONLY != 0 {
                        ("readonly", "non-readonly")
                    } else {
                        ("non-readonly", "readonly")
                    };
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot redeclare {} property {}::${} as {} {}::${}",
                            a,
                            pcn,
                            p.name.dname(),
                            b,
                            cname,
                            p.name.dname()
                        ),
                    );
                }
                if prev.flags & mflags::FINAL != 0 {
                    let pcn = unsafe { (*prev.class).name.dname() };
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot override final property {}::${}",
                            pcn,
                            p.name.dname()
                        ),
                    );
                }
                self.prop_type_check(cls, decl, &cname, &prev, p, set.as_ref())?;
                self.prop_set_vis_check(decl, &cname, &prev, p)?;
                for (mine, theirs, what) in [(&p.get, &prev.get, "get"), (&p.set, &prev.set, "set")]
                {
                    if mine.is_some()
                        && let Some(t) = theirs
                        && t.flags & mflags::FINAL != 0
                    {
                        let pcn = unsafe { (*prev.class).name.dname() };
                        return self.link_err(
                            decl,
                            &format!(
                                "Cannot override final property hook {}::${}::{}()",
                                pcn,
                                p.name.dname(),
                                what
                            ),
                        );
                    }
                }
                let mut ni = info(prev.slot);
                // A property backed in the parent stays backed.
                if prev.flags & mflags::VIRTUAL == 0 {
                    ni.flags &= !mflags::VIRTUAL;
                }
                let backed = ni.flags & mflags::VIRTUAL == 0;
                if p.get.is_none() && ni.get.is_none() {
                    // A plain (backed) property implements an abstract get.
                    ni.get = prev
                        .get
                        .clone()
                        .filter(|g| !(backed && g.flags & mflags::ABSTRACT != 0));
                }
                if p.set.is_none() && ni.set.is_none() {
                    // ... and an abstract set, unless it is readonly.
                    ni.set = prev.set.clone().filter(|s| {
                        !(backed
                            && s.flags & mflags::ABSTRACT != 0
                            && ni.flags & mflags::READONLY == 0)
                    });
                }
                self.prop_backing_checks(decl, &cname, &ni, p)?;
                cls.props.borrow_mut()[idxs[0] as usize] = Rc::new(ni);
                return Ok(());
            }
            if std::ptr::eq(prev.class, cp) {
                return self.link_err(
                    decl,
                    &format!("Cannot redeclare {}::${}", cname, p.name.dname()),
                );
            }
            // Shadows a parent's private property: new slot.
            let slot = cls.num_slots.get();
            cls.num_slots.set(slot + 1);
            let idx = cls.props.borrow().len() as u32;
            cls.props.borrow_mut().push(Rc::new(info(slot)));
            cls.prop_map
                .borrow_mut()
                .get_mut(&p.name)
                .unwrap()
                .insert(0, idx);
            return Ok(());
        }
        let slot = cls.num_slots.get();
        cls.num_slots.set(slot + 1);
        let idx = cls.props.borrow().len() as u32;
        let ni = info(slot);
        self.prop_backing_checks(decl, &cname, &ni, p)?;
        cls.props.borrow_mut().push(Rc::new(ni));
        cls.prop_map.borrow_mut().insert(p.name.clone(), vec![idx]);
        Ok(())
    }

    /// Rules on a property once it is known to be virtual or backed
    /// (tests Zend/tests/property_hooks/default_on_virtual*,
    /// get_by_ref_backed).
    fn prop_backing_checks(
        &mut self,
        decl: &ClassDecl,
        cname: &str,
        ni: &PropInfo,
        p: &PropDecl,
    ) -> R<()> {
        if ni.flags & mflags::VIRTUAL != 0 {
            if p.default.is_some() {
                return self.link_err(
                    decl,
                    &format!(
                        "Cannot specify default value for virtual hooked property {}::${}",
                        cname,
                        p.name.dname()
                    ),
                );
            }
        } else if let Some(g) = &ni.get
            && ni.set.is_some()
            && g.code()
                .is_some_and(|c| c.flags & crate::bytecode::fflags::BY_REF != 0)
        {
            return self.link_err(
                decl,
                &format!(
                    "Get hook of backed property {}::{} with set hook may not return by reference",
                    cname,
                    p.name.dname()
                ),
            );
        }
        Ok(())
    }

    /// The type of a redeclared property: invariant, or covariant for a
    /// parent virtual property with only a get hook and contravariant with
    /// only a set hook (RFC: Property hooks, "Inheritance"; tests
    /// Zend/tests/property_hooks/type_compatibility*,
    /// Zend/tests/type_declarations/variance/property_types_early_bind).
    fn prop_type_check(
        &mut self,
        cls: &Rc<Class>,
        decl: &ClassDecl,
        cname: &str,
        prev: &PropInfo,
        p: &PropDecl,
        set: Option<&Rc<Func>>,
    ) -> R<()> {
        use super::variance::Sub;
        let pcls = unsafe { &*prev.class };
        let pcn = pcls.name.dname();
        let mixed = TypeHint {
            mask: tmask::MIXED,
            classes: Vec::new(),
        };
        let pt = prev.ty.clone().unwrap_or_else(|| mixed.clone());
        let ct = p.ty.clone().unwrap_or_else(|| mixed.clone());
        let virt = prev.flags & mflags::VIRTUAL != 0;
        let get_only = virt && prev.get.is_some() && prev.set.is_none();
        let set_only = virt && prev.set.is_some() && prev.get.is_none();
        // Early binding: an unknown class defers the class to run time.
        let sub = |vm: &mut Vm, a: &TypeHint, ac: &Class, b: &TypeHint, bc: &Class| -> R<bool> {
            let was = std::mem::replace(&mut vm.early_binding, vm.linking_early);
            let r = vm.type_sub(a, ac, b, bc);
            vm.early_binding = was;
            Ok(match r? {
                Sub::Yes => true,
                Sub::No => false,
                Sub::Unresolved(_) => {
                    if vm.linking_early {
                        vm.link_deferred = true;
                        true
                    } else {
                        false
                    }
                }
            })
        };
        let hook_sig = |c: &str, t: &TypeHint, get: bool| -> String {
            if get {
                format!("{}::${}::get(): {}", c, p.name.dname(), t.display())
            } else {
                format!(
                    "{}::${}::set({} $value): void",
                    c,
                    p.name.dname(),
                    t.display()
                )
            }
        };
        if get_only {
            if prev.ty.is_some() && !sub(self, &ct, cls, &pt, pcls)? {
                let msg = format!(
                    "Declaration of {} must be compatible with {}",
                    hook_sig(cname, &ct, true),
                    hook_sig(&pcn, &pt, true)
                );
                return self.link_err(decl, &msg);
            }
        } else if set_only {
            if p.ty.is_some() && !sub(self, &pt, pcls, &ct, cls)? {
                let msg = format!(
                    "Declaration of {} must be compatible with {}",
                    hook_sig(cname, &ct, false),
                    hook_sig(&pcn, &pt, false)
                );
                return self.link_err(decl, &msg);
            }
        } else {
            match (&prev.ty, &p.ty) {
                (Some(a), Some(b)) => {
                    let same = a.display() == b.display()
                        || (a.mask & tmask::MIXED != 0 && b.mask & tmask::MIXED != 0)
                        || (sub(self, a, pcls, b, cls)? && sub(self, b, cls, a, pcls)?);
                    if !same {
                        return self.link_err(
                            decl,
                            &format!(
                                "Type of {}::${} must be {} (as in class {})",
                                cname,
                                p.name.dname(),
                                a.display(),
                                pcn
                            ),
                        );
                    }
                }
                (Some(a), None) => {
                    return self.link_err(
                        decl,
                        &format!(
                            "Type of {}::${} must be {} (as in class {})",
                            cname,
                            p.name.dname(),
                            a.display(),
                            pcn
                        ),
                    );
                }
                (None, Some(_)) => {
                    return self.link_err(
                        decl,
                        &format!(
                            "Type of {}::${} must not be defined (as in class {})",
                            cname,
                            p.name.dname(),
                            pcn
                        ),
                    );
                }
                _ => {}
            }
        }
        // A parent set hook with a wider parameter type: the property
        // must accept it (test Zend/tests/property_hooks/gh15140).
        if let Some(ps) = &prev.set
            && let Some(pty) = ps
                .code()
                .and_then(|c| c.params.first())
                .and_then(|x| x.ty.clone())
        {
            let cty = set
                .and_then(|f| f.code())
                .and_then(|c| c.params.first())
                .and_then(|x| x.ty.clone())
                .unwrap_or(ct.clone());
            let shown = pty.display();
            if p.ty.is_some() && !sub(self, &pty, pcls, &cty, cls)? {
                let what = if pcls.is_interface() {
                    "interface"
                } else {
                    "class"
                };
                return self.link_err(
                    decl,
                    &format!(
                        "Set type of {}::${} must be supertype of {} (as in {} {})",
                        cname,
                        p.name.dname(),
                        shown,
                        what,
                        pcn
                    ),
                );
            }
        }
        Ok(())
    }

    /// A redeclared property can't restrict its set visibility (RFC:
    /// Asymmetric Visibility v2; readonly is implicitly protected(set)).
    fn prop_set_vis_check(
        &mut self,
        decl: &ClassDecl,
        cname: &str,
        prev: &PropInfo,
        p: &PropDecl,
    ) -> R<()> {
        // A virtual parent property with no set hook asks for no writes.
        if prev.flags & mflags::VIRTUAL != 0 && prev.set.is_none() {
            return Ok(());
        }
        let rank = |f: u32| -> (u8, bool) {
            if f & mflags::PRIVATE_SET != 0 {
                (2, true)
            } else if f & mflags::PROTECTED_SET != 0 {
                (1, true)
            } else if f & mflags::PUBLIC_SET != 0 {
                (0, true)
            } else if f & mflags::READONLY != 0 && f & mflags::PUBLIC != 0 {
                (1, true)
            } else if f & mflags::PRIVATE != 0 {
                (2, false)
            } else if f & mflags::PROTECTED != 0 {
                (1, false)
            } else {
                (0, false)
            }
        };
        let (pr, pexplicit) = rank(prev.flags);
        let (nr, _) = rank(p.flags);
        if nr > pr {
            let pcn = unsafe { (*prev.class).name.dname() };
            let msg = if pexplicit {
                format!(
                    "Set access level of {}::${} must be {}(set) (as in class {}) or weaker",
                    cname,
                    p.name.dname(),
                    ["public", "protected", "private"][pr as usize],
                    pcn
                )
            } else {
                format!(
                    "Set access level of {}::${} must be omitted (as in class {})",
                    cname,
                    p.name.dname(),
                    pcn
                )
            };
            return self.link_err(decl, &msg);
        }
        Ok(())
    }

    /// Adds (or overrides with) method `f`, checking inheritance rules.
    fn add_method(&mut self, cls: &Rc<Class>, f: Rc<Func>, decl: &ClassDecl) -> R<()> {
        let ln = f.lname.clone();
        let cname = cls.name.dname();
        let prev = cls.methods.borrow().get(&ln).cloned();
        if let Some(prev) = prev {
            let pscope = prev.scope.get();
            if std::ptr::eq(pscope, Rc::as_ptr(cls)) {
                // Same class (trait method vs own method): own wins.
                return Ok(());
            }
            let pcn = prev.scope().map(|c| c.name.dname()).unwrap_or_default();
            let is_ctor = ln.as_bytes() == b"__construct";
            if prev.flags & mflags::PRIVATE == 0 || prev.flags & mflags::ABSTRACT != 0 {
                if prev.flags & mflags::FINAL != 0 {
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot override final method {}::{}()",
                            pcn,
                            prev.name.dname()
                        ),
                    );
                }
                if prev.flags & mflags::STATIC != 0 && f.flags & mflags::STATIC == 0 {
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot make static method {}::{}() non static in class {}",
                            pcn,
                            prev.name.dname(),
                            cname
                        ),
                    );
                }
                if prev.flags & mflags::STATIC == 0 && f.flags & mflags::STATIC != 0 {
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot make non static method {}::{}() static in class {}",
                            pcn,
                            prev.name.dname(),
                            cname
                        ),
                    );
                }
                if prev.flags & mflags::ABSTRACT == 0 && f.flags & mflags::ABSTRACT != 0 {
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot make non abstract method {}::{}() abstract in class {}",
                            pcn,
                            prev.name.dname(),
                            cname
                        ),
                    );
                }
                let pv = prev.flags & mflags::VISIBILITY;
                let nv = f.flags & mflags::VISIBILITY;
                if vis_rank(nv) > vis_rank(pv) && !(is_ctor && prev.flags & mflags::ABSTRACT == 0) {
                    return self.link_err(
                        decl,
                        &format!(
                            "Access level to {}::{}() must be {} (as in class {}){}",
                            cname,
                            f.name.dname(),
                            vis_name(pv),
                            pcn,
                            if pv == mflags::PROTECTED {
                                " or weaker"
                            } else {
                                ""
                            }
                        ),
                    );
                }
                if !is_ctor
                    || prev.flags & mflags::ABSTRACT != 0
                    || prev.scope().is_some_and(|c| c.is_interface())
                {
                    self.check_signature(cls, &f, &prev, decl)?;
                    self.obligations.push(super::variance::Obligation {
                        child: f.clone(),
                        parent: prev.clone(),
                    });
                    *f.proto.borrow_mut() = Some(prev.clone());
                } else if let Some(pr) = prev.proto.borrow().clone()
                    && (pr.flags & mflags::ABSTRACT != 0
                        || pr.scope().is_some_and(|c| c.is_interface()))
                {
                    // A constructor still answers to an abstract or
                    // interface one further up (test Zend/tests/
                    // type_declarations/variance/abstract_constructor).
                    self.check_signature(cls, &f, &pr, decl)?;
                    self.obligations.push(super::variance::Obligation {
                        child: f.clone(),
                        parent: pr.clone(),
                    });
                    *f.proto.borrow_mut() = Some(pr);
                } else {
                    *f.proto.borrow_mut() = Some(prev.clone());
                }
            }
            cls.methods.borrow_mut().insert(ln, f);
        } else {
            cls.methods.borrow_mut().insert(ln.clone(), f);
            cls.method_order.borrow_mut().push(ln);
        }
        Ok(())
    }

    /// LSP check of a method against the one it overrides (parameter
    /// count and by-reference-ness; types compared textually).
    fn check_signature(
        &mut self,
        cls: &Rc<Class>,
        f: &Rc<Func>,
        prev: &Rc<Func>,
        decl: &ClassDecl,
    ) -> R<()> {
        let (Some(fc), Some(pc)) = (f.code().cloned(), prev.code().cloned()) else {
            let pp = self.params_of(prev).to_vec();
            let fp = self.params_of(f).to_vec();
            return self.check_params_compat(cls, f, prev, &fp, &pp, decl);
        };
        self.check_params_compat(cls, f, prev, &fc.params, &pc.params, decl)
    }

    fn check_params_compat(
        &mut self,
        cls: &Rc<Class>,
        f: &Rc<Func>,
        prev: &Rc<Func>,
        fp: &[ParamInfo],
        pp: &[ParamInfo],
        decl: &ClassDecl,
    ) -> R<()> {
        let f_req = fp
            .iter()
            .take_while(|p| matches!(p.default, ParamDefault::None) && !p.variadic)
            .count();
        let p_req = pp
            .iter()
            .take_while(|p| matches!(p.default, ParamDefault::None) && !p.variadic)
            .count();
        let f_var = fp.last().is_some_and(|p| p.variadic);
        let p_var = pp.last().is_some_and(|p| p.variadic);
        let mut bad = f_req > p_req || (fp.len() < pp.len() && !f_var) || (p_var && !f_var);
        for (i, p) in pp.iter().enumerate() {
            if let Some(q) = fp.get(i)
                && q.by_ref != p.by_ref
            {
                bad = true;
            }
        }
        if !bad {
            return Ok(());
        }
        let _ = (fp, pp);
        let fs = self.variance_sig(f);
        let ps = self.variance_sig(prev);
        let _ = cls;
        let msg = format!("Declaration of {} must be compatible with {}", fs, ps);
        // At the method's line (test Zend/tests/inheritance/
        // constructor_abstract_grantparent).
        if let Some(c) = f.code()
            && *c.file == *decl.file
        {
            return Err(self.fatal_at(&msg, &decl.file, c.line_start));
        }
        self.link_err(decl, &msg)
    }

    pub fn signature_string(&mut self, f: &Func, params: &[ParamInfo]) -> String {
        let mut s = format!("{}(", f.display_name());
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            if let Some(t) = &p.ty {
                s.push_str(&t.display());
                s.push(' ');
            }
            if p.by_ref {
                s.push('&');
            }
            if p.variadic {
                s.push_str("...");
            }
            s.push('$');
            s.push_str(&p.name.dname());
            if let Some(d) = &p.default_text {
                s.push_str(" = ");
                s.push_str(d);
            }
        }
        s.push(')');
        let ret = match &f.kind {
            FuncKind::User(c) => c.ret_type.as_ref().map(|t| t.display()),
            FuncKind::Native { .. } => self.native_sig(f).ret.as_ref().map(|t| t.display()),
        };
        if let Some(r) = ret {
            s.push_str(": ");
            s.push_str(&r);
        }
        s
    }

    fn use_traits(&mut self, cls: &Rc<Class>, decl: &ClassDecl) -> R<()> {
        let cp = Rc::as_ptr(cls);
        let cname = cls.name.dname();
        let mut traits: Vec<Rc<Class>> = Vec::new();
        let mut rules: Vec<TraitRule> = Vec::new();
        for tu in &decl.traits {
            for n in &tu.traits {
                let t = match self.find_class(n, true)? {
                    Some(t) => t,
                    None => {
                        let msg = format!("Trait \"{}\" not found", n.to_string_lossy());
                        return self.throw("Error", &msg);
                    }
                };
                if !t.is_trait() {
                    // An Error (test Zend/tests/traits/error_003).
                    let msg = format!(
                        "{} cannot use {} - it is not a trait",
                        cname,
                        t.name.dname()
                    );
                    return self.throw("Error", &msg);
                }
                if !traits.iter().any(|x| Rc::ptr_eq(x, &t)) {
                    traits.push(t);
                }
            }
            rules.extend(tu.rules.iter().cloned());
        }
        *cls.traits.borrow_mut() = traits.clone();
        // PHP 8.5: #[\Deprecated] traits (RFC: #[\Deprecated] for traits).
        for t in &traits {
            if let Some((since, msg)) = self.deprecation_detail(&t.attrs, Rc::as_ptr(t), false)? {
                let text = super::include::deprecation_text(
                    &format!("Trait {} used by {}", t.name.dname(), cname),
                    since,
                    msg,
                );
                self.raise(super::errors::E_USER_DEPRECATED, &text)?;
            }
        }
        // The rules' traits and methods (tests Zend/tests/traits/bug60165*,
        // bug54441, bug61052, bug62069*, bug63219, bug64235*, error_010).
        let mut excluded: Vec<(PhpStr, PhpStr)> = Vec::new();
        for r in &rules {
            match r {
                TraitRule::Insteadof {
                    trait_,
                    method,
                    instead,
                } => {
                    let t = self.rule_trait(decl, &cname, &traits, trait_)?;
                    let lm = lower(method);
                    if t.find_method_str(&lm).is_none() {
                        return self.link_err(
                            decl,
                            &format!(
                                "A precedence rule was defined for {}::{} but this method does not exist",
                                t.name.dname(),
                                method.dname()
                            ),
                        );
                    }
                    for i in instead {
                        let ti = self.rule_trait(decl, &cname, &traits, i)?;
                        if Rc::ptr_eq(&ti, &t) {
                            return self.link_err(
                                decl,
                                &format!(
                                    "Inconsistent insteadof definition. The method {} is to be used from {}, but {} is also on the exclude list",
                                    method.dname(),
                                    t.name.dname(),
                                    t.name.dname()
                                ),
                            );
                        }
                        if excluded.iter().any(|(tn, mn)| *tn == ti.lname && *mn == lm) {
                            return self.link_err(
                                decl,
                                &format!(
                                    "Failed to evaluate a trait precedence ({}). Method of trait {} was defined to be excluded multiple times",
                                    method.dname(),
                                    ti.name.dname()
                                ),
                            );
                        }
                        excluded.push((ti.lname.clone(), lm.clone()));
                    }
                }
                TraitRule::Alias {
                    trait_,
                    method,
                    alias,
                    ..
                } => {
                    let lm = lower(method);
                    match trait_ {
                        Some(tn) => {
                            let t = self.rule_trait(decl, &cname, &traits, tn)?;
                            if t.find_method_str(&lm).is_none() {
                                return self.link_err(
                                    decl,
                                    &format!(
                                        "An alias was defined for {}::{} but this method does not exist",
                                        t.name.dname(),
                                        method.dname()
                                    ),
                                );
                            }
                        }
                        None => {
                            let having: Vec<&Rc<Class>> = traits
                                .iter()
                                .filter(|t| t.find_method_str(&lm).is_some())
                                .collect();
                            if having.is_empty() {
                                let msg = match alias {
                                    Some(a) => format!(
                                        "An alias ({}) was defined for method {}(), but this method does not exist",
                                        a.dname(),
                                        method.dname()
                                    ),
                                    None => format!(
                                        "The modifiers of the trait method {}() are changed, but this method does not exist. Error",
                                        method.dname()
                                    ),
                                };
                                return self.link_err(decl, &msg);
                            }
                            if having.len() > 1 {
                                return self.link_err(
                                    decl,
                                    &format!(
                                        "An alias was defined for method {}(), which exists in both {} and {}. Use {}::{} or {}::{} to resolve the ambiguity",
                                        method.dname(),
                                        having[0].name.dname(),
                                        having[1].name.dname(),
                                        having[0].name.dname(),
                                        method.dname(),
                                        having[1].name.dname(),
                                        method.dname()
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
        // Collect methods.
        // (target name, method, trait, original name)
        let mut imported: Vec<(PhpStr, Rc<Func>, Rc<Class>, PhpStr)> = Vec::new();
        for t in &traits {
            let order = t.method_order.borrow().clone();
            for ln in order {
                let m = t.methods.borrow().get(&ln).cloned().unwrap();
                // Aliases.
                for r in &rules {
                    if let TraitRule::Alias {
                        trait_,
                        method,
                        flags,
                        alias: Some(alias),
                    } = r
                        && lower(method) == ln
                        && trait_.as_ref().is_none_or(|tn| lower(tn) == t.lname)
                    {
                        let nf = self.trait_method_copy(&m, cp, t, Some(alias.clone()), *flags);
                        imported.push((lower(alias), nf, t.clone(), m.name.clone()));
                    }
                }
                if excluded.iter().any(|(tn, mn)| *tn == t.lname && *mn == ln) {
                    continue;
                }
                let mut vis = 0;
                for r in &rules {
                    if let TraitRule::Alias {
                        trait_,
                        method,
                        flags,
                        alias: None,
                    } = r
                        && lower(method) == ln
                        && trait_.as_ref().is_none_or(|tn| lower(tn) == t.lname)
                    {
                        vis = *flags;
                    }
                }
                let nf = self.trait_method_copy(&m, cp, t, None, vis);
                // The same method reached through several traits (test
                // Zend/tests/traits/bug63911).
                let same = imported.iter().any(|(n, f, _, _)| {
                    *n == ln
                        && match (f.code(), nf.code()) {
                            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                            _ => false,
                        }
                });
                if same {
                    continue;
                }
                imported.push((ln.clone(), nf, t.clone(), m.name.clone()));
            }
        }
        // Conflicts between traits.
        for i in 0..imported.len() {
            for j in i + 1..imported.len() {
                if imported[i].0 == imported[j].0 && !Rc::ptr_eq(&imported[i].2, &imported[j].2) {
                    let own = decl
                        .methods
                        .iter()
                        .any(|m| lower(&m.code.name) == imported[i].0);
                    let a_abs = imported[i].1.flags & mflags::ABSTRACT != 0;
                    let b_abs = imported[j].1.flags & mflags::ABSTRACT != 0;
                    if !own && !a_abs && !b_abs {
                        return self.link_err(decl, &format!(
                            "Trait method {}::{} has not been applied as {}::{}, because of collision with {}::{}",
                            imported[j].2.name.dname(),
                            imported[j].3.dname(),
                            cname,
                            imported[j].1.name.dname(),
                            imported[i].2.name.dname(),
                            imported[i].1.name.dname()
                        ));
                    }
                }
            }
        }
        for (ln, f, _, _) in imported {
            let own = decl.methods.iter().any(|m| lower(&m.code.name) == ln);
            let cur = cls.methods.borrow().get(&ln).cloned();
            // An abstract trait method checks the method implementing it
            // (tests Zend/tests/traits/abstract_method_*, gh14009_002).
            if f.flags & mflags::ABSTRACT != 0
                && let Some(m) = &cur
                && m.flags & mflags::ABSTRACT == 0
            {
                let tn = unsafe { (*f.from_trait.get()).name.dname() };
                if (f.flags ^ m.flags) & mflags::STATIC != 0 {
                    let (a, b) = if f.flags & mflags::STATIC != 0 {
                        ("static", "non static")
                    } else {
                        ("non static", "static")
                    };
                    return self.link_err(
                        decl,
                        &format!(
                            "Cannot make {} method {}::{}() {} in class {}",
                            a,
                            tn,
                            f.name.dname(),
                            b,
                            cname
                        ),
                    );
                }
                self.check_signature(cls, m, &f, decl)?;
                self.obligations.push(super::variance::Obligation {
                    child: m.clone(),
                    parent: f.clone(),
                });
                continue;
            }
            if own {
                continue;
            }
            let prev = cls.methods.borrow().get(&ln).cloned();
            match prev {
                Some(p) if std::ptr::eq(p.scope.get(), cp) => {
                    if p.flags & mflags::ABSTRACT != 0 {
                        cls.methods.borrow_mut().insert(ln, f);
                    }
                }
                _ => self.add_method(cls, f, decl)?,
            }
        }
        // Properties and constants of traits.
        let mut from_trait: HashMap<PhpStr, PhpStr> = HashMap::new();
        let mut const_from_trait: HashMap<PhpStr, PhpStr> = HashMap::new();
        for t in &traits {
            let tprops: Vec<Rc<PropInfo>> = t.props.borrow().clone();
            for p in tprops {
                let rebind = |f: &Option<Rc<Func>>| {
                    f.as_ref().map(|f| {
                        let n = Rc::new(Func::user(f.code().unwrap().clone(), f.flags));
                        n.scope.set(cp);
                        n
                    })
                };
                let existing = cls.prop_map.borrow().get(&p.name).map(|v| v[0]);
                if let Some(i0) = existing
                    && !std::ptr::eq(cls.props.borrow()[i0 as usize].class, cp)
                {
                    // Overrides an inherited (parent or interface) property
                    // as a declaration of the class would.
                    let prev = cls.props.borrow()[i0 as usize].clone();
                    let mut flags = p.flags;
                    if prev.flags & mflags::VIRTUAL == 0 {
                        flags &= !mflags::VIRTUAL;
                    }
                    let backed = flags & mflags::VIRTUAL == 0;
                    let get = rebind(&p.get).or_else(|| {
                        prev.get
                            .clone()
                            .filter(|g| !(backed && g.flags & mflags::ABSTRACT != 0))
                    });
                    let set = rebind(&p.set).or_else(|| {
                        prev.set.clone().filter(|x| {
                            !(backed
                                && x.flags & mflags::ABSTRACT != 0
                                && flags & mflags::READONLY == 0)
                        })
                    });
                    cls.props.borrow_mut()[i0 as usize] = Rc::new(PropInfo {
                        name: p.name.clone(),
                        slot: prev.slot,
                        flags,
                        ty: p.ty.clone(),
                        class: cp,
                        get,
                        set,
                        doc: p.doc.clone(),
                        attrs: p.attrs.clone(),
                        default: p.default.clone(),
                    });
                    from_trait.insert(p.name.clone(), t.name.clone());
                    continue;
                }
                if let Some(idxs) = cls.prop_map.borrow().get(&p.name) {
                    let prev = cls.props.borrow()[idxs[0] as usize].clone();
                    let first = from_trait
                        .get(&p.name)
                        .map(|n| n.dname())
                        .unwrap_or_else(|| unsafe { (*prev.class).name.dname() });
                    if prev.get.is_some()
                        || prev.set.is_some()
                        || p.get.is_some()
                        || p.set.is_some()
                    {
                        return self.link_err(decl, &format!(
                            "{} and {} define the same hooked property (${}) in the composition of {}. Conflict resolution between hooked properties is currently not supported. Class was composed",
                            first,
                            t.name.dname(),
                            p.name.dname(),
                            cname
                        ));
                    }
                    let same_default = match (&prev.default, &p.default) {
                        (Some(ConstExpr::Value(a)), Some(ConstExpr::Value(b))) => {
                            goro_runtime::compare::strict_eq(a, b)
                        }
                        (None, None) => true,
                        // The same expression (tests Zend/tests/traits/
                        // bug74922*).
                        (Some(a), Some(b)) => const_code_same(a, b),
                        _ => false,
                    };
                    if prev.flags & (mflags::VISIBILITY | mflags::READONLY | mflags::STATIC)
                        != p.flags & (mflags::VISIBILITY | mflags::READONLY | mflags::STATIC)
                        || !same_default
                        || prev.ty.as_ref().map(|t| t.display())
                            != p.ty.as_ref().map(|t| t.display())
                    {
                        return self.link_err(decl, &format!(
                            "{} and {} define the same property (${}) in the composition of {}. However, the definition differs and is considered incompatible. Class was composed",
                            first,
                            t.name.dname(),
                            p.name.dname(),
                            cname
                        ));
                    }
                    continue;
                }
                let slot = cls.num_slots.get();
                cls.num_slots.set(slot + 1);
                let idx = cls.props.borrow().len() as u32;
                from_trait.insert(p.name.clone(), t.name.clone());
                cls.props.borrow_mut().push(Rc::new(PropInfo {
                    name: p.name.clone(),
                    slot,
                    flags: p.flags,
                    ty: p.ty.clone(),
                    class: cp,
                    get: rebind(&p.get),
                    set: rebind(&p.set),
                    doc: p.doc.clone(),
                    attrs: p.attrs.clone(),
                    default: p.default.clone(),
                }));
                cls.prop_map.borrow_mut().insert(p.name.clone(), vec![idx]);
            }
            let tstatics: Vec<Rc<StaticProp>> = t.static_props.borrow().clone();
            for sp in tstatics {
                // A static trait property against an instance one of the
                // class (test Zend/tests/traits/bug60369).
                let inst = cls
                    .prop_map
                    .borrow()
                    .get(&sp.name)
                    .map(|v| cls.props.borrow()[v[0] as usize].clone());
                if inst.is_some_and(|p| std::ptr::eq(p.class, cp)) {
                    return self.link_err(decl, &format!(
                        "{} and {} define the same property (${}) in the composition of {}. However, the definition differs and is considered incompatible. Class was composed",
                        cname,
                        t.name.dname(),
                        sp.name.dname(),
                        cname
                    ));
                }
                if cls
                    .find_static(&sp.name)
                    .is_some_and(|x| std::ptr::eq(x.class, cp))
                {
                    continue;
                }
                cls.static_props.borrow_mut().retain(|s| s.name != sp.name);
                let init_now = sp.init.borrow().clone();
                let v = if init_now.is_some() {
                    Value::Null
                } else {
                    sp.cell.borrow().get()
                };
                cls.static_props.borrow_mut().push(Rc::new(StaticProp {
                    name: sp.name.clone(),
                    flags: sp.flags,
                    ty: sp.ty.clone(),
                    class: cp,
                    cell: RefCell::new(PhpRef::new(v)),
                    init: RefCell::new(init_now),
                    doc: sp.doc.clone(),
                    attrs: sp.attrs.clone(),
                }));
            }
            let tconsts: Vec<Rc<ClassConst>> = t.consts.borrow().clone();
            for c in tconsts {
                // Conflicts with the class's own or another trait's
                // constant; inherited ones are overridden (tests
                // Zend/tests/traits/constant_0*).
                if let Some(prev) = cls.find_const(&c.name) {
                    let own_or_trait = std::ptr::eq(prev.class, cp);
                    if own_or_trait {
                        let as_ce = |s: &ConstState| match s {
                            ConstState::Value(v) => Some(ConstExpr::Value(v.clone())),
                            ConstState::Pending(e) => Some(e.clone()),
                            ConstState::Evaluating => None,
                        };
                        let (pa, pb) = (as_ce(&prev.value.borrow()), as_ce(&c.value.borrow()));
                        let same_val = match (pa, pb) {
                            (Some(a), Some(b)) => self.const_expr_same(&a, &b, Rc::as_ptr(t)),
                            _ => false,
                        };
                        let fl = mflags::VISIBILITY | mflags::FINAL;
                        if prev.flags & fl != c.flags & fl
                            || !same_val
                            || prev.ty.as_ref().map(|t| t.display())
                                != c.ty.as_ref().map(|t| t.display())
                        {
                            let first = const_from_trait
                                .get(&c.name)
                                .map(|n| n.dname())
                                .unwrap_or_else(|| cname.clone());
                            return self.link_err(decl, &format!(
                                "{} and {} define the same constant ({}) in the composition of {}. However, the definition differs and is considered incompatible. Class was composed",
                                first,
                                t.name.dname(),
                                c.name.dname(),
                                cname
                            ));
                        }
                        continue;
                    }
                    if prev.flags & mflags::FINAL != 0 {
                        return self.link_err(
                            decl,
                            &format!(
                                "{}::{} cannot override final constant {}::{}",
                                cname,
                                c.name.dname(),
                                unsafe { (*prev.class).name.dname() },
                                c.name.dname()
                            ),
                        );
                    }
                }
                const_from_trait.insert(c.name.clone(), t.name.clone());
                let cc = Rc::new(ClassConst {
                    name: c.name.clone(),
                    value: RefCell::new(c.value.borrow().clone()),
                    flags: c.flags,
                    ty: c.ty.clone(),
                    class: cp,
                    doc: c.doc.clone(),
                    attrs: c.attrs.clone(),
                });
                set_const(cls, cc);
            }
        }
        Ok(())
    }

    /// Whether two constant expressions give the same value: the same
    /// code, or equal results (test Zend/tests/traits/constant_016).
    fn const_expr_same(&mut self, a: &ConstExpr, b: &ConstExpr, scope: *const Class) -> bool {
        if const_code_same(a, b) {
            return true;
        }
        let (Ok(x), Ok(y)) = (
            self.eval_const_expr(a, scope),
            self.eval_const_expr(b, scope),
        ) else {
            return false;
        };
        goro_runtime::compare::strict_eq(&x, &y)
    }

    /// A trait named by a trait rule: it must exist, be a trait and be
    /// used by the class.
    fn rule_trait(
        &mut self,
        decl: &ClassDecl,
        cname: &str,
        traits: &[Rc<Class>],
        n: &PhpStr,
    ) -> R<Rc<Class>> {
        let Some(t) = self.find_class(n, true)? else {
            return self.link_err(
                decl,
                &format!("Could not find trait {}", n.to_string_lossy()),
            );
        };
        if !t.is_trait() {
            return self.link_err(
                decl,
                &format!(
                    "Class {} is not a trait, Only traits may be used in 'as' and 'insteadof' statements",
                    t.name.dname()
                ),
            );
        }
        if !traits.iter().any(|x| Rc::ptr_eq(x, &t)) {
            return self.link_err(
                decl,
                &format!(
                    "Required Trait {} wasn't added to {}",
                    t.name.dname(),
                    cname
                ),
            );
        }
        Ok(t)
    }

    fn trait_method_copy(
        &mut self,
        m: &Rc<Func>,
        cp: *const Class,
        t: &Rc<Class>,
        alias: Option<PhpStr>,
        vis: u32,
    ) -> Rc<Func> {
        let code = m.code().unwrap().clone();
        let mut flags = m.flags | mflags::TRAIT_METHOD;
        if vis & mflags::VISIBILITY != 0 {
            flags = (flags & !mflags::VISIBILITY) | (vis & mflags::VISIBILITY);
        }
        if vis & mflags::FINAL != 0 {
            flags |= mflags::FINAL;
        }
        let mut f = Func::user(code, flags);
        if let Some(a) = alias {
            f.lname = lower(&a);
            f.name = a;
        } else {
            // An alias a trait made of another trait's method keeps its
            // name (test ext/reflection/tests/bug69180.phpt).
            f.lname = m.lname.clone();
            f.name = m.name.clone();
        }
        f.scope.set(cp);
        f.from_trait.set(Rc::as_ptr(t));
        Rc::new(f)
    }

    fn setup_enum(&mut self, cls: &Rc<Class>, decl: &ClassDecl) -> R<()> {
        let cp = Rc::as_ptr(cls);
        // Properties from traits (test Zend/tests/enum/traits-no-properties).
        if !cls.props.borrow().is_empty() || !cls.static_props.borrow().is_empty() {
            return self.link_err(
                decl,
                &format!("Enum {} cannot include properties", cls.name.dname()),
            );
        }
        // name / value properties (readonly).
        let mk = |name: &str, slot: u32, ty: TypeHint| PropInfo {
            name: PhpStr::intern(name.as_bytes()),
            slot,
            flags: mflags::PUBLIC | mflags::READONLY | mflags::TYPED,
            ty: Some(ty),
            class: cp,
            get: None,
            set: None,
            doc: None,
            attrs: Rc::from(Vec::new()),
            default: None,
        };
        let slot = cls.num_slots.get();
        cls.props.borrow_mut().push(Rc::new(mk(
            "name",
            slot,
            TypeHint {
                mask: tmask::STRING,
                classes: Vec::new(),
            },
        )));
        cls.prop_map
            .borrow_mut()
            .insert(PhpStr::intern(b"name"), vec![0]);
        cls.num_slots.set(slot + 1);
        if let Some(b) = decl.enum_backing {
            let idx = cls.props.borrow().len() as u32;
            cls.props.borrow_mut().push(Rc::new(mk(
                "value",
                slot + 1,
                TypeHint {
                    mask: b,
                    classes: Vec::new(),
                },
            )));
            cls.prop_map
                .borrow_mut()
                .insert(PhpStr::intern(b"value"), vec![idx]);
            cls.num_slots.set(slot + 2);
        }
        cls.flags
            .set(cls.flags.get() | cflags::NO_DYNAMIC | cflags::NOT_CLONEABLE);
        // cases(), from(), tryFrom().
        let mut natives: Vec<(&'static str, &'static str, crate::func::NativeFn)> = vec![(
            "cases",
            "function cases(): array",
            crate::builtins::enums::enum_cases,
        )];
        if decl.enum_backing.is_some() {
            natives.push((
                "from",
                "function from(int|string $value): static",
                crate::builtins::enums::enum_from,
            ));
            natives.push((
                "tryFrom",
                "function tryFrom(int|string $value): ?static",
                crate::builtins::enums::enum_try_from,
            ));
        }
        for (n, sig, h) in natives {
            if decl
                .methods
                .iter()
                .any(|m| m.code.name.eq_ignore_ascii_case(n.as_bytes()))
            {
                return self.link_err(
                    decl,
                    &format!("Cannot redeclare {}::{}()", cls.name.dname(), n),
                );
            }
            let f = Rc::new(Func::native(
                n,
                sig,
                h,
                mflags::PUBLIC | mflags::STATIC | mflags::IMPLICIT,
            ));
            f.scope.set(cp);
            let ln = f.lname.clone();
            cls.methods.borrow_mut().insert(ln.clone(), f);
            cls.method_order.borrow_mut().push(ln);
        }
        // Magic methods are not allowed on enums, from traits either (test
        // Zend/tests/enum/traits-no-__construct).
        let own: Vec<Rc<Func>> = cls
            .methods
            .borrow()
            .values()
            .filter(|m| std::ptr::eq(m.scope.get(), cp))
            .cloned()
            .collect();
        for m in &own {
            let l = m.lname.clone();
            if matches!(
                l.as_bytes(),
                b"__construct"
                    | b"__destruct"
                    | b"__clone"
                    | b"__get"
                    | b"__set"
                    | b"__unset"
                    | b"__isset"
                    | b"__tostring"
                    | b"__debuginfo"
                    | b"__serialize"
                    | b"__unserialize"
                    | b"__sleep"
                    | b"__wakeup"
                    | b"__set_state"
            ) {
                return self.link_err(
                    decl,
                    &format!(
                        "Enum {} cannot include magic method {}",
                        cls.name.dname(),
                        m.name.dname()
                    ),
                );
            }
        }
        Ok(())
    }

    /// The singleton object of an enum case.
    /// The case values of a backed enum: each of the backing type, no two
    /// alike (tests Zend/tests/enum/backed-duplicate-*, backed-mismatch).
    pub fn enum_check_cases(&mut self, cls: &Rc<Class>) -> R<()> {
        let key = Rc::as_ptr(cls) as usize;
        if cls.enum_backing.is_none() || self.enums_checked.contains(&key) {
            return Ok(());
        }
        let cases: Vec<Rc<ClassConst>> = cls
            .consts
            .borrow()
            .iter()
            .filter(|c| c.flags & mflags::CASE != 0)
            .cloned()
            .collect();
        let mut seen: Vec<(Value, PhpStr)> = Vec::new();
        for c in cases {
            let state = c.value.borrow().clone();
            let v = match state {
                ConstState::Value(v) => v,
                ConstState::Pending(ce) => {
                    let v = self.eval_const_expr(&ce, Rc::as_ptr(cls))?;
                    *c.value.borrow_mut() = ConstState::Value(v.clone());
                    v
                }
                ConstState::Evaluating => continue,
            };
            let ok = match (&v, cls.enum_backing) {
                (Value::Int(_), Some(m)) => m == tmask::INT,
                (Value::Str(_), Some(m)) => m == tmask::STRING,
                _ => false,
            };
            if !ok {
                return self.type_error(&format!(
                    "Enum case type {} does not match enum backing type {}",
                    v.tname(),
                    if cls.enum_backing == Some(tmask::INT) {
                        "int"
                    } else {
                        "string"
                    }
                ));
            }
            if let Some((_, first)) = seen
                .iter()
                .find(|(x, _)| goro_runtime::compare::strict_eq(x, &v))
            {
                return self.throw(
                    "Error",
                    &format!(
                        "Duplicate value in enum {} for cases {} and {}",
                        cls.name.dname(),
                        first.to_string_lossy(),
                        c.name.to_string_lossy()
                    ),
                );
            }
            seen.push((v, c.name.clone()));
        }
        self.enums_checked.insert(key);
        Ok(())
    }

    pub fn enum_case(&mut self, cls: &Rc<Class>, name: &PhpStr) -> R<Value> {
        if let Some((_, o)) = cls.enum_cases.borrow().iter().find(|(n, _)| n == name) {
            return Ok(Value::Object(o.clone()));
        }
        self.enum_check_cases(cls)?;
        let Some(c) = cls.find_const(name) else {
            return self.throw(
                "Error",
                &format!(
                    "Undefined constant {}::{}",
                    cls.name.dname(),
                    name.to_string_lossy()
                ),
            );
        };
        let value = if cls.enum_backing.is_some() {
            let state = c.value.borrow().clone();
            let v = match state {
                ConstState::Value(v) => v,
                ConstState::Pending(ce) => {
                    let v = self.eval_const_expr(&ce, Rc::as_ptr(cls))?;
                    *c.value.borrow_mut() = ConstState::Value(v.clone());
                    v
                }
                ConstState::Evaluating => Value::Null,
            };
            let ok = match (&v, cls.enum_backing) {
                (Value::Int(_), Some(m)) => m == tmask::INT,
                (Value::Str(_), Some(m)) => m == tmask::STRING,
                _ => false,
            };
            if !ok {
                return self.type_error(&format!(
                    "Enum case type {} does not match enum backing type {}",
                    v.tname(),
                    if cls.enum_backing == Some(tmask::INT) {
                        "int"
                    } else {
                        "string"
                    }
                ));
            }
            Some(v)
        } else {
            None
        };
        let info: Rc<dyn goro_runtime::ClassInfo> = cls.clone();
        let mut slots = vec![Value::Str(name.clone())];
        if let Some(v) = value {
            slots.push(v);
        }
        let o = ObjRef::new(info, slots);
        cls.enum_cases.borrow_mut().push((name.clone(), o.clone()));
        Ok(Value::Object(o))
    }

    fn finish_class(&mut self, cls: &Rc<Class>) {
        let mut m = Magic::default();
        let get = |n: &[u8]| {
            cls.find_method(n)
                .filter(|f| f.flags & mflags::ABSTRACT == 0 || cls.is_interface())
        };
        m.construct = get(b"__construct");
        m.destruct = get(b"__destruct");
        m.get = get(b"__get");
        m.set = get(b"__set");
        m.isset = get(b"__isset");
        m.unset = get(b"__unset");
        // Internal classes written in PHP may take unknown method calls
        // through a hidden `__h_call` (SPL's iterators forward them to
        // the inner iterator; test RecursiveDirectoryIterator_getSubPath_basic).
        m.call = get(b"__call").or_else(|| get(b"__h_call"));
        m.call_static = get(b"__callstatic");
        m.to_string = get(b"__tostring");
        m.invoke = get(b"__invoke");
        m.clone = get(b"__clone");
        m.serialize = get(b"__serialize");
        m.unserialize = get(b"__unserialize");
        m.sleep = get(b"__sleep");
        m.wakeup = get(b"__wakeup");
        m.debug_info = get(b"__debuginfo");
        cls.has_dtor.set(m.destruct.is_some());
        let has_to_string = m.to_string.is_some();
        *cls.magic.borrow_mut() = m;
        // Classes with __toString implement Stringable.
        if has_to_string
            && let Some(s) = self.core.stringable.clone()
            && !cls.interfaces.borrow().iter().any(|i| Rc::ptr_eq(i, &s))
            && !Rc::ptr_eq(cls, &s)
        {
            cls.interfaces.borrow_mut().push(s);
        }
    }

    pub(crate) fn register_class(&mut self, cls: Rc<Class>) {
        let ln = cls.lname.clone();
        if !cls.has_flag(cflags::ANON) {
            self.class_order.push(ln.clone());
        }
        self.classes.insert(ln, cls);
    }

    /// Evaluates the initializers of a class's static properties.
    pub fn ensure_statics(&mut self, cls: &Rc<Class>) -> R<()> {
        if cls.statics_ready.get() {
            return Ok(());
        }
        cls.statics_ready.set(true);
        if let Some(p) = cls.parent.clone() {
            self.ensure_statics(&p)?;
        }
        let sps: Vec<Rc<StaticProp>> = cls.static_props.borrow().clone();
        for sp in sps {
            let init = sp.init.borrow_mut().take();
            if let Some(ce) = init {
                let r = self.eval_const_expr(&ce, sp.class);
                match r {
                    Ok(v) => {
                        let old = sp.cell.borrow().set(v);
                        drop(old);
                    }
                    Err(e) => {
                        *sp.init.borrow_mut() = Some(ce);
                        cls.statics_ready.set(false);
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Evaluates a constant expression in the scope of `class`.
    pub fn eval_const_expr(&mut self, ce: &ConstExpr, class: *const Class) -> R<Value> {
        match ce {
            ConstExpr::Value(v) => Ok(v.clone()),
            ConstExpr::Code(code) => {
                let f = Rc::new(Func::user(code.clone(), 0));
                f.scope.set(class);
                let c = super::calls::Callee {
                    func: f,
                    this: None,
                    static_class: class,
                    scope: class,
                    closure: None,
                    magic_name: None,
                };
                self.call_callee(c, Vec::new())
            }
        }
    }

    /// Declares an internal class from a module's PHP stub.
    pub(crate) fn declare_internal_class(&mut self, m: usize, i: usize) -> R<()> {
        let module = self.modules[m];
        let (_, src) = module.classes[i];
        let unit = match self.compile_source(src.as_bytes(), b"", false) {
            Ok(u) => u,
            Err(e) => panic!(
                "internal class stub {}: {:?}",
                module.classes[i].0, e.message
            ),
        };
        let main = unit.main.clone();
        self.units.push(main.clone());
        for decl in main.classes.iter() {
            if self.classes.contains_key(&lower(&decl.name)) {
                continue;
            }
            let cls = self.declare_class(decl)?;
            cls.flags.set(cls.flags.get() | cflags::INTERNAL);
            // Closures take no dynamic properties (test Zend/tests/closures/
            // closure_write_prop).
            if cls.lname.as_bytes() == b"closure" {
                cls.flags.set(cls.flags.get() | cflags::NO_DYNAMIC);
            }
            // tests Zend/tests/generators/clone, dynamic_properties,
            // errors/serialize_unserialize_error
            if cls.lname.as_bytes() == b"generator" {
                cls.flags.set(
                    cls.flags.get()
                        | cflags::NO_DYNAMIC
                        | cflags::NOT_CLONEABLE
                        | cflags::NOT_SERIALIZABLE,
                );
            }
            // Stub-only markers (php-src stubs' @not-serializable).
            if cls
                .attrs
                .iter()
                .any(|a| a.name.as_bytes() == b"NotSerializable")
            {
                cls.flags
                    .set(cls.flags.get() | cflags::NOT_SERIALIZABLE | cflags::NO_DYNAMIC);
            }
            self.bind_natives(&cls);
        }
        Ok(())
    }

    /// Replaces methods of an internal class by their native handlers.
    fn bind_natives(&mut self, cls: &Rc<Class>) {
        let cp = Rc::as_ptr(cls);
        let names: Vec<PhpStr> = cls.method_order.borrow().clone();
        for ln in names {
            let f = cls.methods.borrow().get(&ln).cloned().unwrap();
            if !std::ptr::eq(f.scope.get(), cp) {
                continue;
            }
            let mut key = cls.lname.to_vec();
            key.extend_from_slice(b"::");
            key.extend_from_slice(&ln);
            let Some(h) = self.native_methods.get(&PhpStr::from(key)).copied() else {
                continue;
            };
            let code = f.code().unwrap().clone();
            let info = std::cell::OnceCell::new();
            let _ = info.set(Rc::new(crate::func::NativeSig {
                params: code.params.clone(),
                ret: code.ret_type.clone(),
                num_required: code.num_required,
                by_ref_return: code.flags & fflags::BY_REF != 0,
                variadic: code.flags & fflags::VARIADIC != 0,
                ret_text: code.ret_type.as_ref().map(|t| t.display()),
            }));
            let nf = Rc::new(Func {
                name: f.name.clone(),
                lname: f.lname.clone(),
                kind: FuncKind::Native {
                    handler: h,
                    sig: "",
                    info,
                },
                flags: f.flags & !mflags::ABSTRACT
                    | (f.flags
                        & mflags::ABSTRACT
                        & if cls.is_interface() {
                            mflags::ABSTRACT
                        } else {
                            0
                        }),
                scope: Cell::new(cp),
                from_trait: Cell::new(std::ptr::null()),
                statics: RefCell::new(Vec::new()),
                proto: RefCell::new(f.proto.borrow().clone()),
            });
            cls.methods.borrow_mut().insert(ln, nf);
        }
        // Native hooks.
        if let Some(h) = self.native_hooks.get(&cls.lname).copied() {
            let hooks = h();
            // SAFETY: the class was just created and is not shared yet in a
            // way that reads `native`.
            let p = Rc::as_ptr(cls) as *mut Class;
            unsafe { (*p).native = hooks };
        } else if let Some(parent) = &cls.parent {
            let p = Rc::as_ptr(cls) as *mut Class;
            unsafe {
                (*p).native = copy_hooks(&parent.native);
            }
        }
        self.finish_class(cls);
    }
}

pub(crate) fn copy_hooks(h: &NativeHooks) -> NativeHooks {
    NativeHooks {
        create: h.create,
        clone: h.clone,
        compare: h.compare,
        cast: h.cast,
        count: h.count,
        debug_props: h.debug_props,
        props_for: h.props_for,
        dim_storage: h.dim_storage,
        dyn_prop: h.dyn_prop,
        get_iterator: h.get_iterator,
        do_operation: h.do_operation,
        do_unary: h.do_unary,
        unserialize_legacy: h.unserialize_legacy,
    }
}

fn set_const(cls: &Class, c: Rc<ClassConst>) {
    let mut m = cls.const_map.borrow_mut();
    match m.get(&c.name) {
        Some(&i) => cls.consts.borrow_mut()[i] = c,
        None => {
            let i = cls.consts.borrow().len();
            m.insert(c.name.clone(), i);
            cls.consts.borrow_mut().push(c);
        }
    }
}

fn vis_rank(v: u32) -> u8 {
    if v & mflags::PRIVATE != 0 {
        2
    } else if v & mflags::PROTECTED != 0 {
        1
    } else {
        0
    }
}

fn vis_name(v: u32) -> &'static str {
    match vis_rank(v) {
        2 => "private",
        1 => "protected",
        _ => "public",
    }
}

fn valid_class_name(n: &[u8]) -> bool {
    !n.is_empty()
        && n.iter()
            .all(|&c| c == b'\\' || c == b'_' || c.is_ascii_alphanumeric() || c >= 0x80)
}

/// Whether two constant expressions are the same value or the same code.
fn const_code_same(a: &ConstExpr, b: &ConstExpr) -> bool {
    match (a, b) {
        (ConstExpr::Value(x), ConstExpr::Value(y)) => goro_runtime::compare::strict_eq(x, y),
        (ConstExpr::Code(x), ConstExpr::Code(y)) => {
            format!("{:?}", x.ops) == format!("{:?}", y.ops)
                && format!("{:?}", x.consts) == format!("{:?}", y.consts)
        }
        _ => false,
    }
}
