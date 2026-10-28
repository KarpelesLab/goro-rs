//! Expressions: a precedence-climbing parser over the operator table of
//! the PHP manual ("Operator Precedence"), plus PHP 8.5's pipe operator
//! (RFC "Pipe operator v3": below `.`, above comparisons; observed on
//! PHP 8.5).

use std::borrow::Cow;

use super::strings::{self, Number, Quote};
use super::{PResult, Parser};
use crate::ast::*;
use crate::token::{T, TF_BINARY_PREFIX, TF_DOUBLE, TF_TAG, Token};

// Binding powers (higher binds tighter).
const BP_OR: u8 = 2;
const BP_XOR: u8 = 4;
const BP_AND: u8 = 6;
/// Assignment, and the operand level of `print`, `yield`, `include`.
pub(crate) const BP_ASSIGN: u8 = 8;
const BP_TERNARY: u8 = 10;
const BP_COALESCE: u8 = 12;
const BP_BOOL_OR: u8 = 14;
const BP_BOOL_AND: u8 = 16;
const BP_BIT_OR: u8 = 18;
const BP_BIT_XOR: u8 = 20;
const BP_BIT_AND: u8 = 22;
const BP_EQUALITY: u8 = 24;
const BP_COMPARE: u8 = 26;
const BP_PIPE: u8 = 27;
const BP_CONCAT: u8 = 28;
const BP_SHIFT: u8 = 30;
const BP_ADD: u8 = 32;
const BP_MUL: u8 = 34;
const BP_NOT: u8 = 36;
const BP_INSTANCEOF: u8 = 38;
const BP_UNARY: u8 = 40;
const BP_POW: u8 = 42;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Assoc {
    Left,
    Right,
    Non,
}

fn binary_op(k: T) -> Option<(u8, Assoc, BinaryOp)> {
    use BinaryOp::*;
    Some(match k {
        T::LogicalOr => (BP_OR, Assoc::Left, BoolOr),
        T::LogicalXor => (BP_XOR, Assoc::Left, LogicalXor),
        T::LogicalAnd => (BP_AND, Assoc::Left, BoolAnd),
        T::Coalesce => (BP_COALESCE, Assoc::Right, Coalesce),
        T::BooleanOr => (BP_BOOL_OR, Assoc::Left, BoolOr),
        T::BooleanAnd => (BP_BOOL_AND, Assoc::Left, BoolAnd),
        T::Pipe => (BP_BIT_OR, Assoc::Left, BitOr),
        T::Caret => (BP_BIT_XOR, Assoc::Left, BitXor),
        T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg => {
            (BP_BIT_AND, Assoc::Left, BitAnd)
        }
        T::IsEqual => (BP_EQUALITY, Assoc::Non, Equal),
        T::IsNotEqual => (BP_EQUALITY, Assoc::Non, NotEqual),
        T::IsIdentical => (BP_EQUALITY, Assoc::Non, Identical),
        T::IsNotIdentical => (BP_EQUALITY, Assoc::Non, NotIdentical),
        T::Spaceship => (BP_EQUALITY, Assoc::Non, Spaceship),
        T::Lt => (BP_COMPARE, Assoc::Non, Less),
        T::IsSmallerOrEqual => (BP_COMPARE, Assoc::Non, LessEqual),
        T::Gt => (BP_COMPARE, Assoc::Non, Greater),
        T::IsGreaterOrEqual => (BP_COMPARE, Assoc::Non, GreaterEqual),
        T::PipeGt => (BP_PIPE, Assoc::Left, Pipe),
        T::Dot => (BP_CONCAT, Assoc::Left, Concat),
        T::Sl => (BP_SHIFT, Assoc::Left, ShiftLeft),
        T::Sr => (BP_SHIFT, Assoc::Left, ShiftRight),
        T::Plus => (BP_ADD, Assoc::Left, Add),
        T::Minus => (BP_ADD, Assoc::Left, Sub),
        T::Star => (BP_MUL, Assoc::Left, Mul),
        T::Slash => (BP_MUL, Assoc::Left, Div),
        T::Percent => (BP_MUL, Assoc::Left, Mod),
        T::Pow => (BP_POW, Assoc::Right, Pow),
        _ => return None,
    })
}

fn assign_op(k: T) -> Option<AssignOp> {
    use AssignOp::*;
    Some(match k {
        T::PlusEqual => Add,
        T::MinusEqual => Sub,
        T::MulEqual => Mul,
        T::DivEqual => Div,
        T::ModEqual => Mod,
        T::PowEqual => Pow,
        T::ConcatEqual => Concat,
        T::AndEqual => BitAnd,
        T::OrEqual => BitOr,
        T::XorEqual => BitXor,
        T::SlEqual => ShiftLeft,
        T::SrEqual => ShiftRight,
        T::CoalesceEqual => Coalesce,
        _ => return None,
    })
}

