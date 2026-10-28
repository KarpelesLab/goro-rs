//! Expressions, assignments, calls and write paths.

use goro_runtime::{Diagnostics, PhpArray, PhpStr, Value};
use goro_syntax::ast::*;

use super::scan::is_superglobal;
use super::{CResult, Compiler, is_ref_capable};
use crate::bytecode::*;

/// Where a write path starts and the steps after it.
struct LPath {
    base: PathBase,
    elems: Vec<PathElem>,
}

impl<'a> Compiler<'a> {
    // ------------------------------------------------------------ helpers

    pub(crate) fn class_spec(&mut self, c: &ClassRef<'a>) -> CResult<ClassSpec> {
        Ok(match c {
            ClassRef::Static(_) => ClassSpec::Static,
            ClassRef::Name(n) => {
                if n.kind == NameKind::Unqualified {
                    if n.name.eq_ignore_ascii_case(b"self") {
                        return Ok(self.self_spec());
                    }
                    if n.name.eq_ignore_ascii_case(b"parent") {
                        return Ok(ClassSpec::Parent);
                    }
                    if n.name.eq_ignore_ascii_case(b"static") {
                        return Ok(ClassSpec::Static);
                    }
                }
                let full = self.resolve_class(n);
                ClassSpec::Named(self.name_pair(&full))
            }
            ClassRef::Expr(e) => {
                let r = self.expr(e)?;
                ClassSpec::Dynamic(r)
            }
        })
    }

    fn self_spec(&mut self) -> ClassSpec {
        match &self.class {
            // Constant expressions keep `self` (their errors name it).
            Some(c)
                if !c.is_trait && self.f.flags & (fflags::CLOSURE | fflags::CONST_EXPR) == 0 =>
            {
                let n = c.name.clone();
                ClassSpec::Named(self.name_pair(&n))
            }
            _ => ClassSpec::SelfRef,
        }
    }

    fn free_spec(&mut self, s: ClassSpec) {
        if let ClassSpec::Dynamic(r) = s {
            self.free(r);
        }
    }

    fn member_name(&mut self, m: &MemberName<'a>) -> CResult<Reg> {
        Ok(match m {
            MemberName::Ident(i) => self.str_const(i.name),
            MemberName::Expr(e) => self.expr(e)?,
        })
    }

    /// Evaluates for side effects only.
    /// `(void) expr`: evaluated for side effects, its result deliberately
    /// ignored.
    pub(crate) fn expr_void(&mut self, e: &'a Expr<'a>) -> CResult<()> {
        let n = self.f.discarded_calls.len();
        self.expr_stmt(e)?;
        self.f.discarded_calls.truncate(n);
        Ok(())
    }

