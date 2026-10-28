//! Statements (PHP manual: "Control Structures", "Namespaces").

use super::expr::starts_expr;
use super::{PResult, Parser};
use crate::ast::*;
use crate::token::T;

impl<'a> Parser<'a> {
    /// Statements up to end of input (`top`: file level, where namespace,
    /// use, const and `__halt_compiler` are allowed).
    pub(crate) fn top_statements(&mut self, top: bool) -> PResult<Vec<Stmt<'a>>> {
        let mut stmts = Vec::new();
        loop {
            match self.peek_kind() {
                T::Eof => break,
                T::RBrace if !top => break,
                _ => {}
            }
            if !self.starts_statement(true) {
                break;
            }
            let s = self.statement(true)?;
            let halt = matches!(s.kind, StmtKind::HaltCompiler { .. });
            stmts.push(s);
            if halt {
                break;
            }
        }
        Ok(stmts)
    }

    /// Whether the current token can start a statement.
    fn starts_statement(&self, top: bool) -> bool {
        let k = self.peek_kind();
        if starts_expr(k) {
            return true;
        }
        match k {
            T::LBrace
            | T::If
            | T::While
            | T::Do
            | T::For
            | T::Foreach
            | T::Switch
            | T::Break
            | T::Continue
            | T::Return
            | T::Echo
            | T::Global
            | T::Unset
            | T::Try
            | T::Goto
            | T::Declare
            | T::InlineHtml
            | T::Semicolon
            | T::Abstract
            | T::Final
            | T::Class
            | T::Interface
            | T::Trait
            | T::Enum
            | T::HaltCompiler => true,
            T::Namespace | T::Use | T::Const => top,
            _ => false,
        }
    }

    /// Inner statements up to `}` or an alternative-syntax end keyword.
    pub(crate) fn inner_statements(&mut self, ends: &[T]) -> PResult<Vec<Stmt<'a>>> {
        let mut stmts = Vec::new();
        loop {
            let k = self.peek_kind();
            if k == T::Eof || ends.contains(&k) || !self.starts_statement(false) {
                break;
            }
            stmts.push(self.statement(false)?);
        }
        Ok(stmts)
    }

    /// `{ statements }` as a function body.
    pub(crate) fn block_body(&mut self) -> PResult<Vec<Stmt<'a>>> {
        self.expect1(T::LBrace)?;
        let stmts = self.inner_statements(&[T::RBrace])?;
        // Observed on PHP 8.5: `{ catch }` lists nothing.
        self.expect(T::RBrace, &[])?;
        Ok(stmts)
    }

    #[inline]
    fn stmt(&self, kind: StmtKind<'a>, start: usize) -> Stmt<'a> {
        Stmt {
            kind,
            span: self.span_from(start),
        }
    }

    /// `;` ending a statement (or `?>`).
    fn end_stmt(&mut self, expected: &[T]) -> PResult<()> {
        if self.eat(T::Semicolon) {
            Ok(())
        } else {
            Err(self.unexpected(expected))
        }
    }

    pub(crate) fn statement(&mut self, top: bool) -> PResult<Stmt<'a>> {
        self.enter()?;
        let r = self.statement_inner(top);
        self.leave();
        r
    }

    fn statement_inner(&mut self, top: bool) -> PResult<Stmt<'a>> {
        let start = self.pos;
        let t = *self.peek();
        match t.kind {
            T::LBrace => {
                self.bump();
                let body = self.inner_statements(&[T::RBrace])?;
                self.expect(T::RBrace, &[])?;
                Ok(self.stmt(StmtKind::Block(body), start))
            }
            T::Semicolon => {
                self.bump();
                Ok(self.stmt(StmtKind::Nop, start))
            }
            T::InlineHtml => {
                self.bump();
                Ok(self.stmt(StmtKind::InlineHtml(self.text(&t)), start))
            }
            T::If => self.if_stmt(),
            T::While => {
                self.bump();
                let cond = self.paren_expr()?;
                if self.eat(T::Colon) {
                    let body = self.inner_statements(&[T::Endwhile])?;
                    self.expect(T::Endwhile, &[])?;
                    self.end_stmt(&[T::Semicolon])?;
                    return Ok(self.stmt(
                        StmtKind::While {
                            cond,
                            body,
                            alt_syntax: true,
                        },
                        start,
                    ));
                }
                let body = self.body_statement()?;
                Ok(self.stmt(
                    StmtKind::While {
                        cond,
                        body,
                        alt_syntax: false,
                    },
                    start,
                ))
            }
            T::Do => {
                self.bump();
                let body = self.body_statement()?;
                self.expect1(T::While)?;
                let cond = self.paren_expr()?;
                self.end_stmt(&[T::Semicolon])?;
                Ok(self.stmt(StmtKind::DoWhile { body, cond }, start))
            }
            T::For => self.for_stmt(),
            T::Foreach => self.foreach_stmt(),
            T::Switch => self.switch_stmt(),
            T::Break | T::Continue => {
                self.bump();
                let e = if starts_expr(self.peek_kind()) {
                    Some(self.expr()?)
                } else {
                    None
                };
                self.end_stmt(&[T::Semicolon])?;
                Ok(self.stmt(
                    if t.kind == T::Break {
                        StmtKind::Break(e)
                    } else {
                        StmtKind::Continue(e)
                    },
                    start,
                ))
            }
            T::Return => {
                self.bump();
                let e = if starts_expr(self.peek_kind()) {
                    Some(self.expr()?)
                } else {
                    None
                };
                self.end_stmt(&[T::Semicolon])?;
                Ok(self.stmt(StmtKind::Return(e), start))
            }
            T::Echo => {
                self.bump();
                let mut exprs = vec![self.expr()?];
                while self.eat(T::Comma) {
                    exprs.push(self.expr()?);
                }
                self.end_stmt(&[T::Comma, T::Semicolon])?;
                Ok(self.stmt(StmtKind::Echo(exprs), start))
            }
            T::Global => {
                self.bump();
                let mut vars = Vec::new();
                loop {
                    let vstart = self.pos;
                    let v = match self.peek_kind() {
                        T::Variable => {
                            let vt = self.bump();
                            Expr {
                                kind: ExprKind::Variable(&self.text(&vt)[1..]),
                                span: self.span_from(vstart),
                            }
                        }
                        T::Dollar => self.var_var()?,
                        _ => return Err(self.unexpected(&[T::Variable, T::Dollar])),
                    };
                    vars.push(v);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.end_stmt(&[T::Comma, T::Semicolon])?;
                Ok(self.stmt(StmtKind::Global(vars), start))
            }
            T::Static if self.peek_at(1) == T::Variable => {
                self.bump();
                let mut vars = Vec::new();
                loop {
                    let vstart = self.pos;
                    let vt = self.expect1(T::Variable)?;
                    let name = Ident {
                        name: &self.text(&vt)[1..],
                        span: self.tok_span(&vt),
                    };
                    let default = if self.eat(T::Eq) {
                        Some(self.expr()?)
                    } else {
                        None
                    };
                    vars.push(StaticVar {
                        name,
                        default,
                        span: self.span_from(vstart),
                    });
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.end_stmt(&[T::Comma, T::Semicolon])?;
                Ok(self.stmt(StmtKind::Static(vars), start))
            }
            T::Unset => {
                self.bump();
                self.expect1(T::LParen)?;
                let mut vars = Vec::new();
                // Observed on PHP 8.5: `unset($a 2)` and `unset(new A)`
                // expect "->", "?->" or "[".
                let postfix = [T::ObjectOperator, T::NullsafeObjectOperator, T::LBracket];
                loop {
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
                            | T::LBracket
                            | T::Array
                            | T::ConstantEncapsedString
                            | T::DoubleQuote
                            | T::New
                    ) {
                        if vars.is_empty() {
                            return Err(self.unexpected(&[]));
                        }
                        if self.at(T::RParen) {
                            break;
                        }
                        return Err(self.unexpected(&[T::RParen]));
                    }
                    let v = self.expr()?;
                    if !super::expr::is_variable(&v)
                        || !matches!(self.peek_kind(), T::Comma | T::RParen)
                    {
                        return Err(self.unexpected(&postfix));
                    }
                    vars.push(v);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.expect1(T::RParen)?;
                self.end_stmt(&[T::Semicolon])?;
                Ok(self.stmt(StmtKind::Unset(vars), start))
            }
            T::Try => self.try_stmt(),
            T::Goto => {
                self.bump();
                let lt = self.expect1(T::String)?;
                let label = self.ident_from(&lt)?;
                self.end_stmt(&[T::Semicolon])?;
                Ok(self.stmt(StmtKind::Goto(label), start))
            }
            T::String if self.peek_at(1) == T::Colon => {
                self.bump();
                self.bump();
                let label = self.ident_from(&t)?;
                Ok(self.stmt(StmtKind::Label(label), start))
            }
            T::Declare => self.declare_stmt(),
            T::Function
                if matches!(self.peek_at(1), T::String | T::Readonly)
                    || (matches!(
                        self.peek_at(1),
                        T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg
                    ) && matches!(self.peek_at(2), T::String | T::Readonly)) =>
            {
                let f = self.function_decl(start, Vec::new())?;
                Ok(self.stmt(StmtKind::Function(Box::new(f)), start))
            }
            T::Abstract | T::Final => {
                let mods = self.class_modifiers()?;
                let c = self.class_decl(start, Vec::new(), mods)?;
                Ok(self.stmt(StmtKind::Class(Box::new(c)), start))
            }
            T::Readonly if self.peek_at(1) != T::LParen => {
                let mods = self.class_modifiers()?;
                let c = self.class_decl(start, Vec::new(), mods)?;
                Ok(self.stmt(StmtKind::Class(Box::new(c)), start))
            }
            T::Class | T::Interface | T::Trait | T::Enum => {
                let c = self.class_decl(start, Vec::new(), Modifiers::default())?;
                Ok(self.stmt(StmtKind::Class(Box::new(c)), start))
            }
            T::Attribute => self.attributed_statement(start, top),
            T::Namespace if top => self.namespace_stmt(),
            T::Use if top => self.use_stmt(),
            T::Const if top => {
                self.bump();
                let items = self.const_items()?;
                Ok(self.stmt(
                    StmtKind::Const {
                        attrs: Vec::new(),
                        items,
                    },
                    start,
                ))
            }
            T::HaltCompiler => {
                self.bump();
                self.expect1(T::LParen)?;
                self.expect1(T::RParen)?;
                let semi = self.expect1(T::Semicolon)?;
                if !top {
                    let sp = self.tok_span(&t);
                    return Err(self.fatal(
                        "__HALT_COMPILER() can only be used from the outermost scope",
                        sp,
                    ));
                }
                Ok(self.stmt(StmtKind::HaltCompiler { offset: semi.end }, start))
            }
            T::VoidCast => {
                // `(void) expr;` (PHP 8.5).
                self.bump();
                let e = self.expr()?;
                let span = self.span_from(start);
                self.end_stmt(&[])?;
                let cast = Expr {
                    kind: ExprKind::Cast {
                        kind: CastKind::Void,
                        expr: Box::new(e),
                    },
                    span,
                };
                Ok(self.stmt(StmtKind::Expr(cast), start))
            }
            _ => {
                let e = self.expr()?;
                self.end_stmt(&[])?;
                Ok(self.stmt(StmtKind::Expr(e), start))
            }
        }
    }

    /// `const A = 1, B = 2;` after `const`.
    fn const_items(&mut self) -> PResult<Vec<ConstItem<'a>>> {
        let mut items = Vec::new();
        loop {
            let istart = self.pos;
            let doc = self.doc_comment_before(istart);
            let nt = self.expect1(T::String)?;
            let name = self.ident_from(&nt)?;
            self.expect1(T::Eq)?;
            let value = self.expr()?;
            items.push(ConstItem {
                name,
                value,
                doc_comment: doc,
                span: self.span_from(istart),
            });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.end_stmt(&[T::Comma, T::Semicolon])?;
        Ok(items)
    }

    /// A statement starting with `#[...]`.
    fn attributed_statement(&mut self, start: usize, top: bool) -> PResult<Stmt<'a>> {
        let attrs = self.attributes()?;
        match self.peek_kind() {
            T::Function
                if matches!(self.peek_at(1), T::String | T::Readonly)
                    || (matches!(
                        self.peek_at(1),
                        T::AmpFollowedByVarOrVararg | T::AmpNotFollowedByVarOrVararg
                    ) && matches!(self.peek_at(2), T::String | T::Readonly)) =>
            {
                let f = self.function_decl(start, attrs)?;
                Ok(self.stmt(StmtKind::Function(Box::new(f)), start))
            }
            T::Abstract | T::Final | T::Readonly => {
                let mods = self.class_modifiers()?;
                let c = self.class_decl(start, attrs, mods)?;
                Ok(self.stmt(StmtKind::Class(Box::new(c)), start))
            }
            T::Class | T::Interface | T::Trait | T::Enum => {
                let c = self.class_decl(start, attrs, Modifiers::default())?;
                Ok(self.stmt(StmtKind::Class(Box::new(c)), start))
            }
            T::Const if top => {
                self.bump();
                let items = self.const_items()?;
                Ok(self.stmt(StmtKind::Const { attrs, items }, start))
            }
            T::Function | T::Fn | T::Static => {
                // Attributes on a closure / arrow function expression.
                self.pos = start;
                let e = self.expr()?;
                self.end_stmt(&[])?;
                Ok(self.stmt(StmtKind::Expr(e), start))
            }
            // Test Zend/tests/constants/oss_fuzz_416302790.phpt: no list.
            _ => Err(self.unexpected(&[])),
        }
    }

    /// `( expr )`
    fn paren_expr(&mut self) -> PResult<Expr<'a>> {
        self.expect1(T::LParen)?;
        let e = self.expr()?;
        self.expect(T::RParen, &[])?;
        Ok(e)
    }

    /// The body of a control structure: one statement (a block becomes its
    /// statements).
    fn body_statement(&mut self) -> PResult<Vec<Stmt<'a>>> {
        if !self.starts_statement(false) {
            return Err(self.unexpected(&[]));
        }
        let s = self.statement(false)?;
        Ok(vec![s])
    }

    fn if_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        let cond = self.paren_expr()?;
        if self.eat(T::Colon) {
            let then = self.inner_statements(&[T::Elseif, T::Else, T::Endif])?;
            let mut elseifs = Vec::new();
            let mut else_ = None;
            loop {
                match self.peek_kind() {
                    T::Elseif => {
                        let estart = self.pos;
                        self.bump();
                        let c = self.paren_expr()?;
                        self.expect1(T::Colon)?;
                        let body = self.inner_statements(&[T::Elseif, T::Else, T::Endif])?;
                        elseifs.push(ElseIf {
                            cond: c,
                            body,
                            span: self.span_from(estart),
                        });
                    }
                    T::Else => {
                        self.bump();
                        self.expect1(T::Colon)?;
                        let body = self.inner_statements(&[T::Endif])?;
                        else_ = Some(body);
                        self.expect(T::Endif, &[])?;
                        break;
                    }
                    T::Endif => {
                        self.bump();
                        break;
                    }
                    _ => return Err(self.unexpected(&[T::Elseif, T::Else, T::Endif])),
                }
            }
            self.end_stmt(&[T::Semicolon])?;
            return Ok(self.stmt(
                StmtKind::If(Box::new(IfStmt {
                    cond,
                    then,
                    elseifs,
                    else_,
                    alt_syntax: true,
                })),
                start,
            ));
        }
        let then = self.body_statement()?;
        let mut elseifs = Vec::new();
        let mut else_ = None;
        loop {
            match self.peek_kind() {
                T::Elseif => {
                    let estart = self.pos;
                    self.bump();
                    let c = self.paren_expr()?;
                    let body = self.body_statement()?;
                    elseifs.push(ElseIf {
                        cond: c,
                        body,
                        span: self.span_from(estart),
                    });
                }
                T::Else => {
                    self.bump();
                    else_ = Some(self.body_statement()?);
                    break;
                }
                _ => break,
            }
        }
        Ok(self.stmt(
            StmtKind::If(Box::new(IfStmt {
                cond,
                then,
                elseifs,
                else_,
                alt_syntax: false,
            })),
            start,
        ))
    }

    /// A `for` expression list. `(void) expr` is allowed (PHP 8.5),
    /// except as the last condition (observed on PHP 8.5: `for (;(void)
    /// true;)` expects ",").
    fn for_exprs(&mut self, end: T, is_cond: bool) -> PResult<Vec<Expr<'a>>> {
        let mut v = Vec::new();
        if self.at(end) {
            return Ok(v);
        }
        loop {
            if self.at(T::VoidCast) {
                let start = self.pos;
                self.bump();
                let e = self.expr()?;
                let span = self.span_from(start);
                v.push(Expr {
                    kind: ExprKind::Cast {
                        kind: CastKind::Void,
                        expr: Box::new(e),
                    },
                    span,
                });
                if is_cond && !self.at(T::Comma) {
                    return Err(self.unexpected(&[T::Comma]));
                }
            } else {
                v.push(self.expr()?);
            }
            if !self.eat(T::Comma) {
                break;
            }
        }
        Ok(v)
    }

    fn for_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        self.expect1(T::LParen)?;
        let init = self.for_exprs(T::Semicolon, false)?;
        self.expect1(T::Semicolon)?;
        let cond = self.for_exprs(T::Semicolon, true)?;
        self.expect1(T::Semicolon)?;
        let step = self.for_exprs(T::RParen, false)?;
        self.expect1(T::RParen)?;
        let (body, alt) = if self.eat(T::Colon) {
            let b = self.inner_statements(&[T::Endfor])?;
            self.expect(T::Endfor, &[])?;
            self.end_stmt(&[T::Semicolon])?;
            (b, true)
        } else {
            (self.body_statement()?, false)
        };
        Ok(self.stmt(
            StmtKind::For(Box::new(ForStmt {
                init,
                cond,
                step,
                body,
                alt_syntax: alt,
            })),
            start,
        ))
    }

    fn foreach_target(&mut self) -> PResult<(Expr<'a>, bool)> {
        let by_ref = self.eat_amp();
        self.list_ok += 1;
        let e = self.expr_bp(super::expr::BP_ASSIGN + 100);
        self.list_ok -= 1;
        Ok((self.to_assign_target(e?), by_ref))
    }

    fn foreach_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        self.expect1(T::LParen)?;
        let expr = self.expr()?;
        self.expect(T::As, &[])?;
        let (first, first_ref) = self.foreach_target()?;
        let (key, value, by_ref) = if self.eat(T::DoubleArrow) {
            let (v, r) = self.foreach_target()?;
            (Some(first), v, r)
        } else {
            (None, first, first_ref)
        };
        self.expect(T::RParen, &[])?;
        let (body, alt) = if self.eat(T::Colon) {
            let b = self.inner_statements(&[T::Endforeach])?;
            self.expect(T::Endforeach, &[])?;
            self.end_stmt(&[T::Semicolon])?;
            (b, true)
        } else {
            (self.body_statement()?, false)
        };
        Ok(self.stmt(
            StmtKind::Foreach(Box::new(ForeachStmt {
                expr,
                key,
                value,
                by_ref,
                body,
                alt_syntax: alt,
            })),
            start,
        ))
    }

    fn switch_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        let subject = self.paren_expr()?;
        let alt = if self.eat(T::Colon) {
            true
        } else {
            self.expect1(T::LBrace)?;
            false
        };
        let end = if alt { T::Endswitch } else { T::RBrace };
        // An optional `;` right after the opening (PHP manual: switch).
        self.eat(T::Semicolon);
        let mut cases = Vec::new();
        loop {
            let cstart = self.pos;
            let cond = match self.peek_kind() {
                T::Case => {
                    self.bump();
                    Some(self.expr()?)
                }
                T::Default => {
                    self.bump();
                    None
                }
                k if k == end => break,
                // Observed on PHP 8.5: "expecting "case" or "default" or
                // "}"" (alternative syntax: "endswitch" first).
                _ => return Err(self.unexpected(&[T::Case, T::Default, end])),
            };
            if !self.eat(T::Colon) && !self.eat(T::Semicolon) {
                // After `case expr` nothing is listed (the expression could
                // continue); after `default`, ":" or ";" (observed on PHP 8.5).
                let ex: &[T] = if cond.is_some() {
                    &[]
                } else {
                    &[T::Colon, T::Semicolon]
                };
                return Err(self.unexpected(ex));
            }
            let body = self.inner_statements(&[T::Case, T::Default, end])?;
            cases.push(SwitchCase {
                cond,
                body,
                span: self.span_from(cstart),
            });
        }
        self.bump();
        if alt {
            self.end_stmt(&[T::Semicolon])?;
        }
        Ok(self.stmt(
            StmtKind::Switch(Box::new(SwitchStmt {
                subject,
                cases,
                alt_syntax: alt,
            })),
            start,
        ))
    }

    fn try_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        let body = self.block_body()?;
        let mut catches = Vec::new();
        while self.at(T::Catch) {
            let cstart = self.pos;
            self.bump();
            self.expect1(T::LParen)?;
            let mut types = Vec::new();
            loop {
                // `static` parses here; the compiler rejects it (test
                // Zend/tests/catch_static.phpt).
                types.push(self.class_name()?);
                if !self.eat(T::Pipe) {
                    break;
                }
            }
            let var = if self.at(T::Variable) {
                let vt = self.bump();
                Some(Ident {
                    name: &self.text(&vt)[1..],
                    span: self.tok_span(&vt),
                })
            } else {
                None
            };
            self.expect1(T::RParen)?;
            let body = self.block_body()?;
            catches.push(Catch {
                types,
                var,
                body,
                span: self.span_from(cstart),
            });
        }
        let finally = if self.eat(T::Finally) {
            Some(self.block_body()?)
        } else {
            None
        };
        if catches.is_empty() && finally.is_none() {
            // A compile error (observed on PHP 8.5: "Cannot use try
            // without catch or finally"); the parser accepts it.
            let sp = self.span_from(start);
            self.compile_error("Cannot use try without catch or finally", sp);
        }
        Ok(self.stmt(
            StmtKind::Try(Box::new(TryStmt {
                body,
                catches,
                finally,
            })),
            start,
        ))
    }

    fn declare_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        self.expect1(T::LParen)?;
        let mut directives = Vec::new();
        loop {
            let nt = self.expect1(T::String)?;
            let name = self.ident_from(&nt)?;
            self.expect1(T::Eq)?;
            let value = self.expr()?;
            directives.push((name, value));
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen, &[T::Comma, T::RParen])?;
        let body = if self.eat(T::Colon) {
            let b = self.inner_statements(&[T::Enddeclare])?;
            self.expect(T::Enddeclare, &[])?;
            self.end_stmt(&[T::Semicolon])?;
            DeclareBody::Alt(b)
        } else if self.eat(T::Semicolon) {
            DeclareBody::None
        } else {
            DeclareBody::Block(self.body_statement()?)
        };
        Ok(self.stmt(
            StmtKind::Declare(Box::new(DeclareStmt { directives, body })),
            start,
        ))
    }

    fn namespace_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        // Reserved words are accepted as names (test
        // Zend/tests/namespaces/namespace_name_reserved_keywords.phpt).
        let k = self.peek_kind();
        let name = if k == T::String || k == T::NameQualified || k.is_semi_reserved() {
            let t = self.bump();
            Some(self.name_from(&t))
        } else {
            None
        };
        if self.at(T::LBrace) {
            self.bump();
            let body = self.top_statements(false)?;
            self.expect(T::RBrace, &[])?;
            return Ok(self.stmt(
                StmtKind::Namespace {
                    name,
                    body: Some(body),
                },
                start,
            ));
        }
        if name.is_none() {
            return Err(self.unexpected(&[T::LBrace]));
        }
        // Observed on PHP 8.5: `namespace A B;` expects "{".
        self.end_stmt(&[T::LBrace])?;
        Ok(self.stmt(StmtKind::Namespace { name, body: None }, start))
    }

    /// `use` at top level (PHP manual: "Using namespaces: Aliasing /
    /// Importing"). Expected-token lists observed on PHP 8.5.
    fn use_stmt(&mut self) -> PResult<Stmt<'a>> {
        let start = self.pos;
        self.bump();
        let kind = match self.peek_kind() {
            T::Function => {
                self.bump();
                UseKind::Function
            }
            T::Const => {
                self.bump();
                UseKind::Const
            }
            _ => UseKind::Normal,
        };
        let name_tokens: &[T] = &[T::String, T::NameFullyQualified, T::NameQualified];
        let first = *self.peek();
        if !matches!(
            first.kind,
            T::String | T::NameQualified | T::NameFullyQualified
        ) {
            if kind == UseKind::Normal {
                return Err(self.unexpected(&[]));
            }
            return Err(self.unexpected(name_tokens));
        }
        self.bump();
        if self.at(T::NsSeparator) && self.peek_at(1) == T::LBrace {
            // Group use: `use A\{...}` (the lexer gives `A` and `\`).
            self.bump();
            self.bump();
            let prefix = self.name_from(&first);
            let mut uses = Vec::new();
            loop {
                let ustart = self.pos;
                let ikind = if kind == UseKind::Normal {
                    match self.peek_kind() {
                        T::Function => {
                            self.bump();
                            UseKind::Function
                        }
                        T::Const => {
                            self.bump();
                            UseKind::Const
                        }
                        _ => UseKind::Normal,
                    }
                } else {
                    kind
                };
                let nt = *self.peek();
                if !matches!(nt.kind, T::String | T::NameQualified) {
                    if !uses.is_empty() {
                        // After a comma (test ns_trailing_comma_error_03).
                        return Err(self.unexpected(&[T::RBrace]));
                    }
                    if kind == UseKind::Normal && ikind == UseKind::Normal {
                        return Err(self.unexpected(&[
                            T::String,
                            T::NameQualified,
                            T::Function,
                            T::Const,
                        ]));
                    }
                    return Err(self.unexpected(&[T::String, T::NameQualified]));
                }
                self.bump();
                let name = self.name_from(&nt);
                let alias = if self.eat(T::As) {
                    let at = self.expect1(T::String)?;
                    Some(self.ident_from(&at)?)
                } else {
                    None
                };
                uses.push(UseItem {
                    name,
                    alias,
                    kind: ikind,
                    span: self.span_from(ustart),
                });
                if !self.eat(T::Comma) || self.at(T::RBrace) {
                    break;
                }
            }
            self.expect1(T::RBrace)?;
            self.end_stmt(&[T::Semicolon])?;
            return Ok(self.stmt(StmtKind::GroupUse { kind, prefix, uses }, start));
        }
        let mut uses = Vec::new();
        let mut cur = first;
        loop {
            let ustart = self.pos - 1;
            let name = self.name_from(&cur);
            let alias = if self.eat(T::As) {
                let at = self.expect1(T::String)?;
                Some(self.ident_from(&at)?)
            } else {
                None
            };
            uses.push(UseItem {
                name,
                alias,
                kind,
                span: self.span_from(ustart),
            });
            if !self.eat(T::Comma) {
                break;
            }
            cur = *self.peek();
            if !matches!(
                cur.kind,
                T::String | T::NameQualified | T::NameFullyQualified
            ) {
                return Err(self.unexpected(name_tokens));
            }
            self.bump();
        }
        self.end_stmt(&[T::Comma, T::Semicolon])?;
        Ok(self.stmt(StmtKind::Use { kind, uses }, start))
    }
}
