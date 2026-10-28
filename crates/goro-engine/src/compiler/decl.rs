//! Functions, closures, parameters, types, classes, attributes and constant
//! expressions.

use std::rc::Rc;

use goro_runtime::{PhpStr, Value};
use goro_syntax::ast::*;

use super::scan::Scan;
use super::{CResult, ClassCtx, Compiler, FState};
use crate::bytecode::*;
use crate::class::{
    ClassDecl, ClassKind, ConstDecl, MethodDecl, PropDecl, TraitRule, TraitUseDecl, mflags,
};

pub(crate) enum FnBody<'a> {
    Block(Block<'a>),
    Expr(&'a Expr<'a>),
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FnKind {
    Function,
    Method { is_static: bool, is_ctor: bool },
    Closure { is_static: bool },
    Arrow { is_static: bool },
}

pub(crate) struct FnSpec<'a> {
    pub name: PhpStr,
    pub params: &'a [Param<'a>],
    pub ret: Option<&'a Type<'a>>,
    pub body: FnBody<'a>,
    pub by_ref: bool,
    pub kind: FnKind,
    pub uses: &'a [ClosureUse<'a>],
    pub span: Span,
    pub doc: Option<&'a [u8]>,
    pub attrs: &'a [AttributeGroup<'a>],
    pub method_name: PhpStr,
    pub func_name: PhpStr,
    pub prop_name: Option<PhpStr>,
}

impl<'a> Compiler<'a> {
    pub(crate) fn func_decl(&mut self, fd: &'a FunctionDecl<'a>) -> CResult<u32> {
        if fd.name.name.eq_ignore_ascii_case(b"assert") && !self.ns.is_empty() {
            // test Zend/tests/custom_assert_forbidden
            return self.err(
                "Defining a custom assert() function is not allowed, as the function has special semantics",
                fd.span.line,
            );
        }
        let full = self.qualify_pub(fd.name.name);
        let name = PhpStr::intern(&full);
        let spec = FnSpec {
            name: name.clone(),
            params: fd.params,
            ret: fd.ret.as_ref(),
            body: FnBody::Block(fd.body),
            by_ref: fd.by_ref,
            kind: FnKind::Function,
            uses: &[],
            span: fd.span,
            doc: fd.doc_comment,
            attrs: fd.attrs,
            method_name: name.clone(),
            func_name: name,
            prop_name: None,
        };
        let saved_class = self.class.take();
        let code = self.compile_fn(spec);
        self.class = saved_class;
        let code = code?;
        self.f.children.push(Rc::new(code));
        Ok(self.f.children.len() as u32 - 1)
    }

    pub(crate) fn qualify_pub(&self, name: &[u8]) -> Vec<u8> {
        if self.ns.is_empty() {
            name.to_vec()
        } else {
            let mut v = self.ns.clone();
            v.push(b'\\');
            v.extend_from_slice(name);
            v
        }
    }

    fn closure_name(&self, line: u32) -> PhpStr {
        // PHP 8.4+: `{closure:enclosing():line}` or `{closure:file:line}`.
        let outer = if self.f.flags & fflags::PSEUDO_MAIN != 0 {
            self.file.to_string_lossy()
        } else if !self.f.method_name.is_empty() {
            let m = self.f.method_name.to_string_lossy();
            if m.starts_with("{closure") {
                m
            } else {
                format!("{}()", m)
            }
        } else {
            self.file.to_string_lossy()
        };
        PhpStr::from(format!("{{closure:{}:{}}}", outer, line))
    }

    pub(crate) fn closure(&mut self, cl: &'a Closure<'a>, span: Span) -> CResult<u32> {
        let name = self.closure_name(span.line);
        for (i, u) in cl.uses.iter().enumerate() {
            if u.name.name == b"this" {
                return self.err("Cannot use $this as lexical variable", u.span.line);
            }
            // tests Zend/tests/closures/closure_use_auto_global,
            // closure_use_variable_twice
            if super::is_superglobal_name(u.name.name) {
                return self.err("Cannot use auto-global as lexical variable", u.span.line);
            }
            if cl.uses[..i].iter().any(|x| x.name.name == u.name.name) {
                return self.err(
                    format!(
                        "Cannot use variable ${} twice",
                        String::from_utf8_lossy(u.name.name)
                    ),
                    u.span.line,
                );
            }
            if cl.params.iter().any(|p| p.name.name == u.name.name) {
                return self.err(
                    format!(
                        "Cannot use lexical variable ${} as a parameter name",
                        String::from_utf8_lossy(u.name.name)
                    ),
                    u.span.line,
                );
            }
        }
        let spec = FnSpec {
            name: name.clone(),
            params: cl.params,
            ret: cl.ret.as_ref(),
            body: FnBody::Block(cl.body),
            by_ref: cl.by_ref,
            kind: FnKind::Closure {
                is_static: cl.is_static,
            },
            uses: cl.uses,
            span,
            doc: cl.doc_comment,
            attrs: cl.attrs,
            method_name: name.clone(),
            func_name: name,
            prop_name: None,
        };
        let code = self.compile_fn(spec)?;
        self.f.children.push(Rc::new(code));
        Ok(self.f.children.len() as u32 - 1)
    }

    pub(crate) fn arrow_fn(&mut self, af: &'a ArrowFn<'a>, span: Span) -> CResult<u32> {
        let name = self.closure_name(span.line);
        let spec = FnSpec {
            name: name.clone(),
            params: af.params,
            ret: af.ret.as_ref(),
            body: FnBody::Expr(af.body),
            by_ref: af.by_ref,
            kind: FnKind::Arrow {
                is_static: af.is_static,
            },
            uses: &[],
            span,
            doc: af.doc_comment,
            attrs: af.attrs,
            method_name: name.clone(),
            func_name: name,
            prop_name: None,
        };
        let code = self.compile_fn(spec)?;
        self.f.children.push(Rc::new(code));
        Ok(self.f.children.len() as u32 - 1)
    }

    /// Compiles a function body into its own [`FuncCode`].
    pub(crate) fn compile_fn(&mut self, spec: FnSpec<'a>) -> CResult<FuncCode> {
        // Variables: parameters first, then closure uses, then the body.
        let mut sc = Scan::default();
        for p in spec.params {
            if p.name.name == b"this" {
                return self.err("Cannot use $this as parameter", p.span.line);
            }
            if sc.vars.contains(&p.name.name) {
                return self.err(
                    format!(
                        "Redefinition of parameter ${}",
                        String::from_utf8_lossy(p.name.name)
                    ),
                    p.span.line,
                );
            }
            sc.add(p.name.name);
        }
        for u in spec.uses {
            sc.add(u.name.name);
        }
        let mut arrow_uses: Vec<&'a [u8]> = Vec::new();
        match &spec.body {
            FnBody::Block(b) => sc.stmts(b),
            FnBody::Expr(e) => {
                let before = sc.vars.len();
                sc.expr(e);
                arrow_uses = sc.vars[spec.params.len()..].to_vec();
                let _ = before;
            }
            FnBody::None => {}
        }
        for p in spec.params {
            if let Some(d) = &p.default {
                sc.expr(d);
            }
        }
        let is_generator = sc.generator;
        let uses_this = sc.uses_this;
        let dynamic = sc.dynamic;
        let parent_f = std::mem::replace(&mut self.f, FState::new(sc.vars));
        self.f.line = spec.span.line;
        self.f.func_name = spec.func_name.clone();
        self.f.method_name = spec.method_name.clone();
        self.f.prop_name = spec.prop_name.clone();
        self.f.is_generator = is_generator;
        let r = self.compile_fn_body(
            &spec,
            &parent_f,
            arrow_uses,
            is_generator,
            uses_this,
            dynamic,
        );
        let child = std::mem::replace(&mut self.f, parent_f);
        let _ = child;
        r
    }