    pub(crate) fn expr_stmt(&mut self, e: &'a Expr<'a>) -> CResult<()> {
        self.f.line = e.span.line;
        match &e.kind {
            ExprKind::Assign { target, value } => self.assign(target, value, NONE),
            ExprKind::AssignOp { op, target, value } => {
                self.assign_op(*op, target, value, NONE).map(|_| ())
            }
            ExprKind::IncDec { op, expr } => self.incdec(*op, expr, NONE).map(|_| ()),
            ExprKind::AssignRef { target, value } => self.assign_ref(target, value, NONE),
            ExprKind::Cast {
                kind: CastKind::Void,
                expr,
            } => self.expr_void(expr),
            ExprKind::Call { .. }
            | ExprKind::CallExpr { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::StaticCall(_) => {
                let r = self.expr(e)?;
                // A call whose result goes unused (for #[\NoDiscard]).
                if let Some(Op::DoCall { .. }) = self.f.ops.last() {
                    let ip = (self.f.ops.len() - 1) as u32;
                    self.f.discarded_calls.push(ip);
                }
                if r != NONE && is_tmp(r) {
                    self.emit(Op::Free {
                        slot: r & SLOT_MASK,
                    });
                }
                self.free(r);
                Ok(())
            }
            _ => {
                let r = self.expr(e)?;
                if r != NONE && is_tmp(r) {
                    // Discard the result now (releases it at once).
                    self.emit(Op::Free {
                        slot: r & SLOT_MASK,
                    });
                }
                self.free(r);
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------ constant folding

    /// The value of a compile-time constant expression, if it is one.
    pub(crate) fn const_value(&mut self, e: &'a Expr<'a>) -> Option<Value> {
        match &e.kind {
            ExprKind::Int(i) => Some(Value::Int(*i)),
            ExprKind::Float(f) => Some(Value::Float(*f)),
            ExprKind::String(s, _) => Some(Value::Str(PhpStr::intern(&super::lit(s)))),
            ExprKind::Const(n) => self.const_name_value(n),
            ExprKind::MagicConst(m) => self.magic_value(*m, e.span.line),
            ExprKind::Unary { op, expr } => {
                let v = self.const_value(expr)?;
                let mut d = Diagnostics::new();
                let r = match op {
                    UnaryOp::Neg => goro_runtime::ops::neg(&v, &mut d).ok()?,
                    UnaryOp::Plus => goro_runtime::ops::plus(&v, &mut d).ok()?,
                    UnaryOp::Not => Value::Bool(goro_runtime::ops::not(&v, &mut d)),
                    UnaryOp::BitNot => goro_runtime::ops::bit_not(&v, &mut d).ok()?,
                    UnaryOp::Silence => return None,
                };
                if d.notices.is_empty() { Some(r) } else { None }
            }
            ExprKind::Binary { op, left, right } => {
                let a = self.const_value(left)?;
                let b = self.const_value(right)?;
                fold_binary(*op, &a, &b)
            }
            ExprKind::Ternary { cond, then, else_ } => {
                let c = self.const_value(cond)?;
                let c = goro_runtime::convert::to_bool_quiet(&c);
                match then {
                    Some(t) => {
                        if c {
                            self.const_value(t)
                        } else {
                            self.const_value(else_)
                        }
                    }
                    None => {
                        if c {
                            self.const_value(cond)
                        } else {
                            self.const_value(else_)
                        }
                    }
                }
            }
            ExprKind::Array { items, .. } => {
                let mut arr = PhpArray::new();
                for it in items.iter() {
                    let it = it.as_ref()?;
                    if it.by_ref || it.unpack {
                        return None;
                    }
                    let v = self.const_value(&it.value)?;
                    match &it.key {
                        Some(k) => {
                            let k = self.const_value(k)?;
                            let key = match &k {
                                Value::Int(i) => goro_runtime::Key::Int(*i),
                                Value::Str(s) => goro_runtime::Key::from_php_str(s),
                                Value::Bool(b) => goro_runtime::Key::Int(*b as i64),
                                _ => return None,
                            };
                            arr.insert(key, v);
                        }
                        None => {
                            arr.push(v).ok()?;
                        }
                    }
                }
                if !arr.is_empty() {
                    arr.data().mark_literal();
                }
                Some(Value::Array(arr))
            }
            _ => None,
        }
    }

    fn const_name_value(&self, n: &Name<'a>) -> Option<Value> {
        let base = if n.kind == NameKind::FullyQualified || n.kind == NameKind::Unqualified {
            n.name
        } else {
            return None;
        };
        if n.kind == NameKind::Unqualified && self.use_const.contains_key(base) {
            return None;
        }
        if base.eq_ignore_ascii_case(b"true") {
            return Some(Value::Bool(true));
        }
        if base.eq_ignore_ascii_case(b"false") {
            return Some(Value::Bool(false));
        }
        if base.eq_ignore_ascii_case(b"null") {
            return Some(Value::Null);
        }
        if base.contains(&b'\\') {
            return None;
        }
        if n.kind == NameKind::Unqualified && !self.ns.is_empty() {
            // A namespaced constant could shadow it.
            return None;
        }
        if let Some(k) = self.known_const {
            return k(base);
        }
        None
    }

    pub(crate) fn magic_value(&self, m: MagicConst, line: u32) -> Option<Value> {
        Some(match m {
            MagicConst::Line => Value::Int(line as i64),
            MagicConst::File => Value::Str(self.file.clone()),
            MagicConst::Dir => {
                let f = self.file.as_bytes();
                let d = match f.iter().rposition(|&c| c == b'/') {
                    Some(0) => &f[..1],
                    Some(i) => &f[..i],
                    None => b".",
                };
                Value::Str(PhpStr::new(d))
            }
            MagicConst::Function => Value::Str(self.f.func_name.clone()),
            MagicConst::Method => Value::Str(self.f.method_name.clone()),
            MagicConst::Namespace => Value::Str(PhpStr::new(&self.ns)),
            MagicConst::Class => match &self.class {
                Some(c) if c.is_trait => return None,
                Some(c) => {
                    if self.f.flags & fflags::CLOSURE != 0 && self.in_const_expr {
                        return None;
                    }
                    Value::Str(c.name.clone())
                }
                None => {
                    if self.f.flags & fflags::CLOSURE != 0 {
                        return None;
                    }
                    Value::Str(PhpStr::empty())
                }
            },
            MagicConst::Trait => match &self.class {
                Some(c) if c.is_trait => Value::Str(c.name.clone()),
                _ => Value::Str(PhpStr::empty()),
            },
            MagicConst::Property => match &self.f.prop_name {
                Some(p) => Value::Str(p.clone()),
                None => Value::Str(PhpStr::empty()),
            },
        })
    }

    // ------------------------------------------------------------ expressions

    pub(crate) fn expr(&mut self, e: &'a Expr<'a>) -> CResult<Reg> {
        self.expr_hint(e, NONE)
    }

    /// Compiles `e`; `hint` is a slot the result may be written to
    /// directly (an assignment target), used by single-instruction
    /// producers.
    pub(crate) fn expr_hint(&mut self, e: &'a Expr<'a>, hint: Reg) -> CResult<Reg> {
        let line = e.span.line;
        let saved_line = self.f.line;
        self.f.line = line;
        let r = self.expr_inner(e, hint);
        self.f.line = saved_line.max(self.f.line.min(saved_line));
        r
    }

    fn dst(&mut self, hint: Reg) -> Reg {
        if hint != NONE { hint } else { self.tmp() }
    }

    fn expr_inner(&mut self, e: &'a Expr<'a>, hint: Reg) -> CResult<Reg> {
        let line = e.span.line;
        match &e.kind {
            ExprKind::Int(i) => Ok(self.int_const(*i)),
            ExprKind::Float(f) => Ok(self.add_const(Value::Float(*f))),
            ExprKind::String(s, _) => Ok(self.str_const(&super::lit(s))),
            ExprKind::Interpolated(parts, _) => self.rope(parts, hint),
            ExprKind::ShellExec(parts) => {
                // PHP 8.5 (RFC: Deprecations for PHP 8.5).
                self.notice(
                    goro_runtime::Level::Deprecated,
                    "The backtick (`) operator is deprecated, use shell_exec() instead",
                    line,
                );
                let s = self.rope(parts, NONE)?;
                let name = self.name_pair(b"shell_exec");
                let cache = self.cache();
                self.emit(Op::InitFCall {
                    name,
                    nargs: 1,
                    cache,
                    fallback: false,
                });
                self.free(s);
                self.emit(Op::Send { src: s, arg: 0 });
                let d = self.dst(hint);
                self.emit(Op::DoCall { dst: d });
                Ok(d)
            }
            ExprKind::Array { items, .. } => {
                if let Some(v) = self.const_value(e) {
                    return Ok(self.add_const(v));
                }
                self.array_literal(items, line)
            }
            ExprKind::List { .. } => {
                self.err("Cannot use list() outside of an assignment context", line)
            }
            ExprKind::MagicConst(m) => match self.magic_value(*m, line) {
                Some(v) => Ok(self.add_const(v)),
                None => {
                    let d = self.dst(hint);
                    let what = match m {
                        MagicConst::Class => {
                            if self.class.as_ref().is_some_and(|c| c.is_trait) {
                                Special::TraitClass
                            } else {
                                Special::SelfClass
                            }
                        }
                        _ => Special::SelfClass,
                    };
                    self.emit(Op::Special { dst: d, what });
                    Ok(d)
                }
            },
            ExprKind::Variable(n) => {
                if *n == b"this" {
                    let d = self.dst(hint);
                    self.emit(Op::Special {
                        dst: d,
                        what: Special::This,
                    });
                    return Ok(d);
                }
                if is_superglobal(n) {
                    return Ok(self.superglobal(n, false));
                }
                Ok(self.cv(n))
            }
            ExprKind::VariableVariable(x) => {
                let name = self.expr(x)?;
                self.free(name);
                let d = self.tmp();
                self.emit(Op::FetchVarVar {
                    dst: d,
                    name,
                    quiet: false,
                });
                Ok(d)
            }
            ExprKind::Index { base, index } => {
                let Some(index) = index else {
                    return self.err("Cannot use [] for reading", line);
                };
                if matches!(base.kind, ExprKind::Variable(b"GLOBALS")) {
                    let name = self.expr(index)?;
                    self.free(name);
                    let d = self.tmp();
                    self.emit(Op::FetchGlobal {
                        dst: d,
                        name,
                        quiet: false,
                    });
                    return Ok(d);
                }
                let b = self.chain_base(base)?;
                let k = self.expr(index)?;
                self.free(b);
                self.free(k);
                let d = self.tmp();
                self.f.line = line;
                self.emit(Op::FetchDim {
                    dst: d,
                    base: b,
                    key: k,
                });
                Ok(d)
            }
            ExprKind::Prop {
                obj,
                name,
                nullsafe,
            } => self.chain_root(e, |c| {
                let o = c.obj_operand(obj)?;
                if *nullsafe {
                    c.nullsafe_check(o)?;
                }
                let n = c.member_name(name)?;
                c.free(o);
                c.free(n);
                let d = c.tmp();
                let cache = c.cache();
                c.f.line = line;
                c.emit(Op::FetchProp {
                    dst: d,
                    obj: o,
                    name: n,
                    cache,
                });
                Ok(d)
            }),
            ExprKind::StaticProp(sp) => {
                let class = self.class_spec(&sp.class)?;
                let n = match &sp.name {
                    StaticPropName::Ident(i) => self.str_const(i.name),
                    StaticPropName::Expr(x) => self.expr(x)?,
                };
                self.free_spec(class);
                self.free(n);
                let d = self.tmp();
                self.f.line = line;
                self.emit(Op::FetchStaticProp {
                    dst: d,
                    class,
                    name: n,
                });
                Ok(d)
            }
            ExprKind::Const(n) => {
                if let Some(v) = self.const_name_value(n) {
                    return Ok(self.add_const(v));
                }
                if n.kind == NameKind::Unqualified
                    && self.ns.is_empty()
                    && n.name == b"__COMPILER_HALT_OFFSET__"
                {
                    // fetched at run time
                }
                let (full, fb) = self.resolve_const_name(n);
                let name = self.name_pair(&full);
                let fallback = if let Some(fb) = fb {
                    self.name_pair(&fb);
                    true
                } else {
                    false
                };
                let cache = self.cache();
                let d = self.dst(hint);
                self.emit(Op::FetchConst {
                    dst: d,
                    name,
                    cache,
                    fallback,
                });
                Ok(d)
            }
            ExprKind::ClassConst(cc) => {
                let is_class = matches!(&cc.name, MemberName::Ident(i) if i.name.eq_ignore_ascii_case(b"class"));
                if is_class {
                    if let ClassRef::Name(n) = &cc.class
                        && !n.is_special_class()
                    {
                        let full = self.resolve_class(n);
                        return Ok(self.str_const(&full));
                    }
                    if let ClassRef::Name(n) = &cc.class
                        && n.name.eq_ignore_ascii_case(b"self")
                        && let ClassSpec::Named(k) = self.self_spec()
                    {
                        let v = self.f.consts[k as usize].clone();
                        return Ok(self.add_const(v));
                    }
                    let class = self.class_spec(&cc.class)?;
                    self.free_spec(class);
                    let d = self.dst(hint);
                    self.emit(Op::ClassName { dst: d, class });
                    return Ok(d);
                }
                // self::X / parent::X outside a class (test
                // Zend/tests/self_class_const_outside_class).
                if let ClassRef::Name(n) = &cc.class
                    && n.kind == NameKind::Unqualified
                    && (n.name.eq_ignore_ascii_case(b"self")
                        || n.name.eq_ignore_ascii_case(b"parent"))
                    && self.class.is_none()
                    && self.f.flags & (fflags::CLOSURE | fflags::CONST_EXPR | fflags::PSEUDO_MAIN)
                        == 0
                {
                    return self.err(
                        format!(
                            "Cannot use \"{}\" when no class scope is active",
                            String::from_utf8_lossy(&n.name.to_ascii_lowercase())
                        ),
                        line,
                    );
                }
                let class = self.class_spec(&cc.class)?;
                let n = self.member_name(&cc.name)?;
                self.free_spec(class);
                self.free(n);
                let d = self.dst(hint);
                let cache = self.cache();
                self.f.line = line;
                self.emit(Op::ClassConst {
                    dst: d,
                    class,
                    name: n,
                    cache,
                });
                Ok(d)
            }
            // `(NAME)(...)` calls the constant's value (PHP 8.5 constants may
            // hold closures: test Zend/tests/closures/closure_const_expr).
            ExprKind::Call { name, args }
                if self.src[name.span.end as usize..]
                    .iter()
                    .find(|c| !c.is_ascii_whitespace())
                    == Some(&b')')
                    && name.kind != NameKind::FullyQualified =>
            {
                let callee: &'a Expr<'a> = Box::leak(Box::new(Expr {
                    kind: ExprKind::Const(*name),
                    span: name.span,
                }));
                self.chain_root(e, |c| {
                    let f = c.expr(callee)?;
                    c.free(f);
                    let nargs = args.as_slice().len() as u32;
                    c.f.line = line;
                    c.emit(Op::InitDynCall { callee: f, nargs });
                    c.finish_call(args, hint, line)
                })
            }
            ExprKind::Call { name, args } => self.chain_root(e, |c| c.call(name, args, hint, line)),
            ExprKind::CallExpr { callee, args } => self.chain_root(e, |c| {
                let f = c.chain_base(callee)?;
                c.free(f);
                let nargs = args.as_slice().len() as u32;
                c.f.line = line;
                c.emit(Op::InitDynCall { callee: f, nargs });
                c.finish_call(args, hint, line)
            }),
            ExprKind::MethodCall {
                obj,
                name,
                args,
                nullsafe,
            } => self.chain_root(e, |c| {
                if let MemberName::Expr(x) = name
                    && let Some(v) = c.const_value(x)
                    && !matches!(v, Value::Str(_))
                {
                    return c.err("Method name must be a string", line);
                }
                let o = c.obj_operand(obj)?;
                if *nullsafe {
                    c.nullsafe_check(o)?;
                }
                let n = c.member_name(name)?;
                c.free(o);
                c.free(n);
                let cache = c.cache();
                // A method call is located at its name (observed on PHP
                // 8.5: a multi-line `(new A)->m()` is on the line of `->m`).
                let line = match name {
                    MemberName::Ident(i) => i.span.line,
                    MemberName::Expr(x) => x.span.line,
                };
                c.f.line = line;
                c.emit(Op::InitMethod {
                    obj: o,
                    name: n,
                    nargs: args.as_slice().len() as u32,
                    cache,
                });
                c.finish_call(args, hint, line)
            }),
            ExprKind::StaticCall(sc) if parent_hook_call(sc, e.span.start).is_some() => {
                self.parent_hook_call(sc, hint, line)
            }
            ExprKind::StaticCall(sc) => self.chain_root(e, |c| {
                let mut class = c.class_spec(&sc.class)?;
                // `self::m()` forwards the late static binding class (PHP
                // manual, Late Static Bindings: forwarding calls), so it
                // stays relative instead of naming the class.
                if let ClassRef::Name(n) = &sc.class
                    && n.kind == NameKind::Unqualified
                    && n.name.eq_ignore_ascii_case(b"self")
                {
                    class = ClassSpec::SelfRef;
                }
                let n = c.member_name(&sc.name)?;
                c.free_spec(class);
                c.free(n);
                let cache = c.cache();
                c.f.line = line;
                c.emit(Op::InitStaticCall {
                    class,
                    name: n,
                    nargs: sc.args.as_slice().len() as u32,
                    cache,
                });
                c.finish_call(&sc.args, hint, line)
            }),
            ExprKind::New { class, args } => self.new_expr(class, args.as_ref(), line),
            ExprKind::Assign { target, value } => {
                let d = self.tmp();
                self.assign(target, value, d)?;
                Ok(d)
            }
            ExprKind::AssignRef { target, value } => {
                let d = self.tmp();
                self.assign_ref(target, value, d)?;
                Ok(d)
            }
            ExprKind::AssignOp { op, target, value } => {
                let d = self.tmp();
                self.assign_op(*op, target, value, d)
            }
            ExprKind::Binary { op, left, right } => self.binary(*op, left, right, hint, e),
            ExprKind::Unary { op, expr } => {
                if *op == UnaryOp::Silence {
                    let save = self.hidden();
                    self.emit(Op::BeginSilence { save });
                    let r = self.expr(expr)?;
                    // Keep the value past EndSilence.
                    let d = if is_tmp(r) || is_const(r) {
                        r
                    } else {
                        let t = self.tmp();
                        self.emit(Op::Copy { dst: t, src: r });
                        t
                    };
                    self.emit(Op::EndSilence { save });
                    self.free(save);
                    return Ok(d);
                }
                if let Some(v) = self.const_value(e) {
                    return Ok(self.add_const(v));
                }
                let a = self.expr(expr)?;
                self.free(a);
                let d = self.dst(hint);
                let op = match op {
                    UnaryOp::Not => Un::Not,
                    UnaryOp::BitNot => Un::BitNot,
                    UnaryOp::Neg => Un::Neg,
                    UnaryOp::Plus => Un::Plus,
                    UnaryOp::Silence => unreachable!(),
                };
                self.f.line = line;
                self.emit(Op::Unary { op, dst: d, a });
                Ok(d)
            }
            ExprKind::IncDec { op, expr } => {
                let d = self.tmp();
                self.incdec(*op, expr, d)
            }
            ExprKind::Cast { kind, expr } => {
                let to = match kind {
                    CastKind::Int => CastTo::Int,
                    CastKind::Float => CastTo::Float,
                    CastKind::String => CastTo::String,
                    CastKind::Bool => CastTo::Bool,
                    CastKind::Array => CastTo::Array,
                    CastKind::Object => CastTo::Object,
                    CastKind::Unset => {
                        return self.err("The (unset) cast is no longer supported", line);
                    }
                    CastKind::Void => {
                        self.expr_void(expr)?;
                        return Ok(self.null_const());
                    }
                };
                let a = self.expr(expr)?;
                self.free(a);
                let d = self.dst(hint);
                self.f.line = line;
                self.emit(Op::Cast { to, dst: d, a });
                Ok(d)
            }
            ExprKind::Ternary { cond, then, else_ } => {
                let d = self.tmp();
                match then {
                    Some(t) => {
                        let c = self.expr(cond)?;
                        self.free(c);
                        let j = self.emit(Op::JmpZ { c, t: 0 });
                        let v = self.expr(t)?;
                        self.free(v);
                        self.emit(Op::Copy { dst: d, src: v });
                        let j2 = self.emit(Op::Jmp { t: 0 });
                        self.patch_here(j);
                        let v = self.expr(else_)?;
                        self.free(v);
                        self.emit(Op::Copy { dst: d, src: v });
                        self.patch_here(j2);
                    }
                    None => {
                        let c = self.expr(cond)?;
                        self.free(c);
                        self.emit(Op::Copy { dst: d, src: c });
                        let j = self.emit(Op::JmpNZ {
                            c: d & SLOT_MASK,
                            t: 0,
                        });
                        let v = self.expr(else_)?;
                        self.free(v);
                        self.emit(Op::Copy { dst: d, src: v });
                        self.patch_here(j);
                    }
                }
                Ok(d)
            }
            ExprKind::Instanceof { expr, class } => {
                let a = self.expr(expr)?;
                let class = self.class_spec(class)?;
                self.free(a);
                self.free_spec(class);
                let d = self.dst(hint);
                self.emit(Op::InstanceOf { dst: d, a, class });
                Ok(d)
            }
            ExprKind::Isset(es) => {
                let d = self.tmp();
                let mut jumps = Vec::new();
                for (i, x) in es.iter().enumerate() {
                    let r = self.isset_one(x, false)?;
                    self.free(r);
                    self.emit(Op::Copy { dst: d, src: r });
                    if i + 1 < es.len() {
                        jumps.push(self.emit(Op::JmpZ {
                            c: d & SLOT_MASK,
                            t: 0,
                        }));
                    }
                }
                for j in jumps {
                    self.patch_here(j);
                }
                Ok(d)
            }
            ExprKind::Empty(x) => self.isset_one(x, true),
            ExprKind::Eval(x) => {
                let s = self.expr(x)?;
                self.free(s);
                let d = self.tmp();
                self.emit(Op::Eval { dst: d, src: s });
                Ok(d)
            }
            ExprKind::Include { kind, expr } => {
                let s = self.expr(expr)?;
                self.free(s);
                let d = self.tmp();
                let kind = match kind {
                    goro_syntax::ast::IncludeKind::Include => crate::bytecode::IncludeKind::Include,
                    goro_syntax::ast::IncludeKind::IncludeOnce => {
                        crate::bytecode::IncludeKind::IncludeOnce
                    }
                    goro_syntax::ast::IncludeKind::Require => crate::bytecode::IncludeKind::Require,
                    goro_syntax::ast::IncludeKind::RequireOnce => {
                        crate::bytecode::IncludeKind::RequireOnce
                    }
                };
                self.emit(Op::Include {
                    dst: d,
                    src: s,
                    kind,
                });
                Ok(d)
            }
            ExprKind::Exit(args) => {
                let src = match args.as_ref().map(|a| a.as_slice()) {
                    Some([a]) => self.expr(&a.value)?,
                    Some([]) | None => NONE,
                    Some(_) => {
                        // exit() takes at most one argument (TypeError at run time).
                        let name = self.name_pair(b"exit");
                        let cache = self.cache();
                        let a = args.as_ref().unwrap();
                        self.emit(Op::InitFCall {
                            name,
                            nargs: a.as_slice().len() as u32,
                            cache,
                            fallback: false,
                        });
                        return self.finish_call(a, NONE, line);
                    }
                };
                self.free(src);
                self.emit(Op::Exit { src });
                Ok(self.null_const())
            }
            ExprKind::Print(x) => {
                let s = self.expr(x)?;
                self.free(s);
                self.emit(Op::Echo { src: s });
                Ok(self.int_const(1))
            }
            ExprKind::Clone { expr, args } => {
                match (expr, args) {
                    (Some(x), None) => {
                        let s = self.expr(x)?;
                        self.free(s);
                        let d = self.tmp();
                        self.emit(Op::Clone { dst: d, src: s });
                        Ok(d)
                    }
                    (_, Some(a)) => {
                        // clone($x, [...]) / clone(...): call the function form.
                        let name = self.name_pair(b"clone");
                        let cache = self.cache();
                        self.emit(Op::InitFCall {
                            name,
                            nargs: a.as_slice().len() as u32,
                            cache,
                            fallback: false,
                        });
                        self.finish_call(a, hint, line)
                    }
                    (None, None) => self.err("Cannot clone nothing", line),
                }
            }
            ExprKind::Throw(x) => {
                let s = self.expr(x)?;
                self.free(s);
                self.f.line = line;
                self.emit(Op::Throw { src: s });
                Ok(self.null_const())
            }
            ExprKind::Yield { key, value } => {
                if self.in_const_expr {
                    return self.err("Cannot use yield in a constant expression", line);
                }
                let k = match key {
                    Some(k) => self.expr(k)?,
                    None => NONE,
                };
                let v = match value {
                    Some(v) => {
                        if self.f.flags & fflags::BY_REF != 0 && is_ref_capable(v) {
                            self.ref_of(v)?
                        } else {
                            self.expr(v)?
                        }
                    }
                    None => NONE,
                };
                self.free(k);
                self.free(v);
                let d = self.tmp();
                self.f.line = line;
                self.emit(Op::Yield {
                    dst: d,
                    val: v,
                    key: k,
                });
                Ok(d)
            }
            ExprKind::YieldFrom(x) => {
                let s = self.expr(x)?;
                self.free(s);
                let d = self.tmp();
                self.f.line = line;
                self.emit(Op::YieldFrom { dst: d, src: s });
                Ok(d)
            }
            ExprKind::Closure(cl) => {
                let idx = self.closure(cl, e.span)?;
                let d = self.tmp();
                self.emit(Op::MakeClosure { dst: d, func: idx });
                Ok(d)
            }
            ExprKind::ArrowFn(af) => {
                let idx = self.arrow_fn(af, e.span)?;
                let d = self.tmp();
                self.emit(Op::MakeClosure { dst: d, func: idx });
                Ok(d)
            }
            ExprKind::Match { subject, arms } => self.match_expr(subject, arms, line),
        }
    }

    fn superglobal(&mut self, n: &[u8], quiet: bool) -> Reg {
        let d = self.tmp();
        if n == b"GLOBALS" {
            self.emit(Op::Special {
                dst: d,
                what: Special::Globals,
            });
        } else {
            let name = self.str_const(n);
            self.emit(Op::FetchGlobal {
                dst: d,
                name,
                quiet,
            });
        }
        d
    }

    /// Evaluates `$x` of `$x->...`: `$this` stays symbolic.
    fn obj_operand(&mut self, obj: &'a Expr<'a>) -> CResult<Reg> {
        if let ExprKind::Variable(n) = &obj.kind
            && *n == b"this"
        {
            return Ok(THIS);
        }
        self.chain_base(obj)
    }

    // ------------------------------------------------------------ nullsafe chains

    /// Compiles the base of a member/index/call access (part of the same
    /// short-circuit chain).
    fn chain_base(&mut self, e: &'a Expr<'a>) -> CResult<Reg> {
        let saved = self.in_chain;
        self.in_chain = true;
        let r = self.expr(e);
        self.in_chain = saved;
        r
    }

    /// Runs `f` for an access expression; if it is the outermost element of
    /// a chain containing `?->`, short-circuited null lands in the result.
    fn chain_root(
        &mut self,
        _e: &'a Expr<'a>,
        f: impl FnOnce(&mut Self) -> CResult<Reg>,
    ) -> CResult<Reg> {
        if self.in_chain {
            let saved = self.in_chain;
            self.in_chain = false;
            let r = f(self);
            self.in_chain = saved;
            return r;
        }
        let saved_jumps = std::mem::take(&mut self.nullsafe_jumps);
        let r = f(self);
        let jumps = std::mem::replace(&mut self.nullsafe_jumps, saved_jumps);
        let r = r?;
        if jumps.is_empty() {
            return Ok(r);
        }
        let d = self.tmp();
        self.free(r);
        self.emit(Op::Copy { dst: d, src: r });
        let end = self.here();
        for (at, _) in jumps {
            // JmpNull writes null to `res` (its own slot is `d`).
            if let Op::JmpNull { res, t, .. } = &mut self.f.ops[at] {
                *res = d;
                *t = end;
            }
        }
        Ok(d)
    }

    fn nullsafe_check(&mut self, o: Reg) -> CResult<()> {
        let at = self.emit(Op::JmpNull {
            c: o & !TMP | (o & TMP),
            res: NONE,
            t: 0,
        });
        self.nullsafe_jumps.push((at, o));
        Ok(())
    }

    // ------------------------------------------------------------ operators

    fn binary(
        &mut self,
        op: BinaryOp,
        left: &'a Expr<'a>,
        right: &'a Expr<'a>,
        hint: Reg,
        e: &'a Expr<'a>,
    ) -> CResult<Reg> {
        let line = e.span.line;
        match op {
            BinaryOp::BoolAnd | BinaryOp::BoolOr => {
                // A literal left operand deciding the result: the right one
                // is not compiled (observed on PHP 8.5: compile errors in it
                // are not raised; test Zend/tests/property_hooks/
                // ast_printing).
                if let Some(t) = literal_truth(left)
                    && t == (op == BinaryOp::BoolOr)
                {
                    let d = self.tmp();
                    let c = self.add_const(Value::Bool(t));
                    self.emit(Op::Copy { dst: d, src: c });
                    return Ok(d);
                }
                let d = self.tmp();
                let a = self.expr(left)?;
                self.free(a);
                let j = if op == BinaryOp::BoolAnd {
                    self.emit(Op::JmpZEx { c: a, res: d, t: 0 })
                } else {
                    self.emit(Op::JmpNZEx { c: a, res: d, t: 0 })
                };
                let b = self.expr(right)?;
                self.free(b);
                self.emit(Op::Unary {
                    op: Un::Bool,
                    dst: d,
                    a: b,
                });
                self.patch_here(j);
                return Ok(d);
            }
            BinaryOp::Coalesce => {
                let d = self.tmp();
                let a = self.quiet_expr(left)?;
                self.free(a);
                let j = self.emit(Op::JmpSet { c: a, res: d, t: 0 });
                let b = self.expr(right)?;
                self.free(b);
                self.emit(Op::Copy { dst: d, src: b });
                self.patch_here(j);
                return Ok(d);
            }
            BinaryOp::Pipe => {
                let a = self.expr(left)?;
                // `x |> f(...)`: call f directly.
                if let ExprKind::Call {
                    name,
                    args: Args::FirstClassCallable,
                } = &right.kind
                {
                    let (full, fb) = self.resolve_func(name);
                    let k = self.name_pair(&full);
                    let fallback = if let Some(fb) = fb {
                        self.name_pair(&fb);
                        true
                    } else {
                        false
                    };
                    let cache = self.cache();
                    self.emit(Op::InitFCall {
                        name: k,
                        nargs: 1,
                        cache,
                        fallback,
                    });
                } else {
                    let f = self.expr(right)?;
                    self.free(f);
                    self.emit(Op::InitDynCall {
                        callee: f,
                        nargs: 1,
                    });
                }
                self.free(a);
                if is_tmp(a) || is_const(a) {
                    self.emit(Op::Send { src: a, arg: 0 });
                } else {
                    let t = self.tmp();
                    self.emit(Op::Copy { dst: t, src: a });
                    self.free(t);
                    self.emit(Op::Send { src: t, arg: 0 });
                }
                let d = self.dst(hint);
                self.f.line = line;
                self.emit(Op::DoCall { dst: d });
                return Ok(d);
            }
            _ => {}
        }
        if let Some(v) = self.const_value(e) {
            return Ok(self.add_const(v));
        }
        // `>` / `>=` are `<` / `<=` with operands swapped (evaluated in
        // source order).
        let a = self.expr(left)?;
        let b = self.expr(right)?;
        self.free(a);
        self.free(b);
        let d = self.dst(hint);
        self.f.line = line;
        let o = match op {
            BinaryOp::Add => Op::Add { dst: d, a, b },
            BinaryOp::Sub => Op::Sub { dst: d, a, b },
            BinaryOp::Mul => Op::Mul { dst: d, a, b },
            BinaryOp::Concat => Op::Concat { dst: d, a, b },
            BinaryOp::Less => Op::Lt { dst: d, a, b },
            BinaryOp::LessEqual => Op::Le { dst: d, a, b },
            BinaryOp::Greater => Op::Lt { dst: d, a: b, b: a },
            BinaryOp::GreaterEqual => Op::Le { dst: d, a: b, b: a },
            BinaryOp::Equal => Op::Eq { dst: d, a, b },
            BinaryOp::Identical => Op::Identical { dst: d, a, b },
            _ => {
                let bin = match op {
                    BinaryOp::Div => Bin::Div,
                    BinaryOp::Mod => Bin::Mod,
                    BinaryOp::Pow => Bin::Pow,
                    BinaryOp::BitAnd => Bin::BitAnd,
                    BinaryOp::BitOr => Bin::BitOr,
                    BinaryOp::BitXor => Bin::BitXor,
                    BinaryOp::ShiftLeft => Bin::Shl,
                    BinaryOp::ShiftRight => Bin::Shr,
                    BinaryOp::NotEqual => Bin::Ne,
                    BinaryOp::NotIdentical => Bin::NotIdentical,
                    BinaryOp::Spaceship => Bin::Spaceship,
                    BinaryOp::LogicalXor => {
                        // xor: both operands to bool, then compare.
                        let ta = self.tmp();
                        self.emit(Op::Unary {
                            op: Un::Bool,
                            dst: ta,
                            a,
                        });
                        let tb = self.tmp();
                        self.emit(Op::Unary {
                            op: Un::Bool,
                            dst: tb,
                            a: b,
                        });
                        self.free(ta);
                        self.free(tb);
                        self.emit(Op::BinOp {
                            op: Bin::NotIdentical,
                            dst: d,
                            a: ta,
                            b: tb,
                        });
                        return Ok(d);
                    }
                    _ => unreachable!(),
                };
                Op::BinOp {
                    op: bin,
                    dst: d,
                    a,
                    b,
                }
            }
        };
        // `xor` handled above; logical xor needs bool conversion:
        self.emit(o);
        Ok(d)
    }

    fn rope(&mut self, parts: &'a [StrPart<'a>], hint: Reg) -> CResult<Reg> {
        let mut ops = Vec::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => {
                    if !s.is_empty() {
                        ops.push(self.str_const(&super::lit(s)));
                    }
                }
                StrPart::Expr(x) => {
                    let r = self.expr(x)?;
                    // Variables are read when the rope is built, after all
                    // parts are evaluated (as PHP does).
                    ops.push(r);
                }
            }
        }
        for &r in &ops {
            self.free(r);
        }
        let d = self.dst(hint);
        let list = self.f.lists.len() as u32;
        self.f.lists.push(ops);
        self.emit(Op::Rope { dst: d, list });
        Ok(d)
    }

    fn array_literal(&mut self, items: &'a [Option<ArrayItem<'a>>], line: u32) -> CResult<Reg> {
        let d = self.tmp();
        self.emit(Op::NewArray {
            dst: d,
            cap: items.len() as u32,
        });
        for it in items {
            let Some(it) = it else {
                return self.err("Cannot use empty array elements in arrays", line);
            };
            self.f.line = it.span.line;
            if it.unpack {
                let s = self.expr(&it.value)?;
                self.free(s);
                self.emit(Op::AddSpread { arr: d, src: s });
                continue;
            }
            let k = match &it.key {
                Some(k) => self.expr(k)?,
                None => NONE,
            };
            if it.by_ref {
                let r = self.ref_of(&it.value)?;
                self.free(k);
                self.free(r);
                self.emit(Op::AddElemRef {
                    arr: d,
                    key: k,
                    val: r,
                });
            } else {
                let v = self.expr(&it.value)?;
                self.free(k);
                self.free(v);
                self.emit(Op::AddElem {
                    arr: d,
                    key: k,
                    val: v,
                });
            }
        }
        Ok(d)
    }

