//! PHP's AST export: the canonical source rendering of an expression (and
//! of the statements and declarations inside closures and anonymous
//! classes) that PHP shows in the message of a failed `assert()` and in
//! other places where it prints code back.
//!
//! The rendering is normalised, not the original text: four-space
//! indentation, `[...]` arrays, single-quoted strings when nothing is
//! interpolated, parentheses chosen by operator precedence, `elseif` /
//! `else if` kept apart, `\exit()` / `\clone()` function forms, and so on.
//! The rules (including PHP's own quirks, such as `interface I implements
//! J` or match arm bodies printed without indentation) come from the
//! expected output of the php-src tests (Zend/tests/assert/expect_015.phpt,
//! expect_020, Zend/tests/pipe_operator/ast.phpt, property_hooks/
//! ast_printing ...) and from spot checks observed on PHP 8.5.

use crate::ast::*;

/// Name rewriting for exports of compiled constant expressions, where PHP
/// shows class, function and constant names resolved (gh9447). Each hook
/// returns `None` to print the name as written.
pub trait Resolver {
    fn class(&self, _n: &Name<'_>) -> Option<Vec<u8>> {
        None
    }
    fn function(&self, _n: &Name<'_>) -> Option<Vec<u8>> {
        None
    }
    fn constant(&self, _n: &Name<'_>) -> Option<Vec<u8>> {
        None
    }
    /// The class of `new` (by default as for [`Resolver::class`]).
    fn new_class(&self, n: &Name<'_>) -> Option<Vec<u8>> {
        self.class(n)
    }
    /// A closure or arrow function compiled into a constant expression,
    /// starting on `line` (PHP shows `Closure({closure:f():3})`).
    fn closure(&self, _line: u32) -> Option<Vec<u8>> {
        None
    }
}

/// Exports one expression with its names as written. `src` is the source
/// the AST was parsed from (used to tell parenthesised arrow functions
/// apart); it may be empty.
pub fn expr(src: &[u8], e: &Expr<'_>) -> Vec<u8> {
    let mut x = Exporter::new(src);
    x.expr(e, 0);
    x.finish()
}

/// Exports the call `name(args)`, as `assert()` describes itself.
pub fn call(src: &[u8], name: &[u8], args: &Args<'_>) -> Vec<u8> {
    let mut x = Exporter::new(src);
    x.out.extend_from_slice(name);
    x.args(args);
    x.finish()
}

// Priorities: a child printed in a context requiring more than its own
// priority is parenthesised.
const P_PRINT: u8 = 60;
/// Operands of `yield` and `yield from`.
const C_YIELD: u8 = 66;
const P_YIELD: u8 = 70;
const P_YIELD_FROM: u8 = 85;
/// The operand of `yield from`.
const C_YIELD_FROM: u8 = 86;
/// Array elements and keys.
const C_ARRAY: u8 = 80;
const P_ASSIGN: u8 = 90;
const P_TERNARY: u8 = 100;
const P_COALESCE: u8 = 110;
const P_UNARY: u8 = 240;
const P_POW: u8 = 250;
const P_INSTANCEOF: u8 = 252;
const P_PRIMARY: u8 = 255;

/// (priority, left context, right context) of a binary operator.
fn binop_prec(op: BinaryOp) -> (u8, u8, u8) {
    use BinaryOp::*;
    let left = |p: u8| (p, p, p + 1);
    match op {
        LogicalXor => left(40),
        BoolOr => left(120),
        BoolAnd => left(130),
        BitOr => left(140),
        BitXor => left(150),
        BitAnd => left(160),
        Equal | NotEqual | Identical | NotIdentical => (170, 171, 171),
        Less | LessEqual | Greater | GreaterEqual | Spaceship => (180, 181, 181),
        Pipe => left(183),
        Concat => left(185),
        ShiftLeft | ShiftRight => left(190),
        Add | Sub => left(200),
        Mul | Div | Mod => left(210),
        Pow => (P_POW, P_POW + 1, P_POW),
        Coalesce => (P_COALESCE, P_COALESCE + 1, P_COALESCE),
    }
}

fn binop_str(op: BinaryOp) -> &'static str {
    use BinaryOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Pow => "**",
        Concat => ".",
        BitAnd => "&",
        BitOr => "|",
        BitXor => "^",
        ShiftLeft => "<<",
        ShiftRight => ">>",
        BoolAnd => "&&",
        BoolOr => "||",
        LogicalXor => "xor",
        Equal => "==",
        NotEqual => "!=",
        Identical => "===",
        NotIdentical => "!==",
        Less => "<",
        LessEqual => "<=",
        Greater => ">",
        GreaterEqual => ">=",
        Spaceship => "<=>",
        Coalesce => "??",
        Pipe => "|>",
    }
}

fn assignop_str(op: AssignOp) -> &'static str {
    use AssignOp::*;
    match op {
        Add => "+=",
        Sub => "-=",
        Mul => "*=",
        Div => "/=",
        Mod => "%=",
        Pow => "**=",
        Concat => ".=",
        BitAnd => "&=",
        BitOr => "|=",
        BitXor => "^=",
        ShiftLeft => "<<=",
        ShiftRight => ">>=",
        Coalesce => "??=",
    }
}

fn is_label(s: &[u8]) -> bool {
    let start = |c: u8| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80;
    match s.first() {
        Some(&c) if start(c) => s[1..].iter().all(|&c| start(c) || c.is_ascii_digit()),
        _ => false,
    }
}

