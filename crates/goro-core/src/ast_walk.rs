//! Generic AST searches (zend_ast_apply() over the AST children), stopping
//! at function and class boundaries like zend_ast_is_special() does:
//! closures, arrow functions, function and class declarations are not
//! entered.

use goro_parser::ast::{CallableTarget, Expr, ExprKind, Statement, StmtKind, StringPart};

/// Whether `pred` holds for `expr` or any expression nested in it.
pub fn expr_any(expr: &Expr, pred: &mut dyn FnMut(&Expr) -> bool) -> bool {
    if pred(expr) {
        return true;
    }
    expr_any_child(expr, &mut |c| expr_any(c, pred))
}

/// Whether `pred` holds for any expression of `stmts` (recursively).
pub fn stmts_any(stmts: &[Statement], pred: &mut dyn FnMut(&Expr) -> bool) -> bool {
    stmts.iter().any(|s| {
        stmt_any_child(s, &mut |c| match c {
            Child::Expr(e) => expr_any(e, pred),
            Child::Block(b) => stmts_any(b, pred),
        })
    })
}

fn opt(e: &Option<Box<Expr>>, f: &mut dyn FnMut(&Expr) -> bool) -> bool {
    e.as_deref().is_some_and(f)
}

/// Calls `f` on the direct child expressions of `expr` (not entering
/// closures and arrow functions); true as soon as one call returns true.
pub fn expr_any_child(expr: &Expr, f: &mut dyn FnMut(&Expr) -> bool) -> bool {
    use ExprKind as E;
    match &expr.kind {
        E::Int(_)
        | E::Float(_)
        | E::String(_)
        | E::True
        | E::False
        | E::Null
        | E::Variable(_)
        | E::ConstantAccess(_)
        | E::Identifier(_)
        | E::Closure { .. }
        | E::ArrowFunction { .. } => false,
        E::InterpolatedString(parts) => parts.iter().any(|p| match p {
            StringPart::Expr(e) => f(e),
            StringPart::Literal(_) => false,
        }),
        E::Array(elems) => elems
            .iter()
            .any(|el| el.key.as_ref().is_some_and(&mut *f) || f(&el.value)),
        E::DynamicVariable(e)
        | E::Cast(_, e)
        | E::Print(e)
        | E::Empty(e)
        | E::Eval(e)
        | E::YieldFrom(e)
        | E::Clone(e)
        | E::Spread(e)
        | E::ThrowExpr(e)
        | E::Suppress(e)
        | E::ShellExec(e) => f(e),
        E::UnaryOp { operand, .. } => f(operand),
        E::BinaryOp { left, right, .. } | E::NullCoalesce { left, right } => f(left) || f(right),
        E::Assign { target, value }
        | E::CompoundAssign { target, value, .. }
        | E::AssignRef { target, value } => f(target) || f(value),
        E::PropertyAccess {
            object, property, ..
        } => f(object) || f(property),
        E::StaticPropertyAccess { class, .. } => f(class),
        E::DynamicStaticPropertyAccess { class, property } => f(class) || f(property),
        E::MethodCall {
            object,
            method,
            args,
            ..
        } => f(object) || f(method) || args.iter().any(|a| f(&a.value)),
        E::StaticMethodCall { class, args, .. } => f(class) || args.iter().any(|a| f(&a.value)),
        E::DynamicStaticMethodCall {
            class,
            method,
            args,
        } => f(class) || f(method) || args.iter().any(|a| f(&a.value)),
        E::ArrayAccess { array, index } => f(array) || opt(index, f),
        E::FunctionCall { name, args } => f(name) || args.iter().any(|a| f(&a.value)),
        E::New { class, args } => f(class) || args.iter().any(|a| f(&a.value)),
        E::Ternary {
            condition,
            if_true,
            if_false,
        } => f(condition) || opt(if_true, f) || f(if_false),
        E::Match { subject, arms } => {
            f(subject)
                || arms.iter().any(|arm| {
                    arm.conditions
                        .as_ref()
                        .is_some_and(|cs| cs.iter().any(&mut *f))
                        || f(&arm.body)
                })
        }
        E::Instanceof { expr, class } => f(expr) || f(class),
        E::Exit(e) => opt(e, f),
        E::Isset(es) => es.iter().any(&mut *f),
        E::Include { path, .. } => f(path),
        E::Yield(k, v) => opt(k, f) || opt(v, f),
        E::Pipe { value, callable } => f(value) || f(callable),
        E::ClassConstAccess { class, .. } => f(class),
        E::DynamicClassConstAccess { class, constant } => f(class) || f(constant),
        E::FirstClassCallable(t) => match t {
            CallableTarget::Function(e) => f(e),
            CallableTarget::Method { object, method, .. } => f(object) || f(method),
            CallableTarget::StaticMethod { class, .. } => f(class),
        },
    }
}