    fn compile_fn_body(
        &mut self,
        spec: &FnSpec<'a>,
        parent: &FState<'a>,
        arrow_uses: Vec<&'a [u8]>,
        is_generator: bool,
        uses_this: bool,
        dynamic: bool,
    ) -> CResult<FuncCode> {
        let mut flags = 0;
        if spec.by_ref {
            flags |= fflags::BY_REF;
        }
        if uses_this {
            flags |= fflags::USES_THIS;
        }
        if dynamic {
            flags |= fflags::DYNAMIC_VARS;
        }
        match spec.kind {
            FnKind::Closure { is_static } => {
                flags |= fflags::CLOSURE;
                if is_static {
                    flags |= fflags::STATIC_CLOSURE;
                }
            }
            FnKind::Arrow { is_static } => {
                flags |= fflags::CLOSURE | fflags::ARROW;
                if is_static {
                    flags |= fflags::STATIC_CLOSURE;
                }
            }
            FnKind::Method { is_static, .. } => {
                self.f.in_class_method = true;
                self.f.is_static_method = is_static;
            }
            _ => {}
        }
        if let FnBody::None = spec.body {
            flags |= fflags::ABSTRACT;
        }
        // Closure uses.
        let mut uses = Vec::new();
        for u in spec.uses {
            let inner = self.cv(u.name.name);
            let outer = parent.cvs.iter().position(|n| *n == u.name.name).unwrap() as u32;
            uses.push((inner, outer, u.by_ref));
        }
        for n in arrow_uses {
            if n == b"this" {
                continue;
            }
            if let Some(outer) = parent.cvs.iter().position(|x| *x == n) {
                let inner = self.cv(n);
                uses.push((inner, outer as u32, false));
            }
        }
        self.f.uses = uses;
        // Return type.
        let ret_type = match spec.ret {
            Some(t) => {
                let th = self.type_hint(t, true)?;
                flags |= fflags::HAS_RETURN_TYPE;
                if th.mask == tmask::VOID {
                    flags |= fflags::RETURNS_VOID;
                }
                if th.mask == tmask::NEVER {
                    flags |= fflags::RETURNS_NEVER;
                }
                if is_generator {
                    self.check_generator_return(&th, t.span().line)?;
                }
                self.f.ret_allows_null = th.allows_null();
                if th.mask & (tmask::VOID | tmask::NEVER) != 0 && spec.by_ref {
                    // allowed for void? PHP: by-ref void is allowed with
                    // deprecation in 8.1 ("Returning by reference from a void
                    // function is deprecated").
                    if th.mask & tmask::VOID != 0 {
                        self.notice(
                            goro_runtime::Level::Deprecated,
                            format!(
                                "{}(): Returning by reference from a void function is deprecated",
                                self.f.method_name.to_string_lossy()
                            ),
                            spec.span.line,
                        );
                    }
                }
                Some(th)
            }
            // __toString() returns string implicitly (PHP 8.0, RFC:
            // Stringable; test Zend/tests/magic_methods/bug26166).
            None if matches!(spec.kind, FnKind::Method { .. })
                && spec.name.eq_ignore_ascii_case(b"__tostring") =>
            {
                flags |= fflags::HAS_RETURN_TYPE;
                Some(TypeHint {
                    mask: tmask::STRING,
                    classes: Vec::new(),
                })
            }
            None => None,
        };
        if let FnKind::Method { is_ctor: true, .. } = spec.kind
            && ret_type.is_some()
        {
            let cname = self
                .class
                .as_ref()
                .map(|c| c.name.to_string_lossy())
                .unwrap_or_default();
            return self.err(
                format!(
                    "Method {}::__construct() cannot declare a return type",
                    cname
                ),
                spec.span.line,
            );
        }
        self.f.flags |= flags;
        // Parameters.
        let (params, num_required) = self.params(spec)?;
        // Prologue: defaults, promoted properties.
        for (i, p) in spec.params.iter().enumerate() {
            if matches!(params[i].default, ParamDefault::Code) {
                self.f.flags |= fflags::HAS_DEFAULT_CODE;
                let at = self.emit(Op::RecvDefault {
                    param: i as u32,
                    t: 0,
                });
                let d = p.default.as_ref().unwrap();
                self.in_const_expr = true;
                let v = self.expr(d);
                self.in_const_expr = false;
                let v = v?;
                self.free(v);
                self.emit(Op::Assign {
                    dst: i as u32,
                    src: v,
                });
                if params[i].ty.is_some() {
                    self.emit(Op::RecvCheck { param: i as u32 });
                }
                self.patch_here(at);
            }
        }
        if let FnKind::Method { is_ctor: true, .. } = spec.kind {
            for (i, p) in spec.params.iter().enumerate() {
                if params[i].promote.is_some() {
                    let name = self.str_const(p.name.name);
                    let cache = self.cache();
                    self.emit(Op::AssignProp {
                        obj: THIS,
                        name,
                        val: i as u32,
                        cache,
                    });
                }
            }
        }
        // Body.
        let line_end = spec.span.end_line;
        match &spec.body {
            FnBody::Block(b) => {
                self.block(b)?;
                self.f.line = line_end;
                let n = self.null_const();
                self.emit_return(n, true);
            }
            FnBody::Expr(e) => {
                self.f.line = e.span.line;
                if spec.by_ref {
                    self.nullsafe_ref_check(e)?;
                }
                if spec.by_ref && super::is_ref_capable(e) {
                    let r = self.ref_of(e)?;
                    self.free(r);
                    self.emit(Op::ReturnRef { src: r });
                } else {
                    let v = self.expr(e)?;
                    self.free(v);
                    if self.f.flags & fflags::RETURNS_VOID != 0 && !is_generator {
                        let n = self.null_const();
                        self.emit(Op::Return { src: n });
                    } else {
                        self.emit_return(v, false);
                    }
                }
            }
            FnBody::None => {
                let n = self.null_const();
                self.emit(Op::Return { src: n });
            }
        }
        let attrs = self.attributes(spec.attrs)?;
        let target = if self.f.flags & fflags::CLOSURE == 0 && self.class.is_some() {
            4
        } else {
            2
        };
        self.check_internal_attrs(&attrs, target, spec.span.line)?;
        let mut code = self.finish_func(
            spec.name.clone(),
            params,
            num_required,
            ret_type,
            spec.span.line,
            line_end,
        )?;
        code.doc_comment = spec.doc.map(PhpStr::new);
        code.attrs = attrs;
        if code
            .attrs
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(b"Deprecated"))
        {
            code.flags |= fflags::DEPRECATED;
        }
        if code
            .attrs
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(b"NoDiscard"))
        {
            // RFC: Marking return values as important (#[\NoDiscard]).
            let line = spec.span.line;
            if spec.prop_name.is_some() {
                return self.err("#[\\NoDiscard] is not supported for property hooks", line);
            }
            let ln = spec.name.to_ascii_lowercase();
            if matches!(spec.kind, FnKind::Method { .. })
                && matches!(&ln[..], b"__construct" | b"__clone")
            {
                return self.err(
                    format!(
                        "Method {} cannot be #[\\NoDiscard]",
                        spec.method_name.to_string_lossy()
                    ),
                    line,
                );
            }
            if let Some(t) = &code.ret_type {
                if t.mask & tmask::VOID != 0 {
                    return self.err(
                        "A void function does not return a value, but #[\\NoDiscard] requires a return value",
                        line,
                    );
                }
                if t.mask & tmask::NEVER != 0 {
                    return self.err(
                        "A never returning function does not return a value, but #[\\NoDiscard] requires a return value",
                        line,
                    );
                }
            }
            code.flags |= fflags::NODISCARD;
        }
        Ok(code)
    }

    fn check_generator_return(&self, th: &TypeHint, line: u32) -> CResult<()> {
        let ok_mask = tmask::ITERABLE | tmask::MIXED | tmask::OBJECT;
        let mut ok = th.mask & ok_mask != 0;
        for c in &th.classes {
            if c.len() == 1 {
                let l = c[0].to_string_lossy().to_ascii_lowercase();
                if matches!(
                    l.as_str(),
                    "generator" | "iterator" | "traversable" | "\\generator"
                ) {
                    ok = true;
                }
            }
        }
        if !ok {
            return self.err(
                format!(
                    "Generator return type must be a supertype of Generator, {} given",
                    th.display()
                ),
                line,
            );
        }
        Ok(())
    }

    fn params(&mut self, spec: &FnSpec<'a>) -> CResult<(Vec<ParamInfo>, u32)> {
        let mut out = Vec::new();
        let n = spec.params.len();
        // Number of required parameters: up to the last one without a
        // default (and not variadic).
        let mut last_required = 0;
        for (i, p) in spec.params.iter().enumerate() {
            if p.default.is_none() && !p.variadic {
                last_required = i + 1;
            }
        }
        for (i, p) in spec.params.iter().enumerate() {
            if p.variadic && i + 1 != n {
                return self.err("Only the last parameter can be variadic", p.span.line);
            }
            if p.variadic && p.default.is_some() {
                return self.err(
                    "Variadic parameter cannot have a default value",
                    p.span.line,
                );
            }
            let mut ty = match &p.ty {
                Some(t) => Some(self.type_hint(t, false)?),
                None => None,
            };
            let default = match &p.default {
                None => ParamDefault::None,
                Some(d) => match self.const_value(d) {
                    Some(v) => ParamDefault::Const(v),
                    None => {
                        self.check_const_expr(d)?;
                        ParamDefault::Code
                    }
                },
            };
            let is_null_default = matches!(&default, ParamDefault::Const(Value::Null));
            // A type made nullable only by its null default (an explicit
            // ?T with null before a required parameter is just optional:
            // test ext/reflection/tests/bug62715.phpt).
            let implicit_null = is_null_default && ty.as_ref().is_some_and(|t| !t.allows_null());
            if let (Some(t), true) = (&mut ty, is_null_default)
                && !t.allows_null()
            {
                // Implicitly nullable (deprecated since PHP 8.4).
                if i >= last_required {
                    self.notice(
                        goro_runtime::Level::Deprecated,
                        format!(
                            "{}(): Implicitly marking parameter ${} as nullable is deprecated, the explicit nullable type must be used instead",
                            self.f.method_name.to_string_lossy(),
                            String::from_utf8_lossy(p.name.name)
                        ),
                        p.span.line,
                    );
                }
                t.mask |= tmask::NULL;
            }
            if p.default.is_some() && i < last_required && !p.variadic {
                let rname = spec.params[last_required - 1].name.name;
                if !implicit_null {
                    self.notice(
                        goro_runtime::Level::Deprecated,
                        format!(
                            "{}(): Optional parameter ${} declared before required parameter ${} is implicitly treated as a required parameter",
                            self.f.method_name.to_string_lossy(),
                            String::from_utf8_lossy(p.name.name),
                            String::from_utf8_lossy(rname)
                        ),
                        p.span.line,
                    );
                } else {
                    self.notice(
                        goro_runtime::Level::Deprecated,
                        format!(
                            "{}(): Implicitly marking parameter ${} as nullable is deprecated, the explicit nullable type must be used instead",
                            self.f.method_name.to_string_lossy(),
                            String::from_utf8_lossy(p.name.name)
                        ),
                        p.span.line,
                    );
                }
            }
            if let (Some(t), ParamDefault::Const(v)) = (&ty, &default)
                && !const_fits_type(t, v)
            {
                return self.err(
                    format!(
                        "Cannot use {} as default value for parameter ${} of type {}",
                        type_word(v),
                        String::from_utf8_lossy(p.name.name),
                        t.display()
                    ),
                    p.span.line,
                );
            }
            if let Some(t) = &ty
                && t.mask & tmask::VOID != 0
            {
                return self.err("void cannot be used as a parameter type", p.span.line);
            }
            if let Some(t) = &ty
                && t.mask & tmask::NEVER != 0
            {
                return self.err("never cannot be used as a parameter type", p.span.line);
            }
            let promote = if p.modifiers.is_empty() {
                None
            } else {
                match spec.kind {
                    FnKind::Method { is_ctor: true, .. } => {}
                    _ => {
                        return self.err(
                            "Cannot declare promoted property outside a constructor",
                            p.span.line,
                        );
                    }
                }
                if p.variadic {
                    return self.err("Cannot declare variadic promoted property", p.span.line);
                }
                Some(p.modifiers.0)
            };
            let default_text = p
                .default
                .as_ref()
                .map(|d| self.resolved_source_text(d.span.start as usize, d.span.end as usize));
            let attrs = self.attributes(p.attrs)?;
            // Promoted parameters are properties too.
            let t = if p.modifiers.0 != 0 { 32 | 8 } else { 32 };
            self.check_internal_attrs(&attrs, t, p.span.line)?;
            let default_ce = if matches!(default, ParamDefault::Code) {
                Some(self.const_expr(p.default.as_ref().unwrap())?)
            } else {
                None
            };
            out.push(ParamInfo {
                name: PhpStr::intern(p.name.name),
                by_ref: p.by_ref,
                variadic: p.variadic,
                ty,
                default,
                default_text,
                promote,
                attrs,
                default_ce,
            });
        }
        if spec.params.iter().any(|p| p.variadic) {
            self.f.flags |= fflags::VARIADIC;
        }
        Ok((out, last_required as u32))
    }

    // ------------------------------------------------------------ types

    pub(crate) fn type_hint(&mut self, t: &Type<'a>, is_return: bool) -> CResult<TypeHint> {
        let mut th = TypeHint::default();
        self.type_into(t, &mut th, is_return, false)?;
        if th.mask & tmask::MIXED != 0 && (th.mask & !tmask::MIXED != 0 || !th.classes.is_empty()) {
            if th.mask & tmask::NULL != 0 && matches!(t, Type::Nullable(..)) {
                return self.err(
                    "Type mixed cannot be marked as nullable since mixed already includes null",
                    t.span().line,
                );
            }
            return self.err(
                "Type mixed can only be used as a standalone type",
                t.span().line,
            );
        }
        if th.mask & tmask::VOID != 0 && (th.mask != tmask::VOID || !th.classes.is_empty()) {
            if matches!(t, Type::Nullable(..)) {
                return self.err("Void can only be used as a standalone type", t.span().line);
            }
            return self.err("Void can only be used as a standalone type", t.span().line);
        }
        if th.mask & tmask::NEVER != 0 && (th.mask != tmask::NEVER || !th.classes.is_empty()) {
            return self.err("never can only be used as a standalone type", t.span().line);
        }
        if th.mask == tmask::NULL && th.classes.is_empty() && !matches!(t, Type::Named(_)) {
            // ?null
        }
        Ok(th)
    }

    fn type_into(
        &mut self,
        t: &Type<'a>,
        th: &mut TypeHint,
        is_return: bool,
        in_union: bool,
    ) -> CResult<()> {
        match t {
            Type::Named(n) => {
                let lname = n.name.to_ascii_lowercase();
                let bit = if n.kind == NameKind::Unqualified {
                    match &lname[..] {
                        b"int" => tmask::INT,
                        b"float" => tmask::FLOAT,
                        b"string" => tmask::STRING,
                        b"bool" => tmask::BOOL,
                        b"false" => tmask::FALSE,
                        b"true" => tmask::TRUE,
                        b"null" => tmask::NULL,
                        b"void" => {
                            if !is_return {
                                return self
                                    .err("void cannot be used as a parameter type", n.span.line);
                            }
                            tmask::VOID
                        }
                        b"never" => {
                            if !is_return {
                                return self
                                    .err("never cannot be used as a parameter type", n.span.line);
                            }
                            tmask::NEVER
                        }
                        b"mixed" => tmask::MIXED,
                        b"object" => tmask::OBJECT,
                        b"iterable" => tmask::ITERABLE,
                        b"callable" => tmask::CALLABLE,
                        b"array" => tmask::ARRAY,
                        b"static" => tmask::STATIC,
                        _ => 0,
                    }
                } else {
                    0
                };
                if bit != 0 {
                    if th.mask & bit != 0 && in_union {
                        let name = if bit == tmask::BOOL {
                            "bool".to_string()
                        } else {
                            String::from_utf8_lossy(&lname).into_owned()
                        };
                        return self
                            .err(format!("Duplicate type {} is redundant", name), n.span.line);
                    }
                    if (bit == tmask::FALSE || bit == tmask::TRUE)
                        && th.mask & tmask::BOOL == tmask::BOOL
                    {
                        return self.err(
                            format!(
                                "Duplicate type {} is redundant",
                                String::from_utf8_lossy(&lname)
                            ),
                            n.span.line,
                        );
                    }
                    if (bit == tmask::FALSE && th.mask & tmask::TRUE != 0)
                        || (bit == tmask::TRUE && th.mask & tmask::FALSE != 0)
                    {
                        return self.err(
                            "Type contains both true and false, bool must be used instead",
                            n.span.line,
                        );
                    }
                    th.mask |= bit;
                    return Ok(());
                }
                let name: Vec<u8> = if n.kind == NameKind::Unqualified
                    && (lname == b"self" || lname == b"parent")
                {
                    if self.class.is_none() && self.f.flags & fflags::CLOSURE == 0 {
                        return self.err(
                            format!(
                                "Cannot use \"{}\" when no class scope is active",
                                String::from_utf8_lossy(&lname)
                            ),
                            n.span.line,
                        );
                    }
                    if lname == b"self" {
                        match &self.class {
                            Some(c) if !c.is_trait => c.name.to_vec(),
                            _ => b"self".to_vec(),
                        }
                    } else {
                        match &self.class {
                            Some(c) if !c.is_trait => match &c.parent {
                                Some(p) => p.to_vec(),
                                None => {
                                    return self.err(
                                        "Cannot use \"parent\" when current class scope has no parent",
                                        n.span.line,
                                    );
                                }
                            },
                            _ => b"parent".to_vec(),
                        }
                    }
                } else {
                    self.resolve_class(n)
                };
                let s = PhpStr::intern(&name);
                if th
                    .classes
                    .iter()
                    .any(|c| c.len() == 1 && c[0].eq_ignore_ascii_case(&s))
                {
                    return self.err(
                        format!("Duplicate type {} is redundant", s.to_string_lossy()),
                        n.span.line,
                    );
                }
                th.classes.push(vec![s]);
                Ok(())
            }
            Type::Array(_) => {
                th.mask |= tmask::ARRAY;
                Ok(())
            }
            Type::Callable(_) => {
                th.mask |= tmask::CALLABLE;
                Ok(())
            }
            Type::Static(sp) => {
                // test Zend/tests/type_declarations/static_type_outside_class
                if !is_return
                    || (self.class.is_none()
                        && !self.file.is_empty()
                        && self.f.flags & fflags::CLOSURE == 0)
                {
                    return self.err(
                        "Cannot use \"static\" when no class scope is active",
                        sp.line,
                    );
                }
                th.mask |= tmask::STATIC;
                Ok(())
            }
            Type::Nullable(inner, _) => {
                self.type_into(inner, th, is_return, false)?;
                if th.mask & tmask::NULL != 0
                    && matches!(inner, Type::Named(n) if n.name.eq_ignore_ascii_case(b"null"))
                {
                    return self.err("null cannot be marked as nullable", inner.span().line);
                }
                th.mask |= tmask::NULL;
                Ok(())
            }
            Type::Union(ts, _) => {
                for t in *ts {
                    match t {
                        Type::Intersection(parts, _) => {
                            let mut names = Vec::new();
                            for p in *parts {
                                let mut sub = TypeHint::default();
                                self.type_into(p, &mut sub, is_return, false)?;
                                names.extend(sub.classes.into_iter().flatten());
                            }
                            th.classes.push(names);
                        }
                        _ => self.type_into(t, th, is_return, true)?,
                    }
                }
                Ok(())
            }
            Type::Intersection(ts, sp) => {
                let mut names = Vec::new();
                for t in *ts {
                    let mut sub = TypeHint::default();
                    self.type_into(t, &mut sub, is_return, false)?;
                    if sub.mask != 0 {
                        let tn = sub.display();
                        return self.err(
                            format!("Type {} cannot be part of an intersection type", tn),
                            sp.line,
                        );
                    }
                    names.extend(sub.classes.into_iter().flatten());
                }
                th.classes.push(names);
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------ const expressions

    /// Class constant and property initializers take no `new` and no
    /// object casts (tests Zend/tests/constexpr/new_not_allowed_*,
    /// constant_expressions_cast_object_property).
    fn member_init_checks(&self, e: &'a Expr<'a>) -> CResult<()> {
        let mut bad: Option<(&'static str, u32)> = None;
        walk_expr(e, &mut |x| {
            if bad.is_some() {
                return;
            }
            match &x.kind {
                ExprKind::New { .. } => {
                    bad = Some((
                        "New expressions are not supported in this context",
                        x.span.line,
                    ))
                }
                ExprKind::Cast {
                    kind: CastKind::Object,
                    ..
                } => {
                    bad = Some((
                        "Object casts are not supported in this context",
                        x.span.line,
                    ))
                }
                _ => {}
            }
        });
        match bad {
            Some((m, l)) => self.err(m, l),
            None => Ok(()),
        }
    }

    /// Checks that `e` is a valid constant expression.
    pub(crate) fn check_const_expr(&self, e: &'a Expr<'a>) -> CResult<()> {
        // Errors PHP finds while compiling the expression come before the
        // constant-expression check (tests Zend/tests/bug70912.phpt,
        // ast/gh21072.phpt, constexpr/gh9138.phpt, class_name/*).
        let mut early: Option<(&'static str, u32)> = None;
        walk_expr(e, &mut |x| {
            if early.is_some() {
                return;
            }
            let msg = match &x.kind {
                ExprKind::Index { index: None, .. } => Some("Cannot use [] for reading"),
                ExprKind::Cast {
                    kind: CastKind::Unset,
                    ..
                } => Some("The (unset) cast is no longer supported"),
                ExprKind::Array { items, .. } if items.iter().any(|i| i.is_none()) => {
                    Some("Cannot use empty array elements in arrays")
                }
                ExprKind::ClassConst(cc)
                    if matches!(cc.class, ClassRef::Static(_))
                        && matches!(&cc.name, MemberName::Ident(i) if i.name.eq_ignore_ascii_case(b"class")) =>
                {
                    Some("static::class cannot be used for compile-time class name resolution")
                }
                ExprKind::StaticCall(sc) if matches!(sc.class, ClassRef::Static(_)) => {
                    Some("\"static\" is not allowed in compile-time constants")
                }
                // tests Zend/tests/constexpr/constant_expressions_*_class_name_error,
                // class_on_expression_in_constant_expression
                ExprKind::ClassConst(cc) if matches!(cc.class, ClassRef::Static(_)) => {
                    Some("\"static::\" is not allowed in compile-time constants")
                }
                ExprKind::ClassConst(cc) if matches!(cc.class, ClassRef::Expr(_)) => {
                    let is_class = matches!(&cc.name, MemberName::Ident(i) if i.name.eq_ignore_ascii_case(b"class"));
                    Some(if is_class {
                        "(expression)::class cannot be used in constant expressions"
                    } else {
                        "Dynamic class names are not allowed in compile-time class constant references"
                    })
                }
                ExprKind::New {
                    args: Some(Args::FirstClassCallable),
                    ..
                } => Some("Cannot create Closure for new expression"),
                // tests Zend/tests/first_class_callable/constexpr/error_dynamic_*
                ExprKind::CallExpr {
                    callee,
                    args: Args::FirstClassCallable,
                } => Some(match callee.kind {
                    ExprKind::Int(_) | ExprKind::Float(_) => "Illegal function name",
                    _ => "Cannot use dynamic function name in constant expression",
                }),
                _ => None,
            };
            if let Some(m) = msg {
                early = Some((m, x.span.line));
            }
        });
        if let Some((m, line)) = early {
            return self.err(m, line);
        }
        if !const_expr_ok(e) {
            return self.err(
                "Constant expression contains invalid operations",
                e.span.line,
            );
        }
        Ok(())
    }

    /// Compiles an initializer (constant, property default, parameter
    /// default, attribute argument).
    pub(crate) fn const_expr(&mut self, e: &'a Expr<'a>) -> CResult<ConstExpr> {
        if let Some(v) = self.const_value(e) {
            return Ok(ConstExpr::Value(v));
        }
        self.check_const_expr(e)?;
        let parent_f = std::mem::replace(&mut self.f, FState::new(Vec::new()));
        self.f.line = e.span.line;
        self.f.flags |= fflags::CONST_EXPR;
        self.f.func_name = parent_f.func_name.clone();
        self.f.method_name = parent_f.method_name.clone();
        self.in_const_expr = true;
        let r = (|| -> CResult<FuncCode> {
            let v = self.expr(e)?;
            self.free(v);
            self.emit(Op::Return { src: v });
            self.finish_func(
                PhpStr::intern(b"{const}"),
                Vec::new(),
                0,
                None,
                e.span.line,
                e.span.line,
            )
        })();
        self.in_const_expr = false;
        self.f = parent_f;
        Ok(ConstExpr::Code(Rc::new(r?)))
    }

    // ------------------------------------------------------------ attributes

    pub(crate) fn attributes(
        &mut self,
        groups: &'a [AttributeGroup<'a>],
    ) -> CResult<Rc<[AttrInfo]>> {
        let mut out = Vec::new();
        for g in groups {
            for a in g.attrs {
                let name = self.resolve_class(&a.name);
                let mut args = Vec::new();
                for arg in a.args {
                    if arg.unpack {
                        return self.err(
                            "Cannot use unpacking in attribute argument list",
                            arg.span.line,
                        );
                    }
                    let v = self.const_expr(&arg.value)?;
                    args.push((arg.name.map(|n| PhpStr::intern(n.name)), v));
                }
                out.push(AttrInfo {
                    name: PhpStr::intern(&name),
                    args,
                    line: a.span.line,
                });
            }
        }
        Ok(Rc::from(out))
    }

    // ------------------------------------------------------------ classes

    pub(crate) fn class_decl(&mut self, cd: &'a goro_syntax::ast::ClassDecl<'a>) -> CResult<u32> {
        let kind = match cd.kind {
            goro_syntax::ast::ClassKind::Class => ClassKind::Class,
            goro_syntax::ast::ClassKind::Interface => ClassKind::Interface,
            goro_syntax::ast::ClassKind::Trait => ClassKind::Trait,
            goro_syntax::ast::ClassKind::Enum => ClassKind::Enum,
        };
        let line = cd.span.line;
        // Names that can't be used (messages observed on PHP 8.5).
        let special = |n: &Name| {
            n.kind == NameKind::Unqualified
                && matches!(
                    &n.name.to_ascii_lowercase()[..],
                    b"self" | b"parent" | b"static"
                )
        };
        if let Some(e) = &cd.extends
            && special(e)
        {
            return self.err(
                format!(
                    "Cannot use \"{}\" as class name, as it is reserved",
                    String::from_utf8_lossy(e.name)
                ),
                line,
            );
        }
        for i in cd.implements.iter() {
            if special(i) {
                return self.err(
                    format!(
                        "Cannot use \"{}\" as interface name, as it is reserved",
                        String::from_utf8_lossy(i.name)
                    ),
                    line,
                );
            }
        }
        for m in cd.members.iter() {
            if let ClassMember::TraitUse(tu) = m {
                for t in tu.traits.iter() {
                    if special(t) {
                        return self.err(
                            format!(
                                "Cannot use \"{}\" as trait name, as it is reserved",
                                String::from_utf8_lossy(t.name)
                            ),
                            line,
                        );
                    }
                }
            }
        }
        let parent = cd.extends.as_ref().map(|n| self.resolve_class(n));
        let name: Vec<u8> = match &cd.name {
            Some(n) => {
                let l = n.name.to_ascii_lowercase();
                if matches!(&l[..], b"self" | b"parent" | b"static") || is_reserved_type_name(&l) {
                    let what = match kind {
                        ClassKind::Class => "a class",
                        ClassKind::Interface => "an interface",
                        ClassKind::Trait => "a trait",
                        ClassKind::Enum => "an enum",
                    };
                    return self.err(
                        format!(
                            "Cannot use \"{}\" as {} name as it is reserved",
                            String::from_utf8_lossy(n.name),
                            what
                        ),
                        line,
                    );
                }
                if n.name == b"_" {
                    // PHP 8.4 (RFC: Deprecations for PHP 8.4).
                    self.notice(
                        goro_runtime::Level::Deprecated,
                        format!(
                            "Using \"_\" as {} name is deprecated since 8.4",
                            match kind {
                                ClassKind::Interface => "an interface",
                                ClassKind::Trait => "a trait",
                                ClassKind::Enum => "an enum",
                                ClassKind::Class => "a class",
                            }
                        ),
                        line,
                    );
                }
                let full = self.qualify_pub(n.name);
                if let Some(imp) = self.use_class.get(&l)
                    && !imp.eq_ignore_ascii_case(&full)
                {
                    return self.err(
                        format!(
                            "Cannot redeclare class {} (previously declared as local import)",
                            String::from_utf8_lossy(&full)
                        ),
                        line,
                    );
                }
                self.declared_classes.insert(full.to_ascii_lowercase());
                full
            }
            None => {
                // "class@anonymous" + NUL + file:line$n (observed on PHP 8.5).
                let prefix: Vec<u8> = match (&parent, cd.implements.first()) {
                    (Some(p), _) => p.clone(),
                    (None, Some(i)) => self.resolve_class(i),
                    (None, None) => b"class".to_vec(),
                };
                let mut v = prefix;
                v.extend_from_slice(b"@anonymous\0");
                v.extend_from_slice(self.file.as_bytes());
                v.extend_from_slice(format!(":{}${:x}", line, super::next_rtd_key()).as_bytes());
                self.anon_count += 1;
                v
            }
        };
        let cname = PhpStr::intern(&name);
        let mut flags = 0;
        if cd.modifiers.has(Modifiers::ABSTRACT) {
            flags |= crate::class::cflags::ABSTRACT;
        }
        if cd.modifiers.has(Modifiers::FINAL) {
            flags |= crate::class::cflags::FINAL;
        }
        if cd.modifiers.has(Modifiers::READONLY) {
            flags |= crate::class::cflags::READONLY;
        }
        if cd.name.is_none() {
            flags |= crate::class::cflags::ANON;
        }
        match kind {
            ClassKind::Interface => flags |= crate::class::cflags::INTERFACE,
            ClassKind::Trait => flags |= crate::class::cflags::TRAIT,
            ClassKind::Enum => flags |= crate::class::cflags::ENUM | crate::class::cflags::FINAL,
            _ => {}
        }
        let interfaces: Vec<PhpStr> = cd
            .implements
            .iter()
            .map(|n| PhpStr::intern(&self.resolve_class(n)))
            .collect();
        let saved_class = self.class.replace(ClassCtx {
            name: cname.clone(),
            parent: parent.as_ref().map(|p| PhpStr::intern(p)),
            is_trait: kind == ClassKind::Trait,
        });
        let r = self.class_members(cd, kind, &cname, line);
        self.class = saved_class;
        let (consts, props, methods, traits) = r?;
        let enum_backing = match &cd.enum_backing {
            Some(t) => {
                let th = self.type_hint(t, false)?;
                if th.mask != tmask::INT && th.mask != tmask::STRING || !th.classes.is_empty() {
                    return self.err(
                        format!(
                            "Enum backing type must be int or string, {} given",
                            th.display()
                        ),
                        line,
                    );
                }
                Some(th.mask)
            }
            None => None,
        };
        let attrs = self.attributes(cd.attrs)?;
        self.check_internal_attrs(&attrs, 1, line)?;
        self.check_class_attrs(&attrs, kind, flags, &cname, line)?;
        let decl = ClassDecl {
            name: cname,
            kind,
            flags,
            parent: parent.map(|p| PhpStr::intern(&p)),
            interfaces,
            enum_backing,
            consts,
            props,
            methods,
            traits,
            attrs,
            doc: cd.doc_comment.map(PhpStr::new),
            file: self.file.clone(),
            line_start: cd.span.line,
            line_end: cd.span.end_line,
            declared: std::cell::Cell::new(false),
            run_declared: std::cell::Cell::new(false),
        };
        self.f.classes.push(Rc::new(decl));
        Ok(self.f.classes.len() as u32 - 1)
    }

    #[allow(clippy::type_complexity)]
    fn class_members(
        &mut self,
        cd: &'a goro_syntax::ast::ClassDecl<'a>,
        kind: ClassKind,
        cname: &PhpStr,
        _line: u32,
    ) -> CResult<(
        Vec<ConstDecl>,
        Vec<PropDecl>,
        Vec<MethodDecl>,
        Vec<TraitUseDecl>,
    )> {
        let mut consts = Vec::new();
        let mut props: Vec<PropDecl> = Vec::new();
        let mut methods: Vec<MethodDecl> = Vec::new();
        let mut traits = Vec::new();
        let cn = cname.to_string_lossy();
        let is_interface = kind == ClassKind::Interface;
        for m in cd.members {
            match m {
                ClassMember::Const(c) => {
                    let mut flags = vis_flags(c.modifiers.0);
                    if c.modifiers.has(Modifiers::FINAL) {
                        flags |= mflags::FINAL;
                    }
                    if is_interface && flags & mflags::PUBLIC == 0 {
                        return self.err(
                            format!(
                                "Access type for interface constant {}::{} must be public",
                                cn,
                                String::from_utf8_lossy(c.items[0].name.name)
                            ),
                            c.span.line,
                        );
                    }
                    if c.modifiers.has(Modifiers::PRIVATE) && c.modifiers.has(Modifiers::FINAL) {
                        return self.err(
                            format!("Private constant {}::{} cannot be final as it is not visible to other classes", cn, String::from_utf8_lossy(c.items[0].name.name)),
                            c.span.line,
                        );
                    }
                    // tests Zend/tests/type_declarations/typed_class_constants_type_error4-6
                    if let Some(Type::Named(n)) = &c.ty
                        && n.kind == NameKind::Unqualified
                        && matches!(
                            &n.name.to_ascii_lowercase()[..],
                            b"callable" | b"void" | b"never"
                        )
                    {
                        return self.err(
                            format!(
                                "Class constant {}::{} cannot have type {}",
                                cn,
                                String::from_utf8_lossy(c.items[0].name.name),
                                String::from_utf8_lossy(&n.name.to_ascii_lowercase())
                            ),
                            c.span.line,
                        );
                    }
                    // (static is a valid constant type)
                    let ty = match &c.ty {
                        Some(t) => Some(self.type_hint(t, true)?),
                        None => None,
                    };
                    let attrs = self.attributes(c.attrs)?;
                    self.check_internal_attrs(&attrs, 16, c.span.line)?;
                    for item in c.items {
                        if item.name.name.eq_ignore_ascii_case(b"class") {
                            return self.err(
                                "A class constant must not be called 'class'; it is reserved for class name fetching",
                                item.span.line,
                            );
                        }
                        if consts
                            .iter()
                            .any(|x: &ConstDecl| x.name.as_bytes() == item.name.name)
                        {
                            return self.err(
                                format!(
                                    "Cannot redefine class constant {}::{}",
                                    cn,
                                    String::from_utf8_lossy(item.name.name)
                                ),
                                item.span.line,
                            );
                        }
                        self.member_init_checks(&item.value)?;
                        let mut value = self.const_expr(&item.value)?;
                        // A literal of the wrong type; int widens to float
                        // (tests typed_class_constants_type_error1,
                        // multiple_constants, type_success1).
                        if let (Some(t), ConstExpr::Value(v)) = (&ty, &value) {
                            if let Value::Int(i) = v
                                && t.mask & tmask::FLOAT != 0
                                && t.mask & tmask::INT == 0
                            {
                                value = ConstExpr::Value(Value::Float(*i as f64));
                            } else if t.classes.is_empty()
                                && t.mask & tmask::STATIC == 0
                                && !const_fits_type(t, v)
                            {
                                return self.err(
                                    format!(
                                        "Cannot use {} as value for class constant {}::{} of type {}",
                                        type_word(v),
                                        cn,
                                        String::from_utf8_lossy(item.name.name),
                                        t.display()
                                    ),
                                    item.span.line,
                                );
                            }
                        }
                        consts.push(ConstDecl {
                            name: PhpStr::intern(item.name.name),
                            value,
                            flags,
                            ty: ty.clone(),
                            doc: item.doc_comment.or(c.doc_comment).map(PhpStr::new),
                            attrs: attrs.clone(),
                            line: item.span.line,
                        });
                    }
                }
                ClassMember::EnumCase(ec) => {
                    if kind != ClassKind::Enum {
                        return self.err("Case can only be used in enums", ec.span.line);
                    }
                    let value = match &ec.value {
                        Some(v) => {
                            if cd.enum_backing.is_none() {
                                return self.err(
                                    format!(
                                        "Case {} of non-backed enum {} must not have a value",
                                        String::from_utf8_lossy(ec.name.name),
                                        cn
                                    ),
                                    ec.span.line,
                                );
                            }
                            let ce = self.const_expr(v)?;
                            // Checked when the enum is first used (test
                            // Zend/tests/enum/backed-mismatch).
                            Some(ce)
                        }
                        None => {
                            if cd.enum_backing.is_some() {
                                return self.err(
                                    format!(
                                        "Case {} of backed enum {} must have a value",
                                        String::from_utf8_lossy(ec.name.name),
                                        cn
                                    ),
                                    ec.span.line,
                                );
                            }
                            None
                        }
                    };
                    let attrs = self.attributes(ec.attrs)?;
                    self.check_internal_attrs(&attrs, 16, ec.span.line)?;
                    consts.push(ConstDecl {
                        name: PhpStr::intern(ec.name.name),
                        value: match value {
                            Some(v) => v,
                            None => ConstExpr::Value(Value::Null),
                        },
                        flags: mflags::PUBLIC | mflags::CASE,
                        ty: None,
                        doc: ec.doc_comment.map(PhpStr::new),
                        attrs,
                        line: ec.span.line,
                    });
                }
                ClassMember::Property(p) => {
                    if is_interface && !p.props.iter().any(|it| it.has_hook_list) {
                        return self
                            .err("Interfaces may only include hooked properties", p.span.line);
                    }
                    if kind == ClassKind::Enum {
                        return self.err(
                            format!("Enum {} cannot include properties", cn),
                            p.span.line,
                        );
                    }
                    let mut flags = vis_flags(p.modifiers.0);
                    if p.modifiers.has(Modifiers::STATIC) {
                        flags |= mflags::STATIC;
                    }
                    if p.modifiers.has(Modifiers::READONLY) || cd.modifiers.has(Modifiers::READONLY)
                    {
                        flags |= mflags::READONLY;
                    }
                    if p.modifiers.has(Modifiers::FINAL) {
                        flags |= mflags::FINAL;
                    }
                    if p.modifiers.has(Modifiers::ABSTRACT) {
                        flags |= mflags::ABSTRACT;
                    }
                    flags |= set_vis_flags(p.modifiers.0);
                    let hooked = p.props.iter().any(|it| it.has_hook_list);
                    self.prop_modifier_checks(
                        &cn,
                        p.props[0].name.name,
                        flags,
                        hooked,
                        is_interface,
                        p.ty.is_some(),
                        p.span.line,
                    )?;
                    for it in p.props.iter() {
                        if it.has_hook_list && it.hooks.is_empty() {
                            return self.err("Property hook list must not be empty", it.span.line);
                        }
                    }
                    // tests typed_properties_017, typed_properties_109
                    if let Some(Type::Named(n)) = &p.ty
                        && n.kind == NameKind::Unqualified
                        && matches!(&n.name.to_ascii_lowercase()[..], b"void" | b"never")
                    {
                        return self.err(
                            format!(
                                "Property {}::${} cannot have type {}",
                                cn,
                                String::from_utf8_lossy(p.props[0].name.name),
                                String::from_utf8_lossy(&n.name.to_ascii_lowercase())
                            ),
                            p.span.line,
                        );
                    }
                    let ty = match &p.ty {
                        Some(t) => {
                            let th = self.type_hint(t, false)?;
                            if th.mask & tmask::CALLABLE != 0 {
                                return self.err(
                                    format!(
                                        "Property {}::${} cannot have type {}",
                                        cn,
                                        String::from_utf8_lossy(p.props[0].name.name),
                                        th.display()
                                    ),
                                    p.span.line,
                                );
                            }
                            flags |= mflags::TYPED;
                            Some(th)
                        }
                        None => None,
                    };
                    if flags & mflags::READONLY != 0 && ty.is_none() {
                        return self.err(
                            format!(
                                "Readonly property {}::${} must have type",
                                cn,
                                String::from_utf8_lossy(p.props[0].name.name)
                            ),
                            p.span.line,
                        );
                    }
                    if flags & mflags::READONLY != 0 && flags & mflags::STATIC != 0 {
                        return self.err(
                            format!(
                                "Static property {}::${} cannot be readonly",
                                cn,
                                String::from_utf8_lossy(p.props[0].name.name)
                            ),
                            p.span.line,
                        );
                    }
                    let attrs = self.attributes(p.attrs)?;
                    self.check_internal_attrs(&attrs, 8, p.span.line)?;
                    for (pi, it) in p.props.iter().enumerate() {
                        if props.iter().any(|x| x.name.as_bytes() == it.name.name) {
                            return self.err(
                                format!(
                                    "Cannot redeclare {}::${}",
                                    cn,
                                    String::from_utf8_lossy(it.name.name)
                                ),
                                it.span.line,
                            );
                        }
                        let default = match &it.default {
                            Some(d) => {
                                if flags & mflags::READONLY != 0 {
                                    return self.err(
                                        format!(
                                            "Readonly property {}::${} cannot have default value",
                                            cn,
                                            String::from_utf8_lossy(it.name.name)
                                        ),
                                        it.span.line,
                                    );
                                }
                                self.member_init_checks(d)?;
                                let ce = self.const_expr(d)?;
                                if let (Some(t), ConstExpr::Value(Value::Null)) = (&ty, &ce)
                                    && !t.allows_null()
                                    && !t.classes.iter().any(|g| g.len() > 1)
                                {
                                    // tests typed_properties_015, 049
                                    return self.err(
                                        format!(
                                            "Default value for property of type {} may not be null. Use the nullable type ?{} to allow null default value",
                                            t.display(),
                                            t.display()
                                        ),
                                        it.span.line,
                                    );
                                }
                                if let (Some(t), ConstExpr::Value(v)) = (&ty, &ce)
                                    && !const_fits_type(t, v)
                                {
                                    return self.err(
                                        format!(
                                            "Cannot use {} as default value for property {}::${} of type {}",
                                            type_word(v),
                                            cn,
                                            String::from_utf8_lossy(it.name.name),
                                            t.display()
                                        ),
                                        it.span.line,
                                    );
                                }
                                Some(ce)
                            }
                            None => None,
                        };
                        let pname = PhpStr::intern(it.name.name);
                        let (get, set, hook_flags) = self.hooks(
                            it.hooks,
                            &pname,
                            ty.as_ref(),
                            &cn,
                            flags,
                            is_interface,
                            it.span.line,
                            p.ty.as_ref(),
                        )?;
                        let mut f = flags;
                        if default.is_some() || (ty.is_none() && flags & mflags::STATIC == 0) {
                            f |= mflags::HAS_DEFAULT;
                        }
                        if it.has_hook_list {
                            // Virtual unless a hook refers to the backing value.
                            if !hooks_use_backing(it.hooks, it.name.name) {
                                f |= mflags::VIRTUAL;
                            }
                        }
                        self.virtual_asym_check(
                            &cn,
                            &pname,
                            f,
                            get.is_some(),
                            set.is_some(),
                            it.span.line,
                        )?;
                        if f & mflags::PRIVATE_SET != 0 {
                            // private(set) is implicitly final (RFC:
                            // Asymmetric Visibility v2).
                            f |= mflags::FINAL;
                        }
                        if self.file.is_empty() && pname.starts_with(b"__") {
                            f |= mflags::HIDDEN;
                        }
                        props.push(PropDecl {
                            name: pname,
                            default,
                            flags: f,
                            ty: ty.clone(),
                            get,
                            set,
                            hook_flags,
                            // A declaration's doc comment belongs to its
                            // first property (observed on PHP 8.5).
                            doc: it
                                .doc_comment
                                .or(if pi == 0 { p.doc_comment } else { None })
                                .map(PhpStr::new),
                            attrs: attrs.clone(),
                            line: it.span.line,
                        });
                    }
                }
                ClassMember::Method(md) => {
                    let lname = md.name.name.to_ascii_lowercase();
                    if methods.iter().any(|x: &MethodDecl| {
                        x.code.name.as_bytes().eq_ignore_ascii_case(md.name.name)
                    }) {
                        return self.err(
                            format!(
                                "Cannot redeclare {}::{}()",
                                cn,
                                String::from_utf8_lossy(md.name.name)
                            ),
                            md.span.line,
                        );
                    }
                    let mut flags = vis_flags(md.modifiers.0);
                    if md.modifiers.has(Modifiers::STATIC) {
                        flags |= mflags::STATIC;
                    }
                    if md.modifiers.has(Modifiers::ABSTRACT) {
                        flags |= mflags::ABSTRACT;
                    }
                    if md.modifiers.has(Modifiers::FINAL) {
                        flags |= mflags::FINAL;
                    }
                    let mname = String::from_utf8_lossy(md.name.name).into_owned();
                    if is_interface {
                        if flags & mflags::PUBLIC == 0 {
                            return self.err(
                                format!(
                                    "Access type for interface method {}::{}() must be public",
                                    cn, mname
                                ),
                                md.span.line,
                            );
                        }
                        if md.body.is_some() {
                            return self.err(
                                format!(
                                    "Interface function {}::{}() cannot contain body",
                                    cn, mname
                                ),
                                md.span.line,
                            );
                        }
                        if md.modifiers.has(Modifiers::FINAL) {
                            return self.err(
                                format!("Interface method {}::{}() must not be final", cn, mname),
                                md.span.line,
                            );
                        }
                        if md.modifiers.has(Modifiers::ABSTRACT) {
                            return self.err(
                                format!(
                                    "Interface method {}::{}() must not be abstract",
                                    cn, mname
                                ),
                                md.span.line,
                            );
                        }
                        flags |= mflags::ABSTRACT;
                    } else if flags & mflags::ABSTRACT != 0 {
                        if md.body.is_some() {
                            return self.err(
                                format!(
                                    "Abstract function {}::{}() cannot contain body",
                                    cn, mname
                                ),
                                md.span.line,
                            );
                        }
                        if flags & mflags::PRIVATE != 0 && kind != ClassKind::Trait {
                            return self.err(
                                format!(
                                    "Abstract function {}::{}() cannot be declared private",
                                    cn, mname
                                ),
                                md.span.line,
                            );
                        }
                        if flags & mflags::FINAL != 0 {
                            return self.err(
                                "Cannot use the final modifier on an abstract method",
                                md.span.line,
                            );
                        }
                        if kind == ClassKind::Class && !cd.modifiers.has(Modifiers::ABSTRACT) {
                            return self.err(
                                format!(
                                    "Class {} declares abstract method {}() and must therefore be declared abstract",
                                    cn, mname
                                ),
                                md.span.line,
                            );
                        }
                        if kind == ClassKind::Enum {
                            return self.err(
                                format!("Enum method {}::{}() must not be abstract", cn, mname),
                                md.span.line,
                            );
                        }
                    } else if md.body.is_none() {
                        return self.err(
                            format!("Non-abstract method {}::{}() must contain body", cn, mname),
                            md.span.line,
                        );
                    }
                    if flags & mflags::PRIVATE != 0
                        && flags & mflags::FINAL != 0
                        && lname != b"__construct"
                    {
                        self.notice(
                            goro_runtime::Level::Warning,
                            "Private methods cannot be final as they are never overridden by other classes",
                            md.span.line,
                        );
                    }
                    let is_ctor = lname == b"__construct";
                    self.check_magic_method(&cn, &lname, md, flags)?;
                    let full = format!("{}::{}", cn, mname);
                    let spec = FnSpec {
                        name: PhpStr::intern(md.name.name),
                        params: md.params,
                        ret: md.ret.as_ref(),
                        body: match md.body {
                            Some(b) => FnBody::Block(b),
                            None => FnBody::None,
                        },
                        by_ref: md.by_ref,
                        kind: FnKind::Method {
                            is_static: flags & mflags::STATIC != 0,
                            is_ctor,
                        },
                        uses: &[],
                        span: md.span,
                        doc: md.doc_comment,
                        attrs: md.attrs,
                        method_name: PhpStr::from(full),
                        func_name: PhpStr::intern(md.name.name),
                        prop_name: None,
                    };
                    if !is_ctor && md.params.iter().any(|p| !p.modifiers.is_empty()) {
                        return self.err(
                            "Cannot declare promoted property outside a constructor",
                            md.span.line,
                        );
                    }
                    if is_ctor {
                        for p in md.params {
                            if p.modifiers.is_empty() {
                                continue;
                            }
                            if flags & mflags::ABSTRACT != 0 || is_interface {
                                return self.err(
                                    "Cannot declare promoted property in an abstract constructor",
                                    p.span.line,
                                );
                            }
                            if props.iter().any(|x| x.name.as_bytes() == p.name.name) {
                                return self.err(
                                    format!(
                                        "Cannot redeclare {}::${}",
                                        cn,
                                        String::from_utf8_lossy(p.name.name)
                                    ),
                                    p.span.line,
                                );
                            }
                            let mut pf = vis_flags(p.modifiers.0) | mflags::PROMOTED;
                            if pf & mflags::VISIBILITY == 0 {
                                pf |= mflags::PUBLIC;
                            }
                            if p.modifiers.has(Modifiers::READONLY)
                                || cd.modifiers.has(Modifiers::READONLY)
                            {
                                pf |= mflags::READONLY;
                            }
                            pf |= set_vis_flags(p.modifiers.0);
                            if p.modifiers.has(Modifiers::FINAL) {
                                pf |= mflags::FINAL;
                            }
                            self.prop_modifier_checks(
                                &cn,
                                p.name.name,
                                pf,
                                !p.hooks.is_empty(),
                                false,
                                p.ty.is_some(),
                                p.span.line,
                            )?;
                            if pf & mflags::PRIVATE_SET != 0 {
                                pf |= mflags::FINAL;
                            }
                            let ty = match &p.ty {
                                Some(t) => {
                                    pf |= mflags::TYPED;
                                    let mut th = self.type_hint(t, false)?;
                                    if matches!(&p.default, Some(Expr { kind: ExprKind::Const(n), .. }) if n.name.eq_ignore_ascii_case(b"null"))
                                    {
                                        th.mask |= tmask::NULL;
                                    }
                                    Some(th)
                                }
                                None => {
                                    if pf & mflags::READONLY != 0 {
                                        return self.err(
                                            format!(
                                                "Readonly property {}::${} must have type",
                                                cn,
                                                String::from_utf8_lossy(p.name.name)
                                            ),
                                            p.span.line,
                                        );
                                    }
                                    pf |= mflags::HAS_DEFAULT;
                                    None
                                }
                            };
                            if let Some(t) = &ty
                                && t.mask & tmask::CALLABLE != 0
                            {
                                return self.err(
                                    format!(
                                        "Property {}::${} cannot have type {}",
                                        cn,
                                        String::from_utf8_lossy(p.name.name),
                                        t.display()
                                    ),
                                    p.span.line,
                                );
                            }
                            let pname = PhpStr::intern(p.name.name);
                            let (get, set, hook_flags) = self.hooks(
                                p.hooks,
                                &pname,
                                ty.as_ref(),
                                &cn,
                                pf,
                                false,
                                p.span.line,
                                p.ty.as_ref(),
                            )?;
                            let attrs = self.attributes(p.attrs)?;
                            props.push(PropDecl {
                                name: pname,
                                default: None,
                                flags: pf,
                                ty,
                                get,
                                set,
                                hook_flags,
                                doc: p.doc_comment.map(PhpStr::new),
                                attrs,
                                line: p.span.line,
                            });
                        }
                    }
                    let code = self.compile_fn(spec)?;
                    methods.push(MethodDecl {
                        code: Rc::new(code),
                        flags,
                    });
                }
                ClassMember::TraitUse(tu) => {
                    if is_interface {
                        return self.err(
                            format!(
                                "Cannot use traits inside of interfaces. {} is used in {}",
                                String::from_utf8_lossy(&self.resolve_class(&tu.traits[0])),
                                cn
                            ),
                            tu.span.line,
                        );
                    }
                    let names: Vec<PhpStr> = tu
                        .traits
                        .iter()
                        .map(|n| PhpStr::intern(&self.resolve_class(n)))
                        .collect();
                    let mut rules = Vec::new();
                    for a in tu.adaptations {
                        // tests Zend/tests/traits/static_in_trait_insteadof_*
                        let names: Vec<&Name<'a>> = match a {
                            TraitAdaptation::Precedence {
                                trait_, insteadof, ..
                            } => std::iter::once(trait_).chain(insteadof.iter()).collect(),
                            TraitAdaptation::Alias { trait_, .. } => trait_.iter().collect(),
                        };
                        if names.iter().any(|n| {
                            n.kind == NameKind::Unqualified
                                && n.name.eq_ignore_ascii_case(b"static")
                        }) {
                            return self.err(
                                "Cannot use \"static\" as trait name, as it is reserved",
                                tu.span.line,
                            );
                        }
                        match a {
                            TraitAdaptation::Precedence {
                                trait_,
                                method,
                                insteadof,
                                ..
                            } => rules.push(TraitRule::Insteadof {
                                trait_: PhpStr::intern(&self.resolve_class(trait_)),
                                method: PhpStr::intern(method.name),
                                instead: insteadof
                                    .iter()
                                    .map(|n| PhpStr::intern(&self.resolve_class(n)))
                                    .collect(),
                            }),
                            TraitAdaptation::Alias {
                                trait_,
                                method,
                                modifiers,
                                alias,
                                span,
                            } => {
                                let mut f = vis_flags(modifiers.0);
                                if modifiers.has(Modifiers::FINAL) {
                                    f |= mflags::FINAL;
                                }
                                if modifiers.has(Modifiers::STATIC) {
                                    return self
                                        .err("Cannot use 'static' as method modifier", span.line);
                                }
                                if modifiers.has(Modifiers::ABSTRACT) {
                                    return self.err(
                                        "Cannot use 'abstract' as method modifier",
                                        span.line,
                                    );
                                }
                                if let Some(al) = alias {
                                    let la = al.name.to_ascii_lowercase();
                                    if is_reserved_type_name(&la) && false {
                                        return self.err("reserved", span.line);
                                    }
                                }
                                rules.push(TraitRule::Alias {
                                    trait_: trait_
                                        .as_ref()
                                        .map(|n| PhpStr::intern(&self.resolve_class(n))),
                                    method: PhpStr::intern(method.name),
                                    flags: f,
                                    alias: alias.map(|a| PhpStr::intern(a.name)),
                                })
                            }
                        }
                    }
                    traits.push(TraitUseDecl {
                        traits: names,
                        rules,
                        line: tu.span.line,
                    });
                }
            }
        }
        Ok((consts, props, methods, traits))
    }

    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn hooks(
        &mut self,
        hooks: &'a [PropertyHook<'a>],
        pname: &PhpStr,
        ty: Option<&TypeHint>,
        cn: &str,
        pflags: u32,
        iface: bool,
        line: u32,
        ast_ty: Option<&'a Type<'a>>,
    ) -> CResult<(Option<Rc<FuncCode>>, Option<Rc<FuncCode>>, u32)> {
        let mut get = None;
        let mut set = None;
        let mut hflags = 0;
        for h in hooks {
            let hname = h.name.name.to_ascii_lowercase();
            let is_get = hname == b"get";
            if !is_get && hname != b"set" {
                return self.err(
                    format!(
                        "Unknown hook \"{}\" for property {}::${}, expected \"get\" or \"set\"",
                        String::from_utf8_lossy(h.name.name),
                        cn,
                        pname.to_string_lossy()
                    ),
                    h.span.line,
                );
            }
            if (is_get && get.is_some()) || (!is_get && set.is_some()) {
                return self.err(
                    format!(
                        "Cannot redeclare property hook \"{}\"",
                        String::from_utf8_lossy(&hname)
                    ),
                    h.span.line,
                );
            }
            if pflags & mflags::STATIC != 0 {
                return self.err("Cannot declare hooks for static property", h.span.line);
            }
            if pflags & mflags::READONLY != 0 {
                return self.err("Hooked properties cannot be readonly", h.span.line);
            }
            // Modifier and signature rules (tests Zend/tests/property_hooks/
            // invalid_*, interface_*, set_*, no_get_parameters).
            let abstract_hook = matches!(h.body, HookBody::Abstract);
            let abstract_prop = iface || pflags & mflags::ABSTRACT != 0;
            if !abstract_hook && iface {
                return self.err("Abstract property hook cannot have body", h.span.line);
            }
            let hfinal = h.modifiers.has(Modifiers::FINAL);
            if pflags & mflags::PRIVATE != 0 && hfinal {
                return self.err(
                    "Property hook cannot be both final and private",
                    h.span.line,
                );
            }
            if abstract_hook && hfinal && abstract_prop {
                return self.err(
                    "Property hook cannot be both abstract and final",
                    h.span.line,
                );
            }
            if pflags & mflags::PRIVATE != 0 {
                if abstract_hook {
                    return self.err(
                        "Property hook cannot be both abstract and private",
                        h.span.line,
                    );
                }
                if hfinal {
                    return self.err(
                        "Property hook cannot be both final and private",
                        h.span.line,
                    );
                }
            }
            if is_get && h.params.is_some() {
                return self.err(
                    format!(
                        "get hook of property {}::${} must not have a parameter list",
                        cn,
                        pname.to_string_lossy()
                    ),
                    h.span.line,
                );
            }
            if !is_get && let Some(ps) = h.params {
                if ps.len() != 1 {
                    return self.err(
                        format!(
                            "set hook of property {}::${} must accept exactly one parameters",
                            cn,
                            pname.to_string_lossy()
                        ),
                        h.span.line,
                    );
                }
                let p = &ps[0];
                let what = if p.default.is_some() {
                    Some("have a default value")
                } else if p.by_ref {
                    Some("be pass-by-reference")
                } else if p.variadic {
                    Some("be variadic")
                } else {
                    None
                };
                if let Some(w) = what {
                    return self.err(
                        format!(
                            "Parameter ${} of set hook {}::${} must not {}",
                            String::from_utf8_lossy(p.name.name),
                            cn,
                            pname.to_string_lossy(),
                            w
                        ),
                        p.span.line,
                    );
                }
            }
            let full = format!(
                "{}::${}::{}",
                cn,
                pname.to_string_lossy(),
                String::from_utf8_lossy(&hname)
            );
            let body = match &h.body {
                HookBody::Abstract => {
                    hflags |= if is_get { 1 } else { 2 };
                    FnBody::None
                }
                HookBody::Block(b) => FnBody::Block(b),
                HookBody::Expr(e) => FnBody::Expr(e),
            };
            // A short `set => expr` assigns the backing value.
            let params: &'a [Param<'a>] = match h.params {
                Some(p) => p,
                None => &[],
            };
            let spec_name = PhpStr::from(format!(
                "${}::{}",
                pname.to_string_lossy(),
                String::from_utf8_lossy(&hname)
            ));
            let spec = FnSpec {
                name: spec_name,
                params,
                // A get hook returns the property type (RFC: Property
                // hooks; test Zend/tests/property_hooks/get_type_check).
                ret: if is_get { ast_ty } else { None },
                body,
                by_ref: h.by_ref,
                kind: FnKind::Method {
                    is_static: false,
                    is_ctor: false,
                },
                uses: &[],
                span: h.span,
                doc: h.doc_comment,
                attrs: h.attrs,
                method_name: PhpStr::from(full),
                func_name: PhpStr::from(format!(
                    "${}::{}",
                    pname.to_string_lossy(),
                    String::from_utf8_lossy(&hname)
                )),
                prop_name: Some(pname.clone()),
            };
            let code = if !is_get && h.params.is_none() {
                // implicit `$value` parameter
                self.compile_set_hook(spec, pname, ty, matches!(h.body, HookBody::Expr(_)))?
            } else if !is_get && matches!(h.body, HookBody::Expr(_)) {
                self.compile_set_hook(spec, pname, ty, true)?
            } else {
                self.compile_fn(spec)?
            };
            if h.modifiers.has(Modifiers::FINAL) {
                // Recorded for Reflection (`{ final get; }`): bits 4/8.
                hflags |= if is_get { 4 } else { 8 };
            }
            if is_get {
                get = Some(Rc::new(code));
            } else {
                set = Some(Rc::new(code));
            }
        }
        if pflags & mflags::ABSTRACT != 0 && hflags & 3 == 0 {
            return self.err(
                format!(
                    "Abstract property {}::${} must specify at least one abstract hook",
                    cn,
                    pname.to_string_lossy()
                ),
                line,
            );
        }
        Ok((get, set, hflags))
    }

    /// Rules on a property's modifiers (tests Zend/tests/property_hooks/
    /// interface_*, abstract_prop_without_hooks, final_private_prop;
    /// Zend/tests/asymmetric_visibility/*).
    #[allow(clippy::too_many_arguments)]
    fn prop_modifier_checks(
        &self,
        cn: &str,
        name: &[u8],
        flags: u32,
        hooked: bool,
        iface: bool,
        typed: bool,
        line: u32,
    ) -> CResult<()> {
        if iface {
            if flags & mflags::ABSTRACT != 0 {
                return self.err("Property in interface cannot be explicitly abstract. All interface members are implicitly abstract", line);
            }
            if flags & mflags::FINAL != 0 {
                return self.err("Property in interface cannot be final", line);
            }
            if flags & (mflags::PROTECTED | mflags::PRIVATE) != 0 {
                return self.err("Property in interface cannot be protected or private", line);
            }
        }
        if flags & mflags::ABSTRACT != 0 && !hooked {
            return self.err("Only hooked properties may be declared abstract", line);
        }
        if flags & mflags::ABSTRACT != 0 && flags & mflags::FINAL != 0 {
            return self.err(
                "Cannot use the final modifier on an abstract property",
                line,
            );
        }
        if flags & mflags::FINAL != 0 && flags & mflags::PRIVATE != 0 {
            return self.err("Property cannot be both final and private", line);
        }
        if flags & mflags::SET_VISIBILITY != 0 {
            if !typed {
                return self.err(
                    format!(
                        "Property with asymmetric visibility {}::${} must have type",
                        cn,
                        String::from_utf8_lossy(name)
                    ),
                    line,
                );
            }
            let get_rank = vis_rank3(flags & mflags::VISIBILITY);
            let set_rank = if flags & mflags::PRIVATE_SET != 0 {
                2
            } else if flags & mflags::PROTECTED_SET != 0 {
                1
            } else {
                0
            };
            if set_rank < get_rank {
                return self.err(
                    format!(
                        "Visibility of property {}::${} must not be weaker than set visibility",
                        cn,
                        String::from_utf8_lossy(name)
                    ),
                    line,
                );
            }
        }
        Ok(())
    }

    fn virtual_asym_check(
        &self,
        cn: &str,
        name: &PhpStr,
        flags: u32,
        get: bool,
        set: bool,
        line: u32,
    ) -> CResult<()> {
        if flags & mflags::VIRTUAL != 0 && flags & mflags::SET_VISIBILITY != 0 && get != set {
            return self.err(
                format!(
                    "{} virtual property {}::${} must not specify asymmetric visibility",
                    if get { "Read-only" } else { "Write-only" },
                    cn,
                    name.to_string_lossy()
                ),
                line,
            );
        }
        Ok(())
    }

    /// A `set` hook without a parameter list gets `$value` of the
    /// property type; `set => expr` assigns the expression to the backing
    /// property.
    fn compile_set_hook(
        &mut self,
        spec: FnSpec<'a>,
        pname: &PhpStr,
        ty: Option<&TypeHint>,
        expr_body: bool,
    ) -> CResult<FuncCode> {
        let mut sc = Scan::default();
        let has_params = !spec.params.is_empty();
        if !has_params {
            sc.add(b"value");
        } else {
            for p in spec.params {
                sc.add(p.name.name);
            }
        }
        match &spec.body {
            FnBody::Block(b) => sc.stmts(b),
            FnBody::Expr(e) => sc.expr(e),
            FnBody::None => {}
        }
        let parent_f = std::mem::replace(&mut self.f, FState::new(sc.vars));
        self.f.line = spec.span.line;
        self.f.func_name = spec.func_name.clone();
        self.f.method_name = spec.method_name.clone();
        self.f.prop_name = Some(pname.clone());
        self.f.in_class_method = true;
        self.f.flags |= fflags::USES_THIS;
        let r = (|| -> CResult<FuncCode> {
            let params = if has_params {
                self.params(&spec)?.0
            } else {
                vec![ParamInfo {
                    name: PhpStr::intern(b"value"),
                    by_ref: false,
                    variadic: false,
                    ty: ty.cloned(),
                    default: ParamDefault::None,
                    default_text: None,
                    promote: None,
                    attrs: Rc::from(Vec::new()),
                    default_ce: None,
                }]
            };
            match &spec.body {
                FnBody::Expr(e) if expr_body => {
                    let v = self.expr(e)?;
                    let name = self.str_const(pname.as_bytes());
                    self.free(v);
                    let cache = self.cache();
                    self.emit(Op::AssignProp {
                        obj: THIS,
                        name,
                        val: v,
                        cache,
                    });
                }
                FnBody::Block(b) => self.block(b)?,
                FnBody::Expr(e) => {
                    let v = self.expr(e)?;
                    self.free(v);
                }
                FnBody::None => {}
            }
            let n = self.null_const();
            self.emit(Op::Return { src: n });
            self.finish_func(
                spec.name.clone(),
                params,
                1,
                None,
                spec.span.line,
                spec.span.end_line,
            )
        })();
        self.f = parent_f;
        let mut c = r?;
        if matches!(spec.body, FnBody::None) {
            c.flags |= fflags::ABSTRACT;
        }
        Ok(c)
    }

    fn check_magic_method(
        &mut self,
        cn: &str,
        lname: &[u8],
        md: &MethodDecl2<'a>,
        flags: u32,
    ) -> CResult<()> {
        let mname = String::from_utf8_lossy(md.name.name);
        let nparams = md.params.len();
        let want: Option<(usize, bool)> = match lname {
            b"__get" | b"__isset" | b"__unset" => Some((1, false)),
            b"__set" => Some((2, false)),
            b"__call" => Some((2, false)),
            b"__callstatic" => Some((2, true)),
            b"__tostring" | b"__clone" | b"__destruct" | b"__sleep" | b"__wakeup"
            | b"__serialize" | b"__debuginfo" => Some((0, false)),
            b"__unserialize" => Some((1, false)),
            b"__set_state" => Some((1, true)),
            _ => None,
        };
        if let Some((n, must_static)) = want {
            if nparams != n {
                let what = match n {
                    0 => "cannot take arguments".to_string(),
                    1 => "must take exactly 1 argument".to_string(),
                    _ => format!("must take exactly {} arguments", n),
                };
                return self.err(format!("Method {}::{}() {}", cn, mname, what), md.span.line);
            }
            if must_static && flags & mflags::STATIC == 0 {
                return self.err(
                    format!("Method {}::{}() must be static", cn, mname),
                    md.span.line,
                );
            }
            if !must_static && flags & mflags::STATIC != 0 && lname != b"__construct" {
                return self.err(
                    format!("Method {}::{}() cannot be static", cn, mname),
                    md.span.line,
                );
            }

            if md.params.iter().any(|p| p.by_ref)
                && matches!(
                    lname,
                    b"__get" | b"__set" | b"__isset" | b"__unset" | b"__call" | b"__callstatic"
                )
            {
                return self.err(
                    format!(
                        "Method {}::{}() cannot take arguments by reference",
                        cn, mname
                    ),
                    md.span.line,
                );
            }
        }
        if matches!(lname, b"__construct" | b"__invoke") && flags & mflags::STATIC != 0 {
            return self.err(
                format!("Method {}::{}() cannot be static", cn, mname),
                md.span.line,
            );
        }
        // Visibility, parameter and return types of magic methods (tests
        // Zend/tests/magic_methods/magic_methods_0*, return_types/0*).
        let magic = matches!(
            lname,
            b"__get"
                | b"__set"
                | b"__isset"
                | b"__unset"
                | b"__call"
                | b"__callstatic"
                | b"__invoke"
                | b"__tostring"
                | b"__debuginfo"
                | b"__serialize"
                | b"__unserialize"
                | b"__sleep"
                | b"__wakeup"
                | b"__set_state"
        );
        if magic && flags & mflags::PUBLIC == 0 {
            self.notice(
                goro_runtime::Level::Warning,
                format!(
                    "The magic method {}::{}() must have public visibility",
                    cn, mname
                ),
                md.span.line,
            );
        }
        let param_types: &[(usize, &str)] = match lname {
            b"__get" | b"__isset" | b"__unset" | b"__set" => &[(0, "string")],
            b"__call" | b"__callstatic" => &[(0, "string"), (1, "array")],
            b"__unserialize" | b"__set_state" => &[(0, "array")],
            _ => &[],
        };
        for &(i, want) in param_types {
            if let Some(p) = md.params.get(i)
                && let Some(t) = &p.ty
            {
                let th = self.type_hint(t, false)?;
                let bit = if want == "string" {
                    tmask::STRING
                } else {
                    tmask::ARRAY
                };
                if th.mask & (bit | tmask::MIXED) == 0 {
                    return self.err(
                        format!(
                            "{}::{}(): Parameter #{} (${}) must be of type {} when declared",
                            cn,
                            mname,
                            i + 1,
                            String::from_utf8_lossy(p.name.name),
                            want
                        ),
                        md.span.line,
                    );
                }
            }
        }
        if let Some(rt) = md.ret {
            if matches!(lname, b"__construct" | b"__destruct") {
                return self.err(
                    format!("Method {}::{}() cannot declare a return type", cn, mname),
                    md.span.line,
                );
            }
            let want: Option<&[&str]> = match lname {
                b"__set" | b"__unset" | b"__unserialize" | b"__wakeup" | b"__clone" => {
                    Some(&["void"])
                }
                b"__isset" => Some(&["bool"]),
                b"__tostring" => Some(&["string"]),
                b"__debuginfo" => Some(&["?array", "array"]),
                b"__serialize" | b"__sleep" => Some(&["array"]),
                b"__set_state" => Some(&["object"]),
                _ => None,
            };
            if let Some(w) = want {
                let th = self.type_hint(&rt, true)?;
                let shown = th.display();
                let ok = w.contains(&shown.as_str())
                    || th.mask & tmask::NEVER != 0
                    || (lname == b"__set_state"
                        && (th.mask & tmask::STATIC != 0 || !th.classes.is_empty())
                        && th.mask & !(tmask::STATIC | tmask::NULL) == 0);
                if !ok {
                    return self.err(
                        format!(
                            "{}::{}(): Return type must be {} when declared",
                            cn, mname, w[0]
                        ),
                        md.span.line,
                    );
                }
            }
        }
        if lname == b"__destruct" && flags & mflags::STATIC != 0 {
            return self.err(
                format!("Method {}::{}() cannot be static", cn, mname),
                md.span.line,
            );
        }
        Ok(())
    }
}

