//! Type variance of inherited methods (PHP manual: "Covariance and
//! Contravariance"; RFCs: Covariant Returns and Contravariant Parameters,
//! Union Types 2.0, Pure intersection types, DNF types, Static return
//! type, mixed type v2, never return type).
//!
//! A method's parameter types may be widened and its return type narrowed
//! with respect to the method it overrides or implements. The checks run
//! once the class is registered, so that classes loaded to answer them
//! (by autoloading) may already refer to it.

use std::rc::Rc;

use goro_runtime::PhpStr;

use super::{R, Vm};
use crate::bytecode::{ParamDefault, ParamInfo, TypeHint, fflags, tmask};
use crate::class::{Class, DisplayName, cflags, lower};
use crate::func::{Func, FuncKind};

/// A pending check: `child` overrides or implements `parent`.
pub(crate) struct Obligation {
    pub child: Rc<Func>,
    pub parent: Rc<Func>,
}

/// Outcome of a subtype test.
#[derive(Debug, PartialEq)]
pub(crate) enum Sub {
    Yes,
    No,
    /// A class the answer depends on is not available.
    Unresolved(PhpStr),
}

impl Sub {
    fn and(self, other: Sub) -> Sub {
        match (self, other) {
            (Sub::No, _) | (_, Sub::No) => Sub::No,
            (Sub::Unresolved(n), _) | (_, Sub::Unresolved(n)) => Sub::Unresolved(n),
            _ => Sub::Yes,
        }
    }
}

/// The scalar bits a type mask covers, with `iterable` as `array` plus
/// the Traversable class (PHP 8.2: iterable is `Traversable|array`).
const SCALARS: u32 = tmask::NULL
    | tmask::FALSE
    | tmask::TRUE
    | tmask::INT
    | tmask::FLOAT
    | tmask::STRING
    | tmask::ARRAY
    | tmask::CALLABLE
    | tmask::RESOURCE;

impl Vm {
    /// Looks a class up for a variance check: autoloading unless classes
    /// are being bound early (when a missing class defers the binding).
    fn variance_class(&mut self, name: &[u8]) -> R<Option<Rc<Class>>> {
        let autoload = !self.early_binding;
        self.find_class(name, autoload)
    }

    /// Resolves `self` / `parent` in a type written in class `scope`.
    fn resolve_rel(name: &PhpStr, scope: &Class) -> PhpStr {
        let l = name.to_ascii_lowercase();
        if l == b"self" {
            return scope.name.clone();
        }
        if l == b"parent"
            && let Some(p) = &scope.parent
        {
            return p.name.clone();
        }
        name.clone()
    }

    /// Whether class `c` (written in `cs`) is a subtype of class `p`
    /// (written in `ps`).
    fn class_sub(&mut self, c: &PhpStr, cs: &Class, p: &PhpStr, ps: &Class) -> R<Sub> {
        let c = Self::resolve_rel(c, cs);
        let p = Self::resolve_rel(p, ps);
        if lower(&c) == lower(&p) {
            return Ok(Sub::Yes);
        }
        let Some(cc) = self.variance_class(&c)? else {
            return Ok(Sub::Unresolved(c));
        };
        let Some(pc) = self.variance_class(&p)? else {
            return Ok(Sub::Unresolved(p));
        };
        Ok(if cc.is_subclass_of(&pc) {
            Sub::Yes
        } else {
            Sub::No
        })
    }

    /// Whether the intersection `g` (written in `cs`) is a subtype of type
    /// `p` (written in `ps`).
    fn group_sub(&mut self, g: &[PhpStr], cs: &Class, p: &TypeHint, ps: &Class) -> R<Sub> {
        if p.mask & (tmask::OBJECT | tmask::MIXED) != 0 {
            return Ok(Sub::Yes);
        }
        let mut unresolved: Option<PhpStr> = None;
        // iterable: a Traversable class.
        if p.mask & tmask::ITERABLE != 0 {
            let t = PhpStr::new(b"Traversable");
            for x in g {
                match self.class_sub(x, cs, &t, ps)? {
                    Sub::Yes => return Ok(Sub::Yes),
                    Sub::Unresolved(n) => unresolved = Some(n),
                    Sub::No => {}
                }
            }
        }
        // callable: Closure and classes with __invoke().
        if p.mask & tmask::CALLABLE != 0 {
            for x in g {
                let x = Self::resolve_rel(x, cs);
                if let Some(c) = self.variance_class(&x)?
                    && (c.lname.as_bytes() == b"closure" || c.find_method(b"__invoke").is_some())
                {
                    return Ok(Sub::Yes);
                }
            }
        }
        for h in &p.classes {
            // g ⊆ h when every class of h has a subtype in g.
            let mut all = Sub::Yes;
            for hc in h {
                let mut any = Sub::No;
                for gc in g {
                    match self.class_sub(gc, cs, hc, ps)? {
                        Sub::Yes => {
                            any = Sub::Yes;
                            break;
                        }
                        Sub::Unresolved(n) => any = Sub::Unresolved(n),
                        Sub::No => {}
                    }
                }
                all = all.and(any);
                if all == Sub::No {
                    break;
                }
            }
            match all {
                Sub::Yes => return Ok(Sub::Yes),
                Sub::Unresolved(n) => unresolved = Some(n),
                Sub::No => {}
            }
        }
        // static: a class of a final scope is static (test
        // Zend/tests/type_declarations/variance/override_static_with_self).
        if p.mask & tmask::STATIC != 0 && g.len() == 1 {
            let x = Self::resolve_rel(&g[0], cs);
            if lower(&x) == cs.lname && cs.has_flag(cflags::FINAL) {
                return Ok(Sub::Yes);
            }
        }
        Ok(match unresolved {
            Some(n) => Sub::Unresolved(n),
            None => Sub::No,
        })
    }