/// A direct child of a statement.
pub enum Child<'a> {
    Expr(&'a Expr),
    Block(&'a [Statement]),
}

/// Calls `f` on the direct expressions and nested statement blocks of
/// `stmt` (function and class declarations are not entered).
pub fn stmt_any_child(stmt: &Statement, f: &mut dyn FnMut(Child<'_>) -> bool) -> bool {
    use StmtKind as S;
    match &stmt.kind {
        S::Expression(e) | S::Throw(e) => f(Child::Expr(e)),
        S::Echo(es) | S::GlobalDynamic(es) | S::Unset(es) => es.iter().any(|e| f(Child::Expr(e))),
        S::Return(e) | S::Break(e) | S::Continue(e) => {
            e.as_ref().is_some_and(|e| f(Child::Expr(e)))
        }
        S::If {
            condition,
            body,
            elseif_clauses,
            else_body,
        } => {
            f(Child::Expr(condition))
                || f(Child::Block(body))
                || elseif_clauses
                    .iter()
                    .any(|(c, b)| f(Child::Expr(c)) || f(Child::Block(b)))
                || else_body.as_deref().is_some_and(|b| f(Child::Block(b)))
        }
        S::While { condition, body } | S::DoWhile { body, condition } => {
            f(Child::Expr(condition)) || f(Child::Block(body))
        }
        S::For {
            init,
            condition,
            update,
            body,
        } => {
            init.iter().any(|e| f(Child::Expr(e)))
                || condition.iter().any(|e| f(Child::Expr(e)))
                || update.iter().any(|e| f(Child::Expr(e)))
                || f(Child::Block(body))
        }
        S::Foreach {
            expr,
            key,
            value,
            body,
            ..
        } => {
            f(Child::Expr(expr))
                || key.as_ref().is_some_and(|k| f(Child::Expr(k)))
                || f(Child::Expr(value))
                || f(Child::Block(body))
        }
        S::Switch { expr, cases } => {
            f(Child::Expr(expr))
                || cases.iter().any(|c| {
                    c.value.as_ref().is_some_and(|v| f(Child::Expr(v))) || f(Child::Block(&c.body))
                })
        }
        S::TryCatch {
            try_body,
            catches,
            finally_body,
        } => {
            f(Child::Block(try_body))
                || catches.iter().any(|c| f(Child::Block(&c.body)))
                || finally_body.as_deref().is_some_and(|b| f(Child::Block(b)))
        }
        S::StaticVar(vars) => vars
            .iter()
            .any(|(_, e)| e.as_ref().is_some_and(|e| f(Child::Expr(e)))),
        S::Declare { body, .. } | S::NamespaceDecl { body, .. } => {
            body.as_deref().is_some_and(|b| f(Child::Block(b)))
        }
        S::InlineHtml(_)
        | S::FunctionDecl { .. }
        | S::ClassDecl { .. }
        | S::Global(_)
        | S::UseDecl(_)
        | S::Label(_)
        | S::Goto(_)
        | S::Nop => false,
    }
}