    fn match_expr(
        &mut self,
        subject: &'a Expr<'a>,
        arms: &'a [MatchArm<'a>],
        line: u32,
    ) -> CResult<Reg> {
        let s0 = self.expr(subject)?;
        let s = self.hidden();
        self.free(s0);
        self.emit(Op::Copy { dst: s, src: s0 });
        let d = self.tmp();
        let mut ends = Vec::new();
        let mut default_arm = None;
        let mut arm_jumps: Vec<(usize, Vec<usize>)> = Vec::new();
        for (i, arm) in arms.iter().enumerate() {
            match arm.conds {
                None => {
                    if default_arm.is_some() {
                        return self.err(
                            "Match expressions may only contain one default arm",
                            arm.span.line,
                        );
                    }
                    default_arm = Some(i);
                }
                Some(conds) => {
                    let mut js = Vec::new();
                    for c in conds {
                        self.f.line = c.span.line;
                        let v = self.expr(c)?;
                        self.free(v);
                        let t = self.tmp();
                        self.emit(Op::Identical { dst: t, a: s, b: v });
                        self.free(t);
                        js.push(self.emit(Op::JmpNZ { c: t, t: 0 }));
                    }
                    arm_jumps.push((i, js));
                }
            }
        }
        // No arm matched.
        let no_match = if let Some(di) = default_arm {
            self.emit(Op::Jmp { t: 0 });
            Some(di)
        } else {
            self.f.line = line;
            self.emit(Op::MatchError { src: s });
            None
        };
        let jmp_default = if no_match.is_some() {
            Some(self.f.ops.len() - 1)
        } else {
            None
        };
        let mut starts = vec![0u32; arms.len()];
        for (i, arm) in arms.iter().enumerate() {
            starts[i] = self.here();
            self.f.line = arm.body.span.line;
            let v = self.expr(&arm.body)?;
            self.free(v);
            self.emit(Op::Copy { dst: d, src: v });
            ends.push(self.emit(Op::Jmp { t: 0 }));
        }
        for (i, js) in arm_jumps {
            for j in js {
                self.patch(j, starts[i]);
            }
        }
        if let (Some(at), Some(di)) = (jmp_default, no_match) {
            self.patch(at, starts[di]);
        }
        for j in ends {
            self.patch_here(j);
        }
        self.emit(Op::Free { slot: s });
        self.free(s);
        Ok(d)
    }

    // ------------------------------------------------------------ isset / empty / quiet

    /// Compiles `e` without "undefined" warnings (`??`, isset chains).
    fn quiet_expr(&mut self, e: &'a Expr<'a>) -> CResult<Reg> {
        match &e.kind {
            ExprKind::Variable(n) if is_superglobal(n) => Ok(self.superglobal(n, true)),
            ExprKind::Index {
                base,
                index: Some(index),
            } if matches!(base.kind, ExprKind::Variable(b"GLOBALS")) => {
                let name = self.expr(index)?;
                self.free(name);
                let d = self.tmp();
                self.emit(Op::FetchGlobal {
                    dst: d,
                    name,
                    quiet: true,
                });
                Ok(d)
            }
            ExprKind::Variable(n) if *n != b"this" => Ok(self.cv(n)),
            ExprKind::VariableVariable(x) => {
                let name = self.expr(x)?;
                self.free(name);
                let d = self.tmp();
                self.emit(Op::FetchVarVar {
                    dst: d,
                    name,
                    quiet: true,
                });
                Ok(d)
            }
            ExprKind::Index {
                base,
                index: Some(index),
            } => {
                let b = self.quiet_expr(base)?;
                let k = self.expr(index)?;
                self.free(b);
                self.free(k);
                let d = self.tmp();
                self.f.line = e.span.line;
                self.emit(Op::FetchDimIs {
                    dst: d,
                    base: b,
                    key: k,
                });
                Ok(d)
            }
            ExprKind::Prop {
                obj,
                name,
                nullsafe: _,
            } => {
                let o = match &obj.kind {
                    ExprKind::Variable(n) if *n == b"this" => THIS,
                    _ => self.quiet_expr(obj)?,
                };
                let n = self.member_name(name)?;
                self.free(o);
                self.free(n);
                let d = self.tmp();
                let cache = self.cache();
                self.f.line = e.span.line;
                self.emit(Op::FetchPropIs {
                    dst: d,
                    obj: o,
                    name: n,
                    cache,
                });
                Ok(d)
            }
            ExprKind::StaticProp(sp) => {
                // A missing static property is null for isset/??.
                let class = self.class_spec(&sp.class)?;
                let n = match &sp.name {
                    StaticPropName::Ident(i) => self.str_const(i.name),
                    StaticPropName::Expr(x) => self.expr(x)?,
                };
                self.free_spec(class);
                self.free(n);
                let t = self.tmp();
                self.emit(Op::IssetStaticProp {
                    dst: t,
                    class,
                    name: n,
                    empty: false,
                });
                // Fetch only if set.
                let d = self.tmp();
                self.free(t);
                let j = self.emit(Op::JmpZEx { c: t, res: d, t: 0 });
                let class2 = self.class_spec(&sp.class)?;
                self.free_spec(class2);
                self.emit(Op::FetchStaticProp {
                    dst: d,
                    class: class2,
                    name: n,
                });
                let j2 = self.emit(Op::Jmp { t: 0 });
                self.patch_here(j);
                let nl = self.null_const();
                self.emit(Op::Copy { dst: d, src: nl });
                self.patch_here(j2);
                Ok(d)
            }
            _ => self.expr(e),
        }
    }

    /// `isset(e)` (or `empty(e)` when `empty`), as a bool operand.
    fn isset_one(&mut self, e: &'a Expr<'a>, empty: bool) -> CResult<Reg> {
        let line = e.span.line;
        match &e.kind {
            ExprKind::Variable(n) => {
                let d = self.tmp();
                if *n == b"this" {
                    let t = self.tmp();
                    self.emit(Op::Special {
                        dst: t,
                        what: Special::ThisQuiet,
                    });
                    self.free(t);
                    self.emit(Op::Isset { dst: d, src: t });
                    if empty {
                        self.emit(Op::Unary {
                            op: Un::Not,
                            dst: d,
                            a: d,
                        });
                    }
                    return Ok(d);
                }
                if is_superglobal(n) {
                    let v = self.superglobal(n, true);
                    self.free(v);
                    if empty {
                        self.emit(Op::Unary {
                            op: Un::Not,
                            dst: d,
                            a: v,
                        });
                    } else {
                        self.emit(Op::Isset { dst: d, src: v });
                    }
                    return Ok(d);
                }
                let var = self.cv(n);
                self.emit(Op::IssetVar { dst: d, var, empty });
                Ok(d)
            }
            ExprKind::VariableVariable(x) => {
                let name = self.expr(x)?;
                self.free(name);
                let d = self.tmp();
                self.emit(Op::IssetVarVar {
                    dst: d,
                    name,
                    empty,
                });
                Ok(d)
            }
            ExprKind::Index {
                base,
                index: Some(index),
            } if matches!(base.kind, ExprKind::Variable(b"GLOBALS")) => {
                let name = self.expr(index)?;
                self.free(name);
                let t = self.tmp();
                self.emit(Op::FetchGlobal {
                    dst: t,
                    name,
                    quiet: true,
                });
                self.free(t);
                let d = self.tmp();
                if empty {
                    self.emit(Op::Unary {
                        op: Un::Not,
                        dst: d,
                        a: t,
                    });
                } else {
                    self.emit(Op::Isset { dst: d, src: t });
                }
                Ok(d)
            }
            ExprKind::Index {
                base,
                index: Some(index),
            } => {
                let b = self.quiet_expr(base)?;
                let k = self.expr(index)?;
                self.free(b);
                self.free(k);
                let d = self.tmp();
                self.f.line = line;
                self.emit(Op::IssetDim {
                    dst: d,
                    base: b,
                    key: k,
                    empty,
                });
                Ok(d)
            }
            ExprKind::Prop { obj, name, .. } => {
                let o = match &obj.kind {
                    ExprKind::Variable(n) if *n == b"this" => THIS,
                    _ => self.quiet_expr(obj)?,
                };
                let n = self.member_name(name)?;
                self.free(o);
                self.free(n);
                let d = self.tmp();
                self.f.line = line;
                self.emit(Op::IssetProp {
                    dst: d,
                    obj: o,
                    name: n,
                    empty,
                });
                Ok(d)
            }
            ExprKind::StaticProp(sp) => {
                let class = self.class_spec(&sp.class)?;
                let n = match &sp.name {
                    StaticPropName::Ident(i) => self.str_const(i.name),
                    StaticPropName::Expr(x) => self.expr(x)?,
                };
                self.free_spec(class);
                self.free(n);
                let d = self.tmp();
                self.emit(Op::IssetStaticProp {
                    dst: d,
                    class,
                    name: n,
                    empty,
                });
                Ok(d)
            }
            ExprKind::Index { index: None, .. } => self.err("Cannot use [] for reading", line),
            _ => {
                if !empty {
                    return self.err(
                        "Cannot use isset() on the result of an expression (you can use \"null !== expression\" instead)",
                        line,
                    );
                }
                let v = self.expr(e)?;
                self.free(v);
                let d = self.tmp();
                self.emit(Op::Unary {
                    op: Un::Not,
                    dst: d,
                    a: v,
                });
                Ok(d)
            }
        }
    }

    // ------------------------------------------------------------ calls

    fn call(&mut self, name: &Name<'a>, args: &'a Args<'a>, hint: Reg, line: u32) -> CResult<Reg> {
        let (full, fb) = self.resolve_func(name);
        // Compile-time specialisations of a few functions (only when the
        // name can't refer to a namespaced function).
        let lname = full.to_ascii_lowercase();
        if fb.is_none() || self.ns.is_empty() {
            let plain = args
                .as_slice()
                .iter()
                .all(|a| a.name.is_none() && !a.unpack)
                && !matches!(args, Args::FirstClassCallable);
            if plain {
                let a = args.as_slice();
                match (&lname[..], a.len()) {
                    (b"strlen", 1) if fb.is_none() || self.ns.is_empty() => {
                        let s = self.expr(&a[0].value)?;
                        self.free(s);
                        let d = self.dst(hint);
                        self.f.line = line;
                        self.emit(Op::Strlen { dst: d, src: s });
                        return Ok(d);
                    }
                    (b"func_get_args", 0) if self.f.flags & fflags::PSEUDO_MAIN == 0 => {
                        let d = self.dst(hint);
                        self.emit(Op::Special {
                            dst: d,
                            what: Special::FuncGetArgs,
                        });
                        return Ok(d);
                    }
                    (b"func_num_args", 0) if self.f.flags & fflags::PSEUDO_MAIN == 0 => {
                        let d = self.dst(hint);
                        self.emit(Op::Special {
                            dst: d,
                            what: Special::FuncNumArgs,
                        });
                        return Ok(d);
                    }
                    (b"get_called_class", 0) => {
                        let d = self.dst(hint);
                        self.emit(Op::Special {
                            dst: d,
                            what: Special::CalledClass,
                        });
                        return Ok(d);
                    }
                    _ => {}
                }
            }
        }
        if lname == b"assert"
            || (fb.as_deref().map(|f| f.eq_ignore_ascii_case(b"assert")) == Some(true))
        {
            // assert() with zend.assertions: the message includes the
            // source text of the expression (handled by the function, which
            // receives the text as a hidden argument).
            if let Some(first) = args.as_slice().first()
                && args.as_slice().len() == 1
                && first.name.is_none()
            {
                let text =
                    &self.src[first.value.span.start as usize..first.value.span.end as usize];
                let desc = format!("assert({})", String::from_utf8_lossy(text));
                let k = self.name_pair(b"assert");
                let cache = self.cache();
                self.emit(Op::InitFCall {
                    name: k,
                    nargs: 2,
                    cache,
                    fallback: false,
                });
                self.f.call_depth += 1;
                let v = self.expr(&first.value)?;
                self.free(v);
                self.emit(Op::Send { src: v, arg: 0 });
                let dc = self.str_const(desc.as_bytes());
                self.emit(Op::Send { src: dc, arg: 1 });
                self.f.call_depth -= 1;
                let d = self.dst(hint);
                self.f.line = line;
                self.emit(Op::DoCall { dst: d });
                return Ok(d);
            }
        }
        let k = self.name_pair(&full);
        let fallback = if let Some(fb) = fb {
            self.name_pair(&fb);
            true
        } else {
            false
        };
        let cache = self.cache();
        self.f.line = line;
        self.emit(Op::InitFCall {
            name: k,
            nargs: args.as_slice().len() as u32,
            cache,
            fallback,
        });
        self.finish_call(args, hint, line)
    }

    /// Sends the arguments of the pending call and calls it.
    /// `parent::$prop::get()` / `parent::$prop::set($v)`: the parent's hook,
    /// or its plain property access (RFC: Property hooks, "Parent property
    /// access"; tests Zend/tests/property_hooks/parent_*).
    fn parent_hook_call(
        &mut self,
        sc: &'a goro_syntax::ast::StaticCallNode<'a>,
        hint: Reg,
        line: u32,
    ) -> CResult<Reg> {
        let (prop, set) = parent_hook_call(sc, 0).unwrap();
        let shown = match &prop {
            goro_syntax::ast::StaticPropName::Ident(i) => {
                String::from_utf8_lossy(i.name).into_owned()
            }
            goro_syntax::ast::StaticPropName::Expr(x) => match &x.kind {
                ExprKind::Int(i) => i.to_string(),
                ExprKind::String(s, _) => String::from_utf8_lossy(s).into_owned(),
                _ => String::new(),
            },
        };
        let what = if set { "set" } else { "get" };
        if self.class.is_none() {
            return self.err("Cannot use \"parent\" when no class scope is active", line);
        }
        let Some(cur) = self.f.prop_name.clone() else {
            return self.err(
                format!(
                    "Must not use parent::${}::{}() outside a property hook",
                    shown, what
                ),
                line,
            );
        };
        let same =
            matches!(&prop, goro_syntax::ast::StaticPropName::Ident(i) if i.name == cur.as_bytes());
        if !same {
            return self.err(
                format!(
                    "Must not use parent::${}::{}() in a different property (${})",
                    shown,
                    what,
                    cur.to_string_lossy()
                ),
                line,
            );
        }
        let cur_kind = if self.f.func_name.as_bytes().ends_with(b"::set") {
            "set"
        } else {
            "get"
        };
        if cur_kind != what {
            return self.err(
                format!(
                    "Must not use parent::${}::{}() in a different property hook ({})",
                    shown, what, cur_kind
                ),
                line,
            );
        }
        if let Args::FirstClassCallable = sc.args {
            return self.err("Cannot create Closure for parent property hook call", line);
        }
        let name = self.str_const(cur.as_bytes());
        self.f.line = line;
        self.emit(Op::InitParentHook { name, set });
        self.finish_call(&sc.args, hint, line)
    }

    fn finish_call(&mut self, args: &'a Args<'a>, hint: Reg, line: u32) -> CResult<Reg> {
        self.f.call_depth += 1;
        if let Args::FirstClassCallable = args {
            self.f.call_depth -= 1;
            let d = self.dst(hint);
            self.emit(Op::CallableConvert { dst: d });
            return Ok(d);
        }
        self.send_args(args.as_slice())?;
        self.f.call_depth -= 1;
        let d = self.dst(hint);
        self.f.line = line;
        self.emit(Op::DoCall { dst: d });
        Ok(d)
    }

    fn send_args(&mut self, args: &'a [Arg<'a>]) -> CResult<()> {
        let mut seen_named = false;
        let mut seen_unpack = false;
        for (i, a) in args.iter().enumerate() {
            let i = i as u32;
            self.f.line = a.span.line;
            if a.unpack {
                if seen_named {
                    return self.err(
                        "Cannot use argument unpacking after named arguments",
                        a.span.line,
                    );
                }
                seen_unpack = true;
                let v = self.expr(&a.value)?;
                self.free(v);
                self.emit(Op::SendUnpack { src: v });
                continue;
            }
            if let Some(n) = &a.name {
                seen_named = true;
                let name = self.str_const(n.name);
                match &a.value.kind {
                    ExprKind::Variable(v) if *v != b"this" && !is_superglobal(v) => {
                        let var = self.cv(v);
                        self.emit(Op::SendNamedVar { var, name });
                    }
                    _ if is_ref_capable(&a.value) => {
                        let j = self.emit(Op::JmpNamedArgByRef { name, t: 0 });
                        let v = self.expr(&a.value)?;
                        self.free(v);
                        self.emit(Op::SendNamed { src: v, name });
                        let j2 = self.emit(Op::Jmp { t: 0 });
                        self.patch_here(j);
                        let r = self.ref_of(&a.value)?;
                        self.free(r);
                        self.emit(Op::SendNamed { src: r, name });
                        self.patch_here(j2);
                    }
                    _ => {
                        let v = self.expr(&a.value)?;
                        self.free(v);
                        self.emit(Op::SendNamed { src: v, name });
                    }
                }
                continue;
            }
            if seen_named {
                return self.err(
                    "Cannot use positional argument after named argument",
                    a.span.line,
                );
            }
            if seen_unpack {
                return self.err(
                    "Cannot use positional argument after argument unpacking",
                    a.span.line,
                );
            }
            match &a.value.kind {
                ExprKind::Variable(v) if *v != b"this" && !is_superglobal(v) => {
                    let var = self.cv(v);
                    self.emit(Op::SendVar { var, arg: i });
                }
                ExprKind::Index { index: None, .. } => {
                    // `f($a[])`: only valid for by-reference parameters.
                    let j = self.emit(Op::JmpArgByRef { arg: i, t: 0 });
                    let msg =
                        self.add_const(Value::Str(PhpStr::intern(b"Cannot use [] for reading")));
                    self.emit(Op::Error {
                        msg: msg & !CONST,
                        kind: 0,
                    });
                    self.patch_here(j);
                    let r = self.ref_of(&a.value)?;
                    self.free(r);
                    self.emit(Op::SendRef { src: r, arg: i });
                }
                _ if is_ref_capable(&a.value) => {
                    let j = self.emit(Op::JmpArgByRef { arg: i, t: 0 });
                    let v = self.expr(&a.value)?;
                    self.free(v);
                    self.emit(Op::Send { src: v, arg: i });
                    let j2 = self.emit(Op::Jmp { t: 0 });
                    self.patch_here(j);
                    let r = self.ref_of(&a.value)?;
                    self.free(r);
                    self.emit(Op::SendRef { src: r, arg: i });
                    self.patch_here(j2);
                }
                ExprKind::Call { .. }
                | ExprKind::MethodCall { .. }
                | ExprKind::StaticCall(_)
                | ExprKind::CallExpr { .. } => {
                    let v = self.expr(&a.value)?;
                    self.free(v);
                    self.emit(Op::SendValRef { src: v, arg: i });
                }
                _ => {
                    let v = self.expr(&a.value)?;
                    self.free(v);
                    self.emit(Op::Send { src: v, arg: i });
                }
            }
        }
        Ok(())
    }

    fn new_expr(
        &mut self,
        class: &'a NewClass<'a>,
        args: Option<&'a Args<'a>>,
        line: u32,
    ) -> CResult<Reg> {
        let nargs = args.map(|a| a.as_slice().len()).unwrap_or(0) as u32;
        let d = self.tmp();
        let at = match class {
            NewClass::Ref(c) => {
                if let ClassRef::Name(n) = c
                    && n.kind == NameKind::Unqualified
                    && n.name.eq_ignore_ascii_case(b"static")
                {
                    // `new static`
                }
                let spec = self.class_spec(c)?;
                self.free_spec(spec);
                self.f.line = line;
                let cache = if matches!(spec, ClassSpec::Named(_)) {
                    self.cache()
                } else {
                    0
                };
                self.emit(Op::New {
                    dst: d,
                    class: spec,
                    nargs,
                    cache,
                })
            }
            NewClass::Anonymous(cd) => {
                let idx = self.class_decl(cd)?;
                self.f.line = line;
                self.emit(Op::NewAnon {
                    dst: d,
                    class: idx,
                    nargs,
                })
            }
        };
        self.f.call_depth += 1;
        if let Some(Args::FirstClassCallable) = args {
            return self.err("Cannot create Closure for new expression", line);
        }
        if let Some(a) = args {
            self.send_args(a.as_slice())?;
        }
        self.f.call_depth -= 1;
        self.f.line = line;
        self.emit(Op::DoCall { dst: NONE });
        self.patch_here(at);
        Ok(d)
    }

    // ------------------------------------------------------------ write paths

    fn lpath(&mut self, e: &'a Expr<'a>, for_unset: bool) -> CResult<LPath> {
        let line = e.span.line;
        match &e.kind {
            ExprKind::Variable(n) if is_superglobal(n) => {
                if *n == b"GLOBALS" {
                    return self.err(
                        "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                        line,
                    );
                }
                let name = self.str_const(n);
                Ok(LPath {
                    base: PathBase::Global(name),
                    elems: Vec::new(),
                })
            }
            ExprKind::Index {
                base,
                index: Some(index),
            } if matches!(base.kind, ExprKind::Variable(b"GLOBALS")) => {
                let name = self.expr(index)?;
                Ok(LPath {
                    base: PathBase::Global(name),
                    elems: Vec::new(),
                })
            }
            ExprKind::Variable(n) => {
                if *n == b"this" {
                    return Ok(LPath {
                        base: PathBase::This,
                        elems: Vec::new(),
                    });
                }
                let cv = self.cv(n);
                Ok(LPath {
                    base: PathBase::Var(cv),
                    elems: Vec::new(),
                })
            }
            ExprKind::VariableVariable(x) => {
                let name = self.expr(x)?;
                Ok(LPath {
                    base: PathBase::VarVar(name),
                    elems: Vec::new(),
                })
            }
            ExprKind::StaticProp(sp) => {
                let class = self.class_spec(&sp.class)?;
                let n = match &sp.name {
                    StaticPropName::Ident(i) => self.str_const(i.name),
                    StaticPropName::Expr(x) => self.expr(x)?,
                };
                Ok(LPath {
                    base: PathBase::StaticProp(class, n),
                    elems: Vec::new(),
                })
            }
            ExprKind::Index { base, index } => {
                let mut p = self.lpath_base(base, for_unset)?;
                match index {
                    Some(i) => {
                        let k = self.expr(i)?;
                        p.elems.push(PathElem::Dim(k));
                    }
                    None => {
                        if for_unset {
                            return self.err("Cannot use [] for unsetting", line);
                        }
                        p.elems.push(PathElem::Append)
                    }
                }
                Ok(p)
            }
            ExprKind::Prop {
                obj,
                name,
                nullsafe,
            } => {
                if *nullsafe {
                    return self.err("Can't use nullsafe operator in write context", line);
                }
                let mut p = match &obj.kind {
                    ExprKind::Variable(n) if *n == b"this" => LPath {
                        base: PathBase::This,
                        elems: Vec::new(),
                    },
                    _ => self.lpath_base(obj, for_unset)?,
                };
                let n = self.member_name(name)?;
                let cache = self.cache();
                p.elems.push(PathElem::Prop(n, cache));
                Ok(p)
            }
            _ => {
                if contains_nullsafe(e) {
                    return self.err("Can't use nullsafe operator in write context", line);
                }
                // Temporary values (call results...).
                let v = self.expr(e)?;
                Ok(LPath {
                    base: PathBase::Tmp(v),
                    elems: Vec::new(),
                })
            }
        }
    }

    fn lpath_base(&mut self, e: &'a Expr<'a>, for_unset: bool) -> CResult<LPath> {
        match &e.kind {
            ExprKind::Variable(_)
            | ExprKind::VariableVariable(_)
            | ExprKind::StaticProp(_)
            | ExprKind::Index { .. } => self.lpath(e, for_unset),
            ExprKind::Prop {
                nullsafe: false, ..
            } => self.lpath(e, for_unset),
            ExprKind::Prop { nullsafe: true, .. } => {
                self.err("Can't use nullsafe operator in write context", e.span.line)
            }
            ExprKind::Call { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::StaticCall(_)
            | ExprKind::CallExpr { .. } => {
                if contains_nullsafe(e) {
                    return self.err("Can't use nullsafe operator in write context", e.span.line);
                }
                self.check_builtin_write(e)?;
                let v = self.expr(e)?;
                Ok(LPath {
                    base: PathBase::Tmp(v),
                    elems: Vec::new(),
                })
            }
            _ => {
                if contains_nullsafe(e) {
                    return self.err("Can't use nullsafe operator in write context", e.span.line);
                }
                let v = self.expr(e)?;
                Ok(LPath {
                    base: PathBase::Tmp(v),
                    elems: Vec::new(),
                })
            }
        }
    }

    fn free_path(&mut self, p: &LPath) {
        match p.base {
            PathBase::Tmp(r) | PathBase::VarVar(r) | PathBase::Global(r) => self.free(r),
            PathBase::StaticProp(c, n) => {
                self.free_spec(c);
                self.free(n);
            }
            _ => {}
        }
        for e in &p.elems {
            match e {
                PathElem::Dim(r) | PathElem::Prop(r, _) => self.free(*r),
                PathElem::Append => {}
            }
        }
    }

    fn emit_path(&mut self, p: LPath, action: PathAction, res: Reg) {
        self.free_path(&p);
        if let PathAction::Assign(v) | PathAction::AssignOp(_, v) | PathAction::BindRef(v) = action
        {
            self.free(v);
        }
        let idx = self.f.paths.len() as u32;
        self.f.paths.push(WritePath {
            base: p.base,
            elems: p.elems.into_boxed_slice(),
            action,
        });
        self.emit(Op::Write { path: idx, res });
    }

    /// An operand holding a reference to the place `e` names.
    pub(crate) fn ref_of(&mut self, e: &'a Expr<'a>) -> CResult<Reg> {
        match &e.kind {
            ExprKind::Variable(n) if *n != b"this" => {
                let var = self.cv(n);
                let d = self.tmp();
                self.emit(Op::RefVar { dst: d, var });
                Ok(d)
            }
            ExprKind::Variable(_) => self.err("Cannot re-assign $this", e.span.line),
            ExprKind::VariableVariable(_)
            | ExprKind::Index { .. }
            | ExprKind::Prop { .. }
            | ExprKind::StaticProp(_) => {
                let p = self.lpath(e, false)?;
                let d = self.tmp();
                self.f.line = e.span.line;
                self.emit_path(p, PathAction::MakeRef, d);
                Ok(d)
            }
            _ => {
                // Not referenceable: a value (callers warn as PHP does).
                self.expr(e)
            }
        }
    }

    /// Binds the place `target` to the reference in operand `src`.
    pub(crate) fn bind_ref_to(&mut self, target: &'a Expr<'a>, src: Reg) -> CResult<()> {
        match &target.kind {
            ExprKind::Variable(n) if *n != b"this" => {
                let dst = self.cv(n);
                self.free(src);
                self.emit(Op::BindRef { dst, src });
                Ok(())
            }
            ExprKind::Variable(_) => self.err("Cannot re-assign $this", target.span.line),
            ExprKind::List { items, .. } | ExprKind::Array { items, .. } => {
                // foreach ($a as &[$x, $y]) / [&$x] = ...
                let h = self.hidden();
                self.emit(Op::Copy { dst: h, src });
                self.free(src);
                self.destructure_ref(items, h, target.span.line)?;
                self.emit(Op::Free { slot: h });
                self.free(h);
                Ok(())
            }
            _ => {
                let p = self.lpath(target, false)?;
                self.emit_path(p, PathAction::BindRef(src), NONE);
                Ok(())
            }
        }
    }

    /// Assigns the value in operand `val` to `target`; result to `res`.
    pub(crate) fn assign_to(&mut self, target: &'a Expr<'a>, val: Reg, res: Reg) -> CResult<()> {
        let line = target.span.line;
        match &target.kind {
            ExprKind::Variable(n) if is_superglobal(n) => {
                let p = self.lpath(target, false)?;
                self.emit_path(p, PathAction::Assign(val), res);
                Ok(())
            }
            ExprKind::Variable(n) => {
                if *n == b"this" {
                    return self.err("Cannot re-assign $this", line);
                }
                let dst = self.cv(n);
                self.free(val);
                if res == NONE {
                    self.emit(Op::Assign { dst, src: val });
                } else {
                    self.emit(Op::AssignRes { dst, src: val, res });
                }
                Ok(())
            }
            ExprKind::Index { base, index } if matches!(base.kind, ExprKind::Variable(n) if n != b"this" && !is_superglobal(n)) =>
            {
                let ExprKind::Variable(n) = base.kind else {
                    unreachable!()
                };
                let var = self.cv(n);
                let key = match index {
                    Some(i) => self.expr(i)?,
                    None => NONE,
                };
                self.free(key);
                self.free(val);
                self.f.line = line;
                self.emit(Op::AssignDim { var, key, val, res });
                Ok(())
            }
            ExprKind::Prop {
                obj,
                name,
                nullsafe: false,
            } if res == NONE && matches!(obj.kind, ExprKind::Variable(n) if !is_superglobal(n)) => {
                let o = match obj.kind {
                    ExprKind::Variable(n) if n == b"this" => THIS,
                    ExprKind::Variable(n) => self.cv(n),
                    _ => unreachable!(),
                };
                let n = self.member_name(name)?;
                self.free(n);
                self.free(val);
                let cache = self.cache();
                self.f.line = line;
                self.emit(Op::AssignProp {
                    obj: o,
                    name: n,
                    val,
                    cache,
                });
                Ok(())
            }
            ExprKind::List { items, .. } | ExprKind::Array { items, .. } => {
                let h = self.hidden();
                self.free(val);
                self.emit(Op::Copy { dst: h, src: val });
                self.destructure(items, h, line)?;
                if res != NONE {
                    self.emit(Op::Copy { dst: res, src: h });
                }
                self.emit(Op::Free { slot: h });
                self.free(h);
                Ok(())
            }
            _ => {
                let p = self.lpath(target, false)?;
                self.f.line = line;
                self.emit_path(p, PathAction::Assign(val), res);
                Ok(())
            }
        }
    }

    fn assign(&mut self, target: &'a Expr<'a>, value: &'a Expr<'a>, res: Reg) -> CResult<()> {
        let line = target.span.line;
        match &target.kind {
            ExprKind::Variable(n) if *n == b"this" => self.err("Cannot re-assign $this", line),
            ExprKind::Variable(n) if is_superglobal(n) => {
                let p = self.lpath(target, false)?;
                let v = self.expr(value)?;
                self.emit_path(p, PathAction::Assign(v), res);
                Ok(())
            }
            ExprKind::Variable(n) => {
                let dst = self.cv(n);
                // Single-instruction producers write the variable directly.
                let direct = res == NONE && writes_directly(value);
                let v = self.expr_hint(value, if direct { dst } else { NONE })?;
                if v == dst && direct {
                    return Ok(());
                }
                self.f.line = line;
                self.assign_to(target, v, res)
            }
            ExprKind::List { items, .. } | ExprKind::Array { items, .. } => {
                if items.iter().flatten().any(|i| i.by_ref) {
                    // [&$a, $b] = $arr: destructure by reference.
                    let r = if is_ref_capable(value) {
                        self.ref_of(value)?
                    } else {
                        self.expr(value)?
                    };
                    let h = self.hidden();
                    self.free(r);
                    self.emit(Op::Copy { dst: h, src: r });
                    self.destructure_ref(items, h, line)?;
                    if res != NONE {
                        self.emit(Op::Copy { dst: res, src: h });
                    }
                    self.emit(Op::Free { slot: h });
                    self.free(h);
                    return Ok(());
                }
                let v = self.expr(value)?;
                self.assign_to(target, v, res)
            }
            ExprKind::Index { base, index } if matches!(base.kind, ExprKind::Variable(n) if n != b"this" && !is_superglobal(n)) =>
            {
                // $cv[k] = v: key evaluated before the value.
                let ExprKind::Variable(n) = base.kind else {
                    unreachable!()
                };
                let var = self.cv(n);
                let key = match index {
                    Some(i) => self.expr(i)?,
                    None => NONE,
                };
                let v = self.expr(value)?;
                self.free(key);
                self.free(v);
                self.f.line = line;
                self.emit(Op::AssignDim {
                    var,
                    key,
                    val: v,
                    res,
                });
                Ok(())
            }
            ExprKind::Prop {
                obj,
                name,
                nullsafe: false,
            } if res == NONE && matches!(obj.kind, ExprKind::Variable(n) if !is_superglobal(n)) => {
                let o = match obj.kind {
                    ExprKind::Variable(n) if n == b"this" => THIS,
                    ExprKind::Variable(n) => self.cv(n),
                    _ => unreachable!(),
                };
                let n = self.member_name(name)?;
                let v = self.expr(value)?;
                self.free(n);
                self.free(v);
                let cache = self.cache();
                self.f.line = line;
                self.emit(Op::AssignProp {
                    obj: o,
                    name: n,
                    val: v,
                    cache,
                });
                Ok(())
            }
            ExprKind::Index { .. }
            | ExprKind::Prop { .. }
            | ExprKind::StaticProp(_)
            | ExprKind::VariableVariable(_) => {
                // Keys before the value (PHP's evaluation order).
                let p = self.lpath(target, false)?;
                let v = self.expr(value)?;
                self.f.line = line;
                self.emit_path(p, PathAction::Assign(v), res);
                Ok(())
            }
            _ => self.err("Cannot assign to this expression", line),
        }
    }

    /// `[$a, 'k' => $b] = $h`: assigns elements of the array in slot `h`.
    pub(crate) fn destructure(
        &mut self,
        items: &'a [Option<ArrayItem<'a>>],
        h: Reg,
        line: u32,
    ) -> CResult<()> {
        let keyed = items.iter().flatten().any(|i| i.key.is_some());
        let positional = items.iter().flatten().any(|i| i.key.is_none());
        if keyed && positional {
            return self.err(
                "Cannot mix keyed and unkeyed array entries in assignments",
                line,
            );
        }
        if items.iter().all(|i| i.is_none()) {
            return self.err("Cannot use empty list", line);
        }
        let mut idx = 0i64;
        for it in items {
            let Some(it) = it else {
                idx += 1;
                continue;
            };
            if it.unpack {
                return self.err("Spread operator is not supported in assignments", line);
            }
            let key = match &it.key {
                Some(k) => self.expr(k)?,
                None => {
                    let k = self.int_const(idx);
                    idx += 1;
                    k
                }
            };
            self.free(key);
            match &it.value.kind {
                ExprKind::List { items: sub, .. } | ExprKind::Array { items: sub, .. } => {
                    let t = self.tmp();
                    self.f.line = it.span.line;
                    self.emit(Op::FetchList {
                        dst: t,
                        base: h,
                        key,
                    });
                    let h2 = self.hidden();
                    self.free(t);
                    self.emit(Op::Copy { dst: h2, src: t });
                    self.destructure(sub, h2, line)?;
                    self.emit(Op::Free { slot: h2 });
                    self.free(h2);
                }
                _ => {
                    let t = self.tmp();
                    self.f.line = it.span.line;
                    self.emit(Op::FetchList {
                        dst: t,
                        base: h,
                        key,
                    });
                    self.assign_to(&it.value, t, NONE)?;
                }
            }
        }
        Ok(())
    }

    /// Destructuring where some items are by reference: slot `h` holds the
    /// source (a reference to it when it is a variable).
    fn destructure_ref(
        &mut self,
        items: &'a [Option<ArrayItem<'a>>],
        h: Reg,
        _line: u32,
    ) -> CResult<()> {
        let mut idx = 0i64;
        for it in items {
            let Some(it) = it else {
                idx += 1;
                continue;
            };
            let key = match &it.key {
                Some(k) => self.expr(k)?,
                None => {
                    let k = self.int_const(idx);
                    idx += 1;
                    k
                }
            };
            if it.by_ref {
                // Reference to h[key].
                let d = self.tmp();
                let idx_path = self.f.paths.len() as u32;
                self.f.paths.push(WritePath {
                    base: PathBase::Var(h),
                    elems: vec![PathElem::Dim(key)].into_boxed_slice(),
                    action: PathAction::MakeRef,
                });
                self.free(key);
                self.emit(Op::Write {
                    path: idx_path,
                    res: d,
                });
                self.bind_ref_to(&it.value, d)?;
            } else if let ExprKind::List { items: sub, .. } | ExprKind::Array { items: sub, .. } =
                &it.value.kind
            {
                let d = self.tmp();
                let idx_path = self.f.paths.len() as u32;
                self.f.paths.push(WritePath {
                    base: PathBase::Var(h),
                    elems: vec![PathElem::Dim(key)].into_boxed_slice(),
                    action: PathAction::MakeRef,
                });
                self.free(key);
                self.emit(Op::Write {
                    path: idx_path,
                    res: d,
                });
                let h2 = self.hidden();
                self.free(d);
                self.emit(Op::Copy { dst: h2, src: d });
                self.destructure_ref(sub, h2, _line)?;
                self.emit(Op::Free { slot: h2 });
                self.free(h2);
            } else {
                let t = self.tmp();
                self.free(key);
                self.emit(Op::FetchList {
                    dst: t,
                    base: h,
                    key,
                });
                self.assign_to(&it.value, t, NONE)?;
            }
        }
        Ok(())
    }

    fn assign_ref(&mut self, target: &'a Expr<'a>, value: &'a Expr<'a>, res: Reg) -> CResult<()> {
        let line = target.span.line;
        if let ExprKind::New { .. } = &value.kind {
            return self.err("Cannot assign reference to non referenceable value", line);
        }
        if matches!(target.kind, ExprKind::List { .. } | ExprKind::Array { .. }) {
            return self.err("Cannot assign reference to non referenceable value", line);
        }
        // `$a = &$b` between plain variables.
        if let (ExprKind::Variable(a), ExprKind::Variable(b)) = (&target.kind, &value.kind) {
            if *a == b"this" || *b == b"this" {
                return self.err("Cannot re-assign $this", line);
            }
            let dst = self.cv(a);
            let src = self.cv(b);
            self.emit(Op::AssignRef { dst, src });
            if res != NONE {
                self.emit(Op::Copy { dst: res, src: dst });
            }
            return Ok(());
        }
        let is_call = matches!(
            value.kind,
            ExprKind::Call { .. }
                | ExprKind::MethodCall { .. }
                | ExprKind::StaticCall(_)
                | ExprKind::CallExpr { .. }
        );
        let r = if is_ref_capable(value) {
            // Target path first (PHP fetches the target for write after the
            // source).
            self.ref_of(value)?
        } else if is_call {
            self.check_builtin_write(value)?;
            let v = self.expr(value)?;
            // A function result: by-reference functions return references;
            // others produce the notice at run time.
            let rr = self.tmp();
            self.free(v);
            let path = self.f.paths.len() as u32;
            self.f.paths.push(WritePath {
                base: PathBase::Tmp(v),
                elems: Box::new([]),
                action: PathAction::MakeRef,
            });
            self.emit(Op::Write { path, res: rr });
            rr
        } else {
            return self.err("Cannot assign reference to non referenceable value", line);
        };
        if res != NONE {
            let h = self.hidden();
            self.emit(Op::Copy { dst: h, src: r });
            self.free(r);
            self.bind_ref_to(target, h)?;
            self.emit(Op::Copy { dst: res, src: h });
            self.emit(Op::Free { slot: h });
            self.free(h);
        } else {
            self.bind_ref_to(target, r)?;
            if is_call && is_tmp(r) {
                // Drop the temporary's handle at once (a by-value result
                // bound after its notice is not shared).
                self.emit(Op::Free {
                    slot: r & SLOT_MASK,
                });
            }
        }
        Ok(())
    }

    fn assign_op(
        &mut self,
        op: AssignOp,
        target: &'a Expr<'a>,
        value: &'a Expr<'a>,
        res: Reg,
    ) -> CResult<Reg> {
        let line = target.span.line;
        let bin = match op {
            AssignOp::Add => Bin::Add,
            AssignOp::Sub => Bin::Sub,
            AssignOp::Mul => Bin::Mul,
            AssignOp::Div => Bin::Div,
            AssignOp::Mod => Bin::Mod,
            AssignOp::Pow => Bin::Pow,
            AssignOp::Concat => Bin::Concat,
            AssignOp::BitAnd => Bin::BitAnd,
            AssignOp::BitOr => Bin::BitOr,
            AssignOp::BitXor => Bin::BitXor,
            AssignOp::ShiftLeft => Bin::Shl,
            AssignOp::ShiftRight => Bin::Shr,
            AssignOp::Coalesce => return self.coalesce_assign(target, value, res),
        };
        match &target.kind {
            ExprKind::Variable(n) if *n != b"this" => {
                let var = self.cv(n);
                let v = self.expr(value)?;
                self.free(v);
                self.f.line = line;
                self.emit(Op::AssignOpVar {
                    op: bin,
                    var,
                    src: v,
                    res,
                });
                Ok(res)
            }
            ExprKind::Variable(_) => self.err("Cannot re-assign $this", line),
            ExprKind::List { .. } | ExprKind::Array { .. } => {
                self.err("Cannot use list() as standalone expression", line)
            }
            _ => {
                let p = self.lpath(target, false)?;
                let v = self.expr(value)?;
                self.f.line = line;
                self.emit_path(p, PathAction::AssignOp(bin, v), res);
                Ok(res)
            }
        }
    }

    fn coalesce_assign(
        &mut self,
        target: &'a Expr<'a>,
        value: &'a Expr<'a>,
        res: Reg,
    ) -> CResult<Reg> {
        // $a ??= b: if $a is set and not null, the result is $a; otherwise
        // $a = b. Sub-expressions of the target are evaluated once.
        let line = target.span.line;
        let d = if res == NONE { self.tmp() } else { res };
        match &target.kind {
            ExprKind::Variable(n) if *n != b"this" => {
                let var = self.cv(n);
                let j = self.emit(Op::JmpSet {
                    c: var,
                    res: d,
                    t: 0,
                });
                let v = self.expr(value)?;
                self.free(v);
                self.emit(Op::AssignRes {
                    dst: var,
                    src: v,
                    res: d,
                });
                self.patch_here(j);
            }
            _ => {
                // Evaluate path operands into hidden slots so they are
                // used twice.
                let p = self.lpath(target, false)?;
                let p = self.pin_path(p);
                // Quiet read of the target through the same operands.
                let cur = self.read_path_quiet(&p, line)?;
                self.free(cur);
                let j = self.emit(Op::JmpSet {
                    c: cur,
                    res: d,
                    t: 0,
                });
                let v = self.expr(value)?;
                self.f.line = line;
                let LPath { base, elems } = p;
                let pinned: Vec<Reg> = path_operands(&base, &elems);
                self.free(v);
                let idx = self.f.paths.len() as u32;
                self.f.paths.push(WritePath {
                    base,
                    elems: elems.into_boxed_slice(),
                    action: PathAction::Assign(v),
                });
                self.emit(Op::Write { path: idx, res: d });
                self.patch_here(j);
                for r in pinned {
                    if !is_const(r) && r != NONE && r != THIS && r & SLOT_MASK >= self.f.num_cvs {
                        self.emit(Op::Free {
                            slot: r & SLOT_MASK,
                        });
                        self.free(r);
                    }
                }
            }
        }
        Ok(d)
    }

    /// Moves temporaries of a path into hidden slots (readable twice).
    fn pin_path(&mut self, p: LPath) -> LPath {
        let pin = |c: &mut Self, r: Reg| -> Reg {
            if is_tmp(r) {
                let h = c.hidden();
                c.emit(Op::Copy { dst: h, src: r });
                c.free(r);
                h
            } else {
                r
            }
        };
        let base = match p.base {
            PathBase::Tmp(r) => PathBase::Var(pin(self, r)),
            PathBase::VarVar(r) => PathBase::VarVar(pin(self, r)),
            PathBase::Global(r) => PathBase::Global(pin(self, r)),
            PathBase::StaticProp(c, n) => {
                let c = match c {
                    ClassSpec::Dynamic(r) => ClassSpec::Dynamic(pin(self, r)),
                    c => c,
                };
                PathBase::StaticProp(c, pin(self, n))
            }
            b => b,
        };
        let elems = p
            .elems
            .into_iter()
            .map(|e| match e {
                PathElem::Dim(r) => PathElem::Dim(pin(self, r)),
                PathElem::Prop(r, c) => PathElem::Prop(pin(self, r), c),
                PathElem::Append => PathElem::Append,
            })
            .collect();
        LPath { base, elems }
    }

    /// Reads the value at a (pinned) path without warnings.
    fn read_path_quiet(&mut self, p: &LPath, line: u32) -> CResult<Reg> {
        let mut cur = match p.base {
            PathBase::Var(r) => r,
            PathBase::This => {
                let t = self.tmp();
                self.emit(Op::Special {
                    dst: t,
                    what: Special::This,
                });
                t
            }
            PathBase::Tmp(r) => r,
            PathBase::VarVar(n) => {
                let t = self.tmp();
                self.emit(Op::FetchVarVar {
                    dst: t,
                    name: n,
                    quiet: true,
                });
                t
            }
            PathBase::Global(n) => {
                let t = self.tmp();
                self.emit(Op::FetchGlobal {
                    dst: t,
                    name: n,
                    quiet: true,
                });
                t
            }
            PathBase::StaticProp(c, n) => {
                let t = self.tmp();
                self.emit(Op::IssetStaticProp {
                    dst: t,
                    class: c,
                    name: n,
                    empty: false,
                });
                let d = self.tmp();
                self.free(t);
                let j = self.emit(Op::JmpZEx { c: t, res: d, t: 0 });
                self.emit(Op::FetchStaticProp {
                    dst: d,
                    class: c,
                    name: n,
                });
                self.patch_here(j);
                d
            }
        };
        let this_base = matches!(p.base, PathBase::This);
        for (i, e) in p.elems.iter().enumerate() {
            let d = self.tmp();
            self.free(cur);
            self.f.line = line;
            match *e {
                PathElem::Dim(k) => {
                    self.emit(Op::FetchDimIs {
                        dst: d,
                        base: cur,
                        key: k,
                    });
                }
                PathElem::Append => {
                    return self.err("Cannot use [] for reading", line);
                }
                PathElem::Prop(n, _) => {
                    let o = if i == 0 && this_base { THIS } else { cur };
                    let cache = self.cache();
                    self.emit(Op::FetchPropIs {
                        dst: d,
                        obj: o,
                        name: n,
                        cache,
                    });
                }
            }
            cur = d;
        }
        Ok(cur)
    }

    fn incdec(
        &mut self,
        op: goro_syntax::ast::IncDec,
        target: &'a Expr<'a>,
        res: Reg,
    ) -> CResult<Reg> {
        let line = target.span.line;
        let k = match op {
            goro_syntax::ast::IncDec::PreInc => crate::bytecode::IncDec::PreInc,
            goro_syntax::ast::IncDec::PreDec => crate::bytecode::IncDec::PreDec,
            goro_syntax::ast::IncDec::PostInc => crate::bytecode::IncDec::PostInc,
            goro_syntax::ast::IncDec::PostDec => crate::bytecode::IncDec::PostDec,
        };
        match &target.kind {
            ExprKind::Variable(n) if *n != b"this" => {
                let var = self.cv(n);
                self.f.line = line;
                self.emit(Op::IncDecVar { op: k, var, res });
                Ok(res)
            }
            ExprKind::Variable(_) => self.err("Cannot increment/decrement $this", line),
            ExprKind::Index { .. }
            | ExprKind::Prop { .. }
            | ExprKind::StaticProp(_)
            | ExprKind::VariableVariable(_) => {
                let p = self.lpath(target, false)?;
                self.f.line = line;
                self.emit_path(p, PathAction::IncDec(k), res);
                Ok(res)
            }
            ExprKind::Call { .. } | ExprKind::CallExpr { .. } => {
                self.err("Can't use function return value in write context", line)
            }
            ExprKind::MethodCall { .. } | ExprKind::StaticCall(_) => {
                self.err("Can't use method return value in write context", line)
            }
            _ => self.err(
                "Cannot increment/decrement the result of an expression",
                line,
            ),
        }
    }

    pub(crate) fn unset(&mut self, e: &'a Expr<'a>) -> CResult<()> {
        let line = e.span.line;
        self.f.line = line;
        match &e.kind {
            ExprKind::Variable(n) => {
                if *n == b"this" {
                    return self.err("Cannot unset $this", line);
                }
                if *n == b"GLOBALS" {
                    return self.err(
                        "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                        line,
                    );
                }
                if is_superglobal(n) {
                    // The global itself goes (test Zend/tests/globals_001).
                    let name = self.str_const(n);
                    self.emit(Op::UnsetVarVar { name });
                    return Ok(());
                }
                let var = self.cv(n);
                self.emit(Op::UnsetVar { var });
                Ok(())
            }
            ExprKind::VariableVariable(x) => {
                let name = self.expr(x)?;
                self.free(name);
                self.emit(Op::UnsetVarVar { name });
                Ok(())
            }
            ExprKind::Index { .. } | ExprKind::Prop { .. } | ExprKind::StaticProp(_) => {
                if let ExprKind::Prop { nullsafe: true, .. } = &e.kind {
                    return self.err("Can't use nullsafe operator in write context", line);
                }
                let p = self.lpath(e, true)?;
                self.emit_path(p, PathAction::Unset, NONE);
                Ok(())
            }
            _ => self.err("Cannot use unset() on the result of an expression", line),
        }
    }
}