    /// Whether type `c` (written in class `cs`) is a subtype of type `p`
    /// (written in class `ps`).
    pub(crate) fn type_sub(
        &mut self,
        c: &TypeHint,
        cs: &Class,
        p: &TypeHint,
        ps: &Class,
    ) -> R<Sub> {
        if c.mask & tmask::NEVER != 0 {
            return Ok(Sub::Yes);
        }
        if c.mask & tmask::VOID != 0 || p.mask & tmask::VOID != 0 {
            return Ok(if c.mask & tmask::VOID != 0 && p.mask & tmask::VOID != 0 {
                Sub::Yes
            } else {
                Sub::No
            });
        }
        if p.mask & tmask::MIXED != 0 {
            return Ok(Sub::Yes);
        }
        if c.mask & tmask::MIXED != 0 {
            return Ok(Sub::No);
        }
        // Scalar parts.
        let mut pm = p.mask;
        if pm & tmask::ITERABLE != 0 {
            pm |= tmask::ARRAY;
        }
        if c.mask & SCALARS & !pm != 0 {
            return Ok(Sub::No);
        }
        let mut res = Sub::Yes;
        if c.mask & tmask::OBJECT != 0 && p.mask & tmask::OBJECT == 0 {
            return Ok(Sub::No);
        }
        if c.mask & tmask::ITERABLE != 0 && p.mask & tmask::ITERABLE == 0 {
            // array|Traversable.
            if p.mask & tmask::ARRAY == 0 {
                return Ok(Sub::No);
            }
            let t = [PhpStr::new(b"Traversable")];
            res = res.and(self.group_sub(&t, cs, p, ps)?);
        }
        if c.mask & tmask::STATIC != 0 && p.mask & tmask::STATIC == 0 {
            // static is the scope class or a subclass.
            let g = [cs.name.clone()];
            let mut q = p.clone();
            q.mask &= !tmask::STATIC;
            res = res.and(self.group_sub(&g, cs, &q, ps)?);
        }
        for g in &c.classes {
            if res == Sub::No {
                break;
            }
            res = res.and(self.group_sub(g, cs, p, ps)?);
        }
        Ok(res)
    }

    /// The return type of a function.
    fn ret_type_of_func(&mut self, f: &Func) -> Option<TypeHint> {
        match &f.kind {
            FuncKind::User(c) => c.ret_type.clone(),
            FuncKind::Native { .. } => self.native_sig(f).ret.clone(),
        }
    }

    /// Checks one obligation: parameter types contravariant, the return
    /// type covariant.
    fn check_obligation(&mut self, ob: &Obligation) -> R<Sub> {
        let cs_ptr = ob.child.scope.get();
        let ps_ptr = ob.parent.scope.get();
        if cs_ptr.is_null() || ps_ptr.is_null() {
            return Ok(Sub::Yes);
        }
        let cs = unsafe { &*cs_ptr };
        let ps = unsafe { &*ps_ptr };
        let cp: Vec<ParamInfo> = self.params_of(&ob.child).to_vec();
        let pp: Vec<ParamInfo> = self.params_of(&ob.parent).to_vec();
        let mut res = Sub::Yes;
        for (i, p) in pp.iter().enumerate() {
            let c = match cp.get(i) {
                Some(c) => c,
                None => match cp.last() {
                    Some(l) if l.variadic => l,
                    _ => continue,
                },
            };
            let Some(ct) = &c.ty else {
                continue;
            };
            let r = match &p.ty {
                None => {
                    if ct.mask & tmask::MIXED != 0 {
                        Sub::Yes
                    } else {
                        Sub::No
                    }
                }
                Some(pt) => self.type_sub(pt, ps, ct, cs)?,
            };
            res = res.and(r);
            if res == Sub::No {
                return Ok(Sub::No);
            }
        }
        // A child's own variadic parameter must accept the parent's one.
        if let (Some(pl), Some(cl)) = (pp.last(), cp.last())
            && pl.variadic
            && cl.variadic
            && cp.len() > pp.len()
            && let Some(ct) = &cl.ty
        {
            let r = match &pl.ty {
                None => {
                    if ct.mask & tmask::MIXED != 0 {
                        Sub::Yes
                    } else {
                        Sub::No
                    }
                }
                Some(pt) => self.type_sub(pt, ps, ct, cs)?,
            };
            res = res.and(r);
        }
        Ok(res)
    }

