//! The PHP 8.5 compile-time deprecation of the predefined
//! `$http_response_header` variable (RFC: Deprecations for PHP 8.5; tests
//! ext/standard/tests/http/http_response_header_deprecated_*.phpt).
//!
//! Observed on PHP 8.5: within one function body (or file), each read of
//! the variable warns, until something writes it (an assignment, a
//! reference, a list() element, a write through `[]` or `->`); isset(),
//! empty(), `??`, unset(), compound assignments, increments, `static`,
//! `catch` and closure `use` neither warn nor stop the warnings, while
//! `global` and foreach targets warn. Parameters and closure `use`
//! variables of that name never warn. Nested functions, closures and
//! arrow functions have their own bodies.

use goro_syntax::ast::*;

const NAME: &[u8] = b"http_response_header";

pub(crate) const MESSAGE: &str = "The predefined locally scoped $http_response_header variable is deprecated, call http_get_last_response_headers() instead";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Read,
    Write,
    Neutral,
}

/// Puts these deprecations in line order among themselves: PHP raises
/// them as it compiles, nested bodies in place, while they are found
/// here a body at a time.
pub(crate) fn order(notices: &mut [super::CompileNotice]) {
    let at: Vec<usize> = (0..notices.len())
        .filter(|&i| notices[i].message == MESSAGE)
        .collect();
    let mut lines: Vec<u32> = at.iter().map(|&i| notices[i].line).collect();
    lines.sort();
    for (i, l) in at.into_iter().zip(lines) {
        notices[i].line = l;
    }
}

/// Whether `src` mentions the variable at all (the walk is skipped
/// otherwise).
pub(crate) fn mentioned(src: &[u8]) -> bool {
    src.windows(NAME.len()).any(|w| w == NAME)
}