type MethodDecl2<'a> = goro_syntax::ast::MethodDecl<'a>;

fn vis_flags(m: u16) -> u32 {
    let mut f = 0;
    if m & Modifiers::PUBLIC != 0 {
        f |= mflags::PUBLIC;
    }
    if m & Modifiers::PROTECTED != 0 {
        f |= mflags::PROTECTED;
    }
    if m & Modifiers::PRIVATE != 0 {
        f |= mflags::PRIVATE;
    }
    if f == 0 {
        f = mflags::PUBLIC;
    }
    f
}

fn set_vis_flags(m: u16) -> u32 {
    let mut f = 0;
    if m & Modifiers::PUBLIC_SET != 0 {
        f |= mflags::PUBLIC_SET;
    }
    if m & Modifiers::PROTECTED_SET != 0 {
        f |= mflags::PROTECTED_SET;
    }
    if m & Modifiers::PRIVATE_SET != 0 {
        f |= mflags::PRIVATE_SET;
    }
    f
}

pub(crate) fn is_reserved_type_name_pub(l: &[u8]) -> bool {
    is_reserved_type_name(l)
}

fn is_reserved_type_name(l: &[u8]) -> bool {
    matches!(
        l,
        b"int"
            | b"float"
            | b"bool"
            | b"string"
            | b"true"
            | b"false"
            | b"null"
            | b"void"
            | b"iterable"
            | b"object"
            | b"mixed"
            | b"never"
            | b"array"
            | b"callable"
    )
}