/// A float the way PHP prints it with `precision` 14 (`%.14G`):
/// `0.1`, `1.0E+25`, `1.2345678901234E+14`.
pub fn format_float(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-INF".into() } else { "INF".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let s = format!("{:.13e}", f.abs());
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).collect();
    let mut out = String::new();
    if f < 0.0 {
        out.push('-');
    }
    if !(-4..14).contains(&exp) {
        let mut d = digits.clone();
        while d.len() > 1 && d.last() == Some(&b'0') {
            d.pop();
        }
        out.push(d[0] as char);
        if d.len() > 1 {
            out.push('.');
            out.extend(d[1..].iter().map(|&c| c as char));
        }
        out.push_str(&format!(
            "E{}{}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        ));
        return out;
    }
    let (int, frac): (Vec<u8>, Vec<u8>) = if exp >= 0 {
        let e = exp as usize + 1;
        (digits[..e].to_vec(), digits[e..].to_vec())
    } else {
        let mut fr = vec![b'0'; (-exp - 1) as usize];
        fr.extend_from_slice(&digits);
        (b"0".to_vec(), fr)
    };
    let mut frac = frac;
    while frac.last() == Some(&b'0') {
        frac.pop();
    }
    out.extend(int.iter().map(|&c| c as char));
    if !frac.is_empty() {
        out.push('.');
        out.extend(frac.iter().map(|&c| c as char));
    }
    out
}

/// A float literal as exported: `1.0`, `1.0E+25`.
fn float_literal(f: f64) -> String {
    let s = format_float(f);
    if s.contains(['.', 'N', 'I']) {
        return s;
    }
    match s.find('E') {
        Some(i) => format!("{}.0{}", &s[..i], &s[i..]),
        None => s + ".0",
    }
}

/// The value of a string literal, or of a `.` chain of literals.
fn lit(e: &Expr<'_>) -> Option<Vec<u8>> {
    match e.kind {
        ExprKind::String(s, _) => Some(s.to_vec()),
        ExprKind::Binary {
            op: BinaryOp::Concat,
            ..
        } => {
            let mut v = Vec::new();
            Exporter::fold_concat(e, &mut v).then_some(v)
        }
        _ => None,
    }
}

pub struct Exporter<'s> {
    pub out: Vec<u8>,
    src: &'s [u8],
    resolver: Option<&'s dyn Resolver>,
    /// Current indentation level (four spaces each).
    ind: usize,
}

impl<'s> Exporter<'s> {
    pub fn new(src: &'s [u8]) -> Self {
        Exporter {
            out: Vec::new(),
            src,
            resolver: None,
            ind: 0,
        }
    }

