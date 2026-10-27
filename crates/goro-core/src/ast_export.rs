//! Port of php-src's `zend_ast_export()` (Zend/zend_ast.c) over goro's AST:
//! the source text `assert()` reports for a failed assertion.
//!
//! The parser hoists anonymous class declarations to the top level of the
//! program; `register_program()` records them so `new class {...}` can be
//! printed with its body.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_parser::ast::*;

thread_local! {
    static ANON_CLASSES: RefCell<HashMap<Vec<u8>, Rc<Statement>>> = RefCell::new(HashMap::new());
}

/// Record the anonymous class declarations of a program about to be
/// compiled (they are needed to print `new class {...}`).
pub fn register_program(program: &Program) {
    fn walk(stmts: &[Statement]) {
        for st in stmts {
            match &st.kind {
                StmtKind::ClassDecl { name, .. } if name.starts_with(b"__anonymous_class_") => {
                    ANON_CLASSES.with(|m| {
                        m.borrow_mut().insert(name.clone(), Rc::new(st.clone()));
                    });
                }
                StmtKind::NamespaceDecl {
                    body: Some(body), ..
                } => walk(body),
                _ => {}
            }
        }
    }
    walk(&program.statements);
    // Anonymous classes of never-compiled code (only printed).
    walk(&goro_parser::take_dead_anon_classes());
}

fn anon_class(name: &[u8]) -> Option<Rc<Statement>> {
    ANON_CLASSES.with(|m| m.borrow().get(name).cloned())
}

/// zend_ast_export() of a whole script (zend_test_compile_to_ast()).
pub fn export_program(program: &Program) -> String {
    register_program(program);
    let mut s = String::new();
    export_stmts(&mut s, &program.statements, 0);
    s
}

/// zend_ast_export("", expr, "").
pub fn export_expr(e: &Expr) -> String {
    let mut s = String::new();
    ex(&mut s, e, 0, 0);
    s
}

fn lossy(b: &[u8]) -> std::borrow::Cow<'_, str> {
    crate::string::lossy(b)
}

fn indent_to(s: &mut String, indent: usize) {
    for _ in 0..indent {
        s.push_str("    ");
    }
}

/// zend_ast_export_str: the body of a single-quoted string.
fn export_str(s: &mut String, b: &[u8]) {
    let mut out = Vec::with_capacity(b.len());
    for &c in b {
        if c == b'\'' || c == b'\\' {
            out.push(b'\\');
        }
        out.push(c);
    }
    s.push_str(&lossy(&out));
}

/// zend_ast_export_qstr: the body of a double-quoted (or backtick) string.
fn export_qstr(s: &mut String, quote: u8, b: &[u8]) {
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    for &c in b {
        if c < b' ' {
            match c {
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                0x0c => out.extend_from_slice(b"\\f"),
                0x0b => out.extend_from_slice(b"\\v"),
                0x1b => out.extend_from_slice(b"\\e"),
                _ => {
                    out.extend_from_slice(b"\\0");
                    out.push(b'0' + c / 8);
                    out.push(b'0' + c % 8);
                }
            }
        } else {
            if c == quote || c == b'$' || c == b'\\' {
                out.push(b'\\');
            }
            out.push(c);
        }
    }
    s.push_str(&lossy(&out));
}

fn valid_var_char(c: u8) -> bool {
    c == b'_' || c >= 127 || c.is_ascii_alphanumeric()
}

fn valid_var_name(b: &[u8]) -> bool {
    match b.first() {
        None => false,
        Some(&c) if c != b'_' && c < 127 && !c.is_ascii_alphabetic() => false,
        _ => b[1..].iter().all(|&c| valid_var_char(c)),
    }
}

/// smart_str_append_double(precision, zero_fraction = true).
fn export_double(s: &mut String, f: f64) {
    let p = crate::value::get_php_precision();
    let p = if p <= 0 { 17 } else { p as usize };
    let mut t = crate::value::format_php_float_with_precision_pub(f, p);
    if f.is_finite() && !t.contains(['.', 'E', 'e']) {
        t.push_str(".0");
    }
    s.push_str(&t);
}

/// zend_ast_export_var: a variable / property / method name.
fn export_var(s: &mut String, e: &Expr, indent: usize) {
    match &e.kind {
        ExprKind::Identifier(n) | ExprKind::String(n) if valid_var_name(n) => {
            s.push_str(&lossy(n));
        }
        ExprKind::Variable(_) | ExprKind::DynamicVariable(_) => ex(s, e, 0, indent),
        ExprKind::Identifier(n) | ExprKind::String(n) => {
            s.push('{');
            s.push_str(&lossy(n));
            s.push('}');
        }
        _ => {
            s.push('{');
            ex(s, e, 0, indent);
            s.push('}');
        }
    }
}

/// zend_ast_export_ns_name: a class / function name as written.
fn export_ns_name(s: &mut String, e: &Expr, indent: usize) {
    match &e.kind {
        ExprKind::Identifier(n) => s.push_str(&lossy(n)),
        ExprKind::ConstantAccess(parts) => s.push_str(&join_name(parts)),
        _ => ex(s, e, 0, indent),
    }
}

fn join_name(parts: &[Vec<u8>]) -> String {
    parts
        .iter()
        .map(|p| lossy(p).into_owned())
        .collect::<Vec<_>>()
        .join("\\")
}

fn export_list(s: &mut String, items: &[Expr], prio: i32, indent: usize) {
    for (i, e) in items.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        ex(s, e, prio, indent);
    }
}

fn export_args(s: &mut String, args: &[Argument], indent: usize) {
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        if let Some(n) = &a.name {
            s.push_str(&lossy(n));
            s.push_str(": ");
        }
        if a.unpack {
            s.push_str("...");
        }
        ex(s, &a.value, 20, indent);
    }
}

fn export_type(s: &mut String, t: &TypeHint) {
    match t {
        TypeHint::Simple(n) => s.push_str(&lossy(n)),
        TypeHint::Nullable(inner) => {
            s.push('?');
            export_type(s, inner);
        }
        TypeHint::Union(ts) => {
            for (i, t) in ts.iter().enumerate() {
                if i > 0 {
                    s.push('|');
                }
                export_type(s, t);
            }
        }
        TypeHint::Intersection(ts) => {
            for (i, t) in ts.iter().enumerate() {
                if i > 0 {
                    s.push('&');
                }
                export_type(s, t);
            }
        }
    }
}