/// Whether a hook body refers to `$this->name` (making the property
/// backed). PHP: a property is virtual when no hook uses the backing value.
/// 0 public, 1 protected, 2 private.
fn vis_rank3(v: u32) -> u8 {
    if v & mflags::PRIVATE != 0 {
        2
    } else if v & mflags::PROTECTED != 0 {
        1
    } else {
        0
    }
}

fn hooks_use_backing(hooks: &[PropertyHook<'_>], name: &[u8]) -> bool {
    fn expr_uses(e: &Expr<'_>, name: &[u8]) -> bool {
        let mut found = false;
        walk_expr(e, &mut |x| {
            if let ExprKind::Prop {
                obj,
                name: MemberName::Ident(i),
                ..
            } = &x.kind
                && matches!(obj.kind, ExprKind::Variable(v) if v == b"this")
                && i.name == name
            {
                found = true;
            }
        });
        found
    }
    for h in hooks {
        match &h.body {
            HookBody::Abstract => {}
            HookBody::Expr(e) => {
                if h.name.name.eq_ignore_ascii_case(b"set") {
                    return true;
                }
                if expr_uses(e, name) {
                    return true;
                }
            }
            HookBody::Block(b) => {
                let mut found = false;
                walk_stmts(b, &mut |x| {
                    if expr_uses(x, name) {
                        found = true;
                    }
                });
                if found {
                    return true;
                }
            }
        }
    }
    // Only abstract hooks: not virtual (interfaces declare them).
    hooks.iter().all(|h| matches!(h.body, HookBody::Abstract)) && false
}

#[allow(clippy::redundant_closure)] // `f` is a &mut dyn FnMut taking &Expr, the items are &&Expr
fn walk_stmts<'a>(stmts: &'a [Stmt<'a>], f: &mut dyn FnMut(&'a Expr<'a>)) {
    for s in stmts {
        match &s.kind {
            StmtKind::Expr(e) => f(e),
            StmtKind::Echo(es) | StmtKind::Unset(es) => es.iter().for_each(|e| f(e)),
            StmtKind::Return(Some(e)) => f(e),
            StmtKind::Block(b) => walk_stmts(b, f),
            StmtKind::If(i) => {
                f(&i.cond);
                walk_stmts(i.then, f);
                for ei in i.elseifs {
                    f(&ei.cond);
                    walk_stmts(ei.body, f);
                }
                if let Some(e) = i.else_ {
                    walk_stmts(e, f);
                }
            }
            StmtKind::While { cond, body, .. } | StmtKind::DoWhile { body, cond } => {
                f(cond);
                walk_stmts(body, f);
            }
            StmtKind::For(fs) => {
                fs.init
                    .iter()
                    .chain(fs.cond)
                    .chain(fs.step)
                    .for_each(|e| f(e));
                walk_stmts(fs.body, f);
            }
            StmtKind::Foreach(fe) => {
                f(&fe.expr);
                walk_stmts(fe.body, f);
            }
            StmtKind::Switch(sw) => {
                f(&sw.subject);
                for c in sw.cases {
                    walk_stmts(c.body, f);
                }
            }
            StmtKind::Try(t) => {
                walk_stmts(t.body, f);
                for c in t.catches {
                    walk_stmts(c.body, f);
                }
                if let Some(b) = t.finally {
                    walk_stmts(b, f);
                }
            }
            _ => {}
        }
    }
}

fn walk_expr<'a>(e: &'a Expr<'a>, f: &mut dyn FnMut(&'a Expr<'a>)) {
    f(e);
    match &e.kind {
        ExprKind::Prop { obj, .. } => walk_expr(obj, f),
        ExprKind::Index { base, index } => {
            walk_expr(base, f);
            if let Some(i) = index {
                walk_expr(i, f);
            }
        }
        ExprKind::Assign { target, value }
        | ExprKind::AssignOp { target, value, .. }
        | ExprKind::AssignRef { target, value } => {
            walk_expr(target, f);
            walk_expr(value, f);
        }
        ExprKind::Binary { left, right, .. } => {
            walk_expr(left, f);
            walk_expr(right, f);
        }
        ExprKind::Unary { expr, .. }
        | ExprKind::Cast { expr, .. }
        | ExprKind::IncDec { expr, .. }
        | ExprKind::Empty(expr)
        | ExprKind::Print(expr)
        | ExprKind::Throw(expr) => walk_expr(expr, f),
        ExprKind::Ternary { cond, then, else_ } => {
            walk_expr(cond, f);
            if let Some(t) = then {
                walk_expr(t, f);
            }
            walk_expr(else_, f);
        }
        ExprKind::Call { args, .. }
        | ExprKind::New {
            args: Some(args), ..
        } => {
            for a in args.as_slice() {
                walk_expr(&a.value, f);
            }
        }
        ExprKind::MethodCall { obj, args, .. } => {
            walk_expr(obj, f);
            for a in args.as_slice() {
                walk_expr(&a.value, f);
            }
        }
        ExprKind::StaticCall(sc) => {
            for a in sc.args.as_slice() {
                walk_expr(&a.value, f);
            }
        }
        ExprKind::CallExpr { callee, args } => {
            walk_expr(callee, f);
            for a in args.as_slice() {
                walk_expr(&a.value, f);
            }
        }
        ExprKind::Isset(es) => es.iter().for_each(|x| walk_expr(x, f)),
        ExprKind::Interpolated(parts, _) => {
            for p in *parts {
                if let StrPart::Expr(x) = p {
                    walk_expr(x, f);
                }
            }
        }
        ExprKind::Array { items, .. } => {
            for it in items.iter().flatten() {
                walk_expr(&it.value, f);
            }
        }
        ExprKind::Match { subject, arms } => {
            walk_expr(subject, f);
            for a in *arms {
                for c in a.conds.iter().flat_map(|c| c.iter()) {
                    walk_expr(c, f);
                }
                walk_expr(&a.body, f);
            }
        }
        ExprKind::Instanceof { expr, .. }
        | ExprKind::Eval(expr)
        | ExprKind::Include { expr, .. }
        | ExprKind::YieldFrom(expr)
        | ExprKind::Clone {
            expr: Some(expr), ..
        } => walk_expr(expr, f),
        ExprKind::Yield { key, value } => {
            for x in [key, value].into_iter().flatten() {
                walk_expr(x, f);
            }
        }
        ExprKind::Exit(Some(args)) => {
            for a in args.as_slice() {
                walk_expr(&a.value, f);
            }
        }
        _ => {}
    }
}