/// A variable-like expression: can be written to, incremented, passed by
/// reference (PHP grammar "variable": includes calls, which the compiler
/// rejects in write context).
pub(crate) fn is_variable(e: &Expr<'_>) -> bool {
    matches!(
        e.kind,
        ExprKind::Variable(_)
            | ExprKind::VariableVariable(_)
            | ExprKind::Index { .. }
            | ExprKind::Prop { .. }
            | ExprKind::StaticProp { .. }
            | ExprKind::Call { .. }
            | ExprKind::CallExpr { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::StaticCall { .. }
    )
}

/// Whether a token can start an expression.
pub(crate) fn starts_expr(k: T) -> bool {
    matches!(
        k,
        T::Variable
            | T::Dollar
            | T::LParen
            | T::LBracket
            | T::Array
            | T::List
            | T::Isset
            | T::Empty
            | T::Eval
            | T::Include
            | T::IncludeOnce
            | T::Require
            | T::RequireOnce
            | T::Exit
            | T::Print
            | T::Yield
            | T::YieldFrom
            | T::Throw
            | T::Clone
            | T::New
            | T::Function
            | T::Fn
            | T::Static
            | T::Match
            | T::Attribute
            | T::IntCast
            | T::DoubleCast
            | T::StringCast
            | T::ArrayCast
            | T::ObjectCast
            | T::BoolCast
            | T::UnsetCast
            | T::VoidCast
            | T::Bang
            | T::Tilde
            | T::Minus
            | T::Plus
            | T::At
            | T::Inc
            | T::Dec
            | T::LNumber
            | T::DNumber
            | T::ConstantEncapsedString
            | T::DoubleQuote
            | T::StartHeredoc
            | T::Backtick
            | T::Line
            | T::File
            | T::Dir
            | T::ClassC
            | T::TraitC
            | T::MethodC
            | T::FuncC
            | T::PropertyC
            | T::NsC
            | T::String
            | T::NameQualified
            | T::NameFullyQualified
            | T::NameRelative
            | T::Readonly
    )
}

impl<'a> Parser<'a> {
    pub(crate) fn expr(&mut self) -> PResult<Expr<'a>> {
        self.expr_bp(0)
    }

    pub(crate) fn expr_bp(&mut self, min: u8) -> PResult<Expr<'a>> {
        Ok(self.expr_bp_p(min)?.0)
    }

    /// Parses an expression; the flag tells whether it is a bare `(...)`.
    pub(crate) fn expr_bp_p(&mut self, min: u8) -> PResult<(Expr<'a>, bool)> {
        self.enter()?;
        let start = self.pos;
        let (mut lhs, mut paren) = self.prefix()?;
        loop {
            let k = self.peek_kind();
            if k == T::Eq {
                if paren
                    || !(is_variable(&lhs)
                        || matches!(
                            lhs.kind,
                            ExprKind::Array { short: true, .. } | ExprKind::List { .. }
                        ))
                {
                    break;
                }
                self.bump();
                if matches!(
                    self.peek_kind(),
                    T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg
                ) {
                    if !is_variable(&lhs) {
                        // `[$a] = &$b` is not valid PHP syntax.
                        return Err(self.unexpected(&[]));
                    }
                    self.bump();
                    // `$a = &$b`: the right side is a variable or `new`
                    // (observed on PHP 8.5: `$a = &$b + 1` is
                    // `($a = &$b) + 1`).
                    let value = if self.at(T::New) {
                        let v = self.new_expr()?;
                        if matches!(v.kind, ExprKind::New { args: None, .. }) {
                            return Err(self.unexpected(&[T::LParen]));
                        }
                        v
                    } else {
                        if !matches!(
                            self.peek_kind(),
                            T::Variable
                                | T::Dollar
                                | T::String
                                | T::NameQualified
                                | T::NameFullyQualified
                                | T::NameRelative
                                | T::Static
                                | T::LParen
                        ) {
                            return Err(self.unexpected(&[]));
                        }
                        let v = self.expr_bp(BP_POW + 1)?;
                        if !is_variable(&v) {
                            return Err(self.unexpected(&[]));
                        }
                        v
                    };
                    let span = self.span_from(start);
                    lhs = Expr {
                        kind: ExprKind::AssignRef {
                            target: Box::new(lhs),
                            value: Box::new(value),
                        },
                        span,
                    };
                } else {
                    let target = self.to_assign_target(lhs);
                    let value = self.expr_bp(BP_ASSIGN)?;
                    let span = self.span_from(start);
                    lhs = Expr {
                        kind: ExprKind::Assign {
                            target: Box::new(target),
                            value: Box::new(value),
                        },
                        span,
                    };
                }
                paren = false;
                continue;
            }
            if let Some(op) = assign_op(k) {
                if paren || !is_variable(&lhs) {
                    break;
                }
                self.bump();
                let value = self.expr_bp(BP_ASSIGN)?;
                let span = self.span_from(start);
                lhs = Expr {
                    kind: ExprKind::AssignOp {
                        op,
                        target: Box::new(lhs),
                        value: Box::new(value),
                    },
                    span,
                };
                paren = false;
                continue;
            }
            if k == T::Inc || k == T::Dec {
                if paren || !is_variable(&lhs) {
                    break;
                }
                self.bump();
                let span = self.span_from(start);
                lhs = Expr {
                    kind: ExprKind::IncDec {
                        op: if k == T::Inc {
                            IncDec::PostInc
                        } else {
                            IncDec::PostDec
                        },
                        expr: Box::new(lhs),
                    },
                    span,
                };
                paren = false;
                continue;
            }
            if k == T::Question {
                if BP_TERNARY < min {
                    break;
                }
                self.bump();
                if let ExprKind::Ternary { then, .. } = &lhs.kind
                    && !paren
                {
                    let inner_short = then.is_none();
                    let outer_short = self.at(T::Colon);
                    let msg = match (inner_short, outer_short) {
                        (true, true) => None,
                        (false, false) => Some(
                            "Unparenthesized `a ? b : c ? d : e` is not supported. Use either `(a ? b : c) ? d : e` or `a ? b : (c ? d : e)`",
                        ),
                        (true, false) => Some(
                            "Unparenthesized `a ?: b ? c : d` is not supported. Use either `(a ?: b) ? c : d` or `a ?: (b ? c : d)`",
                        ),
                        (false, true) => Some(
                            "Unparenthesized `a ? b : c ?: d` is not supported. Use either `(a ? b : c) ?: d` or `a ? b : (c ?: d)`",
                        ),
                    };
                    if let Some(m) = msg {
                        let sp = lhs.span;
                        self.compile_error(m, sp);
                    }
                }
                let then = if self.eat(T::Colon) {
                    None
                } else {
                    let t = self.expr()?;
                    self.expect(T::Colon, &[])?;
                    Some(Box::new(t))
                };
                let else_ = self.expr_bp(BP_TERNARY + 1)?;
                let span = self.span_from(start);
                lhs = Expr {
                    kind: ExprKind::Ternary {
                        cond: Box::new(lhs),
                        then,
                        else_: Box::new(else_),
                    },
                    span,
                };
                paren = false;
                continue;
            }
            if k == T::Instanceof {
                if BP_INSTANCEOF < min {
                    break;
                }
                self.bump();
                let class = self.class_name_reference(true)?;
                let span = self.span_from(start);
                lhs = Expr {
                    kind: ExprKind::Instanceof {
                        expr: Box::new(lhs),
                        class,
                    },
                    span,
                };
                paren = false;
                continue;
            }
            let Some((bp, assoc, op)) = binary_op(k) else {
                break;
            };
            if bp < min {
                break;
            }
            self.bump();
            let rbp = if assoc == Assoc::Right { bp } else { bp + 1 };
            let (right, rparen) = self.expr_bp_p(rbp)?;
            if op == BinaryOp::Pipe && !rparen && matches!(right.kind, ExprKind::ArrowFn(_)) {
                let sp = right.span;
                self.compile_error(
                    "Arrow functions on the right hand side of |> must be parenthesized",
                    sp,
                );
            }
            let span = self.span_from(start);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(right),
                },
                span,
            };
            paren = false;
            if assoc == Assoc::Non
                && let Some((bp2, _, _)) = binary_op(self.peek_kind())
                && bp2 == bp
            {
                // Non-associative: `1 == 2 == 3` (observed on PHP 8.5:
                // "unexpected token "=="").
                return Err(self.unexpected(&[]));
            }
        }
        self.leave();
        Ok((lhs, paren))
    }

    /// `[...]` used as an assignment target becomes a destructuring list.
    pub(crate) fn to_assign_target(&self, e: Expr<'a>) -> Expr<'a> {
        match e.kind {
            ExprKind::Array { items, short: true } => Expr {
                kind: ExprKind::List {
                    items: items
                        .into_iter()
                        .map(|i| i.map(|it| self.list_item(it)))
                        .collect(),
                    short: true,
                },
                span: e.span,
            },
            ExprKind::List { items, short } => Expr {
                kind: ExprKind::List {
                    items: items
                        .into_iter()
                        .map(|i| i.map(|it| self.list_item(it)))
                        .collect(),
                    short,
                },
                span: e.span,
            },
            kind => Expr { kind, span: e.span },
        }
    }

    fn list_item(&self, it: ArrayItem<'a>) -> ArrayItem<'a> {
        ArrayItem {
            value: self.to_assign_target(it.value),
            ..it
        }
    }

    #[inline]
    fn mk(&self, kind: ExprKind<'a>, start: usize) -> Expr<'a> {
        Expr {
            kind,
            span: self.span_from(start),
        }
    }

    /// A prefix operator, a primary expression and its postfix chain. The
    /// flag tells whether the result is a bare parenthesized expression.
    fn prefix(&mut self) -> PResult<(Expr<'a>, bool)> {
        let start = self.pos;
        let t = *self.peek();
        let unary = |p: &mut Self, op: UnaryOp, bp: u8| -> PResult<(Expr<'a>, bool)> {
            p.bump();
            let e = p.expr_bp(bp)?;
            Ok((
                p.mk(
                    ExprKind::Unary {
                        op,
                        expr: Box::new(e),
                    },
                    start,
                ),
                false,
            ))
        };
        match t.kind {
            T::Bang => unary(self, UnaryOp::Not, BP_NOT),
            T::Tilde => unary(self, UnaryOp::BitNot, BP_UNARY),
            T::Minus => unary(self, UnaryOp::Neg, BP_UNARY),
            T::Plus => unary(self, UnaryOp::Plus, BP_UNARY),
            T::At => unary(self, UnaryOp::Silence, BP_UNARY),
            T::IntCast
            | T::DoubleCast
            | T::StringCast
            | T::ArrayCast
            | T::ObjectCast
            | T::BoolCast
            | T::UnsetCast
            | T::VoidCast => {
                self.bump();
                let kind = match t.kind {
                    T::IntCast => CastKind::Int,
                    T::DoubleCast => CastKind::Float,
                    T::StringCast => CastKind::String,
                    T::ArrayCast => CastKind::Array,
                    T::ObjectCast => CastKind::Object,
                    T::BoolCast => CastKind::Bool,
                    T::UnsetCast => CastKind::Unset,
                    _ => CastKind::Void,
                };
                self.check_cast_spelling(&t);
                if kind == CastKind::Void {
                    // `(void)` is a statement, not an expression (PHP 8.5
                    // RFC "Marking return values as important").
                    return Err(self.unexpected_at(start));
                }
                let e = self.expr_bp(BP_UNARY)?;
                Ok((
                    self.mk(
                        ExprKind::Cast {
                            kind,
                            expr: Box::new(e),
                        },
                        start,
                    ),
                    false,
                ))
            }
            T::Inc | T::Dec => {
                self.bump();
                let (e, paren) = self.expr_bp_p(BP_POW + 1)?;
                if paren {
                    // `++($a)` (observed on PHP 8.5).
                    return Err(self.unexpected(&[
                        T::ObjectOperator,
                        T::NullsafeObjectOperator,
                        T::LBracket,
                    ]));
                }
                if !is_variable(&e) {
                    return Err(self.unexpected(&[]));
                }
                let op = if t.kind == T::Inc {
                    IncDec::PreInc
                } else {
                    IncDec::PreDec
                };
                Ok((
                    self.mk(
                        ExprKind::IncDec {
                            op,
                            expr: Box::new(e),
                        },
                        start,
                    ),
                    false,
                ))
            }
            T::New => Ok((self.new_expr()?, false)),
            T::Clone => self.clone_expr(),
            T::Print => {
                self.bump();
                let e = self.expr_bp(BP_ASSIGN)?;
                Ok((self.mk(ExprKind::Print(Box::new(e)), start), false))
            }
            T::Yield => {
                self.bump();
                if !starts_expr(self.peek_kind()) {
                    return Ok((
                        self.mk(
                            ExprKind::Yield {
                                key: None,
                                value: None,
                            },
                            start,
                        ),
                        false,
                    ));
                }
                let v = self.expr_bp(BP_ASSIGN)?;
                if self.eat(T::DoubleArrow) {
                    let value = self.expr_bp(BP_ASSIGN)?;
                    return Ok((
                        self.mk(
                            ExprKind::Yield {
                                key: Some(Box::new(v)),
                                value: Some(Box::new(value)),
                            },
                            start,
                        ),
                        false,
                    ));
                }
                Ok((
                    self.mk(
                        ExprKind::Yield {
                            key: None,
                            value: Some(Box::new(v)),
                        },
                        start,
                    ),
                    false,
                ))
            }
            T::YieldFrom => {
                self.bump();
                let e = self.expr_bp(BP_ASSIGN)?;
                Ok((self.mk(ExprKind::YieldFrom(Box::new(e)), start), false))
            }
            T::Throw => {
                self.bump();
                let e = self.expr_bp(0)?;
                Ok((self.mk(ExprKind::Throw(Box::new(e)), start), false))
            }
            T::Include | T::IncludeOnce | T::Require | T::RequireOnce => {
                self.bump();
                let kind = match t.kind {
                    T::Include => IncludeKind::Include,
                    T::IncludeOnce => IncludeKind::IncludeOnce,
                    T::Require => IncludeKind::Require,
                    _ => IncludeKind::RequireOnce,
                };
                let e = self.expr_bp(BP_ASSIGN)?;
                Ok((
                    self.mk(
                        ExprKind::Include {
                            kind,
                            expr: Box::new(e),
                        },
                        start,
                    ),
                    false,
                ))
            }
            T::Function | T::Fn => Ok((self.closure(start, Vec::new(), false)?, false)),
            T::Attribute => {
                let attrs = self.attributes()?;
                if !matches!(self.peek_kind(), T::Function | T::Fn | T::Static) {
                    return Err(self.unexpected(&[T::Function, T::Fn, T::Static, T::Attribute]));
                }
                let is_static = self.eat(T::Static);
                if !matches!(self.peek_kind(), T::Function | T::Fn) {
                    return Err(self.unexpected(&[T::Function, T::Fn]));
                }
                Ok((self.closure(start, attrs, is_static)?, false))
            }
            T::Static => {
                if matches!(self.peek_at(1), T::Function | T::Fn) {
                    self.bump();
                    return Ok((self.closure(start, Vec::new(), true)?, false));
                }
                self.bump();
                if !self.at(T::DoubleColon) {
                    return Err(self.unexpected(&[T::DoubleColon]));
                }
                let base = ClassRef::Static(self.tok_span(&t));
                let e = self.static_member(base, start)?;
                Ok((self.postfix(e, start)?, false))
            }
            T::Match => Ok((self.match_expr()?, false)),
            _ => self.primary(),
        }
    }

    fn unexpected_at(&mut self, pos: usize) -> super::ParseError {
        self.pos = pos;
        self.unexpected(&[])
    }

    /// Non-canonical cast spellings are deprecated in PHP 8.5 (observed on
    /// PHP 8.5: "Non-canonical cast (integer) is deprecated, use the (int)
    /// cast instead").
    pub(crate) fn check_cast_spelling(&mut self, t: &Token) {
        let text = self.text(t);
        let inner: Vec<u8> = text[1..text.len() - 1]
            .iter()
            .filter(|c| !matches!(c, b' ' | b'\t'))
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let canon = match &inner[..] {
            b"integer" => "int",
            b"boolean" => "bool",
            b"double" => "float",
            b"binary" => "string",
            _ => return,
        };
        let msg = format!(
            "Non-canonical cast ({}) is deprecated, use the ({}) cast instead",
            String::from_utf8_lossy(&text[1..text.len() - 1]).trim_matches([' ', '\t']),
            canon
        );
        self.notice(NoticeKind::Deprecated, msg, t.line);
    }

    /// Primary expressions and their postfix chains.
    fn primary(&mut self) -> PResult<(Expr<'a>, bool)> {
        let start = self.pos;
        let t = *self.peek();
        let e = match t.kind {
            T::Variable => {
                self.bump();
                let name = &self.text(&t)[1..];
                self.mk(ExprKind::Variable(name), start)
            }
            T::Dollar => self.var_var()?,
            T::LParen => {
                self.bump();
                let inner = self.expr()?;
                self.expect(T::RParen, &[])?;
                if !self.is_postfix_start() {
                    // Keep the parentheses in the span.
                    let mut e = inner;
                    e.span = self.span_from(start);
                    return Ok((e, true));
                }
                let mut e = inner;
                e.span = self.span_from(start);
                e
            }
            T::LBracket => {
                self.bump();
                let items = self.array_items(T::RBracket)?;
                self.mk(ExprKind::Array { items, short: true }, start)
            }
            T::Array => {
                self.bump();
                self.expect1(T::LParen)?;
                let items = self.array_items(T::RParen)?;
                self.mk(
                    ExprKind::Array {
                        items,
                        short: false,
                    },
                    start,
                )
            }
            T::List => {
                self.bump();
                self.expect1(T::LParen)?;
                let items = self.array_items(T::RParen)?;
                let e = self.mk(
                    ExprKind::List {
                        items,
                        short: false,
                    },
                    start,
                );
                if self.list_ok == 0 && !self.at(T::Eq) {
                    return Err(self.unexpected(&[T::Eq]));
                }
                return Ok((e, false));
            }
            T::Isset => {
                self.bump();
                self.expect1(T::LParen)?;
                let mut vars = Vec::new();
                loop {
                    vars.push(self.expr()?);
                    if self.eat(T::Comma) {
                        if !starts_expr(self.peek_kind()) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
                self.expect1(T::RParen)?;
                return Ok((self.mk(ExprKind::Isset(vars), start), false));
            }
            T::Empty => {
                self.bump();
                self.expect1(T::LParen)?;
                let e = self.expr()?;
                self.expect(T::RParen, &[])?;
                return Ok((self.mk(ExprKind::Empty(Box::new(e)), start), false));
            }
            T::Eval => {
                self.bump();
                self.expect1(T::LParen)?;
                let e = self.expr()?;
                self.expect(T::RParen, &[])?;
                return Ok((self.mk(ExprKind::Eval(Box::new(e)), start), false));
            }
            T::Exit => {
                self.bump();
                let args = if self.at(T::LParen) {
                    Some(self.arguments()?)
                } else {
                    None
                };
                return Ok((self.mk(ExprKind::Exit(args), start), false));
            }
            T::LNumber | T::DNumber => {
                self.bump();
                let kind = match strings::parse_number(self.text(&t)) {
                    Number::Int(i) => ExprKind::Int(i),
                    Number::Float(f) => ExprKind::Float(f),
                };
                return Ok((self.mk(kind, start), false));
            }
            T::ConstantEncapsedString => {
                self.bump();
                let kind = self.constant_string(&t);
                self.mk(kind, start)
            }
            T::DoubleQuote => {
                self.bump();
                let parts = self.interp_parts(T::DoubleQuote, Quote::Double, 0)?;
                self.expect1(T::DoubleQuote)?;
                let kind = parts_to_kind(parts, StrKind::DoubleQuoted);
                self.mk(kind, start)
            }
            T::StartHeredoc => {
                let e = self.heredoc()?;
                return Ok((e, false));
            }
            T::Backtick => {
                self.bump();
                let parts = self.interp_parts(T::Backtick, Quote::Backtick, 0)?;
                self.expect1(T::Backtick)?;
                let e = self.mk(ExprKind::ShellExec(parts), start);
                return Ok((e, false));
            }
            T::Line
            | T::File
            | T::Dir
            | T::ClassC
            | T::TraitC
            | T::MethodC
            | T::FuncC
            | T::PropertyC
            | T::NsC => {
                self.bump();
                let m = match t.kind {
                    T::Line => MagicConst::Line,
                    T::File => MagicConst::File,
                    T::Dir => MagicConst::Dir,
                    T::ClassC => MagicConst::Class,
                    T::TraitC => MagicConst::Trait,
                    T::MethodC => MagicConst::Method,
                    T::FuncC => MagicConst::Function,
                    T::PropertyC => MagicConst::Property,
                    _ => MagicConst::Namespace,
                };
                self.mk(ExprKind::MagicConst(m), start)
            }
            T::String | T::NameQualified | T::NameFullyQualified | T::NameRelative => {
                self.bump();
                let name = self.name_from(&t);
                match self.peek_kind() {
                    T::LParen => {
                        let args = self.arguments()?;
                        self.mk(ExprKind::Call { name, args }, start)
                    }
                    T::DoubleColon => self.static_member(ClassRef::Name(name), start)?,
                    _ => self.mk(ExprKind::Const(name), start),
                }
            }
            T::Readonly if self.peek_at(1) == T::LParen => {
                // `readonly(...)` calls a function named readonly
                // (observed on PHP 8.5).
                self.bump();
                let name = Name {
                    kind: NameKind::Unqualified,
                    name: self.text(&t),
                    span: self.tok_span(&t),
                };
                let args = self.arguments()?;
                self.mk(ExprKind::Call { name, args }, start)
            }
            _ => return Err(self.unexpected(&[])),
        };
        let e = self.postfix(e, start)?;
        Ok((e, false))
    }

    fn is_postfix_start(&self) -> bool {
        matches!(
            self.peek_kind(),
            T::LBracket
                | T::ObjectOperator
                | T::NullsafeObjectOperator
                | T::DoubleColon
                | T::LParen
        )
    }

    pub(crate) fn name_from(&self, t: &Token) -> Name<'a> {
        let text = self.text(t);
        let (kind, name) = match t.kind {
            T::NameQualified => (NameKind::Qualified, text),
            T::NameFullyQualified => (NameKind::FullyQualified, &text[1..]),
            T::NameRelative => (NameKind::Relative, &text[10..]),
            _ => (NameKind::Unqualified, text),
        };
        Name {
            kind,
            name,
            span: self.tok_span(t),
        }
    }

    /// `$$a`, `${expr}`.
    pub(crate) fn var_var(&mut self) -> PResult<Expr<'a>> {
        let start = self.pos;
        self.bump(); // $
        let inner = match self.peek_kind() {
            T::Variable => {
                let t = self.bump();
                let name = &self.text(&t)[1..];
                Expr {
                    kind: ExprKind::Variable(name),
                    span: self.tok_span(&t),
                }
            }
            T::Dollar => self.var_var()?,
            T::LBrace => {
                self.bump();
                let e = self.expr()?;
                self.expect(T::RBrace, &[])?;
                e
            }
            _ => return Err(self.unexpected(&[T::Variable, T::LBrace, T::Dollar])),
        };
        Ok(self.mk(ExprKind::VariableVariable(Box::new(inner)), start))
    }

    /// The postfix chain: `[...]`, `->`, `?->`, `::`, `(...)`.
    pub(crate) fn postfix(&mut self, mut e: Expr<'a>, start: usize) -> PResult<Expr<'a>> {
        loop {
            match self.peek_kind() {
                T::LBracket => {
                    self.bump();
                    let index = if self.at(T::RBracket) {
                        None
                    } else {
                        Some(Box::new(self.expr()?))
                    };
                    self.expect1(T::RBracket)?;
                    e = self.mk(
                        ExprKind::Index {
                            base: Box::new(e),
                            index,
                        },
                        start,
                    );
                }
                T::ObjectOperator | T::NullsafeObjectOperator => {
                    let nullsafe = self.bump().kind == T::NullsafeObjectOperator;
                    let name = self.member_name()?;
                    if self.at(T::LParen) {
                        let args = self.arguments()?;
                        e = self.mk(
                            ExprKind::MethodCall {
                                obj: Box::new(e),
                                name,
                                args,
                                nullsafe,
                            },
                            start,
                        );
                    } else {
                        e = self.mk(
                            ExprKind::Prop {
                                obj: Box::new(e),
                                name,
                                nullsafe,
                            },
                            start,
                        );
                    }
                }
                T::DoubleColon => {
                    let cls = match e.kind {
                        ExprKind::Const(n) => ClassRef::Name(n),
                        kind => ClassRef::Expr(Box::new(Expr { kind, span: e.span })),
                    };
                    e = self.static_member(cls, start)?;
                }
                T::LParen => {
                    let args = self.arguments()?;
                    e = match e.kind {
                        ExprKind::Const(name) => self.mk(ExprKind::Call { name, args }, start),
                        kind => self.mk(
                            ExprKind::CallExpr {
                                callee: Box::new(Expr { kind, span: e.span }),
                                args,
                            },
                            start,
                        ),
                    };
                }
                _ => return Ok(e),
            }
        }
    }

    /// The name after `->` / `?->`.
    fn member_name(&mut self) -> PResult<MemberName<'a>> {
        let t = *self.peek();
        match t.kind {
            T::String => {
                self.bump();
                Ok(MemberName::Ident(Ident {
                    name: self.text(&t),
                    span: self.tok_span(&t),
                }))
            }
            T::Variable => {
                self.bump();
                Ok(MemberName::Expr(Box::new(Expr {
                    kind: ExprKind::Variable(&self.text(&t)[1..]),
                    span: self.tok_span(&t),
                })))
            }
            T::LBrace => {
                self.bump();
                let e = self.expr()?;
                self.expect(T::RBrace, &[])?;
                Ok(MemberName::Expr(Box::new(e)))
            }
            T::Dollar => Ok(MemberName::Expr(Box::new(self.var_var()?))),
            _ => Err(self.unexpected(&[T::String, T::Variable, T::LBrace, T::Dollar])),
        }
    }

    /// `X::...` after the class part; `::` is the current token.
    fn static_member(&mut self, class: ClassRef<'a>, start: usize) -> PResult<Expr<'a>> {
        self.expect1(T::DoubleColon)?;
        let t = *self.peek();
        match t.kind {
            T::Variable => {
                self.bump();
                let ident = Ident {
                    name: &self.text(&t)[1..],
                    span: self.tok_span(&t),
                };
                if self.at(T::LParen) {
                    // `A::$m()`: method named by a variable.
                    let args = self.arguments()?;
                    let name = MemberName::Expr(Box::new(Expr {
                        kind: ExprKind::Variable(ident.name),
                        span: ident.span,
                    }));
                    return Ok(self.mk(ExprKind::StaticCall { class, name, args }, start));
                }
                Ok(self.mk(
                    ExprKind::StaticProp {
                        class,
                        name: StaticPropName::Ident(ident),
                    },
                    start,
                ))
            }
            T::Dollar => {
                let vv = self.var_var()?;
                let ExprKind::VariableVariable(inner) = vv.kind else {
                    unreachable!()
                };
                if self.at(T::LParen) {
                    let args = self.arguments()?;
                    let name = MemberName::Expr(Box::new(Expr {
                        kind: ExprKind::VariableVariable(inner),
                        span: vv.span,
                    }));
                    return Ok(self.mk(ExprKind::StaticCall { class, name, args }, start));
                }
                Ok(self.mk(
                    ExprKind::StaticProp {
                        class,
                        name: StaticPropName::Expr(inner),
                    },
                    start,
                ))
            }
            T::LBrace => {
                self.bump();
                let e = self.expr()?;
                self.expect(T::RBrace, &[])?;
                let name = MemberName::Expr(Box::new(e));
                if self.at(T::LParen) {
                    let args = self.arguments()?;
                    return Ok(self.mk(ExprKind::StaticCall { class, name, args }, start));
                }
                Ok(self.mk(ExprKind::ClassConst { class, name }, start))
            }
            k if k == T::String || k.is_semi_reserved() => {
                self.bump();
                let ident = self.ident_from(&t)?;
                let name = MemberName::Ident(ident);
                if self.at(T::LParen) {
                    let args = self.arguments()?;
                    return Ok(self.mk(ExprKind::StaticCall { class, name, args }, start));
                }
                Ok(self.mk(ExprKind::ClassConst { class, name }, start))
            }
            _ => Err(self.unexpected(&[])),
        }
    }

    /// An identifier token (or a keyword used as one).
    pub(crate) fn ident_from(&self, t: &Token) -> PResult<Ident<'a>> {
        if t.kind == T::Echo && t.flags & TF_TAG != 0 {
            // Test Zend/tests/short_echo_as_identifier.phpt.
            return Err(self.parse_error_at("Cannot use \"<?=\" as an identifier", t));
        }
        Ok(Ident {
            name: self.text(t),
            span: self.tok_span(t),
        })
    }

    /// Call arguments, `(` is the current token.
    pub(crate) fn arguments(&mut self) -> PResult<Args<'a>> {
        self.expect1(T::LParen)?;
        if self.at(T::Ellipsis) && self.peek_at(1) == T::RParen {
            self.bump();
            self.bump();
            return Ok(Args::FirstClassCallable);
        }
        let mut args = Vec::new();
        let mut first = true;
        while !self.at(T::RParen) {
            if !first
                && !starts_expr(self.peek_kind())
                && !self.at(T::Ellipsis)
                && !self.peek_kind().is_semi_reserved()
            {
                // After a trailing comma only `)` may follow (observed on
                // PHP 8.5: "unexpected token ",", expecting ")"").
                return Err(self.unexpected(&[T::RParen]));
            }
            first = false;
            args.push(self.argument()?);
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect1(T::RParen)?;
        Ok(Args::List(args))
    }

    /// One call argument: `expr`, `...expr` or `name: expr`.
    pub(crate) fn argument(&mut self) -> PResult<Arg<'a>> {
        let start = self.pos;
        let k = self.peek_kind();
        if k == T::Ellipsis {
            self.bump();
            let v = self.expr()?;
            return Ok(Arg {
                name: None,
                value: v,
                unpack: true,
                span: self.span_from(start),
            });
        }
        if (k == T::String || k.is_semi_reserved()) && self.peek_at(1) == T::Colon {
            let t = self.bump();
            let name = self.ident_from(&t)?;
            self.bump();
            let v = self.expr()?;
            return Ok(Arg {
                name: Some(name),
                value: v,
                unpack: false,
                span: self.span_from(start),
            });
        }
        if k.is_semi_reserved() && !matches!(k, T::Fn | T::Function) {
            // A keyword could also start a named argument: when it does
            // not start an expression here, PHP expects `:` after it
            // (observed on PHP 8.5: `foo(print)`).
            if !starts_expr(k) {
                self.bump();
                return Err(self.unexpected(&[T::Colon]));
            }
            let next = self.toks[(self.pos + 1).min(self.toks.len() - 1)].start;
            match self.expr() {
                Ok(v) => {
                    return Ok(Arg {
                        name: None,
                        value: v,
                        unpack: false,
                        span: self.span_from(start),
                    });
                }
                Err(e) => {
                    if e.offset == next
                        && e.message.starts_with("syntax error")
                        && self.toks[start + 1].flags & crate::token::TF_ERROR == 0
                    {
                        self.pos = start + 1;
                        return Err(self.unexpected(&[T::Colon]));
                    }
                    return Err(e);
                }
            }
        }
        let v = self.expr()?;
        Ok(Arg {
            name: None,
            value: v,
            unpack: false,
            span: self.span_from(start),
        })
    }

    /// Array or list items up to `close` (consumed).
    pub(crate) fn array_items(&mut self, close: T) -> PResult<Vec<Option<ArrayItem<'a>>>> {
        self.list_ok += 1;
        let r = self.array_items_inner(close);
        self.list_ok -= 1;
        r
    }

    fn array_items_inner(&mut self, close: T) -> PResult<Vec<Option<ArrayItem<'a>>>> {
        let mut items = Vec::new();
        loop {
            if self.at(close) {
                break;
            }
            if self.at(T::Comma) {
                self.bump();
                items.push(None);
                continue;
            }
            let k = self.peek_kind();
            if !starts_expr(k)
                && k != T::Ellipsis
                && !matches!(
                    k,
                    T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg
                )
            {
                // Observed on PHP 8.5: `[1, FOREACH]` expects "]".
                return Err(self.unexpected(&[close]));
            }
            let start = self.pos;
            let item = if self.at(T::Ellipsis) {
                self.bump();
                let v = self.expr()?;
                ArrayItem {
                    key: None,
                    value: v,
                    by_ref: false,
                    unpack: true,
                    span: self.span_from(start),
                }
            } else if self.eat_amp() {
                let v = self.expr()?;
                ArrayItem {
                    key: None,
                    value: v,
                    by_ref: true,
                    unpack: false,
                    span: self.span_from(start),
                }
            } else {
                let v = self.expr()?;
                if self.eat(T::DoubleArrow) {
                    let by_ref = self.eat_amp();
                    let value = self.expr()?;
                    ArrayItem {
                        key: Some(v),
                        value,
                        by_ref,
                        unpack: false,
                        span: self.span_from(start),
                    }
                } else {
                    ArrayItem {
                        key: None,
                        value: v,
                        by_ref: false,
                        unpack: false,
                        span: self.span_from(start),
                    }
                }
            };
            items.push(Some(item));
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect1(close)?;
        Ok(items)
    }

    pub(crate) fn eat_amp(&mut self) -> bool {
        if matches!(
            self.peek_kind(),
            T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg
        ) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// `new ...`
    fn new_expr(&mut self) -> PResult<Expr<'a>> {
        let start = self.pos;
        self.bump(); // new
        let attrs = if self.at(T::Attribute) {
            self.attributes()?
        } else {
            Vec::new()
        };
        let mut mods = Modifiers::default();
        while matches!(self.peek_kind(), T::Readonly | T::Final | T::Abstract)
            && self.peek_at(1) != T::LParen
        {
            let t = self.bump();
            self.add_modifier(&mut mods, &t, super::decl::ModTarget::AnonClass)?;
        }
        if self.at(T::Class) {
            let cstart = self.pos;
            let doc = self.doc_comment_before(cstart);
            self.bump();
            let args = if self.at(T::LParen) {
                Some(self.arguments()?)
            } else {
                None
            };
            let decl = self.class_body_decl(cstart, ClassKind::Class, None, attrs, mods, doc)?;
            let e = self.mk(
                ExprKind::New {
                    class: NewClass::Anonymous(Box::new(decl)),
                    args,
                },
                start,
            );
            // PHP 8.4: `new class {}->m()`.
            return self.postfix(e, start);
        }
        if !attrs.is_empty() || !mods.is_empty() {
            return Err(self.unexpected(&[T::Class]));
        }
        if !matches!(
            self.peek_kind(),
            T::String
                | T::NameQualified
                | T::NameFullyQualified
                | T::NameRelative
                | T::Static
                | T::Variable
                | T::Dollar
                | T::LParen
        ) {
            return Err(self.unexpected(&[T::Class]));
        }
        let class = self.class_name_reference(false)?;
        let args = if self.at(T::LParen) {
            Some(self.arguments()?)
        } else {
            None
        };
        let has_args = args.is_some();
        let e = self.mk(
            ExprKind::New {
                class: NewClass::Ref(class),
                args,
            },
            start,
        );
        if has_args {
            // PHP 8.4: `new A()->b()` without extra parentheses.
            return self.postfix(e, start);
        }
        Ok(e)
    }

    /// The class part of `new X` or `instanceof X`: a name, `static`, a
    /// variable with property / static property / index accesses, or
    /// `(expr)`.
    pub(crate) fn class_name_reference(&mut self, instanceof: bool) -> PResult<ClassRef<'a>> {
        let t = *self.peek();
        match t.kind {
            T::String | T::NameQualified | T::NameFullyQualified | T::NameRelative => {
                self.bump();
                let name = self.name_from(&t);
                if self.at(T::DoubleColon)
                    && !matches!(self.peek_at(1), T::Variable | T::Dollar)
                    && !instanceof
                {
                    // `new A::C` (test new_without_ctor_arguments_parentheses_constant).
                    self.bump();
                    return Err(self.unexpected(&[T::Variable, T::Dollar]));
                }
                if self.at(T::DoubleColon) && matches!(self.peek_at(1), T::Variable | T::Dollar) {
                    let start = self.pos - 1;
                    let e = self.new_variable_tail(
                        Expr {
                            kind: ExprKind::Const(name),
                            span: name.span,
                        },
                        start,
                    )?;
                    return Ok(ClassRef::Expr(Box::new(e)));
                }
                Ok(ClassRef::Name(name))
            }
            T::Static => {
                self.bump();
                let sp = self.tok_span(&t);
                if self.at(T::DoubleColon) && matches!(self.peek_at(1), T::Variable | T::Dollar) {
                    let start = self.pos - 1;
                    let e = self.new_variable_tail(
                        Expr {
                            kind: ExprKind::Const(Name {
                                kind: NameKind::Unqualified,
                                name: b"static",
                                span: sp,
                            }),
                            span: sp,
                        },
                        start,
                    )?;
                    return Ok(ClassRef::Expr(Box::new(e)));
                }
                Ok(ClassRef::Static(sp))
            }
            T::LParen => {
                self.bump();
                let e = self.expr()?;
                self.expect(T::RParen, &[])?;
                Ok(ClassRef::Expr(Box::new(e)))
            }
            T::Variable | T::Dollar => {
                let start = self.pos;
                let base = if t.kind == T::Variable {
                    self.bump();
                    self.mk(ExprKind::Variable(&self.text(&t)[1..]), start)
                } else {
                    self.var_var()?
                };
                let e = self.new_variable_tail(base, start)?;
                Ok(ClassRef::Expr(Box::new(e)))
            }
            _ => Err(self.unexpected(&[])),
        }
    }

    /// Property, static property and index accesses after a `new` /
    /// `instanceof` class variable (no calls).
    fn new_variable_tail(&mut self, mut e: Expr<'a>, start: usize) -> PResult<Expr<'a>> {
        loop {
            match self.peek_kind() {
                T::LBracket => {
                    self.bump();
                    let index = if self.at(T::RBracket) {
                        None
                    } else {
                        Some(Box::new(self.expr()?))
                    };
                    self.expect1(T::RBracket)?;
                    e = self.mk(
                        ExprKind::Index {
                            base: Box::new(e),
                            index,
                        },
                        start,
                    );
                }
                T::ObjectOperator | T::NullsafeObjectOperator => {
                    let nullsafe = self.bump().kind == T::NullsafeObjectOperator;
                    let name = self.member_name()?;
                    e = self.mk(
                        ExprKind::Prop {
                            obj: Box::new(e),
                            name,
                            nullsafe,
                        },
                        start,
                    );
                }
                T::DoubleColon if matches!(self.peek_at(1), T::Variable | T::Dollar) => {
                    self.bump();
                    let class = match e.kind {
                        ExprKind::Const(n) => {
                            if n.name == b"static" && n.kind == NameKind::Unqualified {
                                ClassRef::Static(n.span)
                            } else {
                                ClassRef::Name(n)
                            }
                        }
                        kind => ClassRef::Expr(Box::new(Expr { kind, span: e.span })),
                    };
                    let name = if self.at(T::Variable) {
                        let t = self.bump();
                        StaticPropName::Ident(Ident {
                            name: &self.text(&t)[1..],
                            span: self.tok_span(&t),
                        })
                    } else {
                        let vv = self.var_var()?;
                        let ExprKind::VariableVariable(inner) = vv.kind else {
                            unreachable!()
                        };
                        StaticPropName::Expr(inner)
                    };
                    e = self.mk(ExprKind::StaticProp { class, name }, start);
                }
                _ => return Ok(e),
            }
        }
    }

    /// `clone $x` or PHP 8.5's `clone($x, [...])` / `clone(...)` (RFC
    /// "Clone with v2"; observed on PHP 8.5: `clone($a)->b` clones `$a`'s
    /// property, `clone($a, [1])->b` is a syntax error).
    fn clone_expr(&mut self) -> PResult<(Expr<'a>, bool)> {
        let start = self.pos;
        self.bump();
        if !self.at(T::LParen) {
            let e = self.expr_bp(BP_POW + 1)?;
            return Ok((
                self.mk(
                    ExprKind::Clone {
                        expr: Some(Box::new(e)),
                        args: None,
                    },
                    start,
                ),
                false,
            ));
        }
        let paren = self.pos;
        let k1 = self.peek_at(1);
        let named = (k1 == T::String || k1.is_semi_reserved()) && self.peek_at(2) == T::Colon;
        if starts_expr(k1) && !named {
            self.bump();
            let first = self.expr()?;
            if self.at(T::RParen) {
                self.bump();
                // `clone(expr)` possibly followed by `->...`: the operand
                // is `(expr)` with its postfix chain.
                let mut operand = first;
                operand.span = self.span_from(paren);
                let operand = self.postfix(operand, paren)?;
                return Ok((
                    self.mk(
                        ExprKind::Clone {
                            expr: Some(Box::new(operand)),
                            args: None,
                        },
                        start,
                    ),
                    false,
                ));
            }
            if !self.at(T::Comma) {
                return Err(self.unexpected(&[]));
            }
            let fspan = first.span;
            let mut args = vec![Arg {
                name: None,
                value: first,
                unpack: false,
                span: fspan,
            }];
            while self.eat(T::Comma) {
                if self.at(T::RParen) {
                    break;
                }
                if !starts_expr(self.peek_kind())
                    && !self.at(T::Ellipsis)
                    && !self.peek_kind().is_semi_reserved()
                {
                    return Err(self.unexpected(&[T::RParen]));
                }
                args.push(self.argument()?);
            }
            self.expect1(T::RParen)?;
            return Ok((
                self.mk(
                    ExprKind::Clone {
                        expr: None,
                        args: Some(Args::List(args)),
                    },
                    start,
                ),
                false,
            ));
        }
        let args = self.arguments()?;
        Ok((
            self.mk(
                ExprKind::Clone {
                    expr: None,
                    args: Some(args),
                },
                start,
            ),
            false,
        ))
    }

    /// `match (x) { ... }`
    fn match_expr(&mut self) -> PResult<Expr<'a>> {
        let start = self.pos;
        self.bump();
        self.expect1(T::LParen)?;
        let subject = self.expr()?;
        self.expect(T::RParen, &[])?;
        self.expect1(T::LBrace)?;
        let mut arms = Vec::new();
        while !self.at(T::RBrace) {
            if !starts_expr(self.peek_kind()) && !self.at(T::Default) {
                return Err(self.unexpected(&[T::RBrace]));
            }
            let astart = self.pos;
            let conds = if self.at(T::Default) {
                self.bump();
                self.eat(T::Comma);
                None
            } else {
                let mut conds = Vec::new();
                loop {
                    conds.push(self.expr()?);
                    if self.eat(T::Comma) {
                        if self.at(T::DoubleArrow) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
                Some(conds)
            };
            self.expect1(T::DoubleArrow)?;
            let body = self.expr()?;
            arms.push(MatchArm {
                conds,
                body,
                span: self.span_from(astart),
            });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect1(T::RBrace)?;
        Ok(self.mk(
            ExprKind::Match {
                subject: Box::new(subject),
                arms,
            },
            start,
        ))
    }

    /// `function (...) use (...) {}` or `fn (...) => expr`, at the
    /// `function` / `fn` token.
    fn closure(
        &mut self,
        start: usize,
        attrs: Vec<AttributeGroup<'a>>,
        is_static: bool,
    ) -> PResult<Expr<'a>> {
        let doc_comment = self.doc_comment_before(start);
        let kw = self.bump();
        let by_ref = self.eat_amp();
        if !self.at(T::LParen) {
            return Err(self.unexpected(&[T::LParen]));
        }
        let params = self.parameter_list()?;
        if kw.kind == T::Fn {
            let ret = if self.eat(T::Colon) {
                Some(self.return_type()?)
            } else {
                None
            };
            self.expect1(T::DoubleArrow)?;
            let body = self.expr_bp(0)?;
            return Ok(self.mk(
                ExprKind::ArrowFn(Box::new(ArrowFn {
                    attrs,
                    is_static,
                    by_ref,
                    params,
                    ret,
                    body: Box::new(body),
                    doc_comment,
                })),
                start,
            ));
        }
        let mut uses = Vec::new();
        if self.eat(T::Use) {
            self.expect1(T::LParen)?;
            loop {
                let ustart = self.pos;
                let by_ref = self.eat_amp();
                let t = self.expect1(T::Variable)?;
                uses.push(ClosureUse {
                    name: Ident {
                        name: &self.text(&t)[1..],
                        span: self.tok_span(&t),
                    },
                    by_ref,
                    span: self.span_from(ustart),
                });
                if self.eat(T::Comma) {
                    if !matches!(
                        self.peek_kind(),
                        T::Variable | T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg
                    ) {
                        break;
                    }
                    continue;
                }
                break;
            }
            self.expect1(T::RParen)?;
        }
        let ret = if self.eat(T::Colon) {
            Some(self.return_type()?)
        } else {
            None
        };
        if !self.at(T::LBrace) {
            return Err(self.unexpected(&[T::LBrace]));
        }
        let body = self.block_body()?;
        Ok(self.mk(
            ExprKind::Closure(Box::new(Closure {
                attrs,
                is_static,
                by_ref,
                params,
                uses,
                ret,
                body,
                doc_comment,
            })),
            start,
        ))
    }

    // ----- strings -----

    fn constant_string(&mut self, t: &Token) -> ExprKind<'a> {
        let text = self.text(t);
        let body = if t.flags & TF_BINARY_PREFIX != 0 {
            &text[1..]
        } else {
            text
        };
        let inner = &body[1..body.len() - 1];
        if t.flags & TF_DOUBLE != 0 {
            let d = strings::unescape_double(inner, Quote::Double);
            for w in d.warnings {
                self.notice(NoticeKind::Warning, w, t.line);
            }
            ExprKind::String(d.bytes, StrKind::DoubleQuoted)
        } else {
            ExprKind::String(strings::unescape_single(inner), StrKind::SingleQuoted)
        }
    }

    /// Parts of an interpolated string up to `close` (not consumed).
    /// `indent`: heredoc closing-marker indentation to strip.
    fn interp_parts(&mut self, close: T, q: Quote, indent: usize) -> PResult<Vec<StrPart<'a>>> {
        let mut parts = Vec::new();
        loop {
            let t = *self.peek();
            match t.kind {
                T::EncapsedAndWhitespace => {
                    self.bump();
                    let raw = self.text(&t);
                    let text = if indent > 0 {
                        let at_ls =
                            t.start > 0 && matches!(self.src[t.start as usize - 1], b'\n' | b'\r');
                        strings::remove_indent(raw, indent, at_ls)
                    } else {
                        Cow::Borrowed(raw)
                    };
                    let decoded = match text {
                        Cow::Borrowed(b) => {
                            let d = strings::unescape_double(b, q);
                            for w in d.warnings {
                                self.notice(NoticeKind::Warning, w, t.line);
                            }
                            d.bytes
                        }
                        Cow::Owned(v) => {
                            let d = strings::unescape_double(&v, q);
                            for w in d.warnings {
                                self.notice(NoticeKind::Warning, w, t.line);
                            }
                            Cow::Owned(d.bytes.into_owned())
                        }
                    };
                    parts.push(StrPart::Lit(decoded));
                }
                T::Variable => {
                    let start = self.pos;
                    self.bump();
                    let mut e = self.mk(ExprKind::Variable(&self.text(&t)[1..]), start);
                    match self.peek_kind() {
                        T::LBracket => {
                            self.bump();
                            let it = *self.peek();
                            let index = match it.kind {
                                T::String => {
                                    self.bump();
                                    self.mk(
                                        ExprKind::String(
                                            Cow::Borrowed(self.text(&it)),
                                            StrKind::SingleQuoted,
                                        ),
                                        start + 2,
                                    )
                                }
                                T::NumString => {
                                    self.bump();
                                    let k = num_string_key(self.text(&it), false);
                                    self.mk(k, start + 2)
                                }
                                T::Minus => {
                                    self.bump();
                                    let nt = self.expect1(T::NumString)?;
                                    let k = num_string_key(self.text(&nt), true);
                                    self.mk(k, start + 2)
                                }
                                T::Variable => {
                                    self.bump();
                                    self.mk(ExprKind::Variable(&self.text(&it)[1..]), start + 2)
                                }
                                _ => {
                                    return Err(self.unexpected(&[
                                        T::Minus,
                                        T::String,
                                        T::Variable,
                                        T::NumString,
                                    ]));
                                }
                            };
                            self.expect1(T::RBracket)?;
                            e = self.mk(
                                ExprKind::Index {
                                    base: Box::new(e),
                                    index: Some(Box::new(index)),
                                },
                                start,
                            );
                        }
                        T::ObjectOperator | T::NullsafeObjectOperator => {
                            let nullsafe = self.bump().kind == T::NullsafeObjectOperator;
                            let nt = self.expect1(T::String)?;
                            let name = MemberName::Ident(Ident {
                                name: self.text(&nt),
                                span: self.tok_span(&nt),
                            });
                            e = self.mk(
                                ExprKind::Prop {
                                    obj: Box::new(e),
                                    name,
                                    nullsafe,
                                },
                                start,
                            );
                        }
                        _ => {}
                    }
                    parts.push(StrPart::Expr(e));
                }
                T::DollarOpenCurlyBraces => {
                    let start = self.pos;
                    self.bump();
                    let e = if self.at(T::StringVarname) {
                        let nt = self.bump();
                        let var = self.mk(ExprKind::Variable(self.text(&nt)), start);
                        if self.eat(T::LBracket) {
                            let idx = self.expr()?;
                            self.expect(T::RBracket, &[])?;
                            self.expect1(T::RBrace)?;
                            self.mk(
                                ExprKind::Index {
                                    base: Box::new(var),
                                    index: Some(Box::new(idx)),
                                },
                                start,
                            )
                        } else {
                            self.expect1(T::RBrace)?;
                            var
                        }
                    } else {
                        let inner = self.expr()?;
                        self.expect(T::RBrace, &[])?;
                        self.mk(ExprKind::VariableVariable(Box::new(inner)), start)
                    };
                    self.deprecated_dollar_brace(&t);
                    parts.push(StrPart::Expr(e));
                }
                T::CurlyOpen => {
                    self.bump();
                    if !matches!(self.peek_kind(), T::Variable | T::Dollar) {
                        return Err(self.unexpected(&[]));
                    }
                    let (e, _) = self.expr_bp_p(BP_POW + 1)?;
                    if !self.at(T::RBrace) {
                        return Err(self.unexpected(&[
                            T::ObjectOperator,
                            T::NullsafeObjectOperator,
                            T::LBracket,
                        ]));
                    }
                    self.bump();
                    parts.push(StrPart::Expr(e));
                }
                k if k == close => return Ok(parts),
                // Observed on PHP 8.5: end of input right after `<<<X`
                // lists nothing.
                T::Eof
                    if close == T::EndHeredoc
                        && parts.is_empty()
                        && self.toks[self.pos - 1].kind == T::StartHeredoc =>
                {
                    return Err(self.unexpected(&[]));
                }
                _ => {
                    // Observed on PHP 8.5: after a lone literal part PHP
                    // lists what may follow; after an interpolation, nothing.
                    let mut ex = vec![T::Variable, T::DollarOpenCurlyBraces, T::CurlyOpen];
                    if close == T::EndHeredoc {
                        ex.push(T::EndHeredoc);
                    }
                    if close == T::Backtick {
                        ex = vec![T::Backtick];
                    } else if !(parts.len() == 1 && matches!(parts[0], StrPart::Lit(_))) {
                        ex.clear();
                    }
                    return Err(self.unexpected(&ex));
                }
            }
        }
    }

    /// `"${var}"` is deprecated (PHP 8.2 RFC "Deprecate ${} string
    /// interpolation").
    fn deprecated_dollar_brace(&mut self, t: &Token) {
        let _ = t;
    }

    fn heredoc(&mut self) -> PResult<Expr<'a>> {
        let start = self.pos;
        let st = self.bump();
        let nowdoc = st.flags & TF_DOUBLE == 0;
        // Find the closing marker to learn its indentation.
        let mut j = self.pos;
        let mut depth = 0usize;
        while j < self.toks.len() {
            match self.toks[j].kind {
                T::StartHeredoc => depth += 1,
                T::EndHeredoc if depth == 0 => break,
                T::EndHeredoc => depth -= 1,
                T::Eof => break,
                _ => {}
            }
            j += 1;
        }
        let indent = if j < self.toks.len() && self.toks[j].kind == T::EndHeredoc {
            let et = self.toks[j];
            let text = self.text(&et);
            text.iter()
                .take_while(|&&c| c == b' ' || c == b'\t')
                .count()
        } else {
            0
        };
        if nowdoc {
            let mut value: Cow<'a, [u8]> = Cow::Borrowed(b"");
            if self.at(T::EncapsedAndWhitespace) {
                let t = self.bump();
                value = strings::remove_indent(self.text(&t), indent, true);
            }
            strings::strip_trailing_newline(&mut value);
            if !self.at(T::EndHeredoc) {
                if self.toks[self.pos - 1].kind == T::StartHeredoc {
                    return Err(self.unexpected(&[]));
                }
                return Err(self.unexpected(&[
                    T::Variable,
                    T::EndHeredoc,
                    T::DollarOpenCurlyBraces,
                    T::CurlyOpen,
                ]));
            }
            self.bump();
            return Ok(self.mk(ExprKind::String(value, StrKind::Nowdoc), start));
        }
        let mut parts = self.interp_parts(T::EndHeredoc, Quote::Heredoc, indent)?;
        self.expect1(T::EndHeredoc)?;
        if let Some(StrPart::Lit(s)) = parts.last_mut() {
            strings::strip_trailing_newline(s);
        }
        if let Some(StrPart::Lit(s)) = parts.last()
            && s.is_empty()
            && parts.len() > 1
        {
            parts.pop();
        }
        let kind = parts_to_kind(parts, StrKind::Heredoc);
        Ok(self.mk(kind, start))
    }
}