    fn check_return(&mut self, ob: &Obligation) -> R<Sub> {
        let cs = unsafe { &*ob.child.scope.get() };
        let ps = unsafe { &*ob.parent.scope.get() };
        let Some(pt) = self.ret_type_of_func(&ob.parent) else {
            return Ok(Sub::Yes);
        };
        let Some(ct) = self.ret_type_of_func(&ob.child) else {
            // __toString() implicitly returns string.
            if ob.child.lname.as_bytes() == b"__tostring" {
                return Ok(Sub::Yes);
            }
            return Ok(Sub::No);
        };
        self.type_sub(&ct, cs, &pt, ps)
    }

    /// Whether every obligation can be decided without loading classes
    /// (early binding defers the class otherwise).
    pub(crate) fn obligations_resolvable(&mut self, obs: &[Obligation]) -> R<bool> {
        for ob in obs {
            if ob.child.scope.get().is_null() || ob.parent.scope.get().is_null() {
                continue;
            }
            if let Sub::Unresolved(_) = self.check_obligation(ob)? {
                return Ok(false);
            }
            if let Sub::Unresolved(_) = self.check_return(ob)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Runs the variance checks of a class being declared.
    pub(crate) fn check_obligations(
        &mut self,
        obs: Vec<Obligation>,
        decl: &Rc<crate::class::ClassDecl>,
    ) -> R<()> {
        for ob in obs {
            if ob.child.scope.get().is_null() || ob.parent.scope.get().is_null() {
                continue;
            }
            // Internal classes are taken as declared.
            if decl.file.is_empty() || unsafe { &*ob.child.scope.get() }.has_flag(cflags::INTERNAL)
            {
                continue;
            }
            let params = self.check_obligation(&ob)?;
            let mut ret = self.check_return(&ob)?;
            if params == Sub::Yes && ret != Sub::Yes && self.tentative_return(&ob.parent) {
                // PHP 8.1: internal methods' return types are tentative.
                let will_change = match &ob.child.kind {
                    FuncKind::User(c) => c
                        .attrs
                        .iter()
                        .any(|a| a.name.eq_ignore_ascii_case(b"ReturnTypeWillChange")),
                    _ => false,
                };
                if !will_change && !matches!(ret, Sub::Unresolved(_)) {
                    let fs = self.variance_sig(&ob.child);
                    let ps = self.variance_sig(&ob.parent);
                    let (file, line) = self.obligation_loc(&ob, decl);
                    self.raise_at(
                        super::errors::E_DEPRECATED,
                        &format!(
                            "Return type of {} should either be compatible with {}, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice",
                            fs, ps
                        ),
                        file,
                        line,
                    )?;
                }
                if !matches!(ret, Sub::Unresolved(_)) {
                    ret = Sub::Yes;
                }
            }
            let r = params.and(ret);
            if r == Sub::Yes {
                continue;
            }
            // A class being autoloaded up the stack: checked once it is.
            if let Sub::Unresolved(n) = &r {
                let l = lower(n);
                if self.autoloading.contains(&l) {
                    self.delayed_obligations.push((l, ob, decl.clone()));
                    continue;
                }
            }
            let fs = self.variance_sig(&ob.child);
            let ps = self.variance_sig(&ob.parent);
            let msg = match r {
                Sub::Unresolved(n) => format!(
                    "Could not check compatibility between {} and {}, because class {} is not available",
                    fs,
                    ps,
                    n.to_string_lossy()
                ),
                _ => format!("Declaration of {} must be compatible with {}", fs, ps),
            };
            let (file, line) = self.obligation_loc(&ob, decl);
            return Err(self.fatal_at(&msg, &file, line));
        }
        Ok(())
    }

    fn obligation_loc(&self, ob: &Obligation, decl: &crate::class::ClassDecl) -> (PhpStr, u32) {
        match &ob.child.kind {
            FuncKind::User(c) if !c.file.is_empty() && *c.file == *decl.file => {
                (decl.file.clone(), c.line_start)
            }
            _ => (decl.file.clone(), decl.line_start),
        }
    }

    /// Whether `f`'s return type is tentative (internal, PHP 8.1).
    fn tentative_return(&self, f: &Func) -> bool {
        crate::builtins::reflection::is_tentative(f)
    }

    /// A method's signature as compatibility errors show it.
    pub(crate) fn variance_sig(&mut self, f: &Func) -> String {
        let params: Vec<ParamInfo> = self.params_of(f).to_vec();
        let by_ref = match &f.kind {
            FuncKind::User(c) => c.flags & fflags::BY_REF != 0,
            FuncKind::Native { .. } => self.native_sig(f).by_ref_return,
        };
        let mut s = String::new();
        if by_ref {
            s.push_str("& ");
        }
        // An abstract trait method is named after its trait, its types
        // after the using class (test Zend/tests/traits/abstract_method_10).
        let tr = f.from_trait.get();
        let in_trait = f.flags & crate::class::mflags::ABSTRACT != 0 && !tr.is_null();
        let scope_name = f.scope().map(|c| c.name.clone());
        let show = |t: &TypeHint| -> String {
            match (&scope_name, in_trait) {
                (Some(n), true) => {
                    let mut t = t.clone();
                    for g in t.classes.iter_mut() {
                        for c in g.iter_mut() {
                            if c.eq_ignore_ascii_case(b"self") {
                                *c = n.clone();
                            }
                        }
                    }
                    t.display()
                }
                _ => t.display(),
            }
        };
        if in_trait {
            s.push_str(&format!(
                "{}::{}",
                unsafe { (*tr).name.dname() },
                f.name.dname()
            ));
        } else {
            s.push_str(&f.display_name());
        }
        s.push('(');
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            if let Some(t) = &p.ty {
                s.push_str(&show(t));
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
            if let Some(d) = sig_default(p) {
                s.push_str(" = ");
                s.push_str(&d);
            }
        }
        s.push(')');
        if let Some(r) = self.ret_type_of_func(f) {
            s.push_str(": ");
            s.push_str(&show(&r));
        }
        s
    }
}

/// A default value as signatures in messages show it: literals, `[]` or
/// `[...]` for arrays, strings cut to 10 characters, other expressions
/// as written.
fn sig_default(p: &ParamInfo) -> Option<String> {
    use goro_runtime::Value;
    let lit = |v: &Value| -> String {
        match v {
            Value::Null => "null".into(),
            Value::Bool(true) => "true".into(),
            Value::Bool(false) => "false".into(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => goro_runtime::convert::float_to_string(*f, 14).to_string_lossy(),
            Value::Str(s) => {
                let b = s.as_bytes();
                if b.len() > 10 {
                    format!("'{}...'", String::from_utf8_lossy(&b[..10]))
                } else {
                    format!("'{}'", String::from_utf8_lossy(b))
                }
            }
            Value::Array(a) => {
                if a.is_empty() {
                    "[]".into()
                } else {
                    "[...]".into()
                }
            }
            _ => "<default>".into(),
        }
    };
    // A constant name as written (test Zend/tests/parameter_default_values/
    // internal_declaration_error_const).
    if let Some(t) = p.default_text.as_deref().map(str::trim)
        && t.starts_with(|c: char| c.is_ascii_alphabetic() || c == '\\' || c == '_')
        && !matches!(t.to_ascii_lowercase().as_str(), "null" | "true" | "false")
        && !t.to_ascii_lowercase().starts_with("array")
        && !matches!(p.default, ParamDefault::None)
    {
        return Some(t.to_string());
    }
    match &p.default {
        ParamDefault::None => None,
        ParamDefault::Const(v) => Some(lit(v)),
        _ => {
            let t = p.default_text.as_deref()?.trim();
            let l = t.to_ascii_lowercase();
            if l == "null" || l == "true" || l == "false" {
                return Some(l);
            }
            if t.starts_with('[') || l.starts_with("array(") || l.starts_with("array (") {
                let inner = t
                    .trim_start_matches(['[', '('])
                    .trim_start_matches("array")
                    .trim_start_matches("ARRAY");
                let inner = inner.trim_matches(|c: char| {
                    c == '(' || c == ')' || c == '[' || c == ']' || c.is_whitespace()
                });
                return Some(if inner.is_empty() {
                    "[]".into()
                } else {
                    "[...]".into()
                });
            }
            Some(t.to_string())
        }
    }
}