/// Whether `e` may appear in a constant expression (PHP manual: "Constant
/// expressions").
fn const_expr_ok(e: &Expr<'_>) -> bool {
    match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(..)
        | ExprKind::MagicConst(_)
        | ExprKind::Const(_) => true,
        ExprKind::Interpolated(..) => false,
        ExprKind::ClassConst(cc) => {
            (match &cc.class {
                ClassRef::Name(_) => true,
                ClassRef::Static(_) => false,
                ClassRef::Expr(x) => const_expr_ok(x),
            }) && (match &cc.name {
                MemberName::Ident(_) => true,
                MemberName::Expr(x) => const_expr_ok(x),
            })
        }
        ExprKind::Array { items, .. } => items.iter().all(|it| match it {
            Some(it) => {
                !it.by_ref && it.key.as_ref().is_none_or(const_expr_ok) && const_expr_ok(&it.value)
            }
            None => false,
        }),
        ExprKind::Unary { op, expr } => *op != UnaryOp::Silence && const_expr_ok(expr),
        ExprKind::Binary { left, right, .. } => const_expr_ok(left) && const_expr_ok(right),
        ExprKind::Ternary { cond, then, else_ } => {
            const_expr_ok(cond) && then.is_none_or(|t| const_expr_ok(t)) && const_expr_ok(else_)
        }
        ExprKind::Index {
            base,
            index: Some(i),
        } => const_expr_ok(base) && const_expr_ok(i),
        ExprKind::Prop { obj, name, .. } => {
            const_expr_ok(obj)
                && match name {
                    MemberName::Ident(_) => true,
                    MemberName::Expr(x) => const_expr_ok(x),
                }
        }
        ExprKind::New {
            class: NewClass::Ref(ClassRef::Name(_)),
            args,
        } => match args {
            None => true,
            Some(Args::List(a)) => a.iter().all(|a| !a.unpack && const_expr_ok(&a.value)),
            Some(Args::FirstClassCallable) => false,
        },
        ExprKind::Cast { expr, kind } => {
            !matches!(kind, CastKind::Unset | CastKind::Void) && const_expr_ok(expr)
        }
        // PHP 8.5: closures and first-class callables in constant expressions.
        ExprKind::Closure(c) => c.is_static && c.uses.is_empty(),
        ExprKind::Call {
            args: Args::FirstClassCallable,
            ..
        } => true,
        ExprKind::StaticCall(sc) => {
            matches!(sc.args, Args::FirstClassCallable) && matches!(sc.class, ClassRef::Name(_))
        }
        _ => false,
    }
}