    pub fn with_resolver(src: &'s [u8], r: &'s dyn Resolver) -> Self {
        Exporter {
            resolver: Some(r),
            ..Exporter::new(src)
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }

    fn s(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn b(&mut self, s: &[u8]) {
        self.out.extend_from_slice(s);
    }

    fn indent(&mut self) {
        for _ in 0..self.ind {
            self.out.extend_from_slice(b"    ");
        }
    }

    /// Whether the source has parentheses around `e` (kept in its span).
    fn parenthesized(&self, e: &Expr<'_>) -> bool {
        self.src.get(e.span.start as usize) == Some(&b'(')
    }

    // ===== names =====

    fn name(&mut self, n: &Name<'_>) {
        match n.kind {
            NameKind::Unqualified | NameKind::Qualified => {}
            NameKind::FullyQualified => self.s("\\"),
            NameKind::Relative => self.s("namespace\\"),
        }
        self.b(n.name);
    }

    /// Names in a list (`implements`, `use` of traits, `insteadof`,
    /// `catch`) print without their leading `\` or `namespace\`
    /// (observed on PHP 8.5).
    fn name_list(&mut self, names: &[Name<'_>], sep: &str) {
        for (i, n) in names.iter().enumerate() {
            if i > 0 {
                self.s(sep);
            }
            self.b(n.name);
        }
    }

    fn class_name(&mut self, n: &Name<'_>) {
        if let Some(r) = self.resolver
            && let Some(v) = r.class(n)
        {
            self.b(&v);
            return;
        }
        self.name(n);
    }

    fn class_ref(&mut self, c: &ClassRef<'_>) {
        match c {
            ClassRef::Name(n) => self.class_name(n),
            ClassRef::Static(_) => self.s("static"),
            ClassRef::Expr(e) => self.expr(e, 0),
        }
    }

    fn ty(&mut self, t: &Type<'_>) {
        match t {
            Type::Named(n) => self.class_name(n),
            Type::Array(_) => self.s("array"),
            Type::Callable(_) => self.s("callable"),
            Type::Static(_) => self.s("static"),
            Type::Nullable(t, _) => {
                self.s("?");
                self.ty(t);
            }
            Type::Union(ts, _) | Type::Intersection(ts, _) => {
                let sep = if matches!(t, Type::Union(..)) {
                    "|"
                } else {
                    "&"
                };
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        self.s(sep);
                    }
                    self.ty(t);
                }
            }
        }
    }

    // ===== literals =====

    fn single_quoted(&mut self, s: &[u8]) {
        self.s("'");
        for &c in s {
            if c == b'\'' || c == b'\\' {
                self.out.push(b'\\');
            }
            self.out.push(c);
        }
        self.s("'");
    }

    /// Literal text inside `"..."` or `` `...` ``.
    fn escaped(&mut self, s: &[u8], quote: u8) {
        for &c in s {
            match c {
                b'\n' => self.s("\\n"),
                b'\r' => self.s("\\r"),
                b'\t' => self.s("\\t"),
                0x0c => self.s("\\f"),
                0x0b => self.s("\\v"),
                0x1b => self.s("\\e"),
                b'\\' => self.s("\\\\"),
                b'$' => self.s("\\$"),
                c if c == quote => {
                    self.out.push(b'\\');
                    self.out.push(c);
                }
                c if c < 0x20 => {
                    let v = format!("\\{:03o}", c);
                    self.s(&v);
                }
                c => self.out.push(c),
            }
        }
    }

    fn interpolated(&mut self, parts: &[StrPart<'_>], quote: u8) {
        self.out.push(quote);
        for (i, p) in parts.iter().enumerate() {
            match p {
                StrPart::Lit(s) => self.escaped(s, quote),
                StrPart::Expr(e) => {
                    let simple = match e.kind {
                        ExprKind::Variable(_) => true,
                        ExprKind::VariableVariable(inner) => {
                            lit(inner).is_some() || matches!(inner.kind, ExprKind::Int(_))
                        }
                        _ => false,
                    };
                    // A bare `$name` unless the text after it would
                    // continue the name.
                    let next_continues = match parts.get(i + 1) {
                        Some(StrPart::Lit(s)) => s.first().is_some_and(|&c| {
                            c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 || c == b'['
                        }),
                        _ => false,
                    };
                    let bare = simple
                        && !(next_continues
                            && match e.kind {
                                ExprKind::Variable(_) => true,
                                ExprKind::VariableVariable(inner) => {
                                    lit(inner).is_some_and(|s| is_label(&s))
                                }
                                _ => false,
                            });
                    if bare {
                        self.expr(e, 0);
                    } else {
                        self.s("{");
                        self.expr(e, 0);
                        self.s("}");
                    }
                }
            }
        }
        self.out.push(quote);
    }

    /// The value of a constant `.` chain of literals (PHP folds it while
    /// parsing: `'a' . 1` is `'a1'`).
    fn fold_concat(e: &Expr<'_>, out: &mut Vec<u8>) -> bool {
        match e.kind {
            ExprKind::String(s, _) => out.extend_from_slice(s),
            ExprKind::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
            ExprKind::Float(f) => out.extend_from_slice(format_float(f).as_bytes()),
            ExprKind::Binary {
                op: BinaryOp::Concat,
                left,
                right,
            } => return Self::fold_concat(left, out) && Self::fold_concat(right, out),
            _ => return false,
        }
        true
    }

    /// The variable named by `x` (`${x}`): `$name` for a literal label,
    /// `$$v` for a variable, else `${expr}`.
    fn dollar_name(&mut self, x: &Expr<'_>) {
        if let Some(s) = lit(x) {
            self.var_name(&s);
            return;
        }
        match x.kind {
            ExprKind::Variable(_) | ExprKind::VariableVariable(_) => {
                self.s("$");
                self.expr(x, 0);
            }
            _ => {
                self.s("${");
                self.expr(x, 0);
                self.s("}");
            }
        }
    }

    /// `$name`, or `${name}` when it is not a valid label.
    fn var_name(&mut self, s: &[u8]) {
        if is_label(s) {
            self.s("$");
            self.b(s);
        } else {
            self.s("${");
            self.b(s);
            self.s("}");
        }
    }

    /// A member name after `->` / `::`: a literal name is printed bare (in
    /// braces when it is not a label), a variable as `$v`, anything else
    /// in braces.
    fn member(&mut self, m: &MemberName<'_>) {
        match m {
            MemberName::Ident(i) => self.b(i.name),
            MemberName::Expr(e) => match lit(e) {
                Some(s) if is_label(&s) => self.b(&s),
                Some(s) => {
                    self.s("{");
                    self.b(&s);
                    self.s("}");
                }
                None => match e.kind {
                    ExprKind::Variable(_) | ExprKind::VariableVariable(_) => self.expr(e, 0),
                    _ => {
                        self.s("{");
                        self.expr(e, 0);
                        self.s("}");
                    }
                },
            },
        }
    }

    // ===== expressions =====

    fn args(&mut self, a: &Args<'_>) {
        self.s("(");
        match a {
            Args::FirstClassCallable => self.s("..."),
            Args::List(v) => {
                for (i, arg) in v.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    if let Some(n) = &arg.name {
                        self.b(n.name);
                        self.s(": ");
                    }
                    if arg.unpack {
                        self.s("...");
                    }
                    self.expr(&arg.value, 0);
                }
            }
        }
        self.s(")");
    }

    fn array_items(&mut self, items: &[Option<ArrayItem<'_>>]) {
        let mut n = items.len();
        while n > 0 && items[n - 1].is_none() {
            n -= 1;
        }
        self.s("[");
        for (i, it) in items[..n].iter().enumerate() {
            if i > 0 {
                self.s(", ");
            }
            let Some(it) = it else { continue };
            if it.unpack {
                self.s("...");
            }
            if let Some(k) = &it.key {
                self.expr(k, C_ARRAY);
                self.s(" => ");
            }
            if it.by_ref {
                self.s("&");
            }
            self.expr(&it.value, C_ARRAY);
        }
        self.s("]");
    }

    /// The base of `->`, `[...]` or `::`: parentheses written around an
    /// arrow function there are not kept (observed on PHP 8.5).
    fn deref_base(&mut self, e: &Expr<'_>) {
        if let ExprKind::ArrowFn(f) = e.kind {
            self.arrow_fn(f);
        } else {
            self.expr(e, 0);
        }
    }

    /// Prints `e` in a context of priority `ctx`.
    pub fn expr(&mut self, e: &Expr<'_>, ctx: u8) {
        let p = self.prio(e);
        // Parentheses written around an arrow function are kept.
        if p < ctx || (matches!(e.kind, ExprKind::ArrowFn(_)) && self.parenthesized(e)) {
            self.s("(");
            self.expr_inner(e);
            self.s(")");
        } else {
            self.expr_inner(e);
        }
    }

    fn prio(&self, e: &Expr<'_>) -> u8 {
        match e.kind {
            ExprKind::Binary { op, left, right } => {
                if op == BinaryOp::Concat && Self::fold_concat(left, &mut Vec::new()) {
                    let mut v = Vec::new();
                    if Self::fold_concat(right, &mut v) {
                        return P_PRIMARY;
                    }
                }
                binop_prec(op).0
            }
            ExprKind::Assign { .. } | ExprKind::AssignRef { .. } | ExprKind::AssignOp { .. } => {
                P_ASSIGN
            }
            ExprKind::Ternary { .. } => P_TERNARY,
            ExprKind::Unary { .. } | ExprKind::Cast { .. } => P_UNARY,
            ExprKind::IncDec { .. } => P_UNARY,
            ExprKind::Instanceof { .. } => P_INSTANCEOF,
            ExprKind::Isset(v) if v.len() > 1 => binop_prec(BinaryOp::BoolAnd).0,
            ExprKind::Print(_) => P_PRINT,
            ExprKind::Yield { .. } => P_YIELD,
            ExprKind::YieldFrom(_) => P_YIELD_FROM,
            _ => P_PRIMARY,
        }
    }

    fn expr_inner(&mut self, e: &Expr<'_>) {
        match e.kind {
            ExprKind::Int(i) => self.s(&i.to_string()),
            ExprKind::Float(f) => self.s(&float_literal(f)),
            ExprKind::String(s, _) => self.single_quoted(s),
            ExprKind::Interpolated(parts, _) => self.interpolated(parts, b'"'),
            ExprKind::ShellExec(parts) => self.interpolated(parts, b'`'),
            ExprKind::Array { items, .. } | ExprKind::List { items, .. } => self.array_items(items),
            ExprKind::MagicConst(m) => self.s(match m {
                MagicConst::Line => "__LINE__",
                MagicConst::File => "__FILE__",
                MagicConst::Dir => "__DIR__",
                MagicConst::Class => "__CLASS__",
                MagicConst::Trait => "__TRAIT__",
                MagicConst::Method => "__METHOD__",
                MagicConst::Function => "__FUNCTION__",
                MagicConst::Property => "__PROPERTY__",
                MagicConst::Namespace => "__NAMESPACE__",
            }),
            ExprKind::Variable(n) => {
                self.s("$");
                self.b(n);
            }
            ExprKind::VariableVariable(inner) => self.dollar_name(inner),
            ExprKind::Index { base, index } => {
                if matches!(base.kind, ExprKind::ArrowFn(_)) {
                    self.deref_base(base);
                } else {
                    self.expr(base, P_INSTANCEOF);
                }
                self.s("[");
                if let Some(i) = index {
                    self.expr(i, 0);
                }
                self.s("]");
            }
            ExprKind::Prop {
                obj,
                name,
                nullsafe,
            } => {
                self.deref_base(obj);
                self.s(if nullsafe { "?->" } else { "->" });
                self.member(&name);
            }
            ExprKind::StaticProp(sp) => {
                self.class_ref(&sp.class);
                self.s("::");
                match sp.name {
                    StaticPropName::Ident(i) => {
                        self.s("$");
                        self.b(i.name);
                    }
                    StaticPropName::Expr(x) => self.dollar_name(x),
                }
            }
            ExprKind::Const(n) => {
                if let Some(r) = self.resolver
                    && let Some(v) = r.constant(&n)
                {
                    self.b(&v);
                } else {
                    self.name(&n);
                }
            }
            ExprKind::ClassConst(cc) => {
                self.class_ref(&cc.class);
                self.s("::");
                match cc.name {
                    MemberName::Ident(i) => self.b(i.name),
                    // `A::{expr}` loses its braces (observed on PHP 8.5).
                    MemberName::Expr(x) => match lit(x) {
                        Some(s) => self.b(&s),
                        None => self.expr(x, 0),
                    },
                }
            }
            ExprKind::Call { name, args } => {
                if let Some(r) = self.resolver
                    && let Some(v) = r.function(&name)
                {
                    self.b(&v);
                } else {
                    self.name(&name);
                }
                self.args(&args);
            }
            ExprKind::CallExpr { callee, args } => {
                match callee.kind {
                    ExprKind::ArrowFn(f) => {
                        self.s("(");
                        self.arrow_fn(f);
                        self.s(")");
                    }
                    ExprKind::Closure(c) => {
                        self.s("(");
                        self.closure(c);
                        self.s(")");
                    }
                    _ if let Some(s) = lit(callee) => {
                        self.s("\\");
                        self.b(&s);
                    }
                    _ => self.expr(callee, 0),
                }
                self.args(&args);
            }
            ExprKind::MethodCall {
                obj,
                name,
                args,
                nullsafe,
            } => {
                self.deref_base(obj);
                self.s(if nullsafe { "?->" } else { "->" });
                self.member(&name);
                self.args(&args);
            }
            ExprKind::StaticCall(sc) => {
                self.class_ref(&sc.class);
                self.s("::");
                self.member(&sc.name);
                self.args(&sc.args);
            }
            ExprKind::New { class, args } => {
                self.s("new ");
                match class {
                    NewClass::Ref(c) => {
                        match c {
                            ClassRef::Name(n)
                                if let Some(r) = self.resolver
                                    && let Some(v) = r.new_class(&n) =>
                            {
                                self.b(&v)
                            }
                            _ => self.class_ref(&c),
                        }
                        match args {
                            Some(a) => self.args(&a),
                            None => self.s("()"),
                        }
                    }
                    NewClass::Anonymous(cd) => {
                        self.attrs_inline(cd.attrs);
                        self.s("class");
                        if let Some(a) = args
                            && !a.as_slice().is_empty()
                        {
                            self.args(&a);
                        }
                        self.class_tail(cd);
                    }
                }
            }
            ExprKind::Assign { target, value } => {
                self.expr(target, P_ASSIGN + 1);
                self.s(" = ");
                self.expr(value, P_ASSIGN);
            }
            ExprKind::AssignRef { target, value } => {
                self.expr(target, P_ASSIGN + 1);
                self.s(" =& ");
                self.expr(value, P_ASSIGN);
            }
            ExprKind::AssignOp { op, target, value } => {
                self.expr(target, P_ASSIGN + 1);
                self.s(" ");
                self.s(assignop_str(op));
                self.s(" ");
                self.expr(value, P_ASSIGN);
            }
            ExprKind::Binary { op, left, right } => {
                if op == BinaryOp::Concat {
                    let mut v = Vec::new();
                    if Self::fold_concat(e, &mut v) {
                        self.single_quoted(&v);
                        return;
                    }
                }
                let (_, l, r) = binop_prec(op);
                self.expr(left, l);
                self.s(" ");
                self.s(binop_str(op));
                self.s(" ");
                self.expr(right, r);
            }
            ExprKind::Unary { op, expr } => {
                self.s(match op {
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Neg => "-",
                    UnaryOp::Plus => "+",
                    UnaryOp::Silence => "@",
                });
                self.expr(expr, P_UNARY + 1);
            }
            ExprKind::IncDec { op, expr } => match op {
                IncDec::PreInc => {
                    self.s("++");
                    self.expr(expr, P_UNARY + 1);
                }
                IncDec::PreDec => {
                    self.s("--");
                    self.expr(expr, P_UNARY + 1);
                }
                IncDec::PostInc => {
                    self.expr(expr, P_UNARY + 1);
                    self.s("++");
                }
                IncDec::PostDec => {
                    self.expr(expr, P_UNARY + 1);
                    self.s("--");
                }
            },
            ExprKind::Cast { kind, expr } => {
                self.s(match kind {
                    CastKind::Int => "(int)",
                    CastKind::Float => "(float)",
                    CastKind::String => "(string)",
                    CastKind::Bool => "(bool)",
                    CastKind::Array => "(array)",
                    CastKind::Object => "(object)",
                    CastKind::Unset => "(unset)",
                    CastKind::Void => "(void)",
                });
                self.expr(expr, P_UNARY + 1);
            }
            ExprKind::Ternary { cond, then, else_ } => {
                self.expr(cond, P_TERNARY);
                match then {
                    Some(t) => {
                        self.s(" ? ");
                        self.expr(t, P_TERNARY + 1);
                        self.s(" : ");
                    }
                    None => self.s(" ?: "),
                }
                self.expr(else_, P_TERNARY + 1);
            }
            ExprKind::Instanceof { expr, class } => {
                self.expr(expr, 0);
                self.s(" instanceof ");
                self.class_ref(&class);
            }
            ExprKind::Isset(v) => {
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        self.s(" && ");
                    }
                    self.s("isset(");
                    self.expr(x, 0);
                    self.s(")");
                }
            }
            ExprKind::Empty(x) => {
                self.s("empty(");
                self.expr(x, 0);
                self.s(")");
            }
            ExprKind::Eval(x) => {
                self.s("eval(");
                self.expr(x, 0);
                self.s(")");
            }
            ExprKind::Include { kind, expr } => {
                self.s(match kind {
                    IncludeKind::Include => "include(",
                    IncludeKind::IncludeOnce => "include_once(",
                    IncludeKind::Require => "require(",
                    IncludeKind::RequireOnce => "require_once(",
                });
                self.expr(expr, 0);
                self.s(")");
            }
            ExprKind::Exit(args) => {
                self.s("\\exit");
                match args {
                    Some(a) => self.args(&a),
                    None => self.s("()"),
                }
            }
            ExprKind::Print(x) => {
                self.s("print ");
                self.expr(x, P_PRINT + 1);
            }
            ExprKind::Clone { expr, args } => {
                self.s("\\clone");
                match args {
                    Some(a) => self.args(&a),
                    None => {
                        self.s("(");
                        if let Some(x) = expr {
                            self.expr(x, 0);
                        }
                        self.s(")");
                    }
                }
            }
            ExprKind::Throw(x) => {
                self.s("throw ");
                self.expr(x, 0);
            }
            ExprKind::Yield { key, value } => {
                self.s("yield ");
                if let Some(k) = key {
                    self.expr(k, C_YIELD);
                    self.s(" => ");
                }
                if let Some(v) = value {
                    self.expr(v, C_YIELD);
                }
            }
            ExprKind::YieldFrom(x) => {
                self.s("yield from ");
                self.expr(x, C_YIELD_FROM);
            }
            ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                if let Some(r) = self.resolver
                    && let Some(v) = r.closure(e.span.line) =>
            {
                self.b(&v)
            }
            ExprKind::Closure(c) => self.closure(c),
            ExprKind::ArrowFn(f) => self.arrow_fn(f),
            ExprKind::Match { subject, arms } => {
                self.s("match (");
                self.expr(subject, 0);
                self.s(") {\n");
                let ind = self.ind;
                for arm in arms {
                    self.ind = ind + 1;
                    self.indent();
                    match arm.conds {
                        Some(cs) => {
                            for (i, c) in cs.iter().enumerate() {
                                if i > 0 {
                                    self.s(", ");
                                }
                                self.ind = 0;
                                self.expr(c, 0);
                            }
                        }
                        None => self.s("default"),
                    }
                    self.s(" => ");
                    // Arm bodies are printed without indentation
                    // (observed on PHP 8.5).
                    self.ind = 0;
                    self.expr(&arm.body, 0);
                    self.s(",\n");
                }
                self.ind = ind;
                self.indent();
                self.s("}");
            }
        }
    }