/// The truth value of a literal (`false`, `0`, `"x"`, `null`...), not of
/// other constants.
fn literal_truth(e: &Expr<'_>) -> Option<bool> {
    match &e.kind {
        ExprKind::Int(i) => Some(*i != 0),
        ExprKind::Float(f) => Some(*f != 0.0),
        ExprKind::String(s, _) => Some(!(s.is_empty() || *s == b"0")),
        ExprKind::Const(n)
            if n.kind == NameKind::Unqualified || n.kind == NameKind::FullyQualified =>
        {
            if n.name.eq_ignore_ascii_case(b"true") {
                Some(true)
            } else if n.name.eq_ignore_ascii_case(b"false") || n.name.eq_ignore_ascii_case(b"null")
            {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `parent::$prop::get(...)` / `parent::$prop::set(...)`: the property
/// name and whether it is `set`.
fn parent_hook_call<'a>(
    sc: &goro_syntax::ast::StaticCallNode<'a>,
    start: u32,
) -> Option<(goro_syntax::ast::StaticPropName<'a>, bool)> {
    let ClassRef::Expr(x) = &sc.class else {
        return None;
    };
    let ExprKind::StaticProp(sp) = &x.kind else {
        return None;
    };
    let ClassRef::Name(n) = &sp.class else {
        return None;
    };
    if n.kind != NameKind::Unqualified || !n.name.eq_ignore_ascii_case(b"parent") {
        return None;
    }
    // `(parent::$prop)::get()` calls a static method of the class named
    // by the static property (test Zend/tests/property_hooks/parent_syntax).
    if start != 0 && start != n.span.start {
        return None;
    }
    let MemberName::Ident(m) = &sc.name else {
        return None;
    };
    let set = if m.name.eq_ignore_ascii_case(b"set") {
        true
    } else if m.name.eq_ignore_ascii_case(b"get") {
        false
    } else {
        return None;
    };
    Some((sp.name, set))
}

/// Whether a value expression is produced by one instruction that may
/// target the assigned variable directly.
fn writes_directly(e: &Expr<'_>) -> bool {
    match &e.kind {
        ExprKind::Binary { op, .. } => !matches!(
            op,
            BinaryOp::BoolAnd | BinaryOp::BoolOr | BinaryOp::Coalesce | BinaryOp::LogicalXor
        ),
        ExprKind::Unary { op, .. } => *op != UnaryOp::Silence,
        ExprKind::Cast { kind, .. } => !matches!(kind, CastKind::Void | CastKind::Unset),
        ExprKind::Interpolated(..) => true,
        ExprKind::Call { args, .. } => !matches!(args, Args::FirstClassCallable),
        ExprKind::MethodCall { args, nullsafe, .. } => {
            !nullsafe && !matches!(args, Args::FirstClassCallable)
        }
        ExprKind::StaticCall(sc) => !matches!(sc.args, Args::FirstClassCallable),
        _ => false,
    }
}

fn contains_nullsafe(e: &Expr<'_>) -> bool {
    match &e.kind {
        ExprKind::Prop { obj, nullsafe, .. } => *nullsafe || contains_nullsafe(obj),
        ExprKind::MethodCall { obj, nullsafe, .. } => *nullsafe || contains_nullsafe(obj),
        ExprKind::Index { base, .. } => contains_nullsafe(base),
        ExprKind::CallExpr { callee, .. } => contains_nullsafe(callee),
        _ => false,
    }
}

fn path_operands(base: &PathBase, elems: &[PathElem]) -> Vec<Reg> {
    let mut v = Vec::new();
    match base {
        PathBase::Var(r) | PathBase::VarVar(r) | PathBase::Tmp(r) | PathBase::Global(r) => {
            v.push(*r)
        }
        PathBase::StaticProp(c, n) => {
            if let ClassSpec::Dynamic(r) = c {
                v.push(*r);
            }
            v.push(*n);
        }
        PathBase::This => {}
    }
    for e in elems {
        match e {
            PathElem::Dim(r) | PathElem::Prop(r, _) => v.push(*r),
            PathElem::Append => {}
        }
    }
    v
}

fn fold_binary(op: BinaryOp, a: &Value, b: &Value) -> Option<Value> {
    use goro_runtime::ops;
    let mut d = Diagnostics::new();
    // Only fold scalar operands (objects/arrays have run-time behaviour).
    let scalar = |v: &Value| {
        matches!(
            v,
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_)
        )
    };
    let arrays = matches!(a, Value::Array(_)) && matches!(b, Value::Array(_));
    if !(scalar(a) && scalar(b)) && !(arrays && op == BinaryOp::Add) {
        return None;
    }
    let r = match op {
        BinaryOp::Add => ops::add(a, b, &mut d).ok()?,
        BinaryOp::Sub => ops::sub(a, b, &mut d).ok()?,
        BinaryOp::Mul => ops::mul(a, b, &mut d).ok()?,
        BinaryOp::Div => ops::div(a, b, &mut d).ok()?,
        BinaryOp::Mod => ops::modulo(a, b, &mut d).ok()?,
        BinaryOp::Pow => ops::pow(a, b, &mut d).ok()?,
        BinaryOp::Concat => Value::Str(ops::concat(a, b, &mut d).ok()?),
        BinaryOp::BitAnd => ops::bit_and(a, b, &mut d).ok()?,
        BinaryOp::BitOr => ops::bit_or(a, b, &mut d).ok()?,
        BinaryOp::BitXor => ops::bit_xor(a, b, &mut d).ok()?,
        BinaryOp::ShiftLeft => ops::shl(a, b, &mut d).ok()?,
        BinaryOp::ShiftRight => ops::shr(a, b, &mut d).ok()?,
        BinaryOp::Equal => Value::Bool(goro_runtime::compare::loose_eq(a, b, &mut d).ok()?),
        BinaryOp::NotEqual => Value::Bool(!goro_runtime::compare::loose_eq(a, b, &mut d).ok()?),
        BinaryOp::Identical => Value::Bool(goro_runtime::compare::strict_eq(a, b)),
        BinaryOp::NotIdentical => Value::Bool(!goro_runtime::compare::strict_eq(a, b)),
        BinaryOp::Less => Value::Bool(goro_runtime::compare::less(a, b, &mut d).ok()?),
        BinaryOp::LessEqual => Value::Bool(goro_runtime::compare::less_eq(a, b, &mut d).ok()?),
        BinaryOp::Greater => Value::Bool(goro_runtime::compare::greater(a, b, &mut d).ok()?),
        BinaryOp::GreaterEqual => {
            Value::Bool(goro_runtime::compare::greater_eq(a, b, &mut d).ok()?)
        }
        BinaryOp::Spaceship => {
            Value::Int(goro_runtime::compare::compare(a, b, &mut d).ok()? as i64)
        }
        BinaryOp::BoolAnd => Value::Bool(
            goro_runtime::convert::to_bool_quiet(a) && goro_runtime::convert::to_bool_quiet(b),
        ),
        BinaryOp::BoolOr => Value::Bool(
            goro_runtime::convert::to_bool_quiet(a) || goro_runtime::convert::to_bool_quiet(b),
        ),
        BinaryOp::LogicalXor => Value::Bool(
            goro_runtime::convert::to_bool_quiet(a) ^ goro_runtime::convert::to_bool_quiet(b),
        ),
        BinaryOp::Coalesce => {
            if a.is_null() {
                b.clone()
            } else {
                a.clone()
            }
        }
        BinaryOp::Pipe => return None,
    };
    if d.notices.is_empty() { Some(r) } else { None }
}

impl<'a> Compiler<'a> {
    /// The functions PHP evaluates specially at compile time cannot be
    /// used in write context (observed on PHP 8.5: "Cannot use result of
    /// built-in function in write context").
    fn check_builtin_write(&self, e: &'a Expr<'a>) -> CResult<()> {
        let ExprKind::Call { name, args } = &e.kind else {
            return Ok(());
        };
        let global = match name.kind {
            NameKind::FullyQualified => true,
            NameKind::Unqualified => self.ns.is_empty(),
            _ => false,
        };
        if !global || matches!(args, Args::FirstClassCallable) {
            return Ok(());
        }
        let list = args.as_slice();
        if list.iter().any(|a| a.unpack || a.name.is_some()) {
            return Ok(());
        }
        let n = list.len();
        let special = match &name.name.to_ascii_lowercase()[..] {
            b"strlen" | b"is_null" | b"is_bool" | b"is_int" | b"is_long" | b"is_integer"
            | b"is_float" | b"is_double" | b"is_string" | b"is_array" | b"is_object"
            | b"is_resource" | b"is_scalar" | b"boolval" | b"intval" | b"floatval"
            | b"doubleval" | b"strval" | b"defined" | b"gettype" => n == 1,
            b"count" | b"sizeof" => n == 1,
            b"get_class" => n <= 1,
            b"get_called_class" | b"func_num_args" | b"func_get_args" => n == 0,
            b"array_key_exists" => n == 2,
            b"sprintf" => n >= 1,
            _ => false,
        };
        if special {
            return self.err(
                "Cannot use result of built-in function in write context",
                e.span.line,
            );
        }
        Ok(())
    }
}