/// Whether a constant default value is accepted by a type (compile-time
/// check for parameter and property defaults).
pub(crate) fn const_fits_type(t: &TypeHint, v: &Value) -> bool {
    use tmask::*;
    let m = t.mask;
    if m & MIXED != 0 {
        return true;
    }
    match v {
        Value::Null => m & NULL != 0,
        Value::Bool(true) => m & TRUE != 0,
        Value::Bool(false) => m & FALSE != 0,
        Value::Int(_) => m & (INT | FLOAT) != 0,
        Value::Float(f) => m & FLOAT != 0 || (m & INT != 0 && f.fract() == 0.0 && false),
        Value::Str(_) => m & (STRING | CALLABLE) != 0,
        Value::Array(_) => m & (ARRAY | ITERABLE | CALLABLE) != 0,
        _ => true,
    }
}

fn type_word(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        _ => "object",
    }
}

/// Parses a native function signature (`function name(int $a = 1): int`)
/// into parameter descriptions, evaluating defaults that are literals.
pub fn parse_native_sig(
    sig: &str,
    known_const: Option<&dyn Fn(&[u8]) -> Option<Value>>,
) -> Result<crate::func::NativeSig, String> {
    let mut src = Vec::from(&b"<?php "[..]);
    // Reserved words (PHP 8.5's clone()) cannot be parsed as function
    // names: the name is not needed here.
    let sig_text = match (sig.find("function "), sig.find('(')) {
        (Some(f), Some(p))
            if f < p && crate::registry::sig_name(sig).eq_ignore_ascii_case("clone") =>
        {
            format!("{}function __clone_fn{}", &sig[..f], &sig[p..])
        }
        _ => sig.to_string(),
    };
    src.extend_from_slice(sig_text.as_bytes());
    src.extend_from_slice(b" {}");
    let arena = goro_syntax::Arena::new();
    let prog = goro_syntax::parse(&arena, &src).map_err(|e| format!("{}: {}", sig, e))?;
    let Some(Stmt {
        kind: StmtKind::Function(fd),
        ..
    }) = prog.stmts.first()
    else {
        return Err(format!("bad signature: {}", sig));
    };
    // SAFETY of lifetimes: everything compiled is owned (PhpStr/Value).
    let kc: Option<&dyn Fn(&[u8]) -> Option<Value>> = known_const;
    let mut c = Compiler {
        file: PhpStr::empty(),
        ns: Vec::new(),
        use_class: Default::default(),
        declared_classes: Default::default(),
        use_func: Default::default(),
        use_const: Default::default(),
        class: None,
        strict: false,
        f: FState::new(fd.params.iter().map(|p| p.name.name).collect()),
        notices: Vec::new(),
        anon_count: 0,
        src: &src,
        known_const: kc,
        ticks: 0,
        past_first_stmt: true,
        zend_multibyte: false,
        in_const_expr: false,
        in_chain: false,
        in_const_stmt: false,
        nullsafe_jumps: Vec::new(),
    };
    c.f.method_name = PhpStr::new(fd.name.name);
    let spec = FnSpec {
        name: PhpStr::new(fd.name.name),
        params: fd.params,
        ret: fd.ret.as_ref(),
        body: FnBody::None,
        by_ref: fd.by_ref,
        kind: FnKind::Function,
        uses: &[],
        span: fd.span,
        doc: None,
        attrs: &[],
        method_name: PhpStr::new(fd.name.name),
        func_name: PhpStr::new(fd.name.name),
        prop_name: None,
    };
    let (mut params, num_required) = c
        .params(&spec)
        .map_err(|e| format!("{}: {}", sig, e.message))?;
    for (i, p) in fd.params.iter().enumerate() {
        if matches!(params[i].default, ParamDefault::Code) {
            let ce = c
                .const_expr(p.default.as_ref().unwrap())
                .map_err(|e| e.message)?;
            params[i].default = ParamDefault::Lazy(ce);
        }
    }
    let ret = match &fd.ret {
        Some(t) => Some(c.type_hint(t, true).map_err(|e| e.message)?),
        None => None,
    };
    let ret_text = ret.as_ref().map(|t| t.display());
    let variadic = params.last().is_some_and(|p| p.variadic);
    Ok(crate::func::NativeSig {
        params,
        ret,
        num_required,
        by_ref_return: fd.by_ref,
        variadic,
        ret_text,
    })
}