/// One `#[...]` per attribute group, as written.
fn export_attributes(s: &mut String, attrs: &[Attribute], indent: usize, newlines: bool) {
    let mut i = 0;
    while i < attrs.len() {
        let group = attrs[i].group;
        s.push_str("#[");
        let start = i;
        while i < attrs.len() && attrs[i].group == group {
            let a = &attrs[i];
            if i > start {
                s.push_str(", ");
            }
            s.push_str(&join_name(&a.name));
            if a.has_args {
                s.push('(');
                export_args(s, &a.args, indent);
                s.push(')');
            }
            i += 1;
        }
        s.push(']');
        if newlines {
            s.push('\n');
            indent_to(s, indent);
        } else {
            s.push(' ');
        }
    }
}

fn visibility_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public ",
        Visibility::Protected => "protected ",
        Visibility::Private => "private ",
    }
}

fn set_visibility_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public(set) ",
        Visibility::Protected => "protected(set) ",
        Visibility::Private => "private(set) ",
    }
}

fn binary_op(op: BinaryOp) -> (&'static str, i32, i32, i32) {
    match op {
        BinaryOp::Add => (" + ", 200, 200, 201),
        BinaryOp::Sub => (" - ", 200, 200, 201),
        BinaryOp::Mul => (" * ", 210, 210, 211),
        BinaryOp::Div => (" / ", 210, 210, 211),
        BinaryOp::Mod => (" % ", 210, 210, 211),
        BinaryOp::ShiftLeft => (" << ", 190, 190, 191),
        BinaryOp::ShiftRight => (" >> ", 190, 190, 191),
        BinaryOp::Concat => (" . ", 185, 185, 186),
        BinaryOp::BitwiseOr => (" | ", 140, 140, 141),
        BinaryOp::BitwiseAnd => (" & ", 160, 160, 161),
        BinaryOp::BitwiseXor => (" ^ ", 150, 150, 151),
        BinaryOp::Identical => (" === ", 170, 171, 171),
        BinaryOp::NotIdentical => (" !== ", 170, 171, 171),
        BinaryOp::Equal => (" == ", 170, 171, 171),
        BinaryOp::NotEqual => (" != ", 170, 171, 171),
        BinaryOp::Less => (" < ", 180, 181, 181),
        BinaryOp::LessEqual => (" <= ", 180, 181, 181),
        BinaryOp::Greater => (" > ", 180, 181, 181),
        BinaryOp::GreaterEqual => (" >= ", 180, 181, 181),
        BinaryOp::Pow => (" ** ", 250, 251, 250),
        BinaryOp::LogicalXor => (" xor ", 40, 40, 41),
        BinaryOp::Spaceship => (" <=> ", 180, 181, 181),
        BinaryOp::BooleanAnd | BinaryOp::LogicalAnd => (" && ", 130, 130, 131),
        BinaryOp::BooleanOr | BinaryOp::LogicalOr => (" || ", 120, 120, 121),
        BinaryOp::Coalesce => (" ?? ", 110, 111, 110),
    }
}

fn assign_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => " += ",
        BinaryOp::Sub => " -= ",
        BinaryOp::Mul => " *= ",
        BinaryOp::Div => " /= ",
        BinaryOp::Mod => " %= ",
        BinaryOp::ShiftLeft => " <<= ",
        BinaryOp::ShiftRight => " >>= ",
        BinaryOp::Concat => " .= ",
        BinaryOp::BitwiseOr => " |= ",
        BinaryOp::BitwiseAnd => " &= ",
        BinaryOp::BitwiseXor => " ^= ",
        BinaryOp::Pow => " **= ",
        BinaryOp::Coalesce => " ??= ",
        _ => " = ",
    }
}

fn binary(
    s: &mut String,
    op: &str,
    (p, pl, pr): (i32, i32, i32),
    l: &Expr,
    r: &Expr,
    prio: i32,
    indent: usize,
) {
    if prio > p {
        s.push('(');
    }
    ex(s, l, pl, indent);
    s.push_str(op);
    ex(s, r, pr, indent);
    if prio > p {
        s.push(')');
    }
}

fn prefix(s: &mut String, op: &str, (p, pl): (i32, i32), e: &Expr, prio: i32, indent: usize) {
    if prio > p {
        s.push('(');
    }
    s.push_str(op);
    ex(s, e, pl, indent);
    if prio > p {
        s.push(')');
    }
}

fn func_op(s: &mut String, op: &str, e: &Expr, indent: usize) {
    s.push_str(op);
    s.push('(');
    ex(s, e, 0, indent);
    s.push(')');
}