/// A string with only literal parts becomes a plain string.
fn parts_to_kind(mut parts: Vec<StrPart<'_>>, kind: StrKind) -> ExprKind<'_> {
    if parts.iter().any(|p| matches!(p, StrPart::Expr(_))) {
        // Literal parts emptied by heredoc indentation removal.
        parts.retain(|p| !matches!(p, StrPart::Lit(s) if s.is_empty()));
    }
    if parts.iter().all(|p| matches!(p, StrPart::Lit(_))) {
        if parts.len() == 1 {
            let StrPart::Lit(s) = parts.pop().unwrap() else {
                unreachable!()
            };
            return ExprKind::String(s, kind);
        }
        let mut out = Vec::new();
        for p in parts {
            if let StrPart::Lit(s) = p {
                out.extend_from_slice(&s);
            }
        }
        return ExprKind::String(Cow::Owned(out), kind);
    }
    ExprKind::Interpolated(parts, kind)
}

/// The key of `"$a[12]"` / `"$a[-1]"`: an integer when the text is a
/// canonical decimal integer, else a string (observed on PHP 8.5).
fn num_string_key(text: &[u8], neg: bool) -> ExprKind<'_> {
    let canonical = !text.is_empty()
        && text.iter().all(|c| c.is_ascii_digit())
        && (text.len() == 1 || text[0] != b'0')
        && !(neg && text == b"0");
    if canonical && let Ok(s) = std::str::from_utf8(text) {
        let full = if neg {
            format!("-{}", s)
        } else {
            s.to_string()
        };
        if let Ok(v) = full.parse::<i64>() {
            return ExprKind::Int(v);
        }
    }
    if neg {
        let mut v = Vec::with_capacity(text.len() + 1);
        v.push(b'-');
        v.extend_from_slice(text);
        ExprKind::String(Cow::Owned(v), StrKind::SingleQuoted)
    } else {
        ExprKind::String(Cow::Borrowed(text), StrKind::SingleQuoted)
    }
}