impl<'a> Compiler<'a> {
    /// Source text of an expression with its class names resolved the way
    /// PHP's AST export shows them (gh9447: `Bar::BAZ` in namespace App is
    /// `\\App\\Bar::BAZ`, `new Qux` imported is `new \\SomewhereElse\\Qux`);
    /// names that resolve to themselves stay as written.
    pub(crate) fn resolved_source_text(&self, start: usize, end: usize) -> String {
        let src: &'a [u8] = self.src;
        let b = &src[start..end];
        let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80;
        let name_char = |c: u8| ident(c) || c == b'\\';
        let mut out: Vec<u8> = Vec::with_capacity(b.len());
        let mut i = 0;
        let mut after_new = false;
        while i < b.len() {
            let c = b[i];
            if c == b'\'' || c == b'"' {
                let st = i;
                i += 1;
                while i < b.len() && b[i] != c {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.extend_from_slice(&b[st..i]);
                after_new = false;
                continue;
            }
            let word_start = i == 0 || !(name_char(b[i - 1]) || b[i - 1] == b'$');
            if word_start && name_char(c) && !c.is_ascii_digit() {
                let st = i;
                while i < b.len() && name_char(b[i]) {
                    i += 1;
                }
                let word = &src[start + st..start + i];
                let mut k = i;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                let is_class_use = after_new || b[k..].starts_with(b"::");
                after_new = word.eq_ignore_ascii_case(b"new");
                if !is_class_use || word.eq_ignore_ascii_case(b"new") {
                    out.extend_from_slice(word);
                    continue;
                }
                let lower = word.to_ascii_lowercase();
                if matches!(&lower[..], b"self" | b"parent" | b"static") {
                    out.extend_from_slice(word);
                    continue;
                }
                let (kind, name) = if let Some(r) = word.strip_prefix(b"\\") {
                    (NameKind::FullyQualified, r)
                } else if lower.starts_with(b"namespace\\") {
                    (NameKind::Relative, &word[10..])
                } else if word.contains(&b'\\') {
                    (NameKind::Qualified, word)
                } else {
                    (NameKind::Unqualified, word)
                };
                let n = Name {
                    kind,
                    name,
                    span: Span {
                        start: (start + st) as u32,
                        end: (start + i) as u32,
                        line: 0,
                        end_line: 0,
                    },
                };
                let resolved = self.resolve_class(&n);
                if kind == NameKind::FullyQualified || resolved != word {
                    out.push(b'\\');
                    out.extend_from_slice(&resolved);
                } else {
                    out.extend_from_slice(word);
                }
                continue;
            }
            if !c.is_ascii_whitespace() {
                after_new = false;
            }
            out.push(c);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

/// Targets and display names of the internal attribute classes (PHP
/// manual: Predefined Attributes; targets as their errors list them on
/// PHP 8.5).
fn internal_attr(lname: &[u8]) -> Option<(&'static str, i64)> {
    Some(match lname {
        b"attribute" => ("Attribute", 1),
        b"returntypewillchange" => ("ReturnTypeWillChange", 4),
        b"allowdynamicproperties" => ("AllowDynamicProperties", 1),
        b"sensitiveparameter" => ("SensitiveParameter", 32),
        b"override" => ("Override", 4 | 8),
        b"deprecated" => ("Deprecated", 1 | 2 | 4 | 16 | 64),
        b"nodiscard" => ("NoDiscard", 2 | 4),
        b"delayedtargetvalidation" => ("DelayedTargetValidation", 127),
        _ => return None,
    })
}

fn attr_lname(a: &AttrInfo) -> Vec<u8> {
    a.name
        .strip_prefix(b"\\")
        .unwrap_or(&a.name)
        .to_ascii_lowercase()
}

pub(crate) fn attr_target_names(f: i64) -> String {
    let mut v = Vec::new();
    for (bit, n) in [
        (1, "class"),
        (2, "function"),
        (4, "method"),
        (8, "property"),
        (16, "class constant"),
        (32, "parameter"),
        (64, "constant"),
    ] {
        if f & bit != 0 {
            v.push(n);
        }
    }
    v.join(", ")
}

impl<'a> Compiler<'a> {
    /// Compile-time checks of internal attributes: their targets (unless
    /// #[\DelayedTargetValidation] is present, RFC: Delayed target
    /// validation) and that they are not repeated.
    pub(crate) fn check_internal_attrs(
        &mut self,
        attrs: &[AttrInfo],
        target: i64,
        line: u32,
    ) -> CResult<()> {
        let names: Vec<Vec<u8>> = attrs.iter().map(attr_lname).collect();
        let delayed = names.iter().any(|n| n == b"delayedtargetvalidation");
        for (i, n) in names.iter().enumerate() {
            let Some((shown, flags)) = internal_attr(n) else {
                continue;
            };
            if names[..i].contains(n) {
                return self.err(
                    format!("Attribute \"{}\" must not be repeated", shown),
                    attrs[i].line.max(line),
                );
            }
            if flags & target == 0 && !delayed {
                let tn = match target & !8 {
                    0 => "property",
                    1 => "class",
                    2 => "function",
                    4 => "method",
                    16 => "class constant",
                    32 => "parameter",
                    _ => "constant",
                };
                return self.err(
                    format!(
                        "Attribute \"{}\" cannot target {} (allowed targets: {})",
                        shown,
                        tn,
                        attr_target_names(flags)
                    ),
                    attrs[i].line.max(line),
                );
            }
        }
        Ok(())
    }

    /// Class-kind restrictions of #[\Attribute], #[\AllowDynamicProperties]
    /// and #[\Deprecated] (observed on PHP 8.5: "Cannot apply #[\Attribute]
    /// to interface Demo").
    fn check_class_attrs(
        &mut self,
        attrs: &[AttrInfo],
        kind: ClassKind,
        flags: u32,
        cname: &PhpStr,
        line: u32,
    ) -> CResult<()> {
        let names: Vec<Vec<u8>> = attrs.iter().map(attr_lname).collect();
        if names.iter().any(|n| n == b"delayedtargetvalidation") {
            return Ok(());
        }
        let kind_name = match kind {
            ClassKind::Class => "class",
            ClassKind::Interface => "interface",
            ClassKind::Trait => "trait",
            ClassKind::Enum => "enum",
        };
        for n in &names {
            let bad = match &n[..] {
                b"attribute" => {
                    if kind == ClassKind::Class && flags & crate::class::cflags::ABSTRACT != 0 {
                        Some(("Attribute", "abstract class"))
                    } else if kind != ClassKind::Class {
                        Some(("Attribute", kind_name))
                    } else {
                        None
                    }
                }
                b"allowdynamicproperties" if kind != ClassKind::Class => {
                    Some(("AllowDynamicProperties", kind_name))
                }
                b"deprecated" if kind != ClassKind::Trait => Some(("Deprecated", kind_name)),
                _ => None,
            };
            if let Some((a, k)) = bad {
                return self.err(
                    format!(
                        "Cannot apply #[\\{}] to {} {}",
                        a,
                        k,
                        cname.to_string_lossy()
                    ),
                    line,
                );
            }
        }
        Ok(())
    }
}