/// zend_ast_export_ex for expressions.
fn ex(s: &mut String, e: &Expr, prio: i32, indent: usize) {
    match &e.kind {
        ExprKind::Int(n) => s.push_str(&n.to_string()),
        ExprKind::Float(f) => export_double(s, *f),
        ExprKind::String(b) => {
            s.push('\'');
            export_str(s, b);
            s.push('\'');
        }
        ExprKind::InterpolatedString(parts) => {
            s.push('"');
            export_encaps(s, b'"', parts, indent);
            s.push('"');
        }
        // ZEND_AST_SHELL_EXEC.
        ExprKind::ShellExec(call) => {
            s.push('`');
            if let ExprKind::FunctionCall { args, .. } = &call.kind {
                match args.first().map(|a| &a.value.kind) {
                    Some(ExprKind::String(b)) => export_qstr(s, b'`', b),
                    Some(ExprKind::InterpolatedString(parts)) => {
                        export_encaps(s, b'`', parts, indent)
                    }
                    _ => {}
                }
            }
            s.push('`');
        }
        ExprKind::True => s.push_str("true"),
        ExprKind::False => s.push_str("false"),
        ExprKind::Null => s.push_str("null"),
        ExprKind::Array(elems) => {
            s.push('[');
            for (i, el) in elems.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                if el.unpack {
                    s.push_str("...");
                    ex(s, &el.value, 80, indent);
                    continue;
                }
                if let Some(k) = &el.key {
                    ex(s, k, 80, indent);
                    s.push_str(" => ");
                }
                if el.by_ref {
                    s.push('&');
                }
                ex(s, &el.value, 80, indent);
            }
            s.push(']');
        }
        ExprKind::Variable(n) => {
            s.push('$');
            if valid_var_name(n) {
                s.push_str(&lossy(n));
            } else {
                s.push_str("{'");
                export_str(s, n);
                s.push_str("'}");
            }
        }
        ExprKind::DynamicVariable(inner) => {
            s.push('$');
            export_var(s, inner, indent);
        }
        ExprKind::BinaryOp { op, left, right } => {
            let (o, p, pl, pr) = binary_op(*op);
            binary(s, o, (p, pl, pr), left, right, prio, indent);
        }
        ExprKind::UnaryOp {
            op,
            operand,
            prefix: is_prefix,
        } => {
            let o = match op {
                UnaryOp::Negate => "-",
                UnaryOp::Plus => "+",
                UnaryOp::BitwiseNot => "~",
                UnaryOp::BooleanNot => "!",
                UnaryOp::PreIncrement | UnaryOp::PostIncrement => "++",
                UnaryOp::PreDecrement | UnaryOp::PostDecrement => "--",
            };
            let postfix = matches!(op, UnaryOp::PostIncrement | UnaryOp::PostDecrement)
                || (!is_prefix && matches!(op, UnaryOp::PreIncrement | UnaryOp::PreDecrement));
            if postfix {
                if prio > 240 {
                    s.push('(');
                }
                ex(s, operand, 241, indent);
                s.push_str(o);
                if prio > 240 {
                    s.push(')');
                }
            } else {
                prefix(s, o, (240, 241), operand, prio, indent);
            }
        }
        ExprKind::Assign { target, value } => {
            binary(s, " = ", (90, 91, 90), target, value, prio, indent)
        }
        ExprKind::AssignRef { target, value } => {
            binary(s, " =& ", (90, 91, 90), target, value, prio, indent)
        }
        ExprKind::CompoundAssign { op, target, value } => {
            binary(s, assign_op(*op), (90, 91, 90), target, value, prio, indent)
        }
        ExprKind::PropertyAccess {
            object,
            property,
            nullsafe,
        } => {
            ex(s, object, 0, indent);
            s.push_str(if *nullsafe { "?->" } else { "->" });
            export_var(s, property, indent);
        }
        ExprKind::StaticPropertyAccess {
            class, property, ..
        } => {
            export_ns_name(s, class, indent);
            s.push_str("::$");
            if valid_var_name(property) {
                s.push_str(&lossy(property));
            } else {
                s.push_str("{'");
                export_str(s, property);
                s.push_str("'}");
            }
        }
        ExprKind::DynamicStaticPropertyAccess { class, property } => {
            export_ns_name(s, class, indent);
            s.push_str("::$");
            export_var(s, property, indent);
        }
        ExprKind::MethodCall {
            object,
            method,
            args,
            nullsafe,
        } => {
            ex(s, object, 0, indent);
            s.push_str(if *nullsafe { "?->" } else { "->" });
            export_var(s, method, indent);
            s.push('(');
            export_args(s, args, indent);
            s.push(')');
        }
        ExprKind::StaticMethodCall {
            class,
            method,
            args,
            ..
        } => {
            // parent::$prop::get() / parent::$prop::set() (lowered by the
            // parser to the hook methods).
            let hook = method
                .strip_prefix(b"__property_get_".as_slice())
                .map(|p| (p, "get"))
                .or_else(|| {
                    method
                        .strip_prefix(b"__property_set_".as_slice())
                        .map(|p| (p, "set"))
                });
            export_ns_name(s, class, indent);
            match hook {
                Some((prop, kind)) => {
                    s.push_str("::$");
                    s.push_str(&lossy(prop));
                    s.push_str("::");
                    s.push_str(kind);
                }
                None => {
                    s.push_str("::");
                    s.push_str(&lossy(method));
                }
            }
            s.push('(');
            export_args(s, args, indent);
            s.push(')');
        }
        ExprKind::DynamicStaticMethodCall {
            class,
            method,
            args,
        } => {
            export_ns_name(s, class, indent);
            s.push_str("::");
            export_var(s, method, indent);
            s.push('(');
            export_args(s, args, indent);
            s.push(')');
        }
        ExprKind::ArrayAccess { array, index } => {
            ex(s, array, 260, indent);
            s.push('[');
            if let Some(i) = index {
                ex(s, i, 0, indent);
            }
            s.push(']');
        }
        ExprKind::FunctionCall { name, args } => {
            if matches!(name.kind, ExprKind::Closure(_) | ExprKind::ArrowFunction(_)) {
                s.push('(');
                ex(s, name, 0, indent);
                s.push(')');
            } else {
                export_ns_name(s, name, indent);
            }
            s.push('(');
            export_args(s, args, indent);
            s.push(')');
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            if prio > 100 {
                s.push('(');
            }
            ex(s, condition, 100, indent);
            match if_true {
                Some(t) => {
                    s.push_str(" ? ");
                    ex(s, t, 101, indent);
                    s.push_str(" : ");
                }
                None => s.push_str(" ?: "),
            }
            ex(s, if_false, 101, indent);
            if prio > 100 {
                s.push(')');
            }
        }
        ExprKind::NullCoalesce { left, right } => {
            binary(s, " ?? ", (110, 111, 110), left, right, prio, indent)
        }
        ExprKind::Match { subject, arms } => {
            s.push_str("match (");
            ex(s, subject, 0, indent);
            s.push_str(") {\n");
            for arm in arms {
                indent_to(s, indent + 1);
                match &arm.conditions {
                    Some(c) => {
                        export_list(s, c, 0, indent + 1);
                        s.push_str(" => ");
                    }
                    None => s.push_str("default => "),
                }
                ex(s, &arm.body, 0, 0);
                s.push_str(",\n");
            }
            indent_to(s, indent);
            s.push('}');
        }
        ExprKind::Closure(decl) => {
            let ClosureExpr {
                is_static,
                params,
                use_vars,
                return_type,
                body,
                attributes,
                returns_by_ref,
                ..
            } = &**decl;
            export_attributes(s, attributes, indent, false);
            if *is_static {
                s.push_str("static ");
            }
            s.push_str("function ");
            if *returns_by_ref {
                s.push('&');
            }
            s.push('(');
            export_params(s, params, indent);
            s.push(')');
            if !use_vars.is_empty() {
                s.push_str(" use(");
                for (i, u) in use_vars.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    if u.by_ref {
                        s.push('&');
                    }
                    s.push('$');
                    s.push_str(&lossy(&u.variable));
                }
                s.push(')');
            }
            if let Some(t) = return_type {
                s.push_str(": ");
                export_type(s, t);
            }
            s.push_str(" {\n");
            export_stmts(s, body, indent + 1);
            indent_to(s, indent);
            s.push('}');
        }
        ExprKind::ArrowFunction(decl) => {
            let ArrowFnExpr {
                is_static,
                params,
                return_type,
                body,
                attributes,
                returns_by_ref,
                parenthesized,
                ..
            } = &**decl;
            if *parenthesized {
                s.push('(');
            }
            export_attributes(s, attributes, indent, false);
            if *is_static {
                s.push_str("static ");
            }
            s.push_str("fn");
            if *returns_by_ref {
                s.push('&');
            }
            s.push('(');
            export_params(s, params, indent);
            s.push(')');
            if let Some(t) = return_type {
                s.push_str(": ");
                export_type(s, t);
            }
            s.push_str(" => ");
            ex(s, body, 0, indent);
            if *parenthesized {
                s.push(')');
            }
        }
        ExprKind::New { class, args } => {
            s.push_str("new ");
            let anon = match &class.kind {
                ExprKind::Identifier(n) if n.starts_with(b"__anonymous_class_") => anon_class(n),
                _ => None,
            };
            match anon.as_deref().map(|st| &st.kind) {
                Some(StmtKind::ClassDecl {
                    extends,
                    implements,
                    body,
                    attributes,
                    ..
                }) => {
                    export_attributes(s, attributes, indent, false);
                    s.push_str("class");
                    if !args.is_empty() {
                        s.push('(');
                        export_args(s, args, indent);
                        s.push(')');
                    }
                    export_class_no_header(s, extends.as_deref(), implements, body, indent);
                }
                _ => {
                    export_ns_name(s, class, indent);
                    s.push('(');
                    export_args(s, args, indent);
                    s.push(')');
                }
            }
        }
        ExprKind::Instanceof { expr, class } => {
            ex(s, expr, 0, indent);
            s.push_str(" instanceof ");
            export_ns_name(s, class, indent);
        }
        ExprKind::Cast(t, inner) => {
            let o = match t {
                CastType::Int => "(int)",
                CastType::Float => "(float)",
                CastType::String => "(string)",
                CastType::Bool => "(bool)",
                CastType::Array => "(array)",
                CastType::Object => "(object)",
                CastType::Unset => "(unset)",
                CastType::Void => "(void)",
            };
            prefix(s, o, (240, 241), inner, prio, indent);
        }
        ExprKind::Print(inner) => prefix(s, "print ", (60, 61), inner, prio, indent),
        ExprKind::Exit(arg) => {
            s.push_str("\\exit(");
            if let Some(a) = arg {
                ex(s, a, 20, indent);
            }
            s.push(')');
        }
        ExprKind::Empty(inner) => func_op(s, "empty", inner, indent),
        ExprKind::Isset(items) => {
            // isset($a, $b) is isset($a) && isset($b).
            if items.len() > 1 && prio > 130 {
                s.push('(');
            }
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    s.push_str(" && ");
                }
                func_op(s, "isset", it, indent);
            }
            if items.len() > 1 && prio > 130 {
                s.push(')');
            }
        }
        ExprKind::Eval(inner) => func_op(s, "eval", inner, indent),
        ExprKind::Include { kind, path } => {
            let o = match kind {
                IncludeKind::Include => "include",
                IncludeKind::IncludeOnce => "include_once",
                IncludeKind::Require => "require",
                IncludeKind::RequireOnce => "require_once",
            };
            func_op(s, o, path, indent);
        }
        // The parser stores `yield key => value` as Yield(value, key).
        ExprKind::Yield(value, key) => {
            if prio > 70 {
                s.push('(');
            }
            s.push_str("yield ");
            if let Some(v) = value {
                if let Some(k) = key {
                    ex(s, k, 70, indent);
                    s.push_str(" => ");
                }
                ex(s, v, 70, indent);
            }
            if prio > 70 {
                s.push(')');
            }
        }
        ExprKind::YieldFrom(inner) => prefix(s, "yield from ", (85, 86), inner, prio, indent),
        ExprKind::Clone(inner) => {
            s.push_str("\\clone(");
            ex(s, inner, 20, indent);
            s.push(')');
        }
        ExprKind::Spread(inner) => {
            s.push_str("...");
            ex(s, inner, prio, indent);
        }
        ExprKind::Pipe { value, callable } => {
            binary(s, " |> ", (183, 183, 184), value, callable, prio, indent)
        }
        ExprKind::ConstantAccess(parts) => s.push_str(&join_name(parts)),
        ExprKind::ClassConstAccess {
            class, constant, ..
        } => {
            export_ns_name(s, class, indent);
            s.push_str("::");
            s.push_str(&lossy(constant));
        }
        ExprKind::DynamicClassConstAccess { class, constant } => {
            export_ns_name(s, class, indent);
            s.push_str("::");
            ex(s, constant, 0, indent);
        }
        ExprKind::ThrowExpr(inner) => {
            s.push_str("throw ");
            ex(s, inner, prio, indent);
        }
        ExprKind::Suppress(inner) => prefix(s, "@", (240, 241), inner, prio, indent),
        ExprKind::Identifier(n) => s.push_str(&lossy(n)),
        ExprKind::FirstClassCallable(target) => match target {
            CallableTarget::Function(name) => {
                export_ns_name(s, name, indent);
                s.push_str("(...)");
            }
            CallableTarget::Method {
                object,
                method,
                nullsafe,
            } => {
                ex(s, object, 0, indent);
                s.push_str(if *nullsafe { "?->" } else { "->" });
                export_var(s, method, indent);
                s.push_str("(...)");
            }
            CallableTarget::StaticMethod { class, method } => {
                export_ns_name(s, class, indent);
                s.push_str("::");
                s.push_str(&lossy(method));
                s.push_str("(...)");
            }
        },
    }
}

