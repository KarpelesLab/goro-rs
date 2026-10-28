//! A compact S-expression rendering of the AST, for tests and debugging.
//!
//! `1 + 2 * $a` renders as `(+ 1 (* 2 $a))`. Statements render one per
//! line. Spans are not shown.

use crate::ast::*;
use std::fmt::Write;

fn bstr(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn quoted(b: &[u8]) -> String {
    let mut s = String::from("\"");
    for &c in b {
        match c {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            b'\r' => s.push_str("\\r"),
            0x20..=0x7e => s.push(c as char),
            _ => {
                let _ = write!(s, "\\x{:02x}", c);
            }
        }
    }
    s.push('"');
    s
}

pub fn name(n: &Name<'_>) -> String {
    match n.kind {
        NameKind::Unqualified | NameKind::Qualified => bstr(n.name),
        NameKind::FullyQualified => format!("\\{}", bstr(n.name)),
        NameKind::Relative => format!("namespace\\{}", bstr(n.name)),
    }
}

fn binop(op: BinaryOp) -> &'static str {
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

fn assignop(op: AssignOp) -> &'static str {
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

fn class_ref(c: &ClassRef<'_>) -> String {
    match c {
        ClassRef::Name(n) => name(n),
        ClassRef::Static(_) => "static".into(),
        ClassRef::Expr(e) => expr(e),
    }
}

fn member(m: &MemberName<'_>) -> String {
    match m {
        MemberName::Ident(i) => bstr(i.name),
        MemberName::Expr(e) => format!("{{{}}}", expr(e)),
    }
}

fn args(a: &Args<'_>) -> String {
    match a {
        Args::FirstClassCallable => " ...".into(),
        Args::List(v) => {
            let mut s = String::new();
            for arg in v.iter() {
                s.push(' ');
                if arg.unpack {
                    s.push_str("...");
                }
                if let Some(n) = &arg.name {
                    let _ = write!(s, "{}:", bstr(n.name));
                }
                s.push_str(&expr(&arg.value));
            }
            s
        }
    }
}

fn items(v: &[Option<ArrayItem<'_>>]) -> String {
    let mut s = String::new();
    for it in v {
        s.push(' ');
        match it {
            None => s.push('_'),
            Some(it) => {
                if it.unpack {
                    s.push_str("...");
                }
                if let Some(k) = &it.key {
                    let _ = write!(s, "{}=>", expr(k));
                }
                if it.by_ref {
                    s.push('&');
                }
                s.push_str(&expr(&it.value));
            }
        }
    }
    s
}

fn parts(v: &[StrPart<'_>]) -> String {
    let mut s = String::new();
    for p in v {
        s.push(' ');
        match p {
            StrPart::Lit(b) => s.push_str(&quoted(b)),
            StrPart::Expr(e) => s.push_str(&expr(e)),
        }
    }
    s
}

pub fn ty(t: &Type<'_>) -> String {
    match t {
        Type::Named(n) => name(n),
        Type::Array(_) => "array".into(),
        Type::Callable(_) => "callable".into(),
        Type::Static(_) => "static".into(),
        Type::Nullable(t, _) => format!("?{}", ty(t)),
        Type::Union(v, _) => v.iter().map(ty).collect::<Vec<_>>().join("|"),
        Type::Intersection(v, _) => format!("({})", v.iter().map(ty).collect::<Vec<_>>().join("&")),
    }
}

fn params(v: &[Param<'_>]) -> String {
    let mut out = Vec::new();
    for p in v {
        let mut s = String::new();
        if !p.modifiers.is_empty() {
            let _ = write!(s, "{} ", mods(p.modifiers));
        }
        if let Some(t) = &p.ty {
            let _ = write!(s, "{} ", ty(t));
        }
        if p.by_ref {
            s.push('&');
        }
        if p.variadic {
            s.push_str("...");
        }
        let _ = write!(s, "${}", bstr(p.name.name));
        if let Some(d) = &p.default {
            let _ = write!(s, "={}", expr(d));
        }
        if !p.hooks.is_empty() {
            let _ = write!(s, " {{{}}}", hooks(p.hooks));
        }
        out.push(s);
    }
    format!("({})", out.join(", "))
}

pub fn mods(m: Modifiers) -> String {
    let names = [
        (Modifiers::PUBLIC, "public"),
        (Modifiers::PROTECTED, "protected"),
        (Modifiers::PRIVATE, "private"),
        (Modifiers::PUBLIC_SET, "public(set)"),
        (Modifiers::PROTECTED_SET, "protected(set)"),
        (Modifiers::PRIVATE_SET, "private(set)"),
        (Modifiers::STATIC, "static"),
        (Modifiers::ABSTRACT, "abstract"),
        (Modifiers::FINAL, "final"),
        (Modifiers::READONLY, "readonly"),
        (Modifiers::VAR, "var"),
    ];
    names
        .iter()
        .filter(|(f, _)| m.has(*f))
        .map(|(_, n)| *n)
        .collect::<Vec<_>>()
        .join(" ")
}

fn hooks(v: &[PropertyHook<'_>]) -> String {
    let mut out = Vec::new();
    for h in v {
        let mut s = String::new();
        if !h.modifiers.is_empty() {
            let _ = write!(s, "{} ", mods(h.modifiers));
        }
        if h.by_ref {
            s.push('&');
        }
        s.push_str(&bstr(h.name.name));
        if let Some(p) = &h.params {
            s.push_str(&params(p));
        }
        match &h.body {
            HookBody::Abstract => s.push(';'),
            HookBody::Expr(e) => {
                let _ = write!(s, " => {}", expr(e));
            }
            HookBody::Block(b) => {
                let _ = write!(s, " {{{}}}", block(b));
            }
        }
        out.push(s);
    }
    out.join(" ")
}

fn ret(r: &Option<Type<'_>>) -> String {
    match r {
        Some(t) => format!(":{}", ty(t)),
        None => String::new(),
    }
}

pub fn expr(e: &Expr<'_>) -> String {
    use ExprKind::*;
    match &e.kind {
        Int(i) => i.to_string(),
        Float(f) => format!("{:?}", f),
        String(s, _) => quoted(s),
        Interpolated(p, _) => format!("(interp{})", parts(p)),
        ShellExec(p) => format!("(shell{})", parts(p)),
        Array { items: v, short } => {
            format!("({}{})", if *short { "[]" } else { "array" }, items(v))
        }
        List { items: v, short } => {
            format!("({}{})", if *short { "list[]" } else { "list" }, items(v))
        }
        MagicConst(m) => format!("{:?}", m),
        Variable(n) => format!("${}", bstr(n)),
        VariableVariable(e) => format!("(${})", expr(e)),
        Index { base, index } => match index {
            Some(i) => format!("([] {} {})", expr(base), expr(i)),
            None => format!("([] {})", expr(base)),
        },
        Prop {
            obj,
            name,
            nullsafe,
        } => format!(
            "({} {} {})",
            if *nullsafe { "?->" } else { "->" },
            expr(obj),
            member(name)
        ),
        StaticProp(StaticPropNode { class, name }) => match name {
            StaticPropName::Ident(i) => format!("(:: {} ${})", class_ref(class), bstr(i.name)),
            StaticPropName::Expr(e) => format!("(:: {} ${{{}}})", class_ref(class), expr(e)),
        },
        Const(n) => name(n),
        ClassConst(ClassConstNode { class, name }) => {
            format!("(:: {} {})", class_ref(class), member(name))
        }
        Call { name: n, args: a } => format!("(call {}{})", name(n), args(a)),
        CallExpr { callee, args: a } => format!("(call {}{})", expr(callee), args(a)),
        MethodCall {
            obj,
            name,
            args: a,
            nullsafe,
        } => {
            format!(
                "({} {} {}{})",
                if *nullsafe { "?->call" } else { "->call" },
                expr(obj),
                member(name),
                args(a)
            )
        }
        StaticCall(StaticCallNode {
            class,
            name,
            args: a,
        }) => format!("(::call {} {}{})", class_ref(class), member(name), args(a)),
        New { class, args: a } => {
            let c = match class {
                NewClass::Ref(c) => class_ref(c),
                NewClass::Anonymous(d) => format!("(class {})", class_body(d)),
            };
            match a {
                Some(a) => format!("(new {}{})", c, args(a)),
                None => format!("(new {})", c),
            }
        }
        Assign { target, value } => format!("(= {} {})", expr(target), expr(value)),
        AssignRef { target, value } => format!("(=& {} {})", expr(target), expr(value)),
        AssignOp { op, target, value } => {
            format!("({} {} {})", assignop(*op), expr(target), expr(value))
        }
        Binary { op, left, right } => format!("({} {} {})", binop(*op), expr(left), expr(right)),
        Unary { op, expr: x } => {
            let o = match op {
                UnaryOp::Not => "!",
                UnaryOp::BitNot => "~",
                UnaryOp::Neg => "neg",
                UnaryOp::Plus => "pos",
                UnaryOp::Silence => "@",
            };
            format!("({} {})", o, expr(x))
        }
        IncDec { op, expr: x } => {
            let o = match op {
                crate::ast::IncDec::PreInc => "++pre",
                crate::ast::IncDec::PreDec => "--pre",
                crate::ast::IncDec::PostInc => "post++",
                crate::ast::IncDec::PostDec => "post--",
            };
            format!("({} {})", o, expr(x))
        }
        Cast { kind, expr: x } => format!("(cast-{:?} {})", kind, expr(x))
            .to_lowercase()
            .replace("cast-", "cast:"),
        Ternary { cond, then, else_ } => match then {
            Some(t) => format!("(? {} {} {})", expr(cond), expr(t), expr(else_)),
            None => format!("(?: {} {})", expr(cond), expr(else_)),
        },
        Instanceof { expr: x, class } => format!("(instanceof {} {})", expr(x), class_ref(class)),
        Isset(v) => format!(
            "(isset {})",
            v.iter().map(expr).collect::<Vec<_>>().join(" ")
        ),
        Empty(x) => format!("(empty {})", expr(x)),
        Eval(x) => format!("(eval {})", expr(x)),
        Include { kind, expr: x } => format!("({:?} {})", kind, expr(x)).to_lowercase(),
        Exit(a) => match a {
            Some(a) => format!("(exit{})", args(a)),
            None => "(exit)".into(),
        },
        Print(x) => format!("(print {})", expr(x)),
        Clone { expr: x, args: a } => match (x, a) {
            (Some(x), _) => format!("(clone {})", expr(x)),
            (None, Some(a)) => format!("(clone-with{})", args(a)),
            (None, None) => "(clone)".into(),
        },
        Throw(x) => format!("(throw {})", expr(x)),
        Yield { key, value } => match (key, value) {
            (Some(k), Some(v)) => format!("(yield {} => {})", expr(k), expr(v)),
            (None, Some(v)) => format!("(yield {})", expr(v)),
            _ => "(yield)".into(),
        },
        YieldFrom(x) => format!("(yield-from {})", expr(x)),
        Closure(c) => {
            let uses: Vec<std::string::String> = c
                .uses
                .iter()
                .map(|u| format!("{}${}", if u.by_ref { "&" } else { "" }, bstr(u.name.name)))
                .collect();
            format!(
                "({}function{}{}{}{} {{{}}})",
                if c.is_static { "static " } else { "" },
                if c.by_ref { "&" } else { "" },
                params(c.params),
                if uses.is_empty() {
                    std::string::String::new()
                } else {
                    format!(" use({})", uses.join(", "))
                },
                ret(&c.ret),
                block(c.body)
            )
        }
        ArrowFn(f) => format!(
            "({}fn{}{}{} => {})",
            if f.is_static { "static " } else { "" },
            if f.by_ref { "&" } else { "" },
            params(f.params),
            ret(&f.ret),
            expr(f.body)
        ),
        Match { subject, arms } => {
            let mut s = format!("(match {}", expr(subject));
            for a in arms.iter() {
                match &a.conds {
                    None => s.push_str(" [default"),
                    Some(c) => {
                        s.push_str(" [");
                        s.push_str(&c.iter().map(expr).collect::<Vec<_>>().join(", "));
                    }
                }
                let _ = write!(s, " => {}]", expr(&a.body));
            }
            s.push(')');
            s
        }
    }
}

fn block(b: &[Stmt<'_>]) -> String {
    b.iter().map(stmt).collect::<Vec<_>>().join(" ")
}

fn attrs(v: &[AttributeGroup<'_>]) -> String {
    let mut s = String::new();
    for g in v {
        s.push_str("#[");
        let a: Vec<std::string::String> = g
            .attrs
            .iter()
            .map(|a| {
                if a.has_args {
                    format!("{}({})", name(&a.name), args_list(a.args))
                } else {
                    name(&a.name)
                }
            })
            .collect();
        s.push_str(&a.join(", "));
        s.push_str("] ");
    }
    s
}

fn args_list(v: &[Arg<'_>]) -> String {
    let mut s = String::new();
    for (i, arg) in v.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        if arg.unpack {
            s.push_str("...");
        }
        if let Some(n) = &arg.name {
            let _ = write!(s, "{}:", bstr(n.name));
        }
        s.push_str(&expr(&arg.value));
    }
    s
}

fn class_body(c: &ClassDecl<'_>) -> String {
    let mut s = String::new();
    if !c.attrs.is_empty() {
        s.push_str(&attrs(c.attrs));
    }
    if !c.modifiers.is_empty() {
        let _ = write!(s, "{} ", mods(c.modifiers));
    }
    let _ = write!(s, "{:?}", c.kind);
    if let Some(n) = &c.name {
        let _ = write!(s, " {}", bstr(n.name));
    }
    if let Some(b) = &c.enum_backing {
        let _ = write!(s, ":{}", ty(b));
    }
    if let Some(e) = &c.extends {
        let _ = write!(s, " extends {}", name(e));
    }
    if !c.implements.is_empty() {
        let _ = write!(
            s,
            " implements {}",
            c.implements.iter().map(name).collect::<Vec<_>>().join(",")
        );
    }
    s.push_str(" {");
    for m in c.members {
        s.push(' ');
        s.push_str(&class_member(m));
    }
    s.push_str(" }");
    s
}

fn class_member(m: &ClassMember<'_>) -> String {
    match m {
        ClassMember::Property(p) => {
            let mut s = attrs(p.attrs);
            let _ = write!(s, "(prop {}", mods(p.modifiers));
            if let Some(t) = &p.ty {
                let _ = write!(s, " {}", ty(t));
            }
            for it in p.props {
                let _ = write!(s, " ${}", bstr(it.name.name));
                if let Some(d) = &it.default {
                    let _ = write!(s, "={}", expr(d));
                }
                if it.has_hook_list {
                    let _ = write!(s, " {{{}}}", hooks(it.hooks));
                }
            }
            s.push(')');
            s
        }
        ClassMember::Const(c) => {
            let mut s = attrs(c.attrs);
            s.push_str("(const");
            if !c.modifiers.is_empty() {
                let _ = write!(s, " {}", mods(c.modifiers));
            }
            if let Some(t) = &c.ty {
                let _ = write!(s, " {}", ty(t));
            }
            for it in c.items {
                let _ = write!(s, " {}={}", bstr(it.name.name), expr(&it.value));
            }
            s.push(')');
            s
        }
        ClassMember::Method(f) => {
            let mut s = attrs(f.attrs);
            let _ = write!(
                s,
                "(method {} {}{}{}{}",
                mods(f.modifiers),
                if f.by_ref { "&" } else { "" },
                bstr(f.name.name),
                params(f.params),
                ret(&f.ret)
            );
            match &f.body {
                Some(b) => {
                    let _ = write!(s, " {{{}}})", block(b));
                }
                None => s.push_str(";)"),
            }
            s
        }
        ClassMember::TraitUse(t) => {
            let mut s = format!(
                "(use {}",
                t.traits.iter().map(name).collect::<Vec<_>>().join(",")
            );
            for a in t.adaptations {
                match a {
                    TraitAdaptation::Precedence {
                        trait_,
                        method,
                        insteadof,
                        ..
                    } => {
                        let _ = write!(
                            s,
                            " [{}::{} insteadof {}]",
                            name(trait_),
                            bstr(method.name),
                            insteadof.iter().map(name).collect::<Vec<_>>().join(",")
                        );
                    }
                    TraitAdaptation::Alias {
                        trait_,
                        method,
                        modifiers,
                        alias,
                        ..
                    } => {
                        s.push_str(" [");
                        if let Some(t) = trait_ {
                            let _ = write!(s, "{}::", name(t));
                        }
                        let _ = write!(s, "{} as", bstr(method.name));
                        if !modifiers.is_empty() {
                            let _ = write!(s, " {}", mods(*modifiers));
                        }
                        if let Some(a) = alias {
                            let _ = write!(s, " {}", bstr(a.name));
                        }
                        s.push(']');
                    }
                }
            }
            s.push(')');
            s
        }
        ClassMember::EnumCase(c) => match &c.value {
            Some(v) => format!("(case {}={})", bstr(c.name.name), expr(v)),
            None => format!("(case {})", bstr(c.name.name)),
        },
    }
}

pub fn stmt(s: &Stmt<'_>) -> String {
    use StmtKind::*;
    match &s.kind {
        Expr(e) => format!("{};", expr(e)),
        Echo(v) => format!(
            "(echo {})",
            v.iter().map(expr).collect::<Vec<_>>().join(" ")
        ),
        InlineHtml(h) => format!("(html {})", quoted(h)),
        Block(b) => format!("{{{}}}", block(b)),
        If(i) => {
            let mut out = format!("(if {} {{{}}}", expr(&i.cond), block(i.then));
            for e in i.elseifs {
                let _ = write!(out, " elseif {} {{{}}}", expr(&e.cond), block(e.body));
            }
            if let Some(e) = &i.else_ {
                let _ = write!(out, " else {{{}}}", block(e));
            }
            out.push(')');
            out
        }
        While { cond, body, .. } => format!("(while {} {{{}}})", expr(cond), block(body)),
        DoWhile { body, cond } => format!("(do {{{}}} {})", block(body), expr(cond)),
        For(f) => {
            let j = |v: &[crate::ast::Expr<'_>]| v.iter().map(expr).collect::<Vec<_>>().join(",");
            format!(
                "(for [{}] [{}] [{}] {{{}}})",
                j(f.init),
                j(f.cond),
                j(f.step),
                block(f.body)
            )
        }
        Foreach(f) => {
            let k = match &f.key {
                Some(k) => format!("{} => ", expr(k)),
                None => String::new(),
            };
            format!(
                "(foreach {} as {}{}{} {{{}}})",
                expr(&f.expr),
                k,
                if f.by_ref { "&" } else { "" },
                expr(&f.value),
                block(f.body)
            )
        }
        Switch(sw) => {
            let mut out = format!("(switch {}", expr(&sw.subject));
            for c in sw.cases {
                match &c.cond {
                    Some(e) => {
                        let _ = write!(out, " [case {}: {}]", expr(e), block(c.body));
                    }
                    None => {
                        let _ = write!(out, " [default: {}]", block(c.body));
                    }
                }
            }
            out.push(')');
            out
        }
        Break(e) => match e {
            Some(e) => format!("(break {})", expr(e)),
            None => "(break)".into(),
        },
        Continue(e) => match e {
            Some(e) => format!("(continue {})", expr(e)),
            None => "(continue)".into(),
        },
        Return(e) => match e {
            Some(e) => format!("(return {})", expr(e)),
            None => "(return)".into(),
        },
        Global(v) => format!(
            "(global {})",
            v.iter().map(expr).collect::<Vec<_>>().join(" ")
        ),
        Static(v) => {
            let vs: Vec<String> = v
                .iter()
                .map(|sv| match &sv.default {
                    Some(d) => format!("${}={}", bstr(sv.name.name), expr(d)),
                    None => format!("${}", bstr(sv.name.name)),
                })
                .collect();
            format!("(static {})", vs.join(" "))
        }
        Unset(v) => format!(
            "(unset {})",
            v.iter().map(expr).collect::<Vec<_>>().join(" ")
        ),
        Try(t) => {
            let mut out = format!("(try {{{}}}", block(t.body));
            for c in t.catches {
                let _ = write!(
                    out,
                    " catch({}{}) {{{}}}",
                    c.types.iter().map(name).collect::<Vec<_>>().join("|"),
                    match &c.var {
                        Some(v) => format!(" ${}", bstr(v.name)),
                        None => String::new(),
                    },
                    block(c.body)
                );
            }
            if let Some(f) = &t.finally {
                let _ = write!(out, " finally {{{}}}", block(f));
            }
            out.push(')');
            out
        }
        Goto(l) => format!("(goto {})", bstr(l.name)),
        Label(l) => format!("(label {})", bstr(l.name)),
        Declare(d) => {
            let dirs: Vec<String> = d
                .directives
                .iter()
                .map(|(n, v)| format!("{}={}", bstr(n.name), expr(v)))
                .collect();
            let body = match &d.body {
                DeclareBody::None => String::new(),
                DeclareBody::Block(b) | DeclareBody::Alt(b) => format!(" {{{}}}", block(b)),
            };
            format!("(declare {}{})", dirs.join(","), body)
        }
        Function(f) => format!(
            "{}(function {}{}{}{} {{{}}})",
            attrs(f.attrs),
            if f.by_ref { "&" } else { "" },
            bstr(f.name.name),
            params(f.params),
            ret(&f.ret),
            block(f.body)
        ),
        Class(c) => format!("({})", class_body(c)),
        Namespace { name: n, body } => {
            let nm = n.as_ref().map(name).unwrap_or_default();
            match body {
                Some(b) => format!("(namespace {} {{{}}})", nm, block(b)),
                None => format!("(namespace {})", nm),
            }
        }
        Use { kind, uses } => {
            let us: Vec<String> = uses
                .iter()
                .map(|u| match &u.alias {
                    Some(a) => format!("{} as {}", name(&u.name), bstr(a.name)),
                    None => name(&u.name),
                })
                .collect();
            format!("(use{} {})", use_kind(*kind), us.join(", "))
        }
        GroupUse { kind, prefix, uses } => {
            let us: Vec<String> = uses
                .iter()
                .map(|u| {
                    let k = if *kind == UseKind::Normal {
                        use_kind(u.kind)
                    } else {
                        ""
                    };
                    match &u.alias {
                        Some(a) => {
                            format!("{}{} as {}", k.trim_start(), name(&u.name), bstr(a.name))
                        }
                        None => format!("{}{}", k.trim_start(), name(&u.name)),
                    }
                })
                .collect();
            format!(
                "(use{} {}\\{{{}}})",
                use_kind(*kind),
                name(prefix),
                us.join(", ")
            )
        }
        Const { attrs: a, items } => {
            let its: Vec<String> = items
                .iter()
                .map(|i| format!("{}={}", bstr(i.name.name), expr(&i.value)))
                .collect();
            format!("{}(const {})", attrs(a), its.join(" "))
        }
        HaltCompiler { offset } => format!("(halt {})", offset),
        Nop => ";".into(),
    }
}

fn use_kind(k: UseKind) -> &'static str {
    match k {
        UseKind::Normal => "",
        UseKind::Function => " function ",
        UseKind::Const => " const ",
    }
}

/// The whole program, one statement per line.
pub fn program(p: &Program<'_>) -> String {
    p.stmts.iter().map(stmt).collect::<Vec<_>>().join("\n")
}
