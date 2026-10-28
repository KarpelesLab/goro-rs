//! Pre-pass over a function body: the variables it names (in order of first
//! appearance, which is the slot order and the order `get_defined_vars()`
//! reports), and whether it is a generator, uses `$this`, or accesses
//! variables by name.

use goro_syntax::ast::*;

/// Superglobals: always the global scope's variables (PHP manual:
/// Superglobals).
pub fn is_superglobal(n: &[u8]) -> bool {
    matches!(
        n,
        b"GLOBALS"
            | b"_SERVER"
            | b"_GET"
            | b"_POST"
            | b"_COOKIE"
            | b"_FILES"
            | b"_ENV"
            | b"_REQUEST"
            | b"_SESSION"
    )
}

#[derive(Default)]
pub struct Scan<'a> {
    pub vars: Vec<&'a [u8]>,
    pub generator: bool,
    pub uses_this: bool,
    pub dynamic: bool,
}

impl<'a> Scan<'a> {
    pub fn add(&mut self, n: &'a [u8]) {
        if n == b"this" {
            self.uses_this = true;
            return;
        }
        if is_superglobal(n) {
            return;
        }
        if !self.vars.contains(&n) {
            self.vars.push(n);
        }
    }

    pub fn stmts(&mut self, s: &'a [Stmt<'a>]) {
        for st in s {
            self.stmt(st);
        }
    }

    fn opt(&mut self, e: &'a Option<Expr<'a>>) {
        if let Some(e) = e {
            self.expr(e);
        }
    }

    pub fn stmt(&mut self, s: &'a Stmt<'a>) {
        match &s.kind {
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Echo(es) | StmtKind::Global(es) | StmtKind::Unset(es) => {
                for e in *es {
                    self.expr(e)
                }
            }
            StmtKind::InlineHtml(_) | StmtKind::Nop | StmtKind::Goto(_) | StmtKind::Label(_) => {}
            StmtKind::Block(b) => self.stmts(b),
            StmtKind::If(i) => {
                self.expr(&i.cond);
                self.stmts(i.then);
                for ei in i.elseifs {
                    self.expr(&ei.cond);
                    self.stmts(ei.body);
                }
                if let Some(b) = i.else_ {
                    self.stmts(b);
                }
            }
            StmtKind::While { cond, body, .. } => {
                self.expr(cond);
                self.stmts(body);
            }
            StmtKind::DoWhile { body, cond } => {
                self.stmts(body);
                self.expr(cond);
            }
            StmtKind::For(f) => {
                for e in f.init.iter().chain(f.cond).chain(f.step) {
                    self.expr(e);
                }
                self.stmts(f.body);
            }
            StmtKind::Foreach(f) => {
                self.expr(&f.expr);
                if let Some(k) = &f.key {
                    self.expr(k);
                }
                self.expr(&f.value);
                self.stmts(f.body);
            }
            StmtKind::Switch(sw) => {
                self.expr(&sw.subject);
                for c in sw.cases {
                    self.opt(&c.cond);
                    self.stmts(c.body);
                }
            }
            StmtKind::Break(e) | StmtKind::Continue(e) | StmtKind::Return(e) => self.opt(e),
            StmtKind::Static(vs) => {
                for v in *vs {
                    self.add(v.name.name);
                    self.opt(&v.default);
                }
            }
            StmtKind::Try(t) => {
                self.stmts(t.body);
                for c in t.catches {
                    if let Some(v) = &c.var {
                        self.add(v.name);
                    }
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
                    self.expr(&c.value);
                }
            }
        }
    }

    fn args(&mut self, a: &'a Args<'a>) {
        for arg in a.as_slice() {
            self.expr(&arg.value);
        }
    }

    fn class_ref(&mut self, c: &'a ClassRef<'a>) {
        if let ClassRef::Expr(e) = c {
            self.expr(e);
        }
    }

    fn member(&mut self, m: &'a MemberName<'a>) {
        if let MemberName::Expr(e) = m {
            self.expr(e);
        }
    }

    fn items(&mut self, items: &'a [Option<ArrayItem<'a>>]) {
        for it in items.iter().flatten() {
            if let Some(k) = &it.key {
                self.expr(k);
            }
            self.expr(&it.value);
        }
    }

    pub fn expr(&mut self, e: &'a Expr<'a>) {
        match &e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::String(..)
            | ExprKind::MagicConst(_)
            | ExprKind::Const(_) => {}
            ExprKind::Interpolated(parts, _) | ExprKind::ShellExec(parts) => {
                for p in *parts {
                    if let StrPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::Array { items, .. } | ExprKind::List { items, .. } => self.items(items),
            ExprKind::Variable(n) => self.add(n),
            ExprKind::VariableVariable(x) => {
                self.dynamic = true;
                self.expr(x);
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                if let Some(i) = index {
                    self.expr(i);
                }
            }
            ExprKind::Prop { obj, name, .. } => {
                self.expr(obj);
                self.member(name);
            }
            ExprKind::StaticProp(sp) => {
                self.class_ref(&sp.class);
                if let StaticPropName::Expr(x) = &sp.name {
                    self.expr(x);
                }
            }
            ExprKind::ClassConst(cc) => {
                self.class_ref(&cc.class);
                self.member(&cc.name);
            }
            ExprKind::Call { name, args } => {
                if name.kind != NameKind::FullyQualified || !name.name.contains(&b'\\') {
                    let n = name.name.rsplit(|&c| c == b'\\').next().unwrap_or(b"");
                    if [
                        &b"compact"[..],
                        b"extract",
                        b"get_defined_vars",
                        b"parse_str",
                        b"func_get_args",
                        b"func_get_arg",
                    ]
                    .iter()
                    .any(|f| n.eq_ignore_ascii_case(f))
                    {
                        self.dynamic = true;
                    }
                }
                self.args(args)
            }
            ExprKind::CallExpr { callee, args } => {
                self.expr(callee);
                self.args(args);
            }
            ExprKind::MethodCall {
                obj, name, args, ..
            } => {
                self.expr(obj);
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
            ExprKind::Assign { target, value }
            | ExprKind::AssignRef { target, value }
            | ExprKind::AssignOp { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            ExprKind::Binary { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Unary { expr, .. }
            | ExprKind::IncDec { expr, .. }
            | ExprKind::Cast { expr, .. }
            | ExprKind::Empty(expr)
            | ExprKind::Print(expr)
            | ExprKind::Throw(expr) => self.expr(expr),
            ExprKind::Eval(x) => {
                self.dynamic = true;
                self.expr(x)
            }
            ExprKind::Include { expr, .. } => {
                self.dynamic = true;
                self.expr(expr)
            }
            ExprKind::Ternary { cond, then, else_ } => {
                self.expr(cond);
                if let Some(t) = then {
                    self.expr(t);
                }
                self.expr(else_);
            }
            ExprKind::Instanceof { expr, class } => {
                self.expr(expr);
                self.class_ref(class);
            }
            ExprKind::Isset(es) => {
                for x in *es {
                    self.expr(x);
                }
            }
            ExprKind::Exit(a) => {
                if let Some(a) = a {
                    self.args(a);
                }
            }
            ExprKind::Clone { expr, args } => {
                if let Some(x) = expr {
                    self.expr(x);
                }
                if let Some(a) = args {
                    self.args(a);
                }
            }
            ExprKind::Yield { key, value } => {
                self.generator = true;
                if let Some(k) = key {
                    self.expr(k);
                }
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            ExprKind::YieldFrom(x) => {
                self.generator = true;
                self.expr(x);
            }
            ExprKind::Closure(c) => {
                for u in c.uses {
                    self.add(u.name.name);
                }
                if !c.is_static {
                    // A closure in a method binds $this.
                }
            }
            ExprKind::ArrowFn(a) => {
                // Variables used in the body are captured from this scope.
                let mut inner = Scan::default();
                inner.expr(a.body);
                for v in inner.vars {
                    if !a.params.iter().any(|p| p.name.name == v) {
                        self.add(v);
                    }
                }
                if inner.uses_this {
                    self.uses_this = true;
                }
            }
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in *arms {
                    if let Some(cs) = arm.conds {
                        for c in cs {
                            self.expr(c);
                        }
                    }
                    self.expr(&arm.body);
                }
            }
        }
    }
}