/// zend_ast_export_encaps_list.
fn export_encaps(s: &mut String, quote: u8, parts: &[StringPart], indent: usize) {
    for (i, p) in parts.iter().enumerate() {
        match p {
            StringPart::Literal(b) => export_qstr(s, quote, b),
            StringPart::Expr(e) => {
                let next_needs_braces = match parts.get(i + 1) {
                    Some(StringPart::Literal(b)) => {
                        b.first().is_some_and(|&c| c == b'[' || valid_var_char(c))
                    }
                    _ => false,
                };
                // A ZEND_AST_VAR whose name is a literal (`$a`, `${'x'}`).
                let simple_var = match &e.kind {
                    ExprKind::Variable(_) => true,
                    ExprKind::DynamicVariable(inner) => matches!(inner.kind, ExprKind::String(_)),
                    _ => false,
                };
                if simple_var && !next_needs_braces {
                    ex(s, e, 0, indent);
                } else {
                    s.push('{');
                    ex(s, e, 0, indent);
                    s.push('}');
                }
            }
        }
    }
}

fn export_params(s: &mut String, params: &[Param], indent: usize) {
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        export_attributes(s, &p.attributes, indent, false);
        if let Some(v) = p.visibility {
            s.push_str(visibility_str(v));
        }
        if let Some(v) = p.set_visibility {
            s.push_str(set_visibility_str(v));
        }
        if p.is_final {
            s.push_str("final ");
        }
        if let Some(t) = &p.type_hint {
            export_type(s, t);
            s.push(' ');
        }
        if p.by_ref {
            s.push('&');
        }
        if p.variadic {
            s.push_str("...");
        }
        s.push('$');
        s.push_str(&lossy(&p.name));
        if let Some(d) = &p.default {
            s.push_str(" = ");
            ex(s, d, 0, indent);
        }
        let hooks = Hooks {
            get: p.get_hook.as_deref(),
            set: p.set_hook.as_ref().map(|(_, b)| b.as_slice()),
            get_final: p.get_hook_final,
            set_final: p.set_hook_final,
            get_abstract: p.get_hook_abstract,
            set_abstract: p.set_hook_abstract,
            attrs: &p.hook_attrs,
        };
        export_hook_list(s, &hooks, indent);
    }
}