/// The lines to warn on, for a body whose own variables (parameters,
/// closure uses) already include the name when `defined`.
pub(crate) fn lines_stmts(body: &[Stmt<'_>], defined: bool) -> Vec<u32> {
    let mut w = Walk {
        done: defined,
        lines: Vec::new(),
    };
    w.stmts(body);
    w.lines
}

pub(crate) fn lines_expr(body: &Expr<'_>, defined: bool) -> Vec<u32> {
    let mut w = Walk {
        done: defined,
        lines: Vec::new(),
    };
    w.expr(body, Mode::Read);
    w.lines
}

struct Walk {
    /// A write was seen: no more warnings.
    done: bool,
    lines: Vec<u32>,
}

impl Walk {
    fn stmts(&mut self, s: &[Stmt<'_>]) {
        for st in s {
            self.stmt(st);
        }
    }

    fn opt(&mut self, e: &Option<Expr<'_>>) {
        if let Some(e) = e {
            self.expr(e, Mode::Read);
        }
    }

    fn stmt(&mut self, s: &Stmt<'_>) {
        match &s.kind {
            StmtKind::Expr(e) => self.expr(e, Mode::Read),
            StmtKind::Echo(es) | StmtKind::Global(es) => {
                for e in *es {
                    self.expr(e, Mode::Read)
                }
            }
            StmtKind::Unset(es) => {
                for e in *es {
                    self.expr(e, Mode::Neutral)
                }
            }
            StmtKind::InlineHtml(_) | StmtKind::Nop | StmtKind::Goto(_) | StmtKind::Label(_) => {}
            StmtKind::Block(b) => self.stmts(b),
            StmtKind::If(i) => {
                self.expr(&i.cond, Mode::Read);
                self.stmts(i.then);
                for ei in i.elseifs {
                    self.expr(&ei.cond, Mode::Read);
                    self.stmts(ei.body);
                }
                if let Some(b) = i.else_ {
                    self.stmts(b);
                }
            }
            StmtKind::While { cond, body, .. } => {
                self.expr(cond, Mode::Read);
                self.stmts(body);
            }
            StmtKind::DoWhile { body, cond } => {
                self.stmts(body);
                self.expr(cond, Mode::Read);
            }
            StmtKind::For(f) => {
                for e in f.init.iter().chain(f.cond).chain(f.step) {
                    self.expr(e, Mode::Read);
                }
                self.stmts(f.body);
            }
            StmtKind::Foreach(f) => {
                self.expr(&f.expr, Mode::Read);
                if let Some(k) = &f.key {
                    self.target(k);
                }
                self.target(&f.value);
                self.stmts(f.body);
            }
            StmtKind::Switch(sw) => {
                self.expr(&sw.subject, Mode::Read);
                for c in sw.cases {
                    self.opt(&c.cond);
                    self.stmts(c.body);
                }
            }
            StmtKind::Break(e) | StmtKind::Continue(e) | StmtKind::Return(e) => self.opt(e),
            StmtKind::Static(vs) => {
                for v in *vs {
                    self.opt(&v.default);
                }
            }
            StmtKind::Try(t) => {
                self.stmts(t.body);
                for c in t.catches {
                    self.stmts(c.body);
                }
                if let Some(f) = t.finally {
                    self.stmts(f);
                }
            }
            StmtKind::Declare(d) => match d.body {
                DeclareBody::None => {}
                DeclareBody::Block(b) | DeclareBody::Alt(b) => self.stmts(b),
            },
            StmtKind::Function(_) | StmtKind::Class(_) => {}
            StmtKind::Namespace { body, .. } => {
                if let Some(b) = body {
                    self.stmts(b);
                }
            }
            StmtKind::Use { .. } | StmtKind::GroupUse { .. } | StmtKind::HaltCompiler { .. } => {}
            StmtKind::Const { items, .. } => {
                for c in *items {
                    self.expr(&c.value, Mode::Read);
                }
            }
        }
    }

    /// A foreach key or value: a plain variable warns, destructuring and
    /// other targets write.
    fn target(&mut self, e: &Expr<'_>) {
        match &e.kind {
            ExprKind::Variable(_) => self.expr(e, Mode::Read),
            _ => self.expr(e, Mode::Write),
        }
    }

    fn args(&mut self, a: &Args<'_>) {
        for arg in a.as_slice() {
            self.expr(&arg.value, Mode::Read);
        }
    }

    fn class_ref(&mut self, c: &ClassRef<'_>) {
        if let ClassRef::Expr(e) = c {
            self.expr(e, Mode::Read);
        }
    }

    fn member(&mut self, m: &MemberName<'_>) {
        if let MemberName::Expr(e) = m {
            self.expr(e, Mode::Read);
        }
    }

    fn items(&mut self, items: &[Option<ArrayItem<'_>>], mode: Mode) {
        for it in items.iter().flatten() {
            if let Some(k) = &it.key {
                self.expr(k, Mode::Read);
            }
            let m = if mode == Mode::Write || it.by_ref {
                Mode::Write
            } else {
                Mode::Read
            };
            self.expr(&it.value, m);
        }
    }

    fn expr(&mut self, e: &Expr<'_>, mode: Mode) {
        match &e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::String(..)
            | ExprKind::MagicConst(_)
            | ExprKind::Const(_) => {}
            ExprKind::Interpolated(parts, _) | ExprKind::ShellExec(parts) => {
                for p in *parts {
                    if let StrPart::Expr(x) = p {
                        self.expr(x, Mode::Read);
                    }
                }
            }
            ExprKind::Array { items, .. } => self.items(items, mode),
            ExprKind::List { items, .. } => self.items(items, Mode::Write),
            ExprKind::Variable(n) => {
                if *n == NAME && !self.done {
                    match mode {
                        Mode::Read => self.lines.push(e.span.line),
                        Mode::Write => self.done = true,
                        Mode::Neutral => {}
                    }
                }
            }
            ExprKind::VariableVariable(x) => self.expr(x, Mode::Read),
            ExprKind::Index { base, index } => {
                self.expr(base, mode);
                if let Some(i) = index {
                    self.expr(i, Mode::Read);
                }
            }
            ExprKind::Prop { obj, name, .. } => {
                self.expr(obj, mode);
                self.member(name);
            }
            ExprKind::StaticProp(sp) => {
                self.class_ref(&sp.class);
                if let StaticPropName::Expr(x) = &sp.name {
                    self.expr(x, Mode::Read);
                }
            }
            ExprKind::ClassConst(cc) => {
                self.class_ref(&cc.class);
                self.member(&cc.name);
            }
            ExprKind::Call { args, .. } => self.args(args),
            ExprKind::CallExpr { callee, args } => {
                self.expr(callee, Mode::Read);
                self.args(args);
            }
            ExprKind::MethodCall {
                obj, name, args, ..
            } => {
                self.expr(obj, Mode::Read);
                self.member(name);
                self.args(args);
            }
            ExprKind::StaticCall(sc) => {
                self.class_ref(&sc.class);
                self.member(&sc.name);
                self.args(&sc.args);
            }
            ExprKind::New { class, args } => {
                if let NewClass::Ref(c) = class {
                    self.class_ref(c);
                }
                if let Some(a) = args {
                    self.args(a);
                }
            }
            ExprKind::Assign { target, value } => {
                self.expr(target, Mode::Write);
                self.expr(value, Mode::Read);
            }
            ExprKind::AssignRef { target, value } => {
                self.expr(target, Mode::Write);
                self.expr(value, Mode::Write);
            }
            ExprKind::AssignOp { target, value, .. } => {
                self.expr(target, Mode::Neutral);
                self.expr(value, Mode::Read);
            }
            ExprKind::Binary { op, left, right } => {
                let m = if *op == BinaryOp::Coalesce {
                    Mode::Neutral
                } else {
                    Mode::Read
                };
                self.expr(left, m);
                self.expr(right, Mode::Read);
            }
            ExprKind::IncDec { expr, .. } | ExprKind::Empty(expr) => self.expr(expr, Mode::Neutral),
            ExprKind::Unary { expr, .. }
            | ExprKind::Cast { expr, .. }
            | ExprKind::Print(expr)
            | ExprKind::Throw(expr)
            | ExprKind::Eval(expr)
            | ExprKind::YieldFrom(expr)
            | ExprKind::Include { expr, .. } => self.expr(expr, Mode::Read),
            ExprKind::Ternary { cond, then, else_ } => {
                self.expr(cond, Mode::Read);
                if let Some(t) = then {
                    self.expr(t, Mode::Read);
                }
                self.expr(else_, Mode::Read);
            }
            ExprKind::Instanceof { expr, class } => {
                self.expr(expr, Mode::Read);
                self.class_ref(class);
            }
            ExprKind::Isset(es) => {
                for x in *es {
                    self.expr(x, Mode::Neutral);
                }
            }
            ExprKind::Exit(a) => {
                if let Some(a) = a {
                    self.args(a);
                }
            }
            ExprKind::Clone { expr, args } => {
                if let Some(x) = expr {
                    self.expr(x, Mode::Read);
                }
                if let Some(a) = args {
                    self.args(a);
                }
            }
            ExprKind::Yield { key, value } => {
                if let Some(k) = key {
                    self.expr(k, Mode::Read);
                }
                if let Some(v) = value {
                    self.expr(v, Mode::Read);
                }
            }
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
            ExprKind::Match { subject, arms } => {
                self.expr(subject, Mode::Read);
                for arm in *arms {
                    if let Some(cs) = arm.conds {
                        for c in cs {
                            self.expr(c, Mode::Read);
                        }
                    }
                    self.expr(&arm.body, Mode::Read);
                }
            }
        }
    }
}