    // ===== functions =====

    fn attr_group(&mut self, g: &AttributeGroup<'_>) {
        self.s("#[");
        for (i, a) in g.attrs.iter().enumerate() {
            if i > 0 {
                self.s(", ");
            }
            self.class_name(&a.name);
            if a.has_args {
                self.args(&Args::List(a.args));
            }
        }
        self.s("]");
    }

    /// `#[A] #[B] ` before a closure, a parameter or `class`.
    fn attrs_inline(&mut self, attrs: &[AttributeGroup<'_>]) {
        for g in attrs {
            self.attr_group(g);
            self.s(" ");
        }
    }

    /// One attribute group per line before a declaration.
    fn attrs_lines(&mut self, attrs: &[AttributeGroup<'_>]) {
        for g in attrs {
            self.indent();
            self.attr_group(g);
            self.s("\n");
        }
    }

    fn params(&mut self, ps: &[Param<'_>]) {
        self.s("(");
        for (i, p) in ps.iter().enumerate() {
            if i > 0 {
                self.s(", ");
            }
            self.attrs_inline(p.attrs);
            let m = p.modifiers;
            for (f, name) in [
                (Modifiers::PUBLIC, "public "),
                (Modifiers::PROTECTED, "protected "),
                (Modifiers::PRIVATE, "private "),
                (Modifiers::PUBLIC_SET, "public(set) "),
                (Modifiers::PROTECTED_SET, "protected(set) "),
                (Modifiers::PRIVATE_SET, "private(set) "),
                (Modifiers::FINAL, "final "),
            ] {
                if m.has(f) {
                    self.s(name);
                }
            }
            if let Some(t) = &p.ty {
                self.ty(t);
                self.s(" ");
            }
            if p.by_ref {
                self.s("&");
            }
            if p.variadic {
                self.s("...");
            }
            self.s("$");
            self.b(p.name.name);
            if let Some(d) = &p.default {
                self.s(" = ");
                self.expr(d, 0);
            }
            if !p.hooks.is_empty() {
                self.hooks(p.hooks);
            }
        }
        self.s(")");
    }

    fn ret(&mut self, r: &Option<Type<'_>>) {
        if let Some(t) = r {
            self.s(": ");
            self.ty(t);
        }
    }

    /// `{` statements `}` at the current indentation.
    fn body(&mut self, b: &[Stmt<'_>]) {
        self.s("{\n");
        self.ind += 1;
        self.stmts(b);
        self.ind -= 1;
        self.indent();
        self.s("}");
    }

    fn closure(&mut self, c: &Closure<'_>) {
        self.attrs_inline(c.attrs);
        if c.is_static {
            self.s("static ");
        }
        self.s("function ");
        if c.by_ref {
            self.s("&");
        }
        self.params(c.params);
        if !c.uses.is_empty() {
            self.s(" use(");
            for (i, u) in c.uses.iter().enumerate() {
                if i > 0 {
                    self.s(", ");
                }
                if u.by_ref {
                    self.s("&");
                }
                self.s("$");
                self.b(u.name.name);
            }
            self.s(")");
        }
        self.ret(&c.ret);
        self.s(" ");
        self.body(c.body);
    }

    fn arrow_fn(&mut self, f: &ArrowFn<'_>) {
        self.attrs_inline(f.attrs);
        if f.is_static {
            self.s("static ");
        }
        self.s("fn");
        if f.by_ref {
            self.s("&");
        }
        self.params(f.params);
        self.ret(&f.ret);
        self.s(" => ");
        self.expr(f.body, 0);
    }

    // ===== classes =====

    /// ` extends A implements B {` members `}` of a class declaration.
    fn class_tail(&mut self, cd: &ClassDecl<'_>) {
        if let Some(t) = &cd.enum_backing {
            self.s(": ");
            self.ty(t);
        }
        if let Some(e) = &cd.extends {
            self.s(" extends ");
            self.class_name(e);
        }
        if !cd.implements.is_empty() {
            // Interfaces print their parents with `implements` too
            // (observed on PHP 8.5).
            self.s(" implements ");
            self.name_list(cd.implements, ", ");
        }
        self.s(" {\n");
        self.ind += 1;
        for m in cd.members {
            self.member_decl(m);
        }
        self.ind -= 1;
        self.indent();
        self.s("}");
    }

    fn visibility(&mut self, m: Modifiers) {
        if m.has(Modifiers::PROTECTED) {
            self.s("protected");
        } else if m.has(Modifiers::PRIVATE) {
            self.s("private");
        } else {
            self.s("public");
        }
    }

    fn hooks(&mut self, hooks: &[PropertyHook<'_>]) {
        self.s(" {\n");
        self.ind += 1;
        for h in hooks {
            self.indent();
            if h.modifiers.has(Modifiers::FINAL) {
                self.s("final ");
            }
            if h.by_ref {
                // Observed on PHP 8.5: `&get` prints as `private(set) get`.
                self.s("private(set) ");
            }
            self.b(h.name.name);
            match h.body {
                HookBody::Abstract => self.s(";\n"),
                HookBody::Expr(e) => {
                    self.s(" => ");
                    self.expr(e, 0);
                    self.s(";\n");
                }
                HookBody::Block(b) => {
                    self.s(" ");
                    self.body(b);
                    self.s("\n");
                }
            }
        }
        self.ind -= 1;
        self.indent();
        self.s("}");
    }

    fn member_decl(&mut self, m: &ClassMember<'_>) {
        match m {
            ClassMember::Const(c) => {
                self.attrs_lines(c.attrs);
                self.indent();
                self.visibility(c.modifiers);
                self.s(" const ");
                if let Some(t) = &c.ty {
                    self.ty(t);
                    self.s(" ");
                }
                for (i, it) in c.items.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    self.b(it.name.name);
                    self.s(" = ");
                    self.expr(&it.value, 0);
                }
                self.s(";\n");
            }
            ClassMember::Property(p) => {
                self.attrs_lines(p.attrs);
                self.indent();
                let m = p.modifiers;
                let mut words: Vec<&str> = Vec::new();
                if m.has(Modifiers::PUBLIC) || m.has(Modifiers::VAR) {
                    words.push("public");
                } else if m.has(Modifiers::PROTECTED) {
                    words.push("protected");
                } else if m.has(Modifiers::PRIVATE) {
                    words.push("private");
                }
                if m.has(Modifiers::PUBLIC_SET) {
                    words.push("public(set)");
                } else if m.has(Modifiers::PROTECTED_SET) {
                    words.push("protected(set)");
                } else if m.has(Modifiers::PRIVATE_SET) {
                    words.push("private(set)");
                }
                if m.has(Modifiers::STATIC) {
                    words.push("static");
                }
                if m.has(Modifiers::READONLY) {
                    words.push("readonly");
                }
                for w in words {
                    self.s(w);
                    self.s(" ");
                }
                if let Some(t) = &p.ty {
                    self.ty(t);
                    self.s(" ");
                }
                let mut hooked = false;
                for (i, it) in p.props.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    self.s("$");
                    self.b(it.name.name);
                    if let Some(d) = &it.default {
                        self.s(" = ");
                        self.expr(d, 0);
                    }
                    if it.has_hook_list {
                        self.hooks(it.hooks);
                        hooked = true;
                    }
                }
                self.s(if hooked { "\n" } else { ";\n" });
            }
            ClassMember::Method(md) => {
                self.attrs_lines(md.attrs);
                self.indent();
                self.visibility(md.modifiers);
                if md.modifiers.has(Modifiers::STATIC) {
                    self.s(" static");
                }
                if md.modifiers.has(Modifiers::ABSTRACT) {
                    self.s(" abstract");
                }
                if md.modifiers.has(Modifiers::FINAL) {
                    self.s(" final");
                }
                self.s(" function ");
                if md.by_ref {
                    self.s("&");
                }
                self.b(md.name.name);
                self.params(md.params);
                self.ret(&md.ret);
                match md.body {
                    Some(b) => {
                        self.s(" ");
                        self.body(b);
                        self.s("\n\n");
                    }
                    None => self.s(";\n\n"),
                }
            }
            ClassMember::TraitUse(tu) => {
                self.indent();
                self.s("use ");
                self.name_list(tu.traits, ", ");
                if tu.adaptations.is_empty() {
                    self.s(";\n");
                    return;
                }
                self.s(" {\n");
                self.ind += 1;
                for a in tu.adaptations {
                    self.indent();
                    match a {
                        TraitAdaptation::Precedence {
                            trait_,
                            method,
                            insteadof,
                            ..
                        } => {
                            self.b(trait_.name);
                            self.s("::");
                            self.b(method.name);
                            self.s(" insteadof ");
                            self.name_list(insteadof, ", ");
                        }
                        TraitAdaptation::Alias {
                            trait_,
                            method,
                            modifiers,
                            alias,
                            ..
                        } => {
                            if let Some(t) = trait_ {
                                self.b(t.name);
                                self.s("::");
                            }
                            self.b(method.name);
                            let vis = modifiers.has(Modifiers::VISIBILITY);
                            if vis || alias.is_some() {
                                self.s(" as");
                            }
                            if vis {
                                self.s(" ");
                                self.visibility(*modifiers);
                            }
                            if let Some(al) = alias {
                                self.s(" ");
                                self.b(al.name);
                            }
                        }
                    }
                    self.s(";\n");
                }
                self.ind -= 1;
                self.indent();
                self.s("}\n");
            }
            ClassMember::EnumCase(c) => {
                self.attrs_lines(c.attrs);
                self.indent();
                self.s("case ");
                self.b(c.name.name);
                if let Some(v) = &c.value {
                    self.s(" = ");
                    self.expr(v, 0);
                }
                self.s(";\n");
            }
        }
    }

    // ===== statements =====

    pub fn stmts(&mut self, list: &[Stmt<'_>]) {
        for s in list {
            self.stmt(s);
        }
    }

    fn line_expr(&mut self, prefix: &str, e: &Expr<'_>, suffix: &str) {
        self.indent();
        self.s(prefix);
        self.expr(e, 0);
        self.s(suffix);
        self.s("\n");
    }

    /// A loop or `if` body, with braces.
    fn block(&mut self, b: &[Stmt<'_>]) {
        self.body(b);
    }

    fn if_chain(&mut self, i: &IfStmt<'_>) {
        self.s("if (");
        self.expr(&i.cond, 0);
        self.s(") ");
        self.block(i.then);
        for ei in i.elseifs {
            self.s(" elseif (");
            self.expr(&ei.cond, 0);
            self.s(") ");
            self.block(ei.body);
        }
        if let Some(e) = i.else_ {
            if !i.alt_syntax
                && let [
                    Stmt {
                        kind: StmtKind::If(inner),
                        ..
                    },
                ] = e
            {
                self.s(" else ");
                self.if_chain(inner);
            } else {
                self.s(" else ");
                self.block(e);
            }
        }
    }

    fn stmt(&mut self, s: &Stmt<'_>) {
        match &s.kind {
            StmtKind::Nop | StmtKind::HaltCompiler { .. } => {}
            StmtKind::Block(b) => self.stmts(b),
            StmtKind::Expr(e) => self.line_expr("", e, ";"),
            StmtKind::Echo(list) => {
                for e in list.iter() {
                    self.line_expr("echo ", e, ";");
                }
            }
            StmtKind::InlineHtml(t) => {
                if !t.is_empty() {
                    self.indent();
                    self.s("echo ");
                    self.single_quoted(t);
                    self.s(";\n");
                }
            }
            StmtKind::If(i) => {
                self.indent();
                self.if_chain(i);
                self.s("\n");
            }
            StmtKind::While { cond, body, .. } => {
                self.indent();
                self.s("while (");
                self.expr(cond, 0);
                self.s(") ");
                self.block(body);
                self.s("\n");
            }
            StmtKind::DoWhile { body, cond } => {
                self.indent();
                self.s("do ");
                self.block(body);
                self.s(" while (");
                self.expr(cond, 0);
                self.s(");\n");
            }
            StmtKind::For(f) => {
                self.indent();
                self.s("for (");
                self.expr_list(f.init);
                self.s(";");
                if !f.cond.is_empty() {
                    self.s(" ");
                    self.expr_list(f.cond);
                }
                self.s(";");
                if !f.step.is_empty() {
                    self.s(" ");
                    self.expr_list(f.step);
                }
                self.s(") ");
                self.block(f.body);
                self.s("\n");
            }
            StmtKind::Foreach(f) => {
                self.indent();
                self.s("foreach (");
                self.expr(&f.expr, 0);
                self.s(" as ");
                if let Some(k) = &f.key {
                    self.expr(k, 0);
                    self.s(" => ");
                }
                if f.by_ref {
                    self.s("&");
                }
                self.expr(&f.value, 0);
                self.s(") ");
                self.block(f.body);
                self.s("\n");
            }
            StmtKind::Switch(sw) => {
                self.indent();
                self.s("switch (");
                self.expr(&sw.subject, 0);
                self.s(") {\n");
                self.ind += 1;
                for c in sw.cases {
                    self.indent();
                    match &c.cond {
                        Some(e) => {
                            self.s("case ");
                            self.expr(e, 0);
                            self.s(":\n");
                        }
                        None => self.s("default:\n"),
                    }
                    self.ind += 1;
                    self.stmts(c.body);
                    self.ind -= 1;
                }
                self.ind -= 1;
                self.indent();
                self.s("}\n");
            }
            StmtKind::Break(l) | StmtKind::Continue(l) | StmtKind::Return(l) => {
                let kw = match s.kind {
                    StmtKind::Break(_) => "break",
                    StmtKind::Continue(_) => "continue",
                    _ => "return",
                };
                self.indent();
                self.s(kw);
                if let Some(e) = l {
                    self.s(" ");
                    self.expr(e, 0);
                }
                self.s(";\n");
            }
            StmtKind::Global(list) => {
                for e in list.iter() {
                    self.line_expr("global ", e, ";");
                }
            }
            StmtKind::Static(vars) => {
                for v in vars.iter() {
                    self.indent();
                    self.s("static $");
                    self.b(v.name.name);
                    if let Some(d) = &v.default {
                        self.s(" = ");
                        self.expr(d, 0);
                    }
                    self.s(";\n");
                }
            }
            StmtKind::Unset(list) => {
                for e in list.iter() {
                    self.line_expr("unset(", e, ");");
                }
            }
            StmtKind::Try(t) => {
                self.indent();
                self.s("try ");
                self.block(t.body);
                for c in t.catches {
                    self.s(" catch (");
                    self.name_list(c.types, "|");
                    if let Some(v) = &c.var {
                        self.s(" $");
                        self.b(v.name);
                    }
                    self.s(") ");
                    self.block(c.body);
                }
                if let Some(f) = t.finally {
                    self.s(" finally ");
                    self.block(f);
                }
                self.s("\n");
            }
            StmtKind::Goto(l) => {
                self.indent();
                self.s("goto ");
                self.b(l.name);
                self.s(";\n");
            }
            StmtKind::Label(l) => {
                self.indent();
                self.b(l.name);
                self.s(":\n");
            }
            StmtKind::Declare(d) => {
                self.indent();
                self.s("declare(");
                for (i, (n, v)) in d.directives.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    self.b(n.name);
                    self.s(" = ");
                    self.expr(v, 0);
                }
                self.s(")");
                match d.body {
                    DeclareBody::None => self.s(";\n"),
                    DeclareBody::Block(b) | DeclareBody::Alt(b) => {
                        self.s(" ");
                        self.block(b);
                        self.s("\n");
                    }
                }
            }
            StmtKind::Function(fd) => {
                self.attrs_lines(fd.attrs);
                self.indent();
                self.s("function ");
                if fd.by_ref {
                    self.s("&");
                }
                self.b(fd.name.name);
                self.params(fd.params);
                self.ret(&fd.ret);
                self.s(" ");
                self.body(fd.body);
                self.s("\n\n");
            }
            StmtKind::Class(cd) => {
                self.attrs_lines(cd.attrs);
                self.indent();
                let m = cd.modifiers;
                if m.has(Modifiers::ABSTRACT) {
                    self.s("abstract ");
                }
                if m.has(Modifiers::FINAL) {
                    self.s("final ");
                }
                if m.has(Modifiers::READONLY) {
                    self.s("readonly ");
                }
                self.s(match cd.kind {
                    ClassKind::Class => "class ",
                    ClassKind::Interface => "interface ",
                    ClassKind::Trait => "trait ",
                    ClassKind::Enum => "enum ",
                });
                if let Some(n) = &cd.name {
                    self.b(n.name);
                }
                self.class_tail(cd);
                self.s("\n\n");
            }
            StmtKind::Namespace { name, body } => {
                self.indent();
                self.s("namespace");
                if let Some(n) = name {
                    self.s(" ");
                    self.b(n.name);
                }
                match body {
                    Some(b) => {
                        self.s(" ");
                        self.block(b);
                        self.s("\n");
                    }
                    None => self.s(";\n"),
                }
            }
            StmtKind::Use { kind, uses } => {
                self.indent();
                self.s("use ");
                self.use_kind(*kind);
                for (i, u) in uses.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    self.b(u.name.name);
                    if let Some(a) = &u.alias {
                        self.s(" as ");
                        self.b(a.name);
                    }
                }
                self.s(";\n");
            }
            StmtKind::GroupUse { kind, prefix, uses } => {
                self.indent();
                self.s("use ");
                self.use_kind(*kind);
                self.b(prefix.name);
                self.s("\\{");
                for (i, u) in uses.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    self.use_kind(u.kind);
                    self.b(u.name.name);
                    if let Some(a) = &u.alias {
                        self.s(" as ");
                        self.b(a.name);
                    }
                }
                self.s("};\n");
            }
            StmtKind::Const { attrs, items } => {
                self.attrs_lines(attrs);
                self.indent();
                self.s("const ");
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        self.s(", ");
                    }
                    self.b(it.name.name);
                    self.s(" = ");
                    self.expr(&it.value, 0);
                }
                self.s(";\n");
            }
        }
    }

    fn use_kind(&mut self, k: UseKind) {
        match k {
            UseKind::Normal => {}
            UseKind::Function => self.s("function "),
            UseKind::Const => self.s("const "),
        }
    }

    fn expr_list(&mut self, list: &[Expr<'_>]) {
        for (i, e) in list.iter().enumerate() {
            if i > 0 {
                self.s(", ");
            }
            self.expr(e, 0);
        }
    }
}