struct Hooks<'a> {
    get: Option<&'a [Statement]>,
    set: Option<&'a [Statement]>,
    get_final: bool,
    set_final: bool,
    get_abstract: bool,
    set_abstract: bool,
    attrs: &'a HookAttrs,
}

/// zend_ast_export_hook_list.
fn export_hook_list(s: &mut String, h: &Hooks, indent: usize) {
    if h.get.is_none() && h.set.is_none() {
        return;
    }
    s.push_str(" {\n");
    let indent = indent + 1;
    indent_to(s, indent);
    let mut first = true;
    let hooks = [
        ("get", h.get, h.get_final, h.get_abstract, h.attrs.get_short),
        ("set", h.set, h.set_final, h.set_abstract, h.attrs.set_short),
    ];
    for (name, body, is_final, is_abstract, short) in hooks {
        let Some(body) = body else { continue };
        if !first {
            s.push('\n');
            indent_to(s, indent);
        }
        first = false;
        if is_final {
            s.push_str("final ");
        }
        s.push_str(name);
        if is_abstract {
            s.push(';');
            continue;
        }
        // `get => expr;` / `set => expr;` (the parser expands the bodies).
        let short_expr = if short {
            match body {
                [
                    Statement {
                        kind: StmtKind::Return(Some(e)),
                        ..
                    },
                ] if name == "get" => Some(e),
                [
                    Statement {
                        kind:
                            StmtKind::Expression(Expr {
                                kind: ExprKind::Assign { value, .. },
                                ..
                            }),
                        ..
                    },
                ] if name == "set" => Some(value.as_ref()),
                _ => None,
            }
        } else {
            None
        };
        match short_expr {
            Some(e) => {
                s.push_str(" => ");
                ex(s, e, 0, indent);
                s.push(';');
            }
            None => {
                s.push_str(" {\n");
                export_stmts(s, body, indent + 1);
                indent_to(s, indent);
                s.push('}');
            }
        }
    }
    s.push('\n');
    indent_to(s, indent - 1);
    s.push('}');
}

/// zend_ast_export_class_no_header.
fn export_class_no_header(
    s: &mut String,
    extends: Option<&[u8]>,
    implements: &[Vec<u8>],
    body: &[ClassMember],
    indent: usize,
) {
    if let Some(e) = extends {
        s.push_str(" extends ");
        s.push_str(&lossy(e));
    }
    if !implements.is_empty() {
        s.push_str(" implements ");
        s.push_str(
            &implements
                .iter()
                .map(|n| lossy(n).into_owned())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    s.push_str(" {\n");
    for m in body {
        // `public $a = 1, $b;` / `const X = 1, Y = 2;` stay one declaration.
        match m {
            ClassMember::ClassConstant {
                name,
                value,
                group_continues: true,
                ..
            } if s.ends_with(";\n") => {
                s.truncate(s.len() - 2);
                s.push_str(", ");
                s.push_str(&lossy(name));
                s.push_str(" = ");
                ex(s, value, 0, indent + 1);
                s.push_str(";\n");
            }
            ClassMember::Property {
                name,
                default,
                group_continues: true,
                ..
            } if s.ends_with(";\n") => {
                s.truncate(s.len() - 2);
                s.push_str(", $");
                s.push_str(&lossy(name));
                if let Some(d) = default {
                    s.push_str(" = ");
                    ex(s, d, 0, indent + 2);
                }
                s.push_str(";\n");
            }
            _ => export_member(s, m, indent + 1),
        }
    }
    indent_to(s, indent);
    s.push('}');
}

/// A class member as zend_ast_export_stmt prints it (with its trailing
/// `;` and newline).
fn export_member(s: &mut String, m: &ClassMember, indent: usize) {
    indent_to(s, indent);
    match m {
        ClassMember::Property {
            name,
            type_hint,
            default,
            visibility,
            set_visibility,
            is_static,
            is_readonly,
            get_hook,
            set_hook,
            get_hook_final,
            set_hook_final,
            get_hook_abstract,
            set_hook_abstract,
            attributes,
            hook_attrs,
            ..
        } => {
            export_attributes(s, attributes, indent, true);
            s.push_str(visibility_str(*visibility));
            if let Some(v) = set_visibility {
                s.push_str(set_visibility_str(*v));
            }
            if *is_static {
                s.push_str("static ");
            }
            if *is_readonly {
                s.push_str("readonly ");
            }
            if let Some(t) = type_hint {
                export_type(s, t);
                s.push(' ');
            }
            s.push('$');
            s.push_str(&lossy(name));
            if let Some(d) = default {
                s.push_str(" = ");
                ex(s, d, 0, indent + 1);
            }
            let hooks = Hooks {
                get: get_hook.as_deref(),
                set: set_hook.as_ref().map(|(_, b)| b.as_slice()),
                get_final: *get_hook_final,
                set_final: *set_hook_final,
                get_abstract: *get_hook_abstract,
                set_abstract: *set_hook_abstract,
                attrs: hook_attrs,
            };
            export_hook_list(s, &hooks, indent);
            if get_hook.is_none() && set_hook.is_none() {
                s.push(';');
            }
        }
        ClassMember::Method {
            name,
            params,
            return_type,
            body,
            visibility,
            is_static,
            is_abstract,
            is_final,
            attributes,
            returns_by_ref,
            ..
        } => {
            export_function(
                s,
                &FnDecl {
                    name: Some(name),
                    params,
                    return_type: return_type.as_ref(),
                    body: body.as_deref(),
                    visibility: Some(*visibility),
                    is_static: *is_static,
                    is_abstract: *is_abstract,
                    is_final: *is_final,
                    attributes,
                    returns_by_ref: *returns_by_ref,
                },
                indent,
            );
        }
        ClassMember::ClassConstant {
            name,
            value,
            visibility,
            attributes,
            type_hint,
            ..
        } => {
            export_attributes(s, attributes, indent, true);
            s.push_str(visibility_str(*visibility));
            s.push_str("const ");
            if let Some(t) = type_hint {
                export_type(s, t);
                s.push(' ');
            }
            s.push_str(&lossy(name));
            s.push_str(" = ");
            ex(s, value, 0, indent);
            s.push(';');
        }
        ClassMember::TraitUse {
            traits,
            adaptations,
            ..
        } => {
            s.push_str("use ");
            s.push_str(
                &traits
                    .iter()
                    .map(|n| lossy(n).into_owned())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if adaptations.is_empty() {
                s.push(';');
            } else {
                s.push_str(" {\n");
                for a in adaptations {
                    indent_to(s, indent + 1);
                    match a {
                        TraitAdaptation::Precedence {
                            trait_name,
                            method,
                            instead_of,
                        } => {
                            s.push_str(&lossy(trait_name));
                            s.push_str("::");
                            s.push_str(&lossy(method));
                            s.push_str(" insteadof ");
                            s.push_str(
                                &instead_of
                                    .iter()
                                    .map(|n| lossy(n).into_owned())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            );
                        }
                        TraitAdaptation::Alias {
                            trait_name,
                            method,
                            new_name,
                            new_visibility,
                            new_final,
                        } => {
                            if let Some(t) = trait_name {
                                s.push_str(&lossy(t));
                                s.push_str("::");
                            }
                            s.push_str(&lossy(method));
                            match new_visibility {
                                Some(Visibility::Public) => s.push_str(" as public"),
                                Some(Visibility::Protected) => s.push_str(" as protected"),
                                Some(Visibility::Private) => s.push_str(" as private"),
                                None if new_name.is_some() || *new_final => s.push_str(" as"),
                                None => {}
                            }
                            if *new_final {
                                s.push_str(" final");
                            }
                            if let Some(n) = new_name {
                                s.push(' ');
                                s.push_str(&lossy(n));
                            }
                        }
                    }
                    s.push_str(";\n");
                }
                indent_to(s, indent);
                s.push('}');
            }
        }
        ClassMember::EnumCase {
            name,
            value,
            attributes,
            ..
        } => {
            export_attributes(s, attributes, indent, true);
            s.push_str("case ");
            s.push_str(&lossy(name));
            if let Some(v) = value {
                s.push_str(" = ");
                ex(s, v, 0, indent);
            }
            s.push(';');
        }
    }
    s.push('\n');
}

struct FnDecl<'a> {
    name: Option<&'a [u8]>,
    params: &'a [Param],
    return_type: Option<&'a TypeHint>,
    body: Option<&'a [Statement]>,
    visibility: Option<Visibility>,
    is_static: bool,
    is_abstract: bool,
    is_final: bool,
    attributes: &'a [Attribute],
    returns_by_ref: bool,
}

/// ZEND_AST_FUNC_DECL / ZEND_AST_METHOD.
fn export_function(s: &mut String, f: &FnDecl, indent: usize) {
    export_attributes(s, f.attributes, indent, true);
    if let Some(v) = f.visibility {
        s.push_str(visibility_str(v));
    }
    if f.is_static {
        s.push_str("static ");
    }
    if f.is_abstract {
        s.push_str("abstract ");
    }
    if f.is_final {
        s.push_str("final ");
    }
    s.push_str("function ");
    if f.returns_by_ref {
        s.push('&');
    }
    if let Some(n) = f.name {
        s.push_str(&lossy(n));
    }
    s.push('(');
    export_params(s, f.params, indent);
    s.push(')');
    if let Some(t) = f.return_type {
        s.push_str(": ");
        export_type(s, t);
    }
    match f.body {
        Some(body) => {
            s.push_str(" {\n");
            export_stmts(s, body, indent + 1);
            indent_to(s, indent);
            s.push_str("}\n");
        }
        None => s.push_str(";\n"),
    }
}

fn export_stmts(s: &mut String, stmts: &[Statement], indent: usize) {
    for st in stmts {
        export_stmt(s, st, indent);
    }
}

/// zend_ast_export_stmt for one statement.
fn export_stmt(s: &mut String, st: &Statement, indent: usize) {
    // Statements the parser keeps as several PHP statements.
    match &st.kind {
        StmtKind::Nop => return,
        StmtKind::ClassDecl { name, .. } if name.starts_with(b"__anonymous_class_") => return,
        StmtKind::Echo(items) if items.len() > 1 => {
            for e in items {
                indent_to(s, indent);
                s.push_str("echo ");
                ex(s, e, 0, indent);
                s.push_str(";\n");
            }
            return;
        }
        StmtKind::Global(names) if names.len() > 1 => {
            for n in names {
                indent_to(s, indent);
                s.push_str("global $");
                s.push_str(&lossy(n));
                s.push_str(";\n");
            }
            return;
        }
        // `global $a, $$b;` is a statement list of ZEND_AST_GLOBALs.
        StmtKind::GlobalDynamic(items) if items.len() > 1 => {
            for e in items {
                indent_to(s, indent);
                s.push_str("global ");
                ex(s, e, 0, indent);
                s.push_str(";\n");
            }
            return;
        }
        StmtKind::StaticVar(vars) if vars.len() > 1 => {
            for (n, d) in vars {
                indent_to(s, indent);
                s.push_str("static $");
                s.push_str(&lossy(n));
                if let Some(d) = d {
                    s.push_str(" = ");
                    ex(s, d, 0, indent);
                }
                s.push_str(";\n");
            }
            return;
        }
        StmtKind::Unset(items) if items.len() > 1 => {
            for e in items {
                indent_to(s, indent);
                func_op(s, "unset", e, indent);
                s.push_str(";\n");
            }
            return;
        }
        _ => {}
    }
    indent_to(s, indent);
    let semicolon = match &st.kind {
        StmtKind::InlineHtml(b) => {
            s.push_str("echo '");
            export_str(s, b);
            s.push('\'');
            true
        }
        StmtKind::Expression(e) => {
            ex(s, e, 0, indent);
            true
        }
        StmtKind::Echo(items) => {
            s.push_str("echo ");
            export_list(s, items, 0, indent);
            true
        }
        StmtKind::Return(e) => {
            s.push_str("return");
            if let Some(e) = e {
                s.push(' ');
                ex(s, e, 0, indent);
            }
            true
        }
        StmtKind::If {
            condition,
            body,
            elseif_clauses,
            else_body,
        } => {
            export_if(
                s,
                condition,
                body,
                elseif_clauses,
                else_body.as_deref(),
                indent,
            );
            false
        }
        StmtKind::While { condition, body } => {
            s.push_str("while (");
            ex(s, condition, 0, indent);
            s.push_str(") {\n");
            export_stmts(s, body, indent + 1);
            indent_to(s, indent);
            s.push('}');
            false
        }
        StmtKind::DoWhile { body, condition } => {
            s.push_str("do {\n");
            export_stmts(s, body, indent + 1);
            indent_to(s, indent);
            s.push_str("} while (");
            ex(s, condition, 0, indent);
            s.push(')');
            true
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            s.push_str("for (");
            export_list(s, init, 0, indent);
            s.push(';');
            if !condition.is_empty() {
                s.push(' ');
                export_list(s, condition, 0, indent);
            }
            s.push(';');
            if !update.is_empty() {
                s.push(' ');
                export_list(s, update, 0, indent);
            }
            s.push_str(") {\n");
            export_stmts(s, body, indent + 1);
            indent_to(s, indent);
            s.push('}');
            false
        }
        StmtKind::Foreach {
            expr,
            key,
            value,
            by_ref,
            body,
        } => {
            s.push_str("foreach (");
            ex(s, expr, 0, indent);
            s.push_str(" as ");
            if let Some(k) = key {
                ex(s, k, 0, indent);
                s.push_str(" => ");
            }
            if *by_ref {
                s.push('&');
            }
            ex(s, value, 0, indent);
            s.push_str(") {\n");
            export_stmts(s, body, indent + 1);
            indent_to(s, indent);
            s.push('}');
            false
        }
        StmtKind::Switch { expr, cases } => {
            s.push_str("switch (");
            ex(s, expr, 0, indent);
            s.push_str(") {\n");
            for c in cases {
                indent_to(s, indent + 1);
                match &c.value {
                    Some(v) => {
                        s.push_str("case ");
                        ex(s, v, 0, indent + 1);
                        s.push_str(":\n");
                    }
                    None => s.push_str("default:\n"),
                }
                export_stmts(s, &c.body, indent + 2);
            }
            indent_to(s, indent);
            s.push('}');
            false
        }
        StmtKind::Break(n) | StmtKind::Continue(n) => {
            s.push_str(if matches!(st.kind, StmtKind::Break(_)) {
                "break"
            } else {
                "continue"
            });
            if let Some(n) = n {
                s.push(' ');
                ex(s, n, 0, indent);
            }
            true
        }
        StmtKind::FunctionDecl {
            name,
            params,
            return_type,
            body,
            attributes,
            returns_by_ref,
            ..
        } => {
            export_function(
                s,
                &FnDecl {
                    name: Some(name),
                    params,
                    return_type: return_type.as_ref(),
                    body: Some(body),
                    visibility: None,
                    is_static: false,
                    is_abstract: false,
                    is_final: false,
                    attributes,
                    returns_by_ref: *returns_by_ref,
                },
                indent,
            );
            false
        }
        StmtKind::ClassDecl {
            name,
            modifiers,
            extends,
            implements,
            body,
            enum_backing_type,
            attributes,
            ..
        } => {
            export_attributes(s, attributes, indent, true);
            if modifiers.is_interface {
                s.push_str("interface ");
            } else if modifiers.is_trait {
                s.push_str("trait ");
            } else if modifiers.is_enum {
                s.push_str("enum ");
            } else {
                if modifiers.is_abstract {
                    s.push_str("abstract ");
                }
                if modifiers.is_final {
                    s.push_str("final ");
                }
                if modifiers.is_readonly {
                    s.push_str("readonly ");
                }
                s.push_str("class ");
            }
            s.push_str(&lossy(name));
            if let (true, Some(t)) = (modifiers.is_enum, enum_backing_type) {
                s.push_str(": ");
                s.push_str(&lossy(t));
            }
            if modifiers.is_interface {
                // An interface's parents are its ZEND_AST_CLASS implements list.
                let parents: Vec<Vec<u8>> = extends.iter().chain(implements).cloned().collect();
                export_class_no_header(s, None, &parents, body, indent);
            } else {
                export_class_no_header(s, extends.as_deref(), implements, body, indent);
            }
            s.push('\n');
            false
        }
        StmtKind::TryCatch {
            try_body,
            catches,
            finally_body,
        } => {
            s.push_str("try {\n");
            export_stmts(s, try_body, indent + 1);
            indent_to(s, indent);
            for c in catches {
                s.push_str("} catch (");
                s.push_str(
                    &c.types
                        .iter()
                        .map(|t| join_name(t))
                        .collect::<Vec<_>>()
                        .join("|"),
                );
                if let Some(v) = &c.variable {
                    s.push_str(" $");
                    s.push_str(&lossy(v));
                }
                s.push_str(") {\n");
                export_stmts(s, &c.body, indent + 1);
                indent_to(s, indent);
            }
            if let Some(f) = finally_body {
                s.push_str("} finally {\n");
                export_stmts(s, f, indent + 1);
                indent_to(s, indent);
            }
            s.push('}');
            false
        }
        StmtKind::Throw(e) => {
            s.push_str("throw ");
            ex(s, e, 0, indent);
            true
        }
        StmtKind::Global(names) => {
            s.push_str("global $");
            if let Some(n) = names.first() {
                s.push_str(&lossy(n));
            }
            true
        }
        StmtKind::GlobalDynamic(items) => {
            s.push_str("global ");
            export_list(s, items, 0, indent);
            true
        }
        StmtKind::StaticVar(vars) => {
            if let Some((n, d)) = vars.first() {
                s.push_str("static $");
                s.push_str(&lossy(n));
                if let Some(d) = d {
                    s.push_str(" = ");
                    ex(s, d, 0, indent);
                }
            }
            true
        }
        StmtKind::Unset(items) => {
            if let Some(e) = items.first() {
                func_op(s, "unset", e, indent);
            }
            true
        }
        // `const X = v;` is a Declare without a body naming no declare()
        // directive.
        StmtKind::Declare {
            directives,
            body: None,
            attributes,
            is_const: true,
        } => {
            export_attributes(s, attributes, indent, true);
            s.push_str("const ");
            for (i, (n, v)) in directives.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&lossy(n));
                s.push_str(" = ");
                ex(s, v, 0, indent);
            }
            true
        }
        StmtKind::Declare {
            directives, body, ..
        } => {
            s.push_str("declare(");
            for (i, (n, v)) in directives.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&lossy(n));
                s.push_str(" = ");
                ex(s, v, 0, indent);
            }
            s.push(')');
            match body {
                Some(b) => {
                    s.push_str(" {\n");
                    export_stmts(s, b, indent + 1);
                    indent_to(s, indent);
                    s.push('}');
                }
                None => s.push(';'),
            }
            false
        }
        StmtKind::NamespaceDecl { name, body } => {
            s.push_str("namespace");
            if let Some(n) = name {
                s.push(' ');
                s.push_str(&join_name(n));
            }
            match body {
                Some(b) => {
                    s.push_str(" {\n");
                    export_stmts(s, b, indent + 1);
                    indent_to(s, indent);
                    s.push_str("}\n");
                }
                None => s.push(';'),
            }
            false
        }
        StmtKind::UseDecl(items) => {
            s.push_str("use ");
            match items.first().map(|i| i.kind) {
                Some(UseKind::Function) => s.push_str("function "),
                Some(UseKind::Constant) => s.push_str("const "),
                _ => {}
            }
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&join_name(&it.name));
                if let Some(a) = &it.alias {
                    s.push_str(" as ");
                    s.push_str(&lossy(a));
                }
            }
            true
        }
        StmtKind::Label(n) => {
            s.push_str(&lossy(n));
            s.push(':');
            false
        }
        StmtKind::Goto(n) => {
            s.push_str("goto ");
            s.push_str(&lossy(n));
            true
        }
        StmtKind::Nop => false,
    };
    if semicolon {
        s.push(';');
    }
    s.push('\n');
}

/// zend_ast_export_if_stmt.
fn export_if(
    s: &mut String,
    condition: &Expr,
    body: &[Statement],
    elseifs: &[(Expr, Vec<Statement>)],
    else_body: Option<&[Statement]>,
    indent: usize,
) {
    s.push_str("if (");
    ex(s, condition, 0, indent);
    s.push_str(") {\n");
    export_stmts(s, body, indent + 1);
    for (c, b) in elseifs {
        indent_to(s, indent);
        s.push_str("} elseif (");
        ex(s, c, 0, indent);
        s.push_str(") {\n");
        export_stmts(s, b, indent + 1);
    }
    if let Some(eb) = else_body {
        indent_to(s, indent);
        s.push_str("} else ");
        // `else if` continues the chain.
        if let [
            Statement {
                kind:
                    StmtKind::If {
                        condition,
                        body,
                        elseif_clauses,
                        else_body,
                    },
                ..
            },
        ] = eb
        {
            export_if(
                s,
                condition,
                body,
                elseif_clauses,
                else_body.as_deref(),
                indent,
            );
            return;
        }
        s.push_str("{\n");
        export_stmts(s, eb, indent + 1);
    }
    indent_to(s, indent);
    s.push('}');
}
